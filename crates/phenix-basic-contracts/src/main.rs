fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!((2..=3).contains(&args.len()), "DESCRIPTOR [GRAPH]");
    std::fs::write(
        &args[1],
        phenix_basic_contracts::definitions::METADATA.bytes(),
    )
    .unwrap();
    if args.len() == 3 {
        std::fs::write(
            &args[2],
            phenix_basic_contracts::definitions::GRAPH_METADATA.bytes(),
        )
        .unwrap();
    }
}
