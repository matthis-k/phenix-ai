//! Opaque composition provenance, separate from contracts and authorization.
use std::sync::Arc;

/// Allocation identity for one host-selected composition generation.
/// Cloning preserves identity; constructing another token never does. This is
/// provenance supplied by a trusted host, not a sandbox or permission grant.
#[derive(Clone, Debug)]
pub struct GraphOrigin(Arc<OriginIdentity>);

#[derive(Debug)]
struct OriginIdentity {
    parent: Option<GraphOrigin>,
}

impl Default for GraphOrigin {
    fn default() -> Self {
        Self(Arc::new(OriginIdentity { parent: None }))
    }
}

impl GraphOrigin {
    /// Create a distinct composition identity retaining this origin's ancestry.
    pub fn child(&self) -> Self {
        Self(Arc::new(OriginIdentity {
            parent: Some(self.clone()),
        }))
    }

    /// Whether this is the given origin or one of its descendants.
    /// This verifies provenance ancestry, not permission or exact producer identity.
    pub fn belongs_to(&self, parent: &Self) -> bool {
        let mut origin = self;
        loop {
            if origin == parent {
                return true;
            }
            match &origin.0.parent {
                Some(ancestor) => origin = ancestor,
                None => return false,
            }
        }
    }
}

impl PartialEq for GraphOrigin {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for GraphOrigin {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ancestry_preserves_distinct_composition_identities() {
        let revision = GraphOrigin::default();
        let first = revision.child();
        let sibling = revision.child();
        let nested = first.child();
        assert_eq!(first, first.clone());
        assert_ne!(first, revision);
        assert_ne!(first, sibling);
        assert!(revision.belongs_to(&revision));
        assert!(first.belongs_to(&revision));
        assert!(nested.belongs_to(&revision));
        assert!(nested.belongs_to(&first));
        assert!(!revision.belongs_to(&first));
        assert!(!first.belongs_to(&sibling));
        assert!(!first.belongs_to(&GraphOrigin::default()));
    }
}
