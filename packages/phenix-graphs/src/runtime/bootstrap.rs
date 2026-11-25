//! Native session construction phases: validate before callbacks, spawn, wire, activate.
use super::*;

fn request_lane_packet_bound(
    compiled: &CompiledNetwork,
    options: &SessionOptions,
) -> Result<usize, Failure> {
    let connections = compiled
        .connections()
        .iter()
        .try_fold(0usize, |total, edge| {
            total.checked_add(edge.capacity().get())
        });
    let ingress = compiled
        .inputs()
        .iter()
        .try_fold(0usize, |total, (name, targets)| {
            compiled.ingress_capacities()[name]
                .get()
                .checked_mul(targets.len())
                .and_then(|capacity| total.checked_add(capacity))
        });
    let captures = options
        .capture_capacity
        .get()
        .checked_mul(compiled.outputs().len());
    connections
        .and_then(|connections| ingress.and_then(|ingress| connections.checked_add(ingress)))
        .and_then(|capacity| captures.and_then(|captures| capacity.checked_add(captures)))
        .and_then(|capacity| capacity.checked_mul(options.max_requests.get()))
        .ok_or_else(|| RuntimeError::LaneCapacityOverflow.failure())
}

fn preflight_options(compiled: &CompiledNetwork, options: &mut SessionOptions) -> Option<Failure> {
    if let Some(lease) = compiled.execution_lease() {
        options.lease = Some(crate::network::HandleLeases::append(
            options.lease.take(),
            vec![lease],
        ));
    }
    if let Some(lease) = options.bound.execution_lease() {
        options.lease = Some(crate::network::HandleLeases::append(
            options.lease.take(),
            vec![lease],
        ));
    }
    let bound_failure = options
        .bound
        .apply(
            compiled,
            &mut options.readiness,
            &mut options.resources,
            &mut options.discard_outputs,
        )
        .err();
    let invalid_path = options
        .readiness
        .keys()
        .chain(options.resources.keys())
        .find(|path| {
            !compiled
                .processes()
                .iter()
                .any(|p| p.path() == path.as_str())
        })
        .cloned();
    // Reject static configuration before invoking any plugin-owned factory.
    // Readiness policy callbacks are never evaluated during this preflight.
    let mut initial_failure = bound_failure
        .or_else(|| {
            invalid_path
                .map(|path| RuntimeError::UnknownConfiguration { path }.failure())
                .or_else(|| {
                    options
                        .discard_outputs
                        .iter()
                        .find(|(path, output)| {
                            !compiled.processes().iter().any(|p| {
                                p.path() == path && p.interface().outputs().contains_key(output)
                            })
                        })
                        .map(|(path, output)| {
                            RuntimeError::InvalidDiscard {
                                path: path.clone(),
                                output: output.clone(),
                            }
                            .failure()
                        })
                })
        })
        .or_else(|| {
            compiled.processes().iter().find_map(|process| {
                options.readiness.get(process.path()).and_then(|policy| {
                    readiness::validate_policy_ports(
                        process.interface().inputs().keys().map(String::as_str),
                        policy,
                    )
                    .err()
                    .map(|error| {
                        Failure::external(format!("process {}: {error}", process.path()), error)
                            .with_kind(FailureKind::Framework)
                            .with_context(crate::FailureContext {
                                path: Some(process.path().into()),
                                ..Default::default()
                            })
                    })
                })
            })
        })
        .or_else(|| {
            (options.request_queue_policy == RequestQueuePolicy::BoundedLanes)
                .then(|| request_lane_packet_bound(compiled, options).err())
                .flatten()
        });
    for process in compiled.processes() {
        if initial_failure.is_some() {
            break;
        }
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            process.factory().correlation_lanes()
        })) {
            Ok(Some(lanes)) if options.max_requests > lanes => {
                initial_failure = Some(
                    RuntimeError::CorrelationBound {
                        path: process.path().into(),
                        lanes: lanes.get(),
                        requests: options.max_requests.get(),
                    }
                    .failure(),
                );
            }
            Err(payload) => {
                initial_failure = Some(
                    crate::process::callback_panicked(CallbackPhase::CorrelationBound, payload)
                        .with_context(crate::FailureContext {
                            path: Some(process.path().into()),
                            phase: Some(CallbackPhase::CorrelationBound),
                            ..Default::default()
                        }),
                );
            }
            _ => {}
        }
    }

    initial_failure
}

