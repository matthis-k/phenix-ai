//! Terminal outcomes are final: cancellation cannot rewrite a failure or
//! completion, nor discard captures retained for inspection after failure.
use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::{Process, ProcessContext, ProcessEmission, Step},
    runtime::{Readiness, SessionOptions, SessionState},
};
use std::sync::Arc;

#[test]
fn cancellation_cannot_overwrite_preflight_failure() {
    let graph = NetworkBuilder::new().build().unwrap().compile().unwrap();
    let mut options = SessionOptions::default();
    options.readiness.insert("missing".into(), Readiness::All);
    let mut session = graph.start_with_options(options);
    assert_eq!(session.state(), SessionState::Failed);
    let failure = session.failure().unwrap().to_owned();

    session.cancel();
    session.cancel();
    assert_eq!(session.state(), SessionState::Failed);
    assert_eq!(session.failure(), Some(failure.as_str()));
}

#[test]
fn cancellation_cannot_overwrite_completed_session() {
    let graph = NetworkBuilder::new().build().unwrap().compile().unwrap();
    let mut session = graph.start();
    assert_eq!(session.state(), SessionState::Completed);

    session.cancel();
    assert_eq!(session.state(), SessionState::Completed);
    assert_eq!(session.disposed_packets(), 0);
}

struct FailAfterCapture {
    emitted: bool,
}

impl Process for FailAfterCapture {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            return Err("deliberate failure after accepted capture".into());
        }
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new("out", Value::local(7_i64))))
    }
}

#[test]
fn failed_session_keeps_committed_capture_after_cancellation() {
    let mut builder = NetworkBuilder::new();
    let source = builder
        .node(
            "source",
            Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap()),
            Arc::new(|| FailAfterCapture { emitted: false }),
        )
        .unwrap();
    builder
        .expose_output("captured", source.output::<i64>("out").unwrap())
        .unwrap();
    builder.start(&source).unwrap();
    let graph = builder.build().unwrap().compile().unwrap();
    let mut session = graph.start();

    let report = session.run_until_idle(12);
    assert_eq!(report.state, SessionState::Failed);
    assert!(session.failure().unwrap().contains("deliberate failure"));
    let failure = session.failure().unwrap().to_owned();
    let disposed = session.disposed_packets();

    session.cancel();
    assert_eq!(session.state(), SessionState::Failed);
    assert_eq!(session.failure(), Some(failure.as_str()));
    assert_eq!(session.disposed_packets(), disposed);
    let packet = session.take_output("captured").unwrap().unwrap();
    assert_eq!(packet.value().borrow::<i64>().copied(), Some(7));
}
