//! In-process composition and producer lifecycle around the existing graph engine.
use phenix_graph::{
    Deployment, Engine, Executor, GraphBuilder, GraphContract, TriggerId, TriggerSelector, Value,
    WaveReport,
};
use phenix_plugin::{ContributionHandle, PluginSnapshot};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Condvar, Mutex},
};

/// Globally qualified contribution identity, ordered lexically by owner then id.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    pub owner: String,
    pub id: String,
}

impl Key {
    pub fn new(owner: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            id: id.into(),
        }
    }
}

/// One external event. Each notice is admitted as a separate engine wave.
pub struct Notice {
    pub trigger: String,
    pub values: Vec<Value>,
}

/// A producer must not wait for an event. Drop must stop and release its resources.
/// Factories must clean up partially acquired resources if activation fails.
pub trait Producer: Send {
    fn poll(&mut self) -> Result<Option<Notice>, String>;

    /// Register notification for newly available events. Return false if unsupported.
    /// Implementations must synchronize registration with event publication.
    fn set_waker(&mut self, _waker: WakeHandle) -> bool {
        false
    }
}

#[derive(Default)]
struct WakeState {
    state: Mutex<(u64, bool)>,
    ready: Condvar,
}

/// Cloneable notification and shutdown handle for the optional blocking driver.
#[derive(Clone, Default)]
pub struct WakeHandle(Arc<WakeState>);

impl WakeHandle {
    pub fn wake(&self) {
        let mut state = self.0.state.lock().unwrap();
        state.0 = state.0.wrapping_add(1);
        self.0.ready.notify_all();
    }

    /// Shutdown is sticky. Use a new handle for a subsequent driver run.
    pub fn shutdown(&self) {
        let mut state = self.0.state.lock().unwrap();
        state.1 = true;
        self.0.ready.notify_all();
    }
}

pub type Compose = dyn Fn(&mut GraphBuilder) -> Result<(), String> + Send + Sync;
pub type Activate = dyn Fn() -> Result<Box<dyn Producer>, String> + Send + Sync;

/// Shared Rust type registered by plugins through the existing Registrar.
/// Composition uses GraphBuilder directly, including its named endpoint APIs.
pub struct Contribution {
    pub id: String,
    pub requires: Vec<Key>,
    /// Public trigger names this contribution's producer may emit.
    pub routes: Vec<String>,
    pub compose: Box<Compose>,
    pub activate: Option<Box<Activate>>,
}

impl phenix_plugin::Contribution for Contribution {
    const KIND: &'static str = "kernel.contribution";
    fn id(&self) -> &str {
        &self.id
    }
}

/// A reusable graph-owned contract definition. The registration id is local to
/// the defining plugin; the contract itself owns its semantic identity.
pub struct ContractDefinition {
    pub id: String,
    pub contract: GraphContract,
}

impl phenix_plugin::Contribution for ContractDefinition {
    const KIND: &'static str = "graph.contract";
    fn id(&self) -> &str {
        &self.id
    }
}

/// An offer names an actual unit, never a purported implementation shape.
/// Every offer is checked after all composition, including unused offers.
pub struct GraphOffer {
    pub id: String,
    pub unit: String,
    pub contract: Key,
}

impl phenix_plugin::Contribution for GraphOffer {
    const KIND: &'static str = "graph.offer";
    fn id(&self) -> &str {
        &self.id
    }
    fn graph_unit(&self) -> Option<&str> {
        Some(&self.unit)
    }
}

/// An explicit host-selected offer. This validates the selected target; it does
/// not discover providers, authorize access or insert edges. The consumer's
/// compose closure must wire this offer's path and declare composition ordering
/// through `Contribution::requires` when needed.
pub struct ContractRequirement {
    pub id: String,
    pub contract: Key,
    pub offer: Key,
}

impl phenix_plugin::Contribution for ContractRequirement {
    const KIND: &'static str = "graph.requirement";
    fn id(&self) -> &str {
        &self.id
    }
}

/// One poll's outcome; admission errors and invocation failures remain distinct.
pub struct ProducerEvent {
    pub producer: Key,
    pub result: Result<WaveReport, ProducerError>,
}

/// Invocation failures remain inside successful WaveReports, not these errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProducerError {
    Poll(String),
    UndeclaredRoute(String),
    Admission(String),
}

pub struct Kernel {
    // Drop clears producers first; field order then keeps leases alive past engine teardown.
    engine: Engine,
    producers: Vec<(Key, Box<dyn Producer>)>,
    leases: Vec<ContributionHandle<Contribution>>,
    keys: Vec<Key>,
    started: bool,
    next_producer: usize,
    routes: BTreeMap<Key, BTreeMap<String, TriggerId>>,
    // Keep definition/offer/consumer code alive after engine and action teardown.
    _snapshots: Vec<PluginSnapshot>,
}

impl Drop for Kernel {
    fn drop(&mut self) {
        self.producers.clear();
    }
}

