//! Real Composer isolated-root revocation, with the full host planner and
//! immutable compilation. Fixture construction and selected Add are untimed.
use phenix_graph::{
    ComponentDef, GraphOrigin,
    compose::{Composer, Edit, Footprint, NetworkPatch, Registration},
    network::NetworkBuilder,
};
use std::{hint::black_box, num::NonZeroUsize, sync::Arc, time::Instant};

fn fixture<const N: usize>() -> Composer {
    let mut builder = NetworkBuilder::new();
    let filler = Arc::new(ComponentDef::map(|n: i64| n));
    for i in 0..N {
        builder.add(format!("pass{i:04}"), filler.clone()).unwrap();
    }
    let source = builder
        .add(
            "alpha",
            Arc::new(ComponentDef::source(|| std::iter::once(7_i64))),
        )
        .unwrap();
    builder.start(&source).unwrap();
    let worker = builder
        .add("omega", Arc::new(ComponentDef::map(|n: i64| n * 2)))
        .unwrap();
    builder
        .connect(
            source.output::<i64>("output").unwrap(),
            worker.input::<i64>("input").unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    builder
        .expose_output("out", worker.output::<i64>("output").unwrap())
        .unwrap();
    let mut composer = Composer::new(builder.build().unwrap()).unwrap();
    let patch = NetworkPatch::new([Edit::Add {
        path: "middle".into(),
        definition: Arc::new(ComponentDef::map(|n: i64| n)),
    }]);
    composer
        .register([Registration::new(
            "orphan",
            [],
            3,
            GraphOrigin::default(),
            [Footprint::Component("middle".into())],
            patch,
        )])
        .unwrap();
    assert_eq!(composer.current().compiled().processes().len(), N + 3);
    composer
}

fn revoke<const N: usize>() -> u128 {
    let mut composer = fixture::<N>();
    let start = Instant::now();
    let removed = composer.revoke("orphan").unwrap();
    let elapsed = start.elapsed().as_nanos();
    assert_eq!(removed.len(), 1);
    assert_eq!(composer.current().compiled().processes().len(), N + 2);
    assert!(composer.current().compiled().inspect("middle").is_err());
    black_box(composer.current().compiled().connections().len());
    elapsed
}

fn measure<const N: usize>() {
    const UPDATES: usize = 6;
    for _ in 0..2 {
        for _ in 0..UPDATES {
            black_box(revoke::<N>());
        }
    }
    let mut samples = Vec::new();
    for _ in 0..7 {
        let sum_ns: u128 = (0..UPDATES).map(|_| revoke::<N>()).sum();
        samples.push(sum_ns / UPDATES as u128);
    }
    println!("q2_isolated_{N}: samples_ns={samples:?}");
    samples.sort_unstable();
    println!("q2_isolated_{N}: median_ns={}", samples[3]);
}

fn main() {
    println!(
        "q2_isolated: real Composer isolated middle root revocation; 2 warmups + 7 timed groups of 6; fixture construction excluded; no RSS"
    );
    measure::<16>();
    measure::<64>();
    measure::<128>();
    measure::<256>();
    measure::<512>();
}
