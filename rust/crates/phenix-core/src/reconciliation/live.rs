use crate::{
    GraphGenerationId, GraphReconciler, Kernel, LayerPolicy, MetadataReconciliationError, PluginId,
    PluginManifest, ReconciliationResult, ResolvedCompositionMetadata, ResolvedHarness,
    ResolvedHarnessActivationError, ServiceId,
};

#[derive(Clone, Debug, PartialEq)]
pub enum LiveReconciliationError {
    NoActiveGeneration,
    ActiveGenerationMismatch {
        kernel: GraphGenerationId,
        reconciler: GraphGenerationId,
    },
    KernelConfigurationMismatch {
        kernel_plugins: Vec<PluginId>,
        resolved_plugins: Vec<PluginId>,
    },
    KernelPluginManifestMismatch {
        plugin: PluginId,
        kernel_manifest: Box<PluginManifest>,
        resolved_manifest: Box<PluginManifest>,
    },
    KernelLayerPolicyMismatch {
        service: ServiceId,
        kernel_layers: Vec<LayerPolicy>,
        resolved_layers: Vec<LayerPolicy>,
    },
    MetadataPolicy(MetadataReconciliationError),
    Runtime(crate::KernelError),
}

impl GraphReconciler {
    pub(crate) fn preflight_live_reconciliation(
        &self,
        kernel: &Kernel,
    ) -> Result<(), LiveReconciliationError> {
        validate_live_reconciliation(self, kernel)
    }

    /// Apply one fully resolved development candidate to a live kernel.
    ///
    /// Candidate resolution happens before this operation. The method verifies that
    /// the live kernel still represents the reconciler's active generation and that
    /// package/layer policy still matches the resolved candidate. Only then are the
    /// generation identity, component graph, and resources replaced together.
    pub fn activate_candidate_on_kernel(
        &mut self,
        kernel: &mut Kernel,
        candidate: ResolvedHarness,
    ) -> Result<ReconciliationResult, LiveReconciliationError> {
        self.preflight_live_reconciliation(kernel)?;
        let preview = self.preview_candidate(&candidate);
        let restart_plugins =
            restart_plugins_for_plan(self.active(), &candidate, &preview.transition_plan);
        kernel
            .reconcile_resolved_generation(&candidate, &restart_plugins)
            .map_err(LiveReconciliationError::Runtime)?;
        Ok(self.activate_candidate(candidate))
    }

    /// Apply a fully resolved development candidate with its pre-activation metadata.
    ///
    /// This is the canonical activation path when composition metadata participates in
    /// reconciliation. Reload, drain, and migration policy is evaluated before either
    /// the reconciler or live kernel generation changes.
    pub fn activate_candidate_on_kernel_with_metadata(
        &mut self,
        kernel: &mut Kernel,
        active_metadata: &ResolvedCompositionMetadata,
        candidate: ResolvedHarness,
        candidate_metadata: &ResolvedCompositionMetadata,
    ) -> Result<ReconciliationResult, LiveReconciliationError> {
        self.preflight_live_reconciliation(kernel)?;
        let preview = self
            .preview_candidate_with_metadata(active_metadata, &candidate, candidate_metadata)
            .map_err(LiveReconciliationError::MetadataPolicy)?;
        let restart_plugins =
            restart_plugins_for_plan(self.active(), &candidate, &preview.graph.transition_plan);
        kernel
            .reconcile_resolved_generation(&candidate, &restart_plugins)
            .map_err(LiveReconciliationError::Runtime)?;
        let mut result = self.activate_candidate(candidate);
        result.transition_plan = preview.graph.transition_plan;
        Ok(result)
    }
}

fn validate_live_reconciliation(
    reconciler: &GraphReconciler,
    kernel: &Kernel,
) -> Result<(), LiveReconciliationError> {
    let kernel_generation = kernel
        .graph_generation()
        .cloned()
        .ok_or(LiveReconciliationError::NoActiveGeneration)?;
    if &kernel_generation != reconciler.active().generation() {
        return Err(LiveReconciliationError::ActiveGenerationMismatch {
            kernel: kernel_generation,
            reconciler: reconciler.active().generation().clone(),
        });
    }

    crate::composition::activation::validate_resolved_harness_configuration(
        kernel,
        reconciler.active(),
    )
    .map_err(map_activation_validation_error)
}

fn restart_plugins_for_plan(
    active: &ResolvedHarness,
    candidate: &ResolvedHarness,
    plan: &[crate::ReconciliationAction],
) -> std::collections::BTreeSet<PluginId> {
    plan.iter()
        .filter_map(|action| match action {
            crate::ReconciliationAction::ActivateComponent(component)
            | crate::ReconciliationAction::StopComponent(component)
            | crate::ReconciliationAction::RestartComponent(component) => candidate
                .components()
                .iter()
                .chain(active.components().iter())
                .find(|manifest| &manifest.id == component)
                .map(|manifest| manifest.owner.clone()),
            _ => None,
        })
        .collect()
}

