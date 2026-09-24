use super::{
    dispatch::{
        invoke_component_service_with, invoke_service_with, ComponentDispatchTarget,
        ComponentInvocationPlan, ServiceDispatchGuards,
    },
    *,
};
use std::panic::{catch_unwind, AssertUnwindSafe};

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
        let states = config
            .manifests()
            .map(|manifest| (manifest.id.clone(), PluginState::Registered))
            .collect();
        Self {
            runtime_generation: RuntimeGeneration::bootstrap(config),
            states,
            embedded_factories: BTreeMap::new(),
            prepared_embedded_instances: BTreeMap::new(),
            instances: BTreeMap::new(),
            events: Arc::new(EventBus::default()),
            tasks: Arc::new(TaskRuntime::default()),
            persistence: Arc::new(Mutex::new(persistence)),
            persistence_bootstrap,
            trace_sink: Arc::new(RuntimeTraceBuffer::default()),
            provenance: Arc::new(ProvenanceBuffer::default()),
            runtime_active: false,
        }
    }

    pub fn kernel_only() -> Self {
        Self::new(KernelConfig::empty())
    }

    pub fn runtime_generation(&self) -> &RuntimeGeneration {
        &self.runtime_generation
    }

    pub fn config(&self) -> &KernelConfig {
        self.runtime_generation.config()
    }

    pub fn persistence_bootstrap(&self) -> Option<&crate::ResolvedPersistenceBootstrap> {
        self.persistence_bootstrap.as_ref()
    }

    pub fn graph_generation(&self) -> Option<&GraphGenerationId> {
        self.runtime_generation.generation()
    }

    pub(crate) fn install_runtime_generation(&mut self, generation: RuntimeGeneration) {
        self.runtime_generation = generation;
    }

    pub fn component_graph(&self) -> &ResolvedComponentGraph {
        self.runtime_generation.component_graph()
    }

    pub fn dispatch_topology(&self) -> &ResolvedDispatchTopology {
        self.runtime_generation.dispatch_topology()
    }

    pub fn active_resources(&self) -> &[SkillResourceMetadata] {
        self.runtime_generation.resources()
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
        self.states.get(plugin).copied()
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
        if self.runtime_active
            && self
                .states
                .values()
                .all(|state| *state == PluginState::Active)
        {
            return Ok(());
        }
        let config = self.config().clone();
        let mut next_states = if self.runtime_active {
            self.states.clone()
        } else {
            config
                .manifests()
                .map(|manifest| (manifest.id.clone(), PluginState::Registered))
                .collect()
        };
        let mut next_instances: BTreeMap<PluginId, Arc<Mutex<Box<dyn PluginInstance>>>> =
            if self.runtime_active {
                self.instances.clone()
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
                        let binding = config.runtime_binding(plugin).cloned().ok_or_else(|| {
                            KernelError::RuntimeProviderUnavailable(runtime.clone())
                        })?;
                        let provider_manifest = config
                            .manifest(&binding.provider)
                            .expect("resolved runtime provider is configured");
                        let provider =
                            next_instances
                                .get(&binding.provider)
                                .cloned()
                                .ok_or_else(|| {
                                    KernelError::PluginNotActive(binding.provider.clone())
                                })?;
                        let live_call = self
                            .tasks
                            .begin_call(&binding.provider, self.graph_generation());
                        let cancellation = live_call.cancellation_token().clone();
                        let prepared_mutations =
                            PreparedMutationScope::new(self.graph_generation());
                        let host = PluginHost {
                            graph_generation: self.graph_generation(),
                            component_graph: self.component_graph(),
                            dispatch_topology: self.dispatch_topology(),
                            config: &config,
                            states: &next_states,
                            instances: &next_instances,
                            plugin: &binding.provider,
                            authority: &provider_manifest.maximum_authority,
                            transaction_context: TransactionContext::unscoped(),
                            call_cancellation: Some(cancellation.clone()),
                            call_stack: BTreeSet::from([binding.provider.clone()]),
                            events: &self.events,
                            tasks: &self.tasks,
                            persistence: &self.persistence,
                            prepared_mutations: &prepared_mutations,
                            trace_sink: self.trace_sink.as_ref(),
                            provenance: &self.provenance,
                            continuation: None,
                            active_services: BTreeSet::new(),
                            active_component_endpoints: BTreeSet::new(),
                        };
                        let mut provider = provider.lock().expect("plugin instance mutex poisoned");
                        let contract = provider.runtime_provider().ok_or_else(|| {
                            KernelError::RuntimeProviderContractUnavailable {
                                runtime: runtime.clone(),
                                provider: binding.provider.clone(),
                            }
                        })?;
                        let prepared = catch_unwind(AssertUnwindSafe(|| {
                            contract.prepare_with_host(
                                RuntimePluginCandidate {
                                    manifest,
                                    artifact,
                                    guest_authority: &manifest.maximum_authority,
                                },
                                &host,
                            )
                        }))
                        .map_err(|_| KernelError::RuntimePrepare {
                            plugin: plugin.clone(),
                            runtime: runtime.clone(),
                            message: "runtime provider panicked".into(),
                        })?;
                        if cancellation.is_cancelled() {
                            prepared_mutations.clear();
                            return Err(KernelError::RuntimePrepare {
                                plugin: plugin.clone(),
                                runtime: runtime.clone(),
                                message: "runtime provider preparation cancelled".into(),
                            });
                        }
                        prepared
                            .map(Some)
                            .map_err(|message| KernelError::RuntimePrepare {
                                plugin: plugin.clone(),
                                runtime: runtime.clone(),
                                message,
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
                            runtime: &self.runtime_generation,
                            states: &next_states,
                            instances: &next_instances,
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
                let host = PluginHost {
                    graph_generation: self.graph_generation(),
                    component_graph: self.component_graph(),
                    dispatch_topology: self.dispatch_topology(),
                    config: &config,
                    states: &next_states,
                    instances: &next_instances,
                    plugin,
                    authority: &manifest.maximum_authority,
                    transaction_context: TransactionContext::unscoped(),
                    call_cancellation: Some(cancellation.clone()),
                    call_stack: BTreeSet::from([plugin.clone()]),
                    events: &self.events,
                    tasks: &self.tasks,
                    persistence: &self.persistence,
                    prepared_mutations: &prepared_mutations,
                    trace_sink: self.trace_sink.as_ref(),
                    provenance: &self.provenance,
                    continuation: None,
                    active_services: BTreeSet::new(),
                    active_component_endpoints: BTreeSet::new(),
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
                            runtime: &self.runtime_generation,
                            states: &next_states,
                            instances: &next_instances,
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
                next_instances.insert(plugin.clone(), Arc::new(Mutex::new(instance)));
            }
            next_states.insert(plugin.clone(), PluginState::Active);
            staged.push(plugin.clone());
        }

        let subscriptions = match self.graph_generation() {
            Some(_generation) => stage_listener_subscriptions(listener::ListenerRuntimeSources {
                runtime: &self.runtime_generation,
                states: &next_states,
                instances: &next_instances,
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
                        runtime: &self.runtime_generation,
                        states: &next_states,
                        instances: &next_instances,
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

        self.events.replace_subscriptions(subscriptions)?;
        self.states = next_states;
        self.instances = next_instances;
        self.runtime_active = true;
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
        let transactions = TransactionContext::unscoped();
        invoke_component_service_with(
            InvocationContext {
                graph_generation: self.graph_generation(),
                component_graph: self.component_graph(),
                dispatch_topology: self.dispatch_topology(),
                config: self.config(),
                states: &self.states,
                instances: &self.instances,
                events: &self.events,
                tasks: &self.tasks,
                persistence: &self.persistence,
                prepared_mutations: &prepared_mutations,
                transactions: &transactions,
                trace_sink: self.trace_sink.as_ref(),
                provenance: &self.provenance,
            },
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
            caller_authority,
            ServiceDispatchGuards {
                call_stack: &BTreeSet::new(),
                active_services: &BTreeSet::new(),
                active_component_endpoints: &BTreeSet::new(),
                terminal_component: Some(component),
            },
        )
    }

    pub fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        caller_authority: &Authority,
        binding: Option<&PluginId>,
    ) -> Result<Vec<u8>, KernelError> {
        let prepared_mutations = PreparedMutationScope::new(self.graph_generation());
        let transactions = TransactionContext::unscoped();
        invoke_service_with(
            InvocationContext {
                graph_generation: self.graph_generation(),
                component_graph: self.component_graph(),
                dispatch_topology: self.dispatch_topology(),
                config: self.config(),
                states: &self.states,
                instances: &self.instances,
                events: &self.events,
                tasks: &self.tasks,
                persistence: &self.persistence,
                prepared_mutations: &prepared_mutations,
                transactions: &transactions,
                trace_sink: self.trace_sink.as_ref(),
                provenance: &self.provenance,
            },
            service,
            input,
            caller_authority,
            binding,
            ServiceDispatchGuards {
                call_stack: &BTreeSet::new(),
                active_services: &BTreeSet::new(),
                active_component_endpoints: &BTreeSet::new(),
                terminal_component: None,
            },
        )
    }

    pub fn stop(&mut self, plugin: &PluginId) -> Result<(), KernelError> {
        let manifest = self
            .config()
            .manifest(plugin)
            .ok_or_else(|| KernelError::UnknownPlugin(plugin.clone()))?;
        let generation = self.graph_generation();
        self.tasks.cancel_calls(plugin, generation);
        self.tasks.cancel_plugin_generation(plugin, generation);
        if let Some(instance) = self.instances.get(plugin) {
            let live_call = self.tasks.begin_call(plugin, generation);
            let cancellation = live_call.cancellation_token().clone();
            let prepared_mutations = PreparedMutationScope::new(generation);
            let host = PluginHost {
                graph_generation: generation,
                component_graph: self.component_graph(),
                dispatch_topology: self.dispatch_topology(),
                config: self.config(),
                states: &self.states,
                instances: &self.instances,
                plugin,
                authority: &manifest.maximum_authority,
                transaction_context: TransactionContext::unscoped(),
                call_cancellation: Some(cancellation.clone()),
                call_stack: BTreeSet::from([plugin.clone()]),
                events: &self.events,
                tasks: &self.tasks,
                persistence: &self.persistence,
                prepared_mutations: &prepared_mutations,
                trace_sink: self.trace_sink.as_ref(),
                provenance: &self.provenance,
                continuation: None,
                active_services: BTreeSet::new(),
                active_component_endpoints: BTreeSet::new(),
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
        self.instances.remove(plugin);
        let state = self
            .states
            .get_mut(plugin)
            .expect("plugin manifest and lifecycle state stay aligned");
        *state = PluginState::Stopped;
        self.events
            .publish(KernelEvent::PluginStopped(plugin.clone()));
        Ok(())
    }
}
