//! Encode the plugin's *actual* Patch as verified link-time metadata.
#[path = "src/graph_patch.rs"]
mod graph_patch;
fn main() {
    println!("cargo:rerun-if-changed=src/graph_patch.rs");
    let descriptor = graph_patch::graph_patch()
        .expect("Basic graph contribution is invalid")
        .encode()
        .expect("Basic graph contribution cannot be encoded");
    let path = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"))
        .join("graph_patch.bin");
    std::fs::write(path, descriptor).expect("cannot write Basic graph descriptor");
}
