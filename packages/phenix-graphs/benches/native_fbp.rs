//! Bounded native-only evidence: no alternate scheduler or benchmark dependency.
use phenix_graph::{
    ComponentDef, GraphOrigin, Interface, NetworkBuilder, PortDef, Process, ProcessContext,
    ProcessEmission, Step, Value,
    compose::{Composer, Edit, Footprint, NetworkPatch, Registration},
    connection::{ConnectionError, ConnectionQueue, ConnectionSet, FanoutTarget, Packet},
    runtime::{Readiness, SessionOptions, SessionState},
};
use std::{
    hint::black_box,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

const ITEMS: usize = 1_000;
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
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
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
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
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
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
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

fn main() {
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
    ] {
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
