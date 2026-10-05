use crate::{
    GenerationId, GraphReconciler, Kernel, LayerPolicy, MetadataReconciliationError, PluginId,
    PluginManifest, ReconciliationResult, ResolvedCompositionMetadata, ResolvedGeneration,
    ResolvedGenerationActivationError, RootExecutionConstraints, ServiceId,
};
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Clone, Debug, PartialEq)]
pub enum LiveReconciliationError {
    NoActiveGeneration,
    ActiveGenerationMismatch {
        kernel: GenerationId,
        reconciler: GenerationId,
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
    ResidentGenerationsPresent(Vec<GenerationId>),
    MetadataPolicy(MetadataReconciliationError),
    Runtime(crate::KernelError),
}

impl Display for LiveReconciliationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoActiveGeneration => f.write_str("no active graph generation"),
            Self::ActiveGenerationMismatch { kernel, reconciler } => write!(
                f,
                "active graph generation mismatch: kernel={}, reconciler={}",
                kernel.as_str(),
                reconciler.as_str()
            ),
            Self::KernelConfigurationMismatch {
                kernel_plugins,
                resolved_plugins,
            } => write!(
                f,
                "kernel plugin set differs from resolved generation: kernel={kernel_plugins:?}, resolved={resolved_plugins:?}"
            ),
            Self::KernelPluginManifestMismatch { plugin, .. } => {
                write!(f, "kernel Plugin manifest differs from resolved generation for {plugin}")
            }
            Self::KernelLayerPolicyMismatch { service, .. } => {
                write!(f, "kernel layer policy differs from resolved generation for {service}")
            }
            Self::ResidentGenerationsPresent(generations) => write!(
                f,
                "stable replacement requires no resident generations; found {:?}",
                generations
                    .iter()
                    .map(GenerationId::as_str)
                    .collect::<Vec<_>>()
            ),
            Self::MetadataPolicy(error) => write!(f, "{error}"),
            Self::Runtime(error) => write!(f, "{error}"),
        }
    }
}

impl Error for LiveReconciliationError {}

impl GraphReconciler {
    pub(crate) fn preflight_live_reconciliation(
        &self,
        kernel: &Kernel,
    ) -> Result<(), LiveReconciliationError> {
        validate_live_reconciliation(self, kernel)
    }

    /// Stage a resolved candidate beside the active generation without changing
    /// the default generation.
    pub fn make_candidate_resident_on_kernel(
        &mut self,
        kernel: &mut Kernel,
        candidate: ResolvedGeneration,
        constraints: &RootExecutionConstraints,
    ) -> Result<GenerationId, LiveReconciliationError> {
        self.preflight_live_reconciliation(kernel)?;
        let generation = kernel
            .make_generation_resident_under_constraints(&candidate, constraints)
            .map_err(LiveReconciliationError::Runtime)?;
        if generation != *self.active.generation() {
            self.resident.insert(generation.clone(), candidate);
        }
        Ok(generation)
    }

    /// Promote one resident generation and retain the previous active Harness as
    /// a resident rollback target.
    pub fn promote_resident_on_kernel(
        &mut self,
        kernel: &mut Kernel,
        generation: &GenerationId,
        constraints: &RootExecutionConstraints,
    ) -> Result<ReconciliationResult, LiveReconciliationError> {
        self.preflight_live_reconciliation(kernel)?;
        if generation == self.active.generation() {
            return Ok(ReconciliationResult {
                previous_generation: generation.clone(),
                active_generation: generation.clone(),
                diff: crate::GraphDiff::default(),
                transition_plan: Vec::new(),
            });
        }

        let candidate = self.resident.remove(generation).ok_or_else(|| {
            LiveReconciliationError::Runtime(crate::KernelError::UnknownGeneration(
                generation.clone(),
            ))
        })?;
        let preview = self.preview_candidate(&candidate);
        if let Err(error) = kernel.promote_generation_under_constraints(generation, constraints) {
            self.resident.insert(generation.clone(), candidate);
            return Err(LiveReconciliationError::Runtime(error));
        }

        let previous = std::mem::replace(&mut self.active, candidate);
        self.resident
            .insert(previous.generation().clone(), previous);
        Ok(ReconciliationResult {
            previous_generation: preview.active_generation,
            active_generation: preview.candidate_generation,
            diff: preview.diff,
            transition_plan: preview.transition_plan,
        })
    }

