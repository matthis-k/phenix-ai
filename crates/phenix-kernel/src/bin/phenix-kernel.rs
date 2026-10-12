use phenix_contract::{Code, Failure, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared},
    Call, Outcome, Phase, Poll,
};
use std::path::Path;
fn main() {
    let mut phase = Phase::OfflinePreparation;
    if let Err(error) = run(&mut phase) {
        println!(
            "{}",
            serde_json::json!({"format":"phenix.inspect.v2","phase":phase,"last_failure":{"code":error.code,"message":error.message,"phase":phase,"context":null}})
        );
        std::process::exit(1);
    }
}
fn run(phase: &mut Phase) -> Result<(), Failure> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 3 || args.len() > 5 || !["inspect", "invoke"].contains(&args[1].as_str()) {
        return Err(Failure::new(Code::InfrastructureFailure,"usage: phenix-kernel inspect COMPOSITION | invoke COMPOSITION REQUEST.bin [--poll-limit=N]"));
    }
    let composition: Composition = serde_json::from_slice(
        &std::fs::read(&args[2])
            .map_err(|e| Failure::new(Code::InfrastructureFailure, e.to_string()))?,
    )
    .map_err(|e| Failure::new(Code::DescriptorInvalid, e.to_string()))?;
    for artifact in composition.artifacts.iter().chain(&composition.definitions) {
        let path = Path::new(artifact)
            .canonicalize()
            .map_err(|e| Failure::new(Code::ArtifactInvalid, e.to_string()))?;
        if !path.starts_with("/nix/store") {
            return Err(Failure::new(
                Code::ArtifactInvalid,
                "CLI requires trusted immutable Nix store packages",
            ));
        }
    }
    let poll_limit = args
        .get(4)
        .map(|arg| {
            arg.strip_prefix("--poll-limit=")
                .ok_or_else(|| Failure::new(Code::SchemaRejected, "unknown CLI option"))
                .and_then(|limit| {
                    limit
                        .parse::<u64>()
                        .map_err(|_| Failure::new(Code::SchemaRejected, "invalid poll limit"))
                })
        })
        .transpose()?;
    let prepared = Prepared::prepare(composition)?;
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
    let outcome = (|| {
        match root.begin(&Value::decode(&input)?)? {
            Call::Ready(outcome) => Ok(outcome),
            Call::Pending(ticket) => {
                // Explicit limits request cancellation; leases survive caller drop.
                let mut polls = 0;
                loop {
                    if poll_limit.is_some_and(|limit| polls >= limit) {
                        let _ = root.cancel(&ticket);
                        return Err(Failure::new(
                            Code::Cancelled,
                            "explicit CLI poll limit exhausted",
                        ));
                    }
                    polls += 1;
                    match root.poll(&ticket)? {
                        Poll::Ready(result) => {
                            if kernel.inspect()["pending"]["physical_leases"] != 0 {
                                let _ = root.cancel(&ticket);
                            }
                            if kernel.inspect()["pending"]["physical_leases"] == 0 {
                                root.release(&ticket)?;
                            }
                            return result;
                        }
                        Poll::Cancelled { .. } => {
                            return Err(Failure::new(Code::Cancelled, "native call cancelled"))
                        }
                        Poll::Pending | Poll::Draining => {}
                    }
                }
            }
        }
    })();
    drop(root);
    let mut invocation_failure = kernel.inspect()["last_failure"].clone();
    if let Err(failure) = &outcome {
        if invocation_failure.is_null() {
            invocation_failure = serde_json::json!({"code":failure.code,"message":failure.message,"phase":"invocation","context":null});
        }
    }
    let shutdown = kernel.shutdown();
    let mut inspection = kernel.inspect();
    if let Err(failure) = &shutdown {
        inspection["shutdown_failure"] = serde_json::json!(failure);
    }
    inspection["outcome"] = match &outcome {
        Ok(Outcome::Success(v)) => serde_json::json!({"kind":"success","value":v.to_json()}),
        Ok(Outcome::Domain(v)) => serde_json::json!({"kind":"domain_error","value":v.to_json()}),
        Err(failure) if failure.code == Code::Cancelled => {
            serde_json::json!({"kind":"cancelled","failure":invocation_failure})
        }
        Err(_) => serde_json::json!({"kind":"kernel_failure","failure":invocation_failure}),
    };
    println!("{inspection}");
    drop(kernel);
    if outcome.is_err() || shutdown.is_err() {
        std::process::exit(1);
    }
    Ok(())
}
