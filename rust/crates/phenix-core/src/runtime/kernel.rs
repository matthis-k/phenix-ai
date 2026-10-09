use super::{
    dispatch::{
        ComponentDispatchTarget, ComponentInvocationPlan, invoke_component_service_with,
        invoke_service_with,
    },
    *,
};
use crate::{
    WorkflowBoundCallError, WorkflowNodeDispatchError, WorkflowRunError, WorkflowRunReport,
};
use std::num::NonZeroU64;
use std::panic::{AssertUnwindSafe, catch_unwind};

impl Kernel {
    pub fn new(config: KernelConfig) -> Self {
        Self::with_persistence(
            config,
            LocalPersistence::open_in_memory().expect("baseline local persistence opens"),
        )
    }

    pub fn with_persistence(
        config: KernelConfig,
        persistence: impl PersistenceBackend + 'static,
    ) -> Self {
        Self::with_boxed_persistence(config, Box::new(persistence), None)
    }

    /// Construct a kernel from the backend and Store Binding prepared together.
    pub fn with_prepared_persistence(
        config: KernelConfig,
        prepared: crate::PreparedPersistence,
    ) -> Self {
        let (plan, backend) = prepared.into_parts();
        Self::with_boxed_persistence(config, backend, Some(plan))
    }

    fn with_boxed_persistence(
        config: KernelConfig,
        persistence: Box<dyn PersistenceBackend>,
        persistence_bootstrap: Option<crate::ResolvedPersistenceBootstrap>,
    ) -> Self {
        Self {
            generation_state: GenerationRuntimeState::bootstrap(config),
            resident_generations: BTreeMap::new(),
            authority_ceiling: None,
            embedded_factories: BTreeMap::new(),
            prepared_embedded_instances: BTreeMap::new(),
            events: Arc::new(EventBus::default()),
            tasks: Arc::new(TaskRuntime::default()),
            persistence: Arc::new(Mutex::new(persistence)),
            persistence_bootstrap,
            trace_sink: Arc::new(RuntimeTraceBuffer::default()),
            provenance: Arc::new(ProvenanceBuffer::default()),
        }
    }

    pub fn kernel_only() -> Self {
        Self::new(KernelConfig::empty())
    }

    pub fn generation_topology(&self) -> &GenerationTopology {
        &self.generation_state.runtime
    }

    pub fn config(&self) -> &KernelConfig {
        self.generation_state.runtime.config()
    }

    pub fn persistence_bootstrap(&self) -> Option<&crate::ResolvedPersistenceBootstrap> {
        self.persistence_bootstrap.as_ref()
    }

    pub fn graph_generation(&self) -> Option<&GenerationId> {
        self.generation_state.runtime.generation()
    }

    pub(crate) fn install_generation_topology(
        &mut self,
        generation: GenerationTopology,
        durable_schemas: Vec<DurableSchemaRegistration>,
        authority_ceiling: Authority,
    ) {
        if self.authority_ceiling.is_none() {
            self.authority_ceiling = Some(authority_ceiling.clone());
        }
        self.generation_state.runtime = generation;
        self.generation_state.authority_ceiling = Some(authority_ceiling.clone());
        self.generation_state.lifecycle_constraints = Some(RootExecutionConstraints {
            authority: authority_ceiling,
            pinned_bindings: BTreeMap::new(),
        });
        self.generation_state.durable_schemas = durable_schemas;
    }

    pub fn authority_ceiling(&self) -> Option<&Authority> {
        self.authority_ceiling.as_ref()
    }

    pub(crate) fn validate_generation_authority(
        &self,
        candidate: &crate::ResolvedGeneration,
    ) -> Result<(), KernelError> {
        let Some(ceiling) = &self.authority_ceiling else {
            return Ok(());
        };
        if ceiling.permits_all(candidate.authority_ceiling()) {
            return Ok(());
        }
        Err(KernelError::GenerationAuthorityExpansion(
            candidate.generation().clone(),
        ))
    }

    pub fn component_graph(&self) -> &ResolvedComponentGraph {
        self.generation_state.runtime.component_graph()
    }

    pub fn dispatch_topology(&self) -> &ResolvedDispatchTopology {
        self.generation_state.runtime.dispatch_topology()
    }

    pub fn active_resources(&self) -> &[SkillResourceMetadata] {
        self.generation_state.runtime.resources()
    }

