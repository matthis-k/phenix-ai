//! Production Composer source-start revocation: changed selected writes,
//! frozen physical graph, and a fresh generation on successful publication.
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
        .add(
            "started",
            Arc::new(ComponentDef::source(|| std::iter::once(7_i64))),
        )
        .unwrap();
    let mut composer = Composer::new(builder.build().unwrap()).unwrap();
    composer
        .register([Registration::new(
            "start",
            [],
            10,
            GraphOrigin::default(),
            [Footprint::Source("started".into())],
            NetworkPatch::new([Edit::AddSource("started".into())]),
        )])
        .unwrap();
    assert_eq!(composer.current().compiled().source_starts().len(), 1);
    composer
}

fn revoke<const N: usize>() -> u128 {
    // Exclude initial compilation and registration from the measured call.
    let mut composer = fixture::<N>();
    let started = Instant::now();
    let removed = composer.revoke("start").unwrap();
    let elapsed = started.elapsed().as_nanos();
    assert_eq!(removed.len(), 1);
    assert!(composer.current().compiled().source_starts().is_empty());
    black_box(composer.current().compiled().processes().len());
    elapsed
}

fn measure<const N: usize>() {
    const REVOKES: usize = 6;
    for _ in 0..2 {
        for _ in 0..REVOKES {
            black_box(revoke::<N>());
        }
    }
    let mut medians = Vec::new();
    for _ in 0..7 {
        let sum_ns: u128 = (0..REVOKES).map(|_| revoke::<N>()).sum();
        medians.push(sum_ns / REVOKES as u128);
    }
    println!("q2_source_prune_{N}: samples_ns={medians:?}");
    medians.sort_unstable();
    println!("q2_source_prune_{N}: median_ns={}", medians[3]);
}

fn main() {
    println!("q2_source_prune benchmark: per-source-revoke only, 2 warmup groups + 7 timed groups of 6 calls; every call uses a fresh preregistered fixture; no RSS instrumentation");
    measure::<16>();
    measure::<64>();
    measure::<128>();
    measure::<256>();
    measure::<512>();
}
