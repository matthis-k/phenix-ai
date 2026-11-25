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

// Paired CI registers this cfg with --check-cfg; ordinary builds use Failure.
#[allow(unexpected_cfgs)]
mod benchmark_error {
    #[cfg(fbp_benchmark_baseline_string_error)]
    pub type StepError = String;
    #[cfg(not(fbp_benchmark_baseline_string_error))]
    pub type StepError = phenix_graph::Failure;

    pub fn application_message(message: &str) -> StepError {
        #[cfg(fbp_benchmark_baseline_string_error)]
        {
            message.to_owned()
        }
        #[cfg(not(fbp_benchmark_baseline_string_error))]
        {
            phenix_graph::Failure::application_message(message)
        }
    }
}

struct DropInput;
impl Process for DropInput {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, benchmark_error::StepError> {
        black_box(
            cx.take("in")
                .ok_or_else(|| benchmark_error::application_message("missing request input"))?,
        );
        Ok(Step::Complete)
    }
}

struct AbsentInput(Arc<AtomicUsize>);
impl Process for AbsentInput {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, benchmark_error::StepError> {
        if cx.take("opt").is_some() || !cx.is_request_closed("opt") || cx.is_eos("opt") {
            return Err(benchmark_error::application_message(
                "expected authenticated request-local empty optional closure",
            ));
        }
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(Step::Complete)
    }
}

fn compiled(mode: &str, breadth: usize, hits: Arc<AtomicUsize>) -> CompiledNetwork {
    compiled_with_capacity(mode, breadth, hits, NonZeroUsize::new(1).unwrap())
}

