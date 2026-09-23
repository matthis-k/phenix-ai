use super::*;
use std::panic::{catch_unwind, AssertUnwindSafe};

fn emit_policy_stage(
    runtime: InvocationContext<'_>,
    policy: &str,
    stage: &str,
    outcome: &str,
    subject: Option<String>,
    revision: Option<String>,
    reason: Option<String>,
) {
    trace::record_runtime_trace(
        runtime.trace_sink,
        RuntimeTraceEvent::PolicyStage {
            policy: policy.to_owned(),
            stage: stage.to_owned(),
            outcome: outcome.to_owned(),
            subject,
            revision,
            reason,
        },
    );
}

fn prepare_active_chain(
    runtime: InvocationContext<'_>,
    mut chain: ResolvedServiceChain,
) -> Result<ResolvedServiceChain, KernelError> {
    let configured_layers = std::mem::take(&mut chain.layers);
    for layer in configured_layers {
        let subject = Some(format!("{}:{}", chain.service, layer.plugin));
        if runtime.states.get(&layer.plugin).copied() == Some(PluginState::Active) {
            emit_policy_stage(
                runtime,
                "kernel.service_chain",
                "layer_availability",
                "allowed",
                subject,
                None,
                None,
            );
            chain.layers.push(layer);
            continue;
        }
        let required = runtime
            .config
            .layer_policy(&chain.service)
            .iter()
            .find(|policy| policy.plugin == layer.plugin)
            .is_some_and(|policy| policy.required);
        if required {
            emit_policy_stage(
                runtime,
                "kernel.service_chain",
                "layer_availability",
                "denied",
                subject,
                None,
                Some("required layer is inactive".into()),
            );
            return Err(KernelError::RequiredLayerUnavailable {
                service: chain.service.clone(),
                plugin: layer.plugin,
            });
        }
        emit_policy_stage(
            runtime,
            "kernel.service_chain",
            "layer_availability",
            "skipped",
            subject,
            None,
            Some("optional layer is inactive".into()),
        );
    }
    let terminal_subject = Some(format!("{}:{}", chain.service, chain.terminal.plugin));
    if runtime.states.get(&chain.terminal.plugin).copied() != Some(PluginState::Active) {
        emit_policy_stage(
            runtime,
            "kernel.service_chain",
            "terminal_availability",
            "denied",
            terminal_subject,
            None,
            Some("terminal provider is inactive".into()),
        );
        return Err(KernelError::PluginNotActive(chain.terminal.plugin.clone()));
    }
    emit_policy_stage(
        runtime,
        "kernel.service_chain",
        "terminal_availability",
        "allowed",
        terminal_subject,
        None,
        None,
    );
    Ok(chain)
}

pub(super) struct ComponentDispatchTarget<'a> {
    pub(super) component: &'a ComponentId,
    pub(super) binding: &'a PluginId,
    pub(super) provider_provenance: Option<ComponentProviderProvenance>,
}

pub(super) fn invoke_component_service_with(
    runtime: InvocationContext<'_>,
    service: &ServiceId,
    target: ComponentDispatchTarget<'_>,
    input: &[u8],
    caller_authority: &Authority,
    guards: ServiceDispatchGuards<'_>,
) -> Result<Vec<u8>, KernelError> {
    let ComponentDispatchTarget {
        component,
        binding,
        provider_provenance,
    } = target;
    let endpoint = ComponentServiceEndpoint {
        component: component.clone(),
        service: service.clone(),
    };
    if guards.active_component_endpoints.contains(&endpoint) {
        return Err(KernelError::CausalServiceReentry(service.clone()));
    }
    let resolved = runtime
        .component_graph
        .component(component)
        .ok_or_else(|| ComponentGraphError::UnknownComponent(component.clone()))?;
    if &resolved.owning_plugin != binding {
        return Err(KernelError::HostOperationDenied {
            plugin: binding.clone(),
            operation: format!(
                "resolved component {component} belongs to {}, not {binding}",
                resolved.owning_plugin
            ),
        });
    }
    let chain = match runtime
        .config
        .resolve_component_chain(service, caller_authority, binding)
    {
        Ok(chain) => {
            emit_policy_stage(
                runtime,
                "kernel.service_chain",
                "component_resolution",
                "allowed",
                Some(service.as_str().to_owned()),
                None,
                None,
            );
            chain
        }
        Err(error) => {
            emit_policy_stage(
                runtime,
                "kernel.service_chain",
                "component_resolution",
                "denied",
                Some(service.as_str().to_owned()),
                None,
                Some(error.to_string()),
            );
            return Err(error);
        }
    };
    let chain = prepare_active_chain(runtime, chain)?;
    let mut next_services = guards.active_services.clone();
    next_services.insert(service.clone());
    let mut next_component_endpoints = guards.active_component_endpoints.clone();
    next_component_endpoints.insert(endpoint);
    let trace = Arc::new(Mutex::new(InvocationTrace::new(
        &chain,
        caller_authority,
        runtime.graph_generation,
        provider_provenance,
    )));
    let result = invoke_resolved_chain_with(
        runtime,
        &chain,
        0,
        input,
        caller_authority,
        ServiceDispatchGuards {
            call_stack: guards.call_stack,
            active_services: &next_services,
            active_component_endpoints: &next_component_endpoints,
            terminal_component: Some(component),
        },
        &trace,
    );
    let completed = trace
        .lock()
        .expect("service invocation trace mutex poisoned")
        .clone()
        .finish();
    runtime.provenance.record(completed.clone());
    emit_runtime_trace(runtime, &completed, input.len(), &result);
    result
}

