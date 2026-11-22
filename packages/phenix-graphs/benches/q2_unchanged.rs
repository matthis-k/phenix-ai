//! Q2 production Composer benchmark: shadowed edits must still be checked,
//! but an unchanged selected topology need not be replayed or recompiled.
use phenix_graph::{
    ComponentDef, GraphOrigin,
    compose::{Composer, Edit, Footprint, NetworkPatch, Registration},
    network::NetworkBuilder,
};
use std::{hint::black_box, sync::Arc, time::Instant};

fn fixture<const N: usize>() -> Composer {
    let mut builder = NetworkBuilder::new();
    let original = Arc::new(ComponentDef::map(|n: i64| n));
    for i in 0..N {
        builder
            .add(format!("node{i:04}"), original.clone())
            .unwrap();
    }
    let mut composer = Composer::new(builder.build().unwrap()).unwrap();
    let patch = NetworkPatch::new([Edit::Replace {
        path: "node0000".into(),
        definition: Arc::new(ComponentDef::map(|n: i64| n + 1)),
    }]);
    composer
        .register([Registration::new(
            "winner",
            [],
            10,
            GraphOrigin::default(),
            patch.footprints(),
            patch,
        )])
        .unwrap();
    composer
}

fn shadowed_update(composer: &mut Composer) {
    let patch = NetworkPatch::new([Edit::Replace {
        path: "node0000".into(),
        definition: Arc::new(ComponentDef::map(|n: i64| n + 2)),
    }]);
    composer
        .register([Registration::new(
            "shadowed",
            [],
            0,
            GraphOrigin::default(),
            [Footprint::Component("node0000".into())],
            patch,
        )])
        .unwrap();
    assert_eq!(composer.current().active().len(), 1);
    let removed = composer.revoke("shadowed").unwrap();
    assert_eq!(removed.len(), 1);
    black_box(composer.current().compiled().processes().len());
}

fn measure<const N: usize>() {
    let mut composer = fixture::<N>();
    assert_eq!(composer.current().compiled().processes().len(), N);
    const UPDATES: usize = 8;
    for _ in 0..2 {
        for _ in 0..UPDATES {
            shadowed_update(&mut composer);
        }
    }
    let mut samples = Vec::new();
    for _ in 0..7 {
        let start = Instant::now();
        for _ in 0..UPDATES {
            shadowed_update(&mut composer);
        }
        // Average microseconds per admitted+revoked shadowed registration.
        samples.push(start.elapsed().as_micros() / UPDATES as u128);
    }
    println!("compose_q2_shadowed{N}_admit_revoke: samples_us={samples:?}");
    samples.sort_unstable();
    println!(
        "compose_q2_shadowed{N}_admit_revoke: median_us={}",
        samples[3]
    );
}

fn main() {
    println!(
        "q2_shadowed: warmup=2x8 samples=7x8; each sample excludes base fixture; no allocator/RSS instrumentation"
    );
    measure::<16>();
    measure::<64>();
    measure::<128>();
    measure::<256>();
}
