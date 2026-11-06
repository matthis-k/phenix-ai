// Definition-only packages emit one descriptor. Graph topology is contributed
// exclusively by executable plugins through the canonical Patch metadata.
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    assert_eq!(args.len(), 2, "DESCRIPTOR");
    std::fs::write(
        &args[1],
        phenix_advanced_contracts::definitions::METADATA.bytes(),
    )
    .expect("failed to write definition descriptor");
}