pub(super) fn invoke_service_with(
    runtime: InvocationContext<'_>,
    service: &ServiceId,
    input: &[u8],
    caller_authority: &Authority,
    binding: Option<&PluginId>,
    guards: ServiceDispatchGuards<'_>,
) -> Result<Vec<u8>, KernelError> {
    if guards.active_services.contains(service) {
        return Err(KernelError::CausalServiceReentry(service.clone()));
    }
    let chain = match runtime
        .config
        .resolve_chain(service, caller_authority, binding)
    {
        Ok(chain) => {
            emit_policy_stage(
                runtime,
                "kernel.service_chain",
                "service_resolution",
                "allowed",
                Some(service.as_str().to_owned()),
                None,
                None,
            );
            chain
        }
        Err(error) => {
            emit_policy_stage(
                runtime,
                "kernel.service_chain",
                "service_resolution",
                "denied",
                Some(service.as_str().to_owned()),
                None,
                Some(error.to_string()),
            );
            return Err(error);
        }
    };
    let chain = prepare_active_chain(runtime, chain)?;
    let mut next_services = guards.active_services.clone();
    next_services.insert(service.clone());
    let trace = Arc::new(Mutex::new(InvocationTrace::new(
        &chain,
        caller_authority,
        runtime.graph_generation,
        None,
    )));
    let result = invoke_resolved_chain_with(
        runtime,
        &chain,
        0,
        input,
        caller_authority,
        ServiceDispatchGuards {
            call_stack: guards.call_stack,
            active_services: &next_services,
            active_component_endpoints: guards.active_component_endpoints,
            terminal_component: None,
        },
        &trace,
    );
    let completed = trace
        .lock()
        .expect("service invocation trace mutex poisoned")
        .clone()
        .finish();
    runtime.provenance.record(completed.clone());
    emit_runtime_trace(runtime, &completed, input.len(), &result);
    result
}

#[derive(Clone, Copy)]
pub(super) struct ServiceDispatchGuards<'a> {
    pub(super) call_stack: &'a BTreeSet<PluginId>,
    pub(super) active_services: &'a BTreeSet<ServiceId>,
    pub(super) active_component_endpoints: &'a BTreeSet<ComponentServiceEndpoint>,
    pub(super) terminal_component: Option<&'a ComponentId>,
}

