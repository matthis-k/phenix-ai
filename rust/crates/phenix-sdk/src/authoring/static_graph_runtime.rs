use super::{StaticPluginDefinition, StaticPluginGraph};

impl StaticPluginGraph {
    fn verify_prepared_manifest(
        &self,
        kernel: &phenix_core::Kernel,
        id: &phenix_core::PluginId,
    ) -> Result<(), phenix_core::KernelError> {
        let descriptor = self
            .descriptor(id)
            .expect("selected static graph IDs have frozen descriptors");
        let manifest = kernel
            .config()
            .manifest(id)
            .ok_or_else(|| phenix_core::KernelError::UnknownPlugin(id.clone()))?;
        let mut selected_dependencies = manifest.dependencies.clone();
        selected_dependencies.sort();
        if descriptor.version != manifest.version
            || descriptor.execution != manifest.execution
            || descriptor.maximum_authority != manifest.maximum_authority
            || self.prepared_dependencies(id) != Some(selected_dependencies.as_slice())
        {
            return Err(phenix_core::KernelError::PreparedPluginMismatch(id.clone()));
        }
        Ok(())
    }

    fn verify_prepared_graph(
        &self,
        kernel: &phenix_core::Kernel,
    ) -> Result<(), phenix_core::KernelError> {
        // An explicitly constructed instance must not bypass checks of other
        // selected plugins. A candidate is one atomic validated graph.
        for id in self.ids() {
            self.verify_prepared_manifest(kernel, id)?;
        }
        Ok(())
    }

    /// Preload every reusable zero-input embedded factory carried by this static graph.
    ///
    /// An embedded descriptor without a factory is not invalid. It represents state that must be
    /// constructed explicitly and prepared with `preload_embedded_instance`.
    pub fn preload_embedded_factories(
        &self,
        kernel: &mut phenix_core::Kernel,
    ) -> Result<(), phenix_core::KernelError> {
        // Check the entire candidate before installing any factory. The
        // selected runtime manifest must match the graph's frozen metadata.
        self.verify_prepared_graph(kernel)?;
        for id in self.ids() {
            let descriptor = self
                .descriptor(id)
                .expect("static graph ids always resolve to descriptors");
            match (&descriptor.execution, descriptor.embedded_factory) {
                (phenix_core::PluginExecution::Embedded, Some(factory)) => {
                    kernel.preload_embedded_factory(id.clone(), factory);
                }
                (phenix_core::PluginExecution::Embedded, None) | (_, None) => {}
                (_, Some(_)) => {
                    return Err(phenix_core::KernelError::WrongExecutionKind(id.clone()));
                }
            }
        }
        Ok(())
    }

    /// Prepare an already-constructed stateful plugin for normal kernel activation.
    ///
    /// Construction remains ordinary Rust. This method only performs the generic type-to-runtime
    /// boundary and derives the plugin identity from the authored type.
    #[doc(hidden)]
    pub fn preload_embedded_instance<T: StaticPluginDefinition>(
        &self,
        kernel: &mut phenix_core::Kernel,
        instance: Box<dyn phenix_core::PluginInstance>,
    ) -> Result<(), phenix_core::KernelError> {
        // Runtime preload uses the selected snapshot, not another read of
        // potentially stateful author descriptors or their dependencies.
        let descriptor = self.prepared_descriptor::<T>().ok_or_else(|| {
            phenix_core::KernelError::UnselectedPluginType(std::any::type_name::<T>())
        })?;
        let id = descriptor.id.clone();
        self.verify_prepared_graph(kernel)?;
        if !matches!(descriptor.execution, phenix_core::PluginExecution::Embedded) {
            return Err(phenix_core::KernelError::WrongExecutionKind(id));
        }
        kernel.preload_embedded_instance(id, instance);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Authority, PluginExecution, PluginId, StaticPluginComponents, StaticPluginDependency,
        StaticPluginDescriptor, StaticPluginResources,
    };

    struct Instance;

    impl phenix_core::PluginInstance for Instance {
        fn start(&mut self, _host: &phenix_core::PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }
    }

    fn factory() -> Box<dyn phenix_core::PluginInstance> {
        Box::new(Instance)
    }

    struct Leaf;
    struct Root;
    struct ExplicitState;
    struct UnselectedImpostor;
    struct UnselectedCallback;
    struct ChangedMetadata;

    static PLUGIN_VERSION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    static PLUGIN_DEPENDENCY_CHANGE: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    static PLUGIN_DESCRIPTOR_READS: std::sync::atomic::AtomicU32 =
        std::sync::atomic::AtomicU32::new(0);

    impl StaticPluginDefinition for ChangedMetadata {
        fn descriptor() -> StaticPluginDescriptor {
            PLUGIN_DESCRIPTOR_READS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut authored = descriptor("fixture.graph.changed-metadata", Vec::new());
            authored.embedded_factory = None;
            authored.version = PLUGIN_VERSION.load(std::sync::atomic::Ordering::SeqCst);
            if PLUGIN_DEPENDENCY_CHANGE.load(std::sync::atomic::Ordering::SeqCst) {
                authored
                    .dependencies
                    .push(StaticPluginDependency::of::<Leaf>());
            }
            authored
        }
    }

