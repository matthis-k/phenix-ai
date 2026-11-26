mod support;

use phenix_graph::{
    ComponentDef, ContractValue, InputKey, Interface, PortDef, Schema, Value,
    compose::{Edit, Footprint, NetworkPatch},
    connection::ConnectionError,
    process::{LocalProcessDispatcher, Process, ProcessContext, ProcessEmission, Step},
};
use phenix_kernel::{
    BuildOptions, CallbackError, CandidateArtifacts, Contribution, InputEvent, Kernel, Key,
    Producer, ProducerError, host::KernelHost,
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const INPUT: InputKey<Payload> = InputKey::encoded("public");
struct Payload {
    number: i64,
    encodes: Arc<AtomicUsize>,
    before: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl Payload {
    fn new(number: i64, encodes: &Arc<AtomicUsize>) -> Self {
        Self {
            number,
            encodes: encodes.clone(),
            before: None,
        }
    }
}
impl ContractValue for Payload {
    fn schema() -> Schema {
        i64::schema()
    }
    fn encode(&self) -> Vec<u8> {
        self.encodes.fetch_add(1, Ordering::SeqCst);
        if let Some(before) = &self.before {
            before();
        }
        self.number.to_le_bytes().to_vec()
    }
    fn decode(bytes: &[u8]) -> Result<Self, phenix_graph::Failure> {
        Ok(Self::new(i64::decode(bytes)?, &Arc::default()))
    }
}

struct Echo(bool);
impl Process for Echo {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        if self.0 {
            self.0 = false;
            return Ok(Step::Complete);
        }
        let Some(value) = cx.take("run") else {
            return Ok(Step::Finished);
        };
        self.0 = true;
        Ok(Step::Emit(ProcessEmission::new("value", value)))
    }
}
fn definition(input: PortDef, output: PortDef) -> Arc<ComponentDef> {
    Arc::new(ComponentDef::primitive(
        Arc::new(Interface::new([input], [output]).unwrap()),
        Arc::new(|| Echo(false)),
    ))
}
struct Source {
    polls: Arc<AtomicUsize>,
    encodes: Arc<AtomicUsize>,
}
impl Producer for Source {
    fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
        let number = self.polls.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(Some(InputEvent::new(
            INPUT,
            Payload::new(number as i64, &self.encodes),
        )))
    }
}
struct Fixture {
    definition: Arc<ComponentDef>,
    declared: bool,
    polls: Arc<AtomicUsize>,
    encodes: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
impl Fixture {
    fn wire() -> Self {
        Self {
            definition: definition(
                PortDef::wire::<i64>("run").unwrap(),
                PortDef::wire::<i64>("value").unwrap(),
            ),
            declared: true,
            polls: Arc::default(),
            encodes: Arc::default(),
            drops: Arc::default(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl PluginInstance for Fixture {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let polls = self.polls.clone();
        let encodes = self.encodes.clone();
        Contribution::new(
            "main",
            NetworkPatch::new([
                Edit::Add {
                    path: "worker".into(),
                    definition: self.definition.clone(),
                },
                Edit::ExposeInput {
                    name: "public".into(),
                    targets: vec![support::path("worker", "run")],
                    capacity: support::capacity(),
                },
                Edit::ExposeOutput {
                    name: "result".into(),
                    from: support::path("worker", "value"),
                },
            ]),
        )
        .route(if self.declared {
            vec!["public".into()]
        } else {
            vec![]
        })
        .prepare(move |_| {
            let polls = polls.clone();
            let encodes = encodes.clone();
            let mut artifacts = CandidateArtifacts::default();
            artifacts.activate(move || {
                Ok(Box::new(Source {
                    polls: polls.clone(),
                    encodes: encodes.clone(),
                }))
            })?;
            Ok(artifacts)
        })
        .register(into)
    }
}
fn options() -> BuildOptions {
    let mut options = BuildOptions::default();
    support::grant(
        &mut options,
        Key::new("fixture", "main"),
        [
            Footprint::Component("worker".into()),
            Footprint::InputExport("public".into()),
            Footprint::OutputExport("result".into()),
        ],
    );
    options
}
fn kernel(fixture: Fixture) -> Kernel {
    Kernel::build_with_options(
        "typed",
        [support::snapshot("fixture", fixture)],
        options().with_dispatcher(Arc::new(LocalProcessDispatcher)),
    )
    .unwrap()
}

#[test]
fn real_generation_and_typed_producer_keep_one_encoded_event_across_full_retry() {
    let fixture = Fixture::wire();
    let polls = fixture.polls.clone();
    let encodes = fixture.encodes.clone();
    let drops = fixture.drops.clone();
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("fixture", |plugin: Fixture| Ok(plugin))
        .unwrap();
    let mut host = KernelHost::new(catalog.instance());
    let stage = host.plugins().prepare_load(&key, fixture).unwrap();
    let candidate = host
        .prepare(
            vec![stage],
            options().with_dispatcher(Arc::new(LocalProcessDispatcher)),
            "typed-host",
            |_| Ok(()),
        )
        .unwrap();
    let generation = host.publish(candidate).unwrap();
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    // Generation forwarding converts a distinct Rust type with the same schema.
    let direct_encodes = Arc::new(AtomicUsize::new(0));
    generation
        .inject_key(INPUT, Payload::new(99, &direct_encodes))
        .unwrap();
    assert_eq!(direct_encodes.load(Ordering::SeqCst), 1);
    generation.run_steps(32).unwrap();
    let direct = generation
        .with_session(|session| session.take_output("result").unwrap().unwrap())
        .unwrap();
    assert_eq!(direct.value().bytes(), 99_i64.to_le_bytes());
    drop(direct);

    let events = generation.drain(10).unwrap();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|event| event.result.is_ok()));
    assert_eq!(polls.load(Ordering::SeqCst), 3);
    assert_eq!(encodes.load(Ordering::SeqCst), 3);
    assert!(generation.drain(100).unwrap().is_empty());
    assert_eq!(polls.load(Ordering::SeqCst), 3);
    assert_eq!(encodes.load(Ordering::SeqCst), 3);
    generation.run_steps(32).unwrap();
    for expected in [1_i64, 2] {
        let packet = generation
            .with_session(|session| session.take_output("result").unwrap().unwrap())
            .unwrap();
        assert_eq!(packet.value().bytes(), expected.to_le_bytes());
        assert_eq!(packet.metadata().unwrap().request(), None);
    }
    assert!(generation.drain(1).unwrap().pop().unwrap().result.is_ok());
    assert_eq!(polls.load(Ordering::SeqCst), 3);
    assert_eq!(encodes.load(Ordering::SeqCst), 3);
    generation.run_steps(32).unwrap();
    let packet = generation
        .with_session(|session| session.take_output("result").unwrap().unwrap())
        .unwrap();
    assert_eq!(packet.value().schema(), &i64::schema().identity());
    assert_eq!(packet.value().bytes(), 3_i64.to_le_bytes());
    drop(host);
    drop(generation);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(packet);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn producer_keys_do_not_grant_routes_and_admission_errors_retain_code() {
    let mut fixture = Fixture::wire();
    fixture.declared = false;
    let encodes = fixture.encodes.clone();
    let mut built = kernel(fixture);
    built.start().unwrap();
    let error = built.drain(1).unwrap().pop().unwrap().result.unwrap_err();
    assert!(matches!(error, ProducerError::UndeclaredRoute(ref name) if name == "public"));
    assert_eq!(encodes.load(Ordering::SeqCst), 1);
    assert_eq!(built.run_steps(32).unwrap().publications, 0);
    drop(built);

    for wrong_type in [false, true] {
        let mut fixture = Fixture::wire();
        if wrong_type {
            fixture.definition = support::function(|value| value);
        }
        let encodes = fixture.encodes.clone();
        let drops = fixture.drops.clone();
        let mut built = kernel(fixture);
        built.start().unwrap();
        if !wrong_type {
            built
                .with_session(|session| session.close_input("public").unwrap())
                .unwrap();
        }
        let error = built.drain(1).unwrap().pop().unwrap().result.unwrap_err();
        let ProducerError::Admission(error) = error else {
            panic!("expected admission failure")
        };
        assert_eq!(
            error.reason,
            if wrong_type {
                ConnectionError::WrongType
            } else {
                ConnectionError::WriterClosed
            }
        );
        assert_eq!(error.value.bytes(), 1_i64.to_le_bytes());
        assert_eq!(error.value.schema(), &i64::schema().identity());
        assert_eq!(encodes.load(Ordering::SeqCst), 1);
        drop(built);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(error);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn kernel_codec_precedes_ownership_checks_and_can_reenter_the_same_kernel() {
    let encodes = Arc::new(AtomicUsize::new(0));
    let mut built = kernel(Fixture::wire());
    let error = built
        .inject_key(INPUT, Payload::new(1, &encodes))
        .unwrap_err();
    assert_eq!(error.reason, ConnectionError::Cancelled);
    assert_eq!(error.value.bytes(), 1_i64.to_le_bytes());
    built.start().unwrap();
    let built = Arc::new(built);
    let owner = built.clone();
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        owner
            .with_session(|_| {
                entered.send(()).unwrap();
                released.recv().unwrap();
            })
            .unwrap()
    });
    ready.recv().unwrap();
    let error = built
        .inject_key(INPUT, Payload::new(2, &encodes))
        .unwrap_err();
    // Release before assertions, so an assertion failure cannot strand the holder.
    release.send(()).unwrap();
    holder.join().unwrap();
    assert_eq!(error.reason, ConnectionError::WouldBlock);
    assert_eq!(error.value.bytes(), 2_i64.to_le_bytes());
    assert_eq!(encodes.load(Ordering::SeqCst), 2);
    built.inject("public", error.value).unwrap();

    built
        .with_session(|_| {
            let error = built
                .inject_key(INPUT, Payload::new(3, &encodes))
                .unwrap_err();
            assert_eq!(error.reason, ConnectionError::Reentrant);
            assert_eq!(error.value.bytes(), 3_i64.to_le_bytes());
            assert_eq!(encodes.load(Ordering::SeqCst), 3);
        })
        .unwrap();

    let inner = built.clone();
    let mut payload = Payload::new(4, &encodes);
    payload.before = Some(Arc::new(move || {
        // If conversion held an outer with_session/active guard, both fail.
        inner.with_session(|_| ()).unwrap();
        let error = inner.inject("missing", Value::local(0_i64)).unwrap_err();
        assert_eq!(error.reason, ConnectionError::InvalidDestination);
        let nested_encodes = Arc::new(AtomicUsize::new(0));
        let error = inner
            .inject_key(
                InputKey::<Payload>::encoded("missing"),
                Payload::new(0, &nested_encodes),
            )
            .unwrap_err();
        assert_eq!(error.reason, ConnectionError::InvalidDestination);
        assert_eq!(nested_encodes.load(Ordering::SeqCst), 1);
    }));
    built.inject_key(INPUT, payload).unwrap();
    assert_eq!(encodes.load(Ordering::SeqCst), 4);
    let error = built
        .inject_key(
            InputKey::<Payload>::encoded("missing"),
            Payload::new(5, &encodes),
        )
        .unwrap_err();
    assert_eq!(error.reason, ConnectionError::InvalidDestination);
    assert_eq!(error.value.bytes(), 5_i64.to_le_bytes());
    assert_eq!(encodes.load(Ordering::SeqCst), 5);
}

#[test]
fn local_move_only_payloads_and_explicit_envelope_leases_need_no_codec() {
    struct MoveOnly(String);
    let mut fixture = Fixture::wire();
    fixture.definition = definition(
        PortDef::local::<MoveOnly>("run"),
        PortDef::local::<MoveOnly>("value"),
    );
    let drops = fixture.drops.clone();
    let mut built = kernel(fixture);
    built.start().unwrap();
    let key = InputKey::<MoveOnly>::new("public");
    let ingress = built.compiled().bind_ingress("public", key).unwrap();
    built
        .inject_bound(&ingress.clone(), MoveOnly("kernel".into()))
        .unwrap();
    drop(ingress);
    let event = InputEvent::new(key, MoveOnly("event".into()));
    built.inject(&event.input, event.value).unwrap();
    built.run_steps(32).unwrap();
    let packet = built
        .with_session(|session| {
            let first = session.take_output("result").unwrap().unwrap();
            assert_eq!(first.value().borrow::<MoveOnly>().unwrap().0, "kernel");
            session.take_output("result").unwrap().unwrap()
        })
        .unwrap();
    assert_eq!(packet.value().borrow::<MoveOnly>().unwrap().0, "event");
    let lease = Arc::new(());
    let weak = Arc::downgrade(&lease);
    let error = built
        .inject(
            "missing",
            key.value(MoveOnly("rejected".into())).with_lease(lease),
        )
        .unwrap_err();
    assert_eq!(error.reason, ConnectionError::InvalidDestination);
    assert_eq!(error.value.borrow::<MoveOnly>().unwrap().0, "rejected");
    drop(built);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert!(weak.upgrade().is_some());
    drop(packet);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    // Rejected raw values retain explicit leases, but infer no kernel lease.
    assert!(weak.upgrade().is_some());
    drop(error);
    assert!(weak.upgrade().is_none());
}

#[test]
fn panicking_codec_is_not_contained_and_does_not_own_the_session() {
    let mut built = kernel(Fixture::wire());
    built.start().unwrap();
    let encodes = Arc::new(AtomicUsize::new(0));
    let mut payload = Payload::new(1, &encodes);
    payload.before = Some(Arc::new(|| panic!("codec side effect")));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || built.inject_key(INPUT, payload)
        ))
        .is_err()
    );
    assert_eq!(encodes.load(Ordering::SeqCst), 1);
    built.inject_key(INPUT, Payload::new(2, &encodes)).unwrap();
    assert_eq!(encodes.load(Ordering::SeqCst), 2);
    assert!(
        built
            .with_session(|session| session
                .failure()
                .map(phenix_graph::Failure::summary)
                .is_none())
            .unwrap()
    );
}

