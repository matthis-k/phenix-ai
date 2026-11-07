//! Run `cargo bench --bench ownership` to compare report retention strategies.
//! No machine-independent throughput assertion is made.
use phenix_graph::{Deployment, GraphBuilder, ReportRetention, Value};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use std::time::Instant;

#[derive(Debug)]
struct Tracked { value: i64, clones: Arc<AtomicUsize> }
impl Clone for Tracked {
    fn clone(&self) -> Self {
        self.clones.fetch_add(1, Ordering::Relaxed);
        Self { value: self.value, clones: self.clones.clone() }
    }
}

fn main() {
    for retention in [ReportRetention::TerminalOutputs, ReportRetention::RetainPayloads] {
        let clones = Arc::new(AtomicUsize::new(0));
        let mut graph = GraphBuilder::new("ownership-bench");
        let first = graph.node("first", |v: Tracked| v).unwrap();
        let mut previous = first.output();
        for i in 0..16 {
            let node = graph.node(format!("step{i}"), |mut v: Tracked| {
                v.value += 1;
                v
            }).unwrap();
            graph.connect(previous, node.inlet()).unwrap();
            previous = node.output();
        }
        let input = graph.expose_input("request", first.inlet()).unwrap();
        let trigger = graph.trigger("run", [input]).unwrap();
        let engine = Deployment::new(graph.compile().unwrap()).report_retention(retention).build().unwrap();
        let runs = 1_000;
        let started = Instant::now();
        for _ in 0..runs {
            let report = engine.fire(trigger.get(), vec![Value::local(Tracked {
                value: 1, clones: clones.clone(),
            })]).unwrap();
            assert!(report.success(), "{:?}", report.error_messages());
        }
        println!("{retention:?} runs={runs} time={:?} payload_clones={}",
            started.elapsed(), clones.load(Ordering::Relaxed));
    }
}
