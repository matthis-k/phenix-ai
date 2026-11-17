//! Opaque application capabilities. Lookup neither invokes the capability nor
//! supplies storage, locking, permission checks, or scheduling authority.
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