pub(super) fn invoke_resolved_chain_with(
    runtime: InvocationContext<'_>,
    chain: &ResolvedServiceChain,
    position: usize,
    input: &[u8],
    caller_authority: &Authority,
    guards: ServiceDispatchGuards<'_>,
    trace: &Arc<Mutex<InvocationTrace>>,
) -> Result<Vec<u8>, KernelError> {
    let (provider, is_layer) = if position < chain.layers.len() {
        (&chain.layers[position], true)
    } else {
        (&chain.terminal, false)
    };
    if runtime.states.get(&provider.plugin).copied() != Some(PluginState::Active) {
        return Err(KernelError::PluginNotActive(provider.plugin.clone()));
    }
    let instance = runtime
        .instances
        .get(&provider.plugin)
        .ok_or_else(|| KernelError::WrongExecutionKind(provider.plugin.clone()))?;
    let shared_invocation = if guards.call_stack.contains(&provider.plugin) {
        instance
            .try_lock()
            .ok()
            .and_then(|instance| instance.shared_invocation())
    } else {
        instance
            .lock()
            .expect("plugin instance mutex poisoned")
            .shared_invocation()
    };
    if guards.call_stack.contains(&provider.plugin) && shared_invocation.is_none() {
        return Err(KernelError::HostOperationDenied {
            plugin: provider.plugin.clone(),
            operation: format!("causal plugin re-entry:{}", chain.service),
        });
    }
    let provider_manifest = runtime
        .config
        .manifest(&provider.plugin)
        .expect("resolved providers are registered");
    let effective_authority = caller_authority.attenuate(&provider_manifest.maximum_authority);
    emit_policy_stage(
        runtime,
        "kernel.authority",
        "provider_attenuation",
        "allowed",
        Some(format!("{}:{}", chain.service, provider.plugin)),
        None,
        None,
    );
    let trace_index = trace
        .lock()
        .expect("service invocation trace mutex poisoned")
        .enter(
            provider.plugin.clone(),
            if is_layer {
                ServiceRole::Layer
            } else {
                ServiceRole::Terminal
            },
            effective_authority.clone(),
        );
    let mut next_stack = guards.call_stack.clone();
    next_stack.insert(provider.plugin.clone());
    let continuation = is_layer.then(|| ContinuationState {
        chain: chain.clone(),
        terminal_component: guards.terminal_component.cloned(),
        next_position: position + 1,
        used: Arc::new(AtomicBool::new(false)),
        trace: Arc::clone(trace),
    });
    let continuation_used = continuation.as_ref().map(|state| Arc::clone(&state.used));
    let live_call = runtime
        .tasks
        .begin_call(&provider.plugin, runtime.graph_generation);
    let call_cancellation = live_call.cancellation_token().clone();
    let host = PluginHost {
        graph_generation: runtime.graph_generation,
        component_graph: runtime.component_graph,
        config: runtime.config,
        states: runtime.states,
        instances: runtime.instances,
        plugin: &provider.plugin,
        authority: &effective_authority,
        call_cancellation: Some(call_cancellation.clone()),
        call_stack: next_stack,
        events: runtime.events,
        tasks: runtime.tasks,
        persistence: runtime.persistence,
        prepared_mutations: runtime.prepared_mutations,
        trace_sink: runtime.trace_sink,
        provenance: runtime.provenance,
        continuation,
        active_services: guards.active_services.clone(),
        active_component_endpoints: guards.active_component_endpoints.clone(),
    };
    if is_layer {
        let result = match shared_invocation.as_ref() {
            Some(invocation) => catch_unwind(AssertUnwindSafe(|| {
                invocation.invoke_layer(&chain.service, input, &host)
            })),
            None => {
                let mut instance = instance.lock().expect("plugin instance mutex poisoned");
                catch_unwind(AssertUnwindSafe(|| {
                    instance.invoke_layer(&chain.service, input, &host)
                }))
            }
        };
        let result = match result {
            Ok(result) => result,
            Err(_) => {
                trace
                    .lock()
                    .expect("service invocation trace mutex poisoned")
                    .set_outcome(trace_index, ServiceParticipantOutcome::Failed);
                return Err(KernelError::ServiceInvoke {
                    plugin: provider.plugin.clone(),
                    service: chain.service.clone(),
                    message: "plugin invocation panicked".into(),
                });
            }
        };
        if call_cancellation.is_cancelled() {
            trace
                .lock()
                .expect("service invocation trace mutex poisoned")
                .set_outcome(trace_index, ServiceParticipantOutcome::Failed);
            return Err(KernelError::ServiceCancelled {
                plugin: provider.plugin.clone(),
                service: chain.service.clone(),
            });
        }
        match result {
            Ok(LayerResult::Handled(output)) => {
                let delegated = continuation_used
                    .as_ref()
                    .is_some_and(|used| used.load(Ordering::Acquire));
                trace
                    .lock()
                    .expect("service invocation trace mutex poisoned")
                    .set_outcome(
                        trace_index,
                        if delegated {
                            ServiceParticipantOutcome::Delegated
                        } else {
                            ServiceParticipantOutcome::Handled
                        },
                    );
                Ok(output)
            }
            Ok(LayerResult::Denied(message)) => {
                trace
                    .lock()
                    .expect("service invocation trace mutex poisoned")
                    .set_outcome(trace_index, ServiceParticipantOutcome::Denied);
                Err(KernelError::ServiceDenied {
                    plugin: provider.plugin.clone(),
                    service: chain.service.clone(),
                    message,
                })
            }
            Err(message) => {
                trace
                    .lock()
                    .expect("service invocation trace mutex poisoned")
                    .set_outcome(trace_index, ServiceParticipantOutcome::Failed);
                Err(KernelError::ServiceInvoke {
                    plugin: provider.plugin.clone(),
                    service: chain.service.clone(),
                    message,
                })
            }
        }
    } else {
        let result = match shared_invocation.as_ref() {
            Some(invocation) => {
                catch_unwind(AssertUnwindSafe(|| match guards.terminal_component {
                    Some(component) => {
                        invocation.invoke_component(component, &chain.service, input, &host)
                    }
                    None => invocation.invoke(&chain.service, input, &host),
                }))
            }
            None => {
                let mut instance = instance.lock().expect("plugin instance mutex poisoned");
                catch_unwind(AssertUnwindSafe(|| match guards.terminal_component {
                    Some(component) => {
                        instance.invoke_component(component, &chain.service, input, &host)
                    }
                    None => instance.invoke(&chain.service, input, &host),
                }))
            }
        };
        let result = match result {
            Ok(result) => result,
            Err(_) => {
                trace
                    .lock()
                    .expect("service invocation trace mutex poisoned")
                    .set_outcome(trace_index, ServiceParticipantOutcome::Failed);
                return Err(KernelError::ServiceInvoke {
                    plugin: provider.plugin.clone(),
                    service: chain.service.clone(),
                    message: "plugin invocation panicked".into(),
                });
            }
        };
        if call_cancellation.is_cancelled() {
            trace
                .lock()
                .expect("service invocation trace mutex poisoned")
                .set_outcome(trace_index, ServiceParticipantOutcome::Failed);
            return Err(KernelError::ServiceCancelled {
                plugin: provider.plugin.clone(),
                service: chain.service.clone(),
            });
        }
        match result {
            Ok(output) => {
                trace
                    .lock()
                    .expect("service invocation trace mutex poisoned")
                    .set_outcome(trace_index, ServiceParticipantOutcome::Succeeded);
                Ok(output)
            }
            Err(message) => {
                trace
                    .lock()
                    .expect("service invocation trace mutex poisoned")
                    .set_outcome(trace_index, ServiceParticipantOutcome::Failed);
                Err(KernelError::ServiceInvoke {
                    plugin: provider.plugin.clone(),
                    service: chain.service.clone(),
                    message,
                })
            }
        }
    }
}

