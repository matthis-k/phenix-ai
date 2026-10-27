use std::path::PathBuf;

fn main() {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    native_abi_consumer::reject_mismatches();
    native_abi_consumer::smoke(&paths).expect("independent compiled-provider smoke");
    println!("Native ABI smoke: both compiled providers, nested data, error, independent generations passed");
}
