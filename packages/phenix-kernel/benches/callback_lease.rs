use phenix_graph::process::LocalProcessDispatcher;
use phenix_kernel::Kernel;
use std::{hint::black_box, sync::Arc, time::Instant};

const ITERATIONS: usize = 100_000;
const SAMPLES: usize = 7;

fn measure(name: &str, mut operation: impl FnMut()) {
    for _ in 0..2 {
        for _ in 0..ITERATIONS {
            operation();
        }
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            operation();
        }
        samples.push(start.elapsed().as_nanos() / ITERATIONS as u128);
    }
    samples.sort_unstable();
    println!("{name}: median_ns_per_call={}", samples[SAMPLES / 2]);
    black_box(samples);
}

fn main() {
    let mut kernel =
        Kernel::build("callback-lease-bench", [], Arc::new(LocalProcessDispatcher)).unwrap();
    kernel.start().unwrap();

    println!("callback_lease: iterations={ITERATIONS} samples={SAMPLES}; setup excluded");
    measure("with_session_noop", || {
        kernel.with_session(|_| ()).unwrap();
        black_box(());
    });
    measure("run_steps_zero", || {
        black_box(kernel.run_steps(0).unwrap());
    });
    measure("drain_zero", || {
        black_box(kernel.drain(0));
    });
}
