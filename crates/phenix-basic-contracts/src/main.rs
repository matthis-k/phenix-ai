fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3, "DESCRIPTOR GRAPH");
    std::fs::write(
        &args[1],
        phenix_basic_contracts::definitions::METADATA.bytes(),
    )
    .unwrap();
    std::fs::write(
        &args[2],
        phenix_basic_contracts::definitions::GRAPH_METADATA.bytes(),
    )
    .unwrap();
}
