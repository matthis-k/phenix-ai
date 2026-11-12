//! Host-authorized native network composition and persistent producer lifecycle.
pub mod host;

use phenix_graph::{
    Value,
    compose::{Composer, Footprint, NetworkPatch, Registration},
    contract::Contract,
    network::{BoundComponent, CompiledNetwork, NetworkBuilder, NetworkError},
    process::ProcessDispatcher,
    runtime::{InjectionError, NetworkSession, RunReport, SessionOptions, SessionState},
};
pub use phenix_plugin::CallbackError;
use phenix_plugin::{ContributionHandle, PluginSnapshot, lease_callback_error};
use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, TryLockError},
};

// Fail fast only on same-thread recursive access to an identical kernel mutex.
// Cross-thread contention still follows std::sync::Mutex's blocking semantics.
thread_local! {
    static ACTIVE_KERNEL_LOCKS: RefCell<BTreeSet<usize>> = RefCell::new(BTreeSet::new());
}

struct KernelLockScope(usize);
impl KernelLockScope {
    fn enter<T>(lock: &Mutex<T>) -> Option<Self> {
        let identity = lock as *const Mutex<T> as usize;
        ACTIVE_KERNEL_LOCKS
            .with(|active| active.borrow_mut().insert(identity))
            .then_some(Self(identity))
    }
}
impl Drop for KernelLockScope {
    fn drop(&mut self) {
        ACTIVE_KERNEL_LOCKS.with(|active| {
            active.borrow_mut().remove(&self.0);
        });
    }
}

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

/// One bounded external admission to one public network input.
pub struct InputEvent {
    pub input: String,
    pub value: Value,
}

/// A producer must not wait for an event. Drop must stop and release its resources.
/// Factories must clean up partially acquired resources if activation fails.
pub trait Producer: Send {
    /// An error is terminal: the kernel reports it once and drops this producer.
    /// Transient unavailability must return Ok(None) and wake when ready to retry.
    fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError>;

    /// Register notification for newly available events. Return false if unsupported.
    /// Implementations must synchronize registration with event publication.
    fn set_waker(&mut self, _waker: WakeHandle) -> bool {
        false
    }
}

#[derive(Default)]
struct WakeState {
    shutdown: Mutex<bool>,
    notifier: Mutex<Option<phenix_graph::runtime::SessionNotifier>>,
}

/// Cloneable notification and shutdown handle for the optional blocking driver.
#[derive(Clone, Default)]
pub struct WakeHandle(Arc<WakeState>);

impl WakeHandle {
    pub fn wake(&self) {
        if let Some(notifier) = self.0.notifier.lock().unwrap().as_ref() {
            notifier.notify();
        }
    }

    /// Shutdown is sticky. Use a new handle for a subsequent driver run.
    pub fn shutdown(&self) {
        *self.0.shutdown.lock().unwrap() = true;
        if let Some(notifier) = self.0.notifier.lock().unwrap().as_ref() {
            notifier.notify();
        }
    }
}

pub type Prepare =
    dyn Fn(&CandidateContext<'_>) -> Result<CandidateArtifacts, CallbackError> + Send + Sync;
pub type Activate = dyn Fn() -> Result<Box<dyn Producer>, CallbackError> + Send + Sync;

/// Owned selection metadata. Projection uses the graph's authoritative contract checker.
#[derive(Clone, Debug)]
pub struct ResolvedImport {
    pub offer: Key,
    pub producer: Key,
    unit: String,
    contract: Contract,
}

impl ResolvedImport {
    pub fn bind<'g>(
        &self,
        network: &'g CompiledNetwork,
    ) -> Result<BoundComponent<'g>, NetworkError> {
        network.bind(&self.unit, &self.contract)
    }
}

/// Declared-import-scoped view provided to a plugin's preparation callback.
/// The complete compiled topology is hidden unless the host explicitly grants inspection.
///
/// ```compile_fail
/// use phenix_kernel::CandidateContext;
/// fn inspect_all(context: &CandidateContext<'_>) {
///     let _ = context.compiled.inputs();
/// }
/// ```
pub struct CandidateContext<'a> {
    compiled: &'a CompiledNetwork,
    imports: &'a BTreeMap<String, ResolvedImport>,
    can_inspect_network: bool,
}

#[derive(Default)]
pub struct CandidateArtifacts {
    /// Optional candidate-specific replacement for the contribution's patch.
    pub patch: Option<NetworkPatch>,
    pub activate: Option<Box<Activate>>,
    pub export: Option<Box<dyn Any + Send + Sync>>,
}