#[test]
fn blocking_conversion_does_not_acquire_kernel_session_ownership() {
    let mut built = kernel(Fixture::wire());
    built.start().unwrap();
    let built = Arc::new(built);
    let encodes = Arc::new(AtomicUsize::new(0));
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let released = std::sync::Mutex::new(released);
    let mut payload = Payload::new(7, &encodes);
    payload.before = Some(Arc::new(move || {
        entered.send(()).unwrap();
        released.lock().unwrap().recv().unwrap();
    }));
    let caller = built.clone();
    let thread = std::thread::spawn(move || caller.inject_key(INPUT, payload));
    ready.recv().unwrap();
    // The codec is blocked, but it has not acquired an admission guard or lease.
    let available = built.with_session(|_| ());
    release.send(()).unwrap();
    thread.join().unwrap().unwrap();
    available.unwrap();
    assert_eq!(encodes.load(Ordering::SeqCst), 1);
}

#[test]
fn bound_conversion_precedes_session_access_and_preserves_refused_value_leases() {
    use phenix_graph::runtime::RequestError;
    let mut built = kernel(Fixture::wire());
    let ingress = built.compiled().bind_ingress("public", INPUT).unwrap();
    let capture = built.compiled().capture("result").unwrap();
    let encodes = Arc::new(AtomicUsize::new(0));
    let lease = Arc::new(());
    let weak = Arc::downgrade(&lease);
    let ingress = ingress.with_lease(lease);
    let error = built
        .inject_bound(&ingress, Payload::new(1, &encodes))
        .unwrap_err();
    assert_eq!(error.reason, ConnectionError::Cancelled);
    assert_eq!(encodes.load(Ordering::SeqCst), 1);
    drop(error);
    built.start().unwrap();
    let built = Arc::new(built);
    let request = built.begin_bound_request(&capture).unwrap().unwrap();
    let inner = built.clone();
    let mut payload = Payload::new(2, &encodes);
    payload.before = Some(Arc::new(move || {
        inner.with_session(|_| ()).unwrap();
    }));
    built
        .inject_bound_request(request, &ingress, payload)
        .unwrap();
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let owner = built.clone();
    let holder = std::thread::spawn(move || {
        owner
            .with_session(|_| {
                entered.send(()).unwrap();
                released.recv().unwrap();
            })
            .unwrap()
    });
    ready.recv().unwrap();
    let ordinary = built
        .inject_bound(&ingress, Payload::new(3, &encodes))
        .unwrap_err();
    let scoped = built
        .inject_bound_request(request, &ingress, Payload::new(4, &encodes))
        .unwrap_err();
    release.send(()).unwrap();
    holder.join().unwrap();
    assert_eq!(ordinary.reason, ConnectionError::WouldBlock);
    assert_eq!(
        scoped.reason,
        RequestError::Transport(ConnectionError::WouldBlock)
    );
    assert_eq!(ordinary.value.bytes(), 3_i64.to_le_bytes());
    assert_eq!(scoped.value.bytes(), 4_i64.to_le_bytes());
    built
        .with_session(|_| {
            assert_eq!(
                built
                    .inject_bound_value(&ingress, Value::of(5_i64))
                    .unwrap_err()
                    .reason,
                ConnectionError::Reentrant
            );
            assert_eq!(
                built
                    .inject_bound_request_value(request, &ingress, Value::of(6_i64))
                    .unwrap_err()
                    .reason,
                RequestError::Transport(ConnectionError::Reentrant)
            );
            assert_eq!(
                built.begin_bound_request(&capture),
                Err(phenix_kernel::SessionAccessError::Reentrant)
            );
        })
        .unwrap();
    built.run_steps(32).unwrap();
    built
        .take_bound_request_output(request, &capture)
        .unwrap()
        .unwrap()
        .unwrap();
    built
        .with_session(|session| session.close_request(request).unwrap())
        .unwrap();
    // Retry the exact constructed bytes through raw bound admission.
    built.inject_bound_value(&ingress, ordinary.value).unwrap();
    assert_eq!(encodes.load(Ordering::SeqCst), 4);
    built.run_steps(32).unwrap();
    built.take_bound_output(&capture).unwrap().unwrap().unwrap();
    let foreign = kernel(Fixture::wire());
    let rejected = foreign
        .inject_bound_value(&ingress, scoped.value)
        .unwrap_err();
    assert_eq!(rejected.reason, ConnectionError::ForeignTopology);
    drop((built, foreign, capture, ingress));
    assert!(weak.upgrade().is_some());
    assert_eq!(rejected.value.bytes(), 4_i64.to_le_bytes());
    drop(rejected);
    assert!(weak.upgrade().is_none());
}