fn map_activation_validation_error(
    error: ResolvedHarnessActivationError,
) -> LiveReconciliationError {
    match error {
        ResolvedHarnessActivationError::KernelConfigurationMismatch {
            kernel_plugins,
            resolved_plugins,
        } => LiveReconciliationError::KernelConfigurationMismatch {
            kernel_plugins,
            resolved_plugins,
        },
        ResolvedHarnessActivationError::KernelPluginManifestMismatch {
            plugin,
            kernel_manifest,
            resolved_manifest,
        } => LiveReconciliationError::KernelPluginManifestMismatch {
            plugin,
            kernel_manifest,
            resolved_manifest,
        },
        ResolvedHarnessActivationError::KernelLayerPolicyMismatch {
            service,
            kernel_layers,
            resolved_layers,
        } => LiveReconciliationError::KernelLayerPolicyMismatch {
            service,
            kernel_layers,
            resolved_layers,
        },
        ResolvedHarnessActivationError::DifferentGenerationAlreadyActive { .. } => {
            unreachable!("configuration validation does not inspect active generation")
        }
        ResolvedHarnessActivationError::DurableSchemaPreparation(_) => {
            unreachable!("configuration validation does not prepare durable schemas")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Authority, CompatibilityMetadata, ComponentHostKind, ComponentManifest,
        ComponentRuntimeMetadata, ComponentStateClass, CompositionMetadataInput, PluginExecution,
        PluginPackageMetadata, ReloadPolicy, ResolvedHarnessActivation, SkillResourceMetadata,
    };
    use std::collections::BTreeSet;

    fn resource(content_identity: &str) -> SkillResourceMetadata {
        SkillResourceMetadata {
            identity: "fixture.skill".into(),
            version: 1,
            content_identity: content_identity.into(),
            dependencies: BTreeSet::new(),
            conflicts: BTreeSet::new(),
            triggers: BTreeSet::new(),
            scope: "execution".into(),
            priority: 0,
            required_tools: BTreeSet::new(),
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

    fn metadata_fixture(reload_policy: ReloadPolicy) -> CompositionMetadataInput {
        let plugin = PluginId::parse("fixture.resources").unwrap();
        let component = crate::ComponentId::parse("fixture.component").unwrap();
        CompositionMetadataInput {
            packages: vec![PluginPackageMetadata {
                manifest: PluginManifest {
                    id: plugin.clone(),
                    version: 1,
                    execution: PluginExecution::Embedded,
                    dependencies: Vec::new(),
                    services: Vec::new(),
                    resource_namespaces: Vec::new(),
                    maximum_authority: Authority::default(),
                },
                packaged_components: BTreeSet::from([component.clone()]),
                packaged_resources: BTreeSet::new(),
                packaged_skills: BTreeSet::new(),
                compatibility: CompatibilityMetadata {
                    minimum_kernel_version: 1,
                    maximum_kernel_version: None,
                },
                durable_namespaces: BTreeSet::new(),
                migrations: Vec::new(),
                configuration_frontends: BTreeSet::new(),
                component_hosts: BTreeSet::from([ComponentHostKind::EmbeddedRust]),
                reload_policy,
            }],
            components: vec![ComponentRuntimeMetadata {
                manifest: ComponentManifest {
                    listeners: Vec::new(),
                    id: component,
                    owner: plugin,
                    imports: Vec::new(),
                    exports: Vec::new(),
                    maximum_authority: Authority::default(),
                },
                version: 1,
                configuration_contracts: BTreeSet::new(),
                requested_capabilities: BTreeSet::new(),
                state_class: ComponentStateClass::Stateless,
                reload_policy,
                interposition_interfaces: BTreeSet::new(),
                event_contributions: BTreeSet::new(),
                controller_contributions: BTreeSet::new(),
            }],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
    }

    #[test]
    fn valid_development_candidate_replaces_the_live_generation_atomically() {
        let plugin = PluginManifest::resource_only(PluginId::parse("fixture.resources").unwrap());
        let initial = ResolvedHarness::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource("sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let candidate = ResolvedHarness::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource("sha256:two")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let expected_generation = candidate.generation().clone();
        let mut kernel = Kernel::new(initial.kernel_config().clone());
        kernel.activate_resolved_harness(&initial).unwrap();
        let mut reconciler = GraphReconciler::new(initial);

        let result = reconciler
            .activate_candidate_on_kernel(&mut kernel, candidate)
            .unwrap();

        assert_eq!(result.active_generation, expected_generation);
        assert_eq!(kernel.graph_generation(), Some(&expected_generation));
        assert_eq!(kernel.active_resources()[0].content_identity, "sha256:two");
        assert_eq!(reconciler.active().generation(), &expected_generation);
    }

    #[test]
    fn metadata_policy_blocks_live_activation_before_generation_mutation() {
        let active_input = metadata_fixture(ReloadPolicy::Restart);
        let mut candidate_input = active_input.clone();
        candidate_input.packages[0].reload_policy = ReloadPolicy::MigrationRequired;
        let (initial, active_metadata) = active_input
            .resolve_inspectable(&Authority::default())
            .unwrap();
        let (candidate, candidate_metadata) = candidate_input
            .resolve_inspectable(&Authority::default())
            .unwrap();
        let initial_generation = initial.generation().clone();
        let component = crate::ComponentId::parse("fixture.component").unwrap();
        let mut kernel = Kernel::new(initial.kernel_config().clone());
        kernel.activate_resolved_harness(&initial).unwrap();
        let mut reconciler = GraphReconciler::new(initial);

        let error = reconciler
            .activate_candidate_on_kernel_with_metadata(
                &mut kernel,
                &active_metadata,
                candidate,
                &candidate_metadata,
            )
            .unwrap_err();

        assert_eq!(
            error,
            LiveReconciliationError::MetadataPolicy(
                MetadataReconciliationError::MigrationRequired { component },
            )
        );
        assert_eq!(kernel.graph_generation(), Some(&initial_generation));
        assert_eq!(reconciler.active().generation(), &initial_generation);
    }

    #[test]
    fn changed_plugin_manifest_activates_as_a_new_generation() {
        let plugin = PluginManifest::resource_only(PluginId::parse("fixture.resources").unwrap());
        let initial = ResolvedHarness::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource("sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let mut changed_plugin = plugin.clone();
        changed_plugin.version += 1;
        let candidate = ResolvedHarness::resolve_with_resources(
            [changed_plugin.clone()],
            [],
            [resource("sha256:two")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let initial_generation = initial.generation().clone();
        let mut kernel = Kernel::new(initial.kernel_config().clone());
        kernel.activate_resolved_harness(&initial).unwrap();
        let mut reconciler = GraphReconciler::new(initial);

        let expected_generation = candidate.generation().clone();
        let result = reconciler
            .activate_candidate_on_kernel(&mut kernel, candidate)
            .unwrap();

        assert_eq!(result.previous_generation, initial_generation);
        assert_eq!(result.active_generation, expected_generation);
        assert_eq!(kernel.graph_generation(), Some(&expected_generation));
        assert_eq!(
            kernel.config().manifest(&changed_plugin.id),
            Some(&changed_plugin)
        );
        assert_eq!(kernel.active_resources()[0].content_identity, "sha256:two");
    }

    #[test]
    fn candidate_plugin_set_replaces_the_live_runtime_generation() {
        let plugin = PluginManifest::resource_only(PluginId::parse("fixture.resources").unwrap());
        let replacement =
            PluginManifest::resource_only(PluginId::parse("fixture.replacement").unwrap());
        let initial = ResolvedHarness::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource("sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let candidate = ResolvedHarness::resolve_with_resources(
            [replacement.clone()],
            [],
            [resource("sha256:two")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let initial_generation = initial.generation().clone();
        let mut kernel = Kernel::new(initial.kernel_config().clone());
        kernel.activate_resolved_harness(&initial).unwrap();
        let mut reconciler = GraphReconciler::new(initial);

        let expected_generation = candidate.generation().clone();
        let result = reconciler
            .activate_candidate_on_kernel(&mut kernel, candidate)
            .unwrap();

        assert_eq!(result.previous_generation, initial_generation);
        assert_eq!(result.active_generation, expected_generation);
        assert_eq!(kernel.graph_generation(), Some(&expected_generation));
        assert!(kernel.config().manifest(&plugin.id).is_none());
        assert_eq!(
            kernel.config().manifest(&replacement.id),
            Some(&replacement)
        );
        assert_eq!(kernel.active_resources()[0].content_identity, "sha256:two");
    }

    #[test]
    fn stale_reconciler_cannot_mutate_a_different_live_generation() {
        let plugin = PluginManifest::resource_only(PluginId::parse("fixture.resources").unwrap());
        let initial = ResolvedHarness::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource("sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let live = ResolvedHarness::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource("sha256:live")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let candidate = ResolvedHarness::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource("sha256:candidate")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let initial_generation = initial.generation().clone();
        let live_generation = live.generation().clone();
        let mut kernel = Kernel::new(crate::KernelConfig::new([plugin]).unwrap());
        kernel.activate_resolved_harness(&live).unwrap();
        let mut reconciler = GraphReconciler::new(initial);

        let error = reconciler
            .activate_candidate_on_kernel(&mut kernel, candidate)
            .unwrap_err();

        assert_eq!(
            error,
            LiveReconciliationError::ActiveGenerationMismatch {
                kernel: live_generation.clone(),
                reconciler: initial_generation.clone(),
            }
        );
        assert_eq!(kernel.graph_generation(), Some(&live_generation));
        assert_eq!(reconciler.active().generation(), &initial_generation);
    }
}
