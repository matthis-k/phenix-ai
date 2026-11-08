//! In-process composition and producer lifecycle around the existing graph engine.
use phenix_graph::{
    Deployment, Engine, Executor, GraphBuilder, TriggerSelector, Value, WaveReport,
};
use phenix_plugin::{ContributionHandle, PluginSnapshot};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
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
}

pub type Compose = dyn Fn(&mut GraphBuilder) -> Result<(), String> + Send + Sync;
pub type Activate = dyn Fn() -> Result<Box<dyn Producer>, String> + Send + Sync;

/// Shared Rust type registered by plugins through the existing Registrar.
/// Composition uses GraphBuilder directly, including its named endpoint APIs.
pub struct Contribution {
    pub id: String,
    pub requires: Vec<Key>,
    pub compose: Box<Compose>,
    pub activate: Option<Box<Activate>>,
}

impl phenix_plugin::Contribution for Contribution {
    const KIND: &'static str = "kernel.contribution";
    fn id(&self) -> &str {
        &self.id
    }
}

/// One poll's outcome; admission errors and invocation failures remain distinct.
pub struct ProducerEvent {
    pub producer: Key,
    pub result: Result<WaveReport, String>,
}

pub struct Kernel {
    // Drop clears producers first; field order then keeps leases alive past engine teardown.
    engine: Engine,
    producers: Vec<(Key, Box<dyn Producer>)>,
    leases: Vec<ContributionHandle<Contribution>>,
    keys: Vec<Key>,
    started: bool,
    next_producer: usize,
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
        let mut pending = BTreeMap::new();
        for snapshot in snapshots {
            for (_, kind, id) in snapshot.keys() {
                if kind != <Contribution as phenix_plugin::Contribution>::KIND {
                    continue;
                }
                let key = Key::new(snapshot.name(), id);
                let handle = snapshot
                    .get::<Contribution>(id)
                    .ok_or_else(|| format!("contribution type mismatch: {key:?}"))?;
                if pending.insert(key.clone(), handle).is_some() {
                    return Err(format!("duplicate contribution: {key:?}"));
                }
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
            (contribution.compose)(&mut graph)?;
        }
        let compiled = graph.compile().map_err(|error| error.to_string())?;
        let engine = Deployment::new(compiled).executor(executor).build()?;
        Ok(Self {
            engine,
            producers: Vec::new(),
            leases,
            keys,
            started: false,
            next_producer: 0,
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
                Ok(Some(notice)) => self.engine.fire(notice.trigger.as_str(), notice.values),
                Err(error) => Err(error),
            };
            events.push(ProducerEvent {
                producer: key.clone(),
                result,
            });
        }
        events
    }
}