impl Kernel {
    /// Resolve all dependencies before composing, compile once, build one deployment,
    /// without activating producers. Lexical keys break topological ties.
    pub fn build(
        name: impl Into<String>,
        snapshots: impl IntoIterator<Item = PluginSnapshot>,
        executor: Arc<dyn Executor>,
    ) -> Result<Self, String> {
        let snapshots: Vec<_> = snapshots.into_iter().collect();
        let mut definitions = BTreeMap::new();
        let mut offers = BTreeMap::new();
        let mut requirements = BTreeMap::new();
        let mut pending = BTreeMap::new();
        for snapshot in &snapshots {
            for (_, kind, id) in snapshot.keys() {
                let key = Key::new(snapshot.name(), id);
                macro_rules! collect {
                    ($ty:ty, $map:ident) => {
                        if kind == <$ty as phenix_plugin::Contribution>::KIND {
                            let handle = snapshot
                                .get::<$ty>(id)
                                .ok_or_else(|| format!("contribution type mismatch: {key:?}"))?;
                            if $map.insert(key.clone(), handle).is_some() {
                                return Err(format!("duplicate contribution: {key:?}"));
                            }
                            continue;
                        }
                    };
                }
                collect!(ContractDefinition, definitions);
                collect!(GraphOffer, offers);
                collect!(ContractRequirement, requirements);
                if kind != <Contribution as phenix_plugin::Contribution>::KIND {
                    continue;
                }
                let handle = snapshot
                    .get::<Contribution>(id)
                    .ok_or_else(|| format!("contribution type mismatch: {key:?}"))?;
                if pending.insert(key.clone(), handle).is_some() {
                    return Err(format!("duplicate contribution: {key:?}"));
                }
            }
        }
        let mut identities = BTreeMap::new();
        for (key, definition) in &definitions {
            if let Some(previous) =
                identities.insert(definition.contract.id(), &definition.contract)
                && previous != &definition.contract
            {
                return Err(format!(
                    "conflicting contract definition at {key:?}: {}",
                    definition.contract.id()
                ));
            }
        }
        for (key, offer) in &offers {
            if !definitions.contains_key(&offer.contract) {
                return Err(format!(
                    "missing contract definition for offer {key:?}: {:?}",
                    offer.contract
                ));
            }
        }
        for (key, requirement) in &requirements {
            if !definitions.contains_key(&requirement.contract) {
                return Err(format!(
                    "missing contract definition for requirement {key:?}: {:?}",
                    requirement.contract
                ));
            }
            if !offers.contains_key(&requirement.offer) {
                return Err(format!(
                    "missing selected offer for requirement {key:?}: {:?}",
                    requirement.offer
                ));
            }
        }
        for (key, contribution) in &pending {
            let mut dependencies = BTreeSet::new();
            for dependency in &contribution.requires {
                if !dependencies.insert(dependency) {
                    return Err(format!("duplicate dependency for {key:?}: {dependency:?}"));
                }
                if !pending.contains_key(dependency) {
                    return Err(format!("missing dependency for {key:?}: {dependency:?}"));
                }
            }
        }
        let mut resolved = BTreeSet::new();
        let mut leases = Vec::new();
        let mut keys = Vec::new();
        while !pending.is_empty() {
            let key = pending
                .iter()
                .find(|(_, c)| c.requires.iter().all(|k| resolved.contains(k)))
                .map(|(key, _)| key.clone())
                .ok_or_else(|| "contribution dependency cycle".to_owned())?;
            leases.push(pending.remove(&key).expect("selected pending contribution"));
            keys.push(key.clone());
            resolved.insert(key);
        }
        // The builder is dropped before leases on composition/compile failure.
        let mut graph = GraphBuilder::new(name);
        for contribution in &leases {
            graph.with_origin(contribution.graph_origin(), |graph| {
                (contribution.compose)(graph)
            })?;
        }
        for (key, offer) in &offers {
            graph
                .require_contract(&offer.unit, &definitions[&offer.contract].contract)
                .map_err(|error| format!("offer {key:?} at {}: {error}", offer.unit))?;
        }
        for (key, requirement) in &requirements {
            let offer = &offers[&requirement.offer];
            graph
                .require_contract(&offer.unit, &definitions[&requirement.contract].contract)
                .map_err(|error| format!("requirement {key:?} at {}: {error}", offer.unit))?;
        }
        let compiled = graph.compile().map_err(|error| error.to_string())?;
        for (key, offer) in &offers {
            let contract = &definitions[&offer.contract].contract;
            let unit = compiled.find(&offer.unit).ok_or_else(|| {
                format!("compiled unit not found for offer {key:?}: {}", offer.unit)
            })?;
            if unit.origin() != Some(offer.graph_origin()) {
                return Err(format!(
                    "compiled offer {key:?} at {}: contribution generation origin mismatch",
                    offer.unit
                ));
            }
            contract
                .check_implementation(&unit)
                .map_err(|error| format!("compiled offer {key:?} at {}: {error}", offer.unit))?;
        }
        for (key, requirement) in &requirements {
            let offer = &offers[&requirement.offer];
            let contract = &definitions[&requirement.contract].contract;
            let unit = compiled.find(&offer.unit).ok_or_else(|| {
                format!(
                    "compiled unit not found for requirement {key:?}: {}",
                    offer.unit
                )
            })?;
            contract.check_implementation(&unit).map_err(|error| {
                format!("compiled requirement {key:?} at {}: {error}", offer.unit)
            })?;
        }
        let mut routes = BTreeMap::new();
        for (key, contribution) in keys.iter().zip(&leases) {
            if contribution.activate.is_none() && !contribution.routes.is_empty() {
                return Err(format!("routes declared without producer: {key:?}"));
            }
            let mut selected = BTreeMap::new();
            for name in &contribution.routes {
                let trigger = compiled
                    .trigger(name)
                    .ok_or_else(|| format!("unknown producer route for {key:?}: {name:?}"))?;
                if selected.insert(name.clone(), trigger).is_some() {
                    return Err(format!("duplicate producer route for {key:?}: {name:?}"));
                }
            }
            routes.insert(key.clone(), selected);
        }
        let engine = Deployment::new(compiled).executor(executor).build()?;
        Ok(Self {
            engine,
            producers: Vec::new(),
            leases,
            keys,
            started: false,
            next_producer: 0,
            routes,
            _snapshots: snapshots,
        })
    }