impl CandidateArtifacts {
    /// Register this candidate's producer factory, capturing its exact composed state.
    /// Only one factory is allowed per contribution in each candidate.
    pub fn activate(
        &mut self,
        factory: impl Fn() -> Result<Box<dyn Producer>, CallbackError> + Send + Sync + 'static,
    ) -> Result<(), CallbackError> {
        if self.activate.is_some() {
            return Err("candidate activation already registered".into());
        }
        self.activate = Some(Box::new(factory));
        Ok(())
    }

    /// Export one typed value owned by this candidate and scoped to its contribution key.
    /// Bundle multiple values in a struct; retrieve it through Kernel::export.
    pub fn export<T: Send + Sync + 'static>(&mut self, value: T) -> Result<(), CallbackError> {
        if self.export.is_some() {
            return Err("candidate export already registered".into());
        }
        self.export = Some(Box::new(value));
        Ok(())
    }
}

impl CandidateContext<'_> {
    /// Look up only this contribution's declared imports. The result owns its
    /// metadata so callers can retain provider and offer identities.
    pub fn import(&self, id: &str) -> Result<ResolvedImport, CallbackError> {
        self.imports
            .get(id)
            .cloned()
            .ok_or_else(|| format!("undeclared import: {id}").into())
    }

    /// Project only a declared import using the graph's authoritative contract
    /// and type checks. Unrelated compiled topology is not available here.
    pub fn bind_import(&self, id: &str) -> Result<BoundComponent<'_>, CallbackError> {
        let imported = self
            .imports
            .get(id)
            .ok_or_else(|| format!("undeclared import: {id}"))?;
        imported.bind(self.compiled).map_err(Into::into)
    }

    /// Full immutable topology inspection requires an explicit, exact
    /// contribution grant in the host's BuildOptions. Not a sandbox boundary.
    pub fn inspect_network(&self) -> Result<&CompiledNetwork, CallbackError> {
        if self.can_inspect_network {
            Ok(self.compiled)
        } else {
            Err("full compiled network inspection not granted".into())
        }
    }
}

/// Shared Rust type registered by plugins through the existing Registrar.
/// Payloads never carry write grants or mutable candidate access.
pub struct Contribution {
    pub id: String,
    pub requires: Vec<Key>,
    /// Public input names this contribution's producer may inject.
    pub routes: Vec<String>,
    pub patch: NetworkPatch,
    pub prepare: Option<Box<Prepare>>,
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
    pub contract: Contract,
}

impl phenix_plugin::Contribution for ContractDefinition {
    const KIND: &'static str = "network.contract";
    fn id(&self) -> &str {
        &self.id
    }
}

/// An offer names an actual unit, never a purported implementation shape.
/// Every offer is checked after all composition, including unused offers.
pub struct NetworkOffer {
    pub id: String,
    /// Exact contribution responsible for creating the offered unit.
    pub producer: Key,
    pub unit: String,
    pub contract: Key,
}

impl phenix_plugin::Contribution for NetworkOffer {
    const KIND: &'static str = "network.offer";
    fn id(&self) -> &str {
        &self.id
    }
    fn component_path(&self) -> Option<&str> {
        Some(&self.unit)
    }
}

