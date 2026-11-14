//! Host-authorized native network composition and persistent producer lifecycle.
mod exclusive;
pub mod host;

use exclusive::{ExclusiveLease, ExclusiveSlot};
use phenix_graph::{
    Value,
    compose::{Composer, Footprint, NetworkPatch, Registration},
    contract::Contract,
    network::{BoundComponent, CompiledNetwork, NetworkBuilder, NetworkError},
    process::{ProcessDispatcher, ProcessExecutionGuard},
    runtime::{InjectionError, NetworkSession, RunReport, SessionOptions, SessionState},
};
pub use phenix_plugin::CallbackError;
use phenix_plugin::{ContributionHandle, PluginSnapshot, lease_callback_error};
use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    ops::{Deref, DerefMut},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

thread_local! {
    static ACTIVE_SESSION_KERNELS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}
static NEXT_KERNEL_SESSION_ID: AtomicUsize = AtomicUsize::new(1);

fn next_kernel_session_id() -> usize {
    NEXT_KERNEL_SESSION_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .expect("kernel session identity exhausted")
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

/// A producer must not wait for an event. Poll/wake callbacks run without the
/// kernel producer mutex; recursive or concurrent drain returns no events.
/// Drop must stop and release resources. Factories clean partial activation.
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

pub struct CandidateContext<'a> {
    pub compiled: &'a CompiledNetwork,
    imports: &'a BTreeMap<String, ResolvedImport>,
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
    /// metadata so endpoint projection need not hold a borrow of the context.
    pub fn import(&self, id: &str) -> Result<ResolvedImport, CallbackError> {
        self.imports
            .get(id)
            .cloned()
            .ok_or_else(|| format!("undeclared import: {id}").into())
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
    ProducerBusy,
    ReentrantSessionAccess,
    UnsupportedWake {
        producer: Key,
    },
    WakePanicked {
        producer: Key,
        message: String,
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
            Self::ProducerBusy => f.write_str("producer callback is already in progress"),
            Self::ReentrantSessionAccess => {
                f.write_str("reentrant access to the same kernel session")
            }
            Self::UnsupportedWake { producer } => write!(
                f,
                "producer does not support wake notifications: {producer:?}"
            ),
            Self::WakePanicked { producer, message } => {
                write!(
                    f,
                    "producer wake registration panicked: {producer:?}: {message}"
                )
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

#[derive(Default)]
struct ProducerState {
    producers: Vec<ProducerSlot>,
    next: usize,
    retired: bool,
    callback_active: bool,
}

struct ProducerSlot {
    key: Key,
    producer: Option<Box<dyn Producer>>,
    pending: Option<InputEvent>,
}

struct ProducerCallbackLease<'a> {
    state: &'a Mutex<ProducerState>,
}

/// Admission won the retirement boundary before invoking session callbacks.
/// Retirement never waits for a permit already issued.
struct ProducerAdmissionPermit;

impl Drop for ProducerCallbackLease<'_> {
    fn drop(&mut self) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .callback_active = false;
    }
}

struct SessionLease<'a> {
    lease: ExclusiveLease<'a, NetworkSession>,
    notify_on_drop: bool,
}

impl SessionLease<'_> {
    fn notify_on_drop(&mut self) {
        self.notify_on_drop = true;
    }
}

impl Deref for SessionLease<'_> {
    type Target = NetworkSession;

    fn deref(&self) -> &Self::Target {
        &self.lease
    }
}

impl DerefMut for SessionLease<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.lease
    }
}

impl Drop for SessionLease<'_> {
    fn drop(&mut self) {
        let notifier = self.notify_on_drop.then(|| self.lease.notifier());
        self.lease.release();
        if let Some(notifier) = notifier {
            notifier.notify();
        }
    }
}

struct ActiveSessionCall {
    identity: usize,
    registered: bool,
}

impl ActiveSessionCall {
    fn enter(identity: usize) -> Result<Self, ()> {
        ACTIVE_SESSION_KERNELS.with(|active| {
            let mut active = active.borrow_mut();
            if active.contains(&identity) {
                Err(())
            } else {
                active.push(identity);
                Ok(Self {
                    identity,
                    registered: true,
                })
            }
        })
    }

