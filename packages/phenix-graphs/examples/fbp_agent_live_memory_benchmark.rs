//! Whole-fixture requested live memory, using the exact timed agent workload.
//! Includes graph construction, session startup, warmup, request driving,
//! sample storage, and teardown. This is neither allocator RSS nor latency.
#[path = "../benches/support/allocation.rs"]
mod allocation;
#[allow(dead_code)]
#[path = "fbp_agent_mixed_benchmark.rs"]
mod workload;

fn main() {
    for idle in [0, 16, 64, 128] {
        for (mode, notified) in [("polled", false), ("notified", true)] {
            allocation::measure(&format!("agent_{mode}_single_{idle}"), || {
                std::hint::black_box(workload::measure(notified, idle, 8));
            });
            allocation::measure(&format!("agent_{mode}_overlap4_{idle}"), || {
                std::hint::black_box(workload::measure_overlapping(notified, idle, 8));
            });
        }
    }
}
