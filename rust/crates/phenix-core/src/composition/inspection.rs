use crate::{
    Authority, ComponentEntryTrigger, ComponentGraphError, ComponentId, ComponentManifest,
    ComponentProcessArgument, ComponentRuntimeMetadata, ConfigurationFrontendMetadata,
    GenerationId, InterfaceId, LayerPolicy, PluginExecution, PluginManifest, PluginPackageMetadata,
    ResolvedComponentGraph, ResolvedCompositionMetadata, ResolvedConfigContributions,
    ResolvedGeneration, ResolvedListener, ServiceId, SkillResourceMetadata, WorkflowDeclaration,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct ResolvedGenerationInspection {
    generation: GenerationId,
    plugins: Vec<PluginManifest>,
    components: Vec<ComponentManifest>,
    workflows: Vec<WorkflowDeclaration>,
    workflow_node_providers: BTreeMap<(ComponentId, String, String), ComponentId>,
    entry_triggers: Vec<ComponentEntryTrigger>,
    process_arguments: Vec<ComponentProcessArgument>,
    resources: Vec<SkillResourceMetadata>,
    component_graph: ResolvedComponentGraph,
    configuration: ResolvedConfigContributions,
    layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
    package_metadata: Vec<PluginPackageMetadata>,
    component_metadata: Vec<ComponentRuntimeMetadata>,
    frontend_metadata: Vec<ConfigurationFrontendMetadata>,
}

#[derive(Clone, Copy, Debug)]
pub struct ResolvedListenerInspection<'a> {
    pub generation: &'a GenerationId,
    pub listener: &'a ResolvedListener,
}

impl ResolvedGenerationInspection {
    pub fn from_resolved(resolved: &ResolvedGeneration) -> Self {
        let mut workflow_node_providers = BTreeMap::new();
        for declaration in resolved.workflows() {
            let compiled = resolved
                .generation_topology()
                .workflow(&declaration.owner, &declaration.name)
                .expect("resolved workflow declaration has compiled bindings");
            for (name, node) in &declaration.topology.nodes {
                let provider = compiled
                    .bound_import(&node.import)
                    .expect("compiled workflow node has a resolved import")
                    .exporter()
                    .clone();
                workflow_node_providers.insert(
                    (
                        declaration.owner.clone(),
                        declaration.name.clone(),
                        name.clone(),
                    ),
                    provider,
                );
            }
        }
        Self {
            generation: resolved.generation().clone(),
            plugins: resolved.plugins().to_vec(),
            components: resolved.components().to_vec(),
            workflows: resolved.workflows().to_vec(),
            workflow_node_providers,
            entry_triggers: resolved.entry_triggers().to_vec(),
            process_arguments: resolved.process_arguments().to_vec(),
            resources: resolved.resources().to_vec(),
            component_graph: resolved.component_graph().clone(),
            configuration: resolved.configuration().clone(),
            layer_policies: resolved.layer_policies().clone(),
            package_metadata: Vec::new(),
            component_metadata: Vec::new(),
            frontend_metadata: Vec::new(),
        }
    }

