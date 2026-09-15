use super::*;
use crate::ResolvedHarness;
use std::panic::{catch_unwind, AssertUnwindSafe};

#[derive(Clone, Copy)]
pub(super) struct StopView<'a> {
    pub(super) generation: Option<&'a GraphGenerationId>,
    pub(super) graph: &'a ResolvedComponentGraph,
    pub(super) config: &'a KernelConfig,
    pub(super) states: &'a BTreeMap<PluginId, PluginState>,
    pub(super) instances: &'a BTreeMap<PluginId, Arc<Mutex<Box<dyn PluginInstance>>>>,
    pub(super) events: &'a EventBus,
    pub(super) tasks: &'a TaskRuntime,
    pub(super) persistence: &'a Mutex<Box<dyn PersistenceBackend>>,
    pub(super) provenance: &'a Mutex<Vec<ServiceInvocationProvenance>>,
}

impl StopView<'_> {
    fn stop(&self, plugin: &PluginId, instance: &Arc<Mutex<Box<dyn PluginInstance>>>) {
        let Some(manifest) = self.config.manifest(plugin) else {
            return;
        };
        self.tasks.cancel_calls(plugin, self.generation);
        self.tasks.cancel_plugin_generation(plugin, self.generation);
        let live_call = self.tasks.begin_call(plugin, self.generation);
        let prepared_mutations = PreparedMutationScope::new(self.generation);
        let host = PluginHost {
            graph_generation: self.generation,
            component_graph: self.graph,
            config: self.config,
            states: self.states,
            instances: self.instances,
            plugin,
            authority: &manifest.maximum_authority,
            call_cancellation: Some(live_call.cancellation_token().clone()),
            call_stack: BTreeSet::from([plugin.clone()]),
            events: self.events,
            tasks: self.tasks,
            persistence: self.persistence,
            prepared_mutations: &prepared_mutations,
            provenance: self.provenance,
            continuation: None,
            active_services: BTreeSet::new(),
            active_component_endpoints: BTreeSet::new(),
        };
        let mut instance = instance
            .lock()
            .expect("plugin instance mutex poisoned during stop");
        let _ = catch_unwind(AssertUnwindSafe(|| instance.stop(&host)));
    }
}

