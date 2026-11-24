//! Bounded native-only evidence: no alternate scheduler or benchmark dependency.
use phenix_graph::{
    ComponentDef, GraphOrigin, Interface, NetworkBuilder, PortDef, Process, ProcessContext,
    ProcessEmission, Step, Value,
    compose::{ComposeError, Composer, Edit, Footprint, NetworkPatch, Registration},
    connection::{ConnectionError, ConnectionQueue, ConnectionSet, FanoutTarget, Packet},
    runtime::{Readiness, SessionOptions, SessionState},
};
use std::{
    collections::BTreeSet,
    hint::black_box,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

const ITEMS: usize = 1_000;
#[path = "support/composition_cases.rs"]
mod composition_cases;
#[path = "support/planning_policy.rs"]
mod planning_policy;
fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}
fn interface(inputs: &[&str], outputs: &[&str]) -> Arc<Interface> {
    Arc::new(
        Interface::new(
            inputs.iter().map(|p| PortDef::local::<i64>(*p)),
            outputs.iter().map(|p| PortDef::local::<i64>(*p)),
        )
        .unwrap(),
    )
}

fn pipeline() {
    let seen = Arc::new(AtomicUsize::new(0));
    let sum = Arc::new(AtomicUsize::new(0));
    let mut b = NetworkBuilder::new();
    // The repeat count belongs to the source iterator, not an alternate executor.
    let source = b
        .add(
            "source",
            Arc::new(ComponentDef::source(|| std::iter::repeat_n(1_i64, ITEMS))),
        )
        .unwrap();
    let map = b
        .add("map", Arc::new(ComponentDef::map(|v: i64| v + 1)))
        .unwrap();
    let (count, total) = (seen.clone(), sum.clone());
    let sink = b
        .add(
            "sink",
            Arc::new(ComponentDef::sink(move |v: &i64| {
                count.fetch_add(1, Ordering::Relaxed);
                total.fetch_add(*v as usize, Ordering::Relaxed);
            })),
        )
        .unwrap();
    b.start(&source).unwrap();
    b.connect(
        source.output::<i64>("output").unwrap(),
        map.input::<i64>("input").unwrap(),
        one(),
    )
    .unwrap();
    b.connect(
        map.output::<i64>("output").unwrap(),
        sink.input::<i64>("input").unwrap(),
        one(),
    )
    .unwrap();
    let mut s = b.build().unwrap().compile().unwrap().start();
    assert_eq!(s.run_until_idle(ITEMS * 10).state, SessionState::Completed);
    assert_eq!(seen.load(Ordering::Relaxed), ITEMS);
    assert_eq!(sum.load(Ordering::Relaxed), ITEMS * 2);
    black_box(sum);
}

fn fanout_retry() {
    let preparations = Arc::new(AtomicUsize::new(0));
    let mut queues = ConnectionSet::new(
        (0..2)
            .map(|_| ConnectionQueue::new(one(), Value::local(0_i64).identity()))
            .collect(),
    );
    let targets: Vec<_> = (0..2)
        .map(|connection| {
            let count = preparations.clone();
            FanoutTarget {
                connection,
                preparation: Some(Arc::new(move |v: &Value| {
                    count.fetch_add(1, Ordering::Relaxed);
                    Ok(v.clone())
                })),
            }
        })
        .collect();
    let mut deliveries = [0; 2];
    let mut effects = 0;
    for n in 0..ITEMS {
        effects += 1;
        let mut tx = queues
            .prepare(&Packet::new(Value::local(n as i64)), &targets)
            .unwrap();
        if n != 0 {
            // Fast reader drained first; slow reader still holds the previous item.
            for _ in 0..8 {
                assert_eq!(queues.commit(&mut tx), Err(ConnectionError::Full));
            }
            assert!(queues.pop(0).is_none());
            assert_eq!(
                queues.pop(1).unwrap().value().borrow::<i64>(),
                Some(&((n - 1) as i64))
            );
            deliveries[1] += 1;
        }
        queues.commit(&mut tx).unwrap();
        assert_eq!(
            queues.pop(0).unwrap().value().borrow::<i64>(),
            Some(&(n as i64))
        );
        deliveries[0] += 1;
    }
    assert_eq!(
        queues.pop(1).unwrap().value().borrow::<i64>(),
        Some(&((ITEMS - 1) as i64))
    );
    deliveries[1] += 1;
    assert_eq!(deliveries, [ITEMS; 2]);
    assert_eq!(effects, ITEMS);
    assert_eq!(preparations.load(Ordering::Relaxed), ITEMS * 2);
}

