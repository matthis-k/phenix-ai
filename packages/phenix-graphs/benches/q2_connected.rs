//! Real Composer mixed primitive Add + two direct Connect edits. Measures
//! registration, selection, replay and publication. Fixture creation excluded.
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
        .add("omega", Arc::new(ComponentDef::map(|n: i64| n * 3)))
        .unwrap();
    builder
        .expose_output("out", worker.output::<i64>("output").unwrap())
        .unwrap();
    let composer = Composer::new(builder.build().unwrap()).unwrap();
    let from = edge("alpha", "middle");
    let to = edge("middle", "omega");
    let registration = Registration::new(
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
    );
    assert_eq!(composer.current().compiled().processes().len(), N + 2);
    (composer, registration)
}

fn add_connected<const N: usize>() -> u128 {
    let (mut composer, registration) = fixture::<N>();
    let start = Instant::now();
    composer.register([registration]).unwrap();
    let elapsed = start.elapsed().as_nanos();
    assert_eq!(composer.current().compiled().processes().len(), N + 3);
    assert_eq!(composer.current().compiled().connections().len(), 2);
    black_box(composer.current().compiled().connections().len());
    elapsed
}

fn measure<const N: usize>() {
    const UPDATES: usize = 6;
    for _ in 0..2 {
        for _ in 0..UPDATES {
            black_box(add_connected::<N>());
        }
    }
    let mut samples = Vec::new();
    for _ in 0..7 {
        let sum_ns: u128 = (0..UPDATES).map(|_| add_connected::<N>()).sum();
        samples.push(sum_ns / UPDATES as u128);
    }
    println!("q2_connected_{N}: samples_ns={samples:?}");
    samples.sort_unstable();
    println!("q2_connected_{N}: median_ns={}", samples[3]);
}

fn main() {
    println!(
        "q2_connected: real Composer Add middle + 2 root Connect; 2 warmups + 7 groups of 6; excludes fixture construction; no RSS"
    );
    measure::<16>();
    measure::<64>();
    measure::<128>();
    measure::<256>();
    measure::<512>();
}