impl Kernel {
    /// Replace the active runtime topology with one fully resolved candidate generation.
    /// Candidate implementations are staged before the canonical config/graph swap so a
    /// start failure leaves the previously active generation untouched.
    pub(crate) fn reconcile_resolved_generation(
        &mut self,
        candidate: &ResolvedHarness,
        restart_plugins: &BTreeSet<PluginId>,
    ) -> Result<(), KernelError> {
        let has_active = self
            .states
            .values()
            .any(|state| *state == PluginState::Active);
        let all_active = self
            .states
            .values()
            .all(|state| *state == PluginState::Active);
        if (self.runtime_active && !all_active) || (!self.runtime_active && has_active) {
            return Err(KernelError::PartiallyActiveRuntime);
        }

        let active_runtime = self.runtime_active;
        let candidate_config = candidate.kernel_config().clone();
        let old_manifests: BTreeMap<_, _> = self
            .config
            .manifests()
            .map(|manifest| (manifest.id.clone(), manifest.clone()))
            .collect();
        let candidate_manifests: BTreeMap<_, _> = candidate_config
            .manifests()
            .map(|manifest| (manifest.id.clone(), manifest.clone()))
            .collect();
        let restart_plugins = runtime_restart_closure(
            &self.config,
            &candidate_config,
            &old_manifests,
            &candidate_manifests,
            restart_plugins,
        );
        let mut next_states = BTreeMap::new();
        let mut next_instances = BTreeMap::new();
        let mut staged = Vec::new();

        for (plugin, manifest) in &candidate_manifests {
            let retain = active_runtime
                && !restart_plugins.contains(plugin)
                && old_manifests.get(plugin) == Some(manifest);
            if retain {
                next_states.insert(plugin.clone(), PluginState::Active);
                if let Some(instance) = self.instances.get(plugin) {
                    next_instances.insert(plugin.clone(), Arc::clone(instance));
                }
            } else {
                next_states.insert(plugin.clone(), PluginState::Registered);
            }
        }

        if active_runtime {
            for plugin in candidate_config.activation_order() {
                if next_states.get(plugin) == Some(&PluginState::Active) {
                    continue;
                }
                let manifest = &candidate_manifests[plugin];
                let instance = (|| -> Result<Option<Box<dyn PluginInstance>>, KernelError> {
                    match &manifest.execution {
                        PluginExecution::ResourceOnly => Ok(None),
                        PluginExecution::Embedded => self.take_embedded_instance(plugin).map(Some),
                        PluginExecution::Runtime { runtime, artifact } => {
                            let binding = candidate_config
                                .runtime_binding(plugin)
                                .cloned()
                                .ok_or_else(|| {
                                    KernelError::RuntimeProviderUnavailable(runtime.clone())
                                })?;
                            let provider_manifest = candidate_config
                                .manifest(&binding.provider)
                                .expect("resolved runtime provider is configured");
                            let provider =
                                next_instances.get(&binding.provider).cloned().ok_or_else(
                                    || KernelError::PluginNotActive(binding.provider.clone()),
                                )?;
                            let live_call = self
                                .tasks
                                .begin_call(&binding.provider, Some(candidate.generation()));
                            let cancellation = live_call.cancellation_token().clone();
                            let prepared_mutations =
                                PreparedMutationScope::new(Some(candidate.generation()));
                            let host = PluginHost {
                                graph_generation: Some(candidate.generation()),
                                component_graph: candidate.component_graph(),
                                config: &candidate_config,
                                states: &next_states,
                                instances: &next_instances,
                                plugin: &binding.provider,
                                authority: &provider_manifest.maximum_authority,
                                call_cancellation: Some(cancellation.clone()),
                                call_stack: BTreeSet::from([binding.provider.clone()]),
                                events: &self.events,
                                tasks: &self.tasks,
                                persistence: &self.persistence,
                                prepared_mutations: &prepared_mutations,
                                provenance: &self.provenance,
                                continuation: None,
                                active_services: BTreeSet::new(),
                                active_component_endpoints: BTreeSet::new(),
                            };
                            let mut provider =
                                provider.lock().expect("plugin instance mutex poisoned");
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
                            .map_err(|_| {
                                KernelError::RuntimePrepare {
                                    plugin: plugin.clone(),
                                    runtime: runtime.clone(),
                                    message: "runtime provider panicked".into(),
                                }
                            })?;
                            if cancellation.is_cancelled() {
                                return Err(KernelError::RuntimePrepare {
                                    plugin: plugin.clone(),
                                    runtime: runtime.clone(),
                                    message: "runtime provider preparation cancelled".into(),
                                });
                            }
                            prepared
                                .map_err(|message| KernelError::RuntimePrepare {
                                    plugin: plugin.clone(),
                                    runtime: runtime.clone(),
                                    message,
                                })
                                .map(Some)
                        }
                    }
                })();
                let instance = match instance {
                    Ok(instance) => instance,
                    Err(error) => {
                        cleanup_staged(
                            &staged,
                            StopView {
                                generation: Some(candidate.generation()),
                                graph: candidate.component_graph(),
                                config: &candidate_config,
                                states: &next_states,
                                instances: &next_instances,
                                events: &self.events,
                                tasks: &self.tasks,
                                persistence: &self.persistence,
                                provenance: &self.provenance,
                            },
                        );
                        return Err(error);
                    }
                };
                if let Some(mut instance) = instance {
                    let live_call = self.tasks.begin_call(plugin, Some(candidate.generation()));
                    let cancellation = live_call.cancellation_token().clone();
                    let prepared_mutations =
                        PreparedMutationScope::new(Some(candidate.generation()));
                    let host = PluginHost {
                        graph_generation: Some(candidate.generation()),
                        component_graph: candidate.component_graph(),
                        config: &candidate_config,
                        states: &next_states,
                        instances: &next_instances,
                        plugin,
                        authority: &manifest.maximum_authority,
                        call_cancellation: Some(cancellation.clone()),
                        call_stack: BTreeSet::from([plugin.clone()]),
                        events: &self.events,
                        tasks: &self.tasks,
                        persistence: &self.persistence,
                        prepared_mutations: &prepared_mutations,
                        provenance: &self.provenance,
                        continuation: None,
                        active_services: BTreeSet::new(),
                        active_component_endpoints: BTreeSet::new(),
                    };
                    let started = catch_unwind(AssertUnwindSafe(|| instance.start(&host)));
                    let failure = match started {
                        Ok(Ok(())) if cancellation.is_cancelled() => {
                            Some("plugin start cancelled".into())
                        }
                        Ok(Ok(())) => None,
                        Ok(Err(message)) => Some(message),
                        Err(_) => Some("plugin start panicked".into()),
                    };
                    if let Some(message) = failure {
                        self.tasks
                            .cancel_plugin_generation(plugin, Some(candidate.generation()));
                        cleanup_staged(
                            &staged,
                            StopView {
                                generation: Some(candidate.generation()),
                                graph: candidate.component_graph(),
                                config: &candidate_config,
                                states: &next_states,
                                instances: &next_instances,
                                events: &self.events,
                                tasks: &self.tasks,
                                persistence: &self.persistence,
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
        }

        let subscriptions = match stage_listener_subscriptions(listener::ListenerRuntimeSources {
            graph: candidate.component_graph(),
            generation: candidate.generation(),
            config: &candidate_config,
            states: &next_states,
            instances: &next_instances,
            events: &self.events,
            tasks: &self.tasks,
            persistence: &self.persistence,
            provenance: &self.provenance,
        }) {
            Ok(subscriptions) => subscriptions,
            Err(error) => {
                cleanup_staged(
                    &staged,
                    StopView {
                        generation: Some(candidate.generation()),
                        graph: candidate.component_graph(),
                        config: &candidate_config,
                        states: &next_states,
                        instances: &next_instances,
                        events: &self.events,
                        tasks: &self.tasks,
                        persistence: &self.persistence,
                        provenance: &self.provenance,
                    },
                );
                return Err(error);
            }
        };

        let retired: Vec<_> = old_manifests
            .iter()
            .filter(|(plugin, old_manifest)| {
                candidate_manifests.get(*plugin) != Some(*old_manifest)
                    || restart_plugins.contains(*plugin)
            })
            .filter_map(|(plugin, _)| {
                self.instances
                    .get(plugin)
                    .map(|instance| (plugin.clone(), Arc::clone(instance)))
            })
            .collect();

        let old_generation = self.graph_generation.clone();
        let old_graph = self.component_graph.clone();
        let old_config = self.config.clone();
        let old_states = self.states.clone();
        let old_instances = self.instances.clone();
        self.events.replace_subscriptions(subscriptions)?;
        self.config = candidate_config;
        self.states = next_states;
        self.instances = next_instances;
        self.install_resolved_graph(
            candidate.generation().clone(),
            candidate.component_graph().clone(),
            candidate.resources().to_vec(),
        );
        self.runtime_active = active_runtime;

        let retired_view = StopView {
            generation: old_generation.as_ref(),
            graph: &old_graph,
            config: &old_config,
            states: &old_states,
            instances: &old_instances,
            events: &self.events,
            tasks: &self.tasks,
            persistence: &self.persistence,
            provenance: &self.provenance,
        };
        for (plugin, instance) in retired {
            retired_view.stop(&plugin, &instance);
            self.events.publish(KernelEvent::PluginStopped(plugin));
        }
        for plugin in staged {
            self.events.publish(KernelEvent::PluginActivated(plugin));
        }
        Ok(())
    }
}

fn runtime_restart_closure(
    active: &KernelConfig,
    candidate: &KernelConfig,
    active_manifests: &BTreeMap<PluginId, PluginManifest>,
    candidate_manifests: &BTreeMap<PluginId, PluginManifest>,
    requested: &BTreeSet<PluginId>,
) -> BTreeSet<PluginId> {
    let mut restart = requested.clone();
    for (plugin, manifest) in candidate_manifests {
        if active_manifests.get(plugin) != Some(manifest) {
            restart.insert(plugin.clone());
        }
    }

    loop {
        let before = restart.len();
        for binding in candidate.runtime_bindings() {
            if active.runtime_binding(&binding.guest) != Some(binding) {
                restart.insert(binding.guest.clone());
            }
            if restart.contains(&binding.provider) {
                restart.insert(binding.guest.clone());
            }
        }
        if restart.len() == before {
            return restart;
        }
    }
}

pub(super) fn cleanup_staged(staged: &[PluginId], view: StopView<'_>) {
    for plugin in staged.iter().rev() {
        if let Some(instance) = view.instances.get(plugin) {
            view.stop(plugin, instance);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{runtime_provider_service, RuntimeId, ServiceContribution};

    fn plugin(value: &str) -> PluginId {
        PluginId::parse(value).unwrap()
    }

    fn runtime(value: &str) -> RuntimeId {
        RuntimeId::parse(value).unwrap()
    }

    fn manifest(
        id: &str,
        execution: PluginExecution,
        services: Vec<ServiceContribution>,
    ) -> PluginManifest {
        PluginManifest {
            id: plugin(id),
            version: 1,
            execution,
            dependencies: Vec::new(),
            services,
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    #[test]
    fn changing_guest_does_not_restart_unchanged_runtime_provider() {
        let runtime = runtime("vendor.runtime");
        let provider = manifest(
            "fixture.bridge",
            PluginExecution::Embedded,
            vec![ServiceContribution {
                service: runtime_provider_service(&runtime),
                role: ServiceRole::Terminal,
                priority: 0,
                required_authority: Authority::default(),
            }],
        );
        let guest = |revision: &str| {
            manifest(
                "fixture.guest",
                PluginExecution::Runtime {
                    runtime: runtime.clone(),
                    artifact: PluginArtifact {
                        locator: "fixture.plugin".into(),
                        revision: crate::ArtifactRevision::from_content(revision.as_bytes()),
                        configuration: BTreeMap::new(),
                    },
                },
                Vec::new(),
            )
        };
        let active = KernelConfig::new([provider.clone(), guest("sha256:v1")]).unwrap();
        let candidate = KernelConfig::new([provider.clone(), guest("sha256:v2")]).unwrap();
        let active_manifests = active
            .manifests()
            .map(|manifest| (manifest.id.clone(), manifest.clone()))
            .collect();
        let candidate_manifests = candidate
            .manifests()
            .map(|manifest| (manifest.id.clone(), manifest.clone()))
            .collect();

        let restart = runtime_restart_closure(
            &active,
            &candidate,
            &active_manifests,
            &candidate_manifests,
            &BTreeSet::new(),
        );

        assert!(restart.contains(&plugin("fixture.guest")));
        assert!(!restart.contains(&provider.id));
    }
}
