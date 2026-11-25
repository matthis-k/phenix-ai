//! Graph-independent, typed in-process contribution staging.
//! Identity is host-supplied provenance, never a permission or ABI guarantee.
use crate::{Code, Failure, identifier};
use std::{
    any::{Any, TypeId},
    collections::BTreeSet,
};

/// Captured registry validation data, available through [`Failure`]'s source chain.
#[derive(Debug, PartialEq, Eq)]
pub enum RegistryFault {
    InvalidOwner {
        owner: String,
    },
    InvalidContribution {
        owner: String,
        kind: &'static str,
        id: String,
    },
    OwnerMismatch {
        expected: String,
        actual: String,
    },
    DuplicateContribution {
        owner: String,
        kind: &'static str,
        id: String,
    },
    InvalidPlugin {
        plugin: String,
    },
    DuplicatePlugin {
        plugin: String,
    },
    ConfigurationTypeMismatch {
        plugin: String,
        expected: TypeId,
        actual: TypeId,
    },
    MissingContribution {
        owner: String,
        kind: &'static str,
        id: String,
    },
    ContributionTypeMismatch {
        owner: String,
        kind: &'static str,
        id: String,
        expected: TypeId,
        actual: TypeId,
    },
}
impl std::fmt::Display for RegistryFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for RegistryFault {}

pub trait Contribution: Any + Send + Sync {
    const KIND: &'static str;
    fn id(&self) -> &str;
    /// Logical path of the actual native component offered by this contribution.
    /// This locator is not a shape declaration, membership claim or permission.
    fn component_path(&self) -> Option<&str> {
        None
    }
}

struct Entry {
    owner: String,
    kind: &'static str,
    id: String,
    value: Box<dyn Any + Send + Sync>,
}

