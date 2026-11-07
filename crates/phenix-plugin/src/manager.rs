//! Rust-only plugin catalog and independent kernel-instance plugin sets.
//!
//! Construction is typed at the call site; implementations are erased only
//! inside the manager. Contributions are retained through leased handles.
//! Dynamically linked cdylibs require a separate ABI adapter (not Rust Any).
use crate::{Contribution, Contributions, PluginInstance, Registrar};
use phenix_contract::{identifier, Code, Failure};
use std::{any::Any, collections::BTreeMap, marker::PhantomData, ops::Deref, sync::Arc};

type Configuration = Box<dyn Any + Send>;
type Constructor = dyn Fn(Configuration) -> Result<Box<dyn PluginInstance>, Failure> + Send + Sync;

struct Factory {
    make: Box<Constructor>,
}

/// A strongly typed registration key. The configuration type is checked at
/// the call site, while the manager stores heterogeneous factories internally.
pub struct PluginKey<C> {
    name: String,
    catalog: Arc<()>,
    _configuration: PhantomData<fn(C) -> C>,
}

impl<C> Clone for PluginKey<C> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            catalog: self.catalog.clone(),
            _configuration: PhantomData,
        }
    }
}

impl<C> PluginKey<C> {
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Registers Rust factories once, then constructs as many independent plugin
/// sets as the kernel needs. The catalog has no graph or resource dependencies.
pub struct PluginCatalog {
    identity: Arc<()>,
    factories: BTreeMap<String, Arc<Factory>>,
}

impl Default for PluginCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginCatalog {
    pub fn new() -> Self {
        Self {
            identity: Arc::new(()),
            factories: BTreeMap::new(),
        }
    }

    pub fn register<C, P, F>(
        &mut self,
        name: impl Into<String>,
        factory: F,
    ) -> Result<PluginKey<C>, Failure>
    where
        C: Send + 'static,
        P: PluginInstance + 'static,
        F: Fn(C) -> Result<P, Failure> + Send + Sync + 'static,
    {
        let name = name.into();
        if name == "root" || !identifier(&name) {
            return Err(Failure::new(
                Code::DescriptorInvalid,
                "invalid plugin identity",
            ));
        }
        if self.factories.contains_key(&name) {
            return Err(Failure::new(
                Code::DescriptorInvalid,
                "duplicate plugin factory",
            ));
        }
        let make = move |configuration: Configuration| {
            let configuration = configuration.downcast::<C>().map_err(|_| {
                Failure::new(Code::SchemaRejected, "plugin configuration type mismatch")
            })?;
            factory(*configuration).map(|instance| Box::new(instance) as Box<dyn PluginInstance>)
        };
        self.factories.insert(
            name.clone(),
            Arc::new(Factory {
                make: Box::new(make),
            }),
        );
        Ok(PluginKey {
            name,
            catalog: self.identity.clone(),
            _configuration: PhantomData,
        })
    }

    /// A new set snapshots the registered factories; further catalog changes
    /// cannot silently change already created kernel instances.
    pub fn instance(&self) -> PluginSet {
        PluginSet {
            catalog: self.identity.clone(),
            factories: self.factories.clone(),
            loaded: BTreeMap::new(),
            next_revision: 0,
        }
    }
}

// Field drop order is deliberate: contributions (which may contain plugin code)
// are dropped before implementation state, and both before the factory/code lease.
struct Loaded {
    contributions: Contributions,
    _implementation: Box<dyn PluginInstance>,
    _factory: Arc<Factory>,
    name: String,
    revision: u64,
}

/// A cloneable lease on an exact plugin revision. Existing users remain pinned
/// when the plugin is unloaded or a newer revision is activated.
#[derive(Clone)]
pub struct PluginSnapshot {
    loaded: Arc<Loaded>,
}

impl PluginSnapshot {
    pub fn name(&self) -> &str {
        &self.loaded.name
    }

    pub fn revision(&self) -> u64 {
        self.loaded.revision
    }

