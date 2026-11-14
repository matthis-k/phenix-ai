use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
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
        atomic::{AtomicUsize, Ordering},
    },
};

struct SelectPort {
    port: &'static str,
    lane: Option<usize>,
    calls: Arc<AtomicUsize>,
}

impl ReadinessPolicy for SelectPort {
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        assert_eq!(snapshot.request.is_some(), self.lane.is_some());
        assert_eq!(snapshot.correlation_lane, self.lane);
        self.calls.fetch_add(1, Ordering::Relaxed);
        snapshot
            .heads
            .iter()
            .filter(|head| head.port == self.port && head.available)
            .map(|head| head.port.clone())
            .collect()
    }
}

struct StagedLane {
    lane: Option<usize>,
    next: Readiness,
    steps: Arc<AtomicUsize>,
}

impl Process for StagedLane {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        assert_eq!(cx.correlation_lane(), self.lane);
        match self.steps.fetch_add(1, Ordering::Relaxed) {
            0 => {
                assert!(cx.take("start").is_some());
                Ok(Step::WaitInputs(self.next.clone()))
            }
            1 => {
                assert!(cx.take("next").is_some());
                Ok(Step::Complete)
            }
            _ => panic!("unexpected activation"),
        }
    }
}

#[test]
fn initial_and_staged_custom_snapshots_preserve_lanes_in_both_queue_modes() {
    for queue_policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for scoped in [false, true] {
            for initial_custom in [false, true] {
                let lane = scoped.then_some(1);
                let initial_calls = Arc::new(AtomicUsize::new(0));
                let staged_calls = Arc::new(AtomicUsize::new(0));
                let steps = Arc::new(AtomicUsize::new(0));
                let next = Readiness::Custom(Arc::new(SelectPort {
                    port: "next",
                    lane,
                    calls: staged_calls.clone(),
                }));
                let mut builder = NetworkBuilder::new();
                let stage = builder
                    .node(
                        "stage",
                        Arc::new(
                            Interface::new(
                                ["start", "next"].map(|name| PortDef::wire::<u64>(name).unwrap()),
                                [],
                            )
                            .unwrap(),
                        ),
                        {
                            let steps = steps.clone();
                            Arc::new(move || StagedLane {
                                lane,
                                next: next.clone(),
                                steps: steps.clone(),
                            })
                        },
                    )
                    .unwrap();
                for name in ["start", "next"] {
                    builder
                        .expose_input(
                            name,
                            stage.wire_input::<u64>(name).unwrap(),
                            NonZeroUsize::new(1).unwrap(),
                        )
                        .unwrap();
                }
                let mut options = SessionOptions {
                    request_queue_policy: queue_policy,
                    ..SessionOptions::default()
                };
                if initial_custom {
                    options.readiness.insert(
                        "stage".into(),
                        Readiness::Custom(Arc::new(SelectPort {
                            port: "start",
                            lane,
                            calls: initial_calls.clone(),
                        })),
                    );
                }
                let mut session = builder
                    .build()
                    .unwrap()
                    .compile()
                    .unwrap()
                    .start_with_options(options);
                // Occupy lane 0 so scoped assertions cannot pass with a fixed zero.
                let held = scoped.then(|| session.begin_request(None).unwrap());
                let request = scoped.then(|| session.begin_request(None).unwrap());
                for (index, port) in ["start", "next"].into_iter().enumerate() {
                    if let Some(request) = request {
                        session
                            .inject_request(request, port, Value::of(7_u64))
                            .unwrap();
                    } else {
                        session.inject(port, Value::of(7_u64)).unwrap();
                    }
                    assert_eq!(session.run_until_idle(8).steps, 1);
                    assert_eq!(steps.load(Ordering::Relaxed), index + 1);
                }
                assert!(staged_calls.load(Ordering::Relaxed) > 0);
                assert_eq!(initial_calls.load(Ordering::Relaxed) > 0, initial_custom);
                for request in request.into_iter().chain(held) {
                    session.close_request(request).unwrap();
                    assert_eq!(
                        session.request_status(request).unwrap(),
                        RequestStatus::Settled { captures: 0 }
                    );
                    session.release_request(request).unwrap();
                }
            }
        }
    }
}