pub struct Registrar {
    owner: String,
    names: BTreeSet<(&'static str, String)>,
    entries: Vec<Entry>,
}
impl Registrar {
    /// Called by the trusted host with its selected package identity.
    /// Creating this value is not itself an authorization grant.
    pub fn new(owner: impl Into<String>) -> Result<Self, Failure> {
        let owner = owner.into();
        if !identifier(&owner) {
            return Err(Failure::with_source(
                Code::DescriptorInvalid,
                "invalid owner",
                Box::new(RegistryFault::InvalidOwner { owner }),
            ));
        }
        Ok(Self {
            owner,
            names: BTreeSet::new(),
            entries: Vec::new(),
        })
    }
    pub fn owner(&self) -> &str {
        &self.owner
    }
    pub fn add<C: Contribution>(&mut self, value: C) -> Result<(), Failure> {
        let id = value.id().to_owned();
        if !identifier(C::KIND) || !identifier(&id) {
            return Err(Failure::with_source(
                Code::DescriptorInvalid,
                "invalid contribution identity",
                Box::new(RegistryFault::InvalidContribution {
                    owner: self.owner.clone(),
                    kind: C::KIND,
                    id,
                }),
            ));
        }
        if !self.names.insert((C::KIND, id.clone())) {
            return Err(Failure::with_source(
                Code::DescriptorInvalid,
                "duplicate contribution",
                Box::new(RegistryFault::DuplicateContribution {
                    owner: self.owner.clone(),
                    kind: C::KIND,
                    id,
                }),
            ));
        }
        self.entries.push(Entry {
            owner: self.owner.clone(),
            kind: C::KIND,
            id,
            value: Box::new(value),
        });
        Ok(())
    }
    /// Admit a same-owner bundle atomically, including its reserved names.
    /// Only stored identities are inspected; erased payload callbacks never run.
    pub fn merge(&mut self, batch: Contributions) -> Result<(), Failure> {
        let mut names = BTreeSet::new();
        for entry in &batch.entries {
            if entry.owner != self.owner || !identifier(entry.kind) || !identifier(&entry.id) {
                let fault = if entry.owner != self.owner {
                    RegistryFault::OwnerMismatch {
                        expected: self.owner.clone(),
                        actual: entry.owner.clone(),
                    }
                } else {
                    RegistryFault::InvalidContribution {
                        owner: entry.owner.clone(),
                        kind: entry.kind,
                        id: entry.id.clone(),
                    }
                };
                return Err(Failure::with_source(
                    Code::DescriptorInvalid,
                    "invalid bundle contribution identity or owner",
                    Box::new(fault),
                ));
            }
            let name = (entry.kind, entry.id.clone());
            if self.names.contains(&name) || !names.insert(name) {
                return Err(Failure::with_source(
                    Code::DescriptorInvalid,
                    "duplicate contribution",
                    Box::new(RegistryFault::DuplicateContribution {
                        owner: entry.owner.clone(),
                        kind: entry.kind,
                        id: entry.id.clone(),
                    }),
                ));
            }
        }
        self.names.extend(names);
        self.entries.extend(batch.entries);
        Ok(())
    }
    pub fn finish(self) -> Contributions {
        let mut entries = self.entries;
        entries.sort_by(|a, b| (&a.owner, a.kind, &a.id).cmp(&(&b.owner, b.kind, &b.id)));
        Contributions { entries }
    }
}

#[derive(Default)]
pub struct Contributions {
    entries: Vec<Entry>,
}
impl Contributions {
    pub fn new() -> Self {
        Self::default()
    }
    /// Validate the entire batch before installing any member.
    pub fn merge(&mut self, batch: Self) -> Result<(), Failure> {
        let mut names: BTreeSet<(&str, &str, &str)> = self
            .entries
            .iter()
            .map(|e| (e.owner.as_str(), e.kind, e.id.as_str()))
            .collect();
        for e in &batch.entries {
            if !names.insert((&e.owner, e.kind, &e.id)) {
                return Err(Failure::with_source(
                    Code::DescriptorInvalid,
                    "duplicate selected contribution",
                    Box::new(RegistryFault::DuplicateContribution {
                        owner: e.owner.clone(),
                        kind: e.kind,
                        id: e.id.clone(),
                    }),
                ));
            }
        }
        self.entries.extend(batch.entries);
        self.entries
            .sort_by(|a, b| (&a.owner, a.kind, &a.id).cmp(&(&b.owner, b.kind, &b.id)));
        Ok(())
    }
    pub fn keys(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.entries
            .iter()
            .map(|e| (e.owner.as_str(), e.kind, e.id.as_str()))
    }
    pub(crate) fn find_index<C: Contribution>(&self, id: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.kind == C::KIND && entry.id == id && entry.value.is::<C>())
    }
    pub(crate) fn binding_index<C: Contribution>(
        &self,
        owner: &str,
        id: &str,
    ) -> Result<usize, RegistryFault> {
        let (index, entry) = self
            .entries
            .iter()
            .enumerate()
            .find(|(_, entry)| entry.kind == C::KIND && entry.id == id)
            .ok_or_else(|| RegistryFault::MissingContribution {
                owner: owner.into(),
                kind: C::KIND,
                id: id.into(),
            })?;
        if !entry.value.is::<C>() {
            return Err(RegistryFault::ContributionTypeMismatch {
                owner: entry.owner.clone(),
                kind: entry.kind,
                id: entry.id.clone(),
                expected: TypeId::of::<C>(),
                actual: entry.value.as_ref().type_id(),
            });
        }
        Ok(index)
    }
    pub(crate) fn get_index<C: Contribution>(&self, index: usize) -> Option<&C> {
        self.entries.get(index)?.value.downcast_ref::<C>()
    }
    pub fn iter<C: Contribution>(&self) -> impl Iterator<Item = (&str, &C)> {
        self.entries
            .iter()
            .filter_map(|e| e.value.downcast_ref::<C>().map(|v| (e.owner.as_str(), v)))
    }
}

