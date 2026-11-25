//! Full Composer connected root primitive revocation. Fixture construction
//! and selected Add+Connect admission are untimed.
use phenix_graph::{
    ComponentDef, GraphOrigin,
    compose::{Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration},
    network::{ConnectionDef, NetworkBuilder, PortPath},
};
use std::{hint::black_box, num::NonZeroUsize, sync::Arc, time::Instant};

fn edge(from: &str, to: &str) -> ConnectionDef {
    ConnectionDef {
        from: PortPath {
            process: from.into(),
            port: "output".into(),
        },
        to: PortPath {
            process: to.into(),
            port: "input".into(),
        },
        capacity: NonZeroUsize::new(2).unwrap(),
    }
}

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
        .add("omega", Arc::new(ComponentDef::map(|n: i64| n * 3)))
        .unwrap();
    builder
        .connect(
            source.output::<i64>("output").unwrap(),
            worker.input::<i64>("input").unwrap(),
            NonZeroUsize::new(2).unwrap(),
        )
        .unwrap();
    builder
        .expose_output("out", worker.output::<i64>("output").unwrap())
        .unwrap();
    let mut composer = Composer::new(builder.build().unwrap()).unwrap();
    let from = edge("alpha", "middle");
    let to = edge("middle", "omega");
    composer
        .register([Registration::new(
            "chain",
            [],
            3,
            GraphOrigin::default(),
            [
                Footprint::Component("middle".into()),
                Footprint::Connection(ConnectionKey::new(from.from.clone(), from.to.clone())),
                Footprint::Connection(ConnectionKey::new(to.from.clone(), to.to.clone())),
            ],
            NetworkPatch::new([
                Edit::Add {
                    path: "middle".into(),
                    definition: Arc::new(ComponentDef::map(|n: i64| n + 1)),
                },
                Edit::Connect(from),
                Edit::Connect(to),
            ]),
        )])
        .unwrap();
    assert_eq!(composer.current().compiled().processes().len(), N + 3);
    assert_eq!(composer.current().compiled().connections().len(), 3);
    composer
}

fn revoke<const N: usize>() -> u128 {
    let mut composer = fixture::<N>();
    let start = Instant::now();
    let removed = composer.revoke("chain").unwrap();
    let elapsed = start.elapsed().as_nanos();
    assert_eq!(removed.len(), 1);
    assert_eq!(composer.current().compiled().processes().len(), N + 2);
    assert_eq!(composer.current().compiled().connections().len(), 1);
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
    println!("q2_connected_revoke_{N}: samples_ns={samples:?}");
    samples.sort_unstable();
    println!("q2_connected_revoke_{N}: median_ns={}", samples[3]);
}

fn main() {
    println!(
        "q2_connected_revoke: real Composer connected root revocation; 2 warmups + 7 groups of 6; excludes fixture/admission; no RSS"
    );
    measure::<16>();
    measure::<64>();
    measure::<128>();
    measure::<256>();
    measure::<512>();
}
