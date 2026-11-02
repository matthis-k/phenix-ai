use phenix_dataflow_experiment::*;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
struct Count;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
unsafe impl GlobalAlloc for Count {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(l.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(n as u64, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static ALLOC: Count = Count;
fn report(name: &str, mut samples: Vec<u128>, elapsed: u128, a: u64, b: u64) {
    samples.sort();
    let n = samples.len();
    println!(
        "{}",
        serde_json::json!({"engine":name,"n":n,"elapsed_ns":elapsed,"median_ns":samples[n/2],"p95_ns":samples[n*95/100],"allocations":ALLOCS.load(Ordering::Relaxed)-a,"allocated_bytes":BYTES.load(Ordering::Relaxed)-b,"trace_enabled":true,"profile":"release"})
    );
}
fn main() {
    let n = std::env::var("PHENIX_BENCH_N")
        .ok()
        .map(|s| s.parse::<u64>().unwrap())
        .unwrap_or(2000);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();
    timely_engine::run(|e| {
        let mut samples = Vec::with_capacity(n as usize);
        let a = ALLOCS.load(Ordering::Relaxed);
        let b = BYTES.load(Ordering::Relaxed);
        let all = Instant::now();
        for root in 1..=n {
            let m = message(root, root % 2 == 0, false);
            let now = Instant::now();
            e.submit(&[m], true).unwrap();
            e.step();
            assert_eq!(e.output.borrow().len(), root as usize);
            samples.push(now.elapsed().as_nanos());
        }
        let elapsed = all.elapsed().as_nanos();
        e.seal();
        e.step();
        assert!(e.drained());
        report("timely", samples, elapsed, a, b);
    });
    let mut e = dfir_engine::Engine::new();
    let mut samples = Vec::with_capacity(n as usize);
    let a = ALLOCS.load(Ordering::Relaxed);
    let b = BYTES.load(Ordering::Relaxed);
    let all = Instant::now();
    for root in 1..=n {
        let m = message(root, root % 2 == 0, false);
        let now = Instant::now();
        e.submit(&[m], true).unwrap();
        e.step();
        assert_eq!(e.output.borrow().len(), root as usize);
        samples.push(now.elapsed().as_nanos());
    }
    report("dfir", samples, all.elapsed().as_nanos(), a, b);
}