pub trait PluginInstance: Send + Sync {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure>;
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Patch(&'static str);
    struct Resource(&'static str);
    impl Contribution for Patch {
        const KIND: &'static str = "graph.patch";
        fn id(&self) -> &str {
            self.0
        }
    }
    impl Contribution for Resource {
        const KIND: &'static str = "resource.offer";
        fn id(&self) -> &str {
            self.0
        }
    }
    #[test]
    fn registry_fault_sources_retain_captured_identity_owner_and_actual_type() {
        use std::error::Error as _;
        let error = Registrar::new("bad/owner").err().unwrap();
        assert_eq!(
            error.source().unwrap().downcast_ref::<RegistryFault>(),
            Some(&RegistryFault::InvalidOwner {
                owner: "bad/owner".into()
            })
        );
        let mut registrar = Registrar::new("alpha").unwrap();
        for id in ["", "invalid/id"] {
            let error = registrar.add(Patch(id)).unwrap_err();
            assert_eq!(
                error.source().unwrap().downcast_ref::<RegistryFault>(),
                Some(&RegistryFault::InvalidContribution {
                    owner: "alpha".into(),
                    kind: Patch::KIND,
                    id: id.into()
                })
            );
        }
        registrar.add(Patch("main")).unwrap();
        let error = registrar.add(Patch("main")).unwrap_err();
        assert_eq!(
            error.source().unwrap().downcast_ref::<RegistryFault>(),
            Some(&RegistryFault::DuplicateContribution {
                owner: "alpha".into(),
                kind: Patch::KIND,
                id: "main".into()
            })
        );
        let mut foreign = Registrar::new("zeta").unwrap();
        foreign.add(Resource("offer")).unwrap();
        let error = registrar.merge(foreign.finish()).unwrap_err();
        assert_eq!(
            error.source().unwrap().downcast_ref::<RegistryFault>(),
            Some(&RegistryFault::OwnerMismatch {
                expected: "alpha".into(),
                actual: "zeta".into()
            })
        );
        struct OtherPatch;
        impl Contribution for OtherPatch {
            const KIND: &'static str = Patch::KIND;
            fn id(&self) -> &str {
                "main"
            }
        }
        let contributions = registrar.finish();
        assert_eq!(
            contributions.binding_index::<OtherPatch>("alpha", "main"),
            Err(RegistryFault::ContributionTypeMismatch {
                owner: "alpha".into(),
                kind: Patch::KIND,
                id: "main".into(),
                expected: TypeId::of::<OtherPatch>(),
                actual: TypeId::of::<Patch>(),
            })
        );
        assert_eq!(
            contributions.binding_index::<Resource>("alpha", "main"),
            Err(RegistryFault::MissingContribution {
                owner: "alpha".into(),
                kind: Resource::KIND,
                id: "main".into()
            })
        );
    }

    #[test]
    fn identity_limits_remain_byte_bounded_and_application_failures_remain_leaves() {
        use std::error::Error as _;
        assert!(Registrar::new("x".repeat(256)).is_ok());
        let error = Registrar::new("x".repeat(257)).err().unwrap();
        assert_eq!(error.code, Code::DescriptorInvalid);
        assert!(error.source().unwrap().is::<RegistryFault>());
        let error = Failure::new(Code::PluginFailed, "application lifecycle failure");
        assert!(error.source().is_none());
    }
    #[test]
    fn multiple_kinds_use_one_graph_agnostic_collector() {
        let mut r = Registrar::new("agent").unwrap();
        r.add(Patch("base")).unwrap();
        r.add(Resource("base")).unwrap();
        let c = r.finish();
        assert_eq!(c.iter::<Patch>().count(), 1);
        assert_eq!(c.iter::<Resource>().count(), 1);
    }
    #[test]
    fn duplicate_within_owner_and_kind_fails() {
        let mut r = Registrar::new("agent").unwrap();
        r.add(Patch("base")).unwrap();
        assert!(r.add(Patch("base")).is_err());
    }
    #[test]
    fn merge_is_atomic_and_owner_preserving() {
        let mut a = Registrar::new("alpha").unwrap();
        a.add(Patch("main")).unwrap();
        let mut inv = a.finish();
        let mut b = Registrar::new("alpha").unwrap();
        b.add(Resource("new")).unwrap();
        b.add(Patch("main")).unwrap();
        assert!(inv.merge(b.finish()).is_err());
        assert_eq!(inv.iter::<Resource>().count(), 0);
        let mut c = Registrar::new("zeta").unwrap();
        c.add(Patch("main")).unwrap();
        inv.merge(c.finish()).unwrap();
        assert_eq!(
            inv.iter::<Patch>()
                .map(|(owner, _)| owner)
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
    }
    #[test]
    fn invalid_names_fail() {
        assert!(Registrar::new("").is_err());
        let mut r = Registrar::new("alpha").unwrap();
        assert!(r.add(Patch("")).is_err());
    }

    struct StatefulId {
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        first: &'static str,
        later: &'static str,
    }
    impl Contribution for StatefulId {
        const KIND: &'static str = "stateful";
        fn id(&self) -> &str {
            if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                self.first
            } else {
                self.later
            }
        }
    }

    #[test]
    fn add_captures_identity_once_and_merge_uses_stored_identity() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let calls = Arc::new(AtomicUsize::new(0));
        let mut bundle = Registrar::new("alpha").unwrap();
        bundle
            .add(StatefulId {
                calls: calls.clone(),
                first: "valid",
                later: "invalid/id",
            })
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let mut destination = Registrar::new("alpha").unwrap();
        destination.merge(bundle.finish()).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            destination.finish().keys().collect::<Vec<_>>(),
            [("alpha", "stateful", "valid")]
        );
    }

