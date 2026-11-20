use phenix_graph::{
    CompiledNetwork, Interface, NetworkBuilder, PortDef, Process, ProcessContext, ProcessEmission,
    ProcessFactory, ResourceError, ResourceGrant, SessionOptions, Step, ValidatedResourceSet,
    Value,
    process::{HandoffCancelError, HandoffState, ProcessHandoff},
    runtime::{
        Readiness, ReadinessPolicy, ReadinessSnapshot, RequestFailurePolicy, RequestStatus,
        SessionState,
    },
};
use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
};

fn resources(grants: Vec<ResourceGrant>) -> ValidatedResourceSet {
    ValidatedResourceSet::try_new(grants).unwrap()
}

struct Counter(Arc<AtomicUsize>);
impl Counter {
    fn increment(&self) -> usize {
        self.0.fetch_add(1, Ordering::SeqCst)
    }
}

#[test]
fn lookup_is_exact_borrowed_and_never_invokes_capability_operations() {
    let calls = Arc::new(AtomicUsize::new(0));
    let shared = Arc::new(Counter(calls.clone()));
    let set = resources(vec![
        ResourceGrant::new("plain", Counter(calls.clone())),
        ResourceGrant::new("arc", shared.clone()),
    ]);
    let context = ProcessContext::default().with_resources(set.clone());
    let clone = ProcessContext::default().with_resources(set);
    assert!(matches!(
        context.resource::<Counter>("absent"),
        Err(ResourceError::NotGranted("absent"))
    ));
    assert!(matches!(
        context.resource::<String>("plain"),
        Err(ResourceError::WrongType("plain"))
    ));
    assert!(matches!(
        context.resource::<Arc<Counter>>("plain"),
        Err(ResourceError::WrongType("plain"))
    ));
    assert!(matches!(
        context.resource::<Counter>("arc"),
        Err(ResourceError::WrongType("arc"))
    ));
    assert!(Arc::ptr_eq(
        context.resource::<Arc<Counter>>("arc").unwrap(),
        &shared
    ));
    assert!(std::ptr::eq(
        context.resource::<Counter>("plain").unwrap(),
        clone.resource::<Counter>("plain").unwrap()
    ));
    for _ in 0..3 {
        let _ = context.resource::<Counter>("plain").unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(context.resource::<Counter>("plain").unwrap().increment(), 0);
    assert_eq!(shared.increment(), 1);
    assert!(matches!(
        ProcessContext::default().resource::<Counter>("plain"),
        Err(ResourceError::NotGranted("plain"))
    ));
}

struct Complete;
impl Process for Complete {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        Ok(Step::Complete)
    }
}

struct CountingFactory(Arc<AtomicUsize>);
impl ProcessFactory for CountingFactory {
    fn spawn(&self) -> Box<dyn Process> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::new(Complete)
    }
    fn correlation_lanes(&self) -> Option<NonZeroUsize> {
        self.0.fetch_add(1, Ordering::SeqCst);
        None
    }
    fn readiness(&self) -> Readiness {
        self.0.fetch_add(1, Ordering::SeqCst);
        Readiness::Custom(Arc::new(CountingPolicy(self.0.clone())))
    }
}
struct CountingPolicy(Arc<AtomicUsize>);
impl ReadinessPolicy for CountingPolicy {
    fn select(&self, _: &ReadinessSnapshot) -> Vec<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        vec![]
    }
    fn notification_driven(&self) -> bool {
        self.0.fetch_add(1, Ordering::SeqCst);
        false
    }
}

#[test]
fn duplicates_of_same_or_different_types_reject_in_both_orders_before_callbacks() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = NetworkBuilder::new();
    let source = builder
        .node(
            "source",
            Arc::new(Interface::default()),
            Arc::new(CountingFactory(calls.clone())),
        )
        .unwrap();
    builder.start(&source).unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for same_type in [false, true] {
        for reverse in [false, true] {
            let mut grants = vec![
                ResourceGrant::new("duplicate", Counter(calls.clone())),
                if same_type {
                    ResourceGrant::new("duplicate", Counter(calls.clone()))
                } else {
                    ResourceGrant::new("duplicate", 42_u64)
                },
            ];
            if reverse {
                grants.reverse();
            }
            let start = || -> Result<_, ResourceError> {
                let set = ValidatedResourceSet::try_new(grants)?;
                Ok(compiled.start_with_options(SessionOptions {
                    resources: BTreeMap::from([("source".into(), set)]),
                    ..Default::default()
                }))
            };
            assert!(matches!(
                start(),
                Err(ResourceError::Duplicate("duplicate"))
            ));
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        }
    }
}