fn spawn_processes(
    compiled: &CompiledNetwork,
    options: &SessionOptions,
    mut initial_failure: Option<Failure>,
) -> (Vec<RuntimeProcess>, Option<Failure>) {
    let processes: Vec<_> = compiled
        .processes()
        .iter()
        .enumerate()
        .map(|(index, p)| {
            let handoff = {
                let process = if initial_failure.is_some() {
                    Box::new(UnavailableProcess) as Box<dyn crate::process::Process>
                } else {
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        p.factory().spawn()
                    })) {
                        Ok(process) => process,
                        Err(payload) => {
                            initial_failure = Some(
                                crate::process::callback_panicked(
                                    CallbackPhase::Construct,
                                    payload,
                                )
                                .with_context(
                                    crate::FailureContext {
                                        process: Some(index),
                                        path: Some(p.path().into()),
                                        phase: Some(CallbackPhase::Construct),
                                        ..Default::default()
                                    },
                                ),
                            );
                            Box::new(UnavailableProcess)
                        }
                    }
                };
                let handoff = ProcessHandoff::new(process);
                match &options.lease {
                    Some(lease) => handoff.with_lease(lease.clone()),
                    None => handoff,
                }
            };
            let policy = match options.readiness.get(p.path()) {
                Some(policy) => policy.clone(),
                None if initial_failure.is_some() => Readiness::default(),
                None => match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    p.factory().readiness()
                })) {
                    Ok(policy) => policy,
                    Err(payload) => {
                        initial_failure.get_or_insert_with(|| {
                            crate::process::callback_panicked(
                                CallbackPhase::FactoryReadiness,
                                payload,
                            )
                            .with_context(crate::FailureContext {
                                process: Some(index),
                                path: Some(p.path().into()),
                                phase: Some(CallbackPhase::FactoryReadiness),
                                ..Default::default()
                            })
                        });
                        Readiness::default()
                    }
                },
            };
            if initial_failure.is_none()
                && let Err(error) = readiness::validate_policy_ports(
                    p.interface().inputs().keys().map(String::as_str),
                    &policy,
                )
            {
                initial_failure = Some(
                    Failure::external(format!("process {}: {error}", p.path()), error)
                        .with_kind(FailureKind::Framework)
                        .with_context(crate::FailureContext {
                            process: Some(index),
                            path: Some(p.path().into()),
                            ..Default::default()
                        }),
                );
            }
            RuntimeProcess {
                handoff,
                inputs: p
                    .interface()
                    .inputs()
                    .keys()
                    .map(|name| InputPort {
                        name: name.clone(),
                        queues: vec![],
                        cursor: 0,
                        eos_delivered: false,
                    })
                    .collect(),
                outputs: p
                    .interface()
                    .outputs()
                    .keys()
                    .map(|name| (name.clone(), vec![]))
                    .collect(),
                pending: None,
                pending_targets: vec![],
                waiting: None,
                waiting_policy: None,
                policy,
                notification_driven: false,
                cursor: 0,
                queued: false,
                done: false,
                source: compiled.source_starts().contains(&index),
                wake: Arc::new(WakeSlot::default()),
                request: None,
                resources: options.resources.get(p.path()).cloned().unwrap_or_default(),
                closed_in_activation: BTreeSet::new(),
            }
        })
        .collect();

    (processes, initial_failure)
}

struct WiredQueues {
    queues: Vec<ConnectionQueue>,
    endpoints: Vec<(QueueEndpoint, QueueEndpoint)>,
    ingress: BTreeMap<String, Vec<usize>>,
    captures: BTreeMap<String, usize>,
}

fn wire_queues(
    compiled: &CompiledNetwork,
    options: &SessionOptions,
    processes: &mut [RuntimeProcess],
) -> WiredQueues {
    let mut queues = vec![];
    let mut endpoints = vec![];
    let mut ingress = BTreeMap::new();
    let mut captures = BTreeMap::new();
    for edge in compiled.connections() {
        let (from, output) = edge.from();
        let (to, input) = edge.to();
        let index = queues.len();
        queues.push(ConnectionQueue::new(
            edge.capacity(),
            compiled.processes()[from].interface().outputs()[output]
                .value
                .clone(),
        ));
        processes[from].outputs.get_mut(output).unwrap().push(index);
        processes[to]
            .inputs
            .iter_mut()
            .find(|p| p.name == input)
            .unwrap()
            .queues
            .push(index);
        endpoints.push((
            QueueEndpoint::Process {
                index: from,
                port: output.into(),
            },
            QueueEndpoint::Process {
                index: to,
                port: input.into(),
            },
        ));
    }
    for (name, targets) in compiled.inputs() {
        let mut indices = vec![];
        for (to, input) in targets {
            let index = queues.len();
            queues.push(ConnectionQueue::new(
                compiled.ingress_capacities()[name],
                compiled.interface().inputs()[name].value.clone(),
            ));
            processes[*to]
                .inputs
                .iter_mut()
                .find(|p| p.name == *input)
                .unwrap()
                .queues
                .push(index);
            endpoints.push((
                QueueEndpoint::Input(name.clone()),
                QueueEndpoint::Process {
                    index: *to,
                    port: input.clone(),
                },
            ));
            indices.push(index);
        }
        ingress.insert(name.clone(), indices);
    }
    for (name, (from, output)) in compiled.outputs() {
        let index = queues.len();
        queues.push(ConnectionQueue::new(
            options.capture_capacity,
            compiled.interface().outputs()[name].value.clone(),
        ));
        processes[*from]
            .outputs
            .get_mut(output)
            .unwrap()
            .push(index);
        endpoints.push((
            QueueEndpoint::Process {
                index: *from,
                port: output.clone(),
            },
            QueueEndpoint::Capture(name.clone()),
        ));
        captures.insert(name.clone(), index);
    }

    WiredQueues {
        queues,
        endpoints,
        ingress,
        captures,
    }
}

