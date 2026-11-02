include!("../native_graph.rs");
fn main() {
    println!("cargo:rerun-if-changed=../native_graph.rs");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(out.join("graph.bin"), graph().native_section().unwrap()).unwrap();
}
