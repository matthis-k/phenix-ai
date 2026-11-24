#[path = "../../phenix-graphs/benches/support/allocation.rs"]
mod allocation;
#[path = "../benches/support/composition_cases.rs"]
mod cases;
#[path = "../benches/support/planning_policy.rs"]
mod policy;
#[path = "../benches/support/recovery_case.rs"]
mod recovery;

fn main() {
    let filter = std::env::args().nth(1).unwrap_or_default();
    for n in [16, 32, 64] {
        for kind in ["prefix", "preview", "dense", "rebuild", "rebuild_preview"] {
            let name = format!("kernel_{kind}{n}");
            if !name.contains(&filter) {
                continue;
            }
            allocation::measure(&name, || match kind {
                "prefix" => cases::build(n, false, false, policy::configure),
                "preview" => cases::build(n, true, false, policy::configure),
                "dense" => cases::build(n, false, true, policy::configure),
                "rebuild" => cases::rebuild(n, false, policy::configure),
                _ => cases::rebuild(n, true, policy::configure),
            });
        }
    }
    for groups in [1, 4, 8] {
        let name = format!("kernel_recovery_preview{groups}");
        if name.contains(&filter) {
            allocation::measure(&name, || recovery::run(groups, true, policy::configure));
        }
    }
}