/// A local declarative import. Only the trusted host can select its provider.
pub struct ContractRequirement {
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

/// Trusted import selection, exact composition grants and native session options.
#[derive(Default)]
pub struct BuildOptions {
    pub imports: ImportPolicy,
    /// The build dispatcher argument is authoritative; the kernel adds snapshot leases.
    pub execution: SessionOptions,
    /// Trusted exact write authority, keyed by contribution identity. No implicit grants.
    pub composition: BTreeMap<Key, CompositionPolicy>,
    /// Exact contributions allowed to inspect the entire compiled candidate
    /// during preparation. Empty by default; distinct from graph write grants.
    pub inspect_network: BTreeSet<Key>,
}

#[derive(Clone, Debug, Default)]
pub struct CompositionPolicy {
    pub grants: BTreeSet<Footprint>,
    pub priority: i64,
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
        cause: NetworkError,
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
        cause: Box<NetworkError>,
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
    const KIND: &'static str = "network.requirement";
    fn id(&self) -> &str {
        &self.id
    }
}

/// One poll's admission outcome, distinct from subsequent session execution.
pub struct ProducerEvent {
    pub producer: Key,
    pub result: Result<(), ProducerError>,
}

/// Process failures belong to the native session, not producer admission.
#[derive(Debug)]
pub enum ProducerError {
    Poll(CallbackError),
    UndeclaredRoute(String),
    Admission(InjectionError),
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
            Self::Admission(cause) => Some(cause),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum DriverError {
    NotStarted,
    UnsupportedWake {
        producer: Key,
    },
    WakeRegistrationPanicked {
        producer: Key,
    },
    SessionStopped {
        state: SessionState,
        cause: Option<String>,
    },
}

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotStarted => f.write_str("kernel not started"),
            Self::UnsupportedWake { producer } => write!(
                f,
                "producer does not support wake notifications: {producer:?}"
            ),
            Self::WakeRegistrationPanicked { producer } => {
                write!(f, "producer wake registration panicked: {producer:?}")
            }
            Self::SessionStopped { state, cause } => {
                write!(f, "native session stopped: {state:?}: {cause:?}")
            }
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

/// A source identity, its temporarily checked-out mutable callback, and
/// at most one retained event under upstream backpressure.
type ProducerSlot = (Key, Option<Box<dyn Producer>>, Option<InputEvent>);

#[derive(Default)]
struct ProducerState {
    producers: Vec<ProducerSlot>,
    next: usize,
    retired: bool,
}

pub struct Kernel {
    // Drop clears producers first; leases outlive session and compiled declarations.
    session: Mutex<NetworkSession>,
    compiled: CompiledNetwork,
    producers: Mutex<ProducerState>,
    // Serialize one whole drain against generation retirement. Inventory
    // mutations use a separate short lock; no user poll/readiness runs under it.
    producer_drain: Mutex<()>,
    artifacts: BTreeMap<Key, CandidateArtifacts>,
    _leases: Vec<ContributionHandle<Contribution>>,
    keys: Vec<Key>,
    started: bool,
    routes: BTreeMap<Key, BTreeSet<String>>,
    // Keep definition/offer/consumer code alive after session and factory teardown.
    _snapshots: Vec<PluginSnapshot>,
    code_lease: Arc<dyn Send + Sync>,
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
    /// Resolve all dependencies before composing and build one native session,
    /// without activating producers. Lexical keys break topological ties.
    pub fn build(
        name: impl Into<String>,
        snapshots: impl IntoIterator<Item = PluginSnapshot>,
        dispatcher: Arc<dyn ProcessDispatcher>,
    ) -> Result<Self, ResolutionError> {
        Self::build_with_options(name, snapshots, dispatcher, BuildOptions::default())
    }

