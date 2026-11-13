//! H4: paired global EOS propagation with unrelated writer-open branches.
//!
//! cargo run --locked --release -p phenix-graph --example fbp_targeted_eos_benchmark
use phenix_graph::{
    component::ComponentDef,
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::{Process, ProcessContext, Step},
    runtime::SessionState,
};
use std::{hint::black_box, num::NonZeroUsize, sync::Arc, time::Instant};

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

struct FinishOnEos;
impl Process for FinishOnEos {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if !cx.is_eos("in") {
            return Err("expected global EOS".into());
        }
        Ok(Step::Finished)
    }
}

fn compiled(chain: usize, unrelated: usize) -> phenix_graph::network::CompiledNetwork {
    assert!(chain > 0 && unrelated > 0);
    let mut builder = NetworkBuilder::new();
    let iface = Arc::new(
        Interface::new(
            [PortDef::wire::<i64>("in").unwrap()],
            [PortDef::wire::<i64>("out").unwrap()],
        )
        .unwrap(),
    );
    let mut nodes = Vec::new();
    for index in 0..chain {
        nodes.push(
            builder
                .node(format!("finish-{index}"), iface.clone(), Arc::new(|| FinishOnEos))
                .unwrap(),
        );
    }
    for index in 1..chain {
        builder
            .connect(
                nodes[index - 1].wire_output::<i64>("out").unwrap(),
                nodes[index].wire_input::<i64>("in").unwrap(),
                one(),
            )
            .unwrap();
    }
    builder
        .expose_input("seed", nodes[0].wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    builder
        .expose_output(
            "tail",
            nodes[chain - 1].wire_output::<i64>("out").unwrap(),
        )
        .unwrap();

    let sink = Arc::new(ComponentDef::sink(|value: &u64| {
        black_box(*value);
    }));
    for index in 0..unrelated {
        let idle = builder.add(format!("idle-{index}"), sink.clone()).unwrap();
        builder
            .expose_input(
                format!("idle-in-{index}"),
                idle.input::<u64>("input").unwrap(),
                one(),
            )
            .unwrap();
    }
    builder.build().unwrap().compile().unwrap()
}

fn measure(chain: usize, unrelated: usize, iterations: usize) -> (f64, f64, f64, f64) {
    let begin = Instant::now();
    let network = compiled(chain, unrelated);
    let compile_ms = begin.elapsed().as_secs_f64() * 1000.0;
    let warmup = 16;
    let mut samples = Vec::with_capacity(iterations);
    for iteration in 0..warmup + iterations {
        let mut session = network.start();
        let begin = Instant::now();
        session.close_input("seed").unwrap();
        let report = session.run_until_idle(chain + 16);
        assert_eq!(report.steps, chain);
        assert!(!report.budget_exhausted);
        assert!(session.output_closed("tail").unwrap());
        assert_eq!(session.state(), SessionState::Quiescent);
        if iteration >= warmup {
            samples.push(begin.elapsed().as_nanos() as f64);
        }
    }
    let mean = samples.iter().sum::<f64>() / iterations as f64;
    samples.sort_by(|a, b| a.total_cmp(b));
    let p50 = samples[(iterations - 1) / 2];
    let p95 = samples[(((iterations - 1) as f64) * 0.95).round() as usize];
    (compile_ms, mean, p50, p95)
}

fn main() {
    eprintln!(
        "scenario=global_writer_eos_through_finished_chain;          unrelated_writer_open_sinks; persistent_graph=true; new_session_per_sample=true;          startup_outside_timer=true; warmup=16"
    );
    println!("mode,chain_nodes,unrelated_nodes,iterations,compile_ms,mean_ns_per_eos,p50_ns,p95_ns");
    for unrelated in [1, 16, 64, 256] {
        let chain = 1;
        let iterations = 800;
        let (compile_ms, mean, p50, p95) = measure(chain, unrelated, iterations);
        println!(
            "sparse,{chain},{unrelated},{iterations},{compile_ms:.3},{mean:.1},{p50:.0},{p95:.0}"
        );
    }
    for chain in [16, 64, 128] {
        let unrelated = 64;
        let iterations = 120;
        let (compile_ms, mean, p50, p95) = measure(chain, unrelated, iterations);
        println!(
            "chain,{chain},{unrelated},{iterations},{compile_ms:.3},{mean:.1},{p50:.0},{p95:.0}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_eos_benchmark_preserves_exact_step_counts() {
        for (chain, unrelated) in [(1, 4), (8, 8)] {
            let sample = measure(chain, unrelated, 2);
            assert!(sample.1.is_finite());
        }
    }
}
