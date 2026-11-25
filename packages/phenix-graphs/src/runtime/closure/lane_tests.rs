use super::*;
use crate::{
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::Process,
};

// Frozen pre-change traversal. The optional projection deliberately filters the
// physical inventory, independently of the production input_ports lookup.
fn reference(
    s: &NetworkSession,
    index: usize,
    port: usize,
    request: Option<RequestId>,
    own: bool,
) -> ClosureProof {
    let queues = &s.processes[index].inputs[port].queues;
    let Some(queue) = queues
        .iter()
        .copied()
        .find(|q| !s.transport.queues()[*q].is_eos())
    else {
        return ClosureProof::GlobalEos;
    };
    let Some(request) = request else {
        return ClosureProof::NotProven(if !s.transport.queues()[queue].is_empty() {
            ClosureWitness::BufferedGlobalInput { queue }
        } else {
            ClosureWitness::OpenGlobalWriter { queue }
        });
    };
    let scope = &s.requests[&request];
    if !scope.sealed {
        return ClosureProof::NotProven(ClosureWitness::UnsealedRequest { request });
    }
    let whole_graph = scope.reachable.len() == s.processes.len();
    let relevant = |q: usize| {
        whole_graph
            || match &s.endpoints[q].0 {
                QueueEndpoint::Process { index, .. } => scope.reachable.contains(index),
                QueueEndpoint::Input(_) | QueueEndpoint::Capture(_) => true,
            }
    };
    let included = |q: usize| {
        !own || s.request_queue_policy == RequestQueuePolicy::SharedFifo
            || s.queue_lanes[q] == Some(scope.lane)
    };
    for queue in queues
        .iter()
        .copied()
        .filter(|q| included(*q) && relevant(*q))
    {
        let count = s.transport.queues()[queue].request_count(request);
        if count != 0 {
            return ClosureProof::NotProven(ClosureWitness::RequestPacket { queue, count });
        }
    }
    let mut visited = BTreeSet::new();
    let mut stack = vec![];
    for queue in queues.iter().filter(|q| included(**q)) {
        if let QueueEndpoint::Process { index, .. } = &s.endpoints[*queue].0
            && (whole_graph || scope.reachable.contains(index))
        {
            stack.push(*index);
        }
    }
    while let Some(producer) = stack.pop() {
        if !visited.insert(producer) {
            continue;
        }
        let p = &s.processes[producer];
        if p.request == Some(request) {
            return ClosureProof::NotProven(ClosureWitness::UpstreamOwner { process: producer });
        }
        if s.closure_obligation(producer, request) {
            return ClosureProof::NotProven(ClosureWitness::ClosureObligation {
                process: producer,
            });
        }
        for queue in p.inputs.iter().flat_map(|port| &port.queues) {
            if !included(*queue) || !relevant(*queue) {
                continue;
            }
            let count = s.transport.queues()[*queue].request_count(request);
            if count != 0 {
                return ClosureProof::NotProven(ClosureWitness::RequestPacket {
                    queue: *queue,
                    count,
                });
            }
            if let QueueEndpoint::Process { index, .. } = &s.endpoints[*queue].0 {
                stack.push(*index);
            }
        }
    }
    ClosureProof::ScopedRequestAbsent
}

struct Idle;
impl Process for Idle {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
        Ok(Step::Complete)
    }
}

fn network(nodes: usize, edges: &[(usize, usize)]) -> CompiledNetwork {
    let mut b = NetworkBuilder::new();
    let interface = Arc::new(
        Interface::new(
            ["a", "b", "empty"].map(PortDef::local::<u64>),
            [PortDef::local::<u64>("out")],
        )
        .unwrap(),
    );
    let nodes: Vec<_> = (0..nodes)
        .map(|n| {
            b.node(format!("n{n}"), interface.clone(), Arc::new(|| Idle))
                .unwrap()
        })
        .collect();
    for (i, node) in nodes.iter().enumerate() {
        b.expose_input(
            format!("in{i}"),
            node.input::<u64>("a").unwrap(),
            NonZeroUsize::new(2).unwrap(),
        )
        .unwrap();
    }
    for &(from, to) in edges {
        b.connect(
            nodes[from].output::<u64>("out").unwrap(),
            nodes[to]
                .input::<u64>(if from % 2 == 0 { "a" } else { "b" })
                .unwrap(),
            NonZeroUsize::new(2).unwrap(),
        )
        .unwrap();
    }
    b.build().unwrap().compile().unwrap()
}

