use phenix_graph::GraphBuilder;
use std::time::Instant;

#[test]
#[ignore = "manual registration timing"]
fn registration_timing() {
    for count in [10, 100, 1000] {
        let start = Instant::now();
        for _ in 0..20 {
            let mut graph = GraphBuilder::new("registration");
            for index in 0..count {
                graph
                    .additions(|g| g.node(format!("node_{index}"), |v: i64| v))
                    .unwrap();
            }
            std::hint::black_box(graph);
        }
        eprintln!("{count} additions: {:?}", start.elapsed() / 20);
    }
}
