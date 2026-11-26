use phenix_graph::{
    CallbackPhase, Failure, InputKey, Interface, NetworkBuilder, PortDef, Process, ProcessContext,
    SessionOptions, Step, Value,
    connection::{ConnectionQueue, ConnectionSet, EdgeAdapter, FanoutTarget, Packet},
    process::{ProcessExecutionGuard, ProcessHandoff},
    runtime::{RequestFailurePolicy, RequestQueuePolicy, RequestStatus},
};
use std::{
    error::Error,
    fmt,
    num::NonZeroUsize,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

struct Cause {
    leases: Vec<Weak<()>>,
    calls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
impl fmt::Display for Cause {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.calls.fetch_add(1, Ordering::SeqCst);
        panic!("external display must not run")
    }
}
impl fmt::Debug for Cause {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.calls.fetch_add(1, Ordering::SeqCst);
        panic!("external debug must not run")
    }
}
impl Error for Cause {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        panic!("external source must not run")
    }
}
impl Drop for Cause {
    fn drop(&mut self) {
        assert!(self.leases.iter().all(|lease| lease.upgrade().is_some()));
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

struct Fails {
    leases: Vec<Weak<()>>,
    calls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
    address: Arc<AtomicUsize>,
}
impl Process for Fails {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, Failure> {
        drop(context.take("in"));
        let failure = Failure::external(
            "explicit host summary",
            Cause {
                leases: self.leases.clone(),
                calls: self.calls.clone(),
                drops: self.drops.clone(),
            },
        );
        self.address.store(
            failure.source().unwrap().downcast_ref::<Cause>().unwrap() as *const Cause as usize,
            Ordering::SeqCst,
        );
        Err(failure)
    }
}

#[test]
fn worker_request_session_preserve_non_clone_cause_and_actual_input_leases_after_all_owners_drop() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for policy in [
            RequestFailurePolicy::FailSession,
            RequestFailurePolicy::RestartAffectedProcess,
        ] {
            let code = Arc::new(());
            let input = Arc::new(());
            let resource = Arc::new(());
            let weak = [
                Arc::downgrade(&code),
                Arc::downgrade(&input),
                Arc::downgrade(&resource),
            ];
            let calls = Arc::new(AtomicUsize::new(0));
            let drops = Arc::new(AtomicUsize::new(0));
            let address = Arc::new(AtomicUsize::new(0));
            let factory_weak = weak.to_vec();
            let factory_calls = calls.clone();
            let factory_drops = drops.clone();
            let factory_address = address.clone();
            let mut graph = NetworkBuilder::new();
            let node = graph
                .node(
                    "worker",
                    Arc::new(Interface::new([PortDef::local::<()>("in")], []).unwrap()),
                    Arc::new(move || Fails {
                        leases: factory_weak.clone(),
                        calls: factory_calls.clone(),
                        drops: factory_drops.clone(),
                        address: factory_address.clone(),
                    }),
                )
                .unwrap();
            graph
                .expose_input(
                    "in",
                    node.input::<()>("in").unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            let compiled = graph.build().unwrap().compile().unwrap();
            let mut session = compiled.start_with_options(SessionOptions {
                lease: Some(code),
                resources: [(
                    "worker".into(),
                    phenix_graph::ValidatedResourceSet::try_new(vec![
                        phenix_graph::ResourceGrant::new("dependency", ()).with_lease(resource),
                    ])
                    .unwrap(),
                )]
                .into(),
                request_queue_policy: queue,
                request_failure_policy: policy,
                ..Default::default()
            });
            let request = session.begin_request(None).unwrap();
            session
                .inject_request(request, "in", Value::local(()).with_lease(input))
                .unwrap();
            session.close_request(request).unwrap();
            let (_, job) = session.next_job().unwrap();
            let ticket = job.ticket();
            let completion = std::thread::spawn(move || job.run()).join().unwrap();
            session.complete_job(completion).unwrap();
            assert_eq!(
                session.request_status(request).unwrap(),
                RequestStatus::Failed
            );
            let failure = session.request_failure(request).unwrap().unwrap().clone();
            assert_eq!(failure.context().ticket, Some(ticket));
            assert_eq!(failure.context().request, Some(request));
            assert_eq!(failure.context().process, Some(0));
            assert_eq!(failure.context().path.as_deref(), Some("worker"));
            let snapshot = session.explain_process(0, Instant::now()).unwrap();
            for _ in 0..4 {
                let _ = format!("{failure} {failure:?} {snapshot:#?}");
            }
            session.release_request(request).unwrap();
            drop(session);
            drop(compiled);
            let clone = failure.clone();
            assert!(std::ptr::eq(
                failure.source().unwrap(),
                clone.source().unwrap()
            ));
            assert_eq!(
                clone.source().unwrap().downcast_ref::<Cause>().unwrap() as *const Cause as usize,
                address.load(Ordering::SeqCst)
            );
            drop(failure);
            assert!(weak.iter().all(|lease| lease.upgrade().is_some()));
            assert_eq!(drops.load(Ordering::SeqCst), 0);
            drop(clone);
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            assert!(weak.iter().all(|lease| lease.upgrade().is_none()));
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            let _ = format!("{snapshot:#?}");
        }
    }
}

#[test]
fn consuming_helper_error_retains_the_independent_input_lease_without_its_envelope() {
    struct MoveOnly;
    let lease = Arc::new(());
    let weak = Arc::downgrade(&lease);
    let calls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let mut context = ProcessContext::new(
        [("in".into(), Value::local(MoveOnly).with_lease(lease))].into(),
        vec![],
    );
    let failure = InputKey::<MoveOnly>::new("in")
        .map_input(&mut context, "", |_, _| {
            Err(Failure::external(
                "helper callback failed",
                Cause {
                    leases: vec![weak.clone()],
                    calls: calls.clone(),
                    drops: drops.clone(),
                },
            ))
        })
        .unwrap_err();
    assert!(context.input("in").is_none());
    drop(context);
    assert_eq!(failure.context().field.as_deref(), Some("in"));
    assert!(weak.upgrade().is_some());
    drop(failure);
    assert!(weak.upgrade().is_none());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

struct Token {
    index: usize,
    events: Arc<Mutex<Vec<usize>>>,
}
impl Drop for Token {
    fn drop(&mut self) {
        self.events.lock().unwrap().push(self.index);
        panic!("token teardown");
    }
}
struct Guard {
    index: usize,
    events: Arc<Mutex<Vec<usize>>>,
    reject: bool,
}
impl ProcessExecutionGuard for Guard {
    fn enter(&self) -> Result<Box<dyn std::any::Any>, Failure> {
        if self.reject {
            return Err(Failure::application_message("entry primary"));
        }
        Ok(Box::new(Token {
            index: self.index,
            events: self.events.clone(),
        }))
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        self.events.lock().unwrap().push(self.index + 10);
        panic!("guard object teardown");
    }
}
struct Never(Arc<AtomicUsize>);
impl Process for Never {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(Failure::application_message("step primary"))
    }
}

#[test]
fn first_entry_or_step_failure_survives_all_reverse_token_and_guard_object_cleanup() {
    for reject in [true, false] {
        let calls = Arc::new(AtomicUsize::new(0));
        let events = Arc::new(Mutex::new(Vec::new()));
        let lease = Arc::new(());
        let weak = Arc::downgrade(&lease);
        let mut handoff = ProcessHandoff::new(Box::new(Never(calls.clone()))).with_lease(lease);
        handoff.admit(ProcessContext::default()).unwrap();
        let job = handoff
            .next_job()
            .unwrap()
            .with_guard(Arc::new(Guard {
                index: 0,
                events: events.clone(),
                reject: false,
            }))
            .with_guard(Arc::new(Guard {
                index: 1,
                events: events.clone(),
                reject,
            }));
        let completion = job.run();
        assert_eq!(
            *events.lock().unwrap(),
            if reject { vec![0] } else { vec![1, 0] }
        );
        let failure = handoff.complete(completion).unwrap().unwrap_err();
        assert_eq!(
            failure.summary(),
            if reject {
                "entry primary"
            } else {
                "step primary"
            }
        );
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(!reject));
        assert_eq!(failure.context().guard_index, reject.then_some(1));
        assert_eq!(
            *events.lock().unwrap(),
            if reject {
                vec![0, 11, 10]
            } else {
                vec![1, 0, 11, 10]
            }
        );
        let secondary = failure.cleanup()[0].clone();
        assert_eq!(secondary.context().phase, Some(CallbackPhase::GuardExit));
        assert_eq!(secondary.context().guard_index, Some(usize::from(!reject)));
        assert_eq!(failure.cleanup().len(), if reject { 2 } else { 3 });
        drop(handoff);
        drop(failure);
        assert!(
            weak.upgrade().is_some(),
            "an independently escaped secondary owns its code lease"
        );
        drop(secondary);
        assert!(weak.upgrade().is_none());
    }
}

