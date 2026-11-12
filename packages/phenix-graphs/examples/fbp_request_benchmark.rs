//! H5: portable, paired benchmark for request membership in bounded FBP queues.
//!
//! Run this identical example against pre-index and indexed runtime commits
//! on the same runner. The setup keeps one process Ready so later request
//! packets remain in a real, bounded ingress queue.
//!
//! cargo run --locked --release -p phenix-graph --example fbp_request_benchmark -- 20000 8 64 256 1024
use phenix_graph::{
    component::ComponentDef,
    network::NetworkBuilder,
    runtime::{RequestStatus, SessionState},
};
use std::{hint::black_box, num::NonZeroUsize, sync::Arc, time::Instant};

fn mean_ns(iterations: usize, mut query: impl FnMut()) -> f64 {
    for _ in 0..64 {
        query();
    }
    let started = Instant::now();
    for _ in 0..iterations {
        query();
    }
    started.elapsed().as_nanos() as f64 / iterations as f64
}

fn measure(depth: usize, scopes: usize, iterations: usize) -> [f64; 4] {
    assert!(depth > 0 && scopes > 0 && iterations > 0);
    let mut builder = NetworkBuilder::new();
    let sink = builder
        .add(
            "sink",
            Arc::new(ComponentDef::sink(|value: &u64| {
                black_box(*value);
            })),
        )
        .unwrap();
    builder
        .expose_input(
            "input",
            sink.input::<u64>("input").unwrap(),
            NonZeroUsize::new(depth).unwrap(),
        )
        .unwrap();
    let network = builder.build().unwrap().compile().unwrap();
    let mut session = network.start();
    assert_eq!(session.state(), SessionState::Quiescent);

    // Needle has an authenticated, sealed scope, but no ingress or queued
    // packet. Neither the old linear scan nor the indexed lookup may infer
    // any foreign request activity from nearby packets.
    let needle = session.begin_request(None).unwrap();
    session.close_request(needle).unwrap();
    let filler: Vec<_> = (0..scopes)
        .map(|_| session.begin_request(None).unwrap())
        .collect();

    // The first admitted packet reserves the process and remains Ready.
    // The next `depth` packets fill one real physical ingress queue. No
    // worker steps run, so the queue remains stable across all queries.
    let began_fill = Instant::now();
    for value in 0..=depth {
        session
            .inject_request_local(filler[value % scopes], "input", black_box(value as u64))
            .unwrap();
    }
    let fill_ns = began_fill.elapsed().as_nanos() as f64 / (depth + 1) as f64;
    let held: usize = filler
        .iter()
        .map(|id| session.request_work(*id).unwrap().queued)
        .sum();
    assert_eq!(held, depth);
    assert_eq!(session.request_work(needle).unwrap().queued, 0);
    assert_eq!(
        session.request_status(needle).unwrap(),
        RequestStatus::Settled { captures: 0 }
    );

    // Each scenario is measured independently; the packet contents and
    // process state remain unchanged. No timings are used as correctness
    // thresholds. The same public API code compiles on both implementations.
    let status_absent_ns = mean_ns(iterations, || {
        black_box(session.request_status(needle).unwrap());
    });
    let work_absent_ns = mean_ns(iterations, || {
        black_box(session.request_work(needle).unwrap());
    });
    let work_present_ns = mean_ns(iterations, || {
        black_box(session.request_work(filler[0]).unwrap());
    });

    // Inspection must never reserve or dequeue packets, even when repeated
    // tens of thousands of times. Verify the exact count remains intact.
    let held_after: usize = filler
        .iter()
        .map(|id| session.request_work(*id).unwrap().queued)
        .sum();
    assert_eq!(held_after, depth);
    assert_eq!(session.request_work(needle).unwrap().queued, 0);
    [fill_ns, status_absent_ns, work_absent_ns, work_present_ns]
}

fn main() {
    let mut args = std::env::args().skip(1);
    let iterations: usize = args
        .next()
        .unwrap_or_else(|| "20000".into())
        .parse()
        .expect("iterations must be positive");
    assert!(iterations > 0);
    let depths: Vec<usize> = args
        .map(|arg| arg.parse::<usize>().expect("depth must be integer"))
        .collect();
    let depths = if depths.is_empty() {
        vec![8, 64, 256, 1024]
    } else {
        depths
    };
    eprintln!(
        "scenario=one_ready_sink_plus_bounded_queued_ingress;          comparisons=sealed_missing_request_work_and_status;          scopes=1_or_16; index_public_api_only; events_per_depth=depth+1;          repeated_inspection={iterations}"
    );
    println!(
        "queue_depth,scopes,iterations,fill_ns_per_packet,status_absent_ns,work_absent_ns,work_present_ns"
    );
    for depth in depths {
        for scopes in [1_usize, 16] {
            let results = measure(depth, scopes, iterations);
            println!(
                "{depth},{scopes},{iterations},{:.1},{:.1},{:.1},{:.1}",
                results[0], results[1], results[2], results[3]
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn request_membership_benchmark_smoke() {
        for scopes in [1, 2, 16] {
            let values = super::measure(16, scopes, 4);
            assert!(values.iter().all(|value| value.is_finite()));
        }
    }
}
