use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::{Process, ProcessContext, ProcessEmission, Step},
    runtime::{SessionOptions, SessionState},
};
use std::{num::NonZeroUsize, sync::Arc};
fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}
struct Repeat;
impl Process for Repeat {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if let Some(v) = cx.take("in") {
            Ok(Step::Emit(ProcessEmission::new("out", v)))
        } else {
            Ok(Step::Complete)
        }
    }
}
#[test]
fn self_cycle_is_bounded_and_reports_capture_backpressure() {
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "loop",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<i64>("in").unwrap()],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(|| Repeat),
        )
        .unwrap();
    b.connect(
        p.wire_output::<i64>("out").unwrap(),
        p.wire_input::<i64>("in").unwrap(),
        one(),
    )
    .unwrap();
    b.expose_input("seed", p.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    b.expose_output("tap", p.wire_output::<i64>("out").unwrap())
        .unwrap();
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(SessionOptions {
            capture_capacity: one(),
            ..Default::default()
        });
    assert_eq!(s.state(), SessionState::Quiescent);
    assert_eq!(s.input_waits().len(), 2);
    s.inject("seed", Value::of(1_i64)).unwrap();
    assert_eq!(s.run_until_idle(100).state, SessionState::Waiting);
    assert_eq!(s.blocked_queues().len(), 1);
    assert!(s.blocked_cycles().is_empty());
    assert!(s.take_output("tap").unwrap().is_some());
    assert_eq!(s.run_until_idle(100).state, SessionState::Waiting);
    s.cancel();
    assert_eq!(s.state(), SessionState::Cancelled);
}

struct Burst {
    remaining: usize,
}
impl Process for Burst {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.remaining == 0 {
            cx.take("in");
            self.remaining = 3;
        }
        self.remaining -= 1;
        Ok(Step::Emit(ProcessEmission::new("out", Value::of(1_i64))))
    }
}
#[test]
fn full_cycle_has_real_queue_dependencies_not_a_progress_guarantee() {
    let mut b = NetworkBuilder::new();
    let iface = Arc::new(
        Interface::new(
            [PortDef::wire::<i64>("in").unwrap()],
            [PortDef::wire::<i64>("out").unwrap()],
        )
        .unwrap(),
    );
    let a = b
        .node("a", iface.clone(), Arc::new(|| Burst { remaining: 0 }))
        .unwrap();
    let c = b
        .node("b", iface, Arc::new(|| Burst { remaining: 0 }))
        .unwrap();
    b.connect(
        a.wire_output::<i64>("out").unwrap(),
        c.wire_input::<i64>("in").unwrap(),
        one(),
    )
    .unwrap();
    b.connect(
        c.wire_output::<i64>("out").unwrap(),
        a.wire_input::<i64>("in").unwrap(),
        one(),
    )
    .unwrap();
    b.expose_input("seed", a.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    let mut s = b.build().unwrap().compile().unwrap().start();
    s.inject("seed", Value::of(1_i64)).unwrap();
    assert_eq!(s.run_until_idle(100).state, SessionState::Waiting);
    let waits = s.blocked_queues();
    assert_eq!(waits.len(), 2);
    assert!(waits.iter().all(|q| q.occupancy == 1 && q.capacity == 1));
    let cycles = s.blocked_cycles();
    assert_eq!(cycles.len(), 1);
    assert_eq!(cycles[0].processes, vec![0, 1]);
    assert_eq!(cycles[0].queues.len(), 2);
}

#[test]
fn full_self_feedback_is_a_single_process_scc() {
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "self",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<i64>("in").unwrap()],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(|| Burst { remaining: 0 }),
        )
        .unwrap();
    b.connect(
        p.wire_output::<i64>("out").unwrap(),
        p.wire_input::<i64>("in").unwrap(),
        one(),
    )
    .unwrap();
    b.expose_input("seed", p.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    let mut s = b.build().unwrap().compile().unwrap().start();
    s.inject("seed", Value::of(1_i64)).unwrap();
    assert_eq!(s.run_until_idle(100).state, SessionState::Waiting);
    let cycles = s.blocked_cycles();
    assert_eq!(cycles.len(), 1);
    assert_eq!(cycles[0].processes, vec![0]);
    assert_eq!(cycles[0].queues.len(), 1);
}

struct OmitRoot;
impl Process for OmitRoot {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        cx.take("in").unwrap();
        Ok(Step::Complete)
    }
}
#[test]
fn request_optional_cycle_does_not_invent_mutual_absence_proofs() {
    use phenix_graph::runtime::{Readiness, RequestStatus};
    let mut b = NetworkBuilder::new();
    let iface = Arc::new(
        Interface::new(
            [PortDef::wire::<i64>("in").unwrap()],
            [PortDef::wire::<i64>("out").unwrap()],
        )
        .unwrap(),
    );
    let root = b
        .node("root", iface.clone(), Arc::new(|| OmitRoot))
        .unwrap();
    let a = b.node("a", iface.clone(), Arc::new(|| Repeat)).unwrap();
    let c = b.node("b", iface, Arc::new(|| Repeat)).unwrap();
    b.expose_input("in", root.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    b.connect(
        root.wire_output::<i64>("out").unwrap(),
        a.wire_input::<i64>("in").unwrap(),
        one(),
    )
    .unwrap();
    b.connect(
        a.wire_output::<i64>("out").unwrap(),
        c.wire_input::<i64>("in").unwrap(),
        one(),
    )
    .unwrap();
    b.connect(
        c.wire_output::<i64>("out").unwrap(),
        a.wire_input::<i64>("in").unwrap(),
        one(),
    )
    .unwrap();
    let mut opts = SessionOptions::default();
    for name in ["a", "b"] {
        opts.readiness.insert(
            name.into(),
            Readiness::Optional {
                required: vec![],
                optional: vec!["in".into()],
            },
        );
    }
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    let id = s.begin_request(None).unwrap();
    s.inject_request(id, "in", Value::of(1_i64)).unwrap();
    s.close_request(id).unwrap();
    let report = s.run_until_idle(30);
    assert_eq!(report.steps, 1);
    assert_eq!(report.state, SessionState::Waiting);
    assert_eq!(s.request_work(id).unwrap().closure_pending, 2);
    assert_eq!(s.request_status(id).unwrap(), RequestStatus::Pending);
    assert!(s.blocked_cycles().is_empty());
    assert_eq!(s.run_until_idle(30).steps, 0);
}
