//! Host-owned failure metadata with borrowed inspection of an original typed cause.
use crate::{TypeIdentity, Value, process::StepTicket, runtime::RequestId};
use std::{error::Error, fmt, sync::Arc};

/// Broad classification, not a substitute for an owning module's typed fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FailureKind {
    Application,
    External,
    Framework,
    Panic,
    Cleanup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CallbackPhase {
    Construct,
    RecoveryConstruct,
    FactoryReadiness,
    CorrelationBound,
    ReadinessRegistration,
    Prepare,
    ProducerActivate,
    ProducerPoll,
    GenerationBind,
    WakeRegistration,
    Event,
    Step,
    GuardEnter,
    GuardExit,
    Readiness,
    Dispatch,
    Adapter,
    Encode,
    Decode,
    Read,
    Cleanup,
}

/// Owned diagnostic metadata, never manufactured execution authority.
/// Unknown identities stay `None`; request and ticket identities come from Graph.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FailureContext {
    pub session: Option<u64>,
    pub generation: Option<u64>,
    pub request: Option<RequestId>,
    pub process: Option<usize>,
    pub path: Option<String>,
    pub ticket: Option<StepTicket>,
    pub output: Option<String>,
    pub field: Option<String>,
    pub guard_index: Option<usize>,
    pub phase: Option<CallbackPhase>,
}

/// Cheap shared ownership. Enrichment copies host data and Arcs, never the error.
/// Attach required leases before escape: enriching one handle cannot enrich an
/// earlier application clone. No raw record or cause ownership is exposed.
#[derive(Clone)]
pub struct Failure(Arc<FailureRecord>);

#[derive(Clone)]
struct FailureRecord {
    origin: Arc<()>,
    kind: FailureKind,
    summary: String,
    context: FailureContext,
    cause: Option<Arc<ArcCause>>,
    secondary: Vec<Failure>,
    leases: Vec<Arc<dyn Send + Sync>>,
}

struct ArcCause(Option<Box<dyn Error + Send + Sync>>);

impl Failure {
    pub(crate) fn retain_leases(mut self, leases: &[Arc<dyn Send + Sync>]) -> Self {
        if leases.is_empty() {
            return self;
        }
        let record = Arc::make_mut(&mut self.0);
        for lease in leases {
            if !record.leases.iter().any(|held| Arc::ptr_eq(held, lease)) {
                record.leases.push(lease.clone());
            }
        }
        for secondary in &mut record.secondary {
            *secondary = secondary.clone().retain_leases(leases);
        }
        self
    }

    /// A genuine host-owned string leaf, with no source.
    pub fn application_message(message: impl Into<String>) -> Self {
        Self(Arc::new(FailureRecord {
            origin: Arc::new(()),
            kind: FailureKind::Application,
            summary: message.into(),
            context: FailureContext::default(),
            cause: None,
            secondary: Vec::new(),
            leases: Vec::new(),
        }))
    }

