use phenix_graph::{
    ComponentDef, Interface, PortDef, Value,
    network::{CompiledNetwork, NetworkBuilder, NetworkDef, PortPath},
    process::{Process, ProcessContext, ProcessEmission, Step},
    runtime::{NetworkSession, SessionOptions, SessionState},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}
fn path(process: &str, port: &str) -> PortPath {
    PortPath {
        process: process.into(),
        port: port.into(),
    }
}
#[derive(Clone)]
struct Command {
    value: i64,
    destinations: Option<Vec<PortPath>>,
}
struct Router {
    emitted: bool,
    effects: Arc<AtomicUsize>,
}
impl Process for Router {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let Some(value) = cx.take("command") else {
            assert!(cx.is_eos("command"));
            return Ok(Step::Finished);
        };
        let command = value.into_owned::<Command>()?;
        self.effects.fetch_add(1, Ordering::SeqCst);
        self.emitted = true;
        Ok(Step::Emit(match command.destinations {
            Some(destinations) => {
                ProcessEmission::select("out", Value::local(command.value), destinations)
            }
            None => ProcessEmission::new("out", Value::local(command.value)),
        }))
    }
}
fn add_router(
    builder: &mut NetworkBuilder,
    effects: Arc<AtomicUsize>,
) -> phenix_graph::network::ComponentRef {
    let router = builder
        .node(
            "router",
            Arc::new(
                Interface::new(
                    [PortDef::local::<Command>("command")],
                    [PortDef::local::<i64>("out")],
                )
                .unwrap(),
            ),
            Arc::new(move || Router {
                emitted: false,
                effects: effects.clone(),
            }),
        )
        .unwrap();
    builder
        .expose_input("input", router.input::<Command>("command").unwrap(), one())
        .unwrap();
    router
}
type Deliveries = Arc<Mutex<Vec<(&'static str, i64)>>>;
struct Sink {
    name: &'static str,
    deliveries: Deliveries,
}
impl Process for Sink {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if let Some(value) = cx.take("run") {
            self.deliveries
                .lock()
                .unwrap()
                .push((self.name, *value.borrow::<i64>().unwrap()));
            Ok(Step::Complete)
        } else {
            assert!(cx.is_eos("run"));
            Ok(Step::Finished)
        }
    }
}
fn fixture(capture: bool) -> (CompiledNetwork, Arc<AtomicUsize>, Deliveries) {
    let effects = Arc::new(AtomicUsize::new(0));
    let deliveries = Arc::new(Mutex::new(vec![]));
    let mut builder = NetworkBuilder::new();
    let router = add_router(&mut builder, effects.clone());
    for name in ["a", "b", "c"] {
        let shared = deliveries.clone();
        let sink = builder
            .node(
                name,
                Arc::new(Interface::new([PortDef::local::<i64>("run")], []).unwrap()),
                Arc::new(move || Sink {
                    name,
                    deliveries: shared.clone(),
                }),
            )
            .unwrap();
        if name != "c" {
            builder
                .connect(
                    router.output::<i64>("out").unwrap(),
                    sink.input::<i64>("run").unwrap(),
                    one(),
                )
                .unwrap();
        }
        if name == "a" {
            builder
                .expose_input("alias_a", sink.input::<i64>("run").unwrap(), one())
                .unwrap();
        }
        if name == "c" {
            builder
                .expose_input("c.external", sink.input::<i64>("run").unwrap(), one())
                .unwrap();
        }
    }
    if capture {
        builder
            .expose_output("tap", router.output::<i64>("out").unwrap())
            .unwrap();
    }
    (
        builder.build().unwrap().compile().unwrap(),
        effects,
        deliveries,
    )
}
fn inject(session: &mut NetworkSession, value: i64, destinations: Option<Vec<PortPath>>) {
    session
        .inject_local(
            "input",
            Command {
                value,
                destinations,
            },
        )
        .unwrap();
}
fn take(session: &mut NetworkSession, name: &str) -> Option<i64> {
    session
        .take_output(name)
        .unwrap()
        .map(|packet| *packet.value().borrow::<i64>().unwrap())
}

