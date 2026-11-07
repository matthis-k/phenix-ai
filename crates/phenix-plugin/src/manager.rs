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
            revisions: BTreeMap::new(),
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

/// A sealed, unpublished plugin candidate. The kernel may compile a graph using
/// the staged snapshot before committing the selected implementation.
/// Dropping this candidate aborts staging without changing the active selection.
pub struct PreparedPlugin {
    catalog: Arc<()>,
    name: String,
    expected_revision: u64,
    expected_active: Option<Arc<Loaded>>,
    loaded: Arc<Loaded>,
}

impl PreparedPlugin {
    pub fn snapshot(&self) -> PluginSnapshot {
        PluginSnapshot {
            loaded: self.loaded.clone(),
        }
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
    revisions: BTreeMap<String, u64>,
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
        require_loaded: bool,
    ) -> Result<PreparedPlugin, Failure> {
        let factory = self.factory(key)?;
        let previous = self.loaded.get(key.name()).cloned();
        if require_loaded != previous.is_some() {
            return Err(Failure::new(
                Code::MissingBinding,
                if require_loaded {
                    "plugin is not loaded"
                } else {
                    "plugin already loaded"
                },
            ));
        }
        // Revision history survives unload. Otherwise an old staged candidate
        // could be committed after unload/reload with the same revision (ABA).
        let expected_revision = *self.revisions.get(key.name()).unwrap_or(&0);
        let revision = expected_revision.checked_add(1).ok_or_else(|| {
            Failure::new(Code::InfrastructureFailure, "plugin revision exhausted")
        })?;
        let instance = (factory.make)(Box::new(configuration))?;
        let mut registrar = Registrar::new(key.name())?;
        instance.contribute(&mut registrar)?;
        let loaded = Arc::new(Loaded {
            contributions: registrar.finish(),
            _implementation: instance,
            _factory: factory,
            name: key.name().to_owned(),
            revision,
        });
        Ok(PreparedPlugin {
            catalog: self.catalog.clone(),
            name: key.name().to_owned(),
            expected_revision,
            expected_active: previous,
            loaded,
        })
    }

    /// Prepare a new plugin without activating it. Kernel composition may
    /// validate the candidate graph and resources before calling commit.
    pub fn stage_load<C: Send + 'static>(
        &self,
        key: &PluginKey<C>,
        configuration: C,
    ) -> Result<PreparedPlugin, Failure> {
        self.stage(key, configuration, false)
    }

    /// Prepare a replacement. The previous revision stays active until commit.
    pub fn stage_reload<C: Send + 'static>(
        &self,
        key: &PluginKey<C>,
        configuration: C,
    ) -> Result<PreparedPlugin, Failure> {
        self.stage(key, configuration, true)
    }

    /// Publish an already validated candidate. Reject stale candidates
    /// atomically, including candidates prepared before an unload.
    ///
    /// The kernel coordinates this with activation of its compiled graph;
    /// this method does not mutate a graph or grant runtime authority.
    pub fn commit(&mut self, staged: PreparedPlugin) -> Result<PluginSnapshot, Failure> {
        if !Arc::ptr_eq(&self.catalog, &staged.catalog) {
            return Err(Failure::new(Code::Denied, "foreign staged plugin"));
        }
        let current = self.loaded.get(&staged.name);
        let same_active = match (&staged.expected_active, current) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        };
        if !same_active
            || self.revisions.get(&staged.name).copied().unwrap_or(0) != staged.expected_revision
        {
            return Err(Failure::new(Code::Denied, "stale plugin composition"));
        }
        self.revisions
            .insert(staged.name.clone(), staged.loaded.revision);
        let loaded = staged.loaded;
        self.loaded.insert(staged.name, loaded.clone());
        Ok(PluginSnapshot { loaded })
    }

    pub fn load<C: Send + 'static>(
        &mut self,
        key: &PluginKey<C>,
        configuration: C,
    ) -> Result<PluginSnapshot, Failure> {
        let candidate = self.stage_load(key, configuration)?;
        self.commit(candidate)
    }

    /// Convenience for plugin-only use. Kernel graph recomposition must use
    /// stage_reload + graph compile + commit to avoid publishing too early.
    pub fn reload<C: Send + 'static>(
        &mut self,
        key: &PluginKey<C>,
        configuration: C,
    ) -> Result<PluginSnapshot, Failure> {
        let candidate = self.stage_reload(key, configuration)?;
        self.commit(candidate)
    }

    /// Logical unload: no new snapshots or activations from this plugin.
    /// Existing graph generations may still own the previous PluginSnapshot.
    pub fn unload<C>(&mut self, key: &PluginKey<C>) -> Result<(), Failure> {
        self.factory(key)?;
        self.loaded
            .remove(key.name())
            .map(|_| ())
            .ok_or_else(|| Failure::new(Code::MissingBinding, "plugin is not loaded"))
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
