use super::*;
use crate::ResolvedHarness;

impl Kernel {
    /// Return every generation that can accept a new root execution.
    ///
    /// The default generation is included with resident alternatives.
    pub fn resident_generation_ids(&self) -> Vec<GraphGenerationId> {
        let mut generations = self
            .resident_generations
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        if let Some(default) = self.graph_generation() {
            generations.push(default.clone());
        }
        generations.sort();
        generations.dedup();
        generations
    }

    /// Capture host-owned constraints from the current default generation.
    ///
    /// The authority is permanently attenuated by the kernel's initial ceiling.
    /// Selected component imports are pinned to their exact resolved provider
    /// endpoint, including execution artifact and effective authority.
    pub fn capture_root_execution_constraints(
        &self,
        caller_authority: &Authority,
        pinned_bindings: impl IntoIterator<Item = (ComponentId, InterfaceId)>,
    ) -> Result<RootExecutionConstraints, KernelError> {
        let authority = self.authority_ceiling.as_ref().map_or_else(
            || caller_authority.clone(),
            |ceiling| caller_authority.attenuate(ceiling),
        );
        let mut pinned = BTreeMap::new();
        for (component, interface) in pinned_bindings {
            let plan = self
                .component_graph()
                .provider_plan(&component, &interface)
                .map_err(KernelError::ComponentGraph)?
                .ok_or_else(|| KernelError::PinnedBindingUnavailable {
                    component: component.clone(),
                    interface: interface.clone(),
                })?;
            pinned.insert((component, interface), plan.primary().clone());
        }
        Ok(RootExecutionConstraints {
            authority,
            pinned_bindings: pinned,
        })
    }

    /// Stage one resolved Harness beside the current default generation.
    ///
    /// Residency does not change the default generation or ambient listener set.
    /// The first implementation requires identical durable schemas so trial
    /// execution cannot migrate shared persistence underneath another resident
    /// generation.
    #[cfg(test)]
    pub(crate) fn make_generation_resident(
        &mut self,
        candidate: &ResolvedHarness,
    ) -> Result<GraphGenerationId, KernelError> {
        let trusted_host_constraints = RootExecutionConstraints {
            authority: candidate.authority_ceiling().clone(),
            pinned_bindings: BTreeMap::new(),
        };
        self.make_generation_resident_under_constraints(candidate, &trusted_host_constraints)
    }

    pub(crate) fn make_generation_resident_under_constraints(
        &mut self,
        candidate: &ResolvedHarness,
        constraints: &RootExecutionConstraints,
    ) -> Result<GraphGenerationId, KernelError> {
        let candidate_generation = candidate.generation().clone();
        self.validate_generation_authority(candidate)?;
        Self::validate_component_graph_root_execution_constraints(
            candidate.component_graph(),
            &candidate_generation,
            constraints,
        )?;
        if self.graph_generation() == Some(&candidate_generation)
            || self
                .resident_generations
                .contains_key(&candidate_generation)
        {
            return Ok(candidate_generation);
        }

        let active_generation = self
            .graph_generation()
            .cloned()
            .ok_or(KernelError::ResolvedGenerationMissing)?;
        if candidate.durable_schemas() != self.generation_state.durable_schemas.as_slice() {
            return Err(KernelError::ResidentGenerationDurableMismatch {
                active: active_generation,
                candidate: candidate_generation,
            });
        }

        let lifecycle_constraints = constraints.with_authority(
            candidate
                .authority_ceiling()
                .attenuate(constraints.authority()),
        );
        let state = self.stage_resident_generation(candidate, &lifecycle_constraints)?;
        self.events.replace_generation_subscriptions(
            candidate_generation.clone(),
            state.subscriptions.clone(),
        )?;
        self.resident_generations
            .insert(candidate_generation.clone(), state);
        Ok(candidate_generation)
    }

    /// Invoke one root against an explicitly selected resident generation.
    ///
    /// Nested calls remain pinned because the selected RuntimeGeneration and
    /// generation-local instances enter the root CallScope together.
    pub fn invoke_in_generation(
        &mut self,
        generation: &GraphGenerationId,
        service: &ServiceId,
        input: &[u8],
        constraints: &RootExecutionConstraints,
        binding: Option<&PluginId>,
    ) -> Result<Vec<u8>, KernelError> {
        let state = if self.graph_generation() == Some(generation) {
            &self.generation_state
        } else {
            self.resident_generations
                .get(generation)
                .ok_or_else(|| KernelError::UnknownGeneration(generation.clone()))?
        };
        Self::validate_root_execution_constraints(state, generation, constraints)?;
        let effective_constraints = RootExecutionConstraints {
            authority: state.constrain_root_authority(constraints.authority()),
            pinned_bindings: constraints.pinned_bindings.clone(),
        };

        let prepared_mutations = PreparedMutationScope::new(Some(generation));
        let runtime = RuntimeServices {
            states: &state.states,
            instances: &state.instances,
            invocations: &state.invocations,
            events: &self.events,
            tasks: &self.tasks,
            persistence: &self.persistence,
            prepared_mutations: &prepared_mutations,
            trace_sink: self.trace_sink.as_ref(),
            provenance: &self.provenance,
        };
        let scope = CallScope::external_with_constraints(
            Arc::new(state.runtime.clone()),
            &effective_constraints,
        );
        dispatch::invoke_service_with(runtime, service, input, binding, scope)
    }

    fn validate_root_execution_constraints(
        state: &GenerationRuntimeState,
        generation: &GraphGenerationId,
        constraints: &RootExecutionConstraints,
    ) -> Result<(), KernelError> {
        Self::validate_component_graph_root_execution_constraints(
            state.runtime.component_graph(),
            generation,
            constraints,
        )
    }

    fn validate_component_graph_root_execution_constraints(
        component_graph: &ResolvedComponentGraph,
        generation: &GraphGenerationId,
        constraints: &RootExecutionConstraints,
    ) -> Result<(), KernelError> {
        for ((component, interface), pinned) in &constraints.pinned_bindings {
            let matches = component_graph
                .provider_plan(component, interface)
                .ok()
                .flatten()
                .is_some_and(|plan| plan.primary() == pinned);
            if !matches {
                return Err(KernelError::PinnedBindingChanged {
                    generation: generation.clone(),
                    component: component.clone(),
                    interface: interface.clone(),
                });
            }
        }
        Ok(())
    }