    #[test]
    fn add_rejects_the_captured_invalid_identity_without_retrying() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let calls = Arc::new(AtomicUsize::new(0));
        let mut destination = Registrar::new("alpha").unwrap();
        assert!(
            destination
                .add(StatefulId {
                    calls: calls.clone(),
                    first: "invalid/id",
                    later: "valid",
                })
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(destination.finish().keys().count(), 0);
    }

    #[test]
    fn registrar_caught_late_collision_installs_and_reserves_nothing() {
        let mut destination = Registrar::new("alpha").unwrap();
        destination.add(Resource("taken")).unwrap();
        let mut bundle = Registrar::new("alpha").unwrap();
        bundle.add(Patch("fresh")).unwrap();
        bundle.add(Resource("fresh")).unwrap();
        bundle.add(Resource("taken")).unwrap();
        assert!(destination.merge(bundle.finish()).is_err());
        assert_eq!(destination.entries.len(), 1);
        destination.add(Patch("fresh")).unwrap();
        destination.add(Resource("fresh")).unwrap();
        assert_eq!(destination.finish().keys().count(), 3);
    }

    #[test]
    fn registrar_merge_rejects_multiowner_bundles_before_mutation() {
        let mut destination = Registrar::new("alpha").unwrap();
        destination.add(Patch("installed")).unwrap();
        let mut local = Registrar::new("alpha").unwrap();
        local.add(Patch("fresh")).unwrap();
        let mut bundle = local.finish();
        let mut foreign = Registrar::new("zeta").unwrap();
        foreign.add(Resource("foreign")).unwrap();
        // Host inventories still support combining distinct owners.
        bundle.merge(foreign.finish()).unwrap();
        assert!(destination.merge(bundle).is_err());
        assert_eq!(destination.entries.len(), 1);
        destination.add(Patch("fresh")).unwrap();
        destination.add(Resource("foreign")).unwrap();
    }

    #[test]
    fn registrar_merge_preflights_all_stored_names_and_identities() {
        for invalid in ["id", "kind", "duplicate"] {
            let mut destination = Registrar::new("alpha").unwrap();
            let mut source = Registrar::new("alpha").unwrap();
            source.add(Patch("fresh")).unwrap();
            source.add(Patch("later")).unwrap();
            let mut bundle = source.finish();
            match invalid {
                "id" => bundle.entries[1].id = "invalid/id".into(),
                "kind" => bundle.entries[1].kind = "invalid/kind",
                "duplicate" => bundle.entries[1].id = "fresh".into(),
                _ => unreachable!(),
            }
            assert!(destination.merge(bundle).is_err());
            assert!(destination.entries.is_empty());
            assert!(destination.names.is_empty());
            destination.add(Patch("fresh")).unwrap();
            destination.add(Patch("later")).unwrap();
        }
    }
}