struct PanicDrop {
    lease: Weak<()>,
    drops: Arc<AtomicUsize>,
}
impl Drop for PanicDrop {
    fn drop(&mut self) {
        assert!(self.lease.upgrade().is_some());
        self.drops.fetch_add(1, Ordering::SeqCst);
        panic!("prepared payload cleanup");
    }
}

#[test]
fn partial_adapter_error_keeps_original_cause_and_reports_prepared_payload_cleanup_atomically() {
    let lease = Arc::new(());
    let weak = Arc::downgrade(&lease);
    let drops = Arc::new(AtomicUsize::new(0));
    let first: Arc<dyn EdgeAdapter> = Arc::new({
        let drops = drops.clone();
        let weak = weak.clone();
        move |_: &Value| {
            Ok(Value::local(PanicDrop {
                lease: weak.clone(),
                drops: drops.clone(),
            })
            .with_lease(lease.clone()))
        }
    });
    let calls = Arc::new(AtomicUsize::new(0));
    let cause_drops = Arc::new(AtomicUsize::new(0));
    let second: Arc<dyn EdgeAdapter> = Arc::new({
        let calls = calls.clone();
        let cause_drops = cause_drops.clone();
        move |_: &Value| {
            Err(Failure::external(
                "adapter primary",
                Cause {
                    leases: vec![],
                    calls: calls.clone(),
                    drops: cause_drops.clone(),
                },
            ))
        }
    });
    let prepared_identity = phenix_graph::TypeIdentity::Local(std::any::TypeId::of::<PanicDrop>());
    let set = ConnectionSet::new(vec![
        ConnectionQueue::new(NonZeroUsize::new(1).unwrap(), prepared_identity.clone()),
        ConnectionQueue::new(NonZeroUsize::new(1).unwrap(), prepared_identity),
    ]);
    let error = set
        .prepare(
            &Packet::new(Value::local(0_u8)),
            &[
                FanoutTarget {
                    connection: 0,
                    preparation: phenix_graph::connection::AdapterRef::Owned(first),
                },
                FanoutTarget {
                    connection: 1,
                    preparation: phenix_graph::connection::AdapterRef::Owned(second),
                },
            ],
        )
        .unwrap_err()
        .into_failure();
    assert!(error.source().unwrap().is::<Cause>());
    assert_eq!(error.summary(), "adapter primary");
    assert_eq!(error.cleanup().len(), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(
        error.cleanup()[0]
            .source()
            .unwrap()
            .is::<phenix_graph::connection::PacketDestructorPanicked>()
    );
    assert!(set.queues().iter().all(ConnectionQueue::is_empty));
    assert!(weak.upgrade().is_some());
    drop(error);
    assert_eq!(cause_drops.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(weak.upgrade().is_none());
}

#[test]
fn primitive_codec_faults_preserve_fields_and_standard_sources() {
    let failure =
        Value::from_wire_parts(<i64 as phenix_graph::ContractValue>::wire_schema(), vec![0])
            .get::<i64>()
            .unwrap_err();
    assert!(matches!(
        failure
            .source()
            .unwrap()
            .downcast_ref::<phenix_graph::ValueError>(),
        Some(phenix_graph::ValueError::Length {
            expected: 8,
            actual: 1,
            ..
        })
    ));
    assert!(
        failure
            .source()
            .unwrap()
            .source()
            .unwrap()
            .is::<std::array::TryFromSliceError>()
    );
    let failure = Value::from_wire_parts(
        <String as phenix_graph::ContractValue>::wire_schema(),
        vec![255],
    )
    .get::<String>()
    .unwrap_err();
    assert!(failure.source().unwrap().is::<std::string::FromUtf8Error>());
    assert_eq!(failure.context().phase, Some(CallbackPhase::Decode));
}

#[test]
fn successful_emission_disposal_keeps_its_extra_lease_through_non_string_panic_payload_drop() {
    struct Payload {
        leases: Vec<Weak<()>>,
        drops: Arc<AtomicUsize>,
    }
    impl Drop for Payload {
        fn drop(&mut self) {
            assert!(self.leases.iter().all(|lease| lease.upgrade().is_some()));
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    struct Panics(Option<Payload>);
    impl Drop for Panics {
        fn drop(&mut self) {
            std::panic::panic_any(self.0.take().unwrap());
        }
    }
    struct Emits(Option<Value>);
    impl Process for Emits {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
            Ok(Step::Emit(phenix_graph::ProcessEmission::new(
                "out",
                self.0.take().unwrap(),
            )))
        }
    }
    let code = Arc::new(());
    let extra = Arc::new(());
    let weak = [Arc::downgrade(&code), Arc::downgrade(&extra)];
    let drops = Arc::new(AtomicUsize::new(0));
    let value = Value::local(Panics(Some(Payload {
        leases: weak.to_vec(),
        drops: drops.clone(),
    })))
    .with_lease(extra);
    let mut handoff = ProcessHandoff::new(Box::new(Emits(Some(value)))).with_lease(code);
    handoff.admit(ProcessContext::default()).unwrap();
    let completion = handoff
        .next_job()
        .unwrap()
        .with_guard(Arc::new(Guard {
            index: 0,
            events: Arc::new(Mutex::new(Vec::new())),
            reject: false,
        }))
        .run();
    let failure = handoff.complete(completion).unwrap().unwrap_err();
    assert_eq!(failure.context().phase, Some(CallbackPhase::GuardExit));
    assert_eq!(failure.context().guard_index, Some(0));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    let secondary = failure
        .cleanup()
        .iter()
        .find(|failure| {
            matches!(
                failure
                    .source()
                    .unwrap()
                    .downcast_ref::<phenix_graph::process::ProcessCleanupError>(),
                Some(phenix_graph::process::ProcessCleanupError::Emission)
            )
        })
        .unwrap()
        .clone();
    drop(handoff);
    drop(failure);
    assert!(weak.iter().all(|lease| lease.upgrade().is_some()));
    drop(secondary);
    assert!(weak.iter().all(|lease| lease.upgrade().is_none()));
}

#[test]
fn invalidated_emission_reports_final_independent_lease_panics_and_attempts_every_release() {
    struct Payload {
        code: Weak<()>,
        input: Weak<()>,
        guards: Vec<Weak<Guard>>,
        index: usize,
        events: Arc<Mutex<Vec<usize>>>,
    }
    impl Drop for Payload {
        fn drop(&mut self) {
            assert!(self.code.upgrade().is_some());
            assert!(self.input.upgrade().is_some());
            assert!(self.guards.iter().all(|guard| guard.upgrade().is_some()));
            self.events.lock().unwrap().push(300 + self.index);
        }
    }
    struct Lease(Option<Payload>);
    impl Drop for Lease {
        fn drop(&mut self) {
            let payload = self.0.take().unwrap();
            payload.events.lock().unwrap().push(200 + payload.index);
            std::panic::panic_any(payload);
        }
    }
    struct OrdinaryValue {
        leases: Vec<Weak<Lease>>,
        events: Arc<Mutex<Vec<usize>>>,
    }
    impl Drop for OrdinaryValue {
        fn drop(&mut self) {
            assert!(self.leases.iter().all(|lease| lease.upgrade().is_some()));
            self.events.lock().unwrap().push(100);
        }
    }
    struct InputValue(Arc<Mutex<Vec<usize>>>);
    impl Drop for InputValue {
        fn drop(&mut self) {
            self.0.lock().unwrap().push(50);
        }
    }
    struct Emits(Option<Value>);
    impl Process for Emits {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, Failure> {
            drop(context.take("in"));
            Ok(Step::Emit(phenix_graph::ProcessEmission::new(
                "out",
                self.0.take().unwrap(),
            )))
        }
    }
    for count in [1, 3] {
        let code = Arc::new(());
        let weak_code = Arc::downgrade(&code);
        let input = Arc::new(());
        let weak_input = Arc::downgrade(&input);
        let events = Arc::new(Mutex::new(Vec::new()));
        let guards: Vec<_> = (0..2)
            .map(|index| {
                Arc::new(Guard {
                    index,
                    events: events.clone(),
                    reject: false,
                })
            })
            .collect();
        let leases: Vec<_> = (0..count)
            .map(|index| {
                Arc::new(Lease(Some(Payload {
                    code: weak_code.clone(),
                    input: weak_input.clone(),
                    guards: guards.iter().map(Arc::downgrade).collect(),
                    index,
                    events: events.clone(),
                })))
            })
            .collect();
        let weak_leases: Vec<_> = leases.iter().map(Arc::downgrade).collect();
        let mut value = Value::local(OrdinaryValue {
            leases: weak_leases.clone(),
            events: events.clone(),
        });
        for lease in leases {
            value = value.with_lease(lease);
        }
        let mut handoff = ProcessHandoff::new(Box::new(Emits(Some(value)))).with_lease(code);
        handoff
            .admit(ProcessContext::new(
                [(
                    "in".into(),
                    Value::local(InputValue(events.clone())).with_lease(input),
                )]
                .into(),
                vec![],
            ))
            .unwrap();
        let mut job = handoff.next_job().unwrap();
        let ticket = job.ticket();
        for guard in guards {
            job = job.with_guard(guard);
        }
        let completion = std::thread::spawn(move || job.run()).join().unwrap();
        let mut expected = vec![50, 1, 100];
        for index in (0..count).rev() {
            expected.extend([200 + index, 300 + index]);
        }
        expected.push(0);
        assert_eq!(*events.lock().unwrap(), expected);
        assert!(weak_leases.iter().all(|lease| lease.upgrade().is_none()));
        let failure = handoff.complete(completion).unwrap().unwrap_err();
        assert_eq!(failure.context().phase, Some(CallbackPhase::GuardExit));
        assert_eq!(failure.context().guard_index, Some(1));
        assert!(
            failure
                .source()
                .unwrap()
                .is::<phenix_graph::process::CallbackPanicked>()
        );
        let emission_failures: Vec<_> = failure
            .cleanup()
            .iter()
            .filter(|failure| {
                failure
                    .source()
                    .unwrap()
                    .downcast_ref::<phenix_graph::process::ProcessCleanupError>()
                    == Some(&phenix_graph::process::ProcessCleanupError::Emission)
            })
            .collect();
        assert_eq!(emission_failures.len(), count);
        for failure in std::iter::once(&failure).chain(emission_failures.iter().copied()) {
            let context = failure.context();
            assert_eq!(context.ticket, Some(ticket));
            assert_eq!(context.session, None);
            assert_eq!(context.generation, None);
            assert_eq!(context.request, None);
            assert_eq!(context.process, None);
            assert_eq!(context.path, None);
        }
        let secondary = emission_failures[0].clone();
        assert_eq!(secondary.context().phase, Some(CallbackPhase::Cleanup));
        assert_eq!(secondary.context().guard_index, None);
        expected.extend([11, 10]);
        assert_eq!(*events.lock().unwrap(), expected);
        assert!(handoff.outstanding().is_none());
        assert_eq!(handoff.state(), phenix_graph::process::HandoffState::Failed);
        drop(failure);
        drop(handoff);
        assert!(weak_code.upgrade().is_some());
        assert!(weak_input.upgrade().is_some());
        drop(secondary);
        assert!(weak_code.upgrade().is_none());
        assert!(weak_input.upgrade().is_none());
    }
}

#[test]
fn accepted_session_cleanup_faults_preserve_actual_identity_on_primaries_and_secondaries() {
    #[derive(Clone, Copy)]
    enum Mode {
        Complete,
        CompleteLeasePanic,
        Reject,
        StepFailure,
        EmitPayloadPanic,
        EmitLeasePanic,
    }
    struct LeasePanic;
    impl Drop for LeasePanic {
        fn drop(&mut self) {
            panic!("independent lease cleanup");
        }
    }
    struct Input {
        code: Weak<()>,
        drops: Arc<AtomicUsize>,
        panic: bool,
    }
    impl Drop for Input {
        fn drop(&mut self) {
            assert!(self.code.upgrade().is_some());
            self.drops.fetch_add(1, Ordering::SeqCst);
            if self.panic {
                panic!("input cleanup");
            }
        }
    }
    struct Worker {
        mode: Mode,
        code: Weak<()>,
        drops: Arc<AtomicUsize>,
    }
    impl Process for Worker {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
            match self.mode {
                Mode::Complete | Mode::CompleteLeasePanic => Ok(Step::Complete),
                Mode::Reject => unreachable!(),
                Mode::StepFailure => Err(Failure::application_message("step primary")
                    .with_context(phenix_graph::FailureContext {
                        field: Some("in".into()),
                        ..Default::default()
                    })),
                Mode::EmitPayloadPanic | Mode::EmitLeasePanic => {
                    let value = match self.mode {
                        Mode::EmitPayloadPanic => Value::local(PanicDrop {
                            lease: self.code.clone(),
                            drops: self.drops.clone(),
                        }),
                        _ => Value::local(()).with_lease(Arc::new(LeasePanic)),
                    };
                    Ok(Step::Emit(phenix_graph::ProcessEmission::new("out", value)))
                }
            }
        }
    }
    for scoped in [false, true] {
        for mode in [
            Mode::Complete,
            Mode::CompleteLeasePanic,
            Mode::Reject,
            Mode::StepFailure,
            Mode::EmitPayloadPanic,
            Mode::EmitLeasePanic,
        ] {
            let code = Arc::new(());
            let weak = Arc::downgrade(&code);
            let input_drops = Arc::new(AtomicUsize::new(0));
            let emission_drops = Arc::new(AtomicUsize::new(0));
            let factory_code = weak.clone();
            let factory_drops = emission_drops.clone();
            let mut graph = NetworkBuilder::new();
            let node = graph
                .node(
                    "worker",
                    Arc::new(
                        Interface::new(
                            [PortDef::local::<Input>("in")],
                            [PortDef::local::<()>("out")],
                        )
                        .unwrap(),
                    ),
                    Arc::new(move || Worker {
                        mode,
                        code: factory_code.clone(),
                        drops: factory_drops.clone(),
                    }),
                )
                .unwrap();
            graph
                .expose_input(
                    "in",
                    node.input::<Input>("in").unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            let compiled = graph.build().unwrap().compile().unwrap();
            let mut session = compiled.start_with_options(SessionOptions {
                lease: Some(code),
                ..Default::default()
            });
            let request = scoped.then(|| session.begin_request(None).unwrap());
            let mut input = Value::local(Input {
                code: weak.clone(),
                drops: input_drops.clone(),
                panic: !matches!(mode, Mode::CompleteLeasePanic),
            });
            if matches!(mode, Mode::CompleteLeasePanic) {
                input = input.with_lease(Arc::new(LeasePanic));
            }
            if let Some(request) = request {
                session.inject_request(request, "in", input).unwrap();
                session.close_request(request).unwrap();
            } else {
                session.inject("in", input).unwrap();
            }
            let (index, mut job) = session.next_job().unwrap();
            let ticket = job.ticket();
            let metadata = job.metadata().unwrap().clone();
            assert_eq!(metadata.request, request);
            assert_eq!(metadata.process, 0);
            assert_eq!(metadata.path, "worker");
            let emits = matches!(mode, Mode::EmitPayloadPanic | Mode::EmitLeasePanic);
            if emits || matches!(mode, Mode::Reject) {
                job = job.with_guard(Arc::new(Guard {
                    index: 0,
                    events: Arc::new(Mutex::new(Vec::new())),
                    reject: false,
                }));
            }
            if matches!(mode, Mode::Reject) {
                session.reject(index, job).unwrap();
            } else {
                session
                    .complete_job(std::thread::spawn(move || job.run()).join().unwrap())
                    .unwrap();
            }
            let failure = match request {
                Some(request) => session.request_failure(request).unwrap().unwrap(),
                None => session.failure().unwrap(),
            }
            .clone();
            for failure in std::iter::once(&failure).chain(failure.cleanup()) {
                let context = failure.context();
                assert_eq!(context.ticket, Some(ticket));
                assert_eq!(context.session, Some(metadata.session));
                assert_eq!(context.generation, Some(metadata.generation));
                assert_eq!(context.request, request);
                assert_eq!(context.process, Some(0));
                assert_eq!(context.path.as_deref(), Some("worker"));
            }
            let owned = if matches!(mode, Mode::Complete | Mode::CompleteLeasePanic) {
                &failure
            } else {
                failure
                    .cleanup()
                    .iter()
                    .find(|failure| {
                        failure
                            .source()
                            .unwrap()
                            .downcast_ref::<phenix_graph::process::ProcessCleanupError>()
                            == Some(&phenix_graph::process::ProcessCleanupError::OwnedState)
                    })
                    .unwrap()
            };
            assert_eq!(
                owned
                    .source()
                    .unwrap()
                    .downcast_ref::<phenix_graph::process::ProcessCleanupError>(),
                Some(&phenix_graph::process::ProcessCleanupError::OwnedState)
            );
            assert_eq!(owned.context().phase, Some(CallbackPhase::Cleanup));
            assert_eq!(input_drops.load(Ordering::SeqCst), 1);
            assert_eq!(
                emission_drops.load(Ordering::SeqCst),
                usize::from(matches!(mode, Mode::EmitPayloadPanic))
            );
            if emits {
                assert_eq!(failure.context().phase, Some(CallbackPhase::GuardExit));
                assert_eq!(failure.context().guard_index, Some(0));
                assert!(failure.cleanup().iter().any(|failure| {
                    failure
                        .source()
                        .unwrap()
                        .downcast_ref::<phenix_graph::process::ProcessCleanupError>()
                        == Some(&phenix_graph::process::ProcessCleanupError::Emission)
                }));
            } else if matches!(mode, Mode::StepFailure) {
                assert_eq!(failure.summary(), "step primary");
                assert_eq!(failure.context().field.as_deref(), Some("in"));
                assert_eq!(failure.context().phase, Some(CallbackPhase::Step));
            }
            let escaped = owned.clone();
            drop(failure);
            drop(session);
            drop(compiled);
            assert!(weak.upgrade().is_some());
            drop(escaped);
            assert!(weak.upgrade().is_none());
        }
    }
}

#[cfg(feature = "derive")]
#[test]
fn generated_codec_rejections_have_typed_fields_tags_offsets_and_trailing_counts() {
    use phenix_graph::{CodecError, ContractValue};
    #[derive(phenix_graph::ContractValue)]
    #[contract(schema = "q7/record")]
    struct Record {
        value: i64,
        text: Option<String>,
    }
    let encoded = Record {
        value: 1,
        text: Some("text".into()),
    }
    .encode();
    let truncated = Record::decode(&encoded[..4]).err().unwrap();
    assert!(
        matches!(truncated.source().unwrap().downcast_ref::<CodecError>(), Some(CodecError::TruncatedHeader { field, offset: 0, remaining: 4 }) if field == "value")
    );
    let mut bad = encoded.clone();
    bad[24] = 9;
    let tag = Record::decode(&bad).err().unwrap();
    assert!(
        matches!(tag.source().unwrap().downcast_ref::<CodecError>(), Some(CodecError::OptionTag { field, tag: Some(9), length: 5 }) if field == "text")
    );
    let mut bad = encoded.clone();
    bad[..8].copy_from_slice(&u64::MAX.to_le_bytes());
    let overflow = Record::decode(&bad).err().unwrap();
    assert!(
        matches!(overflow.source().unwrap().downcast_ref::<CodecError>(), Some(CodecError::OffsetOverflow { field, offset: 8, .. }) if field == "value")
    );
    let mut bad = encoded.clone();
    bad.push(0);
    let trailing = Record::decode(&bad).err().unwrap();
    assert!(
        matches!(trailing.source().unwrap().downcast_ref::<CodecError>(), Some(CodecError::TrailingBytes { offset, total }) if *offset == encoded.len() && *total == encoded.len() + 1)
    );
}

#[test]
fn accepted_reject_cleanup_failure_settles_ticket_and_disposes_inputs_and_process_once() {
    struct ProcessDrop(Arc<AtomicUsize>);
    impl Process for ProcessDrop {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
            unreachable!()
        }
    }
    impl Drop for ProcessDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic!("process cleanup");
        }
    }
    let lease = Arc::new(());
    let weak = Arc::downgrade(&lease);
    let process_drops = Arc::new(AtomicUsize::new(0));
    let input_drops = Arc::new(AtomicUsize::new(0));
    let mut handoff =
        ProcessHandoff::new(Box::new(ProcessDrop(process_drops.clone()))).with_lease(lease.clone());
    handoff
        .admit(ProcessContext::new(
            [(
                "in".into(),
                Value::local(PanicDrop {
                    lease: weak.clone(),
                    drops: input_drops.clone(),
                })
                .with_lease(lease),
            )]
            .into(),
            vec![],
        ))
        .unwrap();
    let job = handoff.next_job().unwrap().with_guard(Arc::new(Guard {
        index: 0,
        events: Arc::new(Mutex::new(Vec::new())),
        reject: false,
    }));
    let failure = handoff.reject(job).unwrap().unwrap_err();
    assert!(handoff.outstanding().is_none());
    assert_eq!(handoff.state(), phenix_graph::process::HandoffState::Failed);
    assert_eq!(input_drops.load(Ordering::SeqCst), 1);
    assert_eq!(process_drops.load(Ordering::SeqCst), 1);
    assert_eq!(failure.cleanup().len(), 1);
    let secondary = failure.cleanup()[0].clone();
    drop(failure);
    drop(handoff);
    assert!(weak.upgrade().is_some());
    drop(secondary);
    assert!(weak.upgrade().is_none());
    assert_eq!(process_drops.load(Ordering::SeqCst), 1);
}