struct Split {
    phase: u8,
    value: i64,
}
impl Process for Split {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        if self.phase == 2 {
            self.phase = 0;
            return Ok(Step::Complete);
        }
        if self.phase == 0 {
            self.value = *cx.take("in").unwrap().borrow::<i64>().unwrap();
        }
        self.phase += 1;
        Ok(Step::Emit(ProcessEmission::new(
            if self.phase == 1 { "a" } else { "b" },
            Value::local(self.value * self.phase as i64),
        )))
    }
}
struct Join {
    count: Arc<AtomicUsize>,
    sum: i64,
}
impl Process for Join {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        let a = *cx.take("a").unwrap().borrow::<i64>().unwrap();
        let b = *cx.take("b").unwrap().borrow::<i64>().unwrap();
        assert_eq!(b, a * 2);
        self.sum += a + b;
        let n = self.count.fetch_add(1, Ordering::Relaxed) + 1;
        assert_eq!(self.sum, 3 * n as i64);
        Ok(Step::Complete)
    }
}
fn split_join() {
    let count = Arc::new(AtomicUsize::new(0));
    let shared = count.clone();
    let mut b = NetworkBuilder::new();
    let split = b
        .node(
            "split",
            interface(&["in"], &["a", "b"]),
            Arc::new(|| Split { phase: 0, value: 0 }),
        )
        .unwrap();
    let join = b
        .node(
            "join",
            interface(&["a", "b"], &[]),
            Arc::new(move || Join {
                count: shared.clone(),
                sum: 0,
            }),
        )
        .unwrap();
    for p in ["a", "b"] {
        b.connect(
            split.output::<i64>(p).unwrap(),
            join.input::<i64>(p).unwrap(),
            one(),
        )
        .unwrap();
    }
    b.expose_input("in", split.input::<i64>("in").unwrap(), one())
        .unwrap();
    let mut opts = SessionOptions::default();
    opts.readiness.insert("join".into(), Readiness::All);
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    for _ in 0..ITEMS {
        s.inject_local("in", 1_i64).unwrap();
        assert_eq!(s.run_until_idle(20).state, SessionState::Quiescent);
    }
    assert_eq!(count.load(Ordering::Relaxed), ITEMS);
}

struct Feedback {
    count: Arc<AtomicUsize>,
}
impl Process for Feedback {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        let Some(v) = cx.take("in") else {
            return Ok(Step::Complete);
        };
        let n = *v.borrow::<i64>().unwrap();
        self.count.fetch_add(1, Ordering::Relaxed);
        Ok(if n == 0 {
            Step::Complete
        } else {
            Step::Emit(ProcessEmission::new("out", Value::local(n - 1)))
        })
    }
}
fn finite_feedback() {
    let count = Arc::new(AtomicUsize::new(0));
    let shared = count.clone();
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "loop",
            interface(&["in"], &["out"]),
            Arc::new(move || Feedback {
                count: shared.clone(),
            }),
        )
        .unwrap();
    b.connect(
        p.output::<i64>("out").unwrap(),
        p.input::<i64>("in").unwrap(),
        one(),
    )
    .unwrap();
    b.expose_input("seed", p.input::<i64>("in").unwrap(), one())
        .unwrap();
    let mut s = b.build().unwrap().compile().unwrap().start();
    s.inject_local("seed", ITEMS as i64).unwrap();
    assert_eq!(s.run_until_idle(ITEMS * 4).state, SessionState::Quiescent);
    assert_eq!(count.load(Ordering::Relaxed), ITEMS + 1);
}

