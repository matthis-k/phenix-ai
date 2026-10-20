fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 2, "DESCRIPTOR");
    std::fs::write(
        &args[1],
        phenix_workspace_contracts::definitions::METADATA.bytes(),
    )
    .unwrap();
}