#[test]
fn default_broadcast_and_explicit_one_many_or_empty_preserve_captures() {
    let (compiled, effects, deliveries) = fixture(true);
    let mut session = compiled.start();
    let cases = [
        (5, None),
        (7, Some(vec![path("a", "run")])),
        (9, Some(vec![path("b", "run"), path("a", "run")])),
        (11, Some(vec![])),
        (13, None),
    ];
    for (value, destinations) in cases {
        inject(&mut session, value, destinations);
        assert_eq!(session.run_until_idle(100).publications, 1);
        assert_eq!(take(&mut session, "tap"), Some(value));
        assert_eq!(session.state(), SessionState::Quiescent);
    }
    assert_eq!(
        *deliveries.lock().unwrap(),
        vec![
            ("a", 5),
            ("b", 5),
            ("a", 7),
            ("a", 9),
            ("b", 9),
            ("a", 13),
            ("b", 13)
        ]
    );
    assert_eq!(effects.load(Ordering::SeqCst), 5);
    assert_eq!(session.discarded_emissions(), 0);
}

#[test]
fn explicit_empty_selection_is_intentional_discard_without_capture_or_outgoing_edges() {
    let effects = Arc::new(AtomicUsize::new(0));
    let mut builder = NetworkBuilder::new();
    add_router(&mut builder, effects.clone());
    let compiled = builder.build().unwrap().compile().unwrap();
    let mut explicit = compiled.start();
    inject(&mut explicit, 1, Some(vec![]));
    let report = explicit.run_until_idle(20);
    assert_eq!(report.state, SessionState::Quiescent);
    assert_eq!(report.publications, 1);
    assert_eq!(report.discarded_emissions, 1);
    let mut implicit = compiled.start();
    inject(&mut implicit, 2, None);
    let report = implicit.run_until_idle(20);
    assert_eq!(report.state, SessionState::Failed);
    assert_eq!(report.publications, 0);
    assert!(implicit.failure().unwrap().contains("unconnected output"));
    assert_eq!(effects.load(Ordering::SeqCst), 2);
    let (connected, _, deliveries) = fixture(false);
    let mut connected = connected.start();
    inject(&mut connected, 3, Some(vec![]));
    assert_eq!(connected.run_until_idle(20).discarded_emissions, 1);
    assert!(deliveries.lock().unwrap().is_empty());
    inject(&mut connected, 4, None);
    assert_eq!(connected.run_until_idle(20).publications, 1);
    assert_eq!(*deliveries.lock().unwrap(), vec![("a", 4), ("b", 4)]);
}

#[test]
fn unknown_duplicate_foreign_and_unconnected_selections_deliver_nothing_atomically() {
    let cases = [
        (
            vec![path("a", "run"), path("missing", "run")],
            "unknown selected input",
        ),
        (
            vec![path("a", "run"), path("b", "missing")],
            "unknown selected input",
        ),
        (
            vec![path("a", "run"), path("a", "run")],
            "duplicate selected input",
        ),
        (
            vec![path("a", "run"), path("foreign/a", "run")],
            "unknown selected input",
        ),
        (vec![path("a", "run"), path("c", "run")], "not connected"),
        (vec![path("router", "out")], "unknown selected input"),
        (vec![path("router", "command")], "not connected"),
        (vec![path("", "input")], "not connected"),
        (
            vec![path("a", "run"), path("", "alias_a")],
            "duplicate selected input",
        ),
    ];
    for (destinations, diagnostic) in cases {
        let (compiled, effects, deliveries) = fixture(true);
        let mut session = compiled.start();
        inject(&mut session, 77, Some(destinations));
        let report = session.run_until_idle(100);
        assert_eq!(report.state, SessionState::Failed);
        assert_eq!(report.publications, 0);
        assert!(
            session.failure().unwrap().contains(diagnostic),
            "{:?}",
            session.failure()
        );
        assert!(deliveries.lock().unwrap().is_empty());
        assert_eq!(take(&mut session, "tap"), None);
        assert_eq!(session.disposed_packets(), 0);
        assert_eq!(effects.load(Ordering::SeqCst), 1);
        assert_eq!(session.run_until_idle(100).steps, 0);
    }
}

