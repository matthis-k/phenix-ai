//! Representative H4 scheduler scenario: persistent request-scoped agent flow
//! with prompt/model/skills, local-versus-remote tool selection, an optional
//! join and persistence, plus unrelated optional/custom service branches.
//!
//! Both modes use the SAME runtime/executable/topology. "polled" opts out of
//! the new contract; "notified" opts in, with an addressed notification when
//! a local-tool permission changes. No network or model calls are simulated.
//!
//! cargo run --locked --release -p phenix-graph --example fbp_agent_mixed_benchmark -- notified 400
use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::{CompiledNetwork, NetworkBuilder},
    process::{Process, ProcessContext, ProcessEmission, Step},
    runtime::{
        Readiness, ReadinessPolicy, ReadinessSnapshot, RequestStatus, SessionOptions, SessionState,
    },
};
use std::{
    hint::black_box,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Instant,
};

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

// A model/tool/skills process must settle the same owned activation after
// emitting. The Value is wire-typed to avoid local-vs-wire test shortcuts.
struct MapStep {
    stage: bool,
    offset: u64,
}
impl Process for MapStep {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.stage {
            self.stage = false;
            return Ok(Step::Complete);
        }
        let value = cx.take("in").ok_or("missing input")?;
        let n = value.get::<u64>()?;
        self.stage = true;
        Ok(Step::Emit(ProcessEmission::new(
            "out",
            Value::of(black_box(n.wrapping_add(self.offset))),
        )))
    }
}

struct RouteStep(bool);
impl Process for RouteStep {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.0 {
            self.0 = false;
            return Ok(Step::Complete);
        }
        let value = cx.take("in").ok_or("missing route input")?;
        let n = value.get::<u64>()?;
        self.0 = true;
        let route = if n % 2 == 0 { "local" } else { "remote" };
        Ok(Step::Emit(ProcessEmission::new(route, value)))
    }
}

struct JoinStep(bool);
impl Process for JoinStep {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.0 {
            self.0 = false;
            return Ok(Step::Complete);
        }
        let context = cx.take("context").ok_or("missing skills context")?;
        let local = cx.take("local");
        let remote = cx.take("remote");
        if local.is_some() == remote.is_some() {
            return Err("expected exactly one tool result".into());
        }
        if (local.is_none() && !cx.is_request_closed("local"))
            || (remote.is_none() && !cx.is_request_closed("remote"))
        {
            return Err("unused tool must be request-locally closed".into());
        }
        if cx.is_eos("local") || cx.is_eos("remote") {
            return Err("request-local absence misreported as global EOS".into());
        }
        let value = local.or(remote).unwrap();
        let answer = context.get::<u64>()?.wrapping_add(value.get::<u64>()?);
        self.0 = true;
        Ok(Step::Emit(ProcessEmission::new(
            "out",
            Value::of(black_box(answer)),
        )))
    }
}

struct Persist(Arc<AtomicUsize>);
impl Process for Persist {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        let result = cx.take("in").ok_or("missing persisted result")?;
        black_box(result.get::<u64>()?);
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(Step::Complete)
    }
}

struct Gate {
    allowed: Arc<AtomicBool>,
    notified: bool,
}
impl ReadinessPolicy for Gate {
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        if self.allowed.load(Ordering::Acquire) {
            snapshot
                .heads
                .iter()
                .filter(|head| head.available)
                .map(|head| head.port.clone())
                .collect()
        } else {
            vec![]
        }
    }

    fn notification_driven(&self) -> bool {
        self.notified
    }
}

fn wire(input: &[&str], output: &[&str]) -> Arc<Interface> {
    Arc::new(
        Interface::new(
            input
                .iter()
                .map(|name| PortDef::wire::<u64>(*name).unwrap()),
            output
                .iter()
                .map(|name| PortDef::wire::<u64>(*name).unwrap()),
        )
        .unwrap(),
    )
}