#[test]
fn managed_packet_disposal_reports_panics_inside_a_contained_failure_value() {
    #[derive(Debug)]
    struct Payload {
        leases: Vec<Weak<()>>,
        drops: Arc<AtomicUsize>,
    }
    impl Drop for Payload {
        fn drop(&mut self) {
            assert!(self.leases.iter().all(|lease| lease.upgrade().is_some()));
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[derive(Debug)]
    struct DropError(Option<Payload>);
    impl fmt::Display for DropError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("drop error")
        }
    }
    impl Error for DropError {}
    impl Drop for DropError {
        fn drop(&mut self) {
            std::panic::panic_any(self.0.take().unwrap());
        }
    }
    let code = Arc::new(());
    let envelope = Arc::new(());
    let leases = vec![Arc::downgrade(&code), Arc::downgrade(&envelope)];
    let weak = leases.clone();
    let drops = Arc::new(AtomicUsize::new(0));
    let failure = Failure::external(
        "source ERROR DATA",
        DropError(Some(Payload {
            leases,
            drops: drops.clone(),
        })),
    )
    .with_lease(code);
    let packet = Packet::new(Value::local(failure).with_lease(envelope));
    assert!(packet.dispose_catching());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(weak.iter().all(|lease| lease.upgrade().is_none()));
}