struct Source {
    emitted: bool,
}
impl Process for Source {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let counter = context
            .resource::<Arc<Counter>>("counter")
            .map_err(|e| phenix_graph::Failure::external("resource access failed", e))?;
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new(
            "out",
            Value::local(counter.increment()),
        )))
    }
}
fn sources() -> CompiledNetwork {
    let mut builder = NetworkBuilder::new();
    for name in ["a", "b"] {
        let source = builder
            .node(
                name,
                Arc::new(Interface::new([], [PortDef::local::<usize>("out")]).unwrap()),
                Arc::new(|| Source { emitted: false }),
            )
            .unwrap();
        builder.start(&source).unwrap();
        builder
            .expose_output(name, source.output::<usize>("out").unwrap())
            .unwrap();
    }
    builder.build().unwrap().compile().unwrap()
}

#[test]
fn process_and_session_identity_is_explicit_for_auto_and_request_sources() {
    let compiled = sources();
    let a = Arc::new(Counter(Arc::new(AtomicUsize::new(10))));
    let b = Arc::new(Counter(Arc::new(AtomicUsize::new(20))));
    let options = SessionOptions {
        resources: BTreeMap::from([
            (
                "a".into(),
                resources(vec![ResourceGrant::new("counter", a.clone())]),
            ),
            (
                "b".into(),
                resources(vec![ResourceGrant::new("counter", b.clone())]),
            ),
        ]),
        ..Default::default()
    };
    let mut auto = compiled.start_with_options(options.clone());
    auto.run_until_idle(16);
    for (name, expected) in [("a", 10), ("b", 20)] {
        let packet = auto.take_output(name).unwrap().unwrap();
        assert_eq!(packet.value().borrow::<usize>(), Some(&expected));
    }
    let mut scoped_options = options;
    scoped_options.auto_start_sources = false;
    let mut scoped = compiled.start_with_options(scoped_options.clone());
    assert_eq!(scoped.run_until_idle(16).steps, 0);
    for (name, expected) in [("a", 11), ("b", 21)] {
        let id = scoped.begin_request(Some(name)).unwrap();
        scoped.start_request_source(id, name).unwrap();
        scoped.close_request(id).unwrap();
        scoped.run_until_idle(16);
        let packet = scoped.take_request_output(id, name).unwrap().unwrap();
        assert_eq!(packet.value().borrow::<usize>(), Some(&expected));
        assert_eq!(
            scoped.request_status(id).unwrap(),
            RequestStatus::Settled { captures: 1 }
        );
        scoped.release_request(id).unwrap();
    }
    // Same key and same type do not infer sharing with another process/session.
    let independent = Arc::new(Counter(Arc::new(AtomicUsize::new(100))));
    scoped_options.resources.insert(
        "b".into(),
        resources(vec![ResourceGrant::new("counter", independent.clone())]),
    );
    let mut other = compiled.start_with_options(scoped_options);
    let id = other.begin_request(Some("b")).unwrap();
    other.start_request_source(id, "b").unwrap();
    other.close_request(id).unwrap();
    other.run_until_idle(16);
    assert_eq!(
        other
            .take_request_output(id, "b")
            .unwrap()
            .unwrap()
            .value()
            .borrow::<usize>(),
        Some(&100)
    );
    assert_eq!(a.0.load(Ordering::SeqCst), 12);
    assert_eq!(b.0.load(Ordering::SeqCst), 22);
    assert_eq!(independent.0.load(Ordering::SeqCst), 101);
    let mut denied = compiled.start();
    assert_eq!(denied.run_until_idle(16).state, SessionState::Failed);
}

struct Continuation {
    stage: u8,
}
impl Process for Continuation {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        let counter = context
            .resource::<Arc<Counter>>("counter")
            .map_err(|e| phenix_graph::Failure::external("resource access failed", e))?;
        counter.increment();
        let step = match self.stage {
            0 => {
                context.take("in").ok_or_else(|| {
                    phenix_graph::Failure::application_message("initial input absent")
                })?;
                Step::Yield
            }
            1 => Step::WaitInput("in".into()),
            2 => {
                context.take("in").ok_or_else(|| {
                    phenix_graph::Failure::application_message("staged input absent")
                })?;
                Step::Complete
            }
            _ => unreachable!(),
        };
        self.stage = (self.stage + 1) % 3;
        Ok(step)
    }
}

