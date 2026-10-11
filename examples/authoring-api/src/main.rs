fn main() {
    std::fs::write(
        std::env::args().nth(1).expect("metadata output path"),
        example_authoring_api::definitions::METADATA.bytes(),
    )
    .unwrap();
}
