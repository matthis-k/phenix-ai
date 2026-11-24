#[path = "../benches/support/allocation.rs"]
mod allocation;
#[path = "../benches/support/composition_cases.rs"]
mod cases;
#[path = "../benches/support/planning_policy.rs"]
mod policy;

fn main() {
    let filter = std::env::args().nth(1).unwrap_or_default();
    for n in [32, 64, 128, 256] {
        for kind in [
            "dense",
            "dense_reverse",
            "splices",
            "hierarchical",
            "defeat",
        ] {
            if kind != "dense" && n > 128 || kind == "defeat" && n > 32 {
                continue;
            }
            let name = format!("q3_{kind}{n}");
            if !name.contains(&filter) {
                continue;
            }
            allocation::measure(&name, || match kind {
                "dense" => cases::dense(n, false, policy::configure),
                "dense_reverse" => cases::dense(n, true, policy::configure),
                "splices" => cases::splices(n, policy::configure),
                "hierarchical" => cases::hierarchical(n, policy::configure),
                _ => cases::defeat(n, policy::configure),
            });
        }
    }
}