    fn inherit(identity: usize) -> Self {
        ACTIVE_SESSION_KERNELS.with(|active| {
            let mut active = active.borrow_mut();
            if active.contains(&identity) {
                Self {
                    identity,
                    registered: false,
                }
            } else {
                active.push(identity);
                Self {
                    identity,
                    registered: true,
                }
            }
        })
    }
}

impl Drop for ActiveSessionCall {
    fn drop(&mut self) {
        if !self.registered {
            return;
        }
        ACTIVE_SESSION_KERNELS.with(|active| {
            let mut active = active.borrow_mut();
            let index = active
                .iter()
                .rposition(|identity| *identity == self.identity)
                .expect("active session call is registered");
            active.remove(index);
        });
    }
}

struct KernelExecutionGuard {
    identity: usize,
}

impl ProcessExecutionGuard for KernelExecutionGuard {
    fn enter(&self) -> Result<Box<dyn Any>, String> {
        Ok(Box::new(ActiveSessionCall::inherit(self.identity)))
    }
}

fn discard_panic_payload(payload: Box<dyn Any + Send>) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload))) {
        // A hostile panic payload can itself panic from Drop. Never let that bypass
        // producer reconciliation or trigger a double panic while containing it.
        std::mem::forget(payload);
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_owned()
    } else {
        "non-string panic payload".into()
    }
}

fn contained_panic_message(context: &str, payload: Box<dyn Any + Send>) -> String {
    let message = format!("{context}: {}", panic_message(payload.as_ref()));
    discard_panic_payload(payload);
    message
}

fn poll_panic_error(payload: Box<dyn Any + Send>) -> CallbackError {
    contained_panic_message("producer poll panicked", payload).into()
}

fn wake_panic_message(payload: Box<dyn Any + Send>) -> String {
    let message = panic_message(payload.as_ref());
    discard_panic_payload(payload);
    message
}

pub struct Kernel {
    // The session and producer mutexes protect ownership transfer only. Plugin
    // callbacks run with the corresponding state leased out of the mutex.
    session: ExclusiveSlot<NetworkSession>,
    session_identity: usize,
    compiled: CompiledNetwork,
    producers: Mutex<ProducerState>,
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
                Some(prepare) => prepare(&CandidateContext {
                    compiled: &context_compiled,
                    imports: imports.get(key).unwrap_or(&empty_imports),
                })
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
        let session_identity = next_kernel_session_id();
        options.execution.execution_guards.insert(
            0,
            Arc::new(KernelExecutionGuard {
                identity: session_identity,
            }),
        );
        let session = compiled.start_with_options(options.execution);
        if let Some(failure) = session.failure() {
            return Err(ResolutionError::Invalid(failure.into()));
        }
        Ok(Self {
            session: ExclusiveSlot::new(session),
            session_identity,
            compiled,
            producers: Mutex::new(ProducerState::default()),
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
                let producer =
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(activate)) {
                        Ok(Ok(producer)) => producer,
                        Ok(Err(cause)) => {
                            return Err(StartError::Activation {
                                contribution: key.clone(),
                                cause: lease_callback_error(cause, self.code_lease.clone()),
                            });
                        }
                        Err(payload) => {
                            let cause =
                                contained_panic_message("producer activation panicked", payload);
                            return Err(StartError::Activation {
                                contribution: key.clone(),
                                cause: lease_callback_error(cause.into(), self.code_lease.clone()),
                            });
                        }
                    };
                producers.push(ProducerSlot {
                    key: key.clone(),
                    producer: Some(producer),
                    pending: None,
                });
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

    fn lease_session(&self) -> SessionLease<'_> {
        SessionLease {
            lease: self.session.take(),
            notify_on_drop: false,
        }
    }