    impl StaticPluginDefinition for UnselectedImpostor {
        fn descriptor() -> StaticPluginDescriptor {
            // Same id and claimed definition, but the type is not in the
            // selected static graph and must never preload executable code.
            let mut authored = descriptor("fixture.graph.explicit-state", Vec::new());
            authored.embedded_factory = None;
            authored
        }
    }

    impl StaticPluginDefinition for UnselectedCallback {
        fn descriptor() -> StaticPluginDescriptor {
            panic!("an unselected type must not execute author code at preload")
        }
    }

    impl StaticPluginDefinition for Leaf {
        fn descriptor() -> StaticPluginDescriptor {
            descriptor("fixture.graph.leaf", Vec::new())
        }
    }

    impl StaticPluginDefinition for Root {
        fn descriptor() -> StaticPluginDescriptor {
            descriptor(
                "fixture.graph.root",
                vec![StaticPluginDependency::of::<Leaf>()],
            )
        }
    }

    impl StaticPluginDefinition for ExplicitState {
        fn descriptor() -> StaticPluginDescriptor {
            let mut descriptor = descriptor("fixture.graph.explicit-state", Vec::new());
            descriptor.embedded_factory = None;
            descriptor
        }
    }

    impl StaticPluginComponents for Leaf {
        fn components() -> Vec<crate::StaticComponentDescriptor> {
            Vec::new()
        }
    }

    impl StaticPluginComponents for Root {
        fn components() -> Vec<crate::StaticComponentDescriptor> {
            Vec::new()
        }
    }

    impl StaticPluginComponents for ExplicitState {
        fn components() -> Vec<crate::StaticComponentDescriptor> {
            Vec::new()
        }
    }

    impl StaticPluginResources for Leaf {
        fn resources() -> Vec<crate::StaticResourceDescriptor> {
            Vec::new()
        }
    }

    impl StaticPluginResources for Root {
        fn resources() -> Vec<crate::StaticResourceDescriptor> {
            Vec::new()
        }
    }

    impl StaticPluginResources for ExplicitState {
        fn resources() -> Vec<crate::StaticResourceDescriptor> {
            Vec::new()
        }
    }

    fn descriptor(
        id: &'static str,
        dependencies: Vec<StaticPluginDependency>,
    ) -> StaticPluginDescriptor {
        StaticPluginDescriptor {
            id: PluginId::parse(id).unwrap(),
            definition: id,
            version: 1,
            execution: PluginExecution::Embedded,
            maximum_authority: Authority::default(),
            dependencies,
            embedded_factory: Some(factory),
            contributions: || Ok(Default::default()),
        }
    }

    #[test]
    fn graph_preloads_transitive_embedded_factories() {
        let graph = StaticPluginGraph::compose::<Root>().unwrap();
        let manifests = [Leaf::manifest(), Root::manifest()];
        let mut kernel =
            phenix_core::Kernel::new(phenix_core::KernelConfig::new(manifests).unwrap());

        graph.preload_embedded_factories(&mut kernel).unwrap();
        kernel.activate_all().unwrap();
    }

    #[test]
    fn factory_preload_rejects_stale_manifest_before_installing_any_factories() {
        let graph = StaticPluginGraph::compose::<Root>().unwrap();
        let leaf = Leaf::manifest();
        let leaf_id = leaf.id.clone();
        let mut root = Root::manifest();
        let root_id = root.id.clone();
        root.version += 1;
        let mut kernel =
            phenix_core::Kernel::new(phenix_core::KernelConfig::new([leaf, root]).unwrap());

        assert_eq!(
            graph.preload_embedded_factories(&mut kernel),
            Err(phenix_core::KernelError::PreparedPluginMismatch(root_id))
        );
        assert_eq!(
            kernel.activate_all(),
            Err(phenix_core::KernelError::EmbeddedFactoryMissing(leaf_id))
        );
    }

    #[test]
    fn preloading_validates_frozen_dependency_and_authority_metadata() {
        let graph = StaticPluginGraph::compose::<Root>().unwrap();
        let leaf = Leaf::manifest();
        let mut root = Root::manifest();
        let root_id = root.id.clone();
        root.dependencies.clear();
        let mut kernel =
            phenix_core::Kernel::new(phenix_core::KernelConfig::new([leaf.clone(), root]).unwrap());
        assert_eq!(
            graph.preload_embedded_factories(&mut kernel),
            Err(phenix_core::KernelError::PreparedPluginMismatch(root_id))
        );

        let graph = StaticPluginGraph::compose::<ExplicitState>().unwrap();
        let mut state = ExplicitState::manifest();
        let id = state.id.clone();
        state.maximum_authority =
            Authority::new([crate::PermissionId::parse("fixture.extra").unwrap()]);
        let mut kernel = phenix_core::Kernel::new(phenix_core::KernelConfig::new([state]).unwrap());
        assert_eq!(
            graph.preload_embedded_instance::<ExplicitState>(&mut kernel, Box::new(Instance)),
            Err(phenix_core::KernelError::PreparedPluginMismatch(id.clone()))
        );
        assert_eq!(
            kernel.activate_all(),
            Err(phenix_core::KernelError::EmbeddedFactoryMissing(id))
        );
    }

