include!("../../../../examples/provider/src/lib.rs");

#[used]
#[no_mangle]
#[link_section = ".phenix.graph"]
pub static PHENIX_GRAPH_V2: [u8; phenix_sdk::NATIVE_SECTION_BYTES] =
    *include_bytes!(concat!(env!("OUT_DIR"), "/graph.bin"));
