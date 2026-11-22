//! Real Composer direct root edge insertion, including full global selection,
//! replay and publication. Only the registration mutation is timed.
use phenix_graph::{
    ComponentDef, GraphOrigin,
    compose::{Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration},
    network::{ConnectionDef, NetworkBuilder, PortPath},
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
        .expose_output("out", worker.output::<i64>("output").unwrap())
        .unwrap();
    let composer = Composer::new(builder.build().unwrap()).unwrap();
    let edge = ConnectionDef {
        from: PortPath {
            process: "alpha".into(),
            port: "output".into(),
        },
        to: PortPath {
            process: "omega".into(),
            port: "input".into(),
        },
        capacity: NonZeroUsize::new(1).unwrap(),
    };
    let registration = Registration::new(
        "edge",
        [],
        3,
        GraphOrigin::default(),
        [Footprint::Connection(ConnectionKey::new(
            edge.from.clone(),
            edge.to.clone(),
        ))],
        NetworkPatch::new([Edit::Connect(edge)]),
    );
    assert!(composer.current().compiled().connections().is_empty());
    (composer, registration)
}

fn add<const N: usize>() -> u128 {
    let (mut composer, registration) = fixture::<N>();
    let start = Instant::now();
    composer.register([registration]).unwrap();
    let elapsed = start.elapsed().as_nanos();
    assert_eq!(composer.current().compiled().processes().len(), N + 2);
    assert_eq!(composer.current().compiled().connections().len(), 1);
    black_box(composer.current().compiled().connections()[0].capacity());
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
    println!("q2_edge_add_{N}: samples_ns={samples:?}");
    samples.sort_unstable();
    println!("q2_edge_add_{N}: median_ns={}", samples[3]);
}

fn main() {
    println!(
        "q2_edge_add: real Composer direct root primitive edge insertion; 2 warmups + 7 groups of 6; fixture construction excluded; no RSS"
    );
    measure::<16>();
    measure::<64>();
    measure::<128>();
    measure::<256>();
    measure::<512>();
}
