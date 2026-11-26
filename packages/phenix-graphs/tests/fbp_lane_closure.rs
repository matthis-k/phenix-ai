use phenix_graph::{
    Failure, Interface, PortDef,
    network::NetworkBuilder,
    process::{Process, ProcessContext, Step},
    runtime::{
        Readiness, ReadinessPolicy, ReadinessSnapshot, RequestQueuePolicy, RequestStatus,
        SessionOptions,
    },
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

struct Root {
    taken: bool,
    gate: Arc<AtomicBool>,
    completed: Arc<AtomicUsize>,
}
impl Process for Root {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, Failure> {
        if !self.taken {
            assert!(cx.take("in").is_some());
            self.taken = true;
        }
        if !self.gate.load(Ordering::SeqCst) {
            return Ok(Step::WaitWake);
        }
        self.taken = false;
        self.completed.fetch_add(1, Ordering::SeqCst);
        Ok(Step::Complete)
    }
}

struct Absent {
    upstream: Arc<AtomicUsize>,
    completed: Arc<AtomicUsize>,
}
impl Process for Absent {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, Failure> {
        assert!(cx.take("in").is_none());
        assert!(cx.is_request_closed("in"));
        assert!(!cx.is_eos("in"));
        assert!(
            self.completed.load(Ordering::SeqCst) < self.upstream.load(Ordering::SeqCst),
            "absence preceded upstream completion"
        );
        self.completed.fetch_add(1, Ordering::SeqCst);
        Ok(Step::Complete)
    }
}

struct Closed(bool);
impl ReadinessPolicy for Closed {
    fn closure_ports(&self, _: usize) -> Vec<String> {
        vec!["in".into()]
    }
    fn notification_driven(&self) -> bool {
        self.0
    }
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        snapshot
            .heads
            .iter()
            .filter(|h| h.closed)
            .map(|h| h.port.clone())
            .collect()
    }
}