#[test]
fn rejected_emission_and_payload_panic_terminate_only_the_original_request() {
    use phenix_graph::runtime::{CleanupStage, Readiness, RuntimeError};

    struct Producer {
        code: Weak<()>,
        drops: Arc<AtomicUsize>,
    }
    impl Process for Producer {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, Failure> {
            if *context.take("in").unwrap().borrow::<bool>().unwrap() {
                return Ok(Step::Complete);
            }
            Ok(Step::Emit(phenix_graph::ProcessEmission::new(
                "out",
                Value::local(PanicDrop {
                    lease: self.code.clone(),
                    drops: self.drops.clone(),
                }),
            )))
        }
    }
    struct Finish;
    impl Process for Finish {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
            Ok(Step::Finished)
        }
    }
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let code = Arc::new(());
        let weak = Arc::downgrade(&code);
        let drops = Arc::new(AtomicUsize::new(0));
        let factory_code = weak.clone();
        let factory_drops = drops.clone();
        let mut graph = NetworkBuilder::new();
        let producer = graph
            .node(
                "producer",
                Arc::new(
                    Interface::new(
                        [PortDef::local::<bool>("in")],
                        [PortDef::local::<PanicDrop>("out")],
                    )
                    .unwrap(),
                ),
                Arc::new(move || Producer {
                    code: factory_code.clone(),
                    drops: factory_drops.clone(),
                }),
            )
            .unwrap();
        let sink = graph
            .node(
                "sink",
                Arc::new(
                    Interface::new(
                        [
                            PortDef::local::<PanicDrop>("in"),
                            PortDef::local::<()>("stop"),
                        ],
                        [],
                    )
                    .unwrap(),
                ),
                Arc::new(|| Finish),
            )
            .unwrap();
        graph
            .connect(
                producer.output::<PanicDrop>("out").unwrap(),
                sink.input::<PanicDrop>("in").unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        graph
            .expose_input(
                "in",
                producer.input::<bool>("in").unwrap(),
                NonZeroUsize::new(2).unwrap(),
            )
            .unwrap();
        graph
            .expose_input(
                "stop",
                sink.input::<()>("stop").unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        let compiled = graph.build().unwrap().compile().unwrap();
        let mut session = compiled.start_with_options(SessionOptions {
            lease: Some(code),
            readiness: [("sink".into(), Readiness::Any)].into(),
            request_queue_policy: queue,
            request_failure_policy: RequestFailurePolicy::RestartAffectedProcess,
            ..Default::default()
        });
        session.inject_local("stop", ()).unwrap();
        assert_eq!(session.run_until_idle(1).steps, 1);
        let first = session.begin_request(None).unwrap();
        let next = session.begin_request(None).unwrap();
        session.inject_request_local(first, "in", false).unwrap();
        session.inject_request_local(next, "in", true).unwrap();
        session.close_request(first).unwrap();
        session.close_request(next).unwrap();
        let (_, job) = session.next_job().unwrap();
        let metadata = job.metadata().unwrap().clone();
        let ticket = job.ticket();
        assert_eq!(metadata.request, Some(first));
        session.complete_job(job.run()).unwrap();
        assert!(session.failure().is_none());
        assert_eq!(
            session.request_status(first).unwrap(),
            RequestStatus::Failed
        );
        assert!(session.request_failure(next).unwrap().is_none());
        let failure = session.request_failure(first).unwrap().unwrap().clone();
        assert_eq!(
            failure.source().unwrap().downcast_ref(),
            Some(&phenix_graph::connection::ConnectionError::ReaderClosed)
        );
        assert_eq!(failure.cleanup().len(), 1);
        assert_eq!(
            failure.cleanup()[0].context().phase,
            Some(CallbackPhase::Cleanup)
        );
        assert_eq!(
            failure.cleanup()[0].source().unwrap().downcast_ref(),
            Some(&RuntimeError::DestructorPanicked {
                stage: CleanupStage::Failure,
            })
        );
        for failure in std::iter::once(&failure).chain(failure.cleanup()) {
            let context = failure.context();
            assert_eq!(context.session, Some(metadata.session));
            assert_eq!(context.generation, Some(metadata.generation));
            assert_eq!(context.request, Some(first));
            assert_eq!(context.process, Some(metadata.process));
            assert_eq!(context.path.as_deref(), Some("producer"));
            assert_eq!(context.ticket, Some(ticket));
            assert_eq!(context.output.as_deref(), Some("out"));
        }
        let disposal = session.request_disposal(first).unwrap();
        assert_eq!(disposal.activations, 1);
        assert_eq!(disposal.handed_jobs, 0);
        assert!(session.request_cancelled_jobs(first).unwrap().is_empty());
        let (_, job) = session.next_job().unwrap();
        assert_eq!(job.metadata().unwrap().request, Some(next));
        assert_ne!(job.ticket(), ticket);
        session.complete_job(job.run()).unwrap();
        session.run_until_idle(8);
        assert_eq!(
            session.request_status(next).unwrap(),
            RequestStatus::Settled { captures: 0 }
        );
        assert!(session.request_failure(next).unwrap().is_none());
        assert!(session.failure().is_none());
        session.cancel_request(first).unwrap();
        assert_eq!(session.request_disposal(first).unwrap(), disposal);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        session.release_request(first).unwrap();
        session.release_request(next).unwrap();
        drop(session);
        drop(compiled);
        assert!(weak.upgrade().is_some());
        drop(failure);
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn request_escalation_shares_complete_ordered_cleanup_without_mutating_escaped_primary() {
    use phenix_graph::runtime::{CleanupStage, RequestError, RuntimeError, SessionState};

    struct PanickingProcess;
    impl Process for PanickingProcess {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, Failure> {
            drop(context.take("in"));
            Ok(Step::WaitWake)
        }
    }
    impl Drop for PanickingProcess {
        fn drop(&mut self) {
            panic!("request or peer process cleanup");
        }
    }
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for policy in [
            RequestFailurePolicy::FailSession,
            RequestFailurePolicy::RestartAffectedProcess,
        ] {
            let spawns = Arc::new(AtomicUsize::new(0));
            let count = spawns.clone();
            let mut graph = NetworkBuilder::new();
            let own = graph
                .node(
                    "own",
                    Arc::new(Interface::new([PortDef::local::<()>("in")], []).unwrap()),
                    Arc::new(move || {
                        assert_eq!(count.fetch_add(1, Ordering::SeqCst), 0, "recovery factory");
                        PanickingProcess
                    }),
                )
                .unwrap();
            let peer = graph
                .node(
                    "peer",
                    Arc::new(Interface::new([PortDef::local::<()>("in")], []).unwrap()),
                    Arc::new(|| PanickingProcess),
                )
                .unwrap();
            for (name, node) in [("own", own), ("peer", peer)] {
                graph
                    .expose_input(
                        name,
                        node.input::<()>("in").unwrap(),
                        NonZeroUsize::new(1).unwrap(),
                    )
                    .unwrap();
            }
            let code = Arc::new(());
            let weak = Arc::downgrade(&code);
            let compiled = graph.build().unwrap().compile().unwrap();
            let mut session = compiled.start_with_options(SessionOptions {
                lease: Some(code.clone()),
                request_queue_policy: queue,
                request_failure_policy: policy,
                ..Default::default()
            });
            session.inject_local("peer", ()).unwrap();
            let (_, job) = session.next_job().unwrap();
            assert_eq!(job.metadata().unwrap().path, "peer");
            session.complete_job(job.run()).unwrap();
            let request = session.begin_request(None).unwrap();
            session.inject_request_local(request, "own", ()).unwrap();
            let (_, job) = session.next_job().unwrap();
            assert_eq!(job.metadata().unwrap().request, Some(request));
            session.complete_job(job.run()).unwrap();
            let calls = Arc::new(AtomicUsize::new(0));
            let drops = Arc::new(AtomicUsize::new(0));
            let primary = Failure::external(
                "host primary",
                Cause {
                    leases: vec![weak.clone()],
                    calls: calls.clone(),
                    drops: drops.clone(),
                },
            )
            .with_lease(code);
            let escaped = primary.clone();
            assert_eq!(
                session.fail_request(request, primary),
                Err(if policy == RequestFailurePolicy::FailSession {
                    RequestError::ProcessDestructorPanicked
                } else {
                    RequestError::RecoveryFailed
                })
            );
            assert_eq!(session.state(), SessionState::Failed);
            assert_eq!(
                session.request_status(request).unwrap(),
                RequestStatus::Failed
            );
            let request_failure = session.request_failure(request).unwrap().unwrap().clone();
            let session_failure = session.failure().unwrap();
            assert!(std::ptr::eq(
                request_failure.source().unwrap(),
                session_failure.source().unwrap()
            ));
            for failure in [&request_failure, session_failure] {
                assert!(std::ptr::eq(
                    failure.source().unwrap(),
                    escaped.source().unwrap()
                ));
                assert!(failure.source().unwrap().is::<Cause>());
                assert_eq!(failure.context().request, Some(request));
                assert_eq!(
                    failure.cleanup().len(),
                    if policy == RequestFailurePolicy::FailSession {
                        2
                    } else {
                        3
                    }
                );
                assert!(std::ptr::eq(
                    failure.source().unwrap(),
                    escaped.source().unwrap()
                ));
                assert!(failure.source().unwrap().is::<Cause>());
                assert_eq!(failure.context().request, Some(request));
                assert_eq!(
                    failure.cleanup().len(),
                    if policy == RequestFailurePolicy::FailSession {
                        2
                    } else {
                        3
                    }
                );
                assert_eq!(
                    failure.cleanup()[0].source().unwrap().downcast_ref(),
                    Some(&RequestError::ProcessDestructorPanicked)
                );
                assert_eq!(failure.cleanup()[0].context().request, Some(request));
                assert_eq!(
                    failure.cleanup()[0].context().phase,
                    Some(CallbackPhase::Cleanup)
                );
                if policy == RequestFailurePolicy::RestartAffectedProcess {
                    let recovery = &failure.cleanup()[1];
                    assert_eq!(recovery.context().request, Some(request));
                    assert_eq!(recovery.context().path.as_deref(), Some("own"));
                    assert_eq!(
                        recovery.context().phase,
                        Some(CallbackPhase::RecoveryConstruct)
                    );
                    assert!(
                        recovery
                            .source()
                            .unwrap()
                            .is::<phenix_graph::process::CallbackPanicked>()
                    );
                }
                assert_eq!(
                    failure
                        .cleanup()
                        .last()
                        .unwrap()
                        .source()
                        .unwrap()
                        .downcast_ref(),
                    Some(&RuntimeError::DestructorPanicked {
                        stage: CleanupStage::Failure
                    })
                );
            }
            assert!(escaped.cleanup().is_empty());
            assert_eq!(escaped.context().request, None);
            let disposal = session.request_disposal(request).unwrap();
            session.cancel_request(request).unwrap();
            assert_eq!(session.request_disposal(request).unwrap(), disposal);
            session.release_request(request).unwrap();
            drop(session);
            drop(compiled);
            drop(escaped);
            assert!(weak.upgrade().is_some());
            drop(request_failure);
            assert!(weak.upgrade().is_none());
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert_eq!(drops.load(Ordering::SeqCst), 1);
        }
    }
}

#[test]
fn recovery_constructor_keeps_retired_request_identity_and_only_known_tickets() {
    use phenix_graph::runtime::{RequestError, SessionState};

    struct Worker(bool);
    impl Process for Worker {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, Failure> {
            drop(context.take("in"));
            if self.0 {
                Err(Failure::application_message("step primary"))
            } else {
                Ok(Step::Yield)
            }
        }
    }
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for cancel in [false, true] {
            for handed in [false, true] {
                let spawns = Arc::new(AtomicUsize::new(0));
                let count = spawns.clone();
                let mut graph = NetworkBuilder::new();
                let node = graph
                    .node(
                        "worker",
                        Arc::new(Interface::new([PortDef::local::<()>("in")], []).unwrap()),
                        Arc::new(move || {
                            assert_eq!(count.fetch_add(1, Ordering::SeqCst), 0, "recovery factory");
                            Worker(!cancel && !handed)
                        }),
                    )
                    .unwrap();
                graph
                    .expose_input(
                        "in",
                        node.input::<()>("in").unwrap(),
                        NonZeroUsize::new(1).unwrap(),
                    )
                    .unwrap();
                let mut session = graph
                    .build()
                    .unwrap()
                    .compile()
                    .unwrap()
                    .start_with_options(SessionOptions {
                        request_queue_policy: queue,
                        request_failure_policy: RequestFailurePolicy::RestartAffectedProcess,
                        ..Default::default()
                    });
                let request = session.begin_request(None).unwrap();
                session.inject_request_local(request, "in", ()).unwrap();
                let (_, job) = session.next_job().unwrap();
                let ticket = job.ticket();
                let metadata = job.metadata().unwrap().clone();
                let retained = if handed {
                    Some(job)
                } else {
                    session.complete_job(job.run()).unwrap();
                    None
                };
                if cancel || handed {
                    let result = if cancel {
                        session.cancel_request(request)
                    } else {
                        session.fail_request(request, Failure::application_message("host primary"))
                    };
                    assert_eq!(result, Err(RequestError::RecoveryFailed));
                }
                assert_eq!(session.state(), SessionState::Failed);
                assert_eq!(
                    session.request_status(request).unwrap(),
                    if cancel {
                        RequestStatus::Cancelled
                    } else {
                        RequestStatus::Failed
                    }
                );
                let failure = session.request_failure(request).unwrap().unwrap();
                let recovery = if cancel {
                    failure
                } else {
                    &failure.cleanup()[0]
                };
                let context = recovery.context();
                assert_eq!(context.session, Some(metadata.session));
                assert_eq!(context.generation, Some(metadata.generation));
                assert_eq!(context.request, Some(request));
                assert_eq!(context.process, Some(metadata.process));
                assert_eq!(context.path.as_deref(), Some("worker"));
                assert_eq!(context.ticket, (handed || !cancel).then_some(ticket));
                assert_eq!(context.phase, Some(CallbackPhase::RecoveryConstruct));
                assert!(matches!(
                    recovery
                        .source()
                        .unwrap()
                        .downcast_ref::<phenix_graph::process::CallbackPanicked>(),
                    Some(phenix_graph::process::CallbackPanicked {
                        phase: CallbackPhase::RecoveryConstruct,
                        ..
                    })
                ));
                let session_failure = session.failure().unwrap();
                if cancel {
                    assert!(std::ptr::eq(
                        failure.source().unwrap(),
                        session_failure.source().unwrap()
                    ));
                } else {
                    assert!(failure.source().is_none());
                    assert!(session_failure.source().is_none());
                }
                let session_recovery = if cancel {
                    session_failure
                } else {
                    &session_failure.cleanup()[0]
                };
                assert_eq!(session_recovery.context(), recovery.context());
                assert!(std::ptr::eq(
                    recovery.source().unwrap(),
                    session_recovery.source().unwrap()
                ));
                assert_eq!(
                    session.request_cancelled_jobs(request).unwrap(),
                    if handed { vec![ticket] } else { vec![] }
                );
                let disposal = session.request_disposal(request).unwrap();
                session.cancel_request(request).unwrap();
                assert_eq!(session.request_disposal(request).unwrap(), disposal);
                if let Some(job) = retained {
                    let completion = session.complete_job(job.run()).unwrap_err().1;
                    assert_eq!(completion.ticket(), ticket);
                    drop(completion);
                }
                assert_eq!(spawns.load(Ordering::SeqCst), 2);
                session.release_request(request).unwrap();
            }
        }
    }
}