    pub fn events(&self) -> Arc<EventBus> {
        Arc::clone(&self.events)
    }

    pub fn tasks(&self) -> &TaskRuntime {
        &self.tasks
    }

    pub fn service_invocation_provenance(&self) -> Vec<ServiceInvocationProvenance> {
        self.provenance.snapshot()
    }

    /// Replace the process-local destination for metadata-only runtime diagnostics.
    ///
    /// The sink is not part of semantic event delivery and cannot affect invocation outcomes.
    pub fn set_runtime_trace_sink(&mut self, sink: Arc<dyn RuntimeTraceSink>) {
        self.trace_sink = sink;
    }

    pub fn state(&self, plugin: &PluginId) -> Option<PluginState> {
        self.generation_state.states.get(plugin).copied()
    }

    pub fn register_embedded_factory<F>(
        &mut self,
        plugin: PluginId,
        factory: F,
    ) -> Result<(), KernelError>
    where
        F: Fn() -> Box<dyn PluginInstance> + Send + Sync + 'static,
    {
        let manifest = self
            .config()
            .manifest(&plugin)
            .ok_or_else(|| KernelError::UnknownPlugin(plugin.clone()))?;
        if !matches!(manifest.execution, PluginExecution::Embedded) {
            return Err(KernelError::WrongExecutionKind(plugin));
        }
        self.preload_embedded_factory(plugin, factory);
        Ok(())
    }

    /// Preload an embedded implementation for a plugin that may enter a future graph generation.
    /// This grants no authority and does not mutate the active composition.
    pub fn preload_embedded_factory<F>(&mut self, plugin: PluginId, factory: F)
    where
        F: Fn() -> Box<dyn PluginInstance> + Send + Sync + 'static,
    {
        self.embedded_factories.insert(plugin, Arc::new(factory));
    }

    /// Preload one already-constructed embedded implementation for the next activation.
    /// Stateful plugins with real construction inputs use this path instead of pretending to have
    /// a reusable zero-argument factory.
    pub fn preload_embedded_instance(
        &mut self,
        plugin: PluginId,
        instance: Box<dyn PluginInstance>,
    ) {
        self.prepared_embedded_instances.insert(plugin, instance);
    }

    pub(super) fn take_embedded_instance(
        &mut self,
        plugin: &PluginId,
    ) -> Result<Box<dyn PluginInstance>, KernelError> {
        if let Some(instance) = self.prepared_embedded_instances.remove(plugin) {
            return Ok(instance);
        }
        self.embedded_factories
            .get(plugin)
            .map(|factory| factory())
            .ok_or_else(|| KernelError::EmbeddedFactoryMissing(plugin.clone()))
    }

