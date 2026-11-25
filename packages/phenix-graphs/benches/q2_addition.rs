//! Paired real Composer isolated-root admission with full deterministic
//! planning, replay and publication. Fixture creation is excluded from timing.
use phenix_graph::{
    ComponentDef, GraphOrigin,
    compose::{Composer, Edit, Footprint, NetworkPatch, Registration},
    network::NetworkBuilder,
};
use std::{hint::black_box, num::NonZeroUsize, sync::Arc, time::Instant};

fn fixture<const N: usize>() -> (Composer, Registration) {
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
    let composer = Composer::new(builder.build().unwrap()).unwrap();
    let patch = NetworkPatch::new([Edit::Add {
        path: "middle".into(),
        definition: Arc::new(ComponentDef::map(|n: i64| n)),
    }]);
    let registration = Registration::new(
        "orphan",
        [],
        3,
        GraphOrigin::default(),
        [Footprint::Component("middle".into())],
        patch,
    );
    assert_eq!(composer.current().compiled().processes().len(), N + 2);
    (composer, registration)
}

fn add<const N: usize>() -> u128 {
    let (mut composer, registration) = fixture::<N>();
    let start = Instant::now();
    composer.register([registration]).unwrap();
    let elapsed = start.elapsed().as_nanos();
    assert_eq!(composer.current().compiled().processes().len(), N + 3);
    assert_eq!(
        composer
            .current()
            .compiled()
            .inspect("middle")
            .unwrap()
            .physical_index,
        Some(1)
    );
    black_box(composer.current().compiled().connections().len());
    elapsed
}

fn measure<const N: usize>() {
    const UPDATES: usize = 6;
    for _ in 0..2 {
        for _ in 0..UPDATES {
            black_box(add::<N>());
        }
    }
    let mut samples = Vec::new();
    for _ in 0..7 {
        let sum_ns: u128 = (0..UPDATES).map(|_| add::<N>()).sum();
        samples.push(sum_ns / UPDATES as u128);
    }
    println!("q2_addition_{N}: samples_ns={samples:?}");
    samples.sort_unstable();
    println!("q2_addition_{N}: median_ns={}", samples[3]);
}

fn main() {
    println!(
        "q2_addition: real Composer isolated middle root admission; 2 warmups + 7 timed groups of 6; fixture construction excluded; no RSS"
    );
    measure::<16>();
    measure::<64>();
    measure::<128>();
    measure::<256>();
    measure::<512>();
}
