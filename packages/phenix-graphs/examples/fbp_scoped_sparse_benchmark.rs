//! H4: paired scoped-request sparse scheduling, with live request scopes.
//!
//! cargo run --locked --release -p phenix-graph --example fbp_scoped_sparse_benchmark -- 5000 1 16 64 256
//! The old unscoped benchmark cannot expose the overly broad
//! `self.requests.is_empty()` prefilter restriction.
use phenix_graph::{
    component::ComponentDef,
    network::NetworkBuilder,
    runtime::{RequestStatus, SessionState},
};
use std::{hint::black_box, num::NonZeroUsize, sync::Arc, time::Instant};

fn compiled(processes: usize) -> phenix_graph::network::CompiledNetwork {
    let mut builder = NetworkBuilder::new();
    let sink = Arc::new(ComponentDef::sink(|value: &u64| {
        black_box(*value);
    }));
    for index in 0..processes {
        let node = builder
            .add(format!("sink-{index}"), sink.clone())
            .unwrap();
        builder
            .expose_input(
                format!("input-{index}"),
                node.input::<u64>("input").unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
    }
    builder.build().unwrap().compile().unwrap()
}

fn measure(processes: usize, iterations: usize) -> (f64, f64, f64, f64, f64) {
    assert!(processes > 0 && iterations > 0);
    let start_build = Instant::now();
    let network = compiled(processes);
    let build_ms = start_build.elapsed().as_secs_f64() * 1000.0;
    let start_session = Instant::now();
    let mut session = network.start();
    let start_ms = start_session.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(session.state(), SessionState::Quiescent);

    let mut run_one = |value: u64| {
        let request = session.begin_request(None).unwrap();
        session
            .inject_request_local(request, "input-0", black_box(value))
            .unwrap();
        session.close_request(request).unwrap();
        let report = session.run_until_idle(32);
        assert!(!report.budget_exhausted);
        assert_eq!(report.steps, 1);
        assert_eq!(
            session.request_status(request).unwrap(),
            RequestStatus::Settled { captures: 0 }
        );
        session.release_request(request).unwrap();
        assert_eq!(session.state(), SessionState::Quiescent);
    };
    for value in 0..64 {
        run_one(value);
    }
    let mut samples = Vec::with_capacity(iterations);
    let begin = Instant::now();
    for value in 0..iterations {
        let start = Instant::now();
        run_one(value as u64);
        samples.push(start.elapsed().as_nanos() as f64);
    }
    let mean = begin.elapsed().as_nanos() as f64 / iterations as f64;
    samples.sort_by(|a, b| a.total_cmp(b));
    let p50 = samples[(iterations - 1) / 2];
    let p95 = samples[(((iterations - 1) as f64) * 0.95).round() as usize];
    (build_ms, start_ms, mean, p50, p95)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let iterations: usize = args
        .next()
        .unwrap_or_else(|| "5000".into())
        .parse()
        .expect("positive iteration count");
    let processes: Vec<usize> = args
        .map(|arg| arg.parse().expect("positive process count"))
        .collect();
    let processes = if processes.is_empty() {
        vec![1, 16, 64, 256]
    } else {
        processes
    };
    eprintln!(
        "scenario=one_scoped_sealed_request_plus_unrelated_open_idle_sinks; \
         physical_input_capacity=1; warmup=64; iterations={iterations}; \
         persistent_session=true; request_status=settled_zero; input=u64_local"
    );
    println!("processes,events,compile_ms,start_ms,mean_ns_per_request,p50_ns,p95_ns");
    for count in processes {
        let (build_ms, start_ms, mean, p50, p95) = measure(count, iterations);
        println!("{count},{iterations},{build_ms:.3},{start_ms:.3},{mean:.1},{p50:.0},{p95:.0}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_sparse_one_step_and_settlement() {
        for count in [1, 8, 32] {
            let values = measure(count, 4);
            assert!(values.2.is_finite());
        }
    }

    #[test]
    fn two_concurrent_scopes_keep_both_distinct_ingress_roots_active() {
        let mut session = compiled(16).start();
        let left = session.begin_request(None).unwrap();
        let right = session.begin_request(None).unwrap();
        session
            .inject_request_local(left, "input-0", 10_u64)
            .unwrap();
        session
            .inject_request_local(right, "input-15", 20_u64)
            .unwrap();
        session.close_request(left).unwrap();
        session.close_request(right).unwrap();
        let report = session.run_until_idle(32);
        assert_eq!(report.steps, 2);
        assert!(!report.budget_exhausted);
        for id in [left, right] {
            assert_eq!(
                session.request_status(id).unwrap(),
                RequestStatus::Settled { captures: 0 }
            );
            session.release_request(id).unwrap();
        }
        assert_eq!(session.state(), SessionState::Quiescent);
    }
}
