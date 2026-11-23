//! Current-API H5/H6 lane matrix, separate from the standalone benchmark copied
//! to historical runtimes by paired CI.
//!
//! cargo run --locked --release -p phenix-graph --example fbp_lane_closure_benchmark -- 21 16
use phenix_graph::{
    Value,
    runtime::{RequestQueuePolicy, RequestStatus, SessionOptions, SessionState},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

// Reuse the real workload; its standalone main and other measurement modes are
// intentionally unused here. The historical CI copy remains self-contained.
#[allow(dead_code)]
#[path = "fbp_closure_benchmark.rs"]
mod workload;

fn main() {
    let mut args = std::env::args().skip(1);
    let iterations = args.next().unwrap_or_else(|| "9".into()).parse().unwrap();
    let breadth = args.next().unwrap_or_else(|| "16".into()).parse().unwrap();
    measure_lane_matrix(iterations, breadth);
}

/// Time only execution of overlapping closures, not graph/session setup,
/// admission/sealing, assertions or release. Every sample reuses lane slots.
fn measure_lane_matrix(iterations: usize, breadth: usize) {
    assert!(iterations > 0 && breadth > 0);
    println!("policy,lanes,chain_nodes,sample,batch_ns,ns_per_request");
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for lanes in [1, 4, 16, 64] {
            let hits = Arc::new(AtomicUsize::new(0));
            let network = workload::compiled_with_capacity(
                "chain",
                breadth,
                hits.clone(),
                NonZeroUsize::new(lanes).unwrap(),
            );
            let mut session = network.start_with_options(SessionOptions {
                max_requests: NonZeroUsize::new(lanes).unwrap(),
                request_queue_policy: policy,
                ..workload::readiness_options("chain", breadth)
            });
            for sample in 0..iterations + 2 {
                let ids: Vec<_> = (0..lanes)
                    .map(|_| session.begin_request(None).unwrap())
                    .collect();
                for &id in &ids {
                    session
                        .inject_request(id, "trigger", Value::of(1_u64))
                        .unwrap();
                    session.close_request(id).unwrap();
                }
                let started = Instant::now();
                let report = session.run_until_idle(lanes * (breadth + 1) * 4 + 32);
                let elapsed = started.elapsed().as_nanos();
                assert!(!report.budget_exhausted);
                assert_eq!(report.steps, lanes * (breadth + 1));
                for id in ids {
                    assert_eq!(
                        session.request_status(id).unwrap(),
                        RequestStatus::Settled { captures: 0 }
                    );
                    session.release_request(id).unwrap();
                }
                assert_eq!(session.state(), SessionState::Quiescent);
                assert_eq!(hits.load(Ordering::Relaxed), (sample + 1) * lanes * breadth);
                if sample >= 2 {
                    println!(
                        "{policy:?},{lanes},{breadth},{},{elapsed},{:.1}",
                        sample - 2,
                        elapsed as f64 / lanes as f64
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lane_matrix_preserves_both_policies_and_all_live_lane_counts() {
        super::measure_lane_matrix(1, 2);
    }
}