    pub fn keys(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.loaded.contributions.keys()
    }

    /// Return a contribution with the plugin's lifetime attached. Graph
    /// adapters should retain these handles inside their compiled actions.
    pub fn get<C: Contribution>(&self, id: &str) -> Option<ContributionHandle<C>> {
        let index = self.loaded.contributions.find_index::<C>(id)?;
        Some(ContributionHandle {
            loaded: self.loaded.clone(),
            index,
            _value: PhantomData,
        })
    }
}

/// Typed contribution whose lifetime pins both plugin state and code.
pub struct ContributionHandle<C: Contribution> {
    loaded: Arc<Loaded>,
    index: usize,
    _value: PhantomData<fn() -> C>,
}

impl<C: Contribution> Clone for ContributionHandle<C> {
    fn clone(&self) -> Self {
        Self {
            loaded: self.loaded.clone(),
            index: self.index,
            _value: PhantomData,
        }
    }
}

impl<C: Contribution> Deref for ContributionHandle<C> {
    type Target = C;

    fn deref(&self) -> &Self::Target {
        // The index was checked at handle creation and the frozen inventory
        // never changes; the lifetime is pinned by Arc<Loaded>.
        self.loaded
            .contributions
            .get_index::<C>(self.index)
            .expect("validated contribution remains present")
    }
}

/// A staged change in one plugin set. It owns the candidate until the host
/// has compiled and validated the graph that will reference it. Dropping it
/// aborts the replacement without changing the active plugin set.
pub struct StagedPlugin {
    catalog: Arc<()>,
    name: String,
    expected_epoch: u64,
    expected_revision: Option<u64>,
    candidate: Option<Arc<Loaded>>,
}

impl StagedPlugin {
    pub fn candidate(&self) -> Option<PluginSnapshot> {
        self.candidate.as_ref().map(|loaded| PluginSnapshot {
            loaded: Arc::clone(loaded),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Independent plugin instances selected for one kernel instance. This is
/// deliberately not a graph builder or a second invocation scheduler.
pub struct PluginSet {
    catalog: Arc<()>,
    factories: BTreeMap<String, Arc<Factory>>,
    loaded: BTreeMap<String, Arc<Loaded>>,
    next_revision: u64,
}

impl PluginSet {
    fn factory<C>(&self, key: &PluginKey<C>) -> Result<Arc<Factory>, Failure> {
        if !Arc::ptr_eq(&self.catalog, &key.catalog) {
            return Err(Failure::new(Code::Denied, "foreign plugin catalog key"));
        }
        self.factories
            .get(key.name())
            .cloned()
            .ok_or_else(|| Failure::new(Code::MissingBinding, "unregistered plugin"))
    }

    fn stage<C: Send + 'static>(
        &self,
        key: &PluginKey<C>,
        configuration: C,
    ) -> Result<Arc<Loaded>, Failure> {
        let factory = self.factory(key)?;
        let revision = self.next_revision.checked_add(1).ok_or_else(|| {
            Failure::new(Code::InfrastructureFailure, "plugin revision exhausted")
        })?;
        let instance = (factory.make)(Box::new(configuration))?;
        let mut registrar = Registrar::new(key.name())?;
        instance.contribute(&mut registrar)?;
        Ok(Arc::new(Loaded {
            contributions: registrar.finish(),
            _implementation: instance,
            _factory: factory,
            name: key.name().to_owned(),
            revision,
        }))
    }

    fn expected_revision(&self, name: &str) -> Option<u64> {
        self.loaded.get(name).map(|loaded| loaded.revision)
    }

    /// Construct and register a new implementation, but do not activate it.
    /// Kernel composition can use the candidate to build a graph and simply
    /// drop this stage if patch resolution or compilation fails.
    pub fn prepare_load<C: Send + 'static>(
        &self,
        key: &PluginKey<C>,
        configuration: C,
    ) -> Result<StagedPlugin, Failure> {
        self.factory(key)?;
        if self.loaded.contains_key(key.name()) {
            return Err(Failure::new(
                Code::DescriptorInvalid,
                "plugin already loaded",
            ));
        }
        Ok(StagedPlugin {
            catalog: self.catalog.clone(),
            name: key.name().to_owned(),
            expected_epoch: self.next_revision,
            expected_revision: None,
            candidate: Some(self.stage(key, configuration)?),
        })
    }

    pub fn prepare_reload<C: Send + 'static>(
        &self,
        key: &PluginKey<C>,
        configuration: C,
    ) -> Result<StagedPlugin, Failure> {
        self.factory(key)?;
        let expected_revision = self
            .expected_revision(key.name())
            .ok_or_else(|| Failure::new(Code::MissingBinding, "plugin is not loaded"))?;
        Ok(StagedPlugin {
            catalog: self.catalog.clone(),
            name: key.name().to_owned(),
            expected_epoch: self.next_revision,
            expected_revision: Some(expected_revision),
            candidate: Some(self.stage(key, configuration)?),
        })
    }

    pub fn prepare_unload<C>(&self, key: &PluginKey<C>) -> Result<StagedPlugin, Failure> {
        self.factory(key)?;
        let expected_revision = self
            .expected_revision(key.name())
            .ok_or_else(|| Failure::new(Code::MissingBinding, "plugin is not loaded"))?;
        Ok(StagedPlugin {
            catalog: self.catalog.clone(),
            name: key.name().to_owned(),
            expected_epoch: self.next_revision,
            expected_revision: Some(expected_revision),
            candidate: None,
        })
    }

    /// Only commit after the caller has completed dependent graph compilation.
    /// Reject stale stages to prevent an older candidate from overwriting
    /// a newer plugin instance. No state changes on failure.
    pub fn commit(&mut self, staged: StagedPlugin) -> Result<Option<PluginSnapshot>, Failure> {
        if !Arc::ptr_eq(&self.catalog, &staged.catalog) {
            return Err(Failure::new(Code::Denied, "foreign plugin catalog"));
        }
        if self.next_revision != staged.expected_epoch
            || self.expected_revision(&staged.name) != staged.expected_revision
        {
            return Err(Failure::new(Code::Denied, "stale plugin stage"));
        }
        let next_revision = self.next_revision.checked_add(1).ok_or_else(|| {
            Failure::new(Code::InfrastructureFailure, "plugin revision exhausted")
        })?;
        if staged
            .candidate
            .as_ref()
            .is_some_and(|loaded| loaded.revision != next_revision)
        {
            return Err(Failure::new(
                Code::InfrastructureFailure,
                "candidate revision mismatch",
            ));
        }
        self.next_revision = next_revision;
        match staged.candidate {
            Some(candidate) => {
                self.loaded.insert(staged.name, candidate.clone());
                Ok(Some(PluginSnapshot { loaded: candidate }))
            }
            None => {
                self.loaded.remove(&staged.name);
                Ok(None)
            }
        }
    }

    /// Convenience for one-step loading. Use prepare_load/commit when loading
    /// must be atomic with graph compilation and generation activation.
    pub fn load<C: Send + 'static>(
        &mut self,
        key: &PluginKey<C>,
        configuration: C,
    ) -> Result<PluginSnapshot, Failure> {
        let staged = self.prepare_load(key, configuration)?;
        self.commit(staged)?.ok_or_else(|| {
            Failure::new(Code::InfrastructureFailure, "loaded stage has no candidate")
        })
    }