fn nested_compile() {
    let mut definition = Arc::new(ComponentDef::map(|v: i64| v + 1));
    for _ in 0..4 {
        let mut b = NetworkBuilder::new();
        for name in ["left", "right"] {
            let p = b.add(name, definition.clone()).unwrap();
            b.expose_input(name, p.input::<i64>("input").unwrap(), one())
                .unwrap();
            b.expose_output(name, p.output::<i64>("output").unwrap())
                .unwrap();
        }
        // Preserve a uniform boundary while also repeating the module definition.
        let network = b.build().unwrap();
        let mut outer = NetworkBuilder::new();
        let p = outer
            .add("module", Arc::new(ComponentDef::composite(network)))
            .unwrap();
        outer
            .expose_input("input", p.input::<i64>("left").unwrap(), one())
            .unwrap();
        outer
            .expose_output("output", p.output::<i64>("left").unwrap())
            .unwrap();
        definition = Arc::new(ComponentDef::composite(outer.build().unwrap()));
    }
    let mut b = NetworkBuilder::new();
    b.add("root", definition).unwrap();
    let network = b.build().unwrap();
    for _ in 0..20 {
        let c = network.compile().unwrap();
        assert_eq!(c.processes().len(), 16);
        for p in c.processes() {
            black_box(c.inspect(p.path()).unwrap());
        }
    }
}

fn compose_replace() {
    let base = Arc::new(ComponentDef::map(|v: i64| v));
    let replacement = Arc::new(ComponentDef::map(|v: i64| v + 1));
    let mut b = NetworkBuilder::new();
    for n in 0..16 {
        b.add(format!("n{n}"), base.clone()).unwrap();
    }
    let mut composer = Composer::new(b.build().unwrap()).unwrap();
    for _ in 0..20 {
        let registrations = (0..16).map(|n| {
            let path = format!("n{n}");
            Registration::new(
                path.clone(),
                [],
                0,
                GraphOrigin::default(),
                [Footprint::Component(path.clone())],
                NetworkPatch::new([Edit::Replace {
                    path,
                    definition: replacement.clone(),
                }]),
            )
        });
        composer.register(registrations).unwrap();
        assert_eq!(composer.current().active().len(), 16);
        for n in 0..16 {
            assert_eq!(composer.revoke(&format!("n{n}")).unwrap().len(), 1);
        }
        assert!(composer.current().active().is_empty());
        black_box(composer.current());
    }
}

/// Composer end-to-end planning and revocation on a reverse-lexical chain.
/// The ADD variant also replays N real component writes and their exact grants.
/// Both include validation, selection, replay and compilation; these are not
/// planner-only measurements.
fn compose_reverse_dependency_chain<const N: usize, const ADD: bool>() {
    assert!(N > 0);
    let mut builder = NetworkBuilder::new();
    builder
        .add("anchor", Arc::new(ComponentDef::map(|v: i64| v)))
        .unwrap();
    let mut composer = Composer::new(builder.build().unwrap()).unwrap();
    let definition = Arc::new(ComponentDef::map(|v: i64| v));
    let registrations = (0..N).map(|index| {
        let id = format!("node{index:04}");
        let dependencies = if index + 1 < N {
            vec![format!("node{:04}", index + 1)]
        } else {
            Vec::new()
        };
        let (grants, patch) = if ADD {
            (
                vec![Footprint::Component(id.clone())],
                NetworkPatch::new([Edit::Add {
                    path: id.clone(),
                    definition: definition.clone(),
                }]),
            )
        } else {
            (Vec::new(), NetworkPatch::new(Vec::<Edit>::new()))
        };
        Registration::new(id, dependencies, 0, GraphOrigin::default(), grants, patch)
    });
    composer.register(registrations).unwrap();
    assert_eq!(composer.current().active().len(), N);
    let removed = composer.revoke(&format!("node{:04}", N - 1)).unwrap();
    assert_eq!(removed.len(), N);
    assert!(composer.current().active().is_empty());
    black_box(composer.current());
}

