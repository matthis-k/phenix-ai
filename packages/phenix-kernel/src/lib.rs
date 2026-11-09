//! In-process composition and producer lifecycle around the existing graph engine.
pub mod host;

use phenix_graph::{
    Deployment, Engine, Executor, GraphBuilder, GraphContract, TriggerId, TriggerSelector, Value,
    WaveReport,
};
pub use phenix_plugin::CallbackError;
use phenix_plugin::{ContributionHandle, PluginSnapshot};
use std::{
    any::Any,
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
    /// An error is terminal: the kernel reports it once and drops this producer.
    /// Transient unavailability must return Ok(None) and wake when ready to retry.
    fn poll(&mut self) -> Result<Option<Notice>, CallbackError>;

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

pub type Compose = dyn Fn(&mut ComposeContext<'_>) -> Result<(), CallbackError> + Send + Sync;
pub type Activate = dyn Fn() -> Result<Box<dyn Producer>, CallbackError> + Send + Sync;

/// Owned selection metadata. Projection uses the graph's authoritative contract checker.
#[derive(Clone, Debug)]
pub struct ResolvedImport {
    pub offer: Key,
    pub producer: Key,
    unit: String,
    contract: GraphContract,
}

impl ResolvedImport {
    pub fn bind<'g>(
        &self,
        graph: &'g GraphBuilder,
    ) -> Result<phenix_graph::BoundAuthoringUnit<'g>, phenix_graph::InterfaceError> {
        graph.bind_contract(&self.unit, &self.contract)
    }
}

pub struct ComposeContext<'a> {
    pub graph: &'a mut GraphBuilder,
    imports: &'a BTreeMap<String, ResolvedImport>,
    artifacts: &'a mut BuildArtifacts,
}

#[derive(Default)]
struct BuildArtifacts {
    activate: Option<Box<Activate>>,
    export: Option<Box<dyn Any + Send + Sync>>,
}

impl ComposeContext<'_> {
    /// Register this candidate's producer factory, capturing its exact composed state.
    /// Only one factory is allowed per contribution in each candidate.
    pub fn activate(
        &mut self,
        factory: impl Fn() -> Result<Box<dyn Producer>, CallbackError> + Send + Sync + 'static,
    ) -> Result<(), CallbackError> {
        if self.artifacts.activate.is_some() {
            return Err("candidate activation already registered".into());
        }
        self.artifacts.activate = Some(Box::new(factory));
        Ok(())
    }

    /// Export one typed value owned by this candidate and scoped to its contribution key.
    /// Bundle multiple values in a struct; retrieve it through Kernel::export.
    pub fn export<T: Send + Sync + 'static>(&mut self, value: T) -> Result<(), CallbackError> {
        if self.artifacts.export.is_some() {
            return Err("candidate export already registered".into());
        }
        self.artifacts.export = Some(Box::new(value));
        Ok(())
    }

    /// Look up only this contribution's declared imports. The result owns its
    /// metadata so endpoint projection need not hold a borrow of the context.
    pub fn import(&self, id: &str) -> Result<ResolvedImport, CallbackError> {
        self.imports
            .get(id)
            .cloned()
            .ok_or_else(|| format!("undeclared import: {id}").into())
    }
}

/// Graph composition and its declared imports/exports are registered as one
/// atomic bundle. Offers and imports belong to this composer, not to a second
/// user-supplied owner key. This value is authoring-only; Graph still validates
/// the compiled unit's contract and exact origin.
pub struct Contribution {
    id: String,
    requires: Vec<Key>,
    routes: Vec<String>,
    compose: Box<Compose>,
    offers: Vec<(String, String, Key)>,
    imports: Vec<(String, Key)>,
}

impl Contribution {
    pub fn new(
        id: impl Into<String>,
        compose: impl Fn(&mut ComposeContext<'_>) -> Result<(), CallbackError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            requires: Vec::new(),
            routes: Vec::new(),
            compose: Box::new(compose),
            offers: Vec::new(),
            imports: Vec::new(),
        }
    }

    /// Explicit composition dependency, besides dependencies inferred by imports.
    pub fn after(mut self, dependencies: impl IntoIterator<Item = Key>) -> Self {
        self.requires.extend(dependencies);
        self
    }