    pub fn reload<C: Send + 'static>(
        &mut self,
        key: &PluginKey<C>,
        configuration: C,
    ) -> Result<PluginSnapshot, Failure> {
        let staged = self.prepare_reload(key, configuration)?;
        self.commit(staged)?.ok_or_else(|| {
            Failure::new(
                Code::InfrastructureFailure,
                "reloaded stage has no candidate",
            )
        })
    }

    /// Logical unload. Existing leases and compiled generations remain alive.
    pub fn unload<C>(&mut self, key: &PluginKey<C>) -> Result<(), Failure> {
        let staged = self.prepare_unload(key)?;
        self.commit(staged)?;
        Ok(())
    }

    pub fn snapshot<C>(&self, key: &PluginKey<C>) -> Result<PluginSnapshot, Failure> {
        self.factory(key)?;
        let loaded = self
            .loaded
            .get(key.name())
            .ok_or_else(|| Failure::new(Code::MissingBinding, "plugin is not loaded"))?;
        Ok(PluginSnapshot {
            loaded: loaded.clone(),
        })
    }

    pub fn snapshots(&self) -> impl Iterator<Item = PluginSnapshot> + '_ {
        self.loaded
            .values()
            .cloned()
            .map(|loaded| PluginSnapshot { loaded })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct GraphPatch {
        name: String,
        value: i32,
    }
    impl Contribution for GraphPatch {
        const KIND: &'static str = "graph.patch";
        fn id(&self) -> &str {
            &self.name
        }
    }