impl NetworkSession {
    pub(super) fn new(compiled: CompiledNetwork, mut options: SessionOptions) -> Self {
        let initial_failure = preflight_options(&compiled, &mut options);
        let (mut processes, mut initial_failure) =
            spawn_processes(&compiled, &options, initial_failure);
        let WiredQueues {
            queues,
            endpoints,
            ingress,
            captures,
        } = wire_queues(&compiled, &options, &mut processes);
        let queue_lanes = if options.request_queue_policy == RequestQueuePolicy::BoundedLanes {
            vec![None; queues.len()]
        } else {
            Vec::new()
        };
        let mut downstream = vec![Vec::new(); compiled.processes().len()];
        for edge in compiled.connections() {
            downstream[edge.from().0].push(edge.to().0);
        }
        // A user policy must explicitly promise to send process-targeted
        // notifications for external selection changes. The default stays
        // polled. Query once and contain plugin panics as session failure.
        let mut always_poll = BTreeSet::new();
        let mut custom_readiness = Vec::new();
        for (index, process) in processes.iter_mut().enumerate() {
            if initial_failure.is_none()
                && let Readiness::Custom(policy) = &process.policy
            {
                custom_readiness.push(index);
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    policy.notification_driven()
                })) {
                    Ok(value) => process.notification_driven = value,
                    Err(payload) => {
                        let failure = crate::process::callback_panicked(
                            CallbackPhase::ReadinessRegistration,
                            payload,
                        )
                        .with_context(crate::FailureContext {
                            process: Some(index),
                            path: Some(compiled.processes()[index].path().into()),
                            phase: Some(CallbackPhase::ReadinessRegistration),
                            ..Default::default()
                        });
                        if initial_failure.is_none() {
                            initial_failure = Some(failure);
                        }
                    }
                }
            }
            if process.source
                || process.inputs.is_empty()
                || matches!(&process.policy, Readiness::Custom(_)) && !process.notification_driven
            {
                always_poll.insert(index);
            }
        }
        let mut session = Self {
            compiled,
            downstream,
            transport: ConnectionSet::new(queues),
            processes,
            custom_readiness,
            ingress,
            captures,
            request_lanes: BTreeMap::new(),
            lane_allocator: request::LaneAllocator::default(),
            request_cursors: BTreeMap::new(),
            queue_lanes,
            endpoints,
            ready: VecDeque::new(),
            always_poll,
            scoped_readiness_poll: BTreeMap::new(),
            dirty_processes: BTreeSet::new(),
            force_full_refresh: true,
            observed_global_wake_epoch: 0,
            terminal: None,
            failure: None,
            publications: 0,
            disposed_packets: 0,
            discarded_emissions: 0,
            input_failure: vec![],
            discard_outputs: options.discard_outputs,
            last_step: None,
            session_id: request::session_id(),
            generation: 0,
            next_request: 0,
            max_requests: options.max_requests.get(),
            request_failure_policy: options.request_failure_policy,
            request_queue_policy: options.request_queue_policy,
            deferred_timeout: options.deferred_timeout,
            dispatched_at: BTreeMap::new(),
            requests: BTreeMap::new(),
            signal: WakeSignal::new(),
            dispatcher: options.dispatcher,
            dispatch_return: None,
            execution_guards: options.execution_guards,
            lease: options.lease,
        };
        if let Some(error) = initial_failure {
            session.fail(error);
            return session;
        }
        for index in 0..session.processes.len() {
            if options.auto_start_sources && session.processes[index].source {
                let resources = session.processes[index].resources.clone();
                session.processes[index]
                    .handoff
                    .admit(ProcessContext::default().with_resources(resources))
                    .unwrap();
            }
        }
        session.refresh();
        session
    }
}