#[test]
fn rejection_continuations_reservations_and_recovery_keep_external_state() {
    let spawns = Arc::new(AtomicUsize::new(0));
    let mut builder = NetworkBuilder::new();
    let node = builder
        .node(
            "worker",
            Arc::new(Interface::new([PortDef::local::<usize>("in")], []).unwrap()),
            Arc::new({
                let spawns = spawns.clone();
                move || {
                    spawns.fetch_add(1, Ordering::SeqCst);
                    Continuation { stage: 0 }
                }
            }),
        )
        .unwrap();
    builder
        .expose_input(
            "in",
            node.input::<usize>("in").unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    let counter = Arc::new(Counter(Arc::new(AtomicUsize::new(0))));
    let mut session = compiled.start_with_options(SessionOptions {
        resources: BTreeMap::from([(
            "worker".into(),
            resources(vec![ResourceGrant::new("counter", counter.clone())]),
        )]),
        request_failure_policy: RequestFailurePolicy::RestartAffectedProcess,
        ..Default::default()
    });
    let first = session.begin_request(None).unwrap();
    session.inject_request_local(first, "in", 1_usize).unwrap();
    let (index, job) = session.next_job().unwrap();
    let ticket = job.ticket();
    session.reject(index, job).unwrap();
    assert_eq!(counter.0.load(Ordering::SeqCst), 0);
    let (_, job) = session.next_job().unwrap();
    assert_ne!(job.ticket(), ticket);
    session.complete_job(job.run()).unwrap();
    session.run_until_idle(8);
    assert_eq!(counter.0.load(Ordering::SeqCst), 2);
    session.inject_request_local(first, "in", 2_usize).unwrap();
    session.close_request(first).unwrap();
    session.run_until_idle(8);
    assert_eq!(counter.0.load(Ordering::SeqCst), 3);
    assert_eq!(
        session.request_status(first).unwrap(),
        RequestStatus::Settled { captures: 0 }
    );
    session.release_request(first).unwrap();

    let cancelled = session.begin_request(None).unwrap();
    session
        .inject_request_local(cancelled, "in", 3_usize)
        .unwrap();
    session.run_until_idle(1);
    assert_eq!(counter.0.load(Ordering::SeqCst), 4);
    session.cancel_request(cancelled).unwrap();
    session.release_request(cancelled).unwrap();
    assert_eq!(spawns.load(Ordering::SeqCst), 2);
    assert_eq!(counter.0.load(Ordering::SeqCst), 4);
    let recovered = session.begin_request(None).unwrap();
    session
        .inject_request_local(recovered, "in", 4_usize)
        .unwrap();
    session.run_until_idle(8);
    assert_eq!(counter.0.load(Ordering::SeqCst), 6);
    session
        .inject_request_local(recovered, "in", 5_usize)
        .unwrap();
    session.close_request(recovered).unwrap();
    session.run_until_idle(8);
    assert_eq!(counter.0.load(Ordering::SeqCst), 7);
    assert_eq!(
        session.request_status(recovered).unwrap(),
        RequestStatus::Settled { captures: 0 }
    );
}

type Events = Arc<Mutex<Vec<&'static str>>>;
struct Lease {
    name: &'static str,
    events: Events,
    panic: bool,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.events.lock().unwrap().push(self.name);
        assert!(!self.panic, "hostile lease");
    }
}
struct DropProbe {
    name: &'static str,
    leases: [Weak<Lease>; 2],
    events: Events,
    panic: bool,
}
impl DropProbe {
    fn record(&self, name: &'static str) {
        for lease in &self.leases {
            assert!(lease.strong_count() > 0, "{name} lost capability lease");
        }
        self.events.lock().unwrap().push(name);
    }
}
struct PanicPayload {
    leases: [Weak<Lease>; 2],
    events: Events,
}
impl Drop for PanicPayload {
    fn drop(&mut self) {
        for lease in &self.leases {
            assert!(
                lease.strong_count() > 0,
                "panic payload lost capability lease"
            );
        }
        self.events.lock().unwrap().push("panic-payload");
        panic!("hostile panic payload");
    }
}
impl Drop for DropProbe {
    fn drop(&mut self) {
        self.record(self.name);
        if self.panic {
            std::panic::panic_any(PanicPayload {
                leases: self.leases.clone(),
                events: self.events.clone(),
            });
        }
    }
}
fn probe_parts(
    name: &'static str,
    events: &Events,
    panic: bool,
    lease_panic: bool,
) -> (DropProbe, [Arc<Lease>; 2]) {
    let leases = ["lease-first", "lease-last"].map(|name| {
        Arc::new(Lease {
            name,
            events: events.clone(),
            panic: lease_panic,
        })
    });
    (
        DropProbe {
            name,
            leases: leases.each_ref().map(Arc::downgrade),
            events: events.clone(),
            panic,
        },
        leases,
    )
}
fn hostile_grant(
    name: &'static str,
    events: &Events,
    panic: bool,
    lease_panic: bool,
) -> ResourceGrant {
    let (probe, [first, last]) = probe_parts(name, events, panic, lease_panic);
    ResourceGrant::new(name, probe)
        .with_lease(first)
        .with_lease(last)
}
fn assert_cleanup(events: &Events, names: &[&'static str], panic: bool) {
    let expected: Vec<_> = names
        .iter()
        .flat_map(|name| {
            let mut events = vec![*name];
            if panic {
                events.push("panic-payload");
            }
            events.extend(["lease-last", "lease-first"]);
            events
        })
        .collect();
    assert_eq!(*events.lock().unwrap(), expected);
}

#[test]
fn direct_grant_drop_keeps_all_capability_leases_through_panic_payload_cleanup() {
    for panic in [false, true] {
        for lease_panic in [false, true] {
            let events = Events::default();
            drop(hostile_grant("direct", &events, panic, lease_panic));
            assert_cleanup(&events, &["direct"], panic);
        }
    }
    let events = Events::default();
    let result = std::panic::catch_unwind({
        let events = events.clone();
        move || {
            let _grant = hostile_grant("unwinding", &events, true, true);
            panic!("outer unwind");
        }
    });
    assert!(result.is_err());
    assert_cleanup(&events, &["unwinding"], true);
}

#[test]
fn duplicate_rejection_safely_disposes_every_hostile_grant_in_both_orders() {
    struct Different(DropProbe);
    for reverse in [false, true] {
        let events = Events::default();
        let (left, [first, last]) = probe_parts("left", &events, true, true);
        let mut grants = vec![
            ResourceGrant::new("duplicate", left)
                .with_lease(first)
                .with_lease(last),
        ];
        let (right, [first, last]) = probe_parts("right", &events, true, true);
        let different = Different(right);
        let _ = &different.0;
        grants.push(
            ResourceGrant::new("duplicate", different)
                .with_lease(first)
                .with_lease(last),
        );
        if reverse {
            grants.reverse();
        }
        assert_eq!(
            ValidatedResourceSet::try_new(grants).unwrap_err(),
            ResourceError::Duplicate("duplicate")
        );
        assert_cleanup(
            &events,
            if reverse {
                &["right", "left"]
            } else {
                &["left", "right"]
            },
            true,
        );
    }
}

#[test]
fn managed_completion_and_cancellation_report_actual_resource_cleanup_panics() {
    for cancel in [false, true] {
        for (payload_panic, lease_panic) in [(true, false), (false, true), (true, true)] {
            let events = Events::default();
            let context = ProcessContext::default().with_resources(resources(vec![
                hostile_grant("left", &events, payload_panic, lease_panic),
                hostile_grant("right", &events, payload_panic, lease_panic),
            ]));
            let mut handoff = ProcessHandoff::new(Box::new(Complete));
            handoff.admit(context).unwrap();
            if cancel {
                assert_eq!(
                    handoff.cancel(),
                    Err(HandoffCancelError::DestructorPanicked { ticket: None })
                );
                assert_cleanup(&events, &["left", "right"], payload_panic);
            } else {
                let job = handoff.next_job().unwrap();
                let error = handoff.complete(job.run()).unwrap().unwrap_err();
                assert!(error.summary().contains("owned destructor panicked"));
                assert_eq!(handoff.state(), HandoffState::Failed);
                if payload_panic {
                    assert_eq!(
                        *events.lock().unwrap(),
                        ["left", "panic-payload", "right", "panic-payload"]
                    );
                }
                drop(error);
                // Completion's escaped Failure owns all known resource code
                // leases, not the capabilities or the ProcessContext itself.
                let mut expected = vec!["left"];
                if payload_panic {
                    expected.push("panic-payload");
                }
                expected.push("right");
                if payload_panic {
                    expected.push("panic-payload");
                }
                expected.extend(["lease-last", "lease-first", "lease-last", "lease-first"]);
                assert_eq!(*events.lock().unwrap(), expected);
            }
        }
    }
}

#[test]
fn retained_runtime_set_and_external_set_final_owner_drop_are_individually_safe() {
    let mut builder = NetworkBuilder::new();
    let source = builder
        .node(
            "source",
            Arc::new(Interface::default()),
            Arc::new(|| Complete),
        )
        .unwrap();
    builder.start(&source).unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    for auto_start_sources in [false, true] {
        for retain_external in [false, true] {
            let events = Events::default();
            let set = resources(vec![
                hostile_grant("left", &events, true, true),
                hostile_grant("right", &events, true, true),
            ]);
            let external = retain_external.then(|| set.clone());
            let mut session = compiled.start_with_options(SessionOptions {
                auto_start_sources,
                resources: BTreeMap::from([("source".into(), set)]),
                ..Default::default()
            });
            session.run_until_idle(8);
            assert!(events.lock().unwrap().is_empty());
            drop(session);
            if retain_external {
                assert!(events.lock().unwrap().is_empty());
            }
            drop(external);
            assert_cleanup(&events, &["left", "right"], true);
        }
    }
}