    /// Discover compatible offers, apply trusted eligibility and selections,
    /// and order composers before invoking any plugin code. A unique eligible
    /// offer is selected automatically; missing or ambiguous imports fail closed.
    pub fn build_with_options(
        _name: impl Into<String>,
        snapshots: impl IntoIterator<Item = PluginSnapshot>,
        dispatcher: Arc<dyn ProcessDispatcher>,
        mut options: BuildOptions,
    ) -> Result<Self, ResolutionError> {
        let policy = &options.imports;
        let snapshots: Vec<_> = snapshots.into_iter().collect();
        let code_lease: Arc<dyn Send + Sync> =
            Arc::new((options.execution.lease.take(), snapshots.clone()));
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
                collect!(NetworkOffer, offers);
                collect!(ContractRequirement, requirements);
                if kind != <Contribution as phenix_plugin::Contribution>::KIND {
                    continue;
                }
                let handle = snapshot
                    .get::<Contribution>(id)
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
        let base = NetworkBuilder::new()
            .build()
            .map_err(|cause| ResolutionError::Compilation { cause })?;
        let mut composer =
            Composer::new(base).map_err(|cause| ResolutionError::Invalid(cause.to_string()))?;
        let registration_ids: BTreeMap<_, _> = keys
            .iter()
            .enumerate()
            .map(|(index, key)| (key.clone(), index.to_string()))
            .collect();
        let empty_imports = BTreeMap::new();
        for (key, contribution) in keys.iter().zip(&leases) {
            let authority = options.composition.get(key).cloned().unwrap_or_default();
            // A self-import may bind the immutable static payload, never a made-up
            // interface. The same host grants and origin checks govern this preview.
            let preview = if imports
                .get(key)
                .is_some_and(|entries| entries.values().any(|import| import.producer == *key))
            {
                let mut preview =
                    Composer::new(composer.current().network().clone()).map_err(|cause| {
                        ResolutionError::Composition {
                            contribution: key.clone(),
                            cause: Box::new(cause),
                        }
                    })?;
                preview
                    .register([Registration::new(
                        "self",
                        [],
                        authority.priority,
                        origins[key].clone(),
                        authority.grants.clone(),
                        contribution.patch.clone(),
                    )])
                    .map_err(|cause| ResolutionError::Composition {
                        contribution: key.clone(),
                        cause: Box::new(cause),
                    })?;
                Some(preview)
            } else {
                None
            };
            let context_compiled = preview
                .as_ref()
                .map_or_else(
                    || composer.current().compiled(),
                    |preview| preview.current().compiled(),
                )
                .with_lease(code_lease.clone());
            let mut owned = match &contribution.prepare {
                Some(prepare) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    prepare(&CandidateContext {
                        compiled: &context_compiled,
                        imports: imports.get(key).unwrap_or(&empty_imports),
                        can_inspect_network: options.inspect_network.contains(key),
                    })
                }))
                .unwrap_or_else(|_| Err("plugin preparation panicked".into()))
                .map_err(|cause| ResolutionError::Composition {
                    contribution: key.clone(),
                    cause: lease_callback_error(cause, code_lease.clone()),
                })?,
                None => CandidateArtifacts::default(),
            };
            let patch = owned
                .patch
                .take()
                .unwrap_or_else(|| contribution.patch.clone());
            artifacts.insert(key.clone(), owned);
            composer
                .register([Registration::new(
                    registration_ids[key].clone(),
                    dependencies[key]
                        .iter()
                        .map(|dependency| registration_ids[dependency].clone()),
                    authority.priority,
                    origins[key].clone(),
                    authority.grants,
                    patch,
                )])
                .map_err(|cause| match cause {
                    phenix_graph::compose::ComposeError::Network(cause) => {
                        ResolutionError::Compilation { cause }
                    }
                    cause => ResolutionError::Composition {
                        contribution: key.clone(),
                        cause: Box::new(cause),
                    },
                })?;
        }
        let compiled = composer.current().compiled().with_lease(code_lease.clone());
        let active = composer.current().active();
        let selected: BTreeSet<_> = keys
            .iter()
            .filter(|key| active.contains(&registration_ids[*key]))
            .cloned()
            .collect();
        artifacts.retain(|key, _| selected.contains(key));
        let (keys, leases): (Vec<_>, Vec<_>) = keys
            .into_iter()
            .zip(leases)
            .filter(|(key, _)| selected.contains(key))
            .unzip();
        // Check all registered definitions against all compiled claims and
        // requests, including units with no offer or selected requirement.
        compiled
            .check_contract_catalog(definitions.values().map(|entry| &entry.contract))
            .map_err(|cause| ResolutionError::Interface {
                offer: None,
                unit: String::new(),
                cause: Box::new(cause),
            })?;
        for (key, offer) in &offers {
            let contract = &definitions[&offer.contract].contract;
            let unit = compiled
                .inspect(&offer.unit)
                .map_err(|_| ResolutionError::MissingUnit {
                    offer: key.clone(),
                    unit: offer.unit.clone(),
                })?;
            if unit.origin.as_ref() != Some(&origins[&offer.producer]) {
                return Err(ResolutionError::WrongOrigin {
                    offer: key.clone(),
                    producer: Box::new(offer.producer.clone()),
                    unit: offer.unit.clone(),
                });
            }
            compiled
                .check_implementation(&offer.unit, contract)
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
            let mut selected = BTreeSet::new();
            for name in &contribution.routes {
                if !compiled.inputs().contains_key(name) {
                    return Err(format!("unknown producer route for {key:?}: {name:?}").into());
                }
                if !selected.insert(name.clone()) {
                    return Err(format!("duplicate producer route for {key:?}: {name:?}").into());
                }
            }
            routes.insert(key.clone(), selected);
        }
        // The session propagates this lease into dispatched jobs and queued packets.
        // Do not let a caller-provided lease displace the plugin revision leases.
        options.execution.lease = Some(code_lease.clone());
        options.execution.dispatcher = dispatcher;
        let session = compiled.start_with_options(options.execution);
        if let Some(failure) = session.failure() {
            return Err(ResolutionError::Invalid(failure.into()));
        }
        Ok(Self {
            session: Mutex::new(session),
            compiled,
            producers: Mutex::new(ProducerState::default()),
            producer_drain: Mutex::new(()),
            artifacts,
            _leases: leases,
            keys,
            started: false,
            routes,
            _snapshots: snapshots,
            code_lease,
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
                let producer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(activate))
                    .unwrap_or_else(|_| Err("producer activation panicked".into()))
                    .map_err(|cause| StartError::Activation {
                        contribution: key.clone(),
                        cause: lease_callback_error(cause, self.code_lease.clone()),
                    })?;
                producers.push((key.clone(), Some(producer), None));
            }
        }
        self.producers
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .producers = producers;
        self.started = true;
        Ok(())
    }

    /// Immutable declaration inspection never admits work or exposes process state.
    pub fn compiled(&self) -> &CompiledNetwork {
        &self.compiled
    }

    /// Retrieve this candidate's composed export, never a shared snapshot slot.
    /// Missing keys and mismatched types return None.
    pub fn export<T: Send + Sync + 'static>(&self, key: &Key) -> Option<&T> {
        self.artifacts.get(key)?.export.as_ref()?.downcast_ref()
    }

    pub fn with_session<R>(
        &self,
        use_session: impl FnOnce(&mut NetworkSession) -> R,
    ) -> Result<R, String> {
        if !self.started {
            return Err("kernel not started".into());
        }
        let _scope = KernelLockScope::enter(&self.session)
            .ok_or_else(|| "reentrant kernel session access".to_owned())?;
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let result = use_session(&mut session);
        session.notifier().notify();
        Ok(result)
    }

    pub fn inject(&self, input: &str, value: Value) -> Result<(), InjectionError> {
        if !self.started {
            return Err(InjectionError {
                reason: phenix_graph::connection::ConnectionError::Cancelled,
                value,
            });
        }
        let Some(_scope) = KernelLockScope::enter(&self.session) else {
            return Err(InjectionError {
                reason: phenix_graph::connection::ConnectionError::Reentrant,
                value,
            });
        };
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let result = session.inject(input, value);
        if result.is_ok() {
            session.notifier().notify();
        }
        result
    }

    pub fn run_steps(&self, budget: usize) -> Result<RunReport, String> {
        if !self.started {
            return Err("kernel not started".into());
        }
        let _scope = KernelLockScope::enter(&self.session)
            .ok_or_else(|| "reentrant kernel session access".to_owned())?;
        Ok(self
            .session
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .run_until_idle(budget))
    }

    /// Perform at most `max_polls` nonblocking producer polls, round-robin across
    /// calls. One whole drain is serialized against retirement, but callbacks
    /// and eager session admission do not hold the producer inventory mutex.
    /// A full input retains one event without repolling. No process code runs here.
    pub fn drain(&self, max_polls: usize) -> Vec<ProducerEvent> {
        let scope = KernelLockScope::enter(&self.producer_drain).expect("recursive producer drain");
        let drain = self
            .producer_drain
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // Producer destructors may reenter inventory inspection. Drop them only
        // after both the drain gate and inventory mutex have been released.
        let mut detached = Vec::new();
        let mut events = Vec::new();
        for _ in 0..max_polls {
            let (index, key, mut producer, pending) = {
                let mut state = self
                    .producers
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if state.retired || state.producers.is_empty() {
                    break;
                }
                let index = state.next;
                state.next = (index + 1) % state.producers.len();
                let (key, producer, pending) = &mut state.producers[index];
                (
                    index,
                    key.clone(),
                    producer.take().expect("exclusive producer drain ownership"),
                    pending.take(),
                )
            };

            let polled = match pending {
                Some(event) => Ok(Some(event)),
                None => {
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| producer.poll()))
                    {
                        Ok(result) => result,
                        Err(_) => Err("producer poll panicked".into()),
                    }
                }
            };
            let mut retained = None;
            let mut terminal_poll = false;
            let outcome = match polled {
                Ok(None) => None,
                Ok(Some(event)) => {
                    if !self.routes[&key].contains(&event.input) {
                        Some(Err(ProducerError::UndeclaredRoute(event.input)))
                    } else {
                        // Session access stays nonblocking. On contention or
                        // Full, retain exactly one owned event, never repoll.
                        let session = match self.session.try_lock() {
                            Ok(session) => Some(session),
                            Err(TryLockError::Poisoned(error)) => Some(error.into_inner()),
                            Err(TryLockError::WouldBlock) => None,
                        };
                        if let Some(mut session) = session {
                            let input = event.input;
                            match session.inject(&input, event.value) {
                                Ok(()) => {
                                    session.notifier().notify();
                                    Some(Ok(()))
                                }
                                Err(error)
                                    if error.reason
                                        == phenix_graph::connection::ConnectionError::Full =>
                                {
                                    retained = Some(InputEvent {
                                        input,
                                        value: error.value,
                                    });
                                    None
                                }
                                Err(error) => Some(Err(ProducerError::Admission(error))),
                            }
                        } else {
                            retained = Some(event);
                            None
                        }
                    }
                }
                Err(error) => {
                    terminal_poll = true;
                    Some(Err(ProducerError::Poll(lease_callback_error(
                        error,
                        self.code_lease.clone(),
                    ))))
                }
            };
            {
                let mut state = self
                    .producers
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let slot = &mut state.producers[index];
                debug_assert!(slot.1.is_none());
                slot.1 = Some(producer);
                slot.2 = retained;
                if terminal_poll {
                    detached.push(state.producers.remove(index));
                    state.next = index;
                }
                if state.next >= state.producers.len() {
                    state.next = 0;
                }
            }
            if let Some(result) = outcome {
                events.push(ProducerEvent {
                    producer: key,
                    result,
                });
            }
        }
        drop(drain);
        drop(scope);
        drop(detached);
        events
    }

    /// Drive a started kernel until shutdown, delivering events without retaining reports.
    /// Every producer must support wake registration. Shutdown is checked between
    /// bounded driver iterations; it cannot preempt a running step or callback.
    pub fn run_until(
        &self,
        wake: &WakeHandle,
        mut on_event: impl FnMut(ProducerEvent),
    ) -> Result<(), DriverError> {
        if !self.started {
            return Err(DriverError::NotStarted);
        }
        let (mut listener, notifier) = self
            .with_session(|session| (session.wake_listener(), session.notifier()))
            .unwrap();
        *wake.0.notifier.lock().unwrap() = Some(notifier);
        let registration_scope =
            KernelLockScope::enter(&self.producer_drain).expect("recursive producer drain");
        let drain = self
            .producer_drain
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let count = self
            .producers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .producers
            .len();
        for index in 0..count {
            let (key, mut producer) = {
                let mut state = self
                    .producers
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let (key, producer, _) = &mut state.producers[index];
                (
                    key.clone(),
                    producer
                        .take()
                        .expect("exclusive producer wake registration"),
                )
            };
            // Producer callbacks never run under inventory locks, including
            // during initial driver wake registration.
            let registered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                producer.set_waker(wake.clone())
            }));
            self.producers
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .producers[index]
                .1 = Some(producer);
            match registered {
                Ok(true) => {}
                Ok(false) => {
                    return Err(DriverError::UnsupportedWake { producer: key });
                }
                Err(_) => return Err(DriverError::WakeRegistrationPanicked { producer: key }),
            }
        }
        drop(drain);
        drop(registration_scope);
        loop {
            {
                if *wake.0.shutdown.lock().unwrap() {
                    return Ok(());
                }
            }
            let mut active = false;
            let count = self
                .producers
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .producers
                .len();
            for _ in 0..count {
                if *wake.0.shutdown.lock().unwrap() {
                    return Ok(());
                }
                for event in self.drain(1) {
                    active = true;
                    on_event(event);
                }
            }
            let report = self.run_steps(64).unwrap();
            if matches!(report.state, SessionState::Failed | SessionState::Cancelled) {
                return Err(DriverError::SessionStopped {
                    state: report.state,
                    cause: self
                        .with_session(|session| session.failure().map(str::to_owned))
                        .unwrap(),
                });
            }
            active |= report.steps > 0 || report.publications > 0;
            if !active {
                listener.wait_timeout(std::time::Duration::from_secs(60));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursive_session_calls_fail_fast_without_discarding_input_ownership() {
        let mut kernel =
            Kernel::build("session-reentrancy", [], Arc::new(LocalProcessDispatcher)).unwrap();
        kernel.start().unwrap();
        let mut independent =
            Kernel::build("independent-session", [], Arc::new(LocalProcessDispatcher)).unwrap();
        independent.start().unwrap();

        kernel
            .with_session(|_| {
                assert_eq!(
                    kernel.with_session(|_| ()).unwrap_err(),
                    "reentrant kernel session access"
                );
                assert_eq!(
                    kernel.run_steps(1).unwrap_err(),
                    "reentrant kernel session access"
                );
                let error = kernel.inject("missing", Value::local(71_i64)).unwrap_err();
                assert_eq!(
                    error.reason,
                    phenix_graph::connection::ConnectionError::Reentrant
                );
                assert_eq!(error.value.get::<i64>().unwrap(), 71);
                independent.with_session(|_| ()).unwrap();
            })
            .unwrap();
        // The first scope has left no stale thread-local owner behind.
        kernel.with_session(|_| ()).unwrap();
        assert_eq!(
            kernel
                .inject("missing", Value::local(2_i64))
                .unwrap_err()
                .reason,
            phenix_graph::connection::ConnectionError::InvalidDestination
        );
    }

    #[test]
    fn recursive_drain_in_poll_is_contained_and_releases_the_whole_drain_gate() {
        use std::sync::Weak;

        struct Recursive(Weak<Kernel>);
        impl Producer for Recursive {
            fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
                self.0.upgrade().unwrap().drain(1);
                Ok(None)
            }
        }

        let mut kernel =
            Kernel::build("recursive-drain", [], Arc::new(LocalProcessDispatcher)).unwrap();
        kernel.start().unwrap();
        let kernel = Arc::new(kernel);
        let key = Key::new("test", "recursive");
        kernel.producers.lock().unwrap().producers.push((
            key.clone(),
            Some(Box::new(Recursive(Arc::downgrade(&kernel)))),
            None,
        ));

        let events = kernel.drain(1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].producer, key);
        assert!(matches!(events[0].result, Err(ProducerError::Poll(_))));
        assert!(kernel.drain(1).is_empty());
    }

    #[test]
    fn recursive_drain_in_wake_registration_is_reported_without_losing_producer() {
        use std::sync::Weak;

        struct RecursiveWake(Weak<Kernel>);
        impl Producer for RecursiveWake {
            fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
                Ok(None)
            }

            fn set_waker(&mut self, _: WakeHandle) -> bool {
                self.0.upgrade().unwrap().drain(1);
                true
            }
        }

        let mut kernel =
            Kernel::build("recursive-wake", [], Arc::new(LocalProcessDispatcher)).unwrap();
        kernel.start().unwrap();
        let kernel = Arc::new(kernel);
        let key = Key::new("test", "recursive-wake");
        kernel.producers.lock().unwrap().producers.push((
            key.clone(),
            Some(Box::new(RecursiveWake(Arc::downgrade(&kernel)))),
            None,
        ));
        assert_eq!(
            kernel
                .run_until(&WakeHandle::default(), |_| {})
                .unwrap_err(),
            DriverError::WakeRegistrationPanicked { producer: key }
        );
        assert!(kernel.producers.lock().unwrap().producers[0].1.is_some());
        assert!(kernel.drain(1).is_empty());
    }

    #[test]
    fn eager_readiness_during_drain_may_inspect_unlocked_inventory() {
        use phenix_graph::{
            ComponentDef,
            network::NetworkBuilder,
            runtime::{Readiness, ReadinessPolicy, ReadinessSnapshot},
        };
        use std::num::NonZeroUsize;
        use std::sync::{
            Weak,
            atomic::{AtomicUsize, Ordering},
        };

        struct InspectInventory(Weak<Kernel>, Arc<AtomicUsize>);
        impl ReadinessPolicy for InspectInventory {
            fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
                let kernel = self.0.upgrade().unwrap();
                let state = kernel
                    .producers
                    .try_lock()
                    .expect("readiness ran while producer inventory locked");
                assert_eq!(state.producers.len(), 1);
                assert!(state.producers[0].1.is_none());
                self.1.fetch_add(1, Ordering::SeqCst);
                snapshot
                    .heads
                    .iter()
                    .filter(|head| head.available)
                    .map(|head| head.port.clone())
                    .collect()
            }
        }

        struct Once(bool);
        impl Producer for Once {
            fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
                if self.0 {
                    return Ok(None);
                }
                self.0 = true;
                Ok(Some(InputEvent {
                    input: "in".into(),
                    value: Value::local(7_i64),
                }))
            }
        }
        let selections = Arc::new(AtomicUsize::new(0));
        let kernel = Arc::new_cyclic(|weak| {
            let mut kernel = Kernel::build(
                "readiness-inventory",
                [],
                Arc::new(phenix_graph::process::LocalProcessDispatcher),
            )
            .unwrap();
            kernel.start().unwrap();

            let mut builder = NetworkBuilder::new();
            let sink = builder
                .add("sink", Arc::new(ComponentDef::sink(|_: &i64| {})))
                .unwrap();
            builder
                .expose_input(
                    "in",
                    sink.input::<i64>("input").unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            kernel.session = Mutex::new(
                builder
                    .build()
                    .unwrap()
                    .compile()
                    .unwrap()
                    .start_with_options(SessionOptions {
                        readiness: BTreeMap::from([(
                            "sink".to_owned(),
                            Readiness::Custom(Arc::new(InspectInventory(
                                weak.clone(),
                                selections.clone(),
                            ))),
                        )]),
                        ..Default::default()
                    }),
            );
            let key = Key::new("test", "one");
            kernel
                .routes
                .insert(key.clone(), BTreeSet::from(["in".to_owned()]));
            kernel.producers.get_mut().unwrap().producers.push((
                key,
                Some(Box::new(Once(false))),
                None,
            ));
            kernel
        });
        assert_eq!(selections.load(Ordering::SeqCst), 0);
        let events = kernel.drain(1);
        assert_eq!(events.len(), 1);
        assert!(events[0].result.is_ok());
        assert!(selections.load(Ordering::SeqCst) > 0);
    }

    #[test]
    fn producer_wake_registration_may_inspect_inventory() {
        use std::sync::Weak;

        struct WakeProbe(Weak<Kernel>);
        impl Producer for WakeProbe {
            fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
                Ok(None)
            }
            fn set_waker(&mut self, _: WakeHandle) -> bool {
                let kernel = self.0.upgrade().unwrap();
                let state = kernel
                    .producers
                    .try_lock()
                    .expect("producer inventory locked during wake registration");
                assert_eq!(state.producers.len(), 1);
                assert!(state.producers[0].1.is_none());
                true
            }
        }

        let mut kernel = Kernel::build(
            "reentrant-waker",
            [],
            Arc::new(phenix_graph::process::LocalProcessDispatcher),
        )
        .unwrap();
        kernel.start().unwrap();
        let kernel = Arc::new(kernel);
        kernel.producers.lock().unwrap().producers.push((
            Key::new("test", "waker"),
            Some(Box::new(WakeProbe(Arc::downgrade(&kernel)))),
            None,
        ));
        let wake = WakeHandle::default();
        wake.shutdown();
        kernel.run_until(&wake, |_| {}).unwrap();
        assert!(kernel.producers.lock().unwrap().producers[0].1.is_some());
    }

    #[test]
    fn producer_poll_may_inspect_inventory_without_a_mutex_deadlock() {
        use std::sync::{
            Weak,
            atomic::{AtomicUsize, Ordering},
        };

        struct Probe(Weak<Kernel>, Arc<AtomicUsize>);
        impl Producer for Probe {
            fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
                let kernel = self.0.upgrade().unwrap();
                let state = kernel
                    .producers
                    .try_lock()
                    .expect("producer inventory was locked during poll callback");
                assert_eq!(state.producers.len(), 1);
                // The in-flight producer retains an inventory slot but cannot
                // be borrowed by another caller during its exclusive poll.
                assert!(state.producers[0].1.is_none());
                self.1.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }
        }

        let mut kernel = Kernel::build(
            "reentrant-producer",
            [],
            Arc::new(phenix_graph::process::LocalProcessDispatcher),
        )
        .unwrap();
        kernel.start().unwrap();
        let kernel = Arc::new(kernel);
        let calls = Arc::new(AtomicUsize::new(0));
        kernel.producers.lock().unwrap().producers.push((
            Key::new("test", "probe"),
            Some(Box::new(Probe(Arc::downgrade(&kernel), calls.clone()))),
            None,
        ));
        assert!(kernel.drain(3).is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(kernel.producers.lock().unwrap().producers.len(), 1);
    }

    #[test]
    fn panic_in_one_producer_retires_only_that_source() {
        struct Panics;
        impl Producer for Panics {
            fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
                panic!("probe producer panic");
            }
        }
        struct Idle;
        impl Producer for Idle {
            fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
                Ok(None)
            }
        }
        let mut kernel = Kernel::build(
            "panic-retirement",
            [],
            Arc::new(phenix_graph::process::LocalProcessDispatcher),
        )
        .unwrap();
        kernel.start().unwrap();
        kernel.producers.get_mut().unwrap().producers = vec![
            (Key::new("test", "broken"), Some(Box::new(Panics)), None),
            (Key::new("test", "live"), Some(Box::new(Idle)), None),
        ];
        let events = kernel.drain(3);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].producer.id, "broken");
        assert!(matches!(events[0].result, Err(ProducerError::Poll(_))));
        let state = kernel.producers.get_mut().unwrap();
        assert_eq!(state.producers.len(), 1);
        assert_eq!(state.producers[0].0.id, "live");
        assert!(state.producers[0].1.is_some());
    }

    #[test]
    fn terminal_producer_destructors_reenter_after_all_failures_are_detached() {
        use std::sync::{
            Weak,
            atomic::{AtomicUsize, Ordering},
        };

        struct Source(Weak<Kernel>, Arc<AtomicUsize>);
        impl Producer for Source {
            fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
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

        let mut kernel = Kernel::build(
            "reentrant",
            [],
            Arc::new(phenix_graph::process::LocalProcessDispatcher),
        )
        .unwrap();
        kernel.start().unwrap();
        let kernel = Arc::new(kernel);
        let drops = Arc::new(AtomicUsize::new(0));
        kernel.producers.lock().unwrap().producers = ["a", "b"]
            .into_iter()
            .map(|name| {
                (
                    Key::new("test", name),
                    Some(Box::new(Source(Arc::downgrade(&kernel), drops.clone()))
                        as Box<dyn Producer>),
                    None,
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
            fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
                self.1.lock().unwrap().push(self.0);
                if self.0 == "broken" {
                    Err("terminal".into())
                } else {
                    Ok(None)
                }
            }
        }
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut kernel = Kernel::build(
            "fair",
            [],
            Arc::new(phenix_graph::process::LocalProcessDispatcher),
        )
        .unwrap();
        kernel.start().unwrap();
        kernel.producers.get_mut().unwrap().producers = ["a", "broken", "b"]
            .into_iter()
            .map(|name| {
                (
                    Key::new("test", name),
                    Some(Box::new(Source(name, log.clone())) as Box<dyn Producer>),
                    None,
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
