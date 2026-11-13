//! Optional readiness polls only registered sealed request-local closure paths.
use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::{Process, ProcessContext, Step},
    runtime::{Readiness, RequestStatus, SessionOptions, SessionState},
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

struct RootDrop;
impl Process for RootDrop {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        cx.take("in").ok_or("missing request root packet")?;
        Ok(Step::Complete)
    }
}

struct ObserveAbsent(Arc<AtomicUsize>);
impl Process for ObserveAbsent {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if cx.take("opt").is_some() || !cx.is_request_closed("opt") || cx.is_eos("opt") {
            return Err("expected request-local absent input, not packet or global EOS".into());
        }
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Step::Complete)
    }
}

#[test]
fn sealed_optional_watch_is_idempotent_and_isolated_across_requests() {
    let observed = Arc::new(AtomicUsize::new(0));
    let unrelated = Arc::new(AtomicUsize::new(0));
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
            Arc::new(|| RootDrop),
        )
        .unwrap();
    let optional_iface =
        Arc::new(Interface::new([PortDef::wire::<u64>("opt").unwrap()], []).unwrap());
    let sink = builder
        .node("optional", optional_iface.clone(), {
            let observed = observed.clone();
            Arc::new(move || ObserveAbsent(observed.clone()))
        })
        .unwrap();
    let unrelated_node = builder
        .node("unrelated", optional_iface, {
            let unrelated = unrelated.clone();
            Arc::new(move || ObserveAbsent(unrelated.clone()))
        })
        .unwrap();
    builder
        .expose_input("trigger", root.wire_input::<u64>("in").unwrap(), one())
        .unwrap();
    builder
        .expose_input(
            "unrelated_in",
            unrelated_node.wire_input::<u64>("opt").unwrap(),
            one(),
        )
        .unwrap();
    builder
        .connect(
            root.wire_output::<u64>("out").unwrap(),
            sink.wire_input::<u64>("opt").unwrap(),
            one(),
        )
        .unwrap();
    let mut options = SessionOptions::default();
    for name in ["optional", "unrelated"] {
        options.readiness.insert(
            name.into(),
            Readiness::Optional {
                required: vec![],
                optional: vec!["opt".into()],
            },
        );
    }
    let mut session = builder
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(options);

    // Both sealed scopes reach only "optional". Repeated sealing of one
    // request must not register two watches and leak one after release.
    let first = session.begin_request(None).unwrap();
    session
        .inject_request(first, "trigger", Value::of(1_u64))
        .unwrap();
    session.close_request(first).unwrap();
    session.close_request(first).unwrap();

    let second = session.begin_request(None).unwrap();
    session
        .inject_request(second, "trigger", Value::of(2_u64))
        .unwrap();
    session.close_request(second).unwrap();

    let report = session.run_until_idle(32);
    assert!(!report.budget_exhausted);
    assert_eq!(report.steps, 4);
    assert_eq!(observed.load(Ordering::SeqCst), 2);
    assert_eq!(unrelated.load(Ordering::SeqCst), 0);
    for id in [first, second] {
        assert_eq!(
            session.request_status(id).unwrap(),
            RequestStatus::Settled { captures: 0 }
        );
    }
    session.release_request(first).unwrap();
    // The second scope still holds its registration. Cancelling a settled
    // scope must clear its watch before wiping request reachability.
    session.cancel_request(second).unwrap();
    session.release_request(second).unwrap();

    let third = session.begin_request(None).unwrap();
    session
        .inject_request(third, "trigger", Value::of(3_u64))
        .unwrap();
    session.close_request(third).unwrap();
    assert_eq!(session.run_until_idle(32).steps, 2);
    assert_eq!(observed.load(Ordering::SeqCst), 3);
    assert_eq!(unrelated.load(Ordering::SeqCst), 0);
    assert_eq!(
        session.request_status(third).unwrap(),
        RequestStatus::Settled { captures: 0 }
    );
    session.release_request(third).unwrap();
    assert_eq!(session.state(), SessionState::Quiescent);
}

struct EmitRequired(bool);
impl Process for EmitRequired {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.0 {
            self.0 = false;
            return Ok(Step::Complete);
        }
        let value = cx.take("in").ok_or("missing required source")?;
        self.0 = true;
        Ok(Step::Emit(phenix_graph::process::ProcessEmission::new(
            "out", value,
        )))
    }
}

struct RequiredAndAbsent(Arc<AtomicUsize>);
impl Process for RequiredAndAbsent {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if cx.take("req").is_none() || cx.take("opt").is_some() || !cx.is_request_closed("opt") {
            return Err("required data and request-local absent optional must be paired".into());
        }
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Step::Complete)
    }
}

#[test]
fn required_packet_waits_for_optional_request_closure_without_any_new_queue_event() {
    let hits = Arc::new(AtomicUsize::new(0));
    let mut builder = NetworkBuilder::new();
    let root_interface = Arc::new(
        Interface::new(
            [PortDef::wire::<u64>("in").unwrap()],
            [PortDef::wire::<u64>("out").unwrap()],
        )
        .unwrap(),
    );
    let required = builder
        .node("required_source", root_interface.clone(), Arc::new(|| EmitRequired(false)))
        .unwrap();
    let silent = builder
        .node("silent_source", root_interface, Arc::new(|| RootDrop))
        .unwrap();
    let join = builder
        .node(
            "join",
            Arc::new(
                Interface::new(
                    [
                        PortDef::wire::<u64>("req").unwrap(),
                        PortDef::wire::<u64>("opt").unwrap(),
                    ],
                    [],
                )
                .unwrap(),
            ),
            {
                let hits = hits.clone();
                Arc::new(move || RequiredAndAbsent(hits.clone()))
            },
        )
        .unwrap();
    builder
        .expose_input("required_in", required.wire_input::<u64>("in").unwrap(), one())
        .unwrap();
    builder
        .expose_input("silent_in", silent.wire_input::<u64>("in").unwrap(), one())
        .unwrap();
    builder
        .connect(
            required.wire_output::<u64>("out").unwrap(),
            join.wire_input::<u64>("req").unwrap(),
            one(),
        )
        .unwrap();
    builder
        .connect(
            silent.wire_output::<u64>("out").unwrap(),
            join.wire_input::<u64>("opt").unwrap(),
            one(),
        )
        .unwrap();
    let mut options = SessionOptions::default();
    options.readiness.insert(
        "join".into(),
        Readiness::Optional {
            required: vec!["req".into()],
            optional: vec!["opt".into()],
        },
    );
    let mut session = builder
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(options);
    for n in 0..3_u64 {
        let request = session.begin_request(None).unwrap();
        session
            .inject_request(request, "required_in", Value::of(n))
            .unwrap();
        session
            .inject_request(request, "silent_in", Value::of(n))
            .unwrap();
        session.close_request(request).unwrap();
        let report = session.run_until_idle(32);
        assert_eq!(report.steps, 4);
        assert!(!report.budget_exhausted);
        assert_eq!(
            session.request_status(request).unwrap(),
            RequestStatus::Settled { captures: 0 }
        );
        session.release_request(request).unwrap();
    }
    assert_eq!(hits.load(Ordering::SeqCst), 3);
    assert_eq!(session.state(), SessionState::Quiescent);
}
