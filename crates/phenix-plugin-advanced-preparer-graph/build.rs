//! Encode the same Advanced patch that the plugin exposes to host tests.
#[path = "src/graph_patch.rs"]
mod graph_patch;
fn main() {
    println!("cargo:rerun-if-changed=src/graph_patch.rs");
    let descriptor = graph_patch::graph_patch()
        .expect("Advanced graph contribution is invalid")
        .encode()
        .expect("Advanced graph contribution cannot be encoded");
    let path = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"))
        .join("graph_patch.bin");
    std::fs::write(path, descriptor).expect("cannot write Advanced graph descriptor");
}