    pub fn activate_all(&mut self) -> Result<(), KernelError> {
        if self.generation_state.active
            && self
                .generation_state
                .states
                .values()
                .all(|state| *state == PluginState::Active)
        {
            return Ok(());
        }
        let config = self.config().clone();
        let mut next_states = if self.generation_state.active {
            self.generation_state.states.clone()
        } else {
            config
                .manifests()
                .map(|manifest| (manifest.id.clone(), PluginState::Registered))
                .collect()
        };
        let mut next_instances: BTreeMap<PluginId, Arc<Mutex<Box<dyn PluginInstance>>>> =
            if self.generation_state.active {
                self.generation_state.instances.clone()
            } else {
                BTreeMap::new()
            };
        let mut next_invocations = if self.generation_state.active {
            self.generation_state.invocations.clone()
        } else {
            BTreeMap::new()
        };
        let mut staged = Vec::new();

        for plugin in config.activation_order() {
            if next_states.get(plugin) == Some(&PluginState::Active) {
                continue;
            }
            let manifest = config
                .manifest(plugin)
                .expect("activation order only contains configured plugins");
            let instance = (|| -> Result<Option<Box<dyn PluginInstance>>, KernelError> {
                match &manifest.execution {
                    PluginExecution::ResourceOnly => Ok(None),
                    PluginExecution::Embedded => self.take_embedded_instance(plugin).map(Some),
                    PluginExecution::Runtime { runtime, artifact } => {
                        let binding =
                            config
                                .plugin_runtime_binding(plugin)
                                .cloned()
                                .ok_or_else(|| {
                                    KernelError::PluginRuntimeAdapterUnavailable(runtime.clone())
                                })?;
                        let adapter_manifest = config
                            .manifest(&binding.adapter_plugin)
                            .expect("resolved plugin runtime adapter is configured");
                        let adapter_authority = self
                            .generation_state
                            .constrain_plugin_authority(&adapter_manifest.maximum_authority);
                        let guest_authority = self
                            .generation_state
                            .constrain_plugin_authority(&manifest.maximum_authority);
                        let adapter_instance = next_instances
                            .get(&binding.adapter_plugin)
                            .cloned()
                            .ok_or_else(|| {
                                KernelError::PluginNotActive(binding.adapter_plugin.clone())
                            })?;
                        let live_call = self
                            .tasks
                            .begin_call(&binding.adapter_plugin, self.graph_generation());
                        let cancellation = live_call.cancellation_token().clone();
                        let prepared_mutations =
                            PreparedMutationScope::new(self.graph_generation());
                        let host = PluginHost {
                            runtime: RuntimeServices {
                                states: &next_states,
                                instances: &next_instances,
                                invocations: &next_invocations,
                                events: &self.events,
                                tasks: &self.tasks,
                                persistence: &self.persistence,
                                prepared_mutations: &prepared_mutations,
                                trace_sink: self.trace_sink.as_ref(),
                                provenance: &self.provenance,
                            },
                            plugin: &binding.adapter_plugin,
                            scope: CallScope::root(
                                Arc::new(self.generation_state.runtime.clone()),
                                &binding.adapter_plugin,
                                &adapter_authority,
                                Some(cancellation.clone()),
                            ),
                            continuation: None,
                        };
                        let mut adapter_instance = adapter_instance
                            .lock()
                            .expect("plugin instance mutex poisoned");
                        let contract =
                            adapter_instance.plugin_runtime_adapter().ok_or_else(|| {
                                KernelError::PluginRuntimeAdapterContractUnavailable {
                                    runtime: runtime.clone(),
                                    adapter_plugin: binding.adapter_plugin.clone(),
                                }
                            })?;
                        let prepared = catch_unwind(AssertUnwindSafe(|| {
                            contract.prepare_with_host(
                                PluginRuntimeCandidate {
                                    manifest,
                                    artifact,
                                    guest_authority: &guest_authority,
                                },
                                &host,
                            )
                        }))
                        .map_err(|_| {
                            KernelError::PluginRuntimePreparation {
                                plugin: plugin.clone(),
                                runtime: runtime.clone(),
                                message: "plugin runtime adapter panicked".into(),
                            }
                        })?;
                        if cancellation.is_cancelled() {
                            prepared_mutations.clear();
                            return Err(KernelError::PluginRuntimePreparation {
                                plugin: plugin.clone(),
                                runtime: runtime.clone(),
                                message: "plugin runtime adapter preparation cancelled".into(),
                            });
                        }
                        prepared.map(Some).map_err(|message| {
                            KernelError::PluginRuntimePreparation {
                                plugin: plugin.clone(),
                                runtime: runtime.clone(),
                                message,
                            }
                        })
                    }
                }
            })();
            let instance = match instance {
                Ok(instance) => instance,
                Err(error) => {
                    reconciliation::cleanup_staged(
                        &staged,
                        reconciliation::StopView {
                            runtime: &self.generation_state.runtime,
                            lifecycle_constraints: self
                                .generation_state
                                .lifecycle_constraints
                                .as_ref(),
                            states: &next_states,
                            instances: &next_instances,
                            invocations: &next_invocations,
                            events: &self.events,
                            tasks: &self.tasks,
                            persistence: &self.persistence,
                            trace_sink: self.trace_sink.as_ref(),
                            provenance: &self.provenance,
                        },
                    );
                    return Err(error);
                }
            };
            if let Some(mut instance) = instance {
                let live_call = self.tasks.begin_call(plugin, self.graph_generation());
                let cancellation = live_call.cancellation_token().clone();
                let prepared_mutations = PreparedMutationScope::new(self.graph_generation());
                let plugin_authority = self
                    .generation_state
                    .constrain_plugin_authority(&manifest.maximum_authority);
                let host = PluginHost {
                    runtime: RuntimeServices {
                        states: &next_states,
                        instances: &next_instances,
                        invocations: &next_invocations,
                        events: &self.events,
                        tasks: &self.tasks,
                        persistence: &self.persistence,
                        prepared_mutations: &prepared_mutations,
                        trace_sink: self.trace_sink.as_ref(),
                        provenance: &self.provenance,
                    },
                    plugin,
                    scope: CallScope::root(
                        Arc::new(self.generation_state.runtime.clone()),
                        plugin,
                        &plugin_authority,
                        Some(cancellation.clone()),
                    ),
                    continuation: None,
                };
                let started = catch_unwind(AssertUnwindSafe(|| instance.start(&host)));
                let failure = match started {
                    Ok(Ok(())) if cancellation.is_cancelled() => {
                        prepared_mutations.clear();
                        Some("plugin start cancelled".into())
                    }
                    Ok(Ok(())) => None,
                    Ok(Err(message)) => Some(message),
                    Err(_) => Some("plugin start panicked".into()),
                };
                if let Some(message) = failure {
                    self.tasks
                        .cancel_plugin_generation(plugin, self.graph_generation());
                    reconciliation::cleanup_staged(
                        &staged,
                        reconciliation::StopView {
                            runtime: &self.generation_state.runtime,
                            lifecycle_constraints: self
                                .generation_state
                                .lifecycle_constraints
                                .as_ref(),
                            states: &next_states,
                            instances: &next_instances,
                            invocations: &next_invocations,
                            events: &self.events,
                            tasks: &self.tasks,
                            persistence: &self.persistence,
                            trace_sink: self.trace_sink.as_ref(),
                            provenance: &self.provenance,
                        },
                    );
                    return Err(KernelError::PluginStart {
                        plugin: plugin.clone(),
                        message,
                    });
                }
                let instance = Arc::new(Mutex::new(instance));
                let invocation = canonical_invocation(&instance);
                next_instances.insert(plugin.clone(), Arc::clone(&instance));
                next_invocations.insert(plugin.clone(), invocation);
            }
            next_states.insert(plugin.clone(), PluginState::Active);
            staged.push(plugin.clone());
        }

        let subscriptions = match self.graph_generation() {
            Some(_generation) => stage_listener_subscriptions(listener::ListenerRuntimeSources {
                runtime: &self.generation_state.runtime,
                states: &next_states,
                instances: &next_instances,
                invocations: &next_invocations,
                events: &self.events,
                tasks: &self.tasks,
                persistence: &self.persistence,
                trace_sink: &self.trace_sink,
                provenance: &self.provenance,
            }),
            None if self.component_graph().listeners().next().is_none() => Ok(Vec::new()),
            None => Err(KernelError::ResolvedGenerationMissing),
        };
        let subscriptions = match subscriptions {
            Ok(subscriptions) => subscriptions,
            Err(error) => {
                reconciliation::cleanup_staged(
                    &staged,
                    reconciliation::StopView {
                        runtime: &self.generation_state.runtime,
                        lifecycle_constraints: self.generation_state.lifecycle_constraints.as_ref(),
                        states: &next_states,
                        instances: &next_instances,
                        invocations: &next_invocations,
                        events: &self.events,
                        tasks: &self.tasks,
                        persistence: &self.persistence,
                        trace_sink: self.trace_sink.as_ref(),
                        provenance: &self.provenance,
                    },
                );
                return Err(error);
            }
        };

        self.events.replace_subscriptions(subscriptions.clone())?;
        if let Some(generation) = self.graph_generation().cloned() {
            self.events
                .replace_generation_subscriptions(generation, subscriptions.clone())?;
        }
        self.generation_state.subscriptions = subscriptions;
        self.generation_state.states = next_states;
        self.generation_state.instances = next_instances;
        self.generation_state.invocations = next_invocations;
        self.generation_state.active = true;
        for plugin in staged {
            self.events.publish(KernelEvent::PluginActivated(plugin));
        }
        Ok(())
    }