    #[test]
    fn explicit_preload_rejects_a_stale_sibling_before_mutating_candidate() {
        let graph = StaticPluginGraph::compose::<Root>().unwrap();
        let leaf = Leaf::manifest();
        let leaf_id = leaf.id.clone();
        let mut root = Root::manifest();
        let root_id = root.id.clone();
        root.version += 1;
        let mut kernel =
            phenix_core::Kernel::new(phenix_core::KernelConfig::new([leaf, root]).unwrap());

        // Leaf itself is valid, but Root belongs to the same selected graph.
        // Installing Leaf before checking Root would be a partial mutation.
        assert_eq!(
            graph.preload_embedded_instance::<Leaf>(&mut kernel, Box::new(Instance)),
            Err(phenix_core::KernelError::PreparedPluginMismatch(root_id))
        );
        assert_eq!(
            kernel.activate_all(),
            Err(phenix_core::KernelError::EmbeddedFactoryMissing(leaf_id))
        );
    }

    #[test]
    fn explicit_instance_preload_uses_frozen_metadata_without_author_callbacks() {
        use std::sync::atomic::Ordering;

        PLUGIN_VERSION.store(1, Ordering::SeqCst);
        PLUGIN_DEPENDENCY_CHANGE.store(false, Ordering::SeqCst);
        PLUGIN_DESCRIPTOR_READS.store(0, Ordering::SeqCst);

        let graph = StaticPluginGraph::compose::<ChangedMetadata>().unwrap();
        assert_eq!(PLUGIN_DESCRIPTOR_READS.load(Ordering::SeqCst), 1);

        let id = PluginId::parse("fixture.graph.changed-metadata").unwrap();
        let mut kernel = phenix_core::Kernel::new(
            phenix_core::KernelConfig::new([phenix_core::PluginManifest {
                id,
                version: 1,
                execution: PluginExecution::Embedded,
                dependencies: Vec::new(),
                services: Vec::new(),
                resource_namespaces: Vec::new(),
                maximum_authority: Authority::default(),
            }])
            .unwrap(),
        );

        // The prepared candidate retains the original descriptor even if
        // callbacks would now report a new version or dependency closure.
        PLUGIN_VERSION.store(2, Ordering::SeqCst);
        PLUGIN_DEPENDENCY_CHANGE.store(true, Ordering::SeqCst);
        graph
            .preload_embedded_instance::<ChangedMetadata>(&mut kernel, Box::new(Instance))
            .unwrap();
        assert_eq!(PLUGIN_DESCRIPTOR_READS.load(Ordering::SeqCst), 1);
        PLUGIN_VERSION.store(1, Ordering::SeqCst);
        PLUGIN_DEPENDENCY_CHANGE.store(false, Ordering::SeqCst);

        kernel.activate_all().unwrap();
    }

    #[test]
    fn embedded_graph_accepts_explicitly_constructed_state() {
        let graph = StaticPluginGraph::compose::<ExplicitState>().unwrap();
        let id = PluginId::parse("fixture.graph.explicit-state").unwrap();
        let mut kernel = phenix_core::Kernel::new(
            phenix_core::KernelConfig::new([phenix_core::PluginManifest {
                id: id.clone(),
                version: 1,
                execution: PluginExecution::Embedded,
                dependencies: Vec::new(),
                services: Vec::new(),
                resource_namespaces: Vec::new(),
                maximum_authority: Authority::default(),
            }])
            .unwrap(),
        );

        graph.preload_embedded_factories(&mut kernel).unwrap();
        assert_eq!(
            kernel.activate_all(),
            Err(phenix_core::KernelError::EmbeddedFactoryMissing(id.clone()))
        );

        assert_eq!(
            graph.preload_embedded_instance::<UnselectedImpostor>(&mut kernel, Box::new(Instance),),
            Err(phenix_core::KernelError::UnselectedPluginType(
                std::any::type_name::<UnselectedImpostor>()
            ))
        );
        assert_eq!(
            graph.preload_embedded_instance::<UnselectedCallback>(&mut kernel, Box::new(Instance)),
            Err(phenix_core::KernelError::UnselectedPluginType(
                std::any::type_name::<UnselectedCallback>()
            ))
        );
        assert_eq!(
            kernel.activate_all(),
            Err(phenix_core::KernelError::EmbeddedFactoryMissing(id.clone()))
        );

        graph
            .preload_embedded_instance::<ExplicitState>(&mut kernel, Box::new(Instance))
            .unwrap();
        kernel.activate_all().unwrap();
    }
}