    pub fn from_resolved_with_metadata(
        resolved: &ResolvedGeneration,
        metadata: &ResolvedCompositionMetadata,
    ) -> Result<Self, &'static str> {
        if resolved.generation() != metadata.generation() {
            return Err("resolved metadata belongs to a different graph generation");
        }
        let mut inspection = Self::from_resolved(resolved);
        inspection.package_metadata = metadata.packages().to_vec();
        inspection.component_metadata = metadata.components().to_vec();
        inspection.frontend_metadata = metadata.frontends().to_vec();
        Ok(inspection)
    }

    pub fn generation(&self) -> &GenerationId {
        &self.generation
    }

    pub fn plugins(&self) -> &[PluginManifest] {
        &self.plugins
    }

    pub fn components(&self) -> &[ComponentManifest] {
        &self.components
    }

    /// Workflow topology selected in this immutable resolved generation.
    pub fn workflows(&self) -> &[WorkflowDeclaration] {
        &self.workflows
    }

    /// The selected provider component for one declared workflow node.
    ///
    /// This reuses the canonical resolved import handle. It cannot select a
    /// different provider or grant additional authority during inspection.
    pub fn workflow_node_provider(
        &self,
        owner: &ComponentId,
        name: &str,
        node: &str,
    ) -> Result<Option<&ComponentId>, ComponentGraphError> {
        Ok(self
            .workflow_node_providers
            .get(&(owner.clone(), name.to_owned(), node.to_owned())))
    }

    pub fn entry_triggers(&self) -> &[ComponentEntryTrigger] {
        &self.entry_triggers
    }

    pub fn process_arguments(&self) -> &[ComponentProcessArgument] {
        &self.process_arguments
    }

    pub fn resources(&self) -> &[SkillResourceMetadata] {
        &self.resources
    }

    pub fn package_metadata(&self) -> &[PluginPackageMetadata] {
        &self.package_metadata
    }

    pub fn component_metadata(&self) -> &[ComponentRuntimeMetadata] {
        &self.component_metadata
    }

    pub fn frontend_metadata(&self) -> &[ConfigurationFrontendMetadata] {
        &self.frontend_metadata
    }

    pub fn component_graph(&self) -> &ResolvedComponentGraph {
        &self.component_graph
    }

    pub fn listeners(&self) -> impl Iterator<Item = ResolvedListenerInspection<'_>> {
        self.component_graph
            .listeners()
            .map(|listener| ResolvedListenerInspection {
                generation: &self.generation,
                listener,
            })
    }

    pub fn configuration(&self) -> &ResolvedConfigContributions {
        &self.configuration
    }

    pub fn layer_policies(&self) -> &BTreeMap<ServiceId, Vec<LayerPolicy>> {
        &self.layer_policies
    }

    pub fn interposition_chain(&self, service: &ServiceId) -> &[LayerPolicy] {
        self.layer_policies
            .get(service)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn component_execution(&self, component: &ComponentId) -> Option<&PluginExecution> {
        self.component_graph
            .component(component)
            .map(|component| &component.execution)
    }

    pub fn resolved_import_provider(
        &self,
        component: &ComponentId,
        interface: &InterfaceId,
    ) -> Result<Option<&ComponentId>, ComponentGraphError> {
        Ok(self
            .component_graph
            .import_handle(component, interface)?
            .map(|handle| handle.exporter()))
    }

    pub fn requested_import_authority(
        &self,
        component: &ComponentId,
        interface: &InterfaceId,
    ) -> Option<&Authority> {
        self.components
            .iter()
            .find(|manifest| &manifest.id == component)?
            .imports
            .iter()
            .find(|import| &import.interface == interface)
            .map(|import| &import.authority)
    }

    pub fn granted_import_authority(
        &self,
        component: &ComponentId,
        interface: &InterfaceId,
    ) -> Result<Option<&Authority>, ComponentGraphError> {
        Ok(self
            .component_graph
            .import_handle(component, interface)?
            .map(|handle| handle.effective_authority()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CallableId, CompatibilityMetadata, ComponentExport, ComponentHostKind, ComponentImport,
        ComponentStateClass, CompositionMetadataInput, ConfigNamespace, ConfigurationFrontendId,
        PermissionId, PluginId, ReloadPolicy, WorkflowEdge, WorkflowNode, WorkflowTopology,
    };
    use std::collections::BTreeSet;

    fn component(value: &str) -> ComponentId {
        ComponentId::parse(value).unwrap()
    }

    fn interface(value: &str) -> InterfaceId {
        InterfaceId::parse(value).unwrap()
    }

    fn capability(value: &str) -> PermissionId {
        PermissionId::parse(value).unwrap()
    }

    fn plugin(id: &str, authority: Authority) -> PluginManifest {
        PluginManifest {
            id: PluginId::parse(id).unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: authority,
        }
    }

    fn resource() -> SkillResourceMetadata {
        SkillResourceMetadata {
            identity: "fixture.skill".into(),
            version: 1,
            content_identity: "sha256:fixture".into(),
            dependencies: BTreeSet::new(),
            conflicts: BTreeSet::new(),
            triggers: BTreeSet::from(["fixture".into()]),
            scope: "execution".into(),
            priority: 1,
            required_tools: BTreeSet::from([CallableId::parse("read").unwrap()]),
            required_interfaces: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
            compatibility: CompatibilityMetadata {
                minimum_kernel_version: 1,
                maximum_kernel_version: None,
            },
            invalidation_targets: BTreeSet::from(["skill-index".into()]),
            reload_policy: ReloadPolicy::Restart,
        }
    }

    #[test]
    fn workflow_inspection_reuses_the_compiled_provider_binding() {
        let plugin = plugin("fixture.workflow-inspection", Authority::default());
        let consumer = component("fixture.workflow-inspection.consumer");
        let provider = component("fixture.workflow-inspection.provider");
        let interface = interface("fixture.workflow-step@1");
        let resolved = ResolvedGeneration::resolve_with_resources(
            [plugin.clone()],
            [
                ComponentManifest {
                    id: consumer.clone(),
                    owner: plugin.id.clone(),
                    imports: vec![ComponentImport {
                        interface: interface.clone(),
                        schema: Default::default(),
                        required: true,
                        authority: Authority::default(),
                    }],
                    exports: Vec::new(),
                    listeners: Vec::new(),
                    maximum_authority: Authority::default(),
                },
                ComponentManifest {
                    id: provider.clone(),
                    owner: plugin.id,
                    imports: Vec::new(),
                    exports: vec![ComponentExport {
                        interface: interface.clone(),
                        schema: Default::default(),
                        priority: 10,
                        required_authority: Authority::default(),
                    }],
                    listeners: Vec::new(),
                    maximum_authority: Authority::default(),
                },
            ],
            [],
            [],
            &Authority::default(),
        )
        .unwrap()
        .with_workflows([WorkflowDeclaration {
            owner: consumer.clone(),
            name: "run".into(),
            topology: WorkflowTopology {
                entry: "step".into(),
                nodes: BTreeMap::from([(
                    "step".into(),
                    WorkflowNode {
                        import: interface.clone(),
                        branches: BTreeMap::from([("done".into(), WorkflowEdge::Finish)]),
                    },
                )]),
            },
        }])
        .unwrap();

        let compiled = resolved
            .generation_topology()
            .workflow(&consumer, "run")
            .unwrap();
        let expected = compiled.bound_import(&interface).unwrap().exporter();
        let inspection = ResolvedGenerationInspection::from_resolved(&resolved);
        assert_eq!(
            inspection
                .workflow_node_provider(&consumer, "run", "step")
                .unwrap(),
            Some(expected)
        );
        assert_eq!(expected, &provider);
        assert!(
            inspection
                .workflow_node_provider(&consumer, "run", "missing")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn resolved_metadata_is_inspectable_without_activating_plugin_behavior() {
        let plugin = PluginManifest::resource_only(PluginId::parse("fixture.resources").unwrap());
        let resolved = ResolvedGeneration::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource()],
            [],
            &Authority::default(),
        )
        .unwrap();

        let inspection = ResolvedGenerationInspection::from_resolved(&resolved);

        assert_eq!(inspection.generation(), resolved.generation());
        assert_eq!(inspection.plugins(), &[plugin]);
        assert!(inspection.components().is_empty());
        assert!(inspection.process_arguments().is_empty());
        assert_eq!(inspection.resources()[0].identity, "fixture.skill");
        assert_eq!(inspection.resources()[0].content_identity, "sha256:fixture");
        assert_eq!(inspection.component_graph().components().count(), 0);
        assert!(inspection.configuration().entries().is_empty());
        assert!(inspection.layer_policies().is_empty());
        assert!(inspection.package_metadata().is_empty());
        assert!(inspection.component_metadata().is_empty());
        assert!(inspection.frontend_metadata().is_empty());
    }

    #[test]
    fn inspection_exposes_process_arguments_from_the_resolved_generation() {
        let plugin_id = PluginId::parse("fixture.process-arguments").unwrap();
        let component_id = component("fixture.process-arguments.component");
        let interface_id = interface("fixture.process-arguments@1");
        let plugin = plugin("fixture.process-arguments", Authority::default());
        let component = ComponentManifest {
            listeners: Vec::new(),
            id: component_id.clone(),
            owner: plugin_id,
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: interface_id.clone(),
                schema: Default::default(),
                priority: 0,
                required_authority: Authority::default(),
            }],
            maximum_authority: Authority::default(),
        };
        let argument = ComponentProcessArgument {
            component: component_id,
            interface: interface_id,
            name: "--fixture-value".into(),
            takes_value: true,
            description: "Fixture process argument".into(),
            required_authority: Authority::default(),
        };
        let resolved =
            ResolvedGeneration::resolve_with_durable_schemas_layer_policies_entry_triggers_and_process_arguments(
                [plugin],
                [component],
                [],
                [],
                [argument.clone()],
                [],
                BTreeMap::new(),
                &Authority::default(),
            )
            .unwrap();

        let inspection = ResolvedGenerationInspection::from_resolved(&resolved);

        assert_eq!(inspection.process_arguments(), &[argument]);
    }

    #[test]
    fn inspection_can_attach_validated_rich_metadata_for_the_same_generation() {
        let plugin_id = PluginId::parse("fixture.rich").unwrap();
        let component_id = component("fixture.rich.component");
        let frontend_id = ConfigurationFrontendId::parse("fixture.rich.config").unwrap();
        let package = PluginPackageMetadata {
            manifest: plugin("fixture.rich", Authority::default()),
            packaged_components: BTreeSet::from([component_id.clone()]),
            packaged_resources: BTreeSet::new(),
            packaged_skills: BTreeSet::new(),
            compatibility: CompatibilityMetadata {
                minimum_kernel_version: 1,
                maximum_kernel_version: None,
            },
            durable_namespaces: BTreeSet::new(),
            migrations: Vec::new(),
            configuration_frontends: BTreeSet::from([frontend_id.clone()]),
            component_hosts: BTreeSet::from([ComponentHostKind::EmbeddedRust]),
            reload_policy: ReloadPolicy::DrainAndRestart,
        };
        let component_metadata = ComponentRuntimeMetadata {
            manifest: ComponentManifest {
                listeners: Vec::new(),
                id: component_id.clone(),
                owner: plugin_id,
                imports: Vec::new(),
                exports: Vec::new(),
                maximum_authority: Authority::default(),
            },
            version: 2,
            configuration_contracts: BTreeSet::from([
                ConfigNamespace::parse("fixture.rich@1").unwrap()
            ]),
            requested_capabilities: BTreeSet::new(),
            state_class: ComponentStateClass::Durable,
            reload_policy: ReloadPolicy::MigrationRequired,
            interposition_interfaces: BTreeSet::new(),
            event_contributions: BTreeSet::new(),
            controller_contributions: BTreeSet::new(),
        };
        let frontend = ConfigurationFrontendMetadata {
            id: frontend_id,
            version: 3,
            accepted_source_kinds: BTreeSet::from(["fixture".into()]),
            exposed_namespaces: BTreeSet::from([ConfigNamespace::parse("fixture.rich@1").unwrap()]),
            watch: true,
            required_authority: Authority::default(),
        };
        let (resolved, metadata) = CompositionMetadataInput {
            packages: vec![package],
            components: vec![component_metadata],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
        .resolve_frontends_inspectable([frontend], [], &Authority::default())
        .unwrap();

        let inspection =
            ResolvedGenerationInspection::from_resolved_with_metadata(&resolved, &metadata)
                .unwrap();

        assert_eq!(inspection.generation(), metadata.generation());
        assert_eq!(
            inspection.package_metadata()[0].reload_policy,
            ReloadPolicy::DrainAndRestart
        );
        assert_eq!(
            inspection.component_metadata()[0].state_class,
            ComponentStateClass::Durable
        );
        assert_eq!(inspection.frontend_metadata()[0].version, 3);
    }

    #[test]
    fn inspection_exposes_provider_and_requested_vs_granted_import_authority() {
        let read = capability("workspace.read");
        let write = capability("workspace.write");
        let provider = plugin(
            "provider-plugin",
            Authority::new([read.clone(), write.clone()]),
        );
        let consumer = plugin(
            "consumer-plugin",
            Authority::new([read.clone(), write.clone()]),
        );
        let interface = interface("fixture.inspect@1");
        let components = [
            ComponentManifest {
                listeners: Vec::new(),
                id: component("provider"),
                owner: provider.id.clone(),
                imports: Vec::new(),
                exports: vec![ComponentExport {
                    interface: interface.clone(),
                    schema: Default::default(),
                    priority: 10,
                    required_authority: Authority::new([read.clone()]),
                }],
                maximum_authority: Authority::new([read.clone()]),
            },
            ComponentManifest {
                listeners: Vec::new(),
                id: component("consumer"),
                owner: consumer.id.clone(),
                imports: vec![ComponentImport {
                    interface: interface.clone(),
                    schema: Default::default(),
                    required: true,
                    authority: Authority::new([read.clone(), write.clone()]),
                }],
                exports: Vec::new(),
                maximum_authority: Authority::new([read.clone(), write.clone()]),
            },
        ];
        let resolved = ResolvedGeneration::resolve(
            [provider, consumer],
            components,
            [],
            &Authority::new([read.clone(), write.clone()]),
        )
        .unwrap();

        let inspection = ResolvedGenerationInspection::from_resolved(&resolved);
        let handle = inspection
            .component_graph()
            .import_handle(&component("consumer"), &interface)
            .unwrap()
            .unwrap();
        let provider = inspection
            .resolved_import_provider(&component("consumer"), &interface)
            .unwrap()
            .unwrap();
        let requested = inspection
            .requested_import_authority(&component("consumer"), &interface)
            .unwrap();
        let granted = inspection
            .granted_import_authority(&component("consumer"), &interface)
            .unwrap()
            .unwrap();

        assert_eq!(provider, &component("provider"));
        assert_eq!(handle.exporter(), provider);
        assert!(requested.permits(&read));
        assert!(requested.permits(&write));
        assert!(granted.permits(&read));
        assert!(!granted.permits(&write));
        assert_eq!(granted, handle.effective_authority());
    }

    #[test]
    fn inspection_exposes_resolved_interposition_chain() {
        let service = ServiceId::parse("fixture.layered@1").unwrap();
        let first = PluginId::parse("layer.first").unwrap();
        let second = PluginId::parse("layer.second").unwrap();
        let layer_plugin = |id: PluginId, priority: i32| PluginManifest {
            id,
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![crate::ServiceContribution {
                role: crate::ServiceRole::Layer,
                service: service.clone(),
                priority,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let resolved = ResolvedGeneration::resolve_with_layer_policies(
            [
                layer_plugin(first.clone(), 20),
                layer_plugin(second.clone(), 10),
            ],
            [],
            [],
            BTreeMap::from([(
                service.clone(),
                vec![
                    LayerPolicy {
                        plugin: first.clone(),
                        priority: 20,
                        required: true,
                        enabled: true,
                    },
                    LayerPolicy {
                        plugin: second.clone(),
                        priority: 10,
                        required: false,
                        enabled: true,
                    },
                ],
            )]),
            &Authority::default(),
        )
        .unwrap();

        let inspection = ResolvedGenerationInspection::from_resolved(&resolved);
        let chain = inspection.interposition_chain(&service);

        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].plugin, first);
        assert_eq!(chain[1].plugin, second);
        assert!(
            inspection
                .interposition_chain(&ServiceId::parse("fixture.missing@1").unwrap())
                .is_empty()
        );
    }

    #[test]
    fn inspection_exposes_runtime_component_execution_kind_before_activation() {
        let runtime = crate::PluginRuntimeId::parse("vendor.runtime").unwrap();
        let bridge = PluginManifest {
            id: PluginId::parse("runtime-bridge").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![crate::ServiceContribution {
                service: crate::plugin_runtime_adapter_service(&runtime),
                role: crate::ServiceRole::Terminal,
                priority: 0,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let provider = PluginManifest {
            id: PluginId::parse("runtime-guest").unwrap(),
            version: 1,
            execution: PluginExecution::Runtime {
                runtime: runtime.clone(),
                artifact: crate::PluginArtifact {
                    locator: "plugin.wasm".into(),
                    revision: crate::ArtifactRevision::from_content(b"fixture"),
                    configuration: BTreeMap::new(),
                },
            },
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let component_id = component("runtime-guest");
        let resolved = ResolvedGeneration::resolve(
            [bridge, provider.clone()],
            [ComponentManifest {
                listeners: Vec::new(),
                id: component_id.clone(),
                owner: provider.id,
                imports: Vec::new(),
                exports: Vec::new(),
                maximum_authority: Authority::default(),
            }],
            [],
            &Authority::default(),
        )
        .unwrap();

        let inspection = ResolvedGenerationInspection::from_resolved(&resolved);
        assert_eq!(
            inspection.component_execution(&component_id),
            Some(&PluginExecution::Runtime {
                runtime: crate::PluginRuntimeId::parse("vendor.runtime").unwrap(),
                artifact: crate::PluginArtifact {
                    locator: "plugin.wasm".into(),
                    revision: crate::ArtifactRevision::from_content(b"fixture"),
                    configuration: BTreeMap::new(),
                },
            })
        );
    }

    #[test]
    fn workflow_inspection_reports_declared_topology_and_canonical_binding() {
        use crate::{WorkflowEdge, WorkflowNode, WorkflowTopology};

        let interface = InterfaceId::parse("fixture.workflow.invoke@1").unwrap();
        let owner = component("fixture.workflow.owner");
        let provider = component("fixture.workflow.provider");
        let consumer_plugin = plugin("fixture.workflow.consumer-plugin", Authority::default());
        let provider_plugin = plugin("fixture.workflow.provider-plugin", Authority::default());
        let manifests = [
            ComponentManifest {
                id: owner.clone(),
                owner: consumer_plugin.id.clone(),
                imports: vec![ComponentImport {
                    interface: interface.clone(),
                    schema: Default::default(),
                    required: true,
                    authority: Authority::default(),
                }],
                exports: Vec::new(),
                listeners: Vec::new(),
                maximum_authority: Authority::default(),
            },
            ComponentManifest {
                id: provider.clone(),
                owner: provider_plugin.id.clone(),
                imports: Vec::new(),
                exports: vec![ComponentExport {
                    interface: interface.clone(),
                    schema: Default::default(),
                    priority: 0,
                    required_authority: Authority::default(),
                }],
                listeners: Vec::new(),
                maximum_authority: Authority::default(),
            },
        ];
        let declaration = WorkflowDeclaration {
            owner: owner.clone(),
            name: "fixture.workflow".into(),
            topology: WorkflowTopology {
                entry: "invoke".into(),
                nodes: BTreeMap::from([(
                    "invoke".into(),
                    WorkflowNode {
                        import: interface.clone(),
                        branches: BTreeMap::from([("complete".into(), WorkflowEdge::Finish)]),
                    },
                )]),
            },
        };
        let resolved = ResolvedGeneration::resolve(
            [consumer_plugin, provider_plugin],
            manifests,
            [],
            &Authority::default(),
        )
        .unwrap()
        .with_workflows([declaration.clone()])
        .unwrap();
        let inspection = ResolvedGenerationInspection::from_resolved(&resolved);

        assert_eq!(inspection.workflows(), &[declaration]);
        assert_eq!(
            inspection
                .workflow_node_provider(&owner, "fixture.workflow", "invoke")
                .unwrap(),
            Some(&provider),
        );
        assert_eq!(
            inspection
                .workflow_node_provider(&owner, "fixture.workflow", "missing")
                .unwrap(),
            None,
        );
    }
}