    pub fn invoke_component(
        &mut self,
        component: &ComponentId,
        service: &ServiceId,
        input: &[u8],
        caller_authority: &Authority,
        binding: &PluginId,
    ) -> Result<Vec<u8>, KernelError> {
        let service_plan = self.dispatch_topology().service(service);
        let layer_plan = service_plan.map_or(&[][..], |plan| plan.layers.as_slice());
        let policy_identity = service_plan.map_or_else(
            || self.config().policy_identity(),
            |plan| plan.policy_identity,
        );
        let prepared_mutations = PreparedMutationScope::new(self.graph_generation());
        let runtime = RuntimeServices {
            states: &self.generation_state.states,
            instances: &self.generation_state.instances,
            invocations: &self.generation_state.invocations,
            events: &self.events,
            tasks: &self.tasks,
            persistence: &self.persistence,
            prepared_mutations: &prepared_mutations,
            trace_sink: self.trace_sink.as_ref(),
            provenance: &self.provenance,
        };
        let root_authority = self
            .generation_state
            .constrain_root_authority(caller_authority);
        let scope = CallScope::external(
            Arc::new(self.generation_state.runtime.clone()),
            &root_authority,
        );
        invoke_component_service_with(
            runtime,
            ComponentInvocationPlan {
                service,
                layers: layer_plan,
                policy_identity,
            },
            ComponentDispatchTarget {
                component,
                binding,
                provider_provenance: None,
            },
            input,
            scope,
        )
    }

