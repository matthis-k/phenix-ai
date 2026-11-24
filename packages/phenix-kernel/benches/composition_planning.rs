#[path = "support/composition_cases.rs"]
mod cases;
#[path = "support/planning_policy.rs"]
mod policy;
#[path = "support/recovery_case.rs"]
mod recovery;
use std::time::Instant;

fn main() {
    let filter = std::env::args()
        .skip(1)
        .find(|argument| !argument.starts_with("--"))
        .unwrap_or_default();
    println!(
        "q3_kernel: warmup=2 samples=7; end-to-end setup/prepare/compile/assertions/teardown included; explicit planning policy adapter; no timing allocator"
    );
    for n in [16, 32, 64] {
        for kind in ["prefix", "preview", "dense", "rebuild", "rebuild_preview"] {
            let name = format!("kernel_{kind}{n}");
            if !name.contains(&filter) {
                continue;
            }
            let run = || match kind {
                "prefix" => cases::build(n, false, false, policy::configure),
                "preview" => cases::build(n, true, false, policy::configure),
                "dense" => cases::build(n, false, true, policy::configure),
                "rebuild" => cases::rebuild(n, false, policy::configure),
                _ => cases::rebuild(n, true, policy::configure),
            };
            for _ in 0..2 {
                run();
            }
            let mut samples = Vec::new();
            for _ in 0..7 {
                let start = Instant::now();
                run();
                samples.push(start.elapsed().as_micros());
            }
            println!("{name}: samples_us={samples:?}");
        }
    }
    for groups in [1, 4, 8] {
        let name = format!("kernel_recovery_preview{groups}");
        if !name.contains(&filter) {
            continue;
        }
        let run = || recovery::run(groups, true, policy::configure);
        for _ in 0..2 {
            run();
        }
        let mut samples = Vec::new();
        for _ in 0..7 {
            let start = Instant::now();
            run();
            samples.push(start.elapsed().as_micros());
        }
        println!("{name}: samples_us={samples:?}");
    }
}