/// Real writes on a wide diamond, or a skewed hub/fan-in followed by a long tail.
/// Removing one branch must remove the join/tail, but preserve sibling branches.
fn compose_dependency_fan_in<const N: usize, const SKEWED: bool>() {
    assert!(N >= 4);
    let id = |index| format!("node{:04}", N - 1 - index);
    let join = if SKEWED { N / 2 } else { N - 1 };
    let definition = Arc::new(ComponentDef::map(|v: i64| v));
    let mut builder = NetworkBuilder::new();
    builder.add("anchor", definition.clone()).unwrap();
    let mut composer = Composer::new(builder.build().unwrap()).unwrap();
    let registrations = (0..N).map(|index| {
        let dependencies = if index == 0 {
            Vec::new()
        } else if index == join {
            (1..join).map(id).collect()
        } else if index > join {
            // The root is both a high-degree hub and a redundant ancestor.
            vec![id(0), id(index - 1)]
        } else {
            vec![id(0)]
        };
        let path = id(index);
        Registration::new(
            path.clone(),
            dependencies,
            0,
            GraphOrigin::default(),
            [Footprint::Component(path.clone())],
            NetworkPatch::new([Edit::Add {
                path,
                definition: definition.clone(),
            }]),
        )
    });
    composer.register(registrations).unwrap();
    assert_eq!(composer.current().active(), &(0..N).map(id).collect());
    assert_eq!(composer.current().compiled().processes().len(), N + 1);
    for index in 0..N {
        assert!(composer.current().compiled().inspect(&id(index)).is_ok());
        assert_eq!(
            composer.current().footprints()[&id(index)],
            BTreeSet::from([Footprint::Component(id(index))])
        );
    }

    let removed = composer.revoke(&id(1)).unwrap();
    assert_eq!(removed, std::iter::once(1).chain(join..N).map(id).collect());
    let remaining: BTreeSet<_> = std::iter::once(0).chain(2..join).map(id).collect();
    assert_eq!(composer.current().active(), &remaining);
    assert_eq!(composer.current().compiled().processes().len(), join);
    for index in 0..N {
        assert_eq!(
            composer.current().compiled().inspect(&id(index)).is_ok(),
            remaining.contains(&id(index))
        );
    }
    assert_eq!(composer.revoke(&id(0)).unwrap(), remaining);
    assert!(composer.current().active().is_empty());
    assert!(composer.current().footprints().is_empty());
    assert_eq!(composer.current().compiled().processes().len(), 1);
    assert!(composer.current().compiled().inspect("anchor").is_ok());
    black_box(composer.current());
}

/// All pairs collide, but the first lexical pair must reject the batch atomically.
/// This measures early rejection, not dense successful selection or solver search.
fn compose_dense_equal_priority_conflict<const N: usize>() {
    assert!(N >= 2);
    let definition = Arc::new(ComponentDef::map(|v: i64| v));
    let mut builder = NetworkBuilder::new();
    builder.add("anchor", definition.clone()).unwrap();
    let mut composer = Composer::new(builder.build().unwrap()).unwrap();
    composer
        .register([Registration::new(
            "sentinel",
            [],
            0,
            GraphOrigin::default(),
            [],
            NetworkPatch::new([]),
        )])
        .unwrap();
    // Reverse admission order makes the expected error independent of insertion.
    let registrations = (0..N).rev().map(|index| {
        Registration::new(
            format!("node{index:04}"),
            [],
            0,
            GraphOrigin::default(),
            [Footprint::Component("anchor".into())],
            NetworkPatch::new([Edit::Replace {
                path: "anchor".into(),
                definition: definition.clone(),
            }]),
        )
    });
    assert_eq!(
        composer.register(registrations).unwrap_err(),
        ComposeError::Conflict {
            first: "node0000".into(),
            second: "node0001".into(),
            footprint: Box::new(Footprint::Component("anchor".into())),
        }
    );
    assert_eq!(
        composer.current().active(),
        &BTreeSet::from(["sentinel".into()])
    );
    assert_eq!(composer.current().footprints().len(), 1);
    assert!(composer.current().footprints()["sentinel"].is_empty());
    assert_eq!(composer.current().compiled().processes().len(), 1);
    assert!(composer.current().compiled().inspect("anchor").is_ok());
    // An absent candidate ID proves failed admission did not publish the registry.
    assert!(composer.revoke("node0000").unwrap().is_empty());
    assert_eq!(
        composer.revoke("sentinel").unwrap(),
        BTreeSet::from(["sentinel".into()])
    );
    assert!(composer.current().active().is_empty());
    black_box(composer.current());
}