#[test]
fn selection_cannot_borrow_an_edge_from_another_output() {
    let effects = Arc::new(AtomicUsize::new(0));
    let shared = effects.clone();
    let deliveries: Deliveries = Arc::new(Mutex::new(vec![]));
    let mut builder = NetworkBuilder::new();
    let router = builder
        .node(
            "router",
            Arc::new(
                Interface::new(
                    [PortDef::local::<Command>("command")],
                    [PortDef::local::<i64>("out"), PortDef::local::<i64>("other")],
                )
                .unwrap(),
            ),
            Arc::new(move || Router {
                emitted: false,
                effects: shared.clone(),
            }),
        )
        .unwrap();
    builder
        .expose_input("input", router.input::<Command>("command").unwrap(), one())
        .unwrap();
    for (name, output) in [("a", "out"), ("b", "other")] {
        let shared = deliveries.clone();
        let sink = builder
            .node(
                name,
                Arc::new(Interface::new([PortDef::local::<i64>("run")], []).unwrap()),
                Arc::new(move || Sink {
                    name,
                    deliveries: shared.clone(),
                }),
            )
            .unwrap();
        builder
            .connect(
                router.output::<i64>(output).unwrap(),
                sink.input::<i64>("run").unwrap(),
                one(),
            )
            .unwrap();
    }
    builder
        .expose_output("tap", router.output::<i64>("out").unwrap())
        .unwrap();
    let mut session = builder.build().unwrap().compile().unwrap().start();
    inject(&mut session, 77, Some(vec![path("b", "run")]));
    assert_eq!(session.run_until_idle(100).state, SessionState::Failed);
    assert!(session.failure().unwrap().contains("not connected"));
    assert!(deliveries.lock().unwrap().is_empty());
    assert_eq!(take(&mut session, "tap"), None);
    assert_eq!(effects.load(Ordering::SeqCst), 1);
}

#[test]
fn oversized_selection_rejects_before_resolution_and_captures_do_not_expand_the_budget() {
    let (compiled, effects, deliveries) = fixture(true);
    let mut session = compiled.start();
    inject(&mut session, 7, Some(vec![path("unknown", "run"); 3]));
    assert_eq!(session.run_until_idle(100).state, SessionState::Failed);
    assert!(
        session
            .failure()
            .unwrap()
            .contains("outgoing connection budget (2)")
    );
    assert!(deliveries.lock().unwrap().is_empty());
    assert_eq!(take(&mut session, "tap"), None);
    assert_eq!(effects.load(Ordering::SeqCst), 1);
}

#[test]
fn erased_paths_resolve_only_existing_edges_and_never_grant_injection_authority() {
    let (compiled, _, deliveries) = fixture(true);
    let mut foreign = NetworkBuilder::new();
    let foreign_sink = foreign
        .node(
            "c",
            Arc::new(Interface::new([PortDef::local::<i64>("run")], []).unwrap()),
            Arc::new(|| Sink {
                name: "foreign",
                deliveries: Arc::new(Mutex::new(vec![])),
            }),
        )
        .unwrap();
    let erased = foreign_sink.input::<i64>("run").unwrap().path().clone();
    assert!(compiled.resolve_input(&erased).is_some());
    let mut session = compiled.start();
    inject(&mut session, 3, Some(vec![erased]));
    assert_eq!(session.run_until_idle(100).state, SessionState::Failed);
    assert!(session.failure().unwrap().contains("not connected"));
    assert!(deliveries.lock().unwrap().is_empty());
    assert_eq!(take(&mut session, "tap"), None);
    let mut allowed = compiled.start();
    inject(&mut allowed, 5, Some(vec![path("", "alias_a")]));
    allowed.run_until_idle(100);
    assert_eq!(*deliveries.lock().unwrap(), vec![("a", 5)]);
    assert_eq!(take(&mut allowed, "tap"), Some(5));
}

struct Echo(bool);
impl Process for Echo {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.0 {
            self.0 = false;
            return Ok(Step::Complete);
        }
        let Some(value) = cx.take("run") else {
            assert!(cx.is_eos("run"));
            return Ok(Step::Finished);
        };
        self.0 = true;
        Ok(Step::Emit(ProcessEmission::new("out", value)))
    }
}
fn nested_module() -> NetworkDef {
    let mut inner = NetworkBuilder::new();
    for name in ["left", "right"] {
        let p = inner
            .node(
                name,
                Arc::new(
                    Interface::new(
                        [PortDef::local::<i64>("run")],
                        [PortDef::local::<i64>("out")],
                    )
                    .unwrap(),
                ),
                Arc::new(|| Echo(false)),
            )
            .unwrap();
        for alias in ["all", "alias"] {
            inner
                .expose_input(alias, p.input::<i64>("run").unwrap(), one())
                .unwrap();
        }
        if name == "left" {
            inner
                .expose_input("one", p.input::<i64>("run").unwrap(), one())
                .unwrap();
        }
        inner
            .expose_output(name, p.output::<i64>("out").unwrap())
            .unwrap();
    }
    let mut outer = NetworkBuilder::new();
    let child = outer.module("inner", inner.build().unwrap()).unwrap();
    for (name, inner_name) in [("public", "all"), ("alias", "alias"), ("one", "one")] {
        outer
            .expose_input(name, child.input::<i64>(inner_name).unwrap(), one())
            .unwrap();
    }
    for name in ["left", "right"] {
        outer
            .expose_output(name, child.output::<i64>(name).unwrap())
            .unwrap();
    }
    outer.build().unwrap()
}
fn nested_fixture(partial: bool) -> (CompiledNetwork, Arc<AtomicUsize>) {
    let effects = Arc::new(AtomicUsize::new(0));
    let mut root = NetworkBuilder::new();
    let router = add_router(&mut root, effects.clone());
    let definition = Arc::new(ComponentDef::composite(nested_module()));
    for name in ["first", "second"] {
        let module = root.add(name, definition.clone()).unwrap();
        if !partial || name == "first" {
            root.connect(
                router.output::<i64>("out").unwrap(),
                module
                    .input::<i64>(if partial { "one" } else { "public" })
                    .unwrap(),
                one(),
            )
            .unwrap();
        }
        for port in ["left", "right"] {
            root.expose_output(
                format!("{name}.{port}"),
                module.output::<i64>(port).unwrap(),
            )
            .unwrap();
        }
    }
    root.expose_output("tap", router.output::<i64>("out").unwrap())
        .unwrap();
    (root.build().unwrap().compile().unwrap(), effects)
}

