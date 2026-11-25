//! Real Composer priority-preemption: selected primitive implementation
//! changes while the compiled process graph's ports and wiring remain fixed.
use phenix_graph::{
    ComponentDef, GraphOrigin,
    compose::{Composer, Edit, Footprint, NetworkPatch, Registration},
    network::NetworkBuilder,
};
use std::{hint::black_box, sync::Arc, time::Instant};

fn fixture<const N: usize>() -> Composer {
    let mut builder = NetworkBuilder::new();
    let passthrough = Arc::new(ComponentDef::map(|n: i64| n));
    for i in 0..N {
        builder
            .add(format!("pass{i:04}"), passthrough.clone())
            .unwrap();
    }
    builder
        .add("worker", Arc::new(ComponentDef::map(|n: i64| n)))
        .unwrap();
    let mut composer = Composer::new(builder.build().unwrap()).unwrap();
    composer
        .register([Registration::new(
            "low",
            [],
            1,
            GraphOrigin::default(),
            [Footprint::Component("worker".into())],
            NetworkPatch::new([Edit::Replace {
                path: "worker".into(),
                definition: Arc::new(ComponentDef::map(|n: i64| n * 2)),
            }]),
        )])
        .unwrap();
    assert_eq!(composer.current().active().len(), 1);
    composer
}

fn preempt<const N: usize>() -> u128 {
    // Only the real high-priority admission is timed, not fixture creation,
    // initial compiler invocation, or initial low-priority selection.
    let mut composer = fixture::<N>();
    let replacement = Registration::new(
        "high",
        [],
        9,
        GraphOrigin::default(),
        [Footprint::Component("worker".into())],
        NetworkPatch::new([Edit::Replace {
            path: "worker".into(),
            definition: Arc::new(ComponentDef::map(|n: i64| n * 3)),
        }]),
    );
    let started = Instant::now();
    composer.register([replacement]).unwrap();
    let duration = started.elapsed().as_nanos();
    assert!(composer.current().active().contains("high"));
    assert!(!composer.current().active().contains("low"));
    black_box(composer.current().compiled().processes().len());
    duration
}

fn measure<const N: usize>() {
    const UPDATES: usize = 6;
    for _ in 0..2 {
        for _ in 0..UPDATES {
            black_box(preempt::<N>());
        }
    }
    let mut samples = Vec::new();
    for _ in 0..7 {
        let sum: u128 = (0..UPDATES).map(|_| preempt::<N>()).sum();
        samples.push(sum / UPDATES as u128);
    }
    println!("q2_root_replace_{N}: samples_ns={samples:?}");
    samples.sort_unstable();
    println!("q2_root_replace_{N}: median_ns={}", samples[3]);
}

fn main() {
    println!(
        "q2_root_replace: 2 warmup groups + 7 measured groups of 6; only Composer high-priority replacement admission timed, fixtures excluded; no RSS"
    );
    measure::<16>();
    measure::<64>();
    measure::<128>();
    measure::<256>();
    measure::<512>();
}
