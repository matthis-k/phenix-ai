use phenix_artifact::{digest, extract, PackageRecord, VerifiedPackage};
// Executes public deployment APIs against real, separately compiled artifacts.
use phenix_contract::{Code, Value};
use phenix_kernel::{
    resolve::{Deployment, Grant, Prepared, Selection},
    Outcome,
};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn selection(interface: &str, operation: &str, provider: &str) -> Selection {
    Selection {
        interface: interface.into(),
        operation: operation.into(),
        provider: provider.into(),
    }
}
fn deployment(provider: &str, consumer: &str) -> Deployment {
    Deployment {
        format: "phenix.deployment.v1".into(),
        artifacts: vec![provider.into(), consumer.into()],
        entry: selection("example.consumer@1", "run", "example.consumer"),
        bindings: vec![selection(
            "example.transform@1",
            "apply",
            "example.provider",
        )],
        grants: vec![
            Grant {
                caller: "root".into(),
                interface: "example.consumer@1".into(),
                operation: "run".into(),
            },
            Grant {
                caller: "example.consumer".into(),
                interface: "example.transform@1".into(),
                operation: "apply".into(),
            },
            Grant {
                caller: "root".into(),
                interface: "example.transform@1".into(),
                operation: "count".into(),
            },
        ],
    }
}
fn request(text: &str) -> Value {
    Value::map([("text", Value::String(text.into()))])
}
fn expect_prepare_error(d: Deployment, code: Code) {
    match Prepared::prepare(d) {
        Err(e) => assert_eq!(e.code, code, "{e}"),
        Ok(_) => panic!("preparation unexpectedly passed"),
    }
}
fn mutate_package(
    source: &Path,
    dest: &Path,
    mutate: impl FnOnce(&mut Value),
    embed: bool,
) -> PathBuf {
    fs::create_dir_all(dest.join("lib")).unwrap();
    let mut record: PackageRecord =
        serde_json::from_slice(&fs::read(source.join("package.json")).unwrap()).unwrap();
    let mut artifact = fs::read(source.join(&record.artifact)).unwrap();
    let old = extract(&artifact).unwrap();
    let mut value = Value::decode(&old).unwrap();
    mutate(&mut value);
    let new = value.encode().unwrap();
    if embed {
        let offset = artifact.windows(old.len()).position(|w| w == old).unwrap();
        let section_start = offset - 4;
        let range = section_start..section_start + phenix_contract::SECTION_BYTES;
        artifact[range.clone()].fill(0);
        artifact[section_start..offset].copy_from_slice(&(new.len() as u32).to_be_bytes());
        artifact[offset..offset + new.len()].copy_from_slice(&new);
    }
    record.artifact_sha256 = digest(&artifact);
    record.descriptor_sha256 = digest(&new);
    fs::write(dest.join(&record.artifact), artifact).unwrap();
    fs::write(dest.join(&record.descriptor), new).unwrap();
    fs::write(
        dest.join("package.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    dest.to_owned()
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 9 {
        eprintln!("usage: phenix-conformance PROVIDER CONSUMER ALTERNATE CYCLE BAD-MAJOR BAD-SHORT BAD-NULL OUTPUT");
        std::process::exit(1);
    }
    let (provider, consumer, alternate) = (&args[1], &args[2], &args[3]);
    let output = Path::new(&args[8]);
    fs::create_dir_all(output).unwrap();
    let work = std::env::temp_dir().join(format!("phenix-conformance-{}", std::process::id()));
    fs::create_dir_all(&work).unwrap();
    let witness = work.join("constructor");
    std::env::set_var("PHENIX_CONSTRUCTOR_WITNESS", &witness);
    let d = deployment(provider, consumer);
    let prepared = Prepared::prepare(d.clone()).unwrap();
    assert!(
        !witness.exists(),
        "offline preparation executed plugin constructor"
    );
    let offline = prepared.inspect();
    assert!(offline["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["abi_negotiated"] == false));
    let mut missing = d.clone();
    missing.artifacts.retain(|p| p != provider);
    expect_prepare_error(missing, Code::MissingBinding);
    let mut unselected = d.clone();
    unselected.bindings.clear();
    expect_prepare_error(unselected.clone(), Code::MissingBinding);
    unselected.artifacts.push(alternate.into());
    expect_prepare_error(unselected, Code::AmbiguousBinding);
    let mut absent = d.clone();
    absent.bindings[0].provider = "unknown".into();
    expect_prepare_error(absent, Code::MissingBinding);
    let mut reordered = d.clone();
    reordered.artifacts.reverse();
    assert_eq!(Prepared::prepare(reordered).unwrap().inspect(), offline);
    let mismatch = mutate_package(
        Path::new(provider),
        &work.join("schema"),
        |v| {
            let Value::Map(m) = v else { panic!() };
            let Value::List(contracts) = m.get_mut("contracts").unwrap() else {
                panic!()
            };
            let Value::Map(contract) = &mut contracts[0] else {
                panic!()
            };
            contract.insert(
                "request".into(),
                Value::map([("kind", Value::String("Bool".into()))]),
            );
        },
        true,
    );
    let mut incompatible = d.clone();
    incompatible.artifacts[0] = mismatch.to_string_lossy().into();
    expect_prepare_error(incompatible, Code::SchemaRejected);
    let unsupported = mutate_package(
        Path::new(provider),
        &work.join("abi"),
        |v| {
            let Value::Map(m) = v else { panic!() };
            m.insert("abi_major".into(), Value::U64(99));
        },
        true,
    );
    assert_eq!(
        VerifiedPackage::verify_trusted_read_only(&unsupported)
            .unwrap_err()
            .code,
        Code::AbiIncompatible
    );
    let metadata = mutate_package(
        Path::new(provider),
        &work.join("metadata"),
        |v| {
            let Value::Map(m) = v else { panic!() };
            m.insert("plugin_version".into(), Value::String("2.0.0".into()));
        },
        false,
    );
    assert_eq!(
        VerifiedPackage::verify_trusted_read_only(&metadata)
            .unwrap_err()
            .code,
        Code::DescriptorInvalid
    );
    let owner = mutate_package(
        Path::new(provider),
        &work.join("owner"),
        |v| {
            let Value::Map(m) = v else { panic!() };
            let Value::List(exports) = m.get_mut("exports").unwrap() else {
                panic!()
            };
            let Value::Map(export) = &mut exports[0] else {
                panic!()
            };
            export.insert("owner".into(), Value::String("other".into()));
        },
        true,
    );
    assert_eq!(
        VerifiedPackage::verify_trusted_read_only(&owner)
            .unwrap_err()
            .code,
        Code::DescriptorInvalid
    );
    let tampered = mutate_package(Path::new(provider), &work.join("tampered"), |_| {}, true);
    let artifact = tampered.join("lib/plugin.so");
    let mut bytes = fs::read(&artifact).unwrap();
    bytes.push(0);
    fs::write(artifact, bytes).unwrap();
    assert_eq!(
        VerifiedPackage::verify_trusted_read_only(&tampered)
            .unwrap_err()
            .code,
        Code::ArtifactInvalid
    );
    let mut unknown = serde_json::to_value(&d).unwrap();
    unknown["implicit_grant"] = true.into();
    assert!(serde_json::from_value::<Deployment>(unknown).is_err());
    assert!(
        !witness.exists(),
        "negative preparation executed plugin code"
    );
    let kernel = prepared.load().unwrap();
    assert!(
        witness.exists(),
        "native loading did not execute constructor witness"
    );
    let root = kernel.admit_default_root().unwrap();
    assert_eq!(
        root.invoke(&request("Phenix ✓")).unwrap(),
        Outcome::Success(request("PHENIX ✓"))
    );
    assert_eq!(
        root.invoke(&request("")).unwrap(),
        Outcome::Domain(Value::String("Empty".into()))
    );
    assert_eq!(
        root.invoke(&request("panic")).unwrap_err().code,
        Code::PluginFailed
    );
    assert_eq!(
        root.invoke(&Value::Bool(true)).unwrap_err().code,
        Code::SchemaRejected
    );
    assert_eq!(
        root.invoke_bytes(&[0, 0]).unwrap_err().code,
        Code::SchemaRejected
    );
    assert_eq!(
        root.invoke(&request("again")).unwrap_err().code,
        Code::PluginFailed
    );
    drop(root);
    let successful = kernel.inspect();
    assert_eq!(successful["calls"], 8);
    drop(kernel);
    let mut denied = d.clone();
    denied.grants.retain(|g| g.caller != "example.consumer");
    let kernel = Prepared::prepare(denied).unwrap().load().unwrap();
    let root = kernel.admit_default_root().unwrap();
    assert_eq!(
        root.invoke(&request("denied")).unwrap_err().code,
        Code::Denied
    );
    drop(root);
    let root = kernel
        .admit_root(selection(
            "example.transform@1",
            "count",
            "example.provider",
        ))
        .unwrap();
    assert_eq!(
        root.invoke(&Value::map([("count", Value::U64(0))]))
            .unwrap(),
        Outcome::Success(Value::map([("count", Value::U64(0))]))
    );
    drop(root);
    let denied = kernel.inspect();
    assert_eq!(denied["denied_operations"], 1);
    assert_eq!(denied["last_failure"]["code"], "Denied");
    drop(kernel);
    let cycle_key = selection("example.cycle@1", "run", "example.cycle");
    let cycle_deployment = Deployment {
        format: "phenix.deployment.v1".into(),
        artifacts: vec![args[4].clone()],
        entry: cycle_key.clone(),
        bindings: vec![cycle_key],
        grants: vec![
            Grant {
                caller: "root".into(),
                interface: "example.cycle@1".into(),
                operation: "run".into(),
            },
            Grant {
                caller: "example.cycle".into(),
                interface: "example.cycle@1".into(),
                operation: "run".into(),
            },
        ],
    };
    let cycle_kernel = Prepared::prepare(cycle_deployment).unwrap().load().unwrap();
    let cycle_root = cycle_kernel.admit_default_root().unwrap();
    assert_eq!(
        cycle_root.invoke(&request("cycle")).unwrap_err().code,
        Code::ReentrancyRejected
    );
    drop(cycle_root);
    assert_eq!(cycle_kernel.inspect()["calls"], 1);
    drop(cycle_kernel);
    for bad in &args[5..8] {
        let deployment = Deployment {
            format: "phenix.deployment.v1".into(),
            artifacts: vec![bad.clone()],
            entry: selection("example.transform@1", "apply", "example.provider"),
            bindings: vec![],
            grants: vec![Grant {
                caller: "root".into(),
                interface: "example.transform@1".into(),
                operation: "apply".into(),
            }],
        };
        let prepared = Prepared::prepare(deployment).unwrap();
        match prepared.load() {
            Err(e) => assert_eq!(e.code, Code::AbiIncompatible),
            Ok(_) => panic!("invalid loaded ABI accepted"),
        }
    }
    fs::write(output.join("native.json"),serde_json::to_vec_pretty(&serde_json::json!({"format":"phenix.conformance.v1","sdk_version":"0.1.0","contract_version":"0.1.0","successful":successful,"denied":denied,"cases":["offline-no-execution","real-nested-call","domain-error","panic-translation","poisoned-provider","missing-binding","single-provider-needs-selection","ambiguous-binding","absent-selection","schema-mismatch","bad-abi","metadata-mismatch","wrong-owner","artifact-tamper","unknown-policy","binding-order","schema-rejection","denied-counter-unchanged","buffer-roundtrip","native-cycle","loaded-abi-major","loaded-short-table","loaded-null-entries"]})).unwrap()).unwrap();
    fs::remove_dir_all(work).unwrap();
    println!("M0.0 native conformance passed");
}