#[test]
fn nested_repeated_module_aliases_select_their_flattened_fanout_without_wrapper_execution() {
    let (compiled, effects) = nested_fixture(false);
    let resolved = compiled.resolve_input(&path("first", "public")).unwrap();
    assert_eq!(
        resolved
            .iter()
            .map(|(index, port)| (compiled.processes()[*index].path(), port.as_str()))
            .collect::<Vec<_>>(),
        vec![("first/inner/left", "run"), ("first/inner/right", "run")]
    );
    assert_eq!(compiled.processes().len(), 5);
    let mut session = compiled.start();
    for (value, destination, expected) in [
        (
            7,
            path("first", "public"),
            vec!["first.left", "first.right"],
        ),
        (
            9,
            path("second/inner", "all"),
            vec!["second.left", "second.right"],
        ),
        (11, path("first/inner/left", "run"), vec!["first.left"]),
    ] {
        inject(&mut session, value, Some(vec![destination]));
        session.run_until_idle(100);
        for output in ["first.left", "first.right", "second.left", "second.right"] {
            assert_eq!(
                take(&mut session, output),
                expected.contains(&output).then_some(value)
            );
        }
        assert_eq!(take(&mut session, "tap"), Some(value));
    }
    assert_eq!(effects.load(Ordering::SeqCst), 3);
}