#[test]
fn active_local_cancellation_escalates_without_fabricating_a_request_primary() {
    use phenix_graph::runtime::{CleanupStage, RequestError, RuntimeError, SessionState};

    struct Worker;
    impl Process for Worker {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
            Ok(Step::Yield)
        }
    }
    impl Drop for Worker {
        fn drop(&mut self) {
            panic!("cancelled process cleanup");
        }
    }
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let mut graph = NetworkBuilder::new();
        let interface = Arc::new(Interface::new([PortDef::local::<()>("in")], []).unwrap());
        for name in ["own", "peer"] {
            let node = graph
                .node(name, interface.clone(), Arc::new(|| Worker))
                .unwrap();
            graph
                .expose_input(
                    name,
                    node.input::<()>("in").unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
        }
        let mut session = graph
            .build()
            .unwrap()
            .compile()
            .unwrap()
            .start_with_options(SessionOptions {
                request_queue_policy: queue,
                request_failure_policy: RequestFailurePolicy::FailSession,
                ..Default::default()
            });
        let request = session.begin_request(None).unwrap();
        session.inject_request_local(request, "own", ()).unwrap();
        assert_eq!(
            session.cancel_request(request),
            Err(RequestError::ProcessDestructorPanicked)
        );
        assert_eq!(
            session.request_status(request).unwrap(),
            RequestStatus::Cancelled
        );
        assert!(session.request_failure(request).unwrap().is_none());
        assert_eq!(session.state(), SessionState::Failed);
        let failure = session.failure().unwrap();
        assert_eq!(
            failure.source().unwrap().downcast_ref(),
            Some(&RuntimeError::ActiveRequestCancelled)
        );
        assert_eq!(failure.cleanup().len(), 2);
        assert_eq!(
            failure.cleanup()[0].source().unwrap().downcast_ref(),
            Some(&RequestError::ProcessDestructorPanicked)
        );
        assert_eq!(
            failure.cleanup()[1].source().unwrap().downcast_ref(),
            Some(&RuntimeError::DestructorPanicked {
                stage: CleanupStage::Failure
            })
        );
        session.release_request(request).unwrap();
    }
}

