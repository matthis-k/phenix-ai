//! Graph-independent, typed in-process contribution staging.
//! Identity is host-supplied provenance, never a permission or ABI guarantee.
use phenix_contract::{identifier, Code, Failure};
use std::{any::{Any, TypeId}, collections::{BTreeMap, BTreeSet}};

pub trait Contribution: Any + Send + Sync {
    const KIND: &'static str;
    fn id(&self) -> &str;
}

struct Entry {
    owner: String,
    kind: &'static str,
    id: String,
    type_id: TypeId,
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
            return Err(Failure::new(Code::DescriptorInvalid, "invalid owner"));
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
        if !identifier(C::KIND) || !identifier(value.id()) {
            return Err(Failure::new(
                Code::DescriptorInvalid,
                "invalid contribution identity",
            ));
        }
        if self.entries.iter().any(|entry| {
            entry.kind == C::KIND && entry.type_id != TypeId::of::<C>()
        }) {
            return Err(Failure::new(Code::SchemaRejected, "contribution kind type mismatch"));
        }
        let id = value.id().to_owned();
        if !self.names.insert((C::KIND, id.clone())) {
            return Err(Failure::new(
                Code::DescriptorInvalid,
                "duplicate contribution",
            ));
        }
        self.entries.push(Entry {
            owner: self.owner.clone(),
            kind: C::KIND,
            id,
            type_id: TypeId::of::<C>(),
            value: Box::new(value),
        });
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
        let mut types: BTreeMap<&str, TypeId> = self.entries.iter()
            .map(|entry| (entry.kind, entry.type_id)).collect();
        for e in &batch.entries {
            if types.insert(e.kind, e.type_id).is_some_and(|known| known != e.type_id) {
                return Err(Failure::new(Code::SchemaRejected, "contribution kind type mismatch"));
            }
            if !names.insert((&e.owner, e.kind, &e.id)) {
                return Err(Failure::new(
                    Code::DescriptorInvalid,
                    "duplicate selected contribution",
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
    struct Spoof(&'static str);
    impl Contribution for Spoof {
        const KIND: &'static str = "graph.patch";
        fn id(&self) -> &str { self.0 }
    }

    #[test]
    fn incompatible_types_cannot_claim_one_registered_kind() {
        let mut registrar = Registrar::new("alpha").unwrap();
        registrar.add(Patch("node")).unwrap();
        assert!(registrar.add(Spoof("different")).is_err());
        let mut inventory = registrar.finish();

        let mut other = Registrar::new("beta").unwrap();
        other.add(Spoof("another")).unwrap();
        assert!(inventory.merge(other.finish()).is_err());
        assert_eq!(inventory.keys().count(), 1);
        assert_eq!(inventory.iter::<Patch>().count(), 1);
    }
    #[test]
    fn invalid_names_fail() {
        assert!(Registrar::new("").is_err());
        let mut r = Registrar::new("alpha").unwrap();
        assert!(r.add(Patch("")).is_err());
    }
}