    struct ResourceOffer(String);
    impl Contribution for ResourceOffer {
        const KIND: &'static str = "resource.offer";
        fn id(&self) -> &str {
            &self.0
        }
    }

    struct Example {
        value: i32,
        drops: Arc<AtomicUsize>,
    }
    impl Drop for Example {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl PluginInstance for Example {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            if self.value < 0 {
                return Err(Failure::new(Code::PluginFailed, "invalid configuration"));
            }
            into.add(GraphPatch {
                name: "calculate".to_owned(),
                value: self.value,
            })?;
            into.add(ResourceOffer("storage".to_owned()))
        }
    }

    #[test]
    fn per_instance_isolation_and_typed_cross_kind_contributions() {
        let mut catalog = PluginCatalog::new();
        let drops = Arc::new(AtomicUsize::new(0));
        let counts = drops.clone();
        let key = catalog
            .register("example", move |value: i32| {
                Ok(Example {
                    value,
                    drops: counts.clone(),
                })
            })
            .unwrap();
        let mut a = catalog.instance();
        let mut b = catalog.instance();
        let a_current = a.load(&key, 3).unwrap();
        let b_current = b.load(&key, 9).unwrap();
        assert_eq!(a_current.get::<GraphPatch>("calculate").unwrap().value, 3);
        assert_eq!(b_current.get::<GraphPatch>("calculate").unwrap().value, 9);
        assert!(a_current.get::<ResourceOffer>("storage").is_some());
        assert_eq!(a.snapshot(&key).unwrap().revision(), 1);
        assert_eq!(b.snapshot(&key).unwrap().revision(), 1);
    }