#[test]
fn discarded_emission_contains_each_final_lease_panic_before_publication_or_completion() {
    use phenix_graph::{
        ProcessEmission,
        runtime::{CleanupStage, RuntimeError, SessionState},
    };

    struct Payload(usize, Weak<()>, Arc<Mutex<Vec<usize>>>);
    impl Drop for Payload {
        fn drop(&mut self) {
            assert!(self.1.upgrade().is_some());
            self.2.lock().unwrap().push(self.0);
        }
    }
    struct Lease(Option<Payload>);
    impl Drop for Lease {
        fn drop(&mut self) {
            std::panic::panic_any(self.0.take().unwrap());
        }
    }
    struct Emits {
        count: usize,
        selected: bool,
        code: Weak<()>,
        events: Arc<Mutex<Vec<usize>>>,
    }
    impl Process for Emits {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, Failure> {
            if context.take("in").is_none() {
                return Ok(Step::Complete);
            }
            let mut value = Value::local(7_i64);
            for index in 0..self.count {
                value = value.with_lease(Arc::new(Lease(Some(Payload(
                    index,
                    self.code.clone(),
                    self.events.clone(),
                )))));
            }
            Ok(Step::Emit(if self.selected {
                ProcessEmission::select("out", value, vec![])
            } else {
                ProcessEmission::new("out", value)
            }))
        }
    }
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for scoped in [false, true] {
            for selected in [false, true] {
                for count in [1, 3] {
                    let code = Arc::new(());
                    let weak = Arc::downgrade(&code);
                    let events = Arc::new(Mutex::new(vec![]));
                    let factory_code = weak.clone();
                    let factory_events = events.clone();
                    let mut graph = NetworkBuilder::new();
                    let node = graph
                        .node(
                            "emit",
                            Arc::new(
                                Interface::new(
                                    [PortDef::local::<()>("in")],
                                    [PortDef::local::<i64>("out")],
                                )
                                .unwrap(),
                            ),
                            Arc::new(move || Emits {
                                count,
                                selected,
                                code: factory_code.clone(),
                                events: factory_events.clone(),
                            }),
                        )
                        .unwrap();
                    graph
                        .expose_input(
                            "in",
                            node.input::<()>("in").unwrap(),
                            NonZeroUsize::new(1).unwrap(),
                        )
                        .unwrap();
                    let compiled = graph.build().unwrap().compile().unwrap();
                    let mut session = compiled.start_with_options(SessionOptions {
                        lease: Some(code),
                        discard_outputs: [("emit".into(), "out".into())].into(),
                        request_queue_policy: queue,
                        request_failure_policy: RequestFailurePolicy::RestartAffectedProcess,
                        ..Default::default()
                    });
                    let request = scoped.then(|| session.begin_request(None).unwrap());
                    if let Some(request) = request {
                        session.inject_request_local(request, "in", ()).unwrap();
                        session.close_request(request).unwrap();
                    } else {
                        session.inject_local("in", ()).unwrap();
                    }
                    let (_, job) = session.next_job().unwrap();
                    let metadata = job.metadata().unwrap().clone();
                    let ticket = job.ticket();
                    session.complete_job(job.run()).unwrap();
                    let failure = if let Some(request) = request {
                        assert_eq!(
                            session.request_status(request).unwrap(),
                            RequestStatus::Failed
                        );
                        assert!(session.failure().is_none());
                        assert!(session.request_cancelled_jobs(request).unwrap().is_empty());
                        session.request_failure(request).unwrap().unwrap().clone()
                    } else {
                        assert_eq!(session.state(), SessionState::Failed);
                        session.failure().unwrap().clone()
                    };
                    assert_eq!(
                        *events.lock().unwrap(),
                        (0..count).rev().collect::<Vec<_>>()
                    );
                    assert_eq!(failure.cleanup().len(), count - 1);
                    for failure in std::iter::once(&failure).chain(failure.cleanup()) {
                        assert_eq!(
                            failure.source().unwrap().downcast_ref(),
                            Some(&RuntimeError::DestructorPanicked {
                                stage: CleanupStage::Failure
                            })
                        );
                        let context = failure.context();
                        assert_eq!(context.session, Some(metadata.session));
                        assert_eq!(context.generation, Some(metadata.generation));
                        assert_eq!(context.request, request);
                        assert_eq!(context.process, Some(metadata.process));
                        assert_eq!(context.path.as_deref(), Some("emit"));
                        assert_eq!(context.ticket, Some(ticket));
                        assert_eq!(context.output.as_deref(), Some("out"));
                        assert_eq!(context.phase, Some(CallbackPhase::Cleanup));
                    }
                    assert_eq!(session.run_until_idle(8).publications, 0);
                    assert_eq!(session.discarded_emissions(), 0);
                    if let Some(request) = request {
                        session.release_request(request).unwrap();
                    }
                    drop(session);
                    drop(compiled);
                    assert!(weak.upgrade().is_some());
                    drop(failure);
                    assert!(weak.upgrade().is_none());
                }
            }
        }
    }
}

