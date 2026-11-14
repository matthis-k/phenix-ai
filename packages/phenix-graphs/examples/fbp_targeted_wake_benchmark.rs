//! H4: targeted external wake latency among unrelated packet and custom nodes.
//!
//! cargo run --locked --release -p phenix-graph --example fbp_targeted_wake_benchmark -- 3000 1 16 64 256
use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::{Process, ProcessContext, Step},
    runtime::{Readiness, ReadinessPolicy, ReadinessSnapshot, SessionOptions},
};
use std::{hint::black_box, num::NonZeroUsize, sync::Arc, time::Instant};

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

#[derive(Default)]
struct AwaitWake(bool);
impl Process for AwaitWake {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if !self.0 {
            let value = cx.take("in").ok_or("missing initial packet")?;
            black_box(value.get::<u64>()?);
            self.0 = true;
            Ok(Step::WaitWake)
        } else {
            self.0 = false;
            Ok(Step::Complete)
        }
    }
}

struct NeverReady;
impl ReadinessPolicy for NeverReady {
    fn select(&self, _snapshot: &ReadinessSnapshot) -> Vec<String> {
        vec![]
    }
}

fn compiled(processes: usize) -> phenix_graph::network::CompiledNetwork {
    let mut builder = NetworkBuilder::new();
    let iface = Arc::new(Interface::new([PortDef::wire::<u64>("in").unwrap()], []).unwrap());
    for index in 0..processes {
        let path = format!("waiter-{index}");
        let node = builder
            .node(path, iface.clone(), Arc::new(AwaitWake::default))
            .unwrap();
        builder
            .expose_input(
                format!("input-{index}"),
                node.wire_input::<u64>("in").unwrap(),
                one(),
            )
            .unwrap();
    }
    builder.build().unwrap().compile().unwrap()
}

fn measure(processes: usize, iterations: usize) -> (f64, f64, f64, f64, f64) {
    assert!(processes > 0 && iterations > 0);
    let begin = Instant::now();
    let network = compiled(processes);
    let compile_ms = begin.elapsed().as_secs_f64() * 1000.0;
    let mut opts = SessionOptions::default();
    // Force the mixed-graph scheduler path: a custom node exists, but only
    // waiter-0 receives inputs/wakes. Custom readiness must remain polled.
    if processes > 1 {
        opts.readiness
            .insert("waiter-1".into(), Readiness::Custom(Arc::new(NeverReady)));
    }
    let begin = Instant::now();
    let mut session = network.start_with_options(opts);
    let start_ms = begin.elapsed().as_secs_f64() * 1000.0;
    let wake = session.wake_handle("waiter-0").unwrap();
    let mut samples = Vec::with_capacity(iterations);
    let warmup = 64;
    for iteration in 0..(warmup + iterations) {
        session
            .inject("input-0", Value::of(iteration as u64))
            .unwrap();
        let (index, job) = session.next_job().expect("initial packet job");
        assert_eq!(index, 0);
        session.complete(index, job.run()).unwrap();
        let begin = Instant::now();
        assert!(wake.wake());
        let (index, job) = session.next_job().expect("targeted resumed job");
        assert_eq!(index, 0);
        session.complete(index, job.run()).unwrap();
        if iteration >= warmup {
            samples.push(begin.elapsed().as_nanos() as f64);
        }
    }
    let mean = samples.iter().sum::<f64>() / iterations as f64;
    samples.sort_by(|a, b| a.total_cmp(b));
    let p50 = samples[(iterations - 1) / 2];
    let p95 = samples[(((iterations - 1) as f64) * 0.95).round() as usize];
    (compile_ms, start_ms, mean, p50, p95)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let iterations: usize = args
        .next()
        .unwrap_or_else(|| "3000".into())
        .parse()
        .expect("positive iterations");
    let processes: Vec<usize> = args
        .map(|arg| arg.parse().expect("positive process count"))
        .collect();
    let processes = if processes.is_empty() {
        vec![1, 16, 64, 256]
    } else {
        processes
    };
    eprintln!(
        "scenario=one_targeted_waitwake_resume_in_persistent_mixed_graph;          other_nodes=open_idle; custom_policy=one_idle_node_for_n_gt_1;          input_capacity=1; warmup=64; iterations={iterations}"
    );
    println!("processes,events,compile_ms,start_ms,mean_ns_per_wake,p50_ns,p95_ns");
    for processes in processes {
        let (compile_ms, start_ms, mean, p50, p95) = measure(processes, iterations);
        println!(
            "{processes},{iterations},{compile_ms:.3},{start_ms:.3},{mean:.1},{p50:.0},{p95:.0}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targeted_wake_workload_completes_in_mixed_graph() {
        for processes in [1, 4, 32] {
            let result = measure(processes, 4);
            assert!(result.2.is_finite());
        }
    }
}
