//! Rust-only plugin catalog and independent kernel-instance plugin sets.
//!
//! Construction is typed at the call site; implementations are erased only
//! inside the manager. Contributions are retained through leased handles.
//! Dynamically linked cdylibs require a separate ABI adapter (not Rust Any).
use crate::registry::RegistryFault;
use crate::{CallbackPhase, Code, Failure, identifier};
use crate::{Contribution, Contributions, PluginInstance, Registrar};
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
    ops::Deref,
    sync::Arc,
};

type Configuration = Box<dyn Any + Send>;
type Constructor = dyn Fn(Configuration) -> Result<Box<dyn PluginInstance>, Failure> + Send + Sync;

/// The operation whose committed stage must contain a candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleOperation {
    Load,
    Reload,
}

/// Framework lifecycle failures. Catalog and set identities remain opaque;
/// these fields describe rejected operations, not transferable authority.
#[derive(Debug, PartialEq, Eq)]
pub enum LifecycleFault {
    ForeignCatalogKey {
        plugin: String,
    },
    UnregisteredPlugin {
        plugin: String,
    },
    RevisionExhausted {
        current_revision: u64,
    },
    AlreadyLoaded {
        plugin: String,
        revision: u64,
    },
    NotLoaded {
        plugin: String,
    },
    DuplicateStage {
        plugin: String,
    },
    ForeignSet {
        plugin: String,
    },
    StaleStage {
        plugin: String,
        expected_epoch: u64,
        actual_epoch: u64,
        expected_revision: Option<u64>,
        actual_revision: Option<u64>,
    },
    CandidateRevisionMismatch {
        plugin: String,
        expected: u64,
        actual: u64,
    },
    MissingCandidate {
        plugin: String,
        operation: LifecycleOperation,
        revision: u64,
    },
}
impl std::fmt::Display for LifecycleFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for LifecycleFault {}

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
            return Err(Failure::with_source(
                Code::DescriptorInvalid,
                "invalid plugin identity",
                Box::new(RegistryFault::InvalidPlugin { plugin: name }),
            ));
        }
        if self.factories.contains_key(&name) {
            return Err(Failure::with_source(
                Code::DescriptorInvalid,
                "duplicate plugin factory",
                Box::new(RegistryFault::DuplicatePlugin { plugin: name }),
            ));
        }
        let plugin = name.clone();
        let make = move |configuration: Configuration| {
            let configuration = configuration.downcast::<C>().map_err(|actual| {
                Failure::with_source(
                    Code::SchemaRejected,
                    "plugin configuration type mismatch",
                    Box::new(RegistryFault::ConfigurationTypeMismatch {
                        plugin: plugin.clone(),
                        expected: std::any::TypeId::of::<C>(),
                        actual: actual.as_ref().type_id(),
                    }),
                )
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
            set_identity: Arc::new(()),
            factories: self.factories.clone(),
            loaded: BTreeMap::new(),
            next_revision: 0,
        }
    }
}

// Field drop order is deliberate: contributions (which may contain plugin code)
// are dropped before implementation state, and both before the factory/code lease.
struct Loaded {
    revision_identity: Arc<()>,
    contributions: Contributions,
    _implementation: Box<dyn PluginInstance>,
    _factory: Arc<Factory>,
    name: String,
    revision: u64,
}

/// A cloneable lease on an exact plugin revision. Existing users remain pinned
/// when the plugin is unloaded or a newer revision is activated.
///
/// The snapshot pins the revision, not any domain-specific runtime object.
/// Hosts retain the required leases when they expose executable contributions.
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

    /// Exact revision identity, independent of the plugin's name and revision number.
    /// This token does not retain executable code or grant authority.
    pub fn revision_identity(&self) -> &Arc<()> {
        &self.loaded.revision_identity
    }

    pub fn keys(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.loaded.contributions.keys()
    }

    /// Resolve a typed contribution, then apply the caller's domain validation.
    /// A retained validation failure pins this revision and its concrete cause.
    pub fn bind<C: Contribution>(
        &self,
        id: &str,
        validate: impl FnOnce(&C) -> Result<(), Failure>,
    ) -> Result<ContributionHandle<C>, Failure> {
        (|| {
            let index = self
                .loaded
                .contributions
                .binding_index::<C>(self.name(), id)
                .map_err(|fault| {
                    Failure::with_source(
                        Code::MissingBinding,
                        "missing contribution or type mismatch",
                        Box::new(fault),
                    )
                })?;
            let handle = ContributionHandle::<C> {
                loaded: self.loaded.clone(),
                index,
                _value: PhantomData,
            };
            validate(&handle)?;
            Ok(handle)
        })()
        .map_err(|error: Failure| {
            let mut error = error.at(self.name(), CallbackPhase::Bind, self.loaded.clone());
            error
                .context
                .as_mut()
                .expect("binding context")
                .contribution = Some(id.into());
            error
        })
    }
    /// Return a contribution with the plugin's lifetime attached.
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
///
/// The identity token identifies the exact revision by allocation identity.
/// Identity alone is not a lease or permission; the handle retains both
/// implementation state and code.
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

