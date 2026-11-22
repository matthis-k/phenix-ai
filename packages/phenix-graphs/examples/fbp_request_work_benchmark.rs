//! Repeated passive RequestWork snapshots while one request owns a shared
//! process and another has buffered input. All unrelated graph inputs remain
//! open. Measures end-to-end native diagnostic cost, excluding graph setup.
use phenix_graph::{
    ComponentDef,
    network::NetworkBuilder,
    runtime::{RequestQueuePolicy, SessionOptions},
};
use std::{
    hint::black_box,
    num::NonZeroUsize,
    sync::Arc,
    time::Instant,
};

fn benchmark(policy: RequestQueuePolicy, unrelated: usize) -> (f64, f64, f64, usize, usize) {
    let mut builder = NetworkBuilder::new();
    let target = builder
        .add("target", Arc::new(ComponentDef::sink(|_: &i64| {})))
        .unwrap();
    builder
        .expose_input(
            "input",
            target.input::<i64>("input").unwrap(),
            NonZeroUsize::new(4).unwrap(),
        )
        .unwrap();
    for index in 0..unrelated {
        let unrelated_node = builder
            .add(
                format!("unrelated-{index}"),
                Arc::new(ComponentDef::sink(|_: &i64| {})),
            )
            .unwrap();
        builder
            .expose_input(
                format!("unrelated-{index}"),
                unrelated_node.input::<i64>("input").unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
    }
    let network = builder.build().unwrap().compile().unwrap();
    let mut session = network.start_with_options(SessionOptions {
        max_requests: NonZeroUsize::new(4).unwrap(),
        request_queue_policy: policy,
        ..Default::default()
    });
    let requests: Vec<_> = (0..4)
        .map(|_| session.begin_request(None).unwrap())
        .collect();
    // First request reserves the physical process. Remaining requests are
    // genuinely queued, not synthetic entries in a diagnostic-only fixture.
    for (value, request) in requests.iter().enumerate() {
        session
            .inject_request_local(*request, "input", value as i64)
            .unwrap();
    }
    let inspected = requests[1];
    let before = session.request_work(inspected).unwrap();
    assert_eq!(before.queued, 1);
    assert_eq!(before.single_flight_blockers, 1);
    assert_eq!(before.capture_backpressure, 0);

    for _ in 0..512 {
        black_box(session.request_work(inspected).unwrap());
    }
    const BATCHES: usize = 24;
    const SNAPSHOTS: usize = 256;
    let mut samples = Vec::with_capacity(BATCHES);
    for _ in 0..BATCHES {
        let start = Instant::now();
        for _ in 0..SNAPSHOTS {
            black_box(session.request_work(inspected).unwrap());
        }
        samples.push(start.elapsed().as_nanos() as f64 / SNAPSHOTS as f64);
    }
    samples.sort_by(|a, b| a.total_cmp(b));
    let mean = samples.iter().sum::<f64>() / BATCHES as f64;
    let p50 = samples[(BATCHES - 1) / 2];
    let p95 = samples[(((BATCHES - 1) as f64) * 0.95).round() as usize];
    (mean, p50, p95, before.queued, before.single_flight_blockers)
}

fn main() {
    println!("policy,unrelated_roots,mean_ns_per_snapshot,p50_ns,p95_ns,queued,blockers");
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for unrelated in [0, 128, 512, 1024] {
            let (mean, p50, p95, queued, blockers) = benchmark(policy, unrelated);
            let name = match policy {
                RequestQueuePolicy::SharedFifo => "shared-fifo",
                RequestQueuePolicy::BoundedLanes => "bounded-lanes",
            };
            println!(
                "{name},{unrelated},{mean:.1},{p50:.1},{p95:.1},{queued},{blockers}"
            );
        }
    }
}
