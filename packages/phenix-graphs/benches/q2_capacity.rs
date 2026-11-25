//! Paired Q2 selected connection-capacity preemption through the production
//! Composer, including the full planner, replay and generation publication.
use phenix_graph::{
    ComponentDef, GraphOrigin,
    compose::{Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration},
    network::{ConnectionDef, NetworkBuilder, PortPath},
};
use std::{hint::black_box, num::NonZeroUsize, sync::Arc, time::Instant};

fn edge(capacity: usize) -> ConnectionDef {
    ConnectionDef {
        from: PortPath {
            process: "source".into(),
            port: "output".into(),
        },
        to: PortPath {
            process: "worker".into(),
            port: "input".into(),
        },
        capacity: NonZeroUsize::new(capacity).unwrap(),
    }
}

fn connect(id: &str, priority: i64, capacity: usize) -> Registration {
    let connection = edge(capacity);
    Registration::new(
        id,
        [],
        priority,
        GraphOrigin::default(),
        [Footprint::Connection(ConnectionKey::new(
            connection.from.clone(),
            connection.to.clone(),
        ))],
        NetworkPatch::new([Edit::Connect(connection)]),
    )
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
    composer.register([connect("low", 0, 1)]).unwrap();
    assert_eq!(
        composer.current().compiled().connections()[0].capacity(),
        NonZeroUsize::new(1).unwrap()
    );
    composer
}

fn preempt<const N: usize>() -> u128 {
    let mut composer = fixture::<N>();
    let start = Instant::now();
    composer.register([connect("high", 8, 4)]).unwrap();
    let elapsed = start.elapsed().as_nanos();
    assert_eq!(
        composer.current().compiled().connections()[0].capacity(),
        NonZeroUsize::new(4).unwrap()
    );
    black_box(composer.current().compiled().processes().len());
    elapsed
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
        let sum_ns: u128 = (0..UPDATES).map(|_| preempt::<N>()).sum();
        samples.push(sum_ns / UPDATES as u128);
    }
    println!("q2_capacity_{N}: samples_ns={samples:?}");
    samples.sort_unstable();
    println!("q2_capacity_{N}: median_ns={}", samples[3]);
}

fn main() {
    println!(
        "q2_capacity: real Composer Connect preemption 1->4; 2 warmups + 7 timed groups of 6; fixture setup excluded; no RSS"
    );
    measure::<16>();
    measure::<64>();
    measure::<128>();
    measure::<256>();
    measure::<512>();
}
