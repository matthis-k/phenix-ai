//! H6 System-allocator allocation-event accounting for the mixed native agent.
//!
//! Reuses the exact compiled graph from `fbp_agent_mixed_benchmark` but keeps
//! instrumentation out of the timed executable. The single-threaded harness
//! counts successful allocation calls and requested bytes, not live heap.
//!
//! cargo run --locked --release -p phenix-graph --example fbp_agent_allocation_benchmark -- notified 200
use phenix_graph::{
    Value,
    runtime::{NetworkSession, ProcessNotifier, RequestStatus, SessionState},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    ops::{AddAssign, Sub},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};

#[allow(dead_code)]
#[path = "fbp_agent_mixed_benchmark.rs"]
mod workload;

struct CountedSystem;

#[global_allocator]
static ALLOCATOR: CountedSystem = CountedSystem;

static TRACKING: AtomicBool = AtomicBool::new(false);
static ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static REALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static REQUESTED_BYTES: AtomicU64 = AtomicU64::new(0);

fn record_allocation(bytes: usize, realloc: bool) {
    if TRACKING.load(Ordering::Relaxed) {
        if realloc {
            REALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        } else {
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        REQUESTED_BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
    }
}

// SAFETY: every allocation and deallocation delegates to the same System
// allocator with unchanged layouts and pointers. Counters never allocate.
unsafe impl GlobalAlloc for CountedSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarding the caller's allocator contract unchanged.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record_allocation(layout.size(), false);
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarding the caller's allocator contract unchanged.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record_allocation(layout.size(), false);
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: forwarding the caller's pointer, layout and size unchanged.
        let result = unsafe { System.realloc(ptr, layout, new_size) };
        if !result.is_null() {
            record_allocation(new_size, true);
        }
        result
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: matching the original System allocation and its layout.
        unsafe { System.dealloc(ptr, layout) };
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Counts {
    alloc: u64,
    realloc: u64,
    bytes: u64,
}

impl AddAssign for Counts {
    fn add_assign(&mut self, rhs: Self) {
        self.alloc += rhs.alloc;
        self.realloc += rhs.realloc;
        self.bytes += rhs.bytes;
    }
}

impl Sub for Counts {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self {
            alloc: self.alloc - rhs.alloc,
            realloc: self.realloc - rhs.realloc,
            bytes: self.bytes - rhs.bytes,
        }
    }
}

fn snapshot() -> Counts {
    Counts {
        alloc: ALLOC_CALLS.load(Ordering::Relaxed),
        realloc: REALLOC_CALLS.load(Ordering::Relaxed),
        bytes: REQUESTED_BYTES.load(Ordering::Relaxed),
    }
}

/// Brackets one non-concurrent phase. No samples or labels are allocated while
/// instrumentation is enabled; the original timed benchmark is unaffected.
fn measured<T>(f: impl FnOnce() -> T) -> (T, Counts) {
    assert!(!TRACKING.swap(true, Ordering::SeqCst));
    let before = snapshot();
    let value = f();
    TRACKING.store(false, Ordering::SeqCst);
    (value, snapshot() - before)
}

#[derive(Default)]
struct Phases {
    turns: u64,
    admission: Counts,
    first_drain: Counts,
    resume: Counts,
    release: Counts,
}

impl Phases {
    fn push(&mut self, turn: Turn) {
        self.turns += 1;
        self.admission += turn.admission;
        self.first_drain += turn.first_drain;
        self.resume += turn.resume;
        self.release += turn.release;
    }

    fn total(&self) -> Counts {
        let mut result = self.admission;
        result += self.first_drain;
        result += self.resume;
        result += self.release;
        result
    }
}

struct Turn {
    admission: Counts,
    first_drain: Counts,
    resume: Counts,
    release: Counts,
}

