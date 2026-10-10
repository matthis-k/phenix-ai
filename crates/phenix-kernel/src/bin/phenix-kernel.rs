use phenix_contract::{Code, Failure, Value};
use phenix_kernel::{
    resolve::{Deployment, Prepared},
    Outcome,
};
use std::path::Path;
#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    OfflinePreparation,
    ExecutableAbi,
    Admission,
    Invocation,
}
fn main() {
    let mut phase = Phase::OfflinePreparation;
    if let Err(error) = run(&mut phase) {
        println!(
            "{}",
            serde_json::json!({"format":"phenix.inspect.v1","phase":phase,"last_failure":{"code":error.code,"message":error.message,"phase":phase,"context":null}})
        );
        std::process::exit(1);
    }
}
fn run(phase: &mut Phase) -> Result<(), Failure> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 3 || args.len() > 4 || !["inspect", "invoke"].contains(&args[1].as_str()) {
        return Err(Failure::new(
            Code::InfrastructureFailure,
            "usage: phenix-kernel inspect DEPLOYMENT | invoke DEPLOYMENT REQUEST.bin",
        ));
    }
    let deployment: Deployment = serde_json::from_slice(
        &std::fs::read(&args[2])
            .map_err(|e| Failure::new(Code::InfrastructureFailure, e.to_string()))?,
    )
    .map_err(|e| Failure::new(Code::DescriptorInvalid, e.to_string()))?;
    for artifact in &deployment.artifacts {
        let p = Path::new(artifact)
            .canonicalize()
            .map_err(|e| Failure::new(Code::ArtifactInvalid, e.to_string()))?;
        if !p.starts_with("/nix/store") {
            return Err(Failure::new(
                Code::ArtifactInvalid,
                "CLI requires trusted immutable Nix store packages",
            ));
        }
    }
    let prepared = Prepared::prepare(deployment)?;
    if args[1] == "inspect" {
        println!("{}", prepared.inspect());
        return Ok(());
    }
    let path = args
        .get(3)
        .ok_or_else(|| Failure::new(Code::InfrastructureFailure, "request path required"))?;
    let input = std::fs::read(path)
        .map_err(|e| Failure::new(Code::InfrastructureFailure, e.to_string()))?;
    // Reject malformed portable input before loading any trusted constructors.
    Value::decode(&input)?;
    *phase = Phase::ExecutableAbi;
    let kernel = prepared.load()?;
    *phase = Phase::Admission;
    let root = kernel.admit_default_root()?;
    *phase = Phase::Invocation;
    let outcome = root.invoke_bytes(&input);
    drop(root);
    let mut inspection = kernel.inspect();
    inspection["outcome"] = match &outcome {
        Ok(Outcome::Success(v)) => serde_json::json!({"kind":"success","value":v.to_json()}),
        Ok(Outcome::Domain(v)) => serde_json::json!({"kind":"domain_error","value":v.to_json()}),
        Err(_) => serde_json::json!({"kind":"kernel_failure","failure":inspection["last_failure"]}),
    };
    println!("{inspection}");
    drop(kernel);
    if outcome.is_err() {
        std::process::exit(1);
    }
    Ok(())
}