fn put(s: &mut NetworkSession, queue: usize, request: Option<RequestId>) {
    let mut packet = Packet::new(Value::local(1_u64));
    packet.metadata = Some(Arc::new(CaptureMetadata {
        session: s.session_id,
        generation: s.generation,
        process: None,
        output: "test".into(),
        activation: 0,
        request,
        capture: None,
    }));
    let mut prepared = s
        .transport
        .prepare(&packet, &[FanoutTarget::identity(queue)])
        .unwrap();
    s.transport.commit(&mut prepared).unwrap();
}

fn compare(s: &NetworkSession, request: RequestId) {
    for index in 0..s.processes.len() {
        for port in 0..s.processes[index].inputs.len() {
            for request in [None, Some(request)] {
                let actual = s.input_closure_proof(index, port, request);
                assert_eq!(
                    actual,
                    reference(s, index, port, request, true),
                    "own-order {index}:{port}"
                );
                // In these canonical-copy topologies even the old duplicate
                // pushes preserve exact first-witness ordering, not just truth.
                assert_eq!(
                    actual,
                    reference(s, index, port, request, false),
                    "baseline {index}:{port}"
                );
                if let ClosureProof::NotProven(ClosureWitness::RequestPacket { queue, count }) =
                    actual
                {
                    assert_eq!(
                        count,
                        s.transport.queues()[queue].request_count(request.unwrap())
                    );
                    if s.request_queue_policy == RequestQueuePolicy::BoundedLanes {
                        assert_eq!(
                            s.queue_lanes[queue],
                            Some(s.requests[&request.unwrap()].lane)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn exhaustive_small_topologies_match_original_and_own_lane_oracles() {
    let possible = [(0, 1), (0, 2), (1, 0), (1, 2), (2, 0), (2, 1)];
    for topology in 0..64 {
        let edges: Vec<_> = possible
            .iter()
            .enumerate()
            .filter_map(|(bit, edge)| (topology & (1 << bit) != 0).then_some(*edge))
            .collect();
        for policy in [
            RequestQueuePolicy::SharedFifo,
            RequestQueuePolicy::BoundedLanes,
        ] {
            let mut s = network(4, &edges).start_with_options(SessionOptions {
                max_requests: NonZeroUsize::new(4).unwrap(),
                request_queue_policy: policy,
                ..Default::default()
            });
            let ids: Vec<_> = (0..4).map(|_| s.begin_request(None).unwrap()).collect();
            // Retain foreign live lanes, cancel/release/reuse the lowest slot.
            s.cancel_request(ids[0]).unwrap();
            s.release_request(ids[0]).unwrap();
            let request = s.begin_request(None).unwrap();
            assert_eq!(s.requests[&request].lane, 0);
            assert_ne!(request, ids[0]);
            // Foreign and unscoped packets must never be request witnesses.
            let foreign_queue = if policy == RequestQueuePolicy::BoundedLanes {
                s.request_lanes[&1].inputs["in3"][0]
            } else {
                s.ingress["in3"][0]
            };
            put(&mut s, foreign_queue, Some(ids[1]));
            let canonical = s.ingress["in3"][0];
            put(&mut s, canonical, None);
            for roots in 0..8 {
                s.requests.get_mut(&request).unwrap().reachable.clear();
                for root in 0..3 {
                    if roots & (1 << root) != 0 {
                        s.note_request_root(request, root);
                    }
                }
                // Base-4 states per producer exhaust all combinations of
                // clear, own owner, two own packets, and closure obligation.
                for state in 0..64 {
                    let mut packet_queues = vec![];
                    for index in 0..3 {
                        let kind = (state >> (index * 2)) & 3;
                        s.processes[index].request = (kind == 1).then_some(request);
                        s.processes[index].policy = if kind == 3 {
                            Readiness::Optional {
                                required: vec![],
                                optional: vec!["a".into()],
                            }
                        } else {
                            Readiness::Any
                        };
                        if kind == 2 && s.requests[&request].reachable.contains(&index) {
                            let queue = if policy == RequestQueuePolicy::BoundedLanes {
                                s.request_lanes[&0].inputs[&format!("in{index}")][0]
                            } else {
                                s.ingress[&format!("in{index}")][0]
                            };
                            put(&mut s, queue, Some(request));
                            put(&mut s, queue, Some(request));
                            packet_queues.push(queue);
                        }
                    }
                    for sealed in [false, true] {
                        s.requests.get_mut(&request).unwrap().sealed = sealed;
                        compare(&s, request);
                    }
                    for queue in packet_queues {
                        assert!(s.transport.pop(queue).is_some());
                        assert!(s.transport.pop(queue).is_some());
                    }
                }
            }
            // Force whole-graph fast path too (node 3 is otherwise irrelevant).
            s.requests.get_mut(&request).unwrap().reachable = (0..4).collect();
            compare(&s, request);
        }
    }
}

#[test]
fn diamond_cycle_self_loop_and_witness_precedence() {
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let mut s = network(5, &[(0, 1), (0, 2), (1, 3), (2, 3), (2, 0), (1, 1)])
            .start_with_options(SessionOptions {
                request_queue_policy: policy,
                ..Default::default()
            });
        let r = s.begin_request(None).unwrap();
        s.note_request_root(r, 0);
        s.requests.get_mut(&r).unwrap().sealed = true;
        for owner_mask in 0..16 {
            for obligation_mask in 0..16 {
                for index in 0..4 {
                    s.processes[index].request = (owner_mask & (1 << index) != 0).then_some(r);
                    s.processes[index].policy = if obligation_mask & (1 << index) != 0 {
                        Readiness::Optional {
                            required: vec![],
                            optional: vec!["a".into()],
                        }
                    } else {
                        Readiness::Any
                    };
                }
                compare(&s, r);
            }
        }
        // All physical writers closed and empty takes precedence over both an
        // unsealed scope and upstream owner/obligation witnesses.
        for q in 0..s.transport.queues().len() {
            s.transport.close_writer(q).unwrap();
        }
        s.requests.get_mut(&r).unwrap().sealed = false;
        compare(&s, r);
        for index in 0..5 {
            for port in 0..3 {
                assert_eq!(
                    s.input_closure_proof(index, port, Some(r)),
                    ClosureProof::GlobalEos
                );
            }
        }
    }
}

#[test]
fn every_physical_own_edge_has_exact_packet_identity_and_count() {
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let mut s = network(4, &[(0, 1), (0, 2), (1, 3), (2, 3), (3, 0)]).start_with_options(
            SessionOptions {
                max_requests: NonZeroUsize::new(4).unwrap(),
                request_queue_policy: policy,
                ..Default::default()
            },
        );
        let ids: Vec<_> = (0..4).map(|_| s.begin_request(None).unwrap()).collect();
        let r = ids[3];
        s.note_request_root(r, 0);
        s.requests.get_mut(&r).unwrap().sealed = true;
        let queues = if policy == RequestQueuePolicy::BoundedLanes {
            assert!(
                !s.request_lanes[&3].input_ports.contains_key(&(0, 2)),
                "unconnected port has no mapping"
            );
            s.request_lanes[&3].queues.clone()
        } else {
            (0..s.transport.queues().len()).collect()
        };
        for queue in queues {
            put(&mut s, queue, Some(r));
            put(&mut s, queue, Some(r));
            compare(&s, r);
            let QueueEndpoint::Process { index, port } = &s.endpoints[queue].1 else {
                unreachable!()
            };
            let port = s.processes[*index]
                .inputs
                .iter()
                .position(|p| &p.name == port)
                .unwrap();
            assert_eq!(
                s.input_closure_proof(*index, port, Some(r)),
                ClosureProof::NotProven(ClosureWitness::RequestPacket { queue, count: 2 })
            );
            s.transport.pop(queue).unwrap();
            s.transport.pop(queue).unwrap();
        }
    }
}

#[test]
fn global_eos_must_inspect_foreign_queues_even_when_own_lane_is_closed() {
    for lanes in [1, 4, 16, 64] {
        let mut s = network(1, &[]).start_with_options(SessionOptions {
            max_requests: NonZeroUsize::new(lanes).unwrap(),
            request_queue_policy: RequestQueuePolicy::BoundedLanes,
            ..Default::default()
        });
        let ids: Vec<_> = (0..lanes).map(|_| s.begin_request(None).unwrap()).collect();
        let own = ids[0];
        s.requests.get_mut(&own).unwrap().sealed = true;
        let last = s.request_lanes[&(lanes - 1)].inputs["in0"][0];
        for queue in 0..s.transport.queues().len() {
            if queue != last {
                s.transport.close_writer(queue).unwrap();
            }
        }
        assert_eq!(s.first_non_eos_queue(0, 0), Some(last));
        assert_eq!(
            s.input_closure_proof(0, 0, Some(own)),
            ClosureProof::ScopedRequestAbsent
        );
        assert_eq!(
            s.input_closure_proof(0, 0, None),
            ClosureProof::NotProven(ClosureWitness::OpenGlobalWriter { queue: last })
        );
        // A foreign packet in a closed writer also prevents global EOS.
        if lanes > 1 {
            put(&mut s, last, Some(ids[lanes - 1]));
            s.transport.close_writer(last).unwrap();
            assert_eq!(
                s.input_closure_proof(0, 0, Some(own)),
                ClosureProof::ScopedRequestAbsent
            );
            assert_eq!(
                s.input_closure_proof(0, 0, None),
                ClosureProof::NotProven(ClosureWitness::BufferedGlobalInput { queue: last })
            );
            s.transport.pop(last).unwrap();
        } else {
            s.transport.close_writer(last).unwrap();
        }
        s.requests.get_mut(&own).unwrap().sealed = false;
        assert_eq!(
            s.input_closure_proof(0, 0, Some(own)),
            ClosureProof::GlobalEos
        );
        compare(&s, own);
    }
}

#[test]
fn sealed_zero_input_source_delegation_blocks_downstream_absence_until_completion() {
    struct Deferred(std::sync::Mutex<Option<ActivationJob>>);
    impl ProcessDispatcher for Deferred {
        fn dispatch(&self, job: ActivationJob) -> DispatchResult {
            assert!(self.0.lock().unwrap().replace(job).is_none());
            DispatchResult::Deferred
        }
    }

    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let mut b = NetworkBuilder::new();
        let source = b
            .node(
                "source",
                Arc::new(Interface::new([], [PortDef::local::<u64>("out")]).unwrap()),
                Arc::new(|| Idle),
            )
            .unwrap();
        b.start(&source).unwrap();
        let sink = b
            .node(
                "sink",
                Arc::new(Interface::new([PortDef::local::<u64>("a")], []).unwrap()),
                Arc::new(|| Idle),
            )
            .unwrap();
        b.connect(
            source.output::<u64>("out").unwrap(),
            sink.input::<u64>("a").unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
        let delegated = Arc::new(Deferred(std::sync::Mutex::new(None)));
        let mut s = b
            .build()
            .unwrap()
            .compile()
            .unwrap()
            .start_with_options(SessionOptions {
                auto_start_sources: false,
                request_queue_policy: policy,
                dispatcher: delegated.clone(),
                readiness: [(
                    "sink".into(),
                    Readiness::Optional {
                        required: vec![],
                        optional: vec!["a".into()],
                    },
                )]
                .into(),
                ..Default::default()
            });
        let source = s.compiled.physical_process_index("source").unwrap();
        let sink = s.compiled.physical_process_index("sink").unwrap();
        let r = s.begin_request(None).unwrap();
        s.start_request_source(r, "source").unwrap();
        s.close_request(r).unwrap();
        assert_eq!(s.run_until_idle(4).steps, 1);
        let job = delegated.0.lock().unwrap().take().unwrap();
        assert!(s.processes[source].inputs.is_empty());
        assert_eq!(job.metadata().unwrap().request, Some(r));
        assert_eq!(
            s.processes[source].handoff.outstanding(),
            Some(job.ticket())
        );
        assert_eq!(
            s.input_closure_proof(sink, 0, Some(r)),
            ClosureProof::NotProven(ClosureWitness::UpstreamOwner { process: source })
        );
        compare(&s, r);
        assert_eq!(s.run_until_idle(4).steps, 0);
        assert_eq!(s.request_status(r).unwrap(), RequestStatus::Pending);
        s.complete(source, job.run()).unwrap();
        assert_eq!(
            s.input_closure_proof(sink, 0, Some(r)),
            ClosureProof::ScopedRequestAbsent
        );
        compare(&s, r);
        let (index, job) = s.next_job().unwrap();
        assert_eq!(index, sink);
        assert_eq!(job.metadata().unwrap().request, Some(r));
        assert_eq!(s.request_work(r).unwrap().queued, 0, "sink is closure-only");
        s.complete(index, job.run()).unwrap();
        assert!(s.next_job().is_none());
        assert_eq!(
            s.request_status(r).unwrap(),
            RequestStatus::Settled { captures: 0 }
        );
    }
}

#[test]
fn closed_writer_own_packet_blocks_absence_until_reader_disposal() {
    let mut s = network(1, &[]).start_with_options(SessionOptions {
        max_requests: NonZeroUsize::new(2).unwrap(),
        request_queue_policy: RequestQueuePolicy::BoundedLanes,
        ..Default::default()
    });
    let r = s.begin_request(None).unwrap();
    let foreign = s.begin_request(None).unwrap();
    s.note_request_root(r, 0);
    s.requests.get_mut(&r).unwrap().sealed = true;
    let queue = s.request_lanes[&s.requests[&r].lane].input_ports[&(0, 0)][0];
    let foreign_queue = s.request_lanes[&s.requests[&foreign].lane].input_ports[&(0, 0)][0];
    put(&mut s, queue, Some(r));
    s.transport.close_writer(queue).unwrap();
    s.transport.close_writer(s.ingress["in0"][0]).unwrap();
    assert!(!s.transport.queues()[foreign_queue].writer_closed());
    assert_eq!(
        s.input_closure_proof(0, 0, Some(r)),
        ClosureProof::NotProven(ClosureWitness::RequestPacket { queue, count: 1 })
    );
    compare(&s, r);
    let disposed = s.transport.close_reader(queue).unwrap();
    assert_eq!(disposed.len(), 1);
    assert_eq!(disposed[0].metadata().unwrap().request(), Some(r));
    assert_eq!(s.transport.queues()[queue].request_count(r), 0);
    assert!(s.transport.queues()[queue].is_eos());
    assert_eq!(
        s.input_closure_proof(0, 0, Some(r)),
        ClosureProof::ScopedRequestAbsent
    );
    assert_eq!(
        s.input_closure_proof(0, 0, None),
        ClosureProof::NotProven(ClosureWitness::OpenGlobalWriter {
            queue: foreign_queue
        })
    );
    compare(&s, r);
    s.transport.close_writer(foreign_queue).unwrap();
    s.requests.get_mut(&r).unwrap().sealed = false;
    assert_eq!(
        s.input_closure_proof(0, 0, Some(r)),
        ClosureProof::GlobalEos
    );
    compare(&s, r);
}

#[test]
#[ignore = "manual setup-excluded profile of the unchanged full global EOS scan"]
fn profile_global_eos_scan() {
    use std::hint::black_box;
    for lanes in [1, 4, 16, 64] {
        let mut s = network(1, &[]).start_with_options(SessionOptions {
            max_requests: NonZeroUsize::new(lanes).unwrap(),
            request_queue_policy: RequestQueuePolicy::BoundedLanes,
            ..Default::default()
        });
        let ids: Vec<_> = (0..lanes).map(|_| s.begin_request(None).unwrap()).collect();
        for queue in 0..s.transport.queues().len() {
            s.transport.close_writer(queue).unwrap();
        }
        for sample in 0..7 {
            let start = Instant::now();
            for _ in 0..1_000_000 {
                black_box(black_box(&s).input_closure_proof(0, 0, Some(black_box(ids[0]))));
            }
            println!(
                "eos,lanes={lanes},physical_queues={},sample={sample},ns_per_proof={:.2}",
                s.processes[0].inputs[0].queues.len(),
                start.elapsed().as_nanos() as f64 / 1_000_000.0
            );
        }
    }
}