pub(crate) fn compiled_with_capacity(
    mode: &str,
    breadth: usize,
    hits: Arc<AtomicUsize>,
    trigger_capacity: NonZeroUsize,
) -> CompiledNetwork {
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
            trigger_capacity,
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

pub(crate) fn readiness_options(
    mode: &str,
    breadth: usize,
) -> phenix_graph::runtime::SessionOptions {
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
    options
}

/// Mixed H5/H6 stress: four authenticated sealed scopes coexist throughout
/// each batch, all with packet-backed roots and request-local Optional
/// closure obligations. No synthetic closures or manually mutated transport.
fn measure_overlapping(mode: &str, breadth: usize, iterations: usize) -> (f64, f64, f64, f64) {
    const ACTIVE: usize = 4;
    let hits = Arc::new(AtomicUsize::new(0));
    let built = Instant::now();
    let network = compiled_with_capacity(
        mode,
        breadth,
        hits.clone(),
        NonZeroUsize::new(ACTIVE).unwrap(),
    );
    let compile_ms = built.elapsed().as_secs_f64() * 1e3;
    let options = readiness_options(mode, breadth);
    let start = Instant::now();
    let mut session = network.start_with_options(options);
    let start_ms = start.elapsed().as_secs_f64() * 1e3;
    let steps_per_scope = if mode == "fan-in" { 2 } else { breadth + 1 };
    let hits_per_scope = if mode == "fan-in" { 1 } else { breadth };

    let sample = |session: &mut phenix_graph::runtime::NetworkSession, batch: usize| {
        let ids: Vec<_> = (0..ACTIVE)
            .map(|_| session.begin_request(None).unwrap())
            .collect();
        for (index, id) in ids.iter().enumerate() {
            session
                .inject_request(
                    *id,
                    "trigger",
                    Value::of(black_box((batch * ACTIVE + index) as u64)),
                )
                .unwrap();
        }
        for id in &ids {
            session.close_request(*id).unwrap();
        }
        let report = session.run_until_idle(ACTIVE * steps_per_scope * 4 + 32);
        assert!(!report.budget_exhausted);
        assert_eq!(report.steps, ACTIVE * steps_per_scope);
        for id in ids {
            assert_eq!(
                session.request_status(id).unwrap(),
                RequestStatus::Settled { captures: 0 }
            );
            session.release_request(id).unwrap();
        }
        assert_eq!(session.state(), SessionState::Quiescent);
    };
    for index in 0..16 {
        sample(&mut session, index);
    }
    let mut samples = Vec::with_capacity(iterations);
    let started = Instant::now();
    for index in 0..iterations {
        let start = Instant::now();
        sample(&mut session, index + 16);
        samples.push(start.elapsed().as_nanos() as f64 / ACTIVE as f64);
    }
    let mean = started.elapsed().as_nanos() as f64 / (iterations * ACTIVE) as f64;
    assert_eq!(
        hits.load(Ordering::Relaxed),
        (16 + iterations) * ACTIVE * hits_per_scope
    );
    samples.sort_by(|a, b| a.total_cmp(b));
    let median = samples[(iterations - 1) / 2];
    (compile_ms, start_ms, mean, median)
}

fn measure(mode: &str, breadth: usize, iterations: usize) -> (f64, f64, f64, f64) {
    assert!(iterations > 0);
    let hits = Arc::new(AtomicUsize::new(0));
    let build = Instant::now();
    let network = compiled(mode, breadth, hits.clone());
    let compile_ms = build.elapsed().as_secs_f64() * 1e3;
    let options = readiness_options(mode, breadth);
    let start = Instant::now();
    let mut session = network.start_with_options(options);
    let start_ms = start.elapsed().as_secs_f64() * 1e3;
    assert_eq!(session.state(), SessionState::Quiescent);
    let expected_steps = if mode == "fan-in" { 2 } else { breadth + 1 };
    let expected_hits = if mode == "fan-in" { 1 } else { breadth };
    let sample = |session: &mut phenix_graph::runtime::NetworkSession, value: usize| {
        let id = session.begin_request(None).unwrap();
        session
            .inject_request(id, "trigger", Value::of(black_box(value as u64)))
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
    let iterations: usize = args.next().unwrap_or_else(|| "250".into()).parse().unwrap();
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
            println!(
                "{mode},{count},{iterations},{compile_ms:.3},{start_ms:.3},{mean:.1},{p50:.0}"
            );
        }
    }
    // Additional same-runtime concurrent-scope evidence. Keep the older
    // isolated-case rows intact for longitudinal paired comparisons.
    let overlap_iterations = iterations.min(80);
    for &count in &breadths {
        for mode in ["fan-in", "chain"] {
            if mode == "chain" && count == 0 {
                continue;
            }
            let (compile_ms, start_ms, mean, p50) =
                measure_overlapping(mode, count, overlap_iterations);
            println!(
                "{mode}-overlap-4,{count},{overlap_iterations},{compile_ms:.3},{start_ms:.3},{mean:.1},{p50:.0}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reachable_closure_fanin_and_chain_smoke() {
        super::measure("fan-in", 8, 3);
        super::measure("chain", 4, 3);
    }

    #[test]
    fn all_reachable_idle_nodes_still_complete_scoped_optional_closure() {
        // Every node belongs to the same reachable request scope. Only the
        // current closure frontier can progress without physical packets,
        // and every downstream optional consumer must execute exactly once.
        super::measure("chain", 64, 6);
    }

    #[test]
    fn four_overlapping_sealed_scopes_complete_optional_closure_without_stealing_ownership() {
        super::measure_overlapping("fan-in", 8, 5);
        super::measure_overlapping("chain", 8, 5);
    }

    #[test]
    fn two_roots_owned_by_same_request_must_both_finish_before_optional_closure() {
        use phenix_graph::runtime::{Readiness, SessionOptions};
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let network = super::compiled("fan-in", 1, hits.clone());
        let mut opts = SessionOptions::default();
        opts.readiness.insert(
            "sink".into(),
            Readiness::Optional {
                required: vec![],
                optional: vec!["opt".into()],
            },
        );
        let mut session = network.start_with_options(opts);
        let id = session.begin_request(None).unwrap();
        session
            .inject_request(id, "trigger", Value::of(7_u64))
            .unwrap();
        session
            .inject_request(id, "unrelated-0", Value::of(8_u64))
            .unwrap();
        session.close_request(id).unwrap();
        let first = session.run_until_idle(1);
        assert_eq!(first.steps, 1);
        assert_eq!(session.request_status(id).unwrap(), RequestStatus::Pending);
        assert_eq!(hits.load(Ordering::Relaxed), 0);
        let rest = session.run_until_idle(32);
        assert_eq!(rest.steps, 2);
        assert_eq!(hits.load(Ordering::Relaxed), 1);
        assert_eq!(
            session.request_status(id).unwrap(),
            RequestStatus::Settled { captures: 0 }
        );
        session.release_request(id).unwrap();
    }
}