/// Deliberately mirrors the timed benchmark's genuine request lifecycle:
/// admission, six initial local-gate steps, explicit notification (if local),
/// eleven total owned steps, settled inspection and release.
fn drive_turn(
    session: &mut NetworkSession,
    notifier: &ProcessNotifier,
    gate: &AtomicBool,
    iteration: usize,
) -> Turn {
    gate.store(false, Ordering::Release);
    let (request, admission) = measured(|| {
        let request = session.begin_request(None).unwrap();
        session
            .inject_request(request, "prompt", Value::of(black_box(iteration as u64)))
            .unwrap();
        session.close_request(request).unwrap();
        request
    });
    let (before, first_drain) = measured(|| session.run_until_idle(64));
    assert!(!before.budget_exhausted);
    let mut steps = before.steps;
    let resume = if iteration % 2 == 0 {
        assert_eq!(before.steps, 6);
        assert_eq!(
            session.request_status(request).unwrap(),
            RequestStatus::Pending
        );
        let (after, counts) = measured(|| {
            gate.store(true, Ordering::Release);
            assert!(notifier.notify());
            session.run_until_idle(64)
        });
        assert!(!after.budget_exhausted);
        steps += after.steps;
        counts
    } else {
        Counts::default()
    };
    assert_eq!(steps, 11);
    let ((), release) = measured(|| {
        assert_eq!(
            session.request_status(request).unwrap(),
            RequestStatus::Settled { captures: 0 }
        );
        session.release_request(request).unwrap();
        assert_eq!(session.state(), SessionState::Quiescent);
    });
    Turn {
        admission,
        first_drain,
        resume,
        release,
    }
}

fn emit(mode: &str, idle: usize, phase: &str, samples: u64, counts: Counts) {
    assert!(samples > 0);
    println!(
        "{mode},{idle},{phase},{samples},{},{},{},{:.3},{:.3},{:.3}",
        counts.alloc,
        counts.realloc,
        counts.bytes,
        counts.alloc as f64 / samples as f64,
        counts.realloc as f64 / samples as f64,
        counts.bytes as f64 / samples as f64
    );
}

fn emit_route(mode: &str, idle: usize, route: &str, phases: &Phases) {
    emit(
        mode,
        idle,
        &format!("{route}_total"),
        phases.turns,
        phases.total(),
    );
    for (name, counts) in [
        ("admission", phases.admission),
        ("first_drain", phases.first_drain),
        ("tool_resume", phases.resume),
        ("release", phases.release),
    ] {
        emit(
            mode,
            idle,
            &format!("{route}_{name}"),
            phases.turns,
            counts,
        );
    }
}

fn run(mode: &str, idle: usize, iterations: usize) {
    let notified = match mode {
        "polled" => false,
        "notified" => true,
        _ => panic!("mode must be polled or notified"),
    };
    let persisted = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(AtomicBool::new(false));
    let ((graph, options), graph_cost) = measured(|| {
        workload::compiled(idle, notified, persisted.clone(), gate.clone())
    });
    let ((mut session, notifier), startup_cost) = measured(|| {
        let session = graph.start_with_options(options);
        let notifier = session.process_notifier("tools.local").unwrap();
        (session, notifier)
    });
    assert_eq!(session.state(), SessionState::Quiescent);
    // Match the original benchmark's warmup without counting its allocations.
    for iteration in 0..12 {
        let turn = drive_turn(&mut session, &notifier, &gate, iteration);
        black_box(turn);
    }
    let mut local = Phases::default();
    let mut remote = Phases::default();
    for iteration in 12..(12 + iterations) {
        let turn = drive_turn(&mut session, &notifier, &gate, iteration);
        if iteration % 2 == 0 {
            local.push(turn);
        } else {
            remote.push(turn);
        }
    }
    assert_eq!(local.turns + remote.turns, iterations as u64);
    assert_eq!(persisted.load(Ordering::Relaxed), 12 + iterations);
    emit(mode, idle, "graph_compile", 1, graph_cost);
    emit(mode, idle, "session_start", 1, startup_cost);
    if local.turns > 0 {
        emit_route(mode, idle, "local", &local);
    }
    if remote.turns > 0 {
        emit_route(mode, idle, "remote", &remote);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "notified".into());
    assert!(mode == "polled" || mode == "notified");
    let iterations: usize = args
        .next()
        .unwrap_or_else(|| "200".into())
        .parse()
        .expect("positive iteration count");
    assert!(iterations > 0);
    eprintln!(
        "allocation_events=System; single_threaded=true; warmup=12; \
         graph=identical_fbp_agent_mixed_benchmark_fixture; \
         requested_bytes_not_live_heap=true; iterations={iterations}"
    );
    println!(
        "mode,idle_nodes,phase,samples,alloc_calls,realloc_calls,requested_bytes,alloc_calls_per_sample,realloc_calls_per_sample,requested_bytes_per_sample"
    );
    for idle in [0, 16, 64, 128] {
        run(&mode, idle, iterations);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_harness_preserves_real_agent_request_lifecycle() {
        run("notified", 4, 4);
        run("polled", 4, 4);
        assert!(!TRACKING.load(Ordering::Relaxed));
    }
}