    /// Make a resident generation the default for future unqualified roots and
    /// ambient event delivery. The previous default remains resident.
    pub(crate) fn promote_generation_under_constraints(
        &mut self,
        generation: &GraphGenerationId,
        constraints: &RootExecutionConstraints,
    ) -> Result<(), KernelError> {
        let state = if self.graph_generation() == Some(generation) {
            &self.generation_state
        } else {
            self.resident_generations
                .get(generation)
                .ok_or_else(|| KernelError::UnknownGeneration(generation.clone()))?
        };
        Self::validate_root_execution_constraints(state, generation, constraints)?;
        self.promote_generation(generation)
    }

    fn promote_generation(&mut self, generation: &GraphGenerationId) -> Result<(), KernelError> {
        if self.graph_generation() == Some(generation) {
            return Ok(());
        }

        let current_generation = self
            .graph_generation()
            .cloned()
            .ok_or(KernelError::ResolvedGenerationMissing)?;
        let candidate = self
            .resident_generations
            .remove(generation)
            .ok_or_else(|| KernelError::UnknownGeneration(generation.clone()))?;

        // Preserve the current default under its generation before changing
        // ambient delivery. Activation normally installs this snapshot already;
        // promotion enforces the invariant at the lifecycle boundary.
        self.events.replace_generation_subscriptions(
            current_generation.clone(),
            self.generation_state.subscriptions.clone(),
        )?;

        if let Err(error) = self
            .events
            .replace_subscriptions(candidate.subscriptions.clone())
        {
            self.resident_generations
                .insert(generation.clone(), candidate);
            return Err(error.into());
        }

        let previous = std::mem::replace(&mut self.generation_state, candidate);
        self.resident_generations
            .insert(current_generation, previous);
        Ok(())
    }

    /// Stop and remove a non-default resident generation.
    ///
    /// Explicit retirement cancels that generation's plugin calls and tasks
    /// through the existing generation-aware stop path.
    pub(crate) fn retire_generation_under_constraints(
        &mut self,
        generation: &GraphGenerationId,
        constraints: &RootExecutionConstraints,
    ) -> Result<(), KernelError> {
        self.retire_generation_with_constraints(generation, Some(constraints))
    }

    #[cfg(test)]
    pub(crate) fn retire_generation(
        &mut self,
        generation: &GraphGenerationId,
    ) -> Result<(), KernelError> {
        self.retire_generation_with_constraints(generation, None)
    }

    fn retire_generation_with_constraints(
        &mut self,
        generation: &GraphGenerationId,
        operation_constraints: Option<&RootExecutionConstraints>,
    ) -> Result<(), KernelError> {
        if self.graph_generation() == Some(generation) {
            return Err(KernelError::DefaultGenerationCannotRetire(
                generation.clone(),
            ));
        }

        if let Some(constraints) = operation_constraints {
            let state = self
                .resident_generations
                .get(generation)
                .ok_or_else(|| KernelError::UnknownGeneration(generation.clone()))?;
            Self::validate_root_execution_constraints(state, generation, constraints)?;
        }

        let state = self
            .resident_generations
            .remove(generation)
            .ok_or_else(|| KernelError::UnknownGeneration(generation.clone()))?;

        // Invalidate generation-local admission before teardown. Deliveries that
        // already captured this subscription set observe the revision change and
        // cancel before starting another listener level.
        self.events.remove_generation_subscriptions(generation);

        let lifecycle_constraints =
            match (state.lifecycle_constraints.as_ref(), operation_constraints) {
                (Some(stored), Some(operation)) => Some(stored.with_authority_and_additional_pins(
                    stored.authority().attenuate(operation.authority()),
                    operation,
                )),
                (Some(stored), None) => Some(stored.clone()),
                (None, Some(operation)) => Some(operation.clone()),
                (None, None) => None,
            };
        let stop_view = reconciliation::StopView {
            runtime: &state.runtime,
            lifecycle_constraints: lifecycle_constraints.as_ref(),
            states: &state.states,
            instances: &state.instances,
            invocations: &state.invocations,
            events: &self.events,
            tasks: &self.tasks,
            persistence: &self.persistence,
            trace_sink: self.trace_sink.as_ref(),
            provenance: &self.provenance,
        };
        for plugin in state.runtime.config().activation_order().iter().rev() {
            if let Some(instance) = state.instances.get(plugin) {
                stop_view.stop(plugin, instance);
                self.events
                    .publish(KernelEvent::PluginStopped(plugin.clone()));
            }
        }
        Ok(())
    }

