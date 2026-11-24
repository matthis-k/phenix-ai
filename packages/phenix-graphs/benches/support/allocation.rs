//! Single-threaded, whole-fixture System allocator evidence, not timed code.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering},
};
struct CountedSystem;
#[global_allocator]
static ALLOCATOR: CountedSystem = CountedSystem;
static TRACKING: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicUsize = AtomicUsize::new(0);
static REALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn allocated(bytes: usize, previous: usize, realloc: bool) {
    if TRACKING.load(Ordering::Relaxed) {
        if realloc {
            REALLOCS.fetch_add(1, Ordering::Relaxed);
        } else {
            CALLS.fetch_add(1, Ordering::Relaxed);
        }
        BYTES.fetch_add(bytes, Ordering::Relaxed);
        let live = LIVE.fetch_add(bytes as isize - previous as isize, Ordering::Relaxed)
            + bytes as isize
            - previous as isize;
        PEAK.fetch_max(live.max(0) as usize, Ordering::Relaxed);
    }
}
// SAFETY: forwards all pointers/layouts unchanged to System; counters allocate
// nothing. Requested live bytes exclude allocator metadata/capacity rounding.
unsafe impl GlobalAlloc for CountedSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: original caller contract forwarded unchanged.
        let result = unsafe { System.alloc(layout) };
        if !result.is_null() {
            allocated(layout.size(), 0, false);
        }
        result
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: original caller contract forwarded unchanged.
        let result = unsafe { System.alloc_zeroed(layout) };
        if !result.is_null() {
            allocated(layout.size(), 0, false);
        }
        result
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: original pointer/layout and requested size forwarded unchanged.
        let result = unsafe { System.realloc(ptr, layout, size) };
        if !result.is_null() {
            allocated(size, layout.size(), true);
        }
        result
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if TRACKING.load(Ordering::Relaxed) {
            LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        }
        // SAFETY: same System allocator and original layout.
        unsafe {
            System.dealloc(ptr, layout);
        }
    }
}

pub fn measure(name: &str, run: impl Fn()) {
    for _ in 0..2 {
        run();
    }
    for sample in 0..7 {
        CALLS.store(0, Ordering::Relaxed);
        REALLOCS.store(0, Ordering::Relaxed);
        BYTES.store(0, Ordering::Relaxed);
        LIVE.store(0, Ordering::Relaxed);
        PEAK.store(0, Ordering::Relaxed);
        assert!(!TRACKING.swap(true, Ordering::Relaxed));
        run();
        TRACKING.store(false, Ordering::Relaxed);
        println!(
            "{name}: sample={sample} allocations={} reallocations={} allocated_bytes={} peak_live_requested_bytes={} residual_requested_bytes={}",
            CALLS.load(Ordering::Relaxed),
            REALLOCS.load(Ordering::Relaxed),
            BYTES.load(Ordering::Relaxed),
            PEAK.load(Ordering::Relaxed),
            LIVE.load(Ordering::Relaxed)
        );
    }
}