    /// Trigger names this composer's optional producer may emit.
    pub fn route<S: Into<String>>(mut self, routes: impl IntoIterator<Item = S>) -> Self {
        self.routes.extend(routes.into_iter().map(Into::into));
        self
    }

    /// Advertise a unit this composer actually creates under a semantic contract.
    pub fn offer(
        mut self,
        id: impl Into<String>,
        unit: impl Into<String>,
        contract: Key,
    ) -> Self {
        self.offers.push((id.into(), unit.into(), contract));
        self
    }

    /// Require a semantic contract; the trusted host chooses a suitable offer.
    pub fn import(mut self, id: impl Into<String>, contract: Key) -> Self {
        self.imports.push((id.into(), contract));
        self
    }

    /// Validate the entire declaration before adding any part of it.
    pub fn register(self, into: &mut phenix_plugin::Registrar) -> Result<(), phenix_plugin::Failure> {
        let owner = into.owner().to_owned();
        let composer = Key::new(&owner, &self.id);
        let mut batch = phenix_plugin::Registrar::new(owner)?;
        batch.add(RegisteredComposer {
            id: self.id,
            requires: self.requires,
            routes: self.routes,
            compose: self.compose,
        })?;
        for (id, unit, contract) in self.offers {
            batch.add(GraphOffer {
                id,
                producer: composer.clone(),
                unit,
                contract,
            })?;
        }
        for (id, contract) in self.imports {
            batch.add(ContractRequirement {
                id,
                consumer: composer.clone(),
                contract,
            })?;
        }
        into.merge(batch.finish())
    }
}

/// Internal registered payload. The plugin registry remains graph-independent.
struct RegisteredComposer {
    id: String,
    requires: Vec<Key>,
    routes: Vec<String>,
    compose: Box<Compose>,
}

impl phenix_plugin::Contribution for RegisteredComposer {
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
struct GraphOffer {
    pub id: String,
    /// Exact contribution responsible for creating the offered unit.
    pub producer: Key,
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

/// A local declarative import. Only the trusted host can select its provider.
struct ContractRequirement {
    pub id: String,
    pub consumer: Key,
    pub contract: Key,
}

/// Trusted host decisions, never plugin contributions. Denial wins over allowance.
#[derive(Clone, Debug, Default)]
pub struct ImportPolicy {
    pub selections: BTreeMap<Key, Key>,
    /// If present, only these (requirement, offer) pairs are eligible.
    pub allow: Option<BTreeSet<(Key, Key)>>,
    pub deny: BTreeSet<(Key, Key)>,
}

/// Trusted import selection and Graph-owned execution limits.
/// Configuration never receives the validated deployment or an admission API.
///
/// ```compile_fail
/// use phenix_kernel::BuildOptions;
/// let options = BuildOptions::default();
/// options.execution.build().unwrap().fire("start", vec![]);
/// ```
/// ```compile_fail
/// use phenix_graph::Deployment;
/// use phenix_kernel::BuildOptions;
/// fn substitute(options: &mut BuildOptions, replacement: Deployment) {
///     options.execution = replacement;
/// }
/// ```
/// ```compile_fail
/// use phenix_graph::Deployment;
/// use phenix_kernel::BuildOptions;
/// let options = BuildOptions {
///     deployment: |deployment: Deployment| deployment,
///     ..Default::default()
/// };
/// ```
#[derive(Default)]
pub struct BuildOptions {
    pub imports: ImportPolicy,
    /// Limits only: cannot replace graphs, build/fire, or override executors.
    pub execution: phenix_graph::ExecutionLimits,
}

#[derive(Debug)]
pub enum ResolutionError {
    DuplicateOwner(String),
    Invalid(String),
    ConflictingDefinition {
        definition: Key,
        contract: String,
    },
    Composition {
        contribution: Key,
        cause: CallbackError,
    },
    Compilation {
        cause: phenix_graph::CompileDiagnostic,
    },
    MissingUnit {
        offer: Key,
        unit: String,
    },
    WrongOrigin {
        offer: Key,
        producer: Box<Key>,
        unit: String,
    },
    Interface {
        offer: Option<Key>,
        unit: String,
        cause: Box<phenix_graph::InterfaceError>,
    },
    Missing {
        requirement: Key,
        contract: Key,
    },
    Ambiguous {
        requirement: Key,
        offers: Vec<Key>,
    },
    Ineligible {
        requirement: Key,
        offer: Key,
        reason: Box<str>,
    },
    Cycle(Vec<Key>),
}

impl std::fmt::Display for ResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateOwner(owner) => write!(f, "duplicate snapshot owner: {owner}"),
            Self::Invalid(message) => f.write_str(message),
            Self::ConflictingDefinition {
                definition,
                contract,
            } => write!(
                f,
                "conflicting contract definition at {definition:?}: {contract}"
            ),
            Self::Composition {
                contribution,
                cause,
            } => write!(f, "composition of {contribution:?}: {cause}"),
            Self::Compilation { cause } => cause.fmt(f),
            Self::MissingUnit { offer, unit } => {
                write!(f, "compiled unit not found for offer {offer:?}: {unit}")
            }
            Self::WrongOrigin {
                offer,
                producer,
                unit,
            } => write!(
                f,
                "offer {offer:?} at {unit}: producer ownership origin mismatch: {producer:?}; contribution generation origin mismatch"
            ),
            Self::Interface { offer, unit, cause } => match offer {
                Some(offer) => write!(f, "compiled offer {offer:?} at {unit}: {cause}"),
                None => write!(f, "host/graph contract catalog conflict: {cause}"),
            },
            Self::Missing {
                requirement,
                contract,
            } => write!(
                f,
                "missing eligible offer for requirement {requirement:?}: {contract:?}"
            ),
            Self::Ambiguous {
                requirement,
                offers,
            } => write!(f, "ambiguous requirement {requirement:?}: {offers:?}"),
            Self::Ineligible {
                requirement,
                offer,
                reason,
            } => write!(
                f,
                "ineligible selection for requirement {requirement:?}, offer {offer:?}: {reason}"
            ),
            Self::Cycle(keys) => write!(f, "contribution dependency cycle: {keys:?}"),
        }
    }
}