fn emit_runtime_trace(
    runtime: InvocationContext<'_>,
    provenance: &ServiceInvocationProvenance,
    input_bytes: usize,
    result: &Result<Vec<u8>, KernelError>,
) {
    trace::record_runtime_trace(
        runtime.trace_sink,
        RuntimeTraceEvent::ServiceInvocation {
            service: provenance.service.as_str().to_owned(),
            input_bytes,
            output_bytes: result.as_ref().ok().map(Vec::len),
            success: result.is_ok(),
            error: result.as_ref().err().map(ToString::to_string),
            terminal_reached: provenance.terminal_reached,
            participants: provenance
                .participants
                .iter()
                .map(|participant| RuntimeTraceParticipant {
                    plugin: participant.plugin.as_str().to_owned(),
                    role: match participant.role {
                        ServiceRole::Terminal => "terminal",
                        ServiceRole::Layer => "layer",
                    }
                    .to_owned(),
                    outcome: match participant.outcome {
                        ServiceParticipantOutcome::Handled => "handled",
                        ServiceParticipantOutcome::Delegated => "delegated",
                        ServiceParticipantOutcome::Denied => "denied",
                        ServiceParticipantOutcome::Failed => "failed",
                        ServiceParticipantOutcome::Succeeded => "succeeded",
                    }
                    .to_owned(),
                })
                .collect(),
        },
    );
}
