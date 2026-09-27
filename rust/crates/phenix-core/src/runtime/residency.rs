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

    /// Stage one resolved Harness beside the current default generation.
    ///
    /// Residency does not change the default generation or ambient listener set.
    /// The first implementation requires identical durable schemas so trial
    /// execution cannot migrate shared persistence underneath another resident
    /// generation.
    pub(crate) fn make_generation_resident(
        &mut self,
        candidate: &ResolvedHarness,
    ) -> Result<GraphGenerationId, KernelError> {
        let candidate_generation = candidate.generation().clone();
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

        let state = self.stage_resident_generation(candidate)?;
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
        caller_authority: &Authority,
        binding: Option<&PluginId>,
    ) -> Result<Vec<u8>, KernelError> {
        if self.graph_generation() == Some(generation) {
            return self.invoke(service, input, caller_authority, binding);
        }

        let state = self
            .resident_generations
            .get(generation)
            .ok_or_else(|| KernelError::UnknownGeneration(generation.clone()))?;
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
        let scope = CallScope::external(Arc::new(state.runtime.clone()), caller_authority);
        dispatch::invoke_service_with(runtime, service, input, binding, scope)
    }

    /// Make a resident generation the default for future unqualified roots and
    /// ambient event delivery. The previous default remains resident.
    pub(crate) fn promote_generation(
        &mut self,
        generation: &GraphGenerationId,
    ) -> Result<(), KernelError> {
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
    pub(crate) fn retire_generation(&mut self, generation: &GraphGenerationId) -> Result<(), KernelError> {
        if self.graph_generation() == Some(generation) {
            return Err(KernelError::DefaultGenerationCannotRetire(
                generation.clone(),
            ));
        }

        let state = self
            .resident_generations
            .remove(generation)
            .ok_or_else(|| KernelError::UnknownGeneration(generation.clone()))?;
        let stop_view = reconciliation::StopView {
            runtime: &state.runtime,
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
        self.events.remove_generation_subscriptions(generation);
        Ok(())
    }

    fn stage_resident_generation(
        &self,
        candidate: &ResolvedHarness,
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
                            scope: CallScope::root(
                                Arc::new(runtime.clone()),
                                &binding.provider,
                                &provider_manifest.maximum_authority,
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
                                    guest_authority: &manifest.maximum_authority,
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
                    scope: CallScope::root(
                        Arc::new(runtime.clone()),
                        plugin,
                        &manifest.maximum_authority,
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
        .map_err(|error| {
            reconciliation::cleanup_staged(
                &staged,
                reconciliation::StopView {
                    runtime: &runtime,
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
            error
        })?;

        Ok(GenerationRuntimeState {
            runtime,
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
        DurableSchema, DurableSchemaRegistration, PluginManifest, ResolvedHarnessActivation,
        ResourceNamespace, ServiceContribution,
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

        kernel.make_generation_resident(&second).unwrap();
        assert_eq!(
            kernel
                .invoke(&service(), &[], &Authority::default(), None)
                .unwrap(),
            b"first"
        );
        assert_eq!(
            kernel
                .invoke_in_generation(
                    &second_generation,
                    &service(),
                    &[],
                    &Authority::default(),
                    None,
                )
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
                .invoke_in_generation(
                    &first_generation,
                    &service(),
                    &[],
                    &Authority::default(),
                    None,
                )
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
            kernel.invoke_in_generation(
                &second_generation,
                &service(),
                &[],
                &Authority::default(),
                None,
            ),
            Err(KernelError::UnknownGeneration(second_generation))
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
}