impl std::error::Error for ResolutionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Compilation { cause } => Some(cause),
            Self::Interface { cause, .. } => Some(cause.as_ref()),
            Self::Composition { cause, .. } => Some(cause.as_ref()),
            _ => None,
        }
    }
}

impl From<String> for ResolutionError {
    fn from(message: String) -> Self {
        Self::Invalid(message)
    }
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
#[derive(Debug)]
pub enum ProducerError {
    Poll(CallbackError),
    UndeclaredRoute(String),
    Admission(String),
}

impl std::fmt::Display for ProducerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Poll(cause) => write!(f, "producer poll: {cause}"),
            Self::UndeclaredRoute(route) => write!(f, "undeclared producer route: {route}"),
            Self::Admission(cause) => write!(f, "producer admission: {cause}"),
        }
    }
}

impl std::error::Error for ProducerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Poll(cause) => Some(cause.as_ref()),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum DriverError {
    NotStarted,
    UnsupportedWake { producer: Key },
}

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotStarted => f.write_str("kernel not started"),
            Self::UnsupportedWake { producer } => write!(
                f,
                "producer does not support wake notifications: {producer:?}"
            ),
        }
    }
}

impl std::error::Error for DriverError {}

#[derive(Debug)]
pub enum StartError {
    AlreadyStarted,
    Activation {
        contribution: Key,
        cause: CallbackError,
    },
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyStarted => f.write_str("kernel already started"),
            Self::Activation {
                contribution,
                cause,
            } => write!(f, "activation of {contribution:?}: {cause}"),
        }
    }
}

impl std::error::Error for StartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Activation { cause, .. } => Some(cause.as_ref()),
            Self::AlreadyStarted => None,
        }
    }
}

#[derive(Default)]
struct ProducerState {
    producers: Vec<(Key, Box<dyn Producer>)>,
    next: usize,
    retired: bool,
}

pub struct Kernel {
    // Drop clears producers first; field order then keeps leases alive past engine teardown.
    engine: Engine,
    producers: Mutex<ProducerState>,
    artifacts: BTreeMap<Key, BuildArtifacts>,
    _leases: Vec<ContributionHandle<Contribution>>,
    keys: Vec<Key>,
    started: bool,
    routes: BTreeMap<Key, BTreeMap<String, TriggerId>>,
    // Keep definition/offer/consumer code alive after engine and action teardown.
    _snapshots: Vec<PluginSnapshot>,
}