#[test]
fn alias_overlap_and_partially_connected_alias_fanout_reject_before_any_delivery() {
    for partial in [false, true] {
        let (compiled, effects) = nested_fixture(partial);
        let mut session = compiled.start();
        let destinations = if partial {
            vec![path("first", "public")]
        } else {
            vec![path("first", "public"), path("first/inner", "alias")]
        };
        inject(&mut session, 77, Some(destinations));
        assert_eq!(session.run_until_idle(100).state, SessionState::Failed);
        assert!(session.failure().unwrap().contains(if partial {
            "not connected"
        } else {
            "duplicate selected input"
        }));
        for output in [
            "tap",
            "first.left",
            "first.right",
            "second.left",
            "second.right",
        ] {
            assert_eq!(take(&mut session, output), None);
        }
        assert_eq!(effects.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn capacity_one_retry_retains_selected_fanout_and_never_repeats_action_effects() {
    let (compiled, effects, deliveries) = fixture(true);
    let mut session = compiled.start_with_options(SessionOptions {
        capture_capacity: one(),
        ..Default::default()
    });
    inject(&mut session, 1, Some(vec![]));
    session.run_until_idle(100);
    inject(&mut session, 2, Some(vec![path("a", "run")]));
    assert_eq!(session.run_until_idle(100).state, SessionState::Waiting);
    inject(&mut session, 3, Some(vec![path("b", "run")]));
    assert!(
        session
            .inject_local(
                "input",
                Command {
                    value: 4,
                    destinations: None
                }
            )
            .is_err()
    );
    for _ in 0..3 {
        assert_eq!(session.run_until_idle(100).steps, 0);
    }
    assert_eq!(effects.load(Ordering::SeqCst), 2);
    assert!(deliveries.lock().unwrap().is_empty());
    assert_eq!(session.blocked_queues().len(), 1);
    assert_eq!(take(&mut session, "tap"), Some(1));
    assert_eq!(session.run_until_idle(100).publications, 1);
    assert_eq!(*deliveries.lock().unwrap(), vec![("a", 2)]);
    assert_eq!(take(&mut session, "tap"), Some(2));
    assert_eq!(session.run_until_idle(100).publications, 1);
    assert_eq!(*deliveries.lock().unwrap(), vec![("a", 2), ("b", 3)]);
    assert_eq!(take(&mut session, "tap"), Some(3));
    assert_eq!(effects.load(Ordering::SeqCst), 3);
}

#[test]
fn an_unselected_full_connection_does_not_block_selected_delivery() {
    let (compiled, effects, deliveries) = fixture(false);
    let mut session = compiled.start();
    inject(&mut session, 1, Some(vec![path("b", "run")]));
    let (router, job) = session.next_job().unwrap();
    session.complete(router, job.run()).unwrap();
    let (sink, held) = session.next_job().unwrap();
    assert_eq!(held.metadata().unwrap().path, "b");
    session.run_until_idle(100);
    inject(&mut session, 2, Some(vec![path("b", "run")]));
    session.run_until_idle(100);
    inject(&mut session, 3, Some(vec![path("a", "run")]));
    assert_eq!(session.run_until_idle(100).publications, 1);
    assert_eq!(*deliveries.lock().unwrap(), vec![("a", 3)]);
    session.complete(sink, held.run()).unwrap();
    session.run_until_idle(100);
    assert_eq!(
        *deliveries.lock().unwrap(),
        vec![("a", 3), ("b", 1), ("b", 2)]
    );
    assert_eq!(effects.load(Ordering::SeqCst), 3);
}

#[test]
fn later_invalid_selection_does_not_roll_back_an_earlier_committed_emission() {
    let (compiled, effects, deliveries) = fixture(true);
    let mut session = compiled.start();
    inject(&mut session, 1, None);
    assert_eq!(session.run_until_idle(100).publications, 1);
    inject(
        &mut session,
        2,
        Some(vec![path("b", "run"), path("unknown", "run")]),
    );
    let failed = session.run_until_idle(100);
    assert_eq!(failed.state, SessionState::Failed);
    assert_eq!(failed.publications, 0);
    assert_eq!(*deliveries.lock().unwrap(), vec![("a", 1), ("b", 1)]);
    assert_eq!(take(&mut session, "tap"), Some(1));
    assert_eq!(take(&mut session, "tap"), None);
    assert_eq!(effects.load(Ordering::SeqCst), 2);
}

struct SequenceRouter {
    phase: usize,
    effects: Arc<AtomicUsize>,
}
impl Process for SequenceRouter {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        self.phase += 1;
        self.effects.fetch_add(1, Ordering::SeqCst);
        Ok(Step::Emit(ProcessEmission::select(
            "out",
            Value::local(self.phase as i64),
            if self.phase == 1 {
                vec![path("a", "run")]
            } else {
                vec![path("a", "run"), path("unknown", "run")]
            },
        )))
    }
}
#[test]
fn committed_progress_within_one_activation_survives_a_later_invalid_selection() {
    let effects = Arc::new(AtomicUsize::new(0));
    let shared = effects.clone();
    let deliveries: Deliveries = Arc::new(Mutex::new(vec![]));
    let mut b = NetworkBuilder::new();
    let router = b
        .node(
            "router",
            Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap()),
            Arc::new(move || SequenceRouter {
                phase: 0,
                effects: shared.clone(),
            }),
        )
        .unwrap();
    b.start(&router).unwrap();
    for name in ["a", "b"] {
        let shared = deliveries.clone();
        let sink = b
            .node(
                name,
                Arc::new(Interface::new([PortDef::local::<i64>("run")], []).unwrap()),
                Arc::new(move || Sink {
                    name,
                    deliveries: shared.clone(),
                }),
            )
            .unwrap();
        b.connect(
            router.output::<i64>("out").unwrap(),
            sink.input::<i64>("run").unwrap(),
            one(),
        )
        .unwrap();
    }
    b.expose_output("tap", router.output::<i64>("out").unwrap())
        .unwrap();
    let mut session = b.build().unwrap().compile().unwrap().start();
    let report = session.run_until_idle(100);
    assert_eq!(report.state, SessionState::Failed);
    assert_eq!(report.publications, 1);
    assert_eq!(*deliveries.lock().unwrap(), vec![("a", 1)]);
    let packet = session.take_output("tap").unwrap().unwrap();
    assert_eq!(packet.value().borrow::<i64>(), Some(&1));
    assert_eq!(packet.metadata().unwrap().activation(), 1);
    assert_eq!(take(&mut session, "tap"), None);
    assert_eq!(effects.load(Ordering::SeqCst), 2);
    assert_eq!(session.run_until_idle(100).steps, 0);
}
