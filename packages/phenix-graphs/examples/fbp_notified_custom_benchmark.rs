//! H4: compare default-polled versus explicitly notified Custom readiness
//! under identical process topology and one active unrelated packet stream.
//!
//! Candidate-only modes use the same executable, CPU and compiled graph
//! topology. The older parent has no notification_driven() API.
use phenix_graph::{
    component::ComponentDef,
    network::NetworkBuilder,
    runtime::{Readiness, ReadinessPolicy, ReadinessSnapshot, SessionOptions},
};
use std::{
    hint::black_box,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Instant,
};

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

struct Gate {
    notified: bool,
    allowed: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}
impl ReadinessPolicy for Gate {
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if !self.allowed.load(Ordering::Acquire) {
            return vec![];
        }
        snapshot
            .heads
            .iter()
            .filter(|h| h.available)
            .map(|h| h.port.clone())
            .collect()
    }
    fn notification_driven(&self) -> bool {
        self.notified
    }
}

fn measure(notified: bool, idle: usize, iterations: usize) -> (f64, f64, f64, usize) {
    assert!(iterations > 0);
    let mut builder = NetworkBuilder::new();
    let sink = Arc::new(ComponentDef::sink(|value: &u64| {
        black_box(*value);
    }));
    let active = builder.add("active", sink.clone()).unwrap();
    builder
        .expose_input("active_in", active.input::<u64>("input").unwrap(), one())
        .unwrap();

    let calls = Arc::new(AtomicUsize::new(0));
    let mut gates = Vec::with_capacity(idle);
    let mut options = SessionOptions::default();
    for index in 0..idle {
        let name = format!("custom-{index}");
        let node = builder.add(name.clone(), sink.clone()).unwrap();
        builder
            .expose_input(
                format!("custom-in-{index}"),
                node.input::<u64>("input").unwrap(),
                one(),
            )
            .unwrap();
        let allowed = Arc::new(AtomicBool::new(false));
        options.readiness.insert(
            name,
            Readiness::Custom(Arc::new(Gate {
                notified,
                allowed: allowed.clone(),
                calls: calls.clone(),
            })),
        );
        gates.push(allowed);
    }

    let graph = builder.build().unwrap().compile().unwrap();
    let mut session = graph.start_with_options(options);
    let mut first_notifier = None;
    for index in 0..idle {
        let name = format!("custom-{index}");
        if index == 0 {
            first_notifier = session.process_notifier(&name);
        }
        session
            .inject_local(&format!("custom-in-{index}"), index as u64)
            .unwrap();
    }
    assert!(session.next_job().is_none());
    let seeded_calls = calls.load(Ordering::Relaxed);

    let warmup = 32;
    let mut samples = Vec::with_capacity(iterations);
    let begin = Instant::now();
    for iteration in 0..warmup + iterations {
        let started = Instant::now();
        session
            .inject_local("active_in", black_box(iteration as u64))
            .unwrap();
        let (index, job) = session.next_job().expect("active packet");
        assert_eq!(session.process_path(index), Some("active"));
        session.complete(index, job.run()).unwrap();
        assert!(session.next_job().is_none());
        if iteration >= warmup {
            samples.push(started.elapsed().as_nanos() as f64);
        }
    }
    let elapsed = begin.elapsed().as_secs_f64();
    let new_calls = calls.load(Ordering::Relaxed) - seeded_calls;
    if idle > 0 && !notified {
        assert!(new_calls > iterations);
    } else {
        assert_eq!(new_calls, 0);
    }

    if let Some(notifier) = first_notifier {
        gates[0].store(true, Ordering::Release);
        assert!(notifier.notify());
        let (index, job) = session.next_job().expect("notified selected job");
        assert_eq!(session.process_path(index), Some("custom-0"));
        session.complete(index, job.run()).unwrap();
    }
    samples.sort_by(|a, b| a.total_cmp(b));
    let p50 = samples[(iterations - 1) / 2];
    let p95 = samples[(((iterations - 1) as f64) * 0.95).round() as usize];
    (elapsed * 1e9 / (iterations + warmup) as f64, p50, p95, new_calls)
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let notified = match args.first().map(String::as_str) {
        Some("notified") => true,
        Some("polled") => false,
        _ => panic!("usage: fbp_notified_custom_benchmark <notified|polled> <iterations>"),
    };
    let iterations = args.get(1).map(|s| s.parse().unwrap()).unwrap_or(1500);
    let mode = if notified { "notified" } else { "polled" };
    println!("mode,custom_nodes,iterations,mean_ns_per_active,p50_ns,p95_ns,custom_select_calls");
    for idle in [0, 16, 64, 256] {
        let (mean, p50, p95, calls) = measure(notified, idle, iterations);
        println!("{mode},{idle},{iterations},{mean:.1},{p50:.0},{p95:.0},{calls}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notified_custom_benchmark_preserves_targeted_admission() {
        for notified in [false, true] {
            for idle in [0, 4] {
                let (mean, _, _, _) = measure(notified, idle, 4);
                assert!(mean.is_finite());
            }
        }
    }
}