#[test]
fn overlapping_reused_lanes_preserve_upstream_packets_owners_and_closure_obligations() {
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for custom in [None, Some(false), Some(true)] {
            let gate = Arc::new(AtomicBool::new(false));
            let root_done = Arc::new(AtomicUsize::new(0));
            let middle_done = Arc::new(AtomicUsize::new(0));
            let sink_done = Arc::new(AtomicUsize::new(0));
            let iface = Arc::new(
                Interface::new(
                    [PortDef::local::<u64>("in")],
                    [PortDef::local::<u64>("out")],
                )
                .unwrap(),
            );
            let mut b = NetworkBuilder::new();
            let root = b
                .node("root", iface.clone(), {
                    let gate = gate.clone();
                    let completed = root_done.clone();
                    Arc::new(move || Root {
                        taken: false,
                        gate: gate.clone(),
                        completed: completed.clone(),
                    })
                })
                .unwrap();
            let middle = b
                .node("middle", iface.clone(), {
                    let upstream = root_done.clone();
                    let completed = middle_done.clone();
                    Arc::new(move || Absent {
                        upstream: upstream.clone(),
                        completed: completed.clone(),
                    })
                })
                .unwrap();
            let sink = b
                .node("sink", iface, {
                    let upstream = middle_done.clone();
                    let completed = sink_done.clone();
                    Arc::new(move || Absent {
                        upstream: upstream.clone(),
                        completed: completed.clone(),
                    })
                })
                .unwrap();
            let capacity = NonZeroUsize::new(4).unwrap();
            b.expose_input("trigger", root.input::<u64>("in").unwrap(), capacity)
                .unwrap();
            for (from, to) in [(&root, &middle), (&middle, &sink)] {
                b.connect(
                    from.output::<u64>("out").unwrap(),
                    to.input::<u64>("in").unwrap(),
                    capacity,
                )
                .unwrap();
            }
            let readiness = match custom {
                None => Readiness::optional(vec![], vec!["in".into()]),
                Some(notified) => Readiness::new(Arc::new(Closed(notified))),
            };
            let network = b.build().unwrap().compile().unwrap();
            let root_index = network
                .processes()
                .iter()
                .position(|p| p.path() == "root")
                .unwrap();
            let middle_index = network
                .processes()
                .iter()
                .position(|p| p.path() == "middle")
                .unwrap();
            let mut s = network.start_with_options(SessionOptions {
                max_requests: capacity,
                request_queue_policy: policy,
                readiness: [
                    ("middle".into(), readiness.clone()),
                    ("sink".into(), readiness),
                ]
                .into(),
                ..Default::default()
            });
            let mut ids: Vec<_> = (0..4).map(|_| s.begin_request(None).unwrap()).collect();
            let cancelled = ids.remove(0);
            s.cancel_request(cancelled).unwrap();
            s.release_request(cancelled).unwrap();
            ids.push(s.begin_request(None).unwrap());
            assert_ne!(ids[3], cancelled);
            for &id in &ids {
                s.inject_request_local(id, "trigger", 1_u64).unwrap();
                s.close_request(id).unwrap();
            }
            let (index, job) = s.next_job().unwrap();
            assert_eq!(index, root_index);
            s.complete(index, job.run()).unwrap();
            assert!(
                s.next_job().is_none(),
                "upstream WaitWake owner/queued roots cannot prove absence"
            );
            assert_eq!(middle_done.load(Ordering::SeqCst), 0);
            assert_eq!(sink_done.load(Ordering::SeqCst), 0);
            assert_eq!(s.request_work(ids[0]).unwrap().waiting_wake, 1);
            for &id in &ids[1..] {
                assert_eq!(s.request_work(id).unwrap().queued, 1);
            }
            // Cancel a packet-bearing queued scope, including its downstream
            // closure obligations, while another request still owns root.
            let cancelled = ids.pop().unwrap();
            s.cancel_request(cancelled).unwrap();
            assert_eq!(s.request_disposal(cancelled).unwrap().queued_packets, 1);
            s.release_request(cancelled).unwrap();
            let replacement = s.begin_request(None).unwrap();
            assert_ne!(replacement, cancelled);
            s.inject_request_local(replacement, "trigger", 2_u64)
                .unwrap();
            s.close_request(replacement).unwrap();
            ids.push(replacement);
            assert!(s.next_job().is_none());
            assert_eq!(s.request_work(ids[0]).unwrap().waiting_wake, 1);
            gate.store(true, Ordering::SeqCst);
            assert!(s.wake_handle("root").unwrap().wake());
            // Hold the first zero-packet middle activation outside the runtime.
            // Other roots may finish, but sink must await middle ownership AND
            // every foreign-owned request's unobserved middle obligation.
            let (held_index, held) = loop {
                let (index, job) = s.next_job().unwrap();
                if index == middle_index {
                    break (index, job);
                }
                assert_eq!(index, root_index);
                s.complete(index, job.run()).unwrap();
            };
            while let Some((index, job)) = s.next_job() {
                assert_eq!(
                    index, root_index,
                    "sink bypassed a middle owner or closure obligation"
                );
                s.complete(index, job.run()).unwrap();
            }
            assert_eq!(sink_done.load(Ordering::SeqCst), 0);
            s.complete(held_index, held.run()).unwrap();
            assert!(!s.run_until_idle(64).budget_exhausted);
            assert!(s.failure().is_none(), "{:?}", s.failure());
            assert_eq!(root_done.load(Ordering::SeqCst), 4);
            assert_eq!(middle_done.load(Ordering::SeqCst), 4);
            assert_eq!(sink_done.load(Ordering::SeqCst), 4);
            for id in ids {
                assert_eq!(
                    s.request_status(id).unwrap(),
                    RequestStatus::Settled { captures: 0 }
                );
                s.release_request(id).unwrap();
            }
        }
    }
}
