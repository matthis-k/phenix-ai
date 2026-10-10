use phenix_artifact::{digest, extract};
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: phenix-describe ARTIFACT DESCRIPTOR-OUTPUT".into());
    }
    let artifact = std::fs::read(&args[1])?;
    let descriptor = extract(&artifact)?;
    std::fs::write(&args[2], &descriptor)?;
    println!(
        "{}",
        serde_json::json!({"artifact_sha256":digest(&artifact),"descriptor_sha256":digest(&descriptor)})
    );
    Ok(())
}