    pub fn root_execution_handle(&self, caller_authority: &Authority) -> RootExecutionHandle {
        let constraints = RootExecutionConstraints {
            authority: self
                .generation_state
                .constrain_root_authority(caller_authority),
            pinned_bindings: BTreeMap::new(),
        };
        self.root_execution_handle_from_state(&self.generation_state, constraints)
    }

    pub(super) fn root_execution_handle_from_state(
        &self,
        state: &GenerationRuntimeState,
        constraints: RootExecutionConstraints,
    ) -> RootExecutionHandle {
        state.root_leases.fetch_add(1, Ordering::AcqRel);
        RootExecutionHandle {
            runtime: Arc::new(state.runtime.clone()),
            constraints,
            states: state.states.clone(),
            instances: state.instances.clone(),
            invocations: state.invocations.clone(),
            events: Arc::clone(&self.events),
            tasks: Arc::clone(&self.tasks),
            persistence: Arc::clone(&self.persistence),
            trace_sink: Arc::clone(&self.trace_sink),
            provenance: Arc::clone(&self.provenance),
            root_leases: Arc::clone(&state.root_leases),
        }
    }

    pub fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        caller_authority: &Authority,
        binding: Option<&PluginId>,
    ) -> Result<Vec<u8>, KernelError> {
        self.root_execution_handle(caller_authority)
            .invoke(service, input, binding)
    }
}

impl RootExecutionHandle {
    /// Invoke the exact component import selected for this pinned generation.
    ///
    /// The import's provider, authority and Layers come from the canonical
    /// resolved component graph. No provider lookup or fallback occurs after
    /// this call starts. A stale or foreign binding is rejected.
    pub fn invoke_import(
        &self,
        import: &ResolvedImportHandle,
        input: &[u8],
    ) -> Result<Vec<u8>, KernelError> {
        let key = (import.importer().clone(), import.interface().clone());
        let selected = self
            .runtime
            .component_graph()
            .import_handle(import.importer(), import.interface())?
            .ok_or_else(|| KernelError::PinnedBindingUnavailable {
                component: key.0.clone(),
                interface: key.1.clone(),
            })?;
        let dispatch = self
            .runtime
            .dispatch_topology()
            .component_import(import.importer(), import.interface())
            .ok_or_else(|| KernelError::PinnedBindingUnavailable {
                component: key.0.clone(),
                interface: key.1.clone(),
            })?;
        if selected != import
            || dispatch.providers.primary() != import
            || self
                .constraints
                .pinned_bindings
                .get(&key)
                .is_some_and(|pin| pin != import)
        {
            return Err(KernelError::PinnedBindingChanged {
                generation: self
                    .runtime
                    .generation()
                    .expect("resolved workflow has a generation")
                    .clone(),
                component: key.0,
                interface: key.1,
            });
        }
        let authority = self
            .constraints
            .authority
            .attenuate(import.effective_authority());
        let provenance = ComponentProviderProvenance::from_plan(
            import.interface().clone(),
            &dispatch.providers,
            import,
            None,
            authority.clone(),
        );
        let prepared_mutations = PreparedMutationScope::new(self.generation());
        let runtime = RuntimeServices {
            states: &self.states,
            instances: &self.instances,
            invocations: &self.invocations,
            events: self.events.as_ref(),
            tasks: self.tasks.as_ref(),
            persistence: self.persistence.as_ref(),
            prepared_mutations: &prepared_mutations,
            trace_sink: self.trace_sink.as_ref(),
            provenance: self.provenance.as_ref(),
        };
        let constraints = self.constraints.with_authority(authority);
        let scope = CallScope::external_with_constraints(Arc::clone(&self.runtime), &constraints);
        invoke_component_service_with(
            runtime,
            ComponentInvocationPlan {
                service: &dispatch.service,
                layers: &dispatch.layers,
                policy_identity: dispatch.policy_identity,
            },
            ComponentDispatchTarget {
                component: import.exporter(),
                binding: import.owning_plugin(),
                provider_provenance: Some(provenance),
            },
            input,
            scope,
        )
    }

