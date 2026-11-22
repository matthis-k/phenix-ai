//! Selected primitive-to-primitive Connect registration revocation
//! through the real Composer, including full planning, replay and publication.
use phenix_graph::{
    ComponentDef, GraphOrigin,
    compose::{Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration},
    network::{ConnectionDef, NetworkBuilder, PortPath},
};
use std::{hint::black_box, num::NonZeroUsize, sync::Arc, time::Instant};

fn edge() -> ConnectionDef {
    ConnectionDef {
        from: PortPath {
            process: "source".into(),
            port: "output".into(),
        },
        to: PortPath {
            process: "worker".into(),
            port: "input".into(),
        },
        capacity: NonZeroUsize::new(1).unwrap(),
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
            "source",
            Arc::new(ComponentDef::source(|| std::iter::once(7_i64))),
        )
        .unwrap();
    builder.start(&source).unwrap();
    builder
        .add("worker", Arc::new(ComponentDef::map(|n: i64| n * 2)))
        .unwrap();
    let mut composer = Composer::new(builder.build().unwrap()).unwrap();
    let connection = edge();
    composer
        .register([Registration::new(
            "edge",
            [],
            0,
            GraphOrigin::default(),
            [Footprint::Connection(ConnectionKey::new(
                connection.from.clone(),
                connection.to.clone(),
            ))],
            NetworkPatch::new([Edit::Connect(connection)]),
        )])
        .unwrap();
    assert_eq!(composer.current().compiled().connections().len(), 1);
    composer
}

fn revoke<const N: usize>() -> u128 {
    let mut composer = fixture::<N>();
    let start = Instant::now();
    let removed = composer.revoke("edge").unwrap();
    let elapsed = start.elapsed().as_nanos();
    assert_eq!(removed.len(), 1);
    assert!(composer.current().compiled().connections().is_empty());
    black_box(composer.current().compiled().processes().len());
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
    println!("q2_edge_prune_{N}: samples_ns={samples:?}");
    samples.sort_unstable();
    println!("q2_edge_prune_{N}: median_ns={}", samples[3]);
}

fn main() {
    println!(
        "q2_edge_prune: real Composer Connect revocation only; 2 warmups + 7 timed groups of 6, fresh setup excluded, no RSS"
    );
    measure::<16>();
    measure::<64>();
    measure::<128>();
    measure::<256>();
    measure::<512>();
}