impl Drop for Kernel {
    fn drop(&mut self) {
        self.producers
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .producers
            .clear();
    }
}

impl Kernel {
    /// Resolve all dependencies before composing, compile once, build one deployment,
    /// without activating producers. Lexical keys break topological ties.
    pub fn build(
        name: impl Into<String>,
        snapshots: impl IntoIterator<Item = PluginSnapshot>,
        executor: Arc<dyn Executor>,
    ) -> Result<Self, ResolutionError> {
        Self::build_with_options(name, snapshots, executor, BuildOptions::default())
    }

    /// Discover compatible offers, apply trusted eligibility and selections,
    /// and order composers before invoking any plugin code. A unique eligible
    /// offer is selected automatically; missing or ambiguous imports fail closed.
    pub fn build_with_options(
        name: impl Into<String>,
        snapshots: impl IntoIterator<Item = PluginSnapshot>,
        executor: Arc<dyn Executor>,
        options: BuildOptions,
    ) -> Result<Self, ResolutionError> {
        let policy = &options.imports;
        let snapshots: Vec<_> = snapshots.into_iter().collect();
        let mut owners = BTreeSet::new();
        for snapshot in &snapshots {
            if !owners.insert(snapshot.name()) {
                return Err(ResolutionError::DuplicateOwner(snapshot.name().into()));
            }
        }
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
                                return Err(format!("duplicate contribution: {key:?}").into());
                            }
                            continue;
                        }
                    };
                }
                collect!(ContractDefinition, definitions);
                collect!(GraphOffer, offers);
                collect!(ContractRequirement, requirements);
                if kind != <RegisteredComposer as phenix_plugin::Contribution>::KIND {
                    continue;
                }
                let handle = snapshot
                    .get::<RegisteredComposer>(id)
                    .ok_or_else(|| format!("contribution type mismatch: {key:?}"))?;
                if pending.insert(key.clone(), handle).is_some() {
                    return Err(format!("duplicate contribution: {key:?}").into());
                }
            }
        }
        let mut identities = BTreeMap::new();
        for (key, definition) in &definitions {
            if let Some(previous) =
                identities.insert(definition.contract.id(), &definition.contract)
                && previous != &definition.contract
            {
                return Err(ResolutionError::ConflictingDefinition {
                    definition: key.clone(),
                    contract: definition.contract.id().into(),
                });
            }
        }
        for (key, offer) in &offers {
            if !definitions.contains_key(&offer.contract) {
                return Err(format!(
                    "missing contract definition for offer {key:?}: {:?}",
                    offer.contract
                )
                .into());
            }
            if offer.producer.owner != key.owner || !pending.contains_key(&offer.producer) {
                return Err(ResolutionError::WrongOrigin {
                    offer: key.clone(),
                    producer: Box::new(offer.producer.clone()),
                    unit: offer.unit.clone(),
                });
            }
        }
        let mut imports: BTreeMap<Key, BTreeMap<String, ResolvedImport>> = BTreeMap::new();
        let mut dependencies: BTreeMap<Key, BTreeSet<Key>> = pending
            .iter()
            .map(|(key, contribution)| {
                (key.clone(), contribution.requires.iter().cloned().collect())
            })
            .collect();
        for (requirement, offer) in policy
            .selections
            .iter()
            .chain(
                policy
                    .allow
                    .iter()
                    .flat_map(|pairs| pairs.iter().map(|(r, o)| (r, o))),
            )
            .chain(policy.deny.iter().map(|(r, o)| (r, o)))
        {
            if !requirements.contains_key(requirement) {
                return Err(
                    format!("host policy names unknown requirement: {requirement:?}").into(),
                );
            }
            if !offers.contains_key(offer) {
                return Err(format!("host policy names unknown offer: {offer:?}").into());
            }
        }
        for (key, requirement) in &requirements {
            if !definitions.contains_key(&requirement.contract) {
                return Err(format!(
                    "missing contract definition for requirement {key:?}: {:?}",
                    requirement.contract
                )
                .into());
            }
            if requirement.consumer.owner != key.owner
                || !pending.contains_key(&requirement.consumer)
            {
                return Err(format!(
                    "requirement {key:?}: invalid consumer ownership: {:?}",
                    requirement.consumer
                )
                .into());
            }
            let requested = &definitions[&requirement.contract].contract;
            let candidates: Vec<_> = offers
                .iter()
                .filter(|(offer_key, offer)| {
                    let pair = (key.clone(), (*offer_key).clone());
                    !policy.deny.contains(&pair)
                        && policy
                            .allow
                            .as_ref()
                            .is_none_or(|allow| allow.contains(&pair))
                        && requested
                            .check_definition(&definitions[&offer.contract].contract)
                            .is_ok()
                })
                .map(|(key, _)| key.clone())
                .collect();
            let selected = if let Some(selected) = policy.selections.get(key) {
                if !candidates.contains(selected) {
                    let reason = match offers.get(selected) {
                        None => "missing selected offer".into(),
                        Some(offer) => match requested
                            .check_definition(&definitions[&offer.contract].contract)
                        {
                            Err(error) => {
                                format!("offer advertising an incompatible contract: {error}")
                            }
                            Ok(()) => "offer denied by host eligibility policy".into(),
                        },
                    };
                    return Err(ResolutionError::Ineligible {
                        requirement: key.clone(),
                        offer: selected.clone(),
                        reason: reason.into(),
                    });
                }
                selected.clone()
            } else {
                match candidates.as_slice() {
                    [] => {
                        return Err(ResolutionError::Missing {
                            requirement: key.clone(),
                            contract: requirement.contract.clone(),
                        });
                    }
                    [selected] => selected.clone(),
                    _ => {
                        return Err(ResolutionError::Ambiguous {
                            requirement: key.clone(),
                            offers: candidates,
                        });
                    }
                }
            };
            let offer = &offers[&selected];
            // A composer can create its own provider before projecting a self-import.
            if offer.producer != requirement.consumer {
                dependencies
                    .get_mut(&requirement.consumer)
                    .unwrap()
                    .insert(offer.producer.clone());
            }
            imports
                .entry(requirement.consumer.clone())
                .or_default()
                .insert(
                    requirement.id.clone(),
                    ResolvedImport {
                        offer: selected,
                        producer: offer.producer.clone(),
                        unit: offer.unit.clone(),
                        contract: requested.clone(),
                    },
                );
        }
        for (key, contribution) in &pending {
            let mut dependencies = BTreeSet::new();
            for dependency in &contribution.requires {
                if !dependencies.insert(dependency) {
                    return Err(format!("duplicate dependency for {key:?}: {dependency:?}").into());
                }
                if !pending.contains_key(dependency) {
                    return Err(format!("missing dependency for {key:?}: {dependency:?}").into());
                }
            }
        }
        let mut resolved = BTreeSet::new();
        // Distinct composers retain their exact plugin revision's ancestry.
        let origins: BTreeMap<_, _> = pending
            .iter()
            .map(|(key, contribution)| (key.clone(), contribution.graph_origin().child()))
            .collect();
        let mut leases = Vec::new();
        let mut keys = Vec::new();
        while !pending.is_empty() {
            let key = pending
                .iter()
                .find(|(key, _)| dependencies[*key].iter().all(|k| resolved.contains(k)))
                .map(|(key, _)| key.clone())
                .ok_or_else(|| ResolutionError::Cycle(pending.keys().cloned().collect()))?;
            leases.push(pending.remove(&key).expect("selected pending contribution"));
            keys.push(key.clone());
            resolved.insert(key);
        }
        // Candidate artifacts and builder are dropped before leases on build failure.
        let mut artifacts = BTreeMap::new();
        let mut graph = GraphBuilder::new(name);
        let empty_imports = BTreeMap::new();
        for (key, contribution) in keys.iter().zip(&leases) {
            graph
                .with_origin(&origins[key], |graph| {
                    (contribution.compose)(&mut ComposeContext {
                        graph,
                        imports: imports.get(key).unwrap_or(&empty_imports),
                        artifacts: artifacts.entry(key.clone()).or_default(),
                    })
                })
                .map_err(|cause| ResolutionError::Composition {
                    contribution: key.clone(),
                    cause,
                })?;
        }
        for (key, offer) in &offers {
            graph
                .require_contract(&offer.unit, &definitions[&offer.contract].contract)
                .map_err(|cause| ResolutionError::Interface {
                    offer: Some(key.clone()),
                    unit: offer.unit.clone(),
                    cause: Box::new(cause),
                })?;
        }
        let compiled = graph
            .compile()
            .map_err(|cause| ResolutionError::Compilation { cause })?;
        // Check all registered definitions against all compiled claims and
        // requests, including units with no offer or selected requirement.
        compiled
            .check_contract_catalog(definitions.values().map(|entry| &entry.contract))
            .map_err(|cause| ResolutionError::Interface {
                offer: None,
                unit: cause.path.to_string(),
                cause: Box::new(cause),
            })?;
        for (key, offer) in &offers {
            let contract = &definitions[&offer.contract].contract;
            let unit = compiled
                .find(&offer.unit)
                .ok_or_else(|| ResolutionError::MissingUnit {
                    offer: key.clone(),
                    unit: offer.unit.clone(),
                })?;
            if unit.origin() != Some(&origins[&offer.producer]) {
                return Err(ResolutionError::WrongOrigin {
                    offer: key.clone(),
                    producer: Box::new(offer.producer.clone()),
                    unit: offer.unit.clone(),
                });
            }
            contract
                .check_implementation(&unit)
                .map_err(|cause| ResolutionError::Interface {
                    offer: Some(key.clone()),
                    unit: offer.unit.clone(),
                    cause: Box::new(cause),
                })?;
        }
        let mut routes = BTreeMap::new();
        for (key, contribution) in keys.iter().zip(&leases) {
            if artifacts[key].activate.is_none() && !contribution.routes.is_empty() {
                return Err(format!("routes declared without producer: {key:?}").into());
            }
            let mut selected = BTreeMap::new();
            for name in &contribution.routes {
                let trigger = compiled
                    .trigger(name)
                    .ok_or_else(|| format!("unknown producer route for {key:?}: {name:?}"))?;
                if selected.insert(name.clone(), trigger).is_some() {
                    return Err(format!("duplicate producer route for {key:?}: {name:?}").into());
                }
            }
            routes.insert(key.clone(), selected);
        }
        let engine = Deployment::new(compiled)
            .max_activations(options.execution.max_activations)
            .executor(executor)
            .build()?;
        Ok(Self {
            engine,
            producers: Mutex::new(ProducerState::default()),
            artifacts,
            _leases: leases,
            keys,
            started: false,
            routes,
            _snapshots: snapshots,
        })
    }

    /// Activate in dependency order. Failure drops all newly acquired producers
    /// and leaves the kernel unstarted, allowing retry. A second successful start
    /// is rejected, including when no contributions have producers.
    pub fn start(&mut self) -> Result<(), StartError> {
        if self.started {
            return Err(StartError::AlreadyStarted);
        }
        let mut producers = Vec::new();
        for key in &self.keys {
            if let Some(activate) = &self.artifacts[key].activate {
                let producer = activate().map_err(|cause| StartError::Activation {
                    contribution: key.clone(),
                    cause,
                })?;
                producers.push((key.clone(), producer));
            }
        }
        self.producers
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .producers = producers;
        self.started = true;
        Ok(())
    }

    /// Low-level engine access bypasses the kernel's started-state admission guard.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Retrieve this deployment's composed export, never a shared snapshot slot.
    /// Missing keys and mismatched types return None.
    pub fn export<T: Send + Sync + 'static>(&self, key: &Key) -> Option<&T> {
        self.artifacts.get(key)?.export.as_ref()?.downcast_ref()
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
    /// calls. Idle polls count toward the bound. Poll errors retire only the failed
    /// producer; admission errors do not stop polling.
    /// This bounds admissions, not the execution time of an individual wave.
    pub fn drain(&self, max_polls: usize) -> Vec<ProducerEvent> {
        // Plugin destructors may reenter drain; release the state lock first.
        let mut detached = Vec::new();
        let mut state = self
            .producers
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut events = Vec::new();
        if state.retired {
            return events;
        }
        for _ in 0..max_polls {
            if state.producers.is_empty() {
                break;
            }
            let index = state.next;
            state.next = (index + 1) % state.producers.len();
            let (key, producer) = &mut state.producers[index];
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
            if matches!(events.last().unwrap().result, Err(ProducerError::Poll(_))) {
                detached.push(state.producers.remove(index));
                state.next = index;
            }
            if state.next >= state.producers.len() {
                state.next = 0;
            }
        }
        drop(state);
        drop(detached);
        events
    }

    /// Drive a started kernel until shutdown, delivering events without retaining reports.
    /// Every producer must support wake registration. Shutdown is checked between waves;
    /// it does not interrupt a running producer poll, wave, or callback.
    pub fn run_until(
        &mut self,
        wake: &WakeHandle,
        mut on_event: impl FnMut(ProducerEvent),
    ) -> Result<(), DriverError> {
        if !self.started {
            return Err(DriverError::NotStarted);
        }
        let state = self
            .producers
            .get_mut()
            .unwrap_or_else(|error| error.into_inner());
        for (key, producer) in &mut state.producers {
            if !producer.set_waker(wake.clone()) {
                return Err(DriverError::UnsupportedWake {
                    producer: key.clone(),
                });
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
            let count = self
                .producers
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .producers
                .len();
            for _ in 0..count {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_producer_destructors_reenter_after_all_failures_are_detached() {
        use std::sync::{
            Weak,
            atomic::{AtomicUsize, Ordering},
        };

        struct Source(Weak<Kernel>, Arc<AtomicUsize>);
        impl Producer for Source {
            fn poll(&mut self) -> Result<Option<Notice>, CallbackError> {
                Err("terminal".into())
            }
        }
        impl Drop for Source {
            fn drop(&mut self) {
                let kernel = self.0.upgrade().unwrap();
                // Fail deterministically rather than hanging if Drop holds the lock.
                {
                    let state = kernel
                        .producers
                        .try_lock()
                        .expect("Drop held producer mutex");
                    assert!(state.producers.is_empty());
                    assert_eq!(state.next, 0);
                }
                assert!(kernel.drain(1).is_empty());
                self.1.fetch_add(1, Ordering::SeqCst);
            }
        }

        let mut kernel =
            Kernel::build("reentrant", [], Arc::new(phenix_graph::LocalExecutor)).unwrap();
        kernel.start().unwrap();
        let kernel = Arc::new(kernel);
        let drops = Arc::new(AtomicUsize::new(0));
        kernel.producers.lock().unwrap().producers = ["a", "b"]
            .into_iter()
            .map(|name| {
                (
                    Key::new("test", name),
                    Box::new(Source(Arc::downgrade(&kernel), drops.clone())) as Box<dyn Producer>,
                )
            })
            .collect();
        let events = kernel.drain(2);
        assert_eq!(
            events
                .iter()
                .map(|event| event.producer.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert!(
            events
                .iter()
                .all(|event| matches!(event.result, Err(ProducerError::Poll(_))))
        );
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        assert!(kernel.drain(2).is_empty());
    }

    #[test]
    fn terminal_failure_preserves_round_robin_and_idle_poll_budget() {
        struct Source(&'static str, Arc<Mutex<Vec<&'static str>>>);
        impl Producer for Source {
            fn poll(&mut self) -> Result<Option<Notice>, CallbackError> {
                self.1.lock().unwrap().push(self.0);
                if self.0 == "broken" {
                    Err("terminal".into())
                } else {
                    Ok(None)
                }
            }
        }
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut kernel = Kernel::build("fair", [], Arc::new(phenix_graph::LocalExecutor)).unwrap();
        kernel.start().unwrap();
        kernel.producers.get_mut().unwrap().producers = ["a", "broken", "b"]
            .into_iter()
            .map(|name| {
                (
                    Key::new("test", name),
                    Box::new(Source(name, log.clone())) as Box<dyn Producer>,
                )
            })
            .collect();
        assert!(kernel.drain(0).is_empty());
        assert!(log.lock().unwrap().is_empty());
        assert!(kernel.drain(1).is_empty());
        let events = kernel.drain(1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].producer, Key::new("test", "broken"));
        assert!(kernel.drain(3).is_empty());
        assert_eq!(*log.lock().unwrap(), ["a", "broken", "b", "a", "b"]);
    }
}