impl<C: Contribution> ContributionHandle<C> {
    /// Exact identity of the leased plugin revision.
    pub fn revision_identity(&self) -> &Arc<()> {
        &self.loaded.revision_identity
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
    set_identity: Arc<()>,
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
    set_identity: Arc<()>,
    factories: BTreeMap<String, Arc<Factory>>,
    loaded: BTreeMap<String, Arc<Loaded>>,
    next_revision: u64,
}

impl PluginSet {
    fn factory<C>(&self, key: &PluginKey<C>) -> Result<Arc<Factory>, Failure> {
        if !Arc::ptr_eq(&self.catalog, &key.catalog) {
            return Err(Failure::with_source(
                Code::Denied,
                "foreign plugin catalog key",
                Box::new(LifecycleFault::ForeignCatalogKey {
                    plugin: key.name().into(),
                }),
            ));
        }
        self.factories.get(key.name()).cloned().ok_or_else(|| {
            Failure::with_source(
                Code::MissingBinding,
                "unregistered plugin",
                Box::new(LifecycleFault::UnregisteredPlugin {
                    plugin: key.name().into(),
                }),
            )
        })
    }

    fn stage<C: Send + 'static>(
        &self,
        key: &PluginKey<C>,
        configuration: C,
    ) -> Result<Arc<Loaded>, Failure> {
        let factory = self.factory(key)?;
        let revision = self.next_revision.checked_add(1).ok_or_else(|| {
            Failure::with_source(
                Code::InfrastructureFailure,
                "plugin revision exhausted",
                Box::new(LifecycleFault::RevisionExhausted {
                    current_revision: self.next_revision,
                }),
            )
        })?;
        let instance = (factory.make)(Box::new(configuration))
            .map_err(|error| error.at(key.name(), CallbackPhase::Construct, factory.clone()))?;
        let mut registrar = Registrar::new(key.name())?;
        let failure = match instance.contribute(&mut registrar) {
            Err(error) => Some(error),
            Ok(()) if registrar.owner() != key.name() => Some(Failure::with_source(
                Code::Denied,
                "plugin replaced registrar owner",
                Box::new(RegistryFault::OwnerMismatch {
                    expected: key.name().into(),
                    actual: registrar.owner().into(),
                }),
            )),
            Ok(()) => None,
        };
        let contributions = registrar.finish();
        if let Some(error) = failure {
            return Err(error.at(
                key.name(),
                CallbackPhase::Contribute,
                Arc::new((contributions, instance, factory)),
            ));
        }
        Ok(Arc::new(Loaded {
            revision_identity: Arc::new(()),
            contributions,
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
        if let Some(loaded) = self.loaded.get(key.name()) {
            return Err(Failure::with_source(
                Code::DescriptorInvalid,
                "plugin already loaded",
                Box::new(LifecycleFault::AlreadyLoaded {
                    plugin: key.name().into(),
                    revision: loaded.revision,
                }),
            ));
        }
        Ok(StagedPlugin {
            set_identity: self.set_identity.clone(),
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
        let expected_revision = self.expected_revision(key.name()).ok_or_else(|| {
            Failure::with_source(
                Code::MissingBinding,
                "plugin is not loaded",
                Box::new(LifecycleFault::NotLoaded {
                    plugin: key.name().into(),
                }),
            )
        })?;
        Ok(StagedPlugin {
            set_identity: self.set_identity.clone(),
            name: key.name().to_owned(),
            expected_epoch: self.next_revision,
            expected_revision: Some(expected_revision),
            candidate: Some(self.stage(key, configuration)?),
        })
    }

    pub fn prepare_unload<C>(&self, key: &PluginKey<C>) -> Result<StagedPlugin, Failure> {
        self.factory(key)?;
        let expected_revision = self.expected_revision(key.name()).ok_or_else(|| {
            Failure::with_source(
                Code::MissingBinding,
                "plugin is not loaded",
                Box::new(LifecycleFault::NotLoaded {
                    plugin: key.name().into(),
                }),
            )
        })?;
        Ok(StagedPlugin {
            set_identity: self.set_identity.clone(),
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
        Ok(self.commit_batch(vec![staged])?.pop().expect("one stage"))
    }

    /// Validate against one common epoch, including duplicate names, before any
    /// mutation. A nonempty batch advances the set epoch once; revisions are
    /// scoped to plugin names, so all replacements share that new epoch.
    pub fn validate_batch(&self, stages: &[StagedPlugin]) -> Result<(), Failure> {
        if stages.is_empty() {
            return Ok(());
        }
        let next_revision = self.next_revision.checked_add(1).ok_or_else(|| {
            Failure::with_source(
                Code::InfrastructureFailure,
                "plugin revision exhausted",
                Box::new(LifecycleFault::RevisionExhausted {
                    current_revision: self.next_revision,
                }),
            )
        })?;
        let mut names = BTreeSet::new();
        for staged in stages {
            if !names.insert(&staged.name) {
                return Err(Failure::with_source(
                    Code::Denied,
                    "duplicate plugin stage",
                    Box::new(LifecycleFault::DuplicateStage {
                        plugin: staged.name.clone(),
                    }),
                ));
            }
            if !Arc::ptr_eq(&self.set_identity, &staged.set_identity) {
                return Err(Failure::with_source(
                    Code::Denied,
                    "stage belongs to a different plugin set",
                    Box::new(LifecycleFault::ForeignSet {
                        plugin: staged.name.clone(),
                    }),
                ));
            }
            if self.next_revision != staged.expected_epoch
                || self.expected_revision(&staged.name) != staged.expected_revision
            {
                return Err(Failure::with_source(
                    Code::Denied,
                    "stale plugin stage",
                    Box::new(LifecycleFault::StaleStage {
                        plugin: staged.name.clone(),
                        expected_epoch: staged.expected_epoch,
                        actual_epoch: self.next_revision,
                        expected_revision: staged.expected_revision,
                        actual_revision: self.expected_revision(&staged.name),
                    }),
                ));
            }
            if let Some(loaded) = &staged.candidate
                && loaded.revision != next_revision
            {
                return Err(Failure::with_source(
                    Code::InfrastructureFailure,
                    "candidate revision mismatch",
                    Box::new(LifecycleFault::CandidateRevisionMismatch {
                        plugin: staged.name.clone(),
                        expected: next_revision,
                        actual: loaded.revision,
                    }),
                ));
            }
        }
        Ok(())
    }

    /// The exact post-commit inventory, without changing the live set.
    pub fn candidate_snapshots(
        &self,
        stages: &[StagedPlugin],
    ) -> Result<Vec<PluginSnapshot>, Failure> {
        self.validate_batch(stages)?;
        let mut snapshots: BTreeMap<_, _> = self
            .snapshots()
            .map(|snapshot| (snapshot.name().to_owned(), snapshot))
            .collect();
        for staged in stages {
            match staged.candidate() {
                Some(snapshot) => {
                    snapshots.insert(staged.name.clone(), snapshot);
                }
                None => {
                    snapshots.remove(&staged.name);
                }
            }
        }
        Ok(snapshots.into_values().collect())
    }

    /// All stages are validated before changing any member. On rejection the
    /// entire live set is unchanged; dropping stages releases their candidates.
    pub fn commit_batch(
        &mut self,
        stages: Vec<StagedPlugin>,
    ) -> Result<Vec<Option<PluginSnapshot>>, Failure> {
        self.validate_batch(&stages)?;
        if !stages.is_empty() {
            self.next_revision += 1; // overflow checked by validate_batch
        }
        Ok(stages
            .into_iter()
            .map(|staged| match staged.candidate {
                Some(candidate) => {
                    self.loaded.insert(staged.name, candidate.clone());
                    Some(PluginSnapshot { loaded: candidate })
                }
                None => {
                    self.loaded.remove(&staged.name);
                    None
                }
            })
            .collect())
    }

    /// Convenience for one-step loading. Use prepare_load/commit when loading
    /// must be atomic with graph compilation and generation activation.
    pub fn load<C: Send + 'static>(
        &mut self,
        key: &PluginKey<C>,
        configuration: C,
    ) -> Result<PluginSnapshot, Failure> {
        let staged = self.prepare_load(key, configuration)?;
        self.commit_candidate(staged, LifecycleOperation::Load)
    }

    pub fn reload<C: Send + 'static>(
        &mut self,
        key: &PluginKey<C>,
        configuration: C,
    ) -> Result<PluginSnapshot, Failure> {
        let staged = self.prepare_reload(key, configuration)?;
        self.commit_candidate(staged, LifecycleOperation::Reload)
    }

    fn commit_candidate(
        &mut self,
        staged: StagedPlugin,
        operation: LifecycleOperation,
    ) -> Result<PluginSnapshot, Failure> {
        let plugin = staged.name.clone();
        self.commit(staged)?.ok_or_else(|| {
            Failure::with_source(
                Code::InfrastructureFailure,
                match operation {
                    LifecycleOperation::Load => "loaded stage has no candidate",
                    LifecycleOperation::Reload => "reloaded stage has no candidate",
                },
                Box::new(LifecycleFault::MissingCandidate {
                    plugin,
                    operation,
                    revision: self.next_revision,
                }),
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
        let loaded = self.loaded.get(key.name()).ok_or_else(|| {
            Failure::with_source(
                Code::MissingBinding,
                "plugin is not loaded",
                Box::new(LifecycleFault::NotLoaded {
                    plugin: key.name().into(),
                }),
            )
        })?;
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

    #[test]
    fn factory_faults_preserve_plugin_and_concrete_configuration_types() {
        use std::{any::TypeId, error::Error as _};
        let mut catalog = PluginCatalog::new();
        let make = |value: i32| {
            Ok(Example {
                value,
                drops: Arc::default(),
            })
        };
        for name in ["root", "bad/id"] {
            let error = catalog.register(name, make).err().unwrap();
            assert_eq!(
                error.source().unwrap().downcast_ref::<RegistryFault>(),
                Some(&RegistryFault::InvalidPlugin {
                    plugin: name.into()
                })
            );
        }
        catalog.register("provider", make).unwrap();
        let error = catalog.register("provider", make).err().unwrap();
        assert_eq!(
            error.source().unwrap().downcast_ref::<RegistryFault>(),
            Some(&RegistryFault::DuplicatePlugin {
                plugin: "provider".into()
            })
        );
        let error = (catalog.factories["provider"].make)(Box::new(42_u64))
            .err()
            .unwrap();
        assert_eq!(
            error.source().unwrap().downcast_ref::<RegistryFault>(),
            Some(&RegistryFault::ConfigurationTypeMismatch {
                plugin: "provider".into(),
                expected: TypeId::of::<i32>(),
                actual: TypeId::of::<u64>(),
            })
        );
    }

    #[test]
    fn binding_faults_distinguish_actual_type_mismatch_from_unknown_kind_and_id() {
        use std::{any::TypeId, error::Error as _};
        struct OtherPatch;
        impl Contribution for OtherPatch {
            const KIND: &'static str = GraphPatch::KIND;
            fn id(&self) -> &str {
                "calculate"
            }
        }
        let mut catalog = PluginCatalog::new();
        let key = catalog
            .register("provider", |value| {
                Ok(Example {
                    value,
                    drops: Arc::default(),
                })
            })
            .unwrap();
        let snapshot = catalog.instance().load(&key, 1).unwrap();
        let mismatch = snapshot
            .bind::<OtherPatch>("calculate", |_| Ok(()))
            .err()
            .unwrap();
        let missing_kind = snapshot
            .bind::<ResourceOffer>("calculate", |_| Ok(()))
            .err()
            .unwrap();
        let missing_id = snapshot
            .bind::<GraphPatch>("absent", |_| Ok(()))
            .err()
            .unwrap();
        for (error, fault) in [
            (
                mismatch,
                RegistryFault::ContributionTypeMismatch {
                    owner: "provider".into(),
                    kind: GraphPatch::KIND,
                    id: "calculate".into(),
                    expected: TypeId::of::<OtherPatch>(),
                    actual: TypeId::of::<GraphPatch>(),
                },
            ),
            (
                missing_kind,
                RegistryFault::MissingContribution {
                    owner: "provider".into(),
                    kind: ResourceOffer::KIND,
                    id: "calculate".into(),
                },
            ),
            (
                missing_id,
                RegistryFault::MissingContribution {
                    owner: "provider".into(),
                    kind: GraphPatch::KIND,
                    id: "absent".into(),
                },
            ),
        ] {
            assert_eq!(error.code, Code::MissingBinding);
            let mut source = error.source().unwrap();
            while !source.is::<RegistryFault>() {
                source = source.source().unwrap();
            }
            assert_eq!(source.downcast_ref::<RegistryFault>(), Some(&fault));
        }
        assert!(snapshot.get::<OtherPatch>("calculate").is_none());
    }

    #[test]
    fn replaced_registrar_owner_is_rejected_before_load_or_reload() {
        struct Replace(bool, bool);
        impl PluginInstance for Replace {
            fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
                if self.0 {
                    *into = Registrar::new("forged")?;
                }
                if self.1 {
                    into.add(ResourceOffer("offer".into()))?;
                }
                Ok(())
            }
        }
        let mut catalog = PluginCatalog::new();
        let key = catalog
            .register("trusted", |(replace, add)| Ok(Replace(replace, add)))
            .unwrap();
        let mut plugins = catalog.instance();
        for add in [false, true] {
            let error = plugins.prepare_load(&key, (true, add)).err().unwrap();
            assert_eq!(error.code, Code::Denied);
            let mut source = std::error::Error::source(&error).unwrap();
            while !source.is::<RegistryFault>() {
                source = source.source().unwrap();
            }
            assert_eq!(
                source.downcast_ref::<RegistryFault>(),
                Some(&RegistryFault::OwnerMismatch {
                    expected: "trusted".into(),
                    actual: "forged".into(),
                })
            );
            let context = error.context.unwrap();
            assert_eq!(context.plugin, "trusted");
            assert_eq!(context.phase, CallbackPhase::Contribute);
            assert_eq!(plugins.snapshots().count(), 0);
        }
        let original = plugins.load(&key, (false, true)).unwrap();
        for add in [false, true] {
            assert!(plugins.prepare_reload(&key, (true, add)).is_err());
            assert!(Arc::ptr_eq(
                &original.loaded,
                &plugins.snapshot(&key).unwrap().loaded
            ));
        }
        assert_eq!(
            original.keys().collect::<Vec<_>>(),
            [("trusted", "resource.offer", "offer")]
        );
    }

    #[test]
    fn plugin_callback_failures_preserve_phase_identity_and_typed_source() {
        use std::error::Error;
        struct Broken;
        impl PluginInstance for Broken {
            fn contribute(&self, _: &mut Registrar) -> Result<(), Failure> {
                Err(Failure::with_source(
                    Code::PluginFailed,
                    "contribution failed",
                    Box::new(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
                ))
            }
        }
        let mut catalog = PluginCatalog::new();
        let key = catalog
            .register("broken", |construct: bool| {
                if construct {
                    Err(Failure::with_source(
                        Code::PluginFailed,
                        "construction failed",
                        Box::new(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
                    ))
                } else {
                    Ok(Broken)
                }
            })
            .unwrap();
        let plugins = catalog.instance();
        for (construct, phase) in [
            (true, CallbackPhase::Construct),
            (false, CallbackPhase::Contribute),
        ] {
            let error = plugins.prepare_load(&key, construct).err().unwrap();
            let context = error.context.as_ref().unwrap();
            assert_eq!(context.plugin, "broken");
            assert_eq!(context.phase, phase);
            let source = error
                .source()
                .unwrap()
                .source()
                .unwrap()
                .downcast_ref::<Failure>()
                .unwrap();
            assert_eq!(
                source
                    .source()
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .unwrap()
                    .kind(),
                std::io::ErrorKind::PermissionDenied
            );
        }
    }

    #[test]
    fn exhausted_revision_rejects_factories_and_commits_without_mutation() {
        let calls = Arc::new(AtomicUsize::new(0));
        let factory_calls = calls.clone();
        let mut catalog = PluginCatalog::new();
        let key = catalog
            .register("example", move |value| {
                factory_calls.fetch_add(1, Ordering::SeqCst);
                Ok(Example {
                    value,
                    drops: Arc::default(),
                })
            })
            .unwrap();
        let other = catalog
            .register("other", |_: ()| -> Result<Example, Failure> {
                panic!("factory must not run after revision exhaustion")
            })
            .unwrap();
        let mut set = catalog.instance();
        set.next_revision = u64::MAX - 1;
        let snapshot = set.load(&key, 1).unwrap();
        assert_eq!(snapshot.revision(), u64::MAX);
        for error in [
            set.prepare_reload(&key, 2).err().unwrap(),
            set.prepare_load(&other, ()).err().unwrap(),
        ] {
            assert_eq!(error.code, Code::InfrastructureFailure);
            assert_eq!(error.message, "plugin revision exhausted");
            assert_eq!(
                std::error::Error::source(&error)
                    .unwrap()
                    .downcast_ref::<LifecycleFault>(),
                Some(&LifecycleFault::RevisionExhausted {
                    current_revision: u64::MAX,
                })
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let unload = set.prepare_unload(&key).unwrap();
        let error = set.commit_batch(vec![unload]).err().unwrap();
        assert_eq!(error.code, Code::InfrastructureFailure);
        assert_eq!(error.message, "plugin revision exhausted");
        assert_eq!(
            std::error::Error::source(&error)
                .unwrap()
                .downcast_ref::<LifecycleFault>(),
            Some(&LifecycleFault::RevisionExhausted {
                current_revision: u64::MAX,
            })
        );
        assert_eq!(set.next_revision, u64::MAX);
        assert_eq!(set.snapshots().count(), 1);
        assert!(Arc::ptr_eq(
            &snapshot.loaded,
            &set.snapshot(&key).unwrap().loaded
        ));
        set.commit_batch(vec![]).unwrap();
        assert_eq!(set.next_revision, u64::MAX);
    }

    #[test]
    fn defensive_stage_faults_expose_public_data_without_changing_validation_rules() {
        use std::error::Error as _;
        let mut catalog = PluginCatalog::new();
        let key = catalog
            .register("example", |value| {
                Ok(Example {
                    value,
                    drops: Arc::default(),
                })
            })
            .unwrap();
        let mut set = catalog.instance();
        let mut staged = set.prepare_load(&key, 1).unwrap();
        Arc::get_mut(staged.candidate.as_mut().unwrap())
            .unwrap()
            .revision = 42;
        for error in [
            set.validate_batch(std::slice::from_ref(&staged))
                .unwrap_err(),
            set.candidate_snapshots(std::slice::from_ref(&staged))
                .err()
                .unwrap(),
            set.commit(staged).err().unwrap(),
        ] {
            assert_eq!(error.code, Code::InfrastructureFailure);
            assert_eq!(error.message, "candidate revision mismatch");
            assert_eq!(
                error.source().unwrap().downcast_ref::<LifecycleFault>(),
                Some(&LifecycleFault::CandidateRevisionMismatch {
                    plugin: "example".into(),
                    expected: 1,
                    actual: 42,
                })
            );
        }
        assert_eq!(set.next_revision, 0);
        assert_eq!(set.snapshots().count(), 0);

        let mut staged = set.prepare_load(&key, 1).unwrap();
        staged.expected_revision = Some(99);
        let error = set.commit(staged).err().unwrap();
        assert_eq!(error.code, Code::Denied);
        assert_eq!(
            error.source().unwrap().downcast_ref::<LifecycleFault>(),
            Some(&LifecycleFault::StaleStage {
                plugin: "example".into(),
                expected_epoch: 0,
                actual_epoch: 0,
                expected_revision: Some(99),
                actual_revision: None,
            })
        );
        assert_eq!(set.next_revision, 0);

        for operation in [LifecycleOperation::Load, LifecycleOperation::Reload] {
            set.load(&key, 1).unwrap();
            // Safe public load/reload cannot produce this malformed candidate-less stage.
            let staged = set.prepare_unload(&key).unwrap();
            let error = set.commit_candidate(staged, operation).err().unwrap();
            assert_eq!(error.code, Code::InfrastructureFailure);
            assert_eq!(
                error.message,
                match operation {
                    LifecycleOperation::Load => "loaded stage has no candidate",
                    LifecycleOperation::Reload => "reloaded stage has no candidate",
                }
            );
            assert_eq!(
                error.source().unwrap().downcast_ref::<LifecycleFault>(),
                Some(&LifecycleFault::MissingCandidate {
                    plugin: "example".into(),
                    operation,
                    revision: set.next_revision,
                })
            );
        }
    }

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
                // Application callback leaf, not a manager-generated failure.
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
    fn batch_validation_is_atomic_and_candidate_inventory_is_exact() {
        let mut catalog = PluginCatalog::new();
        let make = |value| {
            Ok(Example {
                value,
                drops: Arc::default(),
            })
        };
        let a = catalog.register("alpha", make).unwrap();
        let b = catalog.register("beta", make).unwrap();
        let mut set = catalog.instance();
        let stages = vec![
            set.prepare_load(&a, 1).unwrap(),
            set.prepare_load(&b, 2).unwrap(),
        ];
        assert_eq!(set.candidate_snapshots(&stages).unwrap().len(), 2);
        assert_eq!(set.snapshots().count(), 0);
        set.commit_batch(stages).unwrap();
        assert_eq!(set.snapshot(&a).unwrap().revision(), 1);
        assert_eq!(set.snapshot(&b).unwrap().revision(), 1);

        let foreign = catalog.instance().prepare_load(&b, 9).unwrap();
        let valid = set.prepare_reload(&a, 3).unwrap();
        assert!(set.commit_batch(vec![valid, foreign]).is_err());
        assert_eq!(set.snapshot(&a).unwrap().revision(), 1);
        let duplicate = vec![
            set.prepare_unload(&a).unwrap(),
            set.prepare_reload(&a, 4).unwrap(),
        ];
        assert!(set.candidate_snapshots(&duplicate).is_err());
        assert!(set.commit_batch(duplicate).is_err());
        assert_eq!(set.snapshot(&a).unwrap().revision(), 1);

        let stale = set.prepare_unload(&b).unwrap();
        set.reload(&a, 5).unwrap();
        let valid = set.prepare_unload(&a).unwrap();
        assert!(set.commit_batch(vec![valid, stale]).is_err());
        assert_eq!(set.snapshots().count(), 2);
        let stages = vec![
            set.prepare_unload(&a).unwrap(),
            set.prepare_unload(&b).unwrap(),
        ];
        assert!(set.candidate_snapshots(&stages).unwrap().is_empty());
        set.commit_batch(stages).unwrap();
        assert_eq!(set.snapshots().count(), 0);
        set.commit_batch(vec![]).unwrap();
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
    fn stage_from_sibling_set_is_rejected_even_with_same_catalog_and_revision() {
        let mut catalog = PluginCatalog::new();
        let key = catalog
            .register("shared", |value: i32| {
                Ok(Example {
                    value,
                    drops: Arc::new(AtomicUsize::new(0)),
                })
            })
            .unwrap();
        let mut a = catalog.instance();
        let mut b = catalog.instance();
        a.load(&key, 1).unwrap();
        b.load(&key, 2).unwrap();
        let foreign = a.prepare_reload(&key, 99).unwrap();
        assert!(b.commit(foreign).is_err());
        assert_eq!(
            b.snapshot(&key)
                .unwrap()
                .get::<GraphPatch>("calculate")
                .unwrap()
                .value,
            2
        );
        assert_eq!(
            a.snapshot(&key)
                .unwrap()
                .get::<GraphPatch>("calculate")
                .unwrap()
                .value,
            1
        );
    }

    #[test]
    fn factory_failure_does_not_touch_active_instance() {
        let mut catalog = PluginCatalog::new();
        let key = catalog
            .register("fallible", |value: i32| {
                if value < 0 {
                    // Application factory leaf, not a manager-generated failure.
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
        assert!(
            a.register("same", |_: ()| {
                Ok(Example {
                    value: 0,
                    drops: Arc::new(AtomicUsize::new(0)),
                })
            })
            .is_err()
        );
        let mut b = PluginCatalog::new().instance();
        assert!(b.load(&key, ()).is_err());
    }
}