#[test]
fn invalid_emission_cleanup_keeps_exact_accepted_owner_and_independent_leases() {
    use phenix_graph::runtime::{CleanupStage, RuntimeError};

    struct Emits(Weak<()>, Arc<AtomicUsize>, bool);
    impl Process for Emits {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
            Ok(Step::Emit(phenix_graph::ProcessEmission::new(
                if self.2 { "missing" } else { "out" },
                Value::local(PanicDrop {
                    lease: self.0.clone(),
                    drops: self.1.clone(),
                }),
            )))
        }
    }
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for unknown in [false, true] {
            let code = Arc::new(());
            let weak = Arc::downgrade(&code);
            let drops = Arc::new(AtomicUsize::new(0));
            let factory_code = weak.clone();
            let factory_drops = drops.clone();
            let mut graph = NetworkBuilder::new();
            let node = graph
                .node(
                    "emit",
                    Arc::new(
                        Interface::new(
                            [PortDef::local::<()>("in")],
                            [PortDef::local::<i64>("out")],
                        )
                        .unwrap(),
                    ),
                    Arc::new(move || Emits(factory_code.clone(), factory_drops.clone(), unknown)),
                )
                .unwrap();
            graph
                .expose_input(
                    "in",
                    node.input::<()>("in").unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            let compiled = graph.build().unwrap().compile().unwrap();
            let mut session = compiled.start_with_options(SessionOptions {
                lease: Some(code),
                request_queue_policy: queue,
                request_failure_policy: RequestFailurePolicy::RestartAffectedProcess,
                ..Default::default()
            });
            let request = session.begin_request(None).unwrap();
            session.inject_request_local(request, "in", ()).unwrap();
            let (_, job) = session.next_job().unwrap();
            let metadata = job.metadata().unwrap().clone();
            let ticket = job.ticket();
            session.complete_job(job.run()).unwrap();
            let failure = session.request_failure(request).unwrap().unwrap().clone();
            assert_eq!(failure.cleanup().len(), 1);
            let secondary = failure.cleanup()[0].clone();
            assert_eq!(
                secondary.source().unwrap().downcast_ref(),
                Some(&RuntimeError::DestructorPanicked {
                    stage: CleanupStage::Failure
                })
            );
            assert!(
                matches!(failure.source().unwrap().downcast_ref::<RuntimeError>(), Some(RuntimeError::UnknownOutput { .. }) if unknown)
                    || matches!(failure.source().unwrap().downcast_ref::<RuntimeError>(), Some(RuntimeError::EmissionType { .. }) if !unknown)
            );
            for failure in [&failure, &secondary] {
                let context = failure.context();
                assert_eq!(context.session, Some(metadata.session));
                assert_eq!(context.generation, Some(metadata.generation));
                assert_eq!(context.request, Some(request));
                assert_eq!(context.process, Some(metadata.process));
                assert_eq!(context.path.as_deref(), Some("emit"));
                assert_eq!(context.ticket, Some(ticket));
                assert_eq!(
                    context.output.as_deref(),
                    Some(if unknown { "missing" } else { "out" })
                );
            }
            assert_eq!(secondary.context().phase, Some(CallbackPhase::Cleanup));
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            let disposal = session.request_disposal(request).unwrap();
            session.cancel_request(request).unwrap();
            assert_eq!(session.request_disposal(request).unwrap(), disposal);
            session.release_request(request).unwrap();
            drop(session);
            drop(compiled);
            drop(failure);
            assert!(weak.upgrade().is_some());
            drop(secondary);
            assert!(weak.upgrade().is_none());
        }
    }
}

