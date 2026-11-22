//! Real Composer selected revocation of eight independent primitive roots.
//! Fixture build and selected admission are outside the timed region.
use phenix_graph::{
    ComponentDef, GraphOrigin,
    compose::{Composer, Edit, Footprint, NetworkPatch, Registration},
    network::NetworkBuilder,
};
use std::{hint::black_box, num::NonZeroUsize, sync::Arc, time::Instant};

const BATCH: usize = 8;

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
    let definition = Arc::new(ComponentDef::map(|n: i64| n));
    let patch = NetworkPatch::new((0..BATCH).map(|i| Edit::Add {
        path: format!("batch{i:02}"),
        definition: definition.clone(),
    }));
    let footprints = (0..BATCH)
        .map(|i| Footprint::Component(format!("batch{i:02}")))
        .collect::<Vec<_>>();
    composer
        .register([Registration::new(
            "pack",
            [],
            3,
            GraphOrigin::default(),
            footprints,
            patch,
        )])
        .unwrap();
    assert_eq!(composer.current().compiled().processes().len(), N + 2 + BATCH);
    composer
}

fn revoke<const N: usize>() -> u128 {
    let mut composer = fixture::<N>();
    let start = Instant::now();
    let removed = composer.revoke("pack").unwrap();
    let elapsed = start.elapsed().as_nanos();
    assert_eq!(removed.len(), 1);
    assert_eq!(composer.current().compiled().processes().len(), N + 2);
    assert!(composer.current().compiled().inspect("batch00").is_err());
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
    println!("q2_batch_isolated_{N}: samples_ns={samples:?}");
    samples.sort_unstable();
    println!("q2_batch_isolated_{N}: median_ns={}", samples[3]);
}

fn main() {
    println!(
        "q2_batch_isolated: real Composer 8 primitive selected revoke; 2 warmups + 7 timed batches of 6; fixture/admission excluded; no RSS"
    );
    measure::<16>();
    measure::<64>();
    measure::<128>();
    measure::<256>();
    measure::<512>();
}
