use phenix_artifact::{digest, extract};
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--metadata" {
        let package = phenix_artifact::VerifiedContractPackage::verify_trusted_read_only(
            std::path::Path::new(&args[2]),
        )?;
        let metadata = package.metadata();
        println!(
            "{}",
            serde_json::json!({"format":"phenix.contract-package.v2", "plugin":metadata.plugin, "definitions":metadata.definitions.iter().map(|d| &d.contract).collect::<Vec<_>>(), "descriptor_sha256":package.descriptor_sha256(), "package_sha256":package.package_sha256()})
        );
        return Ok(());
    }
    if args.len() != 3 {
        return Err(
            "usage: phenix-describe ARTIFACT DESCRIPTOR-OUTPUT | --metadata PACKAGE".into(),
        );
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