#[test]
fn bound_blocking_and_panicking_conversion_never_owns_the_kernel_session() {
    let mut built = kernel(Fixture::wire());
    built.start().unwrap();
    let built = Arc::new(built);
    let ingress = built.compiled().bind_ingress("public", INPUT).unwrap();
    let encodes = Arc::new(AtomicUsize::new(0));
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let released = std::sync::Mutex::new(released);
    let mut payload = Payload::new(7, &encodes);
    payload.before = Some(Arc::new(move || {
        entered.send(()).unwrap();
        released.lock().unwrap().recv().unwrap();
    }));
    let caller = built.clone();
    let input = ingress.clone();
    let thread = std::thread::spawn(move || caller.inject_bound(&input, payload));
    ready.recv().unwrap();
    let available = built.with_session(|_| ());
    release.send(()).unwrap();
    thread.join().unwrap().unwrap();
    available.unwrap();
    let mut payload = Payload::new(8, &encodes);
    payload.before = Some(Arc::new(|| panic!("bound codec panic")));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || built.inject_bound(&ingress, payload)
        ))
        .is_err()
    );
    built
        .with_session(|session| assert!(session.failure().is_none()))
        .unwrap();
    assert_eq!(encodes.load(Ordering::SeqCst), 2);
}

#[test]
fn bound_full_retry_preserves_constructed_bytes_without_reencoding() {
    use phenix_graph::runtime::{RequestFailurePolicy, RequestQueuePolicy};
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for failure in [
            RequestFailurePolicy::FailSession,
            RequestFailurePolicy::RestartAffectedProcess,
        ] {
            let mut options = options();
            options.execution.request_queue_policy = queue;
            options.execution.request_failure_policy = failure;
            let mut built = Kernel::build_with_options(
                "full",
                [support::snapshot("fixture", Fixture::wire())],
                options.with_dispatcher(Arc::new(LocalProcessDispatcher)),
            )
            .unwrap();
            built.start().unwrap();
            let ingress = built.compiled().bind_ingress("public", INPUT).unwrap();
            let capture = built.compiled().capture("result").unwrap();
            let encodes = Arc::new(AtomicUsize::new(0));
            built
                .inject_bound(&ingress, Payload::new(1, &encodes))
                .unwrap();
            built
                .inject_bound(&ingress, Payload::new(2, &encodes))
                .unwrap();
            let error = built
                .inject_bound(&ingress, Payload::new(3, &encodes))
                .unwrap_err();
            assert_eq!(error.reason, ConnectionError::Full);
            assert_eq!(error.value.bytes(), 3_i64.to_le_bytes());
            built.run_steps(32).unwrap();
            for expected in [1_i64, 2] {
                let packet = built.take_bound_output(&capture).unwrap().unwrap().unwrap();
                assert_eq!(packet.value().bytes(), expected.to_le_bytes());
            }
            built.inject_bound_value(&ingress, error.value).unwrap();
            assert_eq!(encodes.load(Ordering::SeqCst), 3);
            built.run_steps(32).unwrap();
            assert_eq!(
                built
                    .take_bound_output(&capture)
                    .unwrap()
                    .unwrap()
                    .unwrap()
                    .value()
                    .bytes(),
                3_i64.to_le_bytes()
            );
            assert!(
                built
                    .take_bound_output(&capture)
                    .unwrap()
                    .unwrap()
                    .is_none()
            );
        }
    }
}

