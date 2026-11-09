use phenix_graph::{GraphBuilder, GraphContract, GraphUnit, InterfaceShape};
use std::hint::black_box;
use std::time::{Duration, Instant};

// Run with: nix develop --command cargo test --locked -p phenix-graph --release
// --test inspection_cost -- --ignored --nocapture
#[test]
#[ignore = "manual timing measurement; no timing assertions"]
fn inspection_vs_compile() {
    fn median(mut measure: impl FnMut() -> Duration) -> f64 {
        measure(); // Warm up before collecting samples.
        let mut samples: Vec<Duration> =
            (0..11).map(|_| (0..16).map(|_| measure()).sum()).collect();
        samples.sort();
        samples[samples.len() / 2].as_secs_f64() * 1_000_000.0 / 16.0
    }

    let contract = GraphContract::new("bench/empty@1", InterfaceShape::new()).unwrap();
    println!(
        "median of 11 batches of 16 operations, microseconds/op; construction/cloning excluded"
    );
    println!("shape,nodes,inspect_root,inspect_then_snapshot,bind_root,implements_root,compile");
    for nested in [false, true] {
        for count in [10, 100, 1000] {
            let mut graph = GraphBuilder::new("root");
            if nested {
                let mut leaf = GraphBuilder::new("leaf");
                leaf.node("worker", |x: i64| x).unwrap();
                let leaf = leaf.module();
                let mut wrapper = GraphBuilder::new("wrapper");
                for index in 0..10 {
                    wrapper.instance(format!("leaf{index}"), &leaf).unwrap();
                }
                let wrapper = wrapper.module();
                for index in 0..count / 10 {
                    graph.instance(format!("wrapper{index}"), &wrapper).unwrap();
                }
            } else {
                for index in 0..count {
                    graph.node(format!("worker{index}"), |x: i64| x).unwrap();
                }
            }
            graph.implements_public(&contract).unwrap();
            assert_eq!(
                graph.clone().compile().unwrap().physical_node_count(),
                count
            );
            let inspect = median(|| {
                let start = Instant::now();
                black_box(graph.inspect("").unwrap());
                start.elapsed()
            });
            let inspect_snapshot = median(|| {
                let start = Instant::now();
                black_box(graph.inspect("").unwrap().inspect().unwrap());
                start.elapsed()
            });
            let bind = median(|| {
                let start = Instant::now();
                black_box(graph.bind_contract("", &contract).unwrap());
                start.elapsed()
            });
            let implements = median(|| {
                let mut copy = graph.clone();
                let start = Instant::now();
                copy.implements_public(&contract).unwrap();
                start.elapsed()
            });
            let compile = median(|| {
                let copy = graph.clone();
                let start = Instant::now();
                let compiled = black_box(copy.compile().unwrap());
                let elapsed = start.elapsed();
                black_box(compiled);
                elapsed
            });
            println!(
                "{},{count},{inspect:.1},{inspect_snapshot:.1},{bind:.1},{implements:.1},{compile:.1}",
                if nested { "nested" } else { "named" }
            );
        }
    }
}