    /// Activate in dependency order. Failure drops all newly acquired producers
    /// and leaves the kernel unstarted, allowing retry. A second successful start
    /// is rejected, including when no contributions have producers.
    pub fn start(&mut self) -> Result<(), String> {
        if self.started {
            return Err("kernel already started".into());
        }
        let mut producers = Vec::new();
        for (key, contribution) in self.keys.iter().zip(&self.leases) {
            if let Some(activate) = &contribution.activate {
                let producer = activate()?;
                producers.push((key.clone(), producer));
            }
        }
        self.producers = producers;
        self.started = true;
        Ok(())
    }

    /// Low-level engine access bypasses the kernel's started-state admission guard.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn fire<T: TriggerSelector>(
        &self,
        trigger: T,
        values: Vec<Value>,
    ) -> Result<WaveReport, String> {
        if !self.started {
            return Err("kernel not started".into());
        }
        self.engine.fire(trigger, values)
    }

    /// Perform at most `max_polls` nonblocking producer polls, round-robin across
    /// calls. Idle polls count toward the bound. Errors do not stop other polls.
    /// This bounds admissions, not the execution time of an individual wave.
    pub fn drain(&mut self, max_polls: usize) -> Vec<ProducerEvent> {
        let mut events = Vec::new();
        if self.producers.is_empty() {
            return events;
        }
        let count = self.producers.len();
        for _ in 0..max_polls {
            let (key, producer) = &mut self.producers[self.next_producer];
            self.next_producer = (self.next_producer + 1) % count;
            let result = match producer.poll() {
                Ok(None) => continue,
                Ok(Some(notice)) => match self.routes[key].get(&notice.trigger) {
                    Some(trigger) => self
                        .engine
                        .fire(*trigger, notice.values)
                        .map_err(ProducerError::Admission),
                    None => Err(ProducerError::UndeclaredRoute(notice.trigger)),
                },
                Err(error) => Err(ProducerError::Poll(error)),
            };
            events.push(ProducerEvent {
                producer: key.clone(),
                result,
            });
        }
        events
    }

    /// Drive a started kernel until shutdown, delivering events without retaining reports.
    /// Every producer must support wake registration. Shutdown is checked between waves;
    /// it does not interrupt a running producer poll, wave, or callback.
    pub fn run_until(
        &mut self,
        wake: &WakeHandle,
        mut on_event: impl FnMut(ProducerEvent),
    ) -> Result<(), String> {
        if !self.started {
            return Err("kernel not started".into());
        }
        for (key, producer) in &mut self.producers {
            if !producer.set_waker(wake.clone()) {
                return Err(format!(
                    "producer does not support wake notifications: {key:?}"
                ));
            }
        }
        loop {
            let generation = {
                let state = wake.0.state.lock().unwrap();
                if state.1 {
                    return Ok(());
                }
                state.0
            };
            let mut active = false;
            for _ in 0..self.producers.len() {
                if wake.0.state.lock().unwrap().1 {
                    return Ok(());
                }
                for event in self.drain(1) {
                    active = true;
                    on_event(event);
                }
            }
            if !active {
                let state = wake.0.state.lock().unwrap();
                let _state = wake
                    .0
                    .ready
                    .wait_while(state, |state| !state.1 && state.0 == generation)
                    .unwrap();
            }
        }
    }
}
