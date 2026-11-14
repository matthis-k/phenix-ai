//! H6: reproducible native FBP scheduling benchmark, not a fixed-speed test.
//!
//! cargo run --release -p phenix-graph --example fbp_benchmark -- 10000 1 16 64 256
//! CSV on stdout; workload and environment metadata are printed on stderr.
use phenix_graph::{component::ComponentDef, network::NetworkBuilder, runtime::SessionState};
use std::{hint::black_box, num::NonZeroUsize, sync::Arc, time::Instant};

fn measure(processes: usize, iterations: usize) -> (f64, f64, f64, f64, f64) {
    assert!(processes > 0 && iterations > 0);
    let capacity = NonZeroUsize::new(1).unwrap();
    let start_build = Instant::now();
    let mut builder = NetworkBuilder::new();
    let sink = Arc::new(ComponentDef::sink(|value: &u64| {
        black_box(*value);
    }));
    for index in 0..processes {
        let node = builder
            .add(format!("sink-{index}"), sink.clone())
            .expect("unique sink name");
        builder
            .expose_input(
                format!("input-{index}"),
                node.input::<u64>("input").expect("typed port"),
                capacity,
            )
            .expect("unique input projection");
    }
    let network = builder.build().unwrap().compile().unwrap();
    let build_ms = start_build.elapsed().as_secs_f64() * 1000.0;
    let start_session = Instant::now();
    let mut session = network.start();
    let start_ms = start_session.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(session.state(), SessionState::Quiescent);

    // Warming up is outside the timed region; use the *same* long-lived session.
    for value in 0..64 {
        session.inject_local("input-0", value as u64).unwrap();
        let report = session.run_until_idle(32);
        assert!(!report.budget_exhausted);
        assert!(report.steps > 0);
        assert_eq!(report.state, SessionState::Quiescent);
    }

    let mut samples_ns = Vec::with_capacity(iterations);
    let total_begin = Instant::now();
    let mut total_steps = 0usize;
    for value in 0..iterations {
        let begin = Instant::now();
        session
            .inject_local("input-0", black_box(value as u64))
            .unwrap();
        let report = session.run_until_idle(32);
        assert_eq!(report.state, SessionState::Quiescent);
        assert!(!report.budget_exhausted);
        assert!(report.steps > 0);
        total_steps += report.steps;
        samples_ns.push(begin.elapsed().as_nanos() as u64);
    }
    let total_ns = total_begin.elapsed().as_secs_f64() * 1e9;
    assert!(total_steps >= iterations, "missing expected process work");
    samples_ns.sort_unstable();
    let p50_ns = samples_ns[(iterations - 1) / 2] as f64;
    let p95_ns = samples_ns[(((iterations - 1) as f64) * 0.95).round() as usize] as f64;
    (
        build_ms,
        start_ms,
        total_ns / iterations as f64,
        p50_ns,
        p95_ns,
    )
}

fn main() {
    let mut args = std::env::args().skip(1);
    let iterations: usize = args
        .next()
        .unwrap_or_else(|| "10000".into())
        .parse()
        .expect("iterations must be a positive integer");
    assert!(iterations > 0);
    let processes = args
        .map(|arg| arg.parse::<usize>().expect("process count must be integer"))
        .collect::<Vec<_>>();
    let processes = if processes.is_empty() {
        vec![1, 16, 64, 256]
    } else {
        processes
    };
    eprintln!(
        "scenario=one_open_hot_ingress_plus_idle_open_ingresses; capacity=1; \
         warmup=64; events={iterations}; persistent_session=true; input=u64_local"
    );
    println!("processes,events,compile_ms,start_ms,mean_ns_per_event,p50_ns,p95_ns");
    for processes in processes {
        let (compile_ms, start_ms, mean_ns, p50_ns, p95_ns) = measure(processes, iterations);
        println!(
            "{processes},{iterations},{compile_ms:.3},{start_ms:.3},{mean_ns:.1},{p50_ns:.0},{p95_ns:.0}"
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_benchmark_smoke() {
        let values = super::measure(2, 4);
        assert!(values.2 > 0.0);
    }
}