    fn try_lease_session(&self) -> Option<SessionLease<'_>> {
        self.session.try_take().map(|lease| SessionLease {
            lease,
            notify_on_drop: false,
        })
    }

    /// Run an exclusive session operation without holding the kernel mutex.
    /// Same-thread re-entry through this Kernel returns an error instead of waiting
    /// on its own session lease.
    pub fn with_session<R>(
        &self,
        use_session: impl FnOnce(&mut NetworkSession) -> R,
    ) -> Result<R, String> {
        if !self.started {
            return Err("kernel not started".into());
        }
        let _active = ActiveSessionCall::enter(self.session_identity)
            .map_err(|()| "reentrant access to the same kernel session".to_owned())?;
        let mut session = self.lease_session();
        session.notify_on_drop();
        let result = use_session(&mut session);
        Ok(result)
    }

    /// Inject one value. Reentrant same-thread session access returns the value
    /// with `ConnectionError::ReentrantSessionAccess`.
    pub fn inject(&self, input: &str, value: Value) -> Result<(), InjectionError> {
        if !self.started {
            return Err(InjectionError {
                reason: phenix_graph::connection::ConnectionError::Cancelled,
                value,
            });
        }
        let _active = match ActiveSessionCall::enter(self.session_identity) {
            Ok(active) => active,
            Err(()) => {
                return Err(InjectionError {
                    reason: phenix_graph::connection::ConnectionError::ReentrantSessionAccess,
                    value,
                });
            }
        };
        let mut session = self.lease_session();
        let result = session.inject(input, value);
        if result.is_ok() {
            session.notify_on_drop();
        }
        result
    }

    /// Drive a session budget with no kernel mutex held across dispatcher/process
    /// callbacks. Same-thread re-entry through this Kernel fails fast.
    pub fn run_steps(&self, budget: usize) -> Result<RunReport, String> {
        if !self.started {
            return Err("kernel not started".into());
        }
        let _active = ActiveSessionCall::enter(self.session_identity)
            .map_err(|()| "reentrant access to the same kernel session".to_owned())?;
        Ok(self.lease_session().run_until_idle(budget))
    }

    /// Perform at most `max_polls` nonblocking producer polls, round-robin across
    /// calls. Idle polls count toward the bound. Poll errors retire only the failed
    /// producer; admission errors do not stop polling.
    /// A full/busy input retains one event without repolling. Plugin polling runs
    /// outside kernel mutexes; an in-flight callback that returns after retirement
    /// cannot admit its event.
    pub fn drain(&self, max_polls: usize) -> Vec<ProducerEvent> {
        if max_polls == 0 {
            return Vec::new();
        }
        let callback_lease = {
            let mut state = self
                .producers
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if state.retired || state.callback_active {
                return Vec::new();
            }
            state.callback_active = true;
            ProducerCallbackLease {
                state: &self.producers,
            }
        };
        let mut events = Vec::new();
        let mut detached = Vec::new();
        for _ in 0..max_polls {
            let selected = {
                let mut state = self
                    .producers
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if state.retired || state.producers.is_empty() {
                    None
                } else {
                    let len = state.producers.len();
                    let index = state.next;
                    state.next = (index + 1) % len;
                    let slot = &mut state.producers[index];
                    Some((
                        slot.key.clone(),
                        slot.producer
                            .take()
                            .expect("producer callback lease is exclusive"),
                        slot.pending.take(),
                    ))
                }
            };
            let Some((key, producer, pending)) = selected else {
                break;
            };
            let mut producer = Some(producer);

            let polled = match pending {
                Some(event) => Ok(Some(event)),
                None => match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    producer
                        .as_mut()
                        .expect("active producer is present")
                        .poll()
                })) {
                    Ok(result) => result,
                    Err(payload) => Err(poll_panic_error(payload)),
                },
            };
            let active = {
                let state = self
                    .producers
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                !state.retired
                    && state
                        .producers
                        .iter()
                        .any(|slot| slot.key == key && slot.producer.is_none())
            };
            if !active {
                drop(producer);
                drop(polled);
                break;
            }

            let mut result = None;
            let mut pending = None;
            let mut terminal = false;
            let mut admission_claimed = false;
            match polled {
                Err(error) => {
                    result = Some(Err(ProducerError::Poll(lease_callback_error(
                        error,
                        self.code_lease.clone(),
                    ))));
                    terminal = true;
                }
                Ok(None) => {}
                Ok(Some(mut event)) => {
                    event.value = event.value.with_lease(self.code_lease.clone());
                    let Some(_admission_permit) = self.claim_producer_admission(&key) else {
                        drop(event);
                        drop(producer);
                        break;
                    };
                    admission_claimed = true;
                    if !self.routes[&key].contains(&event.input) {
                        result = Some(Err(ProducerError::UndeclaredRoute(event.input)));
                    } else if let Some(mut session) = self.try_lease_session() {
                        if let Ok(_active) = ActiveSessionCall::enter(self.session_identity) {
                            match session.inject(&event.input, event.value) {
                                Ok(()) => {
                                    session.notify_on_drop();
                                    result = Some(Ok(()));
                                }
                                Err(error)
                                    if error.reason
                                        == phenix_graph::connection::ConnectionError::Full =>
                                {
                                    pending = Some(InputEvent {
                                        input: event.input,
                                        value: error.value,
                                    });
                                }
                                Err(error) => {
                                    result = Some(Err(ProducerError::Admission(error)));
                                }
                            }
                        } else {
                            pending = Some(event);
                        }
                    } else {
                        pending = Some(event);
                    }
                }
            }

            let mut retired = false;
            let mut removed = None;
            {
                let mut state = self
                    .producers
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let index = state
                    .producers
                    .iter()
                    .position(|slot| slot.key == key && slot.producer.is_none());
                if state.retired {
                    retired = true;
                } else if let Some(index) = index {
                    if terminal {
                        removed = Some(state.producers.remove(index));
                        if state.producers.is_empty() || index >= state.producers.len() {
                            state.next = 0;
                        } else {
                            state.next = index;
                        }
                    } else {
                        state.producers[index].pending = pending.take();
                        state.producers[index].producer =
                            Some(producer.take().expect("active producer is restored once"));
                    }
                } else {
                    retired = true;
                }
            }
            drop(removed);
            if retired {
                drop(producer);
                drop(pending);
                if admission_claimed && let Some(result) = result.take() {
                    events.push(ProducerEvent {
                        producer: key,
                        result,
                    });
                }
                break;
            }
            if terminal {
                detached.push(producer.take().expect("terminal producer is detached once"));
            }
            drop(producer);
            if let Some(result) = result {
                events.push(ProducerEvent {
                    producer: key,
                    result,
                });
            }
        }
        drop(detached);
        drop(callback_lease);
        events
    }

    fn claim_producer_admission(&self, key: &Key) -> Option<ProducerAdmissionPermit> {
        let state = self
            .producers
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        (!state.retired
            && state
                .producers
                .iter()
                .any(|slot| &slot.key == key && slot.producer.is_none()))
        .then_some(ProducerAdmissionPermit)
    }

    fn register_wakers(&self, wake: &WakeHandle) -> Result<bool, DriverError> {
        let count = {
            let mut state = self
                .producers
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if state.retired {
                return Ok(false);
            }
            if state.callback_active {
                return Err(DriverError::ProducerBusy);
            }
            state.callback_active = true;
            state.producers.len()
        };
        let _callback_lease = ProducerCallbackLease {
            state: &self.producers,
        };
        for index in 0..count {
            let (key, mut producer) = {
                let mut state = self
                    .producers
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if state.retired {
                    return Ok(false);
                }
                let slot = &mut state.producers[index];
                let Some(producer) = slot.producer.take() else {
                    continue;
                };
                (slot.key.clone(), producer)
            };
            let registered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                producer.set_waker(wake.clone())
            }));
            let outcome = match registered {
                Ok(registered) => Ok(registered),
                Err(payload) => Err(wake_panic_message(payload)),
            };
            let mut state = self
                .producers
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if state.retired {
                drop(state);
                drop(producer);
                return Ok(false);
            }
            state.producers[index].producer = Some(producer);
            drop(state);
            match outcome {
                Ok(true) => {}
                Ok(false) => return Err(DriverError::UnsupportedWake { producer: key }),
                Err(message) => {
                    return Err(DriverError::WakePanicked {
                        producer: key,
                        message,
                    });
                }
            }
        }
        Ok(true)
    }

    /// Drive a started kernel until shutdown, delivering events without retaining reports.
    /// Every producer must support wake registration. Shutdown is checked between
    /// bounded driver iterations; it cannot preempt a running step or callback.
    /// Wake registration runs without the producer mutex and contains panics. A
    /// concurrent producer callback returns `DriverError::ProducerBusy`.
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
            .map_err(|_| DriverError::ReentrantSessionAccess)?;
        *wake.0.notifier.lock().unwrap() = Some(notifier);
        if !self.register_wakers(wake)? {
            return Ok(());
        }
        loop {
            {
                if *wake.0.shutdown.lock().unwrap() {
                    return Ok(());
                }
            }
            let mut active = false;
            let count = {
                let state = self
                    .producers
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if state.retired {
                    return Ok(());
                }
                state.producers.len()
            };
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
            active |= report.steps > 0
                || report.publications > 0
                || report.state == SessionState::Runnable;
            if !active {
                let timeout = report
                    .next_deferred_timeout
                    .unwrap_or(std::time::Duration::from_secs(60))
                    .min(std::time::Duration::from_secs(60));
                listener.wait_timeout(timeout);
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
            .map(|name| ProducerSlot {
                key: Key::new("test", name),
                producer: Some(
                    Box::new(Source(Arc::downgrade(&kernel), drops.clone())) as Box<dyn Producer>
                ),
                pending: None,
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
        kernel.producers.get_mut().unwrap().producers = ["a", "b", "broken"]
            .into_iter()
            .map(|name| ProducerSlot {
                key: Key::new("test", name),
                producer: Some(Box::new(Source(name, log.clone())) as Box<dyn Producer>),
                pending: None,
            })
            .collect();
        assert!(kernel.drain(0).is_empty());
        assert!(log.lock().unwrap().is_empty());
        assert!(kernel.drain(1).is_empty());
        assert!(kernel.drain(1).is_empty());
        let events = kernel.drain(1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].producer, Key::new("test", "broken"));
        assert!(kernel.drain(3).is_empty());
        assert_eq!(*log.lock().unwrap(), ["a", "b", "broken", "a", "b", "a"]);
    }

    #[test]
    fn producer_callbacks_run_without_the_producer_mutex_and_reentry_is_bounded() {
        use std::sync::{
            Weak,
            atomic::{AtomicUsize, Ordering},
        };

        struct Source(Weak<Kernel>, Arc<AtomicUsize>);
        impl Producer for Source {
            fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
                let kernel = self.0.upgrade().unwrap();
                assert!(kernel.producers.try_lock().is_ok());
                assert!(kernel.session.try_take().is_some());
                assert!(kernel.drain(1).is_empty());
                self.1.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }

            fn set_waker(&mut self, wake: WakeHandle) -> bool {
                let kernel = self.0.upgrade().unwrap();
                assert!(kernel.producers.try_lock().is_ok());
                assert!(kernel.session.try_take().is_some());
                assert!(kernel.drain(1).is_empty());
                wake.shutdown();
                true
            }
        }

        let mut kernel = Kernel::build(
            "callback-lock",
            [],
            Arc::new(phenix_graph::process::LocalProcessDispatcher),
        )
        .unwrap();
        kernel.start().unwrap();
        let kernel = Arc::new(kernel);
        let polls = Arc::new(AtomicUsize::new(0));
        kernel
            .producers
            .lock()
            .unwrap()
            .producers
            .push(ProducerSlot {
                key: Key::new("test", "source"),
                producer: Some(Box::new(Source(Arc::downgrade(&kernel), polls.clone()))),
                pending: None,
            });

        assert!(kernel.drain(1).is_empty());
        assert_eq!(polls.load(Ordering::SeqCst), 1);
        kernel.run_until(&WakeHandle::default(), |_| {}).unwrap();
        assert!(!kernel.producers.is_poisoned());
    }

    #[test]
    fn producer_panics_are_contained_without_poisoning_kernel_state() {
        struct Panicking;
        impl Producer for Panicking {
            fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
                panic!("poll panic")
            }
        }

        let mut kernel = Kernel::build(
            "callback-panic",
            [],
            Arc::new(phenix_graph::process::LocalProcessDispatcher),
        )
        .unwrap();
        kernel.start().unwrap();
        kernel
            .producers
            .get_mut()
            .unwrap()
            .producers
            .push(ProducerSlot {
                key: Key::new("test", "panic"),
                producer: Some(Box::new(Panicking)),
                pending: None,
            });

        let events = kernel.drain(1);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].result, Err(ProducerError::Poll(_))));
        assert!(kernel.drain(1).is_empty());
        assert!(!kernel.producers.is_poisoned());
    }

    #[test]
    fn panicking_panic_payload_destructors_do_not_bypass_producer_reconciliation() {
        struct PanickingPayload;
        impl Drop for PanickingPayload {
            fn drop(&mut self) {
                panic!("panic payload destructor panic");
            }
        }
        struct Panicking;
        impl Producer for Panicking {
            fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
                std::panic::panic_any(PanickingPayload)
            }
        }

        let mut kernel = Kernel::build(
            "panic-payload-drop",
            [],
            Arc::new(phenix_graph::process::LocalProcessDispatcher),
        )
        .unwrap();
        kernel.start().unwrap();
        kernel
            .producers
            .get_mut()
            .unwrap()
            .producers
            .push(ProducerSlot {
                key: Key::new("test", "panic-payload"),
                producer: Some(Box::new(Panicking)),
                pending: None,
            });

        let events = kernel.drain(1);
        assert!(matches!(
            events.as_slice()[0].result,
            Err(ProducerError::Poll(_))
        ));
        assert!(kernel.drain(1).is_empty());
        assert!(!kernel.producers.is_poisoned());
    }

    #[test]
    fn wake_registration_panics_are_reported_without_poisoning_kernel_state() {
        struct PanickingPayload;
        impl Drop for PanickingPayload {
            fn drop(&mut self) {
                panic!("wake panic payload destructor panic");
            }
        }
        struct Panicking;
        impl Producer for Panicking {
            fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
                Ok(None)
            }

            fn set_waker(&mut self, _: WakeHandle) -> bool {
                std::panic::panic_any(PanickingPayload)
            }
        }

        let mut kernel = Kernel::build(
            "wake-callback-panic",
            [],
            Arc::new(phenix_graph::process::LocalProcessDispatcher),
        )
        .unwrap();
        kernel.start().unwrap();
        kernel
            .producers
            .get_mut()
            .unwrap()
            .producers
            .push(ProducerSlot {
                key: Key::new("test", "panic"),
                producer: Some(Box::new(Panicking)),
                pending: None,
            });

        assert!(matches!(
            kernel.run_until(&WakeHandle::default(), |_| {}),
            Err(DriverError::WakePanicked { message, .. })
                if message == "non-string panic payload"
        ));
        assert!(!kernel.producers.is_poisoned());
        assert!(kernel.drain(1).is_empty());
    }

    #[test]
    fn same_thread_session_reentry_fails_fast_and_preserves_rejected_input() {
        let mut kernel = Kernel::build(
            "session-reentry",
            [],
            Arc::new(phenix_graph::process::LocalProcessDispatcher),
        )
        .unwrap();
        kernel.start().unwrap();
        let kernel = Arc::new(kernel);

        let nested = kernel
            .with_session(|_| {
                assert!(kernel.session.lock_is_available());
                kernel.run_steps(1)
            })
            .unwrap()
            .unwrap_err();
        assert!(nested.contains("reentrant access"));
        let error = kernel
            .with_session(|_| kernel.inject("input", Value::local(41_i64)))
            .unwrap()
            .unwrap_err();
        assert_eq!(
            error.reason,
            phenix_graph::connection::ConnectionError::ReentrantSessionAccess
        );
        assert_eq!(error.value.borrow::<i64>(), Some(&41));
    }

    #[test]
    fn panicking_session_callback_restores_exclusive_ownership() {
        let mut kernel = Kernel::build(
            "session-panic",
            [],
            Arc::new(phenix_graph::process::LocalProcessDispatcher),
        )
        .unwrap();
        kernel.start().unwrap();

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            kernel
                .with_session(|_| panic!("session callback panic"))
                .unwrap();
        }));
        assert!(panic.is_err());
        assert!(kernel.session.try_take().is_some());
        assert!(kernel.run_steps(0).is_ok());
    }
}