    fn stage_resident_generation(
        &self,
        candidate: &ResolvedHarness,
        lifecycle_constraints: &RootExecutionConstraints,
    ) -> Result<GenerationRuntimeState, KernelError> {
        let runtime = candidate.runtime_generation().clone();
        let config = runtime.config().clone();
        let generation = runtime
            .generation()
            .ok_or(KernelError::ResolvedGenerationMissing)?
            .clone();
        let mut states = config
            .manifests()
            .map(|manifest| (manifest.id.clone(), PluginState::Registered))
            .collect::<BTreeMap<_, _>>();
        let mut instances = BTreeMap::new();
        let mut invocations = BTreeMap::new();
        let mut staged = Vec::new();

        for plugin in config.activation_order() {
            let manifest = config
                .manifest(plugin)
                .expect("activation order only contains configured plugins");
            let instance = (|| -> Result<Option<Box<dyn PluginInstance>>, KernelError> {
                match &manifest.execution {
                    PluginExecution::ResourceOnly => Ok(None),
                    PluginExecution::Embedded => self
                        .embedded_factories
                        .get(plugin)
                        .map(|factory| factory())
                        .map(Some)
                        .ok_or_else(|| KernelError::EmbeddedFactoryMissing(plugin.clone())),
                    PluginExecution::Runtime {
                        runtime: runtime_id,
                        artifact,
                    } => {
                        let binding = config.runtime_binding(plugin).cloned().ok_or_else(|| {
                            KernelError::RuntimeProviderUnavailable(runtime_id.clone())
                        })?;
                        let provider_manifest = config
                            .manifest(&binding.provider)
                            .expect("resolved runtime provider is configured");
                        let provider_authority = constrain_authority_to_ceiling(
                            Some(lifecycle_constraints.authority()),
                            &provider_manifest.maximum_authority,
                        );
                        let guest_authority = constrain_authority_to_ceiling(
                            Some(lifecycle_constraints.authority()),
                            &manifest.maximum_authority,
                        );
                        let provider =
                            instances.get(&binding.provider).cloned().ok_or_else(|| {
                                KernelError::PluginNotActive(binding.provider.clone())
                            })?;
                        let live_call = self.tasks.begin_call(&binding.provider, Some(&generation));
                        let cancellation = live_call.cancellation_token().clone();
                        let prepared_mutations = PreparedMutationScope::new(Some(&generation));
                        let host = PluginHost {
                            runtime: RuntimeServices {
                                states: &states,
                                instances: &instances,
                                invocations: &invocations,
                                events: &self.events,
                                tasks: &self.tasks,
                                persistence: &self.persistence,
                                prepared_mutations: &prepared_mutations,
                                trace_sink: self.trace_sink.as_ref(),
                                provenance: &self.provenance,
                            },
                            plugin: &binding.provider,
                            scope: CallScope::root_with_constraints(
                                Arc::new(runtime.clone()),
                                &binding.provider,
                                &provider_authority,
                                lifecycle_constraints,
                                Some(cancellation.clone()),
                            ),
                            continuation: None,
                        };
                        let mut provider = provider.lock().expect("plugin instance mutex poisoned");
                        let contract = provider.runtime_provider().ok_or_else(|| {
                            KernelError::RuntimeProviderContractUnavailable {
                                runtime: runtime_id.clone(),
                                provider: binding.provider.clone(),
                            }
                        })?;
                        let prepared = catch_unwind(AssertUnwindSafe(|| {
                            contract.prepare_with_host(
                                RuntimePluginCandidate {
                                    manifest,
                                    artifact,
                                    guest_authority: &guest_authority,
                                },
                                &host,
                            )
                        }))
                        .map_err(|_| KernelError::RuntimePrepare {
                            plugin: plugin.clone(),
                            runtime: runtime_id.clone(),
                            message: "runtime provider panicked".into(),
                        })?;
                        if cancellation.is_cancelled() {
                            prepared_mutations.clear();
                            return Err(KernelError::RuntimePrepare {
                                plugin: plugin.clone(),
                                runtime: runtime_id.clone(),
                                message: "runtime provider preparation cancelled".into(),
                            });
                        }
                        prepared
                            .map(Some)
                            .map_err(|message| KernelError::RuntimePrepare {
                                plugin: plugin.clone(),
                                runtime: runtime_id.clone(),
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
                            runtime: &runtime,
                            lifecycle_constraints: Some(lifecycle_constraints),
                            states: &states,
                            instances: &instances,
                            invocations: &invocations,
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
                let live_call = self.tasks.begin_call(plugin, Some(&generation));
                let cancellation = live_call.cancellation_token().clone();
                let prepared_mutations = PreparedMutationScope::new(Some(&generation));
                let plugin_authority = constrain_authority_to_ceiling(
                    Some(lifecycle_constraints.authority()),
                    &manifest.maximum_authority,
                );
                let host = PluginHost {
                    runtime: RuntimeServices {
                        states: &states,
                        instances: &instances,
                        invocations: &invocations,
                        events: &self.events,
                        tasks: &self.tasks,
                        persistence: &self.persistence,
                        prepared_mutations: &prepared_mutations,
                        trace_sink: self.trace_sink.as_ref(),
                        provenance: &self.provenance,
                    },
                    plugin,
                    scope: CallScope::root_with_constraints(
                        Arc::new(runtime.clone()),
                        plugin,
                        &plugin_authority,
                        lifecycle_constraints,
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
                        .cancel_plugin_generation(plugin, Some(&generation));
                    reconciliation::cleanup_staged(
                        &staged,
                        reconciliation::StopView {
                            runtime: &runtime,
                            lifecycle_constraints: Some(lifecycle_constraints),
                            states: &states,
                            instances: &instances,
                            invocations: &invocations,
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
                instances.insert(plugin.clone(), Arc::clone(&instance));
                invocations.insert(plugin.clone(), invocation);
            }

            states.insert(plugin.clone(), PluginState::Active);
            staged.push(plugin.clone());
        }

        let subscriptions = stage_listener_subscriptions(listener::ListenerRuntimeSources {
            runtime: &runtime,
            states: &states,
            instances: &instances,
            invocations: &invocations,
            events: &self.events,
            tasks: &self.tasks,
            persistence: &self.persistence,
            trace_sink: &self.trace_sink,
            provenance: &self.provenance,
        })
        .inspect_err(|_| {
            reconciliation::cleanup_staged(
                &staged,
                reconciliation::StopView {
                    runtime: &runtime,
                    lifecycle_constraints: Some(lifecycle_constraints),
                    states: &states,
                    instances: &instances,
                    invocations: &invocations,
                    events: &self.events,
                    tasks: &self.tasks,
                    persistence: &self.persistence,
                    trace_sink: self.trace_sink.as_ref(),
                    provenance: &self.provenance,
                },
            );
        })?;

        Ok(GenerationRuntimeState {
            runtime,
            authority_ceiling: Some(candidate.authority_ceiling().clone()),
            lifecycle_constraints: Some(lifecycle_constraints.clone()),
            durable_schemas: candidate.durable_schemas().to_vec(),
            subscriptions,
            states,
            instances,
            invocations,
            active: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ComponentManifest, DurableSchema, DurableSchemaRegistration, PluginManifest,
        ResolvedHarnessActivation, ResourceNamespace, ServiceContribution,
    };

    fn plugin(value: &str) -> PluginId {
        PluginId::parse(value).unwrap()
    }

    fn service() -> ServiceId {
        ServiceId::parse("fixture.residency.echo@1").unwrap()
    }

    fn manifest(id: &str) -> PluginManifest {
        PluginManifest {
            id: plugin(id),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                service: service(),
                role: ServiceRole::Terminal,
                priority: 0,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    struct Echo(&'static [u8]);

    impl PluginInstance for Echo {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            _service: &ServiceId,
            _input: &[u8],
            _host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            Ok(self.0.to_vec())
        }
    }

    type LifecycleObservation = (&'static str, GraphGenerationId, bool);

    struct AuthorityEcho {
        capability: CapabilityId,
        lifecycle: Arc<Mutex<Vec<LifecycleObservation>>>,
    }

    impl AuthorityEcho {
        fn record(&self, phase: &'static str, host: &PluginHost<'_>) {
            self.lifecycle
                .lock()
                .expect("authority lifecycle observation mutex poisoned")
                .push((
                    phase,
                    host.graph_generation()
                        .expect("resolved lifecycle call has a generation")
                        .clone(),
                    host.authority().permits(&self.capability),
                ));
        }
    }

    impl PluginInstance for AuthorityEcho {
        fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
            self.record("start", host);
            Ok(())
        }

        fn invoke(
            &mut self,
            _service: &ServiceId,
            _input: &[u8],
            host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            Ok(if host.authority().permits(&self.capability) {
                b"permitted".to_vec()
            } else {
                b"denied".to_vec()
            })
        }

        fn stop(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
            self.record("stop", host);
            Ok(())
        }
    }

    fn listener_event() -> EventTypeId {
        EventTypeId::parse("fixture.residency.observed").unwrap()
    }

    struct GenerationEventListener {
        seen: Arc<Mutex<Vec<GraphGenerationId>>>,
    }

    impl PluginListener for GenerationEventListener {
        fn handle(&self, _event: &EventEnvelope, host: &PluginHost<'_>) -> Result<(), String> {
            self.seen
                .lock()
                .expect("listener observation mutex poisoned")
                .push(
                    host.graph_generation()
                        .expect("resolved listener has a graph generation")
                        .clone(),
                );
            Ok(())
        }
    }

    struct GenerationEventPlugin {
        seen: Arc<Mutex<Vec<GraphGenerationId>>>,
    }

    impl PluginInstance for GenerationEventPlugin {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn bind_plugin_listener(
            &mut self,
            _listener: &ResolvedListener,
            _generation: &GraphGenerationId,
        ) -> Option<Result<Arc<dyn PluginListener>, String>> {
            Some(Ok(Arc::new(GenerationEventListener {
                seen: Arc::clone(&self.seen),
            })))
        }

        fn invoke(
            &mut self,
            _service: &ServiceId,
            _input: &[u8],
            host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            let receipt = host
                .dispatch_event(listener_event(), 1, 7, 0, Vec::new())
                .map_err(|error| error.to_string())?;
            match receipt.wait() {
                crate::EventDeliveryStatus::Succeeded(report) if report.failures.is_empty() => {
                    Ok(b"delivered".to_vec())
                }
                status => Err(format!("event delivery failed: {status:?}")),
            }
        }
    }

    #[test]
    fn ambient_and_causal_listener_delivery_use_only_the_selected_generation() {
        let plugin_id = plugin("fixture.residency.events");
        let component_id = ComponentId::parse("fixture.residency.events").unwrap();
        let first_manifest = manifest(plugin_id.as_str());
        let mut second_manifest = first_manifest.clone();
        second_manifest.version += 1;
        let component = ComponentManifest {
            id: component_id,
            owner: plugin_id.clone(),
            imports: Vec::new(),
            exports: Vec::new(),
            listeners: vec![crate::ComponentListener {
                id: crate::SubscriptionId::parse("fixture.residency.events/observed").unwrap(),
                event: listener_event(),
                event_version: 1,
                method: "observed".into(),
                payload_schema: crate::PhenixSchema::Any,
                projection: crate::ListenerProjection::Exact,
                dependencies: Vec::new(),
                failure_policy: crate::EventFailurePolicy::FailDelivery,
                required_authority: Authority::default(),
            }],
            maximum_authority: Authority::default(),
        };
        let first = ResolvedHarness::resolve(
            [first_manifest.clone()],
            [component.clone()],
            [],
            &Authority::default(),
        )
        .unwrap();
        let second =
            ResolvedHarness::resolve([second_manifest], [component], [], &Authority::default())
                .unwrap();
        let first_generation = first.generation().clone();
        let second_generation = second.generation().clone();
        let seen = Arc::new(Mutex::new(Vec::new()));

        let mut kernel = Kernel::new(first.kernel_config().clone());
        kernel.activate_resolved_harness(&first).unwrap();
        let seen_for_factory = Arc::clone(&seen);
        kernel.preload_embedded_factory(plugin_id.clone(), move || {
            Box::new(GenerationEventPlugin {
                seen: Arc::clone(&seen_for_factory),
            })
        });
        kernel.activate_all().unwrap();
        kernel.make_generation_resident(&second).unwrap();

        let ambient = EventEnvelope {
            event_type: listener_event(),
            version: 1,
            emitter: plugin_id,
            causality_id: 6,
            kernel_policy_revision: 0,
            payload: Vec::new(),
        };
        let report = kernel
            .events()
            .dispatch(&ambient, &Authority::default())
            .unwrap();
        assert!(report.failures.is_empty());
        assert_eq!(
            seen.lock()
                .expect("listener observation mutex poisoned")
                .as_slice(),
            std::slice::from_ref(&first_generation)
        );
        seen.lock()
            .expect("listener observation mutex poisoned")
            .clear();

        let constraints = kernel
            .capture_root_execution_constraints(&Authority::default(), [])
            .unwrap();
        assert_eq!(
            kernel
                .invoke_in_generation(&second_generation, &service(), &[], &constraints, None,)
                .unwrap(),
            b"delivered"
        );
        assert_eq!(
            seen.lock()
                .expect("listener observation mutex poisoned")
                .as_slice(),
            std::slice::from_ref(&second_generation)
        );

        kernel.promote_generation(&second_generation).unwrap();
        seen.lock()
            .expect("listener observation mutex poisoned")
            .clear();

        let report = kernel
            .events()
            .dispatch(&ambient, &Authority::default())
            .unwrap();
        assert!(report.failures.is_empty());
        assert_eq!(
            seen.lock()
                .expect("listener observation mutex poisoned")
                .as_slice(),
            std::slice::from_ref(&second_generation)
        );
        seen.lock()
            .expect("listener observation mutex poisoned")
            .clear();

        assert_eq!(
            kernel
                .invoke_in_generation(&first_generation, &service(), &[], &constraints, None)
                .unwrap(),
            b"delivered"
        );
        assert_eq!(
            seen.lock()
                .expect("listener observation mutex poisoned")
                .as_slice(),
            std::slice::from_ref(&first_generation)
        );
    }

    #[test]
    fn selected_generation_ceiling_attenuates_explicit_and_default_roots() {
        let read = CapabilityId::parse("fixture.read").unwrap();
        let write = CapabilityId::parse("fixture.write").unwrap();
        let broad = Authority::new([read.clone(), write.clone()]);
        let narrow = Authority::new([read]);
        let mut plugin_manifest = manifest("fixture.residency.authority");
        plugin_manifest.maximum_authority = broad.clone();

        let first = ResolvedHarness::resolve([plugin_manifest.clone()], [], [], &broad).unwrap();
        let second = ResolvedHarness::resolve([plugin_manifest.clone()], [], [], &narrow).unwrap();
        let first_generation = first.generation().clone();
        let second_generation = second.generation().clone();

        let mut kernel = Kernel::new(first.kernel_config().clone());
        kernel.activate_resolved_harness(&first).unwrap();
        let write_for_factory = write.clone();
        let lifecycle = Arc::new(Mutex::new(Vec::new()));
        let lifecycle_for_factory = Arc::clone(&lifecycle);
        kernel.preload_embedded_factory(plugin_manifest.id, move || {
            Box::new(AuthorityEcho {
                capability: write_for_factory.clone(),
                lifecycle: Arc::clone(&lifecycle_for_factory),
            })
        });
        kernel.activate_all().unwrap();
        let constraints = kernel
            .capture_root_execution_constraints(&broad, [])
            .unwrap();

        assert_eq!(
            kernel.invoke(&service(), &[], &broad, None).unwrap(),
            b"permitted"
        );

        kernel.make_generation_resident(&second).unwrap();
        assert_eq!(
            kernel
                .invoke_in_generation(&second_generation, &service(), &[], &constraints, None)
                .unwrap(),
            b"denied"
        );

        assert_eq!(
            lifecycle
                .lock()
                .expect("authority lifecycle observation mutex poisoned")
                .as_slice(),
            &[
                ("start", first_generation.clone(), true),
                ("start", second_generation.clone(), false),
            ]
        );

        kernel.promote_generation(&second_generation).unwrap();
        assert_eq!(
            kernel.invoke(&service(), &[], &broad, None).unwrap(),
            b"denied"
        );

        kernel.promote_generation(&first_generation).unwrap();
        kernel.retire_generation(&second_generation).unwrap();
        assert_eq!(
            lifecycle
                .lock()
                .expect("authority lifecycle observation mutex poisoned")
                .as_slice(),
            &[
                ("start", first_generation, true),
                ("start", second_generation.clone(), false),
                ("stop", second_generation, false),
            ]
        );
    }

    #[test]
    fn resident_lifecycle_is_bounded_by_admitting_root_authority() {
        let read = CapabilityId::parse("fixture.read").unwrap();
        let write = CapabilityId::parse("fixture.write").unwrap();
        let broad = Authority::new([read.clone(), write.clone()]);
        let narrow = Authority::new([read]);

        let mut first_manifest = manifest("fixture.residency.admission-authority");
        first_manifest.maximum_authority = broad.clone();
        let mut second_manifest = first_manifest.clone();
        second_manifest.version += 1;

        let first = ResolvedHarness::resolve([first_manifest.clone()], [], [], &broad).unwrap();
        let second = ResolvedHarness::resolve([second_manifest], [], [], &broad).unwrap();
        let first_generation = first.generation().clone();
        let second_generation = second.generation().clone();

        let lifecycle = Arc::new(Mutex::new(Vec::new()));
        let lifecycle_for_factory = Arc::clone(&lifecycle);
        let write_for_factory = write.clone();

        let mut kernel = Kernel::new(first.kernel_config().clone());
        kernel.activate_resolved_harness(&first).unwrap();
        kernel.preload_embedded_factory(first_manifest.id, move || {
            Box::new(AuthorityEcho {
                capability: write_for_factory.clone(),
                lifecycle: Arc::clone(&lifecycle_for_factory),
            })
        });
        kernel.activate_all().unwrap();

        let constraints = kernel
            .capture_root_execution_constraints(&narrow, [])
            .unwrap();
        kernel
            .make_generation_resident_under_constraints(&second, &constraints)
            .unwrap();

        assert_eq!(
            kernel
                .invoke_in_generation(&second_generation, &service(), &[], &constraints, None)
                .unwrap(),
            b"denied"
        );
        assert_eq!(
            lifecycle
                .lock()
                .expect("authority lifecycle observation mutex poisoned")
                .as_slice(),
            &[
                ("start", first_generation, true),
                ("start", second_generation.clone(), false),
            ]
        );

        kernel.retire_generation(&second_generation).unwrap();
        assert_eq!(
            lifecycle
                .lock()
                .expect("authority lifecycle observation mutex poisoned")
                .last(),
            Some(&("stop", second_generation, false))
        );
    }

    #[test]
    fn retirement_is_bounded_by_operation_authority() {
        let read = CapabilityId::parse("fixture.read").unwrap();
        let write = CapabilityId::parse("fixture.write").unwrap();
        let broad = Authority::new([read.clone(), write.clone()]);
        let narrow = Authority::new([read]);

        let mut first_manifest = manifest("fixture.residency.retire-authority");
        first_manifest.maximum_authority = broad.clone();
        let mut second_manifest = first_manifest.clone();
        second_manifest.version += 1;

        let first = ResolvedHarness::resolve([first_manifest.clone()], [], [], &broad).unwrap();
        let second = ResolvedHarness::resolve([second_manifest], [], [], &broad).unwrap();
        let first_generation = first.generation().clone();
        let second_generation = second.generation().clone();

        let lifecycle = Arc::new(Mutex::new(Vec::new()));
        let lifecycle_for_factory = Arc::clone(&lifecycle);
        let write_for_factory = write.clone();

        let mut kernel = Kernel::new(first.kernel_config().clone());
        kernel.activate_resolved_harness(&first).unwrap();
        kernel.preload_embedded_factory(first_manifest.id, move || {
            Box::new(AuthorityEcho {
                capability: write_for_factory.clone(),
                lifecycle: Arc::clone(&lifecycle_for_factory),
            })
        });
        kernel.activate_all().unwrap();
        kernel.make_generation_resident(&second).unwrap();

        let constraints = kernel
            .capture_root_execution_constraints(&narrow, [])
            .unwrap();
        kernel
            .retire_generation_under_constraints(&second_generation, &constraints)
            .unwrap();

        assert_eq!(
            lifecycle
                .lock()
                .expect("authority lifecycle observation mutex poisoned")
                .as_slice(),
            &[
                ("start", first_generation, true),
                ("start", second_generation.clone(), true),
                ("stop", second_generation, false),
            ]
        );
    }

    struct GenerationTaskPlugin {
        started: std::sync::mpsc::Sender<GraphGenerationId>,
        cancelled: std::sync::mpsc::Sender<GraphGenerationId>,
    }

    impl PluginInstance for GenerationTaskPlugin {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            _service: &ServiceId,
            _input: &[u8],
            host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            let scope = host
                .task_scope()
                .ok_or_else(|| "resolved invocation requires a task scope".to_owned())?;
            let started = self.started.clone();
            let cancelled = self.cancelled.clone();
            let _task = scope.spawn(&Authority::default(), move |token| {
                started
                    .send(token.graph_generation().clone())
                    .expect("task start observer remains connected");
                while !token.is_cancelled() {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                cancelled
                    .send(token.graph_generation().clone())
                    .expect("task cancellation observer remains connected");
            });
            Ok(b"spawned".to_vec())
        }
    }

    #[test]
    fn retiring_one_generation_cancels_only_its_tasks() {
        let plugin_id = plugin("fixture.residency.tasks");
        let first_manifest = manifest(plugin_id.as_str());
        let mut second_manifest = first_manifest.clone();
        second_manifest.version += 1;
        let first =
            ResolvedHarness::resolve([first_manifest.clone()], [], [], &Authority::default())
                .unwrap();
        let second =
            ResolvedHarness::resolve([second_manifest], [], [], &Authority::default()).unwrap();
        let first_generation = first.generation().clone();
        let second_generation = second.generation().clone();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (cancelled_tx, cancelled_rx) = std::sync::mpsc::channel();

        let mut kernel = Kernel::new(first.kernel_config().clone());
        kernel.activate_resolved_harness(&first).unwrap();
        kernel.preload_embedded_factory(plugin_id.clone(), move || {
            Box::new(GenerationTaskPlugin {
                started: started_tx.clone(),
                cancelled: cancelled_tx.clone(),
            })
        });
        kernel.activate_all().unwrap();
        let constraints = kernel
            .capture_root_execution_constraints(&Authority::default(), [])
            .unwrap();

        assert_eq!(
            kernel
                .invoke(&service(), &[], &Authority::default(), None)
                .unwrap(),
            b"spawned"
        );
        assert_eq!(
            started_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap(),
            first_generation
        );

        kernel.make_generation_resident(&second).unwrap();
        assert_eq!(
            kernel
                .invoke_in_generation(&second_generation, &service(), &[], &constraints, None,)
                .unwrap(),
            b"spawned"
        );
        assert_eq!(
            started_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap(),
            second_generation
        );

        kernel.promote_generation(&second_generation).unwrap();
        assert_eq!(
            cancelled_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        );
        kernel.retire_generation(&first_generation).unwrap();
        assert_eq!(
            cancelled_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap(),
            first_generation
        );
        assert_eq!(
            cancelled_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        );

        kernel.stop(&plugin_id).unwrap();
        assert_eq!(
            cancelled_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap(),
            second_generation
        );
    }

    #[test]
    fn explicit_roots_can_compare_promote_and_rollback_generations() {
        let first_manifest = manifest("fixture.residency.first");
        let second_manifest = manifest("fixture.residency.second");
        let first =
            ResolvedHarness::resolve([first_manifest.clone()], [], [], &Authority::default())
                .unwrap();
        let second =
            ResolvedHarness::resolve([second_manifest.clone()], [], [], &Authority::default())
                .unwrap();
        let first_generation = first.generation().clone();
        let second_generation = second.generation().clone();

        let mut kernel = Kernel::new(first.kernel_config().clone());
        kernel
            .activate_resolved_harness(&first)
            .expect("first generation activates");
        kernel.preload_embedded_factory(first_manifest.id.clone(), || Box::new(Echo(b"first")));
        kernel.preload_embedded_factory(second_manifest.id.clone(), || Box::new(Echo(b"second")));
        kernel.activate_all().unwrap();
        let constraints = kernel
            .capture_root_execution_constraints(&Authority::default(), [])
            .unwrap();

        kernel.make_generation_resident(&second).unwrap();
        assert_eq!(
            kernel
                .invoke(&service(), &[], &Authority::default(), None)
                .unwrap(),
            b"first"
        );
        assert_eq!(
            kernel
                .invoke_in_generation(&second_generation, &service(), &[], &constraints, None,)
                .unwrap(),
            b"second"
        );

        kernel.promote_generation(&second_generation).unwrap();
        assert_eq!(kernel.graph_generation(), Some(&second_generation));
        assert_eq!(
            kernel
                .invoke(&service(), &[], &Authority::default(), None)
                .unwrap(),
            b"second"
        );
        assert_eq!(
            kernel
                .invoke_in_generation(&first_generation, &service(), &[], &constraints, None,)
                .unwrap(),
            b"first"
        );

        kernel.promote_generation(&first_generation).unwrap();
        assert_eq!(kernel.graph_generation(), Some(&first_generation));
        assert_eq!(
            kernel
                .invoke(&service(), &[], &Authority::default(), None)
                .unwrap(),
            b"first"
        );

        kernel.retire_generation(&second_generation).unwrap();
        assert_eq!(
            kernel.invoke_in_generation(&second_generation, &service(), &[], &constraints, None,),
            Err(KernelError::UnknownGeneration(second_generation))
        );
    }

    #[test]
    fn failed_resident_staging_keeps_default_generation_usable() {
        let first_manifest = manifest("fixture.residency.first");
        let failing_manifest = manifest("fixture.residency.failing");
        let first =
            ResolvedHarness::resolve([first_manifest.clone()], [], [], &Authority::default())
                .unwrap();
        let failing =
            ResolvedHarness::resolve([failing_manifest.clone()], [], [], &Authority::default())
                .unwrap();
        let first_generation = first.generation().clone();
        let failing_generation = failing.generation().clone();

        let mut kernel = Kernel::new(first.kernel_config().clone());
        kernel.activate_resolved_harness(&first).unwrap();
        kernel.preload_embedded_factory(first_manifest.id, || Box::new(Echo(b"first")));
        kernel.activate_all().unwrap();

        assert_eq!(
            kernel.make_generation_resident(&failing),
            Err(KernelError::EmbeddedFactoryMissing(failing_manifest.id))
        );
        assert_eq!(kernel.graph_generation(), Some(&first_generation));
        assert!(!kernel
            .resident_generation_ids()
            .contains(&failing_generation));
        assert_eq!(
            kernel
                .invoke(&service(), &[], &Authority::default(), None)
                .unwrap(),
            b"first"
        );
    }

    #[test]
    fn selected_generation_cannot_change_a_pinned_binding() {
        let consumer = plugin("fixture.residency.consumer");
        let first_provider = plugin("fixture.residency.provider-a");
        let second_provider = plugin("fixture.residency.provider-b");
        let consumer_component = ComponentId::parse("fixture.residency.consumer").unwrap();
        let first_component = ComponentId::parse("fixture.residency.provider-a").unwrap();
        let second_component = ComponentId::parse("fixture.residency.provider-b").unwrap();
        let interface = InterfaceId::parse("fixture.residency.environment@1").unwrap();

        let plugin_only = |id: PluginId| PluginManifest {
            id,
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let consumer_manifest = plugin_only(consumer.clone());
        let first_provider_manifest = plugin_only(first_provider.clone());
        let second_provider_manifest = plugin_only(second_provider.clone());
        let consumer_node = ComponentManifest {
            id: consumer_component.clone(),
            owner: consumer.clone(),
            imports: vec![crate::ComponentImport {
                interface: interface.clone(),
                schema: Default::default(),
                required: true,
                authority: Authority::default(),
            }],
            exports: Vec::new(),
            listeners: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let provider_node = |id: ComponentId, owner: PluginId| ComponentManifest {
            id,
            owner,
            imports: Vec::new(),
            exports: vec![crate::ComponentExport {
                interface: interface.clone(),
                schema: Default::default(),
                priority: 100,
                required_authority: Authority::default(),
            }],
            listeners: Vec::new(),
            maximum_authority: Authority::default(),
        };

        let first = ResolvedHarness::resolve(
            [consumer_manifest.clone(), first_provider_manifest.clone()],
            [
                consumer_node.clone(),
                provider_node(first_component.clone(), first_provider.clone()),
            ],
            [],
            &Authority::default(),
        )
        .unwrap();
        let first_generation = first.generation().clone();
        let mut changed_provider_manifest = first_provider_manifest.clone();
        changed_provider_manifest.version += 1;
        let changed_provider = ResolvedHarness::resolve(
            [consumer_manifest.clone(), changed_provider_manifest],
            [
                consumer_node.clone(),
                provider_node(first_component.clone(), first_provider.clone()),
            ],
            [],
            &Authority::default(),
        )
        .unwrap();
        let changed_provider_generation = changed_provider.generation().clone();

        let changed_schema =
            crate::InterfaceSchema::new(crate::PhenixSchema::Never, crate::PhenixSchema::Never);
        let mut schema_consumer_node = consumer_node.clone();
        schema_consumer_node.imports[0].schema = changed_schema.clone();
        let mut schema_provider_node =
            provider_node(first_component.clone(), first_provider.clone());
        schema_provider_node.exports[0].schema = changed_schema;
        let schema_changed = ResolvedHarness::resolve(
            [consumer_manifest.clone(), first_provider_manifest],
            [schema_consumer_node, schema_provider_node],
            [],
            &Authority::default(),
        )
        .unwrap();
        let schema_changed_generation = schema_changed.generation().clone();

        let second = ResolvedHarness::resolve(
            [consumer_manifest, second_provider_manifest],
            [
                consumer_node,
                provider_node(second_component, second_provider.clone()),
            ],
            [],
            &Authority::default(),
        )
        .unwrap();
        let second_generation = second.generation().clone();

        let mut kernel = Kernel::new(first.kernel_config().clone());
        kernel.activate_resolved_harness(&first).unwrap();
        kernel.preload_embedded_factory(consumer, || Box::new(Echo(b"consumer")));
        kernel.preload_embedded_factory(first_provider, || Box::new(Echo(b"provider-a")));
        kernel.preload_embedded_factory(second_provider, || Box::new(Echo(b"provider-b")));
        kernel.activate_all().unwrap();
        let constraints = kernel
            .capture_root_execution_constraints(
                &Authority::default(),
                [(consumer_component.clone(), interface.clone())],
            )
            .unwrap();

        assert_eq!(
            kernel.make_generation_resident_under_constraints(&changed_provider, &constraints),
            Err(KernelError::PinnedBindingChanged {
                generation: changed_provider_generation,
                component: consumer_component.clone(),
                interface: interface.clone(),
            })
        );
        assert_eq!(
            kernel.make_generation_resident_under_constraints(&schema_changed, &constraints),
            Err(KernelError::PinnedBindingChanged {
                generation: schema_changed_generation,
                component: consumer_component.clone(),
                interface: interface.clone(),
            })
        );
        assert_eq!(
            kernel.make_generation_resident_under_constraints(&second, &constraints),
            Err(KernelError::PinnedBindingChanged {
                generation: second_generation.clone(),
                component: consumer_component.clone(),
                interface: interface.clone(),
            })
        );

        kernel.make_generation_resident(&second).unwrap();
        assert_eq!(
            kernel.promote_generation_under_constraints(&second_generation, &constraints),
            Err(KernelError::PinnedBindingChanged {
                generation: second_generation.clone(),
                component: consumer_component.clone(),
                interface: interface.clone(),
            })
        );
        assert_eq!(
            kernel.retire_generation_under_constraints(&second_generation, &constraints),
            Err(KernelError::PinnedBindingChanged {
                generation: second_generation.clone(),
                component: consumer_component,
                interface,
            })
        );
        assert!(kernel
            .resident_generation_ids()
            .contains(&second_generation));
        assert_eq!(kernel.graph_generation(), Some(&first_generation));
    }

    #[test]
    fn resident_candidate_cannot_expand_initial_authority_ceiling() {
        let read = CapabilityId::parse("fixture.read").unwrap();
        let write = CapabilityId::parse("fixture.write").unwrap();
        let initial_authority = Authority::new([read.clone()]);
        let broader_authority = Authority::new([read, write]);
        let first =
            ResolvedHarness::resolve([], [], [], &initial_authority).expect("initial resolves");
        let second =
            ResolvedHarness::resolve([], [], [], &broader_authority).expect("candidate resolves");
        let second_generation = second.generation().clone();

        let mut kernel = Kernel::new(first.kernel_config().clone());
        kernel.activate_resolved_harness(&first).unwrap();
        kernel.activate_all().unwrap();

        assert_eq!(
            kernel.make_generation_resident(&second),
            Err(KernelError::GenerationAuthorityExpansion(second_generation))
        );
    }

    #[test]
    fn resident_candidate_cannot_change_durable_schema() {
        let namespace =
            ResourceNamespace::parse("fixture.residency.state").expect("valid namespace");
        let owner = plugin("fixture.residency.owner");
        let mut durable_manifest = PluginManifest::resource_only(owner.clone());
        durable_manifest.resource_namespaces.push(namespace.clone());

        let first = ResolvedHarness::resolve_with_durable_schemas(
            [durable_manifest.clone()],
            [],
            [DurableSchemaRegistration::new(
                owner.clone(),
                DurableSchema::new(namespace.clone(), 1),
            )],
            [],
            &Authority::default(),
        )
        .unwrap();
        let second = ResolvedHarness::resolve_with_durable_schemas(
            [durable_manifest],
            [],
            [DurableSchemaRegistration::new(
                owner,
                DurableSchema::new(namespace, 2),
            )],
            [],
            &Authority::default(),
        )
        .unwrap();
        let first_generation = first.generation().clone();
        let second_generation = second.generation().clone();

        let mut kernel = Kernel::new(first.kernel_config().clone());
        kernel.activate_resolved_harness(&first).unwrap();
        kernel.activate_all().unwrap();

        assert_eq!(
            kernel.make_generation_resident(&second),
            Err(KernelError::ResidentGenerationDurableMismatch {
                active: first_generation,
                candidate: second_generation,
            })
        );
    }
    #[test]
    fn nested_dispatch_stays_inside_the_root_generation() {
        fn nested_service() -> ServiceId {
            ServiceId::parse("fixture.residency.nested@1").unwrap()
        }

        struct NestedCaller;

        impl PluginInstance for NestedCaller {
            fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
                Ok(())
            }

            fn invoke(
                &mut self,
                _service: &ServiceId,
                _input: &[u8],
                host: &PluginHost<'_>,
            ) -> Result<Vec<u8>, String> {
                host.invoke_service_abi(&nested_service(), &[], host.authority(), None)
                    .map_err(|error| error.to_string())
            }
        }

        let caller_id = plugin("fixture.residency.nested-caller");
        let first_provider_id = plugin("fixture.residency.nested-a");
        let second_provider_id = plugin("fixture.residency.nested-b");

        let caller_manifest = manifest(caller_id.as_str());
        let mut first_provider_manifest = manifest(first_provider_id.as_str());
        first_provider_manifest.services[0].service = nested_service();
        let mut second_provider_manifest = manifest(second_provider_id.as_str());
        second_provider_manifest.services[0].service = nested_service();

        let first = ResolvedHarness::resolve(
            [caller_manifest.clone(), first_provider_manifest],
            [],
            [],
            &Authority::default(),
        )
        .unwrap();
        let second = ResolvedHarness::resolve(
            [caller_manifest, second_provider_manifest],
            [],
            [],
            &Authority::default(),
        )
        .unwrap();
        let first_generation = first.generation().clone();
        let second_generation = second.generation().clone();

        let mut kernel = Kernel::new(first.kernel_config().clone());
        kernel.activate_resolved_harness(&first).unwrap();
        kernel.preload_embedded_factory(caller_id, || Box::new(NestedCaller));
        kernel.preload_embedded_factory(first_provider_id, || Box::new(Echo(b"A")));
        kernel.preload_embedded_factory(second_provider_id, || Box::new(Echo(b"B")));
        kernel.activate_all().unwrap();

        let constraints = kernel
            .capture_root_execution_constraints(&Authority::default(), [])
            .unwrap();
        kernel.make_generation_resident(&second).unwrap();

        assert_eq!(
            kernel
                .invoke_in_generation(&first_generation, &service(), &[], &constraints, None)
                .unwrap(),
            b"A"
        );
        assert_eq!(
            kernel
                .invoke_in_generation(&second_generation, &service(), &[], &constraints, None)
                .unwrap(),
            b"B"
        );

        kernel.promote_generation(&second_generation).unwrap();

        assert_eq!(
            kernel
                .invoke(&service(), &[], &Authority::default(), None)
                .unwrap(),
            b"B"
        );
        assert_eq!(
            kernel
                .invoke_in_generation(&first_generation, &service(), &[], &constraints, None)
                .unwrap(),
            b"A"
        );
    }

    #[test]
    fn resident_generations_share_kernel_owned_persistence() {
        struct DurableValue {
            namespace: ResourceNamespace,
        }

        impl PluginInstance for DurableValue {
            fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
                Ok(())
            }

            fn invoke(
                &mut self,
                _service: &ServiceId,
                input: &[u8],
                host: &PluginHost<'_>,
            ) -> Result<Vec<u8>, String> {
                if input == b"read" {
                    return host
                        .read_durable(&self.namespace, "value")
                        .map(|value| value.unwrap_or_default())
                        .map_err(|error| error.to_string());
                }

                host.transact_durable(
                    &self.namespace,
                    &[crate::TransactionOp::Put {
                        key: "value".into(),
                        value: input.to_vec(),
                    }],
                )
                .map_err(|error| error.to_string())?;
                Ok(input.to_vec())
            }
        }

        let owner = plugin("fixture.residency.persistence");
        let namespace = ResourceNamespace::parse("fixture.residency.persistence.state").unwrap();
        let read = CapabilityId::parse("kernel.persistence.read").unwrap();
        let write = CapabilityId::parse("kernel.persistence.write").unwrap();
        let authority = Authority::new([read, write]);

        let mut first_manifest = manifest(owner.as_str());
        first_manifest.resource_namespaces.push(namespace.clone());
        first_manifest.maximum_authority = authority.clone();
        let mut second_manifest = first_manifest.clone();
        second_manifest.version += 1;

        let schema =
            DurableSchemaRegistration::new(owner.clone(), DurableSchema::new(namespace.clone(), 1));
        let first = ResolvedHarness::resolve_with_durable_schemas(
            [first_manifest],
            [],
            [schema.clone()],
            [],
            &authority,
        )
        .unwrap();
        let second = ResolvedHarness::resolve_with_durable_schemas(
            [second_manifest],
            [],
            [schema],
            [],
            &authority,
        )
        .unwrap();
        let first_generation = first.generation().clone();
        let second_generation = second.generation().clone();

        let mut kernel = Kernel::new(first.kernel_config().clone());
        kernel.activate_resolved_harness(&first).unwrap();
        let namespace_for_factory = namespace.clone();
        kernel.preload_embedded_factory(owner, move || {
            Box::new(DurableValue {
                namespace: namespace_for_factory.clone(),
            })
        });
        kernel.activate_all().unwrap();

        let constraints = kernel
            .capture_root_execution_constraints(&authority, [])
            .unwrap();
        kernel.make_generation_resident(&second).unwrap();

        assert_eq!(
            kernel
                .invoke(&service(), b"from-a", &authority, None)
                .unwrap(),
            b"from-a"
        );
        assert_eq!(
            kernel
                .invoke_in_generation(&second_generation, &service(), b"read", &constraints, None,)
                .unwrap(),
            b"from-a"
        );
        assert_eq!(
            kernel
                .invoke_in_generation(
                    &second_generation,
                    &service(),
                    b"from-b",
                    &constraints,
                    None,
                )
                .unwrap(),
            b"from-b"
        );
        assert_eq!(
            kernel
                .invoke_in_generation(&first_generation, &service(), b"read", &constraints, None,)
                .unwrap(),
            b"from-b"
        );

        kernel.promote_generation(&second_generation).unwrap();
        assert_eq!(
            kernel
                .invoke(&service(), b"read", &authority, None)
                .unwrap(),
            b"from-b"
        );
        assert_eq!(
            kernel
                .invoke_in_generation(&first_generation, &service(), b"read", &constraints, None,)
                .unwrap(),
            b"from-b"
        );

        kernel.promote_generation(&first_generation).unwrap();
        assert_eq!(
            kernel
                .invoke(&service(), b"read", &authority, None)
                .unwrap(),
            b"from-b"
        );
        assert_eq!(
            kernel
                .invoke_in_generation(&second_generation, &service(), b"read", &constraints, None,)
                .unwrap(),
            b"from-b"
        );
    }
}