    #[test]
    fn reload_is_atomic_and_unload_respects_leases() {
        let mut catalog = PluginCatalog::new();
        let drops = Arc::new(AtomicUsize::new(0));
        let counts = drops.clone();
        let key = catalog
            .register("example", move |value: i32| {
                Ok(Example {
                    value,
                    drops: counts.clone(),
                })
            })
            .unwrap();
        let mut plugins = catalog.instance();
        let pinned = plugins.load(&key, 11).unwrap();
        let action = pinned.get::<GraphPatch>("calculate").unwrap();
        assert!(plugins.reload(&key, -1).is_err());
        assert_eq!(plugins.snapshot(&key).unwrap().revision(), 1);
        let current = plugins.reload(&key, 22).unwrap();
        assert_eq!(current.revision(), 2);
        assert_eq!(action.value, 11);
        plugins.unload(&key).unwrap();
        assert!(plugins.snapshot(&key).is_err());
        assert_eq!(current.get::<GraphPatch>("calculate").unwrap().value, 22);
        assert_eq!(drops.load(Ordering::SeqCst), 1); // rejected staged instance
        drop(pinned);
        drop(action);
        drop(current);
        assert_eq!(drops.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn staged_reload_is_not_visible_until_explicit_commit() {
        let mut catalog = PluginCatalog::new();
        let key = catalog
            .register("model", |n: i32| {
                Ok(Example {
                    value: n,
                    drops: Arc::new(AtomicUsize::new(0)),
                })
            })
            .unwrap();
        let mut plugins = catalog.instance();
        plugins.load(&key, 1).unwrap();
        let staged = plugins.prepare_reload(&key, 9).unwrap();
        assert_eq!(
            staged
                .candidate()
                .unwrap()
                .get::<GraphPatch>("calculate")
                .unwrap()
                .value,
            9
        );
        assert_eq!(
            plugins
                .snapshot(&key)
                .unwrap()
                .get::<GraphPatch>("calculate")
                .unwrap()
                .value,
            1
        );
        drop(staged); // simulated compiler failure: no graph or plugin change
        assert_eq!(plugins.snapshot(&key).unwrap().revision(), 1);
        let replacement = plugins.prepare_reload(&key, 5).unwrap();
        let activated = plugins.commit(replacement).unwrap().unwrap();
        assert_eq!(activated.get::<GraphPatch>("calculate").unwrap().value, 5);
    }

    #[test]
    fn stale_prepared_changes_cannot_replace_current_revision() {
        let mut catalog = PluginCatalog::new();
        let key = catalog
            .register("model", |n: i32| {
                Ok(Example {
                    value: n,
                    drops: Arc::new(AtomicUsize::new(0)),
                })
            })
            .unwrap();
        let mut plugins = catalog.instance();
        plugins.load(&key, 1).unwrap();
        let older = plugins.prepare_reload(&key, 2).unwrap();
        let newer = plugins.prepare_reload(&key, 3).unwrap();
        plugins.commit(newer).unwrap();
        assert!(plugins.commit(older).is_err());
        assert_eq!(
            plugins
                .snapshot(&key)
                .unwrap()
                .get::<GraphPatch>("calculate")
                .unwrap()
                .value,
            3
        );
    }

    #[test]
    fn prepared_unload_is_transactional_and_preserves_live_leases() {
        let mut catalog = PluginCatalog::new();
        let drops = Arc::new(AtomicUsize::new(0));
        let counts = drops.clone();
        let key = catalog
            .register("model", move |n: i32| {
                Ok(Example {
                    value: n,
                    drops: counts.clone(),
                })
            })
            .unwrap();
        let mut plugins = catalog.instance();
        let initial = plugins.load(&key, 4).unwrap();
        let prepared = plugins.prepare_unload(&key).unwrap();
        assert!(plugins.snapshot(&key).is_ok());
        plugins.commit(prepared).unwrap();
        assert!(plugins.snapshot(&key).is_err());
        assert_eq!(initial.get::<GraphPatch>("calculate").unwrap().value, 4);
        drop(initial);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn factory_failure_does_not_touch_active_instance() {
        let mut catalog = PluginCatalog::new();
        let key = catalog
            .register("fallible", |value: i32| {
                if value < 0 {
                    Err(Failure::new(Code::PluginFailed, "construction failed"))
                } else {
                    Ok(Example {
                        value,
                        drops: Arc::new(AtomicUsize::new(0)),
                    })
                }
            })
            .unwrap();
        let mut plugins = catalog.instance();
        plugins.load(&key, 4).unwrap();
        assert!(plugins.reload(&key, -8).is_err());
        assert_eq!(plugins.snapshot(&key).unwrap().revision(), 1);
    }

    #[test]
    fn cross_catalog_keys_are_denied_and_duplicate_ids_rejected() {
        let mut a = PluginCatalog::new();
        let key = a
            .register("same", |_: ()| {
                Ok(Example {
                    value: 0,
                    drops: Arc::new(AtomicUsize::new(0)),
                })
            })
            .unwrap();
        assert!(a
            .register("same", |_: ()| {
                Ok(Example {
                    value: 0,
                    drops: Arc::new(AtomicUsize::new(0)),
                })
            })
            .is_err());
        let mut b = PluginCatalog::new().instance();
        assert!(b.load(&key, ()).is_err());
    }
}