pub(crate) fn compiled(
    idle: usize,
    notified: bool,
    persisted: Arc<AtomicUsize>,
    local_allowed: Arc<AtomicBool>,
) -> (CompiledNetwork, SessionOptions) {
    let mut b = NetworkBuilder::new();
    let map_interface = wire(&["in"], &["out"]);
    let route_interface = wire(&["in"], &["local", "remote"]);
    let root = b
        .node(
            "prompt",
            map_interface.clone(),
            Arc::new(|| MapStep {
                stage: false,
                offset: 0,
            }),
        )
        .unwrap();
    let model = b
        .node("model", route_interface, Arc::new(|| RouteStep(false)))
        .unwrap();
    let skills = b
        .node(
            "skills",
            map_interface.clone(),
            Arc::new(|| MapStep {
                stage: false,
                offset: 300,
            }),
        )
        .unwrap();
    let local = b
        .node(
            "tools.local",
            map_interface.clone(),
            Arc::new(|| MapStep {
                stage: false,
                offset: 1000,
            }),
        )
        .unwrap();
    let remote = b
        .node(
            "tools.remote",
            map_interface.clone(),
            Arc::new(|| MapStep {
                stage: false,
                offset: 2000,
            }),
        )
        .unwrap();
    let join = b
        .node(
            "join",
            wire(&["context", "local", "remote"], &["out"]),
            Arc::new(|| JoinStep(false)),
        )
        .unwrap();
    let persist = b
        .node("persistence", wire(&["in"], &[]), {
            let persisted = persisted.clone();
            Arc::new(move || Persist(persisted.clone()))
        })
        .unwrap();

    b.expose_input("prompt", root.wire_input::<u64>("in").unwrap(), one())
        .unwrap();
    for endpoint in [
        model.wire_input::<u64>("in").unwrap(),
        skills.wire_input::<u64>("in").unwrap(),
    ] {
        b.connect(root.wire_output::<u64>("out").unwrap(), endpoint, one())
            .unwrap();
    }
    b.connect(
        model.wire_output::<u64>("local").unwrap(),
        local.wire_input::<u64>("in").unwrap(),
        one(),
    )
    .unwrap();
    b.connect(
        model.wire_output::<u64>("remote").unwrap(),
        remote.wire_input::<u64>("in").unwrap(),
        one(),
    )
    .unwrap();
    for (source, target) in [
        (
            skills.wire_output::<u64>("out").unwrap(),
            join.wire_input::<u64>("context").unwrap(),
        ),
        (
            local.wire_output::<u64>("out").unwrap(),
            join.wire_input::<u64>("local").unwrap(),
        ),
        (
            remote.wire_output::<u64>("out").unwrap(),
            join.wire_input::<u64>("remote").unwrap(),
        ),
    ] {
        b.connect(source, target, one()).unwrap();
    }
    b.connect(
        join.wire_output::<u64>("out").unwrap(),
        persist.wire_input::<u64>("in").unwrap(),
        one(),
    )
    .unwrap();

    let mut options = SessionOptions::default();
    options.readiness.insert(
        "join".into(),
        Readiness::Optional {
            required: vec!["context".into()],
            optional: vec!["local".into(), "remote".into()],
        },
    );
    options.readiness.insert(
        "tools.local".into(),
        Readiness::Custom(Arc::new(Gate {
            allowed: local_allowed,
            notified,
        })),
    );

    // 50% idle Custom; 50% idle Optional. Every branch is writer-open and
    // unrelated to the request's graph reachability.
    for index in 0..idle {
        let name = format!("idle-{index}");
        let node = b
            .node(
                name.clone(),
                wire(&["in"], &[]),
                Arc::new(|| Persist(Arc::new(AtomicUsize::new(0)))),
            )
            .unwrap();
        b.expose_input(
            format!("idle-input-{index}"),
            node.wire_input::<u64>("in").unwrap(),
            one(),
        )
        .unwrap();
        options.readiness.insert(
            name,
            if index % 2 == 0 {
                Readiness::Custom(Arc::new(Gate {
                    allowed: Arc::new(AtomicBool::new(false)),
                    notified,
                }))
            } else {
                Readiness::Optional {
                    required: vec![],
                    optional: vec!["in".into()],
                }
            },
        );
    }
    (b.build().unwrap().compile().unwrap(), options)
}

fn measure(notified: bool, idle: usize, iterations: usize) -> (f64, f64, f64, usize) {
    let persisted = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(AtomicBool::new(false));
    let (graph, options) = compiled(idle, notified, persisted.clone(), gate.clone());
    let mut session = graph.start_with_options(options);
    let notifier = session.process_notifier("tools.local").unwrap();
    assert_eq!(session.state(), SessionState::Quiescent);
    let warmup = 12;
    let mut samples = Vec::with_capacity(iterations);
    for iteration in 0..(iterations + warmup) {
        gate.store(false, Ordering::Release);
        let began = Instant::now();
        let request = session.begin_request(None).unwrap();
        session
            .inject_request(request, "prompt", Value::of(black_box(iteration as u64)))
            .unwrap();
        session.close_request(request).unwrap();
        let before = session.run_until_idle(64);
        assert!(!before.budget_exhausted);
        let mut steps = before.steps;
        if iteration % 2 == 0 {
            assert_eq!(before.steps, 6);
            assert_eq!(
                session.request_status(request).unwrap(),
                RequestStatus::Pending
            );
            gate.store(true, Ordering::Release);
            assert!(notifier.notify());
            let after = session.run_until_idle(64);
            assert!(!after.budget_exhausted);
            steps += after.steps;
        }
        assert_eq!(steps, 11);
        assert_eq!(
            session.request_status(request).unwrap(),
            RequestStatus::Settled { captures: 0 }
        );
        session.release_request(request).unwrap();
        assert_eq!(session.state(), SessionState::Quiescent);
        if iteration >= warmup {
            samples.push(began.elapsed().as_nanos() as f64);
        }
    }
    assert_eq!(persisted.load(Ordering::Relaxed), iterations + warmup);
    samples.sort_by(|a, b| a.total_cmp(b));
    let mean = samples.iter().sum::<f64>() / iterations as f64;
    let p50 = samples[(iterations - 1) / 2];
    let p95 = samples[(((iterations - 1) as f64) * 0.95).round() as usize];
    (mean, p50, p95, persisted.load(Ordering::Relaxed))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().expect("mode: polled|notified");
    let notified = match mode.as_str() {
        "polled" => false,
        "notified" => true,
        _ => panic!("mode must be polled|notified"),
    };
    let iterations: usize = args
        .next()
        .unwrap_or_else(|| "400".into())
        .parse()
        .expect("positive iteration count");
    assert!(iterations > 0);
    eprintln!(
        "scenario=request_scoped_model_skills_tool_switch_optional_join_persistence;          alternating_local_remote; tool_permission_event_every_second_turn;          unrelated_writer_open_custom_optional; persistent_session;          warmup=12; iterations={iterations}"
    );
    println!("mode,idle_nodes,iterations,mean_ns_per_turn,p50_ns,p95_ns,persisted");
    for idle in [0, 16, 64, 128] {
        let (mean, p50, p95, persisted) = measure(notified, idle, iterations);
        println!("{mode},{idle},{iterations},{mean:.1},{p50:.0},{p95:.0},{persisted}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_agent_loop_preserves_routing_closure_notification_and_persistence() {
        for notified in [false, true] {
            for idle in [0, 4, 16] {
                let result = measure(notified, idle, 6);
                assert!(result.0.is_finite());
                assert_eq!(result.3, 18);
            }
        }
    }
}
