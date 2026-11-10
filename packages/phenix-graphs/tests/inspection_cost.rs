use phenix_graph::{DeclaredValue, GraphBuilder, GraphContract, GraphUnit, InterfaceShape};
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

#[test]
#[ignore = "manual timing measurement; no timing assertions"]
fn repeated_inspection() {
    fn median(mut measure: impl FnMut()) -> f64 {
        measure();
        let mut samples = (0..11)
            .map(|_| {
                let start = Instant::now();
                for _ in 0..16 {
                    measure();
                }
                start.elapsed()
            })
            .collect::<Vec<_>>();
        samples.sort();
        samples[5].as_secs_f64() * 1_000_000.0 / 16.0
    }
    let contract = GraphContract::new(
        "bench/output@1",
        InterfaceShape::new()
            .output("value", DeclaredValue::local::<i64>())
            .unwrap(),
    )
    .unwrap();
    println!("shape,nodes,path,repeat_snapshot,repeat_contract,repeat_output_attempt,catalog");
    for nested in [false, true] {
        for count in [10, 100, 1000] {
            let mut service = GraphBuilder::new("service");
            let worker = service.node("worker", |x: i64| x).unwrap();
            service.expose_output("value", worker.output()).unwrap();
            if nested {
                let mut leaf = GraphBuilder::new("leaf");
                leaf.node("worker", |x: i64| x).unwrap();
                let leaf = leaf.module();
                let mut wrapper = GraphBuilder::new("wrapper");
                for i in 0..10 {
                    wrapper.instance(format!("leaf{i}"), &leaf).unwrap();
                }
                let wrapper = wrapper.module();
                for i in 0..count / 10 {
                    service.instance(format!("wrapper{i}"), &wrapper).unwrap();
                }
            } else {
                for i in 1..count {
                    service.node(format!("worker{i}"), |x: i64| x).unwrap();
                }
            }
            service.implements_public(&contract).unwrap();
            let mut graph = GraphBuilder::new("root");
            graph.instance("service", &service.module()).unwrap();
            let output = graph
                .discover("service")
                .unwrap()
                .output::<i64, phenix_graph::LocalValue>()
                .unwrap();
            graph.expose_output("value", output).unwrap();
            graph.implements_public(&contract).unwrap();
            let compiled = graph.clone().compile().unwrap();
            for path in ["", "service"] {
                let view = graph.inspect(path).unwrap();
                let bound = graph.bind_contract(path, &contract).unwrap();
                let snapshot = median(|| {
                    black_box(view.inspect().unwrap());
                });
                let check = median(|| {
                    contract.check_implementation(black_box(&view)).unwrap();
                });
                // Root typed endpoint lookup is currently unsupported; measure its rejection too.
                assert_eq!(
                    bound
                        .operation()
                        .unwrap()
                        .output::<i64, phenix_graph::LocalValue>()
                        .is_ok(),
                    !path.is_empty()
                );
                let output = median(|| {
                    let _ = black_box(
                        bound
                            .operation()
                            .unwrap()
                            .output::<i64, phenix_graph::LocalValue>(),
                    );
                });
                let catalog = median(|| {
                    compiled.check_contract_catalog([&contract]).unwrap();
                });
                println!(
                    "{},{},{path:?},{snapshot:.1},{check:.1},{output:.1},{catalog:.1}",
                    if nested { "nested" } else { "named" },
                    if nested { count + 1 } else { count }
                );
            }
        }
    }
}