#[test]
fn late_return_and_inherited_cleanup_update_only_matching_current_failure_slots() {
    use phenix_graph::{
        ComponentDef,
        process::{ActivationJob, DispatchResult, ProcessDispatcher},
        runtime::{CleanupStage, RequestError, RuntimeError, SessionState},
    };
    use std::time::Duration;

    struct Work;
    impl Process for Work {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
            Ok(Step::Yield)
        }
    }
    struct DelayedReturn {
        held: Mutex<Option<ActivationJob>>,
        returned: Mutex<
            Option<(
                phenix_graph::process::StepTicket,
                phenix_graph::runtime::ActivationMetadata,
            )>,
        >,
        reject: bool,
    }
    impl ProcessDispatcher for DelayedReturn {
        fn dispatch(&self, job: ActivationJob) -> DispatchResult {
            match job.metadata().unwrap().path.as_str() {
                "a" => {
                    *self.held.lock().unwrap() = Some(job);
                    DispatchResult::Deferred
                }
                "b" => {
                    *self.returned.lock().unwrap() =
                        Some((job.ticket(), job.metadata().unwrap().clone()));
                    std::thread::sleep(Duration::from_millis(100));
                    if self.reject {
                        DispatchResult::Rejected(job)
                    } else {
                        DispatchResult::Completed(job.run())
                    }
                }
                _ => DispatchResult::Completed(job.run()),
            }
        }
    }
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for policy in [
            RequestFailurePolicy::FailSession,
            RequestFailurePolicy::RestartAffectedProcess,
        ] {
            for reject in [false, true] {
                let code = Arc::new(());
                let weak = Arc::downgrade(&code);
                let drops = Arc::new(AtomicUsize::new(0));
                let mut graph = NetworkBuilder::new();
                let spawns = Arc::new(AtomicUsize::new(0));
                for name in ["a", "b"] {
                    let spawns = spawns.clone();
                    let node = graph
                        .node(
                            name,
                            Arc::new(
                                Interface::new([PortDef::local::<PanicDrop>("in")], []).unwrap(),
                            ),
                            Arc::new(move || {
                                if name == "a" {
                                    assert_eq!(
                                        spawns.fetch_add(1, Ordering::SeqCst),
                                        0,
                                        "recovery fault"
                                    );
                                }
                                Work
                            }),
                        )
                        .unwrap();
                    graph
                        .expose_input(
                            name,
                            node.input::<PanicDrop>("in").unwrap(),
                            NonZeroUsize::new(1).unwrap(),
                        )
                        .unwrap();
                }
                let peer_code = weak.clone();
                let peer_drops = drops.clone();
                let peer = graph
                    .add(
                        "peer",
                        Arc::new(ComponentDef::map(move |_: ()| PanicDrop {
                            lease: peer_code.clone(),
                            drops: peer_drops.clone(),
                        })),
                    )
                    .unwrap();
                graph
                    .expose_input(
                        "peer",
                        peer.input::<()>("input").unwrap(),
                        NonZeroUsize::new(1).unwrap(),
                    )
                    .unwrap();
                graph
                    .expose_output("capture", peer.output::<PanicDrop>("output").unwrap())
                    .unwrap();
                let compiled = graph.build().unwrap().compile().unwrap();
                let dispatcher = Arc::new(DelayedReturn {
                    held: Mutex::new(None),
                    returned: Mutex::new(None),
                    reject,
                });
                let mut session = compiled.start_with_options(SessionOptions {
                    lease: Some(code),
                    dispatcher: dispatcher.clone(),
                    deferred_timeout: Some(Duration::from_millis(50)),
                    request_queue_policy: queue,
                    request_failure_policy: policy,
                    ..Default::default()
                });
                let unrelated = session.begin_request(None).unwrap();
                let unrelated_failure = Failure::application_message("unrelated earlier outcome");
                session
                    .fail_request(unrelated, unrelated_failure.clone())
                    .unwrap();
                let peer = session.begin_request(None).unwrap();
                session.inject_request_local(peer, "peer", ()).unwrap();
                session.close_request(peer).unwrap();
                session.run_until_idle(8);
                let request = session.begin_request(None).unwrap();
                for input in ["a", "b"] {
                    session
                        .inject_request_local(
                            request,
                            input,
                            PanicDrop {
                                lease: weak.clone(),
                                drops: drops.clone(),
                            },
                        )
                        .unwrap();
                }
                session.close_request(request).unwrap();
                assert_eq!(session.run_until_idle(2).state, SessionState::Failed);
                let held = dispatcher.held.lock().unwrap().take().unwrap();
                let (ticket, metadata) = dispatcher.returned.lock().unwrap().take().unwrap();
                assert_eq!(
                    session.request_cancelled_jobs(request).unwrap(),
                    [held.ticket(), ticket]
                );
                assert_eq!(session.request_disposal(request).unwrap().handed_jobs, 2);
                let before = session.failure().unwrap().clone();
                let recovery = usize::from(policy == RequestFailurePolicy::RestartAffectedProcess);
                assert_eq!(before.cleanup().len(), recovery + 1);
                assert!(
                    matches!(before.source().unwrap().downcast_ref::<RuntimeError>(), Some(RuntimeError::DeferredTimeout { ticket, .. }) if *ticket == held.ticket())
                );
                let late = &before.cleanup()[recovery];
                assert_eq!(
                    late.source().unwrap().downcast_ref(),
                    Some(&RuntimeError::DestructorPanicked {
                        stage: CleanupStage::LateReturn
                    })
                );
                assert_eq!(late.context().session, Some(metadata.session));
                assert_eq!(late.context().generation, Some(metadata.generation));
                assert_eq!(late.context().request, Some(request));
                assert_eq!(late.context().process, Some(metadata.process));
                assert_eq!(late.context().path.as_deref(), Some("b"));
                assert_eq!(late.context().ticket, Some(ticket));
                assert_eq!(late.context().phase, Some(CallbackPhase::Cleanup));
                assert_eq!(
                    session
                        .request_failure(request)
                        .unwrap()
                        .unwrap()
                        .cleanup()
                        .len(),
                    before.cleanup().len()
                );

                assert_eq!(
                    session.cancel_request(peer),
                    Err(RequestError::PacketDestructorPanicked)
                );
                let current = session.failure().unwrap();
                let request_failure = session.request_failure(request).unwrap().unwrap();
                assert!(current.same_origin(&before));
                assert!(std::ptr::eq(
                    current.source().unwrap(),
                    before.source().unwrap()
                ));
                assert_eq!(
                    before.cleanup().len(),
                    recovery + 1,
                    "escaped clone changed"
                );
                assert_eq!(current.cleanup().len(), recovery + 2);
                assert_eq!(request_failure.cleanup().len(), current.cleanup().len());
                for (request, session) in request_failure.cleanup().iter().zip(current.cleanup()) {
                    assert!(request.same_origin(session));
                    assert!(std::ptr::eq(
                        request.source().unwrap(),
                        session.source().unwrap()
                    ));
                    assert_eq!(request.context(), session.context());
                }
                assert_eq!(
                    current
                        .cleanup()
                        .last()
                        .unwrap()
                        .source()
                        .unwrap()
                        .downcast_ref(),
                    Some(&RequestError::PacketDestructorPanicked)
                );
                let unrelated_current = session.request_failure(unrelated).unwrap().unwrap();
                assert!(unrelated_current.same_origin(&unrelated_failure));
                assert!(unrelated_current.cleanup().is_empty());
                assert_eq!(
                    session.request_status(request).unwrap(),
                    RequestStatus::Failed
                );
                assert_eq!(session.request_status(peer).unwrap(), RequestStatus::Failed);
                let escaped = request_failure.cleanup().last().unwrap().clone();
                let snapshot = session.explain_request(request, Instant::now()).unwrap();
                let disposal = session.request_disposal(request).unwrap();
                session.cancel_request(request).unwrap();
                assert_eq!(session.request_disposal(request).unwrap(), disposal);
                assert_eq!(
                    session.request_cancelled_jobs(request).unwrap(),
                    [held.ticket(), ticket]
                );
                for id in [request, peer, unrelated] {
                    session.release_request(id).unwrap();
                }
                drop(held);
                assert_eq!(drops.load(Ordering::SeqCst), 3);
                drop(session);
                drop(compiled);
                drop(before);
                assert!(weak.upgrade().is_some());
                drop(escaped);
                assert!(weak.upgrade().is_none());
                let _ = format!("{snapshot:#?}");
            }
        }
    }
}
