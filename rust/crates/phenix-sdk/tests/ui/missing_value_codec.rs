use phenix_sdk as phenix;

struct MissingCodec;

#[derive(phenix::PhenixValue)]
struct Invalid {
    value: MissingCodec,
}

fn main() {}
