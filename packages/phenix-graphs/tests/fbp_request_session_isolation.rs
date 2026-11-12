//! M8: isolate correlated requests with existing bounded FBP sessions.
//! A shared-session FIFO join intentionally remains head-only; this test does
//! not claim to repair crossed-head blocking within *one* session.
use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::{CompiledNetwork, NetworkBuilder},
    process::{Process, ProcessContext, ProcessEmission, Step},
    runtime::{NetworkSession, Readiness, RequestError, RequestStatus, SessionOptions},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

struct Join {
    emitted: bool,
    shared_effects: Arc<AtomicUsize>,
}
impl Process for Join {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let a = cx.take("a").ok_or("join missing a")?.get::<i64>()?;
        let b = cx.take("b").ok_or("join missing b")?.get::<i64>()?;
        self.shared_effects.fetch_add(1, Ordering::SeqCst);
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new("out", Value::of(a + b))))
    }
}
fn compiled_join(shared_effects: Arc<AtomicUsize>, capacity: NonZeroUsize) -> CompiledNetwork {
    let mut builder = NetworkBuilder::new();
    let joined = builder
        .node(
            "join",
            Arc::new(
                Interface::new(
                    [
                        PortDef::wire::<i64>("a").unwrap(),
                        PortDef::wire::<i64>("b").unwrap(),
                    ],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(move || Join {
                emitted: false,
                shared_effects: shared_effects.clone(),
            }),
        )
        .unwrap();
    for port in ["a", "b"] {
        builder
            .expose_input(port, joined.wire_input::<i64>(port).unwrap(), capacity)
            .unwrap();
    }
    builder
        .expose_output("out", joined.wire_output::<i64>("out").unwrap())
        .unwrap();
    builder.build().unwrap().compile().unwrap()
}

// The host owns this bound. There is no second scheduler, correlation registry,
// transport, or new API inside phenix-graph.
fn start_bounded(
    compiled: &CompiledNetwork,
    active: &mut Vec<NetworkSession>,
    maximum: usize,
) -> Option<usize> {
    if active.len() >= maximum {
        return None;
    }
    let mut options = SessionOptions {
        capture_capacity: one(),
        ..Default::default()
    };
    options.readiness.insert("join".into(), Readiness::All);
    active.push(compiled.start_with_options(options));
    Some(active.len() - 1)
}

#[test]
fn independent_bounded_sessions_settle_crossed_request_arrivals_without_skipping_fifo() {
    // Independent process instances can still share explicitly supplied state.
    // This does not grant implicit cross-session access or relax queue FIFO.
    let shared_effects = Arc::new(AtomicUsize::new(0));
    let compiled = compiled_join(shared_effects.clone(), one());
    let mut active = Vec::new();
    let a = start_bounded(&compiled, &mut active, 2).unwrap();
    let b = start_bounded(&compiled, &mut active, 2).unwrap();
    assert!(start_bounded(&compiled, &mut active, 2).is_none());

    let ra = active[a].begin_request(Some("out")).unwrap();
    let rb = active[b].begin_request(Some("out")).unwrap();
    // Request identities are bound to the owning session even if a caller
    // knows another session's opaque ID.
    let foreign = active[a]
        .inject_request(rb, "a", Value::of(99_i64))
        .unwrap_err();
    assert_eq!(foreign.reason, RequestError::Foreign);
    assert_eq!(foreign.value.get::<i64>().unwrap(), 99);

    // The arrival order *across sessions* is crossed, yet each owned FIFO
    // only contains its own request packets. No packet skips a FIFO head.
    active[a].inject_request(ra, "a", Value::of(1_i64)).unwrap();
    active[b]
        .inject_request(rb, "b", Value::of(20_i64))
        .unwrap();
    active[b]
        .inject_request(rb, "a", Value::of(10_i64))
        .unwrap();
    active[a].inject_request(ra, "b", Value::of(2_i64)).unwrap();
    active[a].close_request(ra).unwrap();
    active[b].close_request(rb).unwrap();

    for session in &mut active {
        let report = session.run_until_idle(64);
        assert!(!report.budget_exhausted);
        assert!(session.failure().is_none());
    }
    let first = active[a].take_request_output(ra, "out").unwrap().unwrap();
    assert_eq!(first.value().get::<i64>().unwrap(), 3);
    let second = active[b].take_request_output(rb, "out").unwrap().unwrap();
    assert_eq!(second.value().get::<i64>().unwrap(), 30);
    assert_eq!(
        active[a].request_status(ra).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    assert_eq!(
        active[b].request_status(rb).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    assert_eq!(shared_effects.load(Ordering::SeqCst), 2);

    active[a].release_request(ra).unwrap();
    active[b].release_request(rb).unwrap();
    active.remove(b);
    assert!(start_bounded(&compiled, &mut active, 2).is_some());
}

#[test]
fn correlated_shared_session_keeps_crossed_request_pairs_isolated() {
    // CorrelatedAll permits opt-in scoped selection past an unrelated FIFO
    // head. Captures remain a single strict FIFO, so the host drains both.
    let effects = Arc::new(AtomicUsize::new(0));
    let compiled = compiled_join(effects.clone(), NonZeroUsize::new(2).unwrap());
    let mut options = SessionOptions {
        capture_capacity: NonZeroUsize::new(2).unwrap(),
        ..Default::default()
    };
    options
        .readiness
        .insert("join".into(), Readiness::CorrelatedAll);
    let mut session = compiled.start_with_options(options);
    let first = session.begin_request(Some("out")).unwrap();
    let second = session.begin_request(Some("out")).unwrap();
    session
        .inject_request(first, "a", Value::of(1_i64))
        .unwrap();
    session
        .inject_request(second, "b", Value::of(20_i64))
        .unwrap();
    session
        .inject_request(second, "a", Value::of(10_i64))
        .unwrap();
    session
        .inject_request(first, "b", Value::of(2_i64))
        .unwrap();
    session.close_request(first).unwrap();
    session.close_request(second).unwrap();

    let report = session.run_until_idle(128);
    assert!(!report.budget_exhausted);
    assert!(session.failure().is_none());
    let mut outputs = Vec::new();
    for _ in 0..2 {
        let packet = session.take_output("out").unwrap().unwrap();
        let request = packet.metadata().unwrap().request().unwrap();
        session.validate_capture(request, "out", &packet).unwrap();
        outputs.push((request.sequence(), packet.value().get::<i64>().unwrap()));
    }
    outputs.sort_unstable_by_key(|(id, _)| *id);
    assert_eq!(outputs, [(first.sequence(), 3), (second.sequence(), 30)]);
    assert_eq!(effects.load(Ordering::SeqCst), 2);
    for request in [first, second] {
        assert_eq!(
            session.request_status(request).unwrap(),
            RequestStatus::Settled { captures: 1 }
        );
        session.release_request(request).unwrap();
    }
}

#[test]
fn cancelling_queued_shared_request_preserves_other_correlated_work() {
    let effects = Arc::new(AtomicUsize::new(0));
    let compiled = compiled_join(effects.clone(), NonZeroUsize::new(2).unwrap());
    let mut options = SessionOptions {
        capture_capacity: NonZeroUsize::new(2).unwrap(),
        ..Default::default()
    };
    options
        .readiness
        .insert("join".into(), Readiness::CorrelatedAll);
    let mut session = compiled.start_with_options(options);
    let cancelled = session.begin_request(Some("out")).unwrap();
    let survivor = session.begin_request(Some("out")).unwrap();
    session
        .inject_request(cancelled, "a", Value::of(1_i64))
        .unwrap();
    session
        .inject_request(survivor, "b", Value::of(20_i64))
        .unwrap();
    session
        .inject_request(survivor, "a", Value::of(10_i64))
        .unwrap();

    let tickets = session.cancel_request(cancelled).unwrap();
    assert!(tickets.is_empty());
    assert_eq!(
        session.request_status(cancelled).unwrap(),
        RequestStatus::Cancelled
    );
    assert_eq!(
        session.request_disposal(cancelled).unwrap().queued_packets,
        1
    );
    session.close_request(survivor).unwrap();
    let report = session.run_until_idle(128);
    assert!(!report.budget_exhausted);
    assert!(session.failure().is_none());
    let packet = session
        .take_request_output(survivor, "out")
        .unwrap()
        .unwrap();
    assert_eq!(packet.value().get::<i64>().unwrap(), 30);
    assert_eq!(packet.metadata().unwrap().request(), Some(survivor));
    assert_eq!(
        session.request_status(survivor).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    session.release_request(cancelled).unwrap();
    session.release_request(survivor).unwrap();
}
