//! H4 paired benchmark: unrelated writer-open optional readiness policies.
//!
//! cargo run --locked --release -p phenix-graph --example fbp_optional_idle_benchmark -- 3000
use phenix_graph::{
    component::ComponentDef,
    network::NetworkBuilder,
    runtime::{Readiness, RequestStatus, SessionOptions, SessionState},
};
use std::{hint::black_box, num::NonZeroUsize, sync::Arc, time::Instant};

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

fn measure(
    optional_nodes: usize,
    iterations: usize,
    scoped: bool,
) -> (f64, f64, f64, f64, f64) {
    let mut builder = NetworkBuilder::new();
    let sink = Arc::new(ComponentDef::sink(|value: &u64| {
        black_box(*value);
    }));
    let active = builder.add("active", sink.clone()).unwrap();
    builder
        .expose_input("active", active.input::<u64>("input").unwrap(), one())
        .unwrap();
    let mut options = SessionOptions::default();
    for i in 0..optional_nodes {
        let name = format!("optional-{i}");
        let node = builder.add(name.clone(), sink.clone()).unwrap();
        builder
            .expose_input(
                format!("optional-input-{i}"),
                node.input::<u64>("input").unwrap(),
                one(),
            )
            .unwrap();
        options.readiness.insert(
            name,
            Readiness::Optional {
                required: vec![],
                optional: vec!["input".into()],
            },
        );
    }
    let begin = Instant::now();
    let graph = builder.build().unwrap().compile().unwrap();
    let compile_ms = begin.elapsed().as_secs_f64() * 1000.0;
    let begin = Instant::now();
    let mut session = graph.start_with_options(options);
    let start_ms = begin.elapsed().as_secs_f64() * 1000.0;
    let mut samples = Vec::with_capacity(iterations);
    let warmup = 32;
    for iteration in 0..warmup + iterations {
        let begin = Instant::now();
        if scoped {
            let request = session.begin_request(None).unwrap();
            session
                .inject_request_local(request, "active", black_box(iteration as u64))
                .unwrap();
            session.close_request(request).unwrap();
            let report = session.run_until_idle(8);
            assert_eq!(report.steps, 1);
            assert!(!report.budget_exhausted);
            assert_eq!(
                session.request_status(request).unwrap(),
                RequestStatus::Settled { captures: 0 }
            );
            session.release_request(request).unwrap();
        } else {
            session
                .inject_local("active", black_box(iteration as u64))
                .unwrap();
            let report = session.run_until_idle(8);
            assert_eq!(report.steps, 1);
            assert!(!report.budget_exhausted);
        }
        assert_eq!(session.state(), SessionState::Quiescent);
        if iteration >= warmup {
            samples.push(begin.elapsed().as_nanos() as f64);
        }
    }
    let mean = samples.iter().sum::<f64>() / iterations as f64;
    samples.sort_by(|a, b| a.total_cmp(b));
    let median = samples[(iterations - 1) / 2];
    let p95 = samples[(((iterations - 1) as f64) * 0.95).round() as usize];
    (compile_ms, start_ms, mean, median, p95)
}

fn main() {
    let iterations = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "3000".into())
        .parse::<usize>()
        .expect("positive iterations");
    assert!(iterations > 0);
    eprintln!(
        "scenario=one_active_sink_with_unrelated_writer_open_optional_sinks;          warmup=32; all_optional_inputs_open_and_empty; iterations={iterations}"
    );
    println!(
        "mode,optional_nodes,iterations,compile_ms,start_ms,mean_ns_per_event,p50_ns,p95_ns"
    );
    for scoped in [false, true] {
        for optional_nodes in [0, 16, 64, 256] {
            let (compile_ms, start_ms, mean, p50, p95) =
                measure(optional_nodes, iterations, scoped);
            let mode = if scoped { "scoped-unrelated" } else { "unscoped" };
            println!(
                "{mode},{optional_nodes},{iterations},{compile_ms:.3},{start_ms:.3},{mean:.1},{p50:.0},{p95:.0}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_idle_benchmark_preserves_packet_and_request_progress() {
        for scoped in [false, true] {
            for optional_nodes in [0, 4, 16] {
                let result = measure(optional_nodes, 4, scoped);
                assert!(result.2.is_finite());
            }
        }
    }
}