    /// Execute a workflow declared by a component in this root's pinned generation.
    ///
    /// Domain adapters prepare service inputs and project typed results into
    /// declared outcomes. The kernel owns dispatch, authority and service Layers.
    pub fn execute_workflow<State, Error>(
        &self,
        workflow: (&ComponentId, &str),
        state: &mut State,
        mut prepare: impl FnMut(&str, &InterfaceId, &mut State) -> Result<Vec<u8>, Error>,
        mut project: impl FnMut(&str, &InterfaceId, &[u8], &mut State) -> Result<String, Error>,
        cancelled: impl FnMut() -> bool,
        step_limit: Option<NonZeroU64>,
    ) -> Result<
        WorkflowRunReport,
        WorkflowRunError<WorkflowBoundCallError<WorkflowNodeDispatchError<Error>>>,
    > {
        let (owner, name) = workflow;
        let compiled = self.runtime.workflow(owner, name).ok_or_else(|| {
            WorkflowRunError::MissingWorkflow {
                owner: owner.clone(),
                name: name.to_owned(),
            }
        })?;
        compiled.execute_bound(
            state,
            |node, service, import, state, cancelled| {
                let request = prepare(node, service, state)
                    .map_err(WorkflowNodeDispatchError::Prepare)
                    .map_err(crate::workflow::WorkflowInvocationError::Failed)?;
                if cancelled() {
                    return Err(crate::workflow::WorkflowInvocationError::Cancelled);
                }
                let output = self
                    .invoke_import(import, &request)
                    .map_err(WorkflowNodeDispatchError::Invoke)
                    .map_err(crate::workflow::WorkflowInvocationError::Failed)?;
                project(node, service, &output, state)
                    .map_err(WorkflowNodeDispatchError::Project)
                    .map_err(crate::workflow::WorkflowInvocationError::Failed)
            },
            cancelled,
            step_limit,
        )
    }

    /// Execute the same pinned workflow with a typed data-only frame.
    ///
    /// The frame is caller-owned execution data, not a capability grant or a
    /// second provider resolver. Every mutation uses WorkflowFrame::set.
    /// Failed response projection rolls back that node's frame edits; it does
    /// not undo service side effects. Cancellation retains the last committed
    /// frame and never retries an invocation.
    pub fn execute_workflow_with_frame<State, Error>(
        &self,
        workflow: (&ComponentId, &str),
        execution: (&mut State, &mut crate::WorkflowFrame),
        mut prepare: impl FnMut(
            &str,
            &InterfaceId,
            &crate::WorkflowFrame,
            &mut State,
        ) -> Result<Vec<u8>, Error>,
        mut project: impl FnMut(
            &str,
            &InterfaceId,
            &[u8],
            &mut crate::WorkflowFrame,
            &mut State,
        ) -> Result<String, Error>,
        cancelled: impl FnMut() -> bool,
        step_limit: Option<NonZeroU64>,
    ) -> Result<
        WorkflowRunReport,
        WorkflowRunError<WorkflowBoundCallError<WorkflowNodeDispatchError<Error>>>,
    > {
        let (state, frame) = execution;
        let (owner, name) = workflow;
        let compiled = self.runtime.workflow(owner, name).ok_or_else(|| {
            WorkflowRunError::MissingWorkflow {
                owner: owner.clone(),
                name: name.to_owned(),
            }
        })?;
        let expected =
            compiled
                .frame_schema()
                .ok_or_else(|| WorkflowRunError::MissingFrameSchema {
                    owner: owner.clone(),
                    name: name.to_owned(),
                })?;
        if expected != frame.schema() {
            return Err(WorkflowRunError::FrameSchemaMismatch {
                owner: owner.clone(),
                name: name.to_owned(),
            });
        }
        self.execute_workflow(
            workflow,
            &mut (state, frame),
            |node, import, context| prepare(node, import, &*context.1, &mut *context.0),
            |node, import, response, context| {
                let snapshot = (*context.1).clone();
                match project(node, import, response, &mut *context.1, &mut *context.0) {
                    Ok(outcome) => Ok(outcome),
                    Err(error) => {
                        *context.1 = snapshot;
                        Err(error)
                    }
                }
            },
            cancelled,
            step_limit,
        )
    }