#[test]
fn repeated_bound_raw_refusals_retain_stable_owners_without_reencoding() {
    use phenix_graph::runtime::{RequestError, RequestQueuePolicy};

    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for scoped in [false, true] {
            let fixture = Fixture::wire();
            let drops = fixture.drops.clone();
            let mut options = options();
            options.execution.request_queue_policy = queue;
            let mut built = Kernel::build_with_options(
                "repeated-raw-retry",
                [support::snapshot("fixture", fixture)],
                options.with_dispatcher(Arc::new(LocalProcessDispatcher)),
            )
            .unwrap();
            built.start().unwrap();
            let codec = Arc::new(());
            let declaration = Arc::new(());
            let payload_owner = Arc::new(());
            let codec_weak = Arc::downgrade(&codec);
            let declaration_weak = Arc::downgrade(&declaration);
            let payload_weak = Arc::downgrade(&payload_owner);
            let compiled = built.compiled().with_lease(declaration.clone());
            let ingress = compiled
                .bind_ingress("public", INPUT)
                .unwrap()
                .with_lease(codec.clone());
            drop(compiled);
            let capture = built.compiled().capture("result").unwrap();
            let request = scoped.then(|| built.begin_bound_request(None).unwrap().unwrap());
            let encodes = Arc::new(AtomicUsize::new(0));
            for number in [1, 2] {
                if let Some(request) = request {
                    built
                        .inject_bound_request(request, &ingress, Payload::new(number, &encodes))
                        .unwrap();
                } else {
                    built
                        .inject_bound(&ingress, Payload::new(number, &encodes))
                        .unwrap();
                }
            }
            let foreign = kernel(Fixture::wire());
            // Start without handle leases to prove even foreign raw refusal attaches them.
            let value = INPUT
                .value(Payload::new(3, &encodes))
                .with_lease(payload_owner.clone());
            let identity = value.bytes().as_ptr() as usize;
            let counts = || {
                (
                    Arc::strong_count(&codec),
                    Arc::strong_count(&declaration),
                    Arc::strong_count(&payload_owner),
                )
            };
            let before = counts();
            let value = if let Some(request) = request {
                let error = foreign
                    .inject_bound_request_value(request, &ingress, value)
                    .unwrap_err();
                assert_eq!(error.reason, RequestError::Foreign);
                error.value
            } else {
                let error = foreign.inject_bound_value(&ingress, value).unwrap_err();
                assert_eq!(error.reason, ConnectionError::ForeignTopology);
                error.value
            };
            assert_eq!(counts(), (before.0 + 1, before.1 + 1, before.2));
            let retained = counts();
            let retry = |owner: &Kernel, mut value: Value, reason: ConnectionError| {
                for _ in 0..1000 {
                    value = if let Some(request) = request {
                        let error = owner
                            .inject_bound_request_value(request, &ingress, value)
                            .unwrap_err();
                        assert_eq!(
                            error.reason,
                            if reason == ConnectionError::ForeignTopology {
                                RequestError::Foreign
                            } else {
                                RequestError::Transport(reason)
                            }
                        );
                        error.value
                    } else {
                        let error = owner.inject_bound_value(&ingress, value).unwrap_err();
                        assert_eq!(error.reason, reason);
                        error.value
                    };
                    assert_eq!(
                        counts(),
                        retained,
                        "lease growth on {reason:?}, {queue:?}, scoped={scoped}"
                    );
                    assert_eq!(value.bytes().as_ptr() as usize, identity);
                    assert_eq!(value.bytes(), 3_i64.to_le_bytes());
                    assert_eq!(encodes.load(Ordering::SeqCst), 3);
                }
                value
            };
            let value = retry(&foreign, value, ConnectionError::ForeignTopology);
            let value = retry(&built, value, ConnectionError::Full);
            // Another thread sees Busy, not same-thread reentry. Scoped threads
            // cannot strand a holder if a regression assertion panics.
            let value = built
                .with_session(|_| {
                    std::thread::scope(|scope| {
                        let retry = &retry;
                        let built = &built;
                        scope
                            .spawn(move || retry(built, value, ConnectionError::WouldBlock))
                            .join()
                            .unwrap()
                    })
                })
                .unwrap();
            built.run_steps(32).unwrap();
            let take = || {
                if let Some(request) = request {
                    built
                        .take_bound_request_output(request, &capture)
                        .unwrap()
                        .unwrap()
                        .unwrap()
                } else {
                    built.take_bound_output(&capture).unwrap().unwrap().unwrap()
                }
            };
            for number in [1_i64, 2] {
                assert_eq!(take().value().bytes(), number.to_le_bytes());
            }
            if let Some(request) = request {
                built
                    .inject_bound_request_value(request, &ingress, value)
                    .unwrap();
                built
                    .with_session(|session| session.close_request(request).unwrap())
                    .unwrap();
            } else {
                built.inject_bound_value(&ingress, value).unwrap();
            }
            built.run_steps(32).unwrap();
            let escaped = take().into_value();
            assert_eq!(escaped.bytes(), 3_i64.to_le_bytes());
            assert_eq!(encodes.load(Ordering::SeqCst), 3);
            if let Some(request) = request {
                built
                    .with_session(|session| session.release_request(request).unwrap())
                    .unwrap();
            }
            drop((
                built,
                foreign,
                ingress,
                capture,
                codec,
                declaration,
                payload_owner,
            ));
            assert_eq!(drops.load(Ordering::SeqCst), 0);
            assert_eq!(codec_weak.strong_count(), 1);
            assert_eq!(declaration_weak.strong_count(), 1);
            assert_eq!(payload_weak.strong_count(), 1);
            drop(escaped);
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            assert_eq!(codec_weak.strong_count(), 0);
            assert_eq!(declaration_weak.strong_count(), 0);
            assert_eq!(payload_weak.strong_count(), 0);
        }
    }
}