    /// Retire one non-active resident Harness and its generation-local runtime
    /// state.
    pub fn retire_resident_on_kernel(
        &mut self,
        kernel: &mut Kernel,
        generation: &GenerationId,
        constraints: &RootExecutionConstraints,
    ) -> Result<(), LiveReconciliationError> {
        self.preflight_live_reconciliation(kernel)?;
        if !self.resident.contains_key(generation) {
            return Err(LiveReconciliationError::Runtime(
                crate::KernelError::UnknownGeneration(generation.clone()),
            ));
        }
        kernel
            .retire_generation_under_constraints(generation, constraints)
            .map_err(LiveReconciliationError::Runtime)?;
        self.resident.remove(generation);
        Ok(())
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
        candidate: ResolvedGeneration,
    ) -> Result<ReconciliationResult, LiveReconciliationError> {
        self.preflight_live_reconciliation(kernel)?;
        self.require_stable_replacement_mode()?;
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
        candidate: ResolvedGeneration,
        candidate_metadata: &ResolvedCompositionMetadata,
    ) -> Result<ReconciliationResult, LiveReconciliationError> {
        self.preflight_live_reconciliation(kernel)?;
        self.require_stable_replacement_mode()?;
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

    fn require_stable_replacement_mode(&self) -> Result<(), LiveReconciliationError> {
        let resident = self.resident_generations().cloned().collect::<Vec<_>>();
        if resident.is_empty() {
            Ok(())
        } else {
            Err(LiveReconciliationError::ResidentGenerationsPresent(
                resident,
            ))
        }
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

    crate::composition::activation::validate_resolved_generation_configuration(
        kernel,
        reconciler.active(),
    )
    .map_err(map_activation_validation_error)
}

fn restart_plugins_for_plan(
    active: &ResolvedGeneration,
    candidate: &ResolvedGeneration,
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
    error: ResolvedGenerationActivationError,
) -> LiveReconciliationError {
    match error {
        ResolvedGenerationActivationError::KernelConfigurationMismatch {
            kernel_plugins,
            resolved_plugins,
        } => LiveReconciliationError::KernelConfigurationMismatch {
            kernel_plugins,
            resolved_plugins,
        },
        ResolvedGenerationActivationError::KernelPluginManifestMismatch {
            plugin,
            kernel_manifest,
            resolved_manifest,
        } => LiveReconciliationError::KernelPluginManifestMismatch {
            plugin,
            kernel_manifest,
            resolved_manifest,
        },
        ResolvedGenerationActivationError::KernelLayerPolicyMismatch {
            service,
            kernel_layers,
            resolved_layers,
        } => LiveReconciliationError::KernelLayerPolicyMismatch {
            service,
            kernel_layers,
            resolved_layers,
        },
        ResolvedGenerationActivationError::DifferentGenerationAlreadyActive { .. } => {
            unreachable!("configuration validation does not inspect active generation")
        }
        ResolvedGenerationActivationError::DurableSchemaPreparation(_) => {
            unreachable!("configuration validation does not prepare durable schemas")
        }
        ResolvedGenerationActivationError::AuthorityCeiling(_) => {
            unreachable!("configuration validation does not enforce activation authority")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Authority, CompatibilityMetadata, ComponentHostKind, ComponentManifest,
        ComponentRuntimeMetadata, ComponentStateClass, CompositionMetadataInput, PluginExecution,
        PluginPackageMetadata, ReloadPolicy, ResolvedGenerationActivation, SkillResourceMetadata,
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
    fn one_shot_replacement_is_blocked_while_trial_generations_are_resident() {
        let plugin = PluginManifest::resource_only(PluginId::parse("fixture.resources").unwrap());
        let initial = ResolvedGeneration::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource("sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let trial = ResolvedGeneration::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource("sha256:trial")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let replacement = ResolvedGeneration::resolve_with_resources(
            [plugin],
            [],
            [resource("sha256:replacement")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let initial_generation = initial.generation().clone();
        let trial_generation = trial.generation().clone();

        let mut kernel = Kernel::new(initial.kernel_config().clone());
        kernel.activate_resolved_generation(&initial).unwrap();
        let mut reconciler = GraphReconciler::new(initial);

        let constraints = kernel
            .capture_root_execution_constraints(&Authority::default(), [])
            .unwrap();
        reconciler
            .make_candidate_resident_on_kernel(&mut kernel, trial, &constraints)
            .unwrap();

        assert_eq!(
            reconciler
                .activate_candidate_on_kernel(&mut kernel, replacement)
                .unwrap_err(),
            LiveReconciliationError::ResidentGenerationsPresent(vec![trial_generation.clone()])
        );
        assert_eq!(kernel.graph_generation(), Some(&initial_generation));
        assert_eq!(reconciler.active().generation(), &initial_generation);
        assert!(reconciler.resident(&trial_generation).is_some());
    }

    #[test]
    fn resident_lifecycle_keeps_reconciler_and_kernel_in_sync() {
        let plugin = PluginManifest::resource_only(PluginId::parse("fixture.resources").unwrap());
        let initial = ResolvedGeneration::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource("sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let trial = ResolvedGeneration::resolve_with_resources(
            [plugin],
            [],
            [resource("sha256:trial")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let initial_generation = initial.generation().clone();
        let trial_generation = trial.generation().clone();

        let mut kernel = Kernel::new(initial.kernel_config().clone());
        kernel.activate_resolved_generation(&initial).unwrap();
        let mut reconciler = GraphReconciler::new(initial);
        let constraints = kernel
            .capture_root_execution_constraints(&Authority::default(), [])
            .unwrap();

        reconciler
            .make_candidate_resident_on_kernel(&mut kernel, trial, &constraints)
            .unwrap();
        assert_eq!(kernel.graph_generation(), Some(&initial_generation));
        assert_eq!(reconciler.active().generation(), &initial_generation);
        assert!(reconciler.resident(&trial_generation).is_some());
        assert!(kernel.resident_generation_ids().contains(&trial_generation));

        let promoted = reconciler
            .promote_resident_on_kernel(&mut kernel, &trial_generation, &constraints)
            .unwrap();
        assert_eq!(promoted.previous_generation, initial_generation);
        assert_eq!(promoted.active_generation, trial_generation);
        assert_eq!(kernel.graph_generation(), Some(&trial_generation));
        assert_eq!(reconciler.active().generation(), &trial_generation);
        assert_eq!(
            kernel.active_resources()[0].content_identity,
            "sha256:trial"
        );
        assert!(reconciler.resident(&initial_generation).is_some());
        assert!(kernel
            .resident_generation_ids()
            .contains(&initial_generation));

        let rolled_back = reconciler
            .promote_resident_on_kernel(&mut kernel, &initial_generation, &constraints)
            .unwrap();
        assert_eq!(rolled_back.previous_generation, trial_generation);
        assert_eq!(rolled_back.active_generation, initial_generation);
        assert_eq!(kernel.graph_generation(), Some(&initial_generation));
        assert_eq!(reconciler.active().generation(), &initial_generation);
        assert_eq!(kernel.active_resources()[0].content_identity, "sha256:one");
        assert!(reconciler.resident(&trial_generation).is_some());

        reconciler
            .retire_resident_on_kernel(&mut kernel, &trial_generation, &constraints)
            .unwrap();
        assert!(reconciler.resident(&trial_generation).is_none());
        assert!(!kernel.resident_generation_ids().contains(&trial_generation));
        assert_eq!(kernel.graph_generation(), Some(&initial_generation));
    }

    #[test]
    fn valid_development_candidate_replaces_the_live_generation_atomically() {
        let plugin = PluginManifest::resource_only(PluginId::parse("fixture.resources").unwrap());
        let initial = ResolvedGeneration::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource("sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let candidate = ResolvedGeneration::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource("sha256:two")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let expected_generation = candidate.generation().clone();
        let mut kernel = Kernel::new(initial.kernel_config().clone());
        kernel.activate_resolved_generation(&initial).unwrap();
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
        kernel.activate_resolved_generation(&initial).unwrap();
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
        let initial = ResolvedGeneration::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource("sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let mut changed_plugin = plugin.clone();
        changed_plugin.version += 1;
        let candidate = ResolvedGeneration::resolve_with_resources(
            [changed_plugin.clone()],
            [],
            [resource("sha256:two")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let initial_generation = initial.generation().clone();
        let mut kernel = Kernel::new(initial.kernel_config().clone());
        kernel.activate_resolved_generation(&initial).unwrap();
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
    fn candidate_plugin_set_replaces_the_live_generation_topology() {
        let plugin = PluginManifest::resource_only(PluginId::parse("fixture.resources").unwrap());
        let replacement =
            PluginManifest::resource_only(PluginId::parse("fixture.replacement").unwrap());
        let initial = ResolvedGeneration::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource("sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let candidate = ResolvedGeneration::resolve_with_resources(
            [replacement.clone()],
            [],
            [resource("sha256:two")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let initial_generation = initial.generation().clone();
        let mut kernel = Kernel::new(initial.kernel_config().clone());
        kernel.activate_resolved_generation(&initial).unwrap();
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
        let initial = ResolvedGeneration::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource("sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let live = ResolvedGeneration::resolve_with_resources(
            [plugin.clone()],
            [],
            [resource("sha256:live")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let candidate = ResolvedGeneration::resolve_with_resources(
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
        kernel.activate_resolved_generation(&live).unwrap();
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
