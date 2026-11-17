use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::{Process, ProcessContext, ProcessFactory, Step},
    runtime::{
        Readiness, ReadinessPolicy, ReadinessSnapshot, RequestQueuePolicy, RequestStatus,
        SessionOptions, SessionState,
    },
};
use std::{num::NonZeroUsize, sync::Arc};

struct DropInput;
impl Process for DropInput {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        assert!(cx.take("in").is_some());
        Ok(Step::Complete)
    }
}

struct ObserveAbsence;
impl Process for ObserveAbsence {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        assert!(cx.take("opt").is_none());
        assert!(cx.is_request_closed("opt"));
        assert!(!cx.is_eos("opt"));
        Ok(Step::Complete)
    }
}

struct AbsentFactory(bool);
impl ProcessFactory for AbsentFactory {
    fn spawn(&self) -> Box<dyn Process> {
        Box::new(ObserveAbsence)
    }

    fn readiness(&self) -> Readiness {
        Readiness::Custom(Arc::new(SelectAbsence(self.0)))
    }
}

struct SelectAbsence(bool);
impl ReadinessPolicy for SelectAbsence {
    fn closure_ports(&self, _: usize) -> Vec<String> {
        vec!["opt".into()]
    }

    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        snapshot
            .heads
            .iter()
            .filter(|head| head.closed)
            .map(|head| head.port.clone())
            .collect()
    }

    fn notification_driven(&self) -> bool {
        self.0
    }
}

#[test]
fn factory_custom_closure_is_registered_for_polled_and_notified_policies_on_each_request() {
    let one = NonZeroUsize::new(1).unwrap();
    for notified in [false, true] {
        for request_queue_policy in [
            RequestQueuePolicy::SharedFifo,
            RequestQueuePolicy::BoundedLanes,
        ] {
            let mut builder = NetworkBuilder::new();
            let root = builder
                .node(
                    "root",
                    Arc::new(
                        Interface::new(
                            [PortDef::wire::<u64>("in").unwrap()],
                            [PortDef::wire::<u64>("out").unwrap()],
                        )
                        .unwrap(),
                    ),
                    Arc::new(|| DropInput),
                )
                .unwrap();
            let sink = builder
                .node(
                    "sink",
                    Arc::new(Interface::new([PortDef::wire::<u64>("opt").unwrap()], []).unwrap()),
                    Arc::new(AbsentFactory(notified)),
                )
                .unwrap();
            builder
                .expose_input("trigger", root.wire_input::<u64>("in").unwrap(), one)
                .unwrap();
            builder
                .connect(
                    root.wire_output::<u64>("out").unwrap(),
                    sink.wire_input::<u64>("opt").unwrap(),
                    one,
                )
                .unwrap();
            let mut session = builder
                .build()
                .unwrap()
                .compile()
                .unwrap()
                .start_with_options(SessionOptions {
                    request_queue_policy,
                    ..SessionOptions::default()
                });
            for value in 0..2_u64 {
                let request = session.begin_request(None).unwrap();
                session
                    .inject_request(request, "trigger", Value::of(value))
                    .unwrap();
                session.close_request(request).unwrap();
                let report = session.run_until_idle(8);
                assert_eq!(report.steps, 2);
                assert!(!report.budget_exhausted);
                assert_eq!(
                    session.request_status(request).unwrap(),
                    RequestStatus::Settled { captures: 0 }
                );
                session.release_request(request).unwrap();
                assert_eq!(session.state(), SessionState::Quiescent);
            }
        }
    }
}
