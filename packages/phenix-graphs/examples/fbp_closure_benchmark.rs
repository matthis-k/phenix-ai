//! Paired H5 scoped-closure benchmark: unrelated upstream fan-in versus
//! wholly reachable optional closure chains. Real persistent sessions, no mock
//! traversal calls or manually synthesized request ownership.
//!
//! cargo run --locked --release -p phenix-graph --example fbp_closure_benchmark -- 250 0 16 64 128
use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::{CompiledNetwork, NetworkBuilder},
    process::{Process, ProcessContext, Step},
    runtime::{RequestStatus, SessionState},
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

struct DropInput;
impl Process for DropInput {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        black_box(cx.take("in").ok_or("missing request input")?);
        Ok(Step::Complete)
    }
}

struct AbsentInput(Arc<AtomicUsize>);
impl Process for AbsentInput {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if cx.take("opt").is_some() || !cx.is_request_closed("opt") || cx.is_eos("opt") {
            return Err("expected authenticated request-local empty optional closure".into());
        }
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(Step::Complete)
    }
}

fn compiled(mode: &str, breadth: usize, hits: Arc<AtomicUsize>) -> CompiledNetwork {
    let mut builder = NetworkBuilder::new();
    let root_iface = Arc::new(
        Interface::new(
            [PortDef::wire::<u64>("in").unwrap()],
            [PortDef::wire::<u64>("out").unwrap()],
        )
        .unwrap(),
    );
    let optional_iface = Arc::new(
        Interface::new(
            [PortDef::wire::<u64>("opt").unwrap()],
            [PortDef::wire::<u64>("out").unwrap()],
        )
        .unwrap(),
    );
    let root = builder
        .node("root", root_iface.clone(), Arc::new(|| DropInput))
        .unwrap();
    builder
        .expose_input(
            "trigger",
            root.wire_input::<u64>("in").unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();

    if mode == "fan-in" {
        let captured = hits.clone();
        let sink = builder
            .node(
                "sink",
                Arc::new(Interface::new([PortDef::wire::<u64>("opt").unwrap()], []).unwrap()),
                Arc::new(move || AbsentInput(captured.clone())),
            )
            .unwrap();
        builder
            .connect(
                root.wire_output::<u64>("out").unwrap(),
                sink.wire_input::<u64>("opt").unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        // Each unrelated producer is permanently open to independent host
        // ingress but has no request-R root. They share the optional port.
        for index in 0..breadth {
            let name = format!("unrelated-{index}");
            let producer = builder
                .node(name.clone(), root_iface.clone(), Arc::new(|| DropInput))
                .unwrap();
            builder
                .expose_input(
                    name,
                    producer.wire_input::<u64>("in").unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            builder
                .connect(
                    producer.wire_output::<u64>("out").unwrap(),
                    sink.wire_input::<u64>("opt").unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
        }
    } else {
        assert_eq!(mode, "chain");
        let mut upstream = root.wire_output::<u64>("out").unwrap();
        for index in 0..breadth {
            let name = format!("optional-{index}");
            let captured = hits.clone();
            let node = builder
                .node(
                    name.clone(),
                    optional_iface.clone(),
                    Arc::new(move || AbsentInput(captured.clone())),
                )
                .unwrap();
            builder
                .connect(
                    upstream,
                    node.wire_input::<u64>("opt").unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            upstream = node.wire_output::<u64>("out").unwrap();
        }
    }

    builder.build().unwrap().compile().unwrap()
}

fn measure(mode: &str, breadth: usize, iterations: usize) -> (f64, f64, f64, f64) {
    assert!(iterations > 0);
    let hits = Arc::new(AtomicUsize::new(0));
    let build = Instant::now();
    let network = compiled(mode, breadth, hits.clone());
    let compile_ms = build.elapsed().as_secs_f64() * 1e3;
    let mut options = phenix_graph::runtime::SessionOptions::default();
    if mode == "fan-in" {
        options.readiness.insert(
            "sink".into(),
            phenix_graph::runtime::Readiness::Optional {
                required: vec![],
                optional: vec!["opt".into()],
            },
        );
    } else {
        for index in 0..breadth {
            options.readiness.insert(
                format!("optional-{index}"),
                phenix_graph::runtime::Readiness::Optional {
                    required: vec![],
                    optional: vec!["opt".into()],
                },
            );
        }
    }
    let start = Instant::now();
    let mut session = network.start_with_options(options);
    let start_ms = start.elapsed().as_secs_f64() * 1e3;
    assert_eq!(session.state(), SessionState::Quiescent);
    let expected_steps = if mode == "fan-in" { 2 } else { breadth + 1 };
    let expected_hits = if mode == "fan-in" { 1 } else { breadth };
    let sample = |session: &mut phenix_graph::runtime::NetworkSession, value: usize| {
        let id = session.begin_request(None).unwrap();
        session
            .inject_request_local(id, "trigger", black_box(value as u64))
            .unwrap();
        session.close_request(id).unwrap();
        let report = session.run_until_idle(4 * expected_steps + 32);
        assert!(!report.budget_exhausted);
        assert_eq!(report.steps, expected_steps);
        assert_eq!(
            session.request_status(id).unwrap(),
            RequestStatus::Settled { captures: 0 }
        );
        session.release_request(id).unwrap();
        assert_eq!(session.state(), SessionState::Quiescent);
    };
    for index in 0..16 {
        sample(&mut session, index);
    }
    let mut times = Vec::with_capacity(iterations);
    let began = Instant::now();
    for index in 0..iterations {
        let start = Instant::now();
        sample(&mut session, index + 16);
        times.push(start.elapsed().as_nanos() as f64);
    }
    let mean = began.elapsed().as_nanos() as f64 / iterations as f64;
    assert_eq!(
        hits.load(Ordering::Relaxed),
        (iterations + 16) * expected_hits
    );
    times.sort_by(|a, b| a.total_cmp(b));
    let median = times[(iterations - 1) / 2];
    (compile_ms, start_ms, mean, median)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let iterations: usize = args
        .next()
        .unwrap_or_else(|| "250".into())
        .parse()
        .unwrap();
    let breadths: Vec<_> = args.map(|s| s.parse::<usize>().unwrap()).collect();
    let breadths = if breadths.is_empty() {
        vec![0, 16, 64, 128]
    } else {
        breadths
    };
    println!("mode,other_or_chain_nodes,iterations,compile_ms,start_ms,mean_ns_per_request,p50_ns");
    for &count in &breadths {
        for mode in ["fan-in", "chain"] {
            if mode == "chain" && count == 0 {
                continue;
            }
            let (compile_ms, start_ms, mean, p50) = measure(mode, count, iterations);
            println!("{mode},{count},{iterations},{compile_ms:.3},{start_ms:.3},{mean:.1},{p50:.0}");
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reachable_closure_fanin_and_chain_smoke() {
        super::measure("fan-in", 8, 3);
        super::measure("chain", 4, 3);
    }
}