fn main() {
    let filter = std::env::args()
        .skip(1)
        .find(|argument| !argument.starts_with("--"))
        .unwrap_or_default();
    println!(
        "native_fbp: warmup=2 samples=7; elapsed includes setup, validation and teardown; no allocation instrumentation"
    );
    for (name, run) in [
        ("source_map_sink_1000", pipeline as fn()),
        ("capacity1_fanout_1000_retry8", fanout_retry),
        ("stateful_split_join_1000", split_join),
        ("finite_feedback_1001", finite_feedback),
        ("nested16_compile_inspect_x20", nested_compile),
        ("compose16_replace_revoke_x20", compose_replace),
        (
            "compose_revlex_chain16_register_revoke",
            compose_reverse_dependency_chain::<16, false>,
        ),
        (
            "compose_revlex_chain64_register_revoke",
            compose_reverse_dependency_chain::<64, false>,
        ),
        (
            "compose_revlex_chain128_register_revoke",
            compose_reverse_dependency_chain::<128, false>,
        ),
        (
            "compose_revlex_add16_register_revoke",
            compose_reverse_dependency_chain::<16, true>,
        ),
        (
            "compose_revlex_add64_register_revoke",
            compose_reverse_dependency_chain::<64, true>,
        ),
        (
            "compose_revlex_add128_register_revoke",
            compose_reverse_dependency_chain::<128, true>,
        ),
        (
            "compose_diamond_add16_register_partial_revoke",
            compose_dependency_fan_in::<16, false>,
        ),
        (
            "compose_diamond_add64_register_partial_revoke",
            compose_dependency_fan_in::<64, false>,
        ),
        (
            "compose_diamond_add128_register_partial_revoke",
            compose_dependency_fan_in::<128, false>,
        ),
        (
            "compose_skewed_add16_register_partial_revoke",
            compose_dependency_fan_in::<16, true>,
        ),
        (
            "compose_skewed_add64_register_partial_revoke",
            compose_dependency_fan_in::<64, true>,
        ),
        (
            "compose_skewed_add128_register_partial_revoke",
            compose_dependency_fan_in::<128, true>,
        ),
        (
            "compose_dense_equal64_reject",
            compose_dense_equal_priority_conflict::<64>,
        ),
        (
            "compose_dense_equal256_reject",
            compose_dense_equal_priority_conflict::<256>,
        ),
        (
            "compose_dense_equal1024_reject",
            compose_dense_equal_priority_conflict::<1024>,
        ),
        ("q3_dense32", || {
            composition_cases::dense(32, false, planning_policy::configure)
        }),
        ("q3_dense64", || {
            composition_cases::dense(64, false, planning_policy::configure)
        }),
        ("q3_dense128", || {
            composition_cases::dense(128, false, planning_policy::configure)
        }),
        ("q3_dense256", || {
            composition_cases::dense(256, false, planning_policy::configure)
        }),
        ("q3_dense_reverse128", || {
            composition_cases::dense(128, true, planning_policy::configure)
        }),
        ("q3_defeat4", || {
            composition_cases::defeat(4, planning_policy::configure)
        }),
        ("q3_defeat16", || {
            composition_cases::defeat(16, planning_policy::configure)
        }),
        ("q3_defeat32", || {
            composition_cases::defeat(32, planning_policy::configure)
        }),
        ("q3_splices16", || {
            composition_cases::splices(16, planning_policy::configure)
        }),
        ("q3_splices64", || {
            composition_cases::splices(64, planning_policy::configure)
        }),
        ("q3_splices128", || {
            composition_cases::splices(128, planning_policy::configure)
        }),
        ("q3_hierarchical16", || {
            composition_cases::hierarchical(16, planning_policy::configure)
        }),
        ("q3_hierarchical64", || {
            composition_cases::hierarchical(64, planning_policy::configure)
        }),
        ("q3_hierarchical128", || {
            composition_cases::hierarchical(128, planning_policy::configure)
        }),
    ] {
        if !name.contains(&filter) {
            continue;
        }
        for _ in 0..2 {
            run();
        }
        let mut samples = Vec::new();
        for _ in 0..7 {
            let start = Instant::now();
            run();
            samples.push(start.elapsed().as_micros());
        }
        println!("{name}: samples_us={samples:?}");
        samples.sort_unstable();
        println!("{name}: median_us={}", samples[3]);
    }
}
