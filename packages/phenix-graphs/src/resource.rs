//! Opaque application capabilities. Lookup neither invokes the capability nor
//! supplies storage, locking, permission checks, or scheduling authority.
//!
//! ## Resource Capabilities
//!
//! Graph resolves opaque capabilities supplied by the host. It provides no store,
//! resource locks, read/write callbacks, or permission engine. A capability's public
//! API defines its authority: an application-private read facade must not expose its
//! inner mutable handle or a conversion to a writer. A write facade may expose both
//! read and write operations. Real Storage handles and their authorization remain
//! outside Graph; no Graph-specific storage dependency is required.
//!
//! ```rust
//! use phenix_graph::{ProcessContext, ResourceError, ResourceGrant, ValidatedResourceSet};
//! use std::sync::Arc;
//! let capability = Arc::new(String::from("application-owned capability"));
//! let resources = ValidatedResourceSet::try_new(vec![
//!     ResourceGrant::new("service", capability.clone()),
//! ])?;
//! let context = ProcessContext::default().with_resources(resources);
//! assert!(Arc::ptr_eq(context.resource::<Arc<String>>("service")?, &capability));
//! assert!(matches!(context.resource::<String>("service"), Err(ResourceError::WrongType("service"))));
//! assert!(matches!(context.resource::<String>("absent"), Err(ResourceError::NotGranted("absent"))));
//! # Ok::<(), ResourceError>(())
//! ```
//!
//! `ResourceGrant::new(key, capability)` preserves the exact Rust `Any` type, including
//! an explicit `Arc<C>` wrapper. Lookup returns `&C` and performs no capability
//! operation. `ValidatedResourceSet::try_new` rejects duplicate keys regardless of
//! type or order, before a set can be used in a context or session. Rejection consumes
//! the input and may execute application destructors. Empty/default sets are valid;
//! clones preserve validation and share the granted capabilities. The same key is
//! valid in different processes, without implying shared state. Configure each
//! physical path through `SessionOptions::resources`; auto-started sources, explicit
//! request sources, and ordinary reservations receive clones of that process's set.
//! Recovery replaces process-private state, not these external capabilities or effects.
//!
//! `with_lease` appends a capability-specific code/resource lease. Each grant keeps
//! all its leases alive through capability destruction and destruction of a caught
//! panic payload, then attempts each lease drop separately. Direct grant/set drops,
//! rejected sets, and retained session sets use the same cleanup. Managed context
//! cleanup reports caught destructor panics through the existing handoff failure
//! path; ordinary `Drop` cannot report them. This is not a sandbox or a guarantee
//! against all in-process aborts, non-yielding destructors, or double panics inside
//! one application's own destructor. Resources do not schedule work or roll back
//! committed external effects.
//!
//! The executable `resource_capabilities` example keeps the facade's lock private.
//! Its exact source is compile-checked from outside the facade module: a reader
//! cannot call `write`, access the inner lock, or convert into a writer. The writer
//! can both read and write the same external state.
//!
use std::any::Any;
use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceError {
    Duplicate(&'static str),
    NotGranted(&'static str),
    WrongType(&'static str),
}
impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duplicate(key) => write!(f, "duplicate resource grant: {key}"),
            Self::NotGranted(key) => write!(f, "resource not granted: {key}"),
            Self::WrongType(key) => write!(f, "resource has wrong Rust type: {key}"),
        }
    }
}
impl std::error::Error for ResourceError {}

/// A capability of exactly `C`, including any explicit `Arc<C>` wrapper.
/// The host, not Graph, chooses which operations that capability exposes.
#[derive(Clone)]
pub struct ResourceGrant {
    key: &'static str,
    capability: Option<Arc<dyn Any + Send + Sync>>,
    leases: Vec<Arc<dyn Send + Sync>>,
}
impl ResourceGrant {
    pub fn new<C: Any + Send + Sync>(key: &'static str, capability: C) -> Self {
        Self {
            key,
            capability: Some(Arc::new(capability)),
            leases: Vec::new(),
        }
    }

    /// Append a code/resource lease; all leases outlive capability cleanup,
    /// including destruction of a caught panic payload.
    pub fn with_lease(mut self, lease: Arc<dyn Send + Sync>) -> Self {
        self.leases.push(lease);
        self
    }

    fn cleanup(&mut self) -> bool {
        let mut panicked = drop_catching(self.capability.take());
        while let Some(lease) = self.leases.pop() {
            panicked |= drop_catching(lease);
        }
        panicked
    }

    fn dispose_catching(mut self) -> bool {
        self.cleanup()
    }
}
impl Drop for ResourceGrant {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
impl fmt::Debug for ResourceGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResourceGrant")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

/// Unique resource keys for one physical process. Cloning shares capabilities,
/// not application state isolation. Empty sets need no validation.
#[derive(Clone, Debug, Default)]
pub struct ValidatedResourceSet(Vec<ResourceGrant>);
impl ValidatedResourceSet {
    pub(crate) fn known_leases(&self) -> impl Iterator<Item = &Arc<dyn Send + Sync>> {
        self.0.iter().flat_map(|grant| &grant.leases)
    }

    /// Reject duplicate keys before the set can reach a process or session.
    /// Rejection consumes the grants and can run application destructors.
    pub fn try_new(grants: Vec<ResourceGrant>) -> Result<Self, ResourceError> {
        // Wrap first so rejection never unwinds through a raw Vec of grants.
        let set = Self(grants);
        let mut keys = BTreeSet::new();
        for grant in &set.0 {
            if !keys.insert(grant.key) {
                return Err(ResourceError::Duplicate(grant.key));
            }
        }
        Ok(set)
    }

    pub(crate) fn resource<C: Any + Send + Sync>(
        &self,
        key: &'static str,
    ) -> Result<&C, ResourceError> {
        let grant = self
            .0
            .iter()
            .find(|grant| grant.key == key)
            .ok_or(ResourceError::NotGranted(key))?;
        grant
            .capability
            .as_ref()
            .and_then(|capability| capability.downcast_ref())
            .ok_or(ResourceError::WrongType(key))
    }

    fn cleanup(&mut self) -> bool {
        let mut panicked = false;
        for grant in self.0.drain(..) {
            panicked |= grant.dispose_catching();
        }
        panicked
    }

    pub(crate) fn dispose_catching(mut self) -> bool {
        self.cleanup()
    }
}
impl Drop for ValidatedResourceSet {
    fn drop(&mut self) {
        let _ = self.cleanup();
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