    pub fn invoke(
        &self,
        service: &ServiceId,
        input: &[u8],
        binding: Option<&PluginId>,
    ) -> Result<Vec<u8>, KernelError> {
        let prepared_mutations = PreparedMutationScope::new(self.generation());
        let runtime = RuntimeServices {
            states: &self.states,
            instances: &self.instances,
            invocations: &self.invocations,
            events: self.events.as_ref(),
            tasks: self.tasks.as_ref(),
            persistence: self.persistence.as_ref(),
            prepared_mutations: &prepared_mutations,
            trace_sink: self.trace_sink.as_ref(),
            provenance: self.provenance.as_ref(),
        };
        let scope =
            CallScope::external_with_constraints(Arc::clone(&self.runtime), &self.constraints);
        invoke_service_with(runtime, service, input, binding, scope)
    }
}

impl Kernel {
    pub fn stop(&mut self, plugin: &PluginId) -> Result<(), KernelError> {
        let manifest = self
            .config()
            .manifest(plugin)
            .ok_or_else(|| KernelError::UnknownPlugin(plugin.clone()))?;
        let generation = self.graph_generation();
        self.tasks.cancel_calls(plugin, generation);
        self.tasks.cancel_plugin_generation(plugin, generation);
        if let Some(instance) = self.generation_state.instances.get(plugin) {
            let live_call = self.tasks.begin_call(plugin, generation);
            let cancellation = live_call.cancellation_token().clone();
            let prepared_mutations = PreparedMutationScope::new(generation);
            let plugin_authority = self
                .generation_state
                .constrain_plugin_authority(&manifest.maximum_authority);
            let host = PluginHost {
                runtime: RuntimeServices {
                    states: &self.generation_state.states,
                    instances: &self.generation_state.instances,
                    invocations: &self.generation_state.invocations,
                    events: &self.events,
                    tasks: &self.tasks,
                    persistence: &self.persistence,
                    prepared_mutations: &prepared_mutations,
                    trace_sink: self.trace_sink.as_ref(),
                    provenance: &self.provenance,
                },
                plugin,
                scope: CallScope::root(
                    Arc::new(self.generation_state.runtime.clone()),
                    plugin,
                    &plugin_authority,
                    Some(cancellation.clone()),
                ),
                continuation: None,
            };
            let mut instance = instance.lock().expect("plugin instance mutex poisoned");
            let stopped = catch_unwind(AssertUnwindSafe(|| instance.stop(&host)));
            match stopped {
                Ok(Ok(())) if cancellation.is_cancelled() => {
                    prepared_mutations.clear();
                    return Err(KernelError::PluginStop {
                        plugin: plugin.clone(),
                        message: "plugin stop cancelled".into(),
                    });
                }
                Ok(Ok(())) => {}
                Ok(Err(message)) => {
                    return Err(KernelError::PluginStop {
                        plugin: plugin.clone(),
                        message,
                    });
                }
                Err(_) => {
                    return Err(KernelError::PluginStop {
                        plugin: plugin.clone(),
                        message: "plugin stop panicked".into(),
                    });
                }
            }
        }
        self.generation_state.instances.remove(plugin);
        self.generation_state.invocations.remove(plugin);
        let state = self
            .generation_state
            .states
            .get_mut(plugin)
            .expect("plugin manifest and lifecycle state stay aligned");
        *state = PluginState::Stopped;
        self.events
            .publish(KernelEvent::PluginStopped(plugin.clone()));
        Ok(())
    }
}
