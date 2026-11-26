//! Opaque composition provenance, separate from contracts and authorization.
use std::sync::Arc;

/// Allocation identity for one host-selected composition generation.
/// Cloning preserves identity; constructing another token never does. This is
/// provenance supplied by a trusted host, not a sandbox or permission grant.
#[derive(Clone, Debug)]
pub struct GraphOrigin(OriginIdentity);

#[derive(Clone, Debug)]
enum OriginIdentity {
    Root(Arc<()>),
    Child(Arc<GraphOrigin>),
}

impl Default for GraphOrigin {
    fn default() -> Self {
        Self::from_root(Arc::new(()))
    }
}

impl GraphOrigin {
    /// Associate a root with a host-owned identity. Reusing the exact token
    /// preserves identity, without retaining executable code or granting authority.
    pub fn from_root(identity: Arc<()>) -> Self {
        Self(OriginIdentity::Root(identity))
    }

    /// Create a distinct composition identity retaining this origin's ancestry.
    pub fn child(&self) -> Self {
        Self(OriginIdentity::Child(Arc::new(self.clone())))
    }

    /// Whether this is the given origin or one of its descendants.
    /// This verifies provenance ancestry, not permission or exact producer identity.
    pub fn belongs_to(&self, parent: &Self) -> bool {
        let mut origin = self;
        loop {
            if origin == parent {
                return true;
            }
            match &origin.0 {
                OriginIdentity::Child(ancestor) => origin = ancestor,
                OriginIdentity::Root(_) => return false,
            }
        }
    }
}

impl PartialEq for GraphOrigin {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (OriginIdentity::Root(a), OriginIdentity::Root(b)) => Arc::ptr_eq(a, b),
            (OriginIdentity::Child(a), OriginIdentity::Child(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
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
        let token = Arc::new(());
        let root = GraphOrigin::from_root(token.clone());
        let same = GraphOrigin::from_root(token);
        assert_eq!(root, same);
        assert!(root.child().belongs_to(&same));
        assert!(revision.belongs_to(&revision));
        assert!(first.belongs_to(&revision));
        assert!(nested.belongs_to(&revision));
        assert!(nested.belongs_to(&first));
        assert!(!revision.belongs_to(&first));
        assert!(!first.belongs_to(&sibling));
        assert!(!first.belongs_to(&GraphOrigin::default()));
    }
}
