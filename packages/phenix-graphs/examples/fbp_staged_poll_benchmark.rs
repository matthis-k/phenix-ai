//! H4: completed ordinary staged processes must not tax unrelated persistent turns.
//!
//! cargo run --locked --release -p phenix-graph --example fbp_staged_poll_benchmark -- 3000
use phenix_graph::{
    Failure, Value,
    component::ComponentDef,
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::{Process, ProcessContext, Step},
    runtime::{Readiness, SessionOptions, SessionState},
};
use std::{hint::black_box, num::NonZeroUsize, sync::Arc, time::Instant};

#[derive(Default)]
struct Pair(Option<u64>);

impl Process for Pair {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, Failure> {
        let first = match self.0.take() {
            Some(value) => value,
            None => cx.take("first").expect("first packet").get::<u64>()?,
        };
        if let Some(second) = cx.take("second") {
            assert_eq!(first, second.get::<u64>()?);
            black_box(first);
            Ok(Step::Complete)
        } else {
            self.0 = Some(first);
            Ok(Step::WaitInput("second".into()))
        }
    }
}

#[derive(Clone, Copy)]
enum Driver {
    Jobs,
    Report,
}

impl Driver {
    fn name(self) -> &'static str {
        match self {
            Self::Jobs => "next_job_complete",
            Self::Report => "run_until_idle",
        }
    }
}

struct Measurement {
    setup_ms: f64,
    mean: f64,
    p50: f64,
    p95: f64,
    turns_per_second: f64,
}

fn measure(processes: usize, iterations: usize, staged: bool, driver: Driver) -> Measurement {
    assert!(processes > 0 && iterations > 0);
    let setup = Instant::now();
    let mut builder = NetworkBuilder::new();
    let active = builder
        .add(
            "active",
            Arc::new(ComponentDef::sink(|value: &u64| {
                black_box(*value);
            })),
        )
        .unwrap();
    builder
        .expose_input(
            "active",
            active.input::<u64>("input").unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    let interface = Arc::new(
        Interface::new(
            [
                PortDef::wire::<u64>("first").unwrap(),
                PortDef::wire::<u64>("second").unwrap(),
            ],
            [],
        )
        .unwrap(),
    );
    let mut options = SessionOptions::default();
    let mut inputs = Vec::with_capacity(processes);
    for index in 0..processes {
        let name = format!("pair-{index:03}");
        let node = builder
            .node(name.clone(), interface.clone(), Arc::new(Pair::default))
            .unwrap();
        let first = format!("first-{index}");
        let second = format!("second-{index}");
        for (external, port) in [(&first, "first"), (&second, "second")] {
            builder
                .expose_input(
                    external.clone(),
                    node.wire_input::<u64>(port).unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
        }
        // Ordinary packet readiness, not custom or optional polling.
        options.readiness.insert(name, Readiness::ANY);
        inputs.push((first, second));
    }
    let network = builder.build().unwrap().compile().unwrap();
    let mut session = network.start_with_options(options);
    assert!(session.next_job().is_none());
    if staged {
        // All processes own a WaitInput continuation simultaneously, then all
        // resume and complete. Ingress writers stay open throughout measurement.
        for (index, (first, _)) in inputs.iter().enumerate() {
            session.inject(first, Value::of(index as u64)).unwrap();
            let (actual, job) = session.next_job().expect("initial staged job");
            assert_eq!(actual, index + 1);
            session.complete(actual, job.run()).unwrap();
        }
        assert!(session.next_job().is_none());
        for (index, (_, second)) in inputs.iter().enumerate() {
            session.inject(second, Value::of(index as u64)).unwrap();
            let (actual, job) = session.next_job().expect("resumed staged job");
            assert_eq!(actual, index + 1);
            session.complete(actual, job.run()).unwrap();
        }
        assert!(session.next_job().is_none());
    }
    assert_eq!(session.state(), SessionState::Quiescent);
    let setup_ms = setup.elapsed().as_secs_f64() * 1000.0;
    let warmup = 64;
    let mut samples = Vec::with_capacity(iterations);
    for turn in 0..warmup + iterations {
        let begin = Instant::now();
        let value = black_box(turn as u64);
        session.inject_local("active", value).unwrap();
        match driver {
            Driver::Jobs => {
                let (index, job) = session.next_job().expect("one addressed sink");
                assert_eq!(index, 0);
                session.complete(index, job.run()).unwrap();
                assert!(session.next_job().is_none());
                // Do not call state() here: it scans the network separately
                // from refresh polling. The report driver includes that cost.
            }
            Driver::Report => {
                let report = session.run_until_idle(2);
                assert_eq!(report.steps, 1);
                assert_eq!(report.publications, 0);
                assert!(!report.budget_exhausted);
                assert_eq!(report.state, SessionState::Quiescent);
            }
        }
        let elapsed = begin.elapsed().as_nanos() as f64;
        if turn >= warmup {
            samples.push(elapsed);
        }
    }
    assert_eq!(session.state(), SessionState::Quiescent);
    assert!(session.next_job().is_none());
    let mean = samples.iter().sum::<f64>() / iterations as f64;
    samples.sort_by(|a, b| a.total_cmp(b));
    Measurement {
        setup_ms,
        mean,
        p50: samples[(iterations - 1) / 2],
        p95: samples[((iterations - 1) as f64 * 0.95).round() as usize],
        turns_per_second: 1e9 / mean,
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let iterations = args
        .next()
        .unwrap_or_else(|| "3000".into())
        .parse::<usize>()
        .expect("positive iterations");
    assert!(iterations > 0);
    // Reverse within-binary order on alternating paired CI repetitions too.
    let histories = match args.next().as_deref() {
        None | Some("staged-first") => [true, false],
        Some("fresh-first") => [false, true],
        Some(_) => panic!("expected staged-first or fresh-first"),
    };
    assert!(args.next().is_none(), "unexpected argument");
    eprintln!(
        "scenario=ordinary_two_input_pairs; staged=all_wait_once_then_all_complete; \
         control=fresh_never_staged_same_network; active=separate_sink; \
         writers=open; capacity=1; warmup=64; setup_excluded=true; \
         jobs_state_scan=false; report_state_scan=true; throughput=reciprocal_mean_turn_latency"
    );
    println!(
        "driver,history,idle_pairs,total_processes,turns,setup_ms,mean_ns_per_turn,p50_ns,p95_ns,turns_per_second"
    );
    for driver in [Driver::Jobs, Driver::Report] {
        for processes in [16, 64, 256] {
            for staged in histories {
                let m = measure(processes, iterations, staged, driver);
                let history = if staged { "staged_completed" } else { "fresh" };
                println!(
                    "{},{history},{processes},{},{iterations},{:.3},{:.1},{:.0},{:.0},{:.1}",
                    driver.name(),
                    processes + 1,
                    m.setup_ms,
                    m.mean,
                    m.p50,
                    m.p95,
                    m.turns_per_second
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_and_fresh_pairs_complete_persistent_turns_with_both_drivers() {
        for driver in [Driver::Jobs, Driver::Report] {
            for staged in [true, false] {
                for processes in [1, 16, 64, 256] {
                    let m = measure(processes, 4, staged, driver);
                    assert!(m.mean.is_finite() && m.turns_per_second > 0.0);
                }
            }
        }
    }
}