    /// `summary` must be supplied by the host. No cause callbacks run here.
    pub fn external<E: Error + Send + Sync + 'static>(
        summary: impl Into<String>,
        cause: E,
    ) -> Self {
        let mut failure = Self::application_message(summary);
        let record = Arc::make_mut(&mut failure.0);
        record.kind = FailureKind::External;
        record.cause = Some(Arc::new(ArcCause(Some(Box::new(cause)))));
        failure
    }

    pub fn kind(&self) -> FailureKind {
        self.0.kind
    }

    pub fn summary(&self) -> &str {
        &self.0.summary
    }

    pub fn context(&self) -> &FailureContext {
        &self.0.context
    }

    /// Whether both handles descend from the same constructed failure, even
    /// after copy-on-write enrichment. This grants no execution authority and
    /// does not imply that their metadata, cleanup, or retained leases match.
    pub fn same_origin(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0.origin, &other.0.origin)
    }

    /// Secondary cleanup failures, separate from the original source chain.
    pub fn cleanup(&self) -> &[Failure] {
        &self.0.secondary
    }

    /// Categorize a typed fault without converting it into a string code.
    pub fn with_kind(mut self, kind: FailureKind) -> Self {
        Arc::make_mut(&mut self.0).kind = kind;
        self
    }

    /// Replace this handle's metadata, without adding a source-chain wrapper.
    pub fn with_context(mut self, context: FailureContext) -> Self {
        Arc::make_mut(&mut self.0).context = context;
        self
    }

    /// Retain code/resources on this handle and recursively on its existing
    /// cleanup handles, deduplicated per record. Leases outlive cause and caught
    /// panic-payload cleanup; previously escaped clones are not enriched.
    pub fn with_lease(self, lease: Arc<dyn Send + Sync>) -> Self {
        self.retain_leases(std::slice::from_ref(&lease))
    }

    /// Attach a secondary, recursively inheriting this handle's known leases.
    /// Previously escaped clones of either handle are not enriched.
    pub fn with_cleanup(mut self, failure: Failure) -> Self {
        let record = Arc::make_mut(&mut self.0);
        record.secondary.push(failure.retain_leases(&record.leases));
        self
    }

    /// Copy an exact local Failure and every known envelope lease, not the Value
    /// or its payload Arc. Rejection also retains all known envelope leases.
    pub fn from_value(value: &Value) -> Result<Self, Self> {
        let result = value.borrow::<Self>().cloned().ok_or_else(|| {
            Self::external(
                "expected an exact local Failure value",
                WrongFailureValue {
                    actual: value.identity(),
                },
            )
            .with_kind(FailureKind::Framework)
        });
        let retain = |failure: Self| failure.retain_leases(value.known_leases());
        result.map(retain).map_err(retain)
    }

    /// Consume this handle, reporting panics actually caught during its cleanup.
    /// `false` does not promise future shared-owner cleanup will be panic-free.
    pub fn dispose_catching(self) -> bool {
        Arc::into_inner(self.0).is_some_and(|mut record| record.dispose_catching())
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.summary())
    }
}

impl fmt::Debug for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Failure")
            .field("kind", &self.kind())
            .field("summary", &self.summary())
            .field("context", self.context())
            .field("cleanup_count", &self.0.secondary.len())
            .finish_non_exhaustive()
    }
}

impl Error for Failure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // Borrow the original concrete error, not its source or an Arc wrapper.
        self.0.cause.as_ref()?.0.as_deref().map(|cause| cause as _)
    }
}

/// The envelope was not an exact local Failure. This host-only fault never owns
/// or formats an application payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrongFailureValue {
    actual: TypeIdentity,
}

impl WrongFailureValue {
    pub fn actual(&self) -> &TypeIdentity {
        &self.actual
    }
}

impl fmt::Display for WrongFailureValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected an exact local Failure value")
    }
}

impl Error for WrongFailureValue {}

impl FailureRecord {
    fn dispose_catching(&mut self) -> bool {
        // Detach callback-bearing owners before cleanup. Every record lease stays
        // attached until each cause/secondary and its caught panic payload drops.
        let cause = self.cause.take();
        let secondary = std::mem::take(&mut self.secondary);
        let mut panicked = cause
            .and_then(Arc::into_inner)
            .is_some_and(|mut cause| cause.dispose_catching());
        for failure in secondary {
            panicked |= failure.dispose_catching();
        }
        while let Some(lease) = self.leases.pop() {
            panicked |= drop_catching(lease);
        }
        panicked
    }
}

impl Drop for FailureRecord {
    fn drop(&mut self) {
        let _ = self.dispose_catching();
    }
}

impl ArcCause {
    fn dispose_catching(&mut self) -> bool {
        drop_catching(self.0.take())
    }
}

impl Drop for ArcCause {
    fn drop(&mut self) {
        let _ = self.dispose_catching();
    }
}

fn drop_catching<T>(value: T) -> bool {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(value))) {
        crate::discard_panic_payload(payload);
        true
    } else {
        false
    }
}
