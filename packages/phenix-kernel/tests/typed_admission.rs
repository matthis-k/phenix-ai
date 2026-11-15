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
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        Ok(Self::new(i64::decode(bytes)?, &Arc::default()))
    }
}

struct Echo(bool);
impl Process for Echo {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
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
        Arc::new(LocalProcessDispatcher),
        options(),
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
            options(),
            Arc::new(LocalProcessDispatcher),
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
    built.inject_key(key, MoveOnly("kernel".into())).unwrap();
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
            .with_session(|session| session.failure().is_none())
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
