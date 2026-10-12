use phenix_artifact::{digest, extract, PackageRecord, VerifiedPackage};
// Executes public deployment APIs against real, separately compiled artifacts.
use phenix_contract::{Authority,Code,Value,ValueSchema};
use std::collections::BTreeSet;
use phenix_kernel::{
    resolve::{Composition, ScopedGrant, Prepared, Selection},
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
fn deployment(provider:&str,consumer:&str)->Composition {
    let entry=selection("example.consumer@1","run","example.consumer");
    let grants=vec![
        ScopedGrant {caller:"root".into(),interface:entry.interface.clone(),operation:entry.operation.clone(),resources:BTreeSet::new()},
        ScopedGrant {caller:"example.consumer".into(),interface:"example.transform.apply@1".into(),operation:"apply".into(),resources:BTreeSet::new()},
        ScopedGrant {caller:"root".into(),interface:"example.transform.count@1".into(),operation:"count".into(),resources:BTreeSet::new()},
    ];
    Composition {format:"phenix.composition.v2".into(),artifacts:vec![provider.into(),consumer.into()],definitions:vec![],entry,
        slots:vec![phenix_conformance::binding("example.consumer","example.transform.apply@1/apply","example.provider")],
        authority:Authority {operations:grants.iter().map(ScopedGrant::key).collect(),resources:BTreeSet::new()},grants,excluded:BTreeSet::new(),substitutions:vec![],resource:None}
}
fn request(text: &str) -> Value {
    Value::map([("text", Value::String(text.into()))])
}
fn expect_prepare_error(d: Composition, code: Code) {
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
        let range = section_start..section_start + phenix_contract::NATIVE_SECTION_BYTES;
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
    if args.len() != 11 {
        eprintln!("usage: phenix-conformance PROVIDER CONSUMER ALTERNATE CYCLE BAD-MAJOR BAD-SHORT BAD-NULL BAD-RELEASE BAD-CREATE OUTPUT");
        std::process::exit(1);
    }
    let (provider, consumer, alternate) = (&args[1], &args[2], &args[3]);
    let output = Path::new(&args[10]);
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
    let signed = mutate_package(
        Path::new(provider),
        &work.join("signed-version"),
        |v| {
            let Value::Map(fields) = v else { panic!() };
            let Value::List(definitions)=fields.get_mut("definitions").unwrap() else {panic!()};
            let Value::Map(definition)=&mut definitions[0] else {panic!()};
            definition.insert("revision".into(),Value::I64(1));
        },
        true,
    );
    assert_eq!(
        VerifiedPackage::verify_trusted_read_only(&signed)
            .unwrap_err()
            .code,
        Code::DescriptorInvalid
    );
    assert!(offline["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["abi_negotiated"] == false));
    let mut missing = d.clone();
    missing.artifacts.retain(|p| p != provider);
    expect_prepare_error(missing, Code::MissingBinding);
    let mut unselected = d.clone();
    unselected.slots.clear();
    expect_prepare_error(unselected.clone(), Code::MissingBinding);
    unselected.artifacts.push(alternate.into());
    expect_prepare_error(unselected, Code::MissingBinding);
    let mut absent = d.clone();
    absent.slots[0].provider = "unknown".into();
    expect_prepare_error(absent, Code::MissingBinding);
    let mut reordered = d.clone();
    reordered.artifacts.reverse();
    assert_eq!(Prepared::prepare(reordered).unwrap().inspect(), offline);
    let mismatch = mutate_package(
        Path::new(provider),
        &work.join("schema"),
        |v| {
            let Value::Map(m) = v else { panic!() };
            let Value::List(definitions)=m.get_mut("definitions").unwrap() else {panic!()};
            let Value::Map(definition)=&mut definitions[0] else {panic!()};
            let Value::List(operations)=definition.get_mut("operations").unwrap() else {panic!()};
            let Value::Map(operation)=&mut operations[0] else {panic!()};
            operation.insert("request".into(),Value::Bytes(ValueSchema::Bool.encode().unwrap()));
        },
        true,
    );
    let mut incompatible = d.clone();
    incompatible.artifacts[0] = mismatch.to_string_lossy().into();
    expect_prepare_error(incompatible, Code::DescriptorInvalid);
    let unsupported = mutate_package(
        Path::new(provider),
        &work.join("abi"),
        |v| {
            let Value::Map(m) = v else { panic!() };
            m.insert("execution".into(), Value::String("legacy-native".into()));
        },
        true,
    );
    assert_eq!(
        VerifiedPackage::verify_trusted_read_only(&unsupported)
            .unwrap_err()
            .code,
        Code::DescriptorInvalid
    );
    let metadata = mutate_package(
        Path::new(provider),
        &work.join("metadata"),
        |v| {
            let Value::Map(m) = v else { panic!() };
            m.insert("version".into(), Value::String("2.0.0".into()));
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
            let Value::List(exports) = m.get_mut("provides").unwrap() else {
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
    assert!(serde_json::from_value::<Composition>(unknown).is_err());
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
    let before_encode_failure = kernel.inspect()["calls"].clone();
    assert_eq!(
        root.invoke(&Value::Bytes(vec![0; phenix_contract::MAX_BYTES + 1]))
            .unwrap_err()
            .code,
        Code::SchemaRejected
    );
    assert_eq!(kernel.inspect()["calls"], before_encode_failure);
    assert_eq!(kernel.inspect()["last_failure"]["phase"], "invocation");
    assert_eq!(
        root.invoke(&request("again")).unwrap_err().code,
        Code::PluginFailed
    );
    drop(root);
    let successful = kernel.inspect();
    assert_eq!(successful["calls"], 8);
    drop(kernel);
    // The plugin handles its undeclared import and returns successfully. The
    // denial remains an invocation observation, with no selected count provider.
    let kernel = Prepared::prepare(d.clone()).unwrap().load().unwrap();
    let root = kernel.admit_default_root().unwrap();
    assert_eq!(
        root.invoke(&request("handled-denial")).unwrap(),
        Outcome::Success(request("handled"))
    );
    let handled_failure = kernel.inspect()["last_failure"].clone();
    assert_eq!(handled_failure["code"], "Denied");
    assert_eq!(handled_failure["phase"], "invocation");
    assert_eq!(
        handled_failure["context"],
        serde_json::json!({
            "caller":"example.consumer", "provider":null,
            "operation":"example.transform.count@1/count",
        })
    );
    assert_eq!(kernel.inspect()["calls"], 1);
    assert_eq!(kernel.inspect()["denied_operations"], 1);
    assert_eq!(
        root.invoke(&request("ok")).unwrap(),
        Outcome::Success(request("OK"))
    );
    assert_eq!(kernel.inspect()["last_failure"], handled_failure);
    assert_eq!(
        root.invoke(&request("replace-failure")).unwrap_err().code,
        Code::PluginFailed
    );
    let replaced_failure = kernel.inspect()["last_failure"].clone();
    assert_eq!(replaced_failure["message"], "consumer replacement failure");
    assert_eq!(
        replaced_failure["context"],
        serde_json::json!({
            "caller":"root", "provider":"example.consumer",
            "operation":"example.consumer@1/run",
        })
    );
    drop(root);
    // Every recoverable admission error must be observable and leave the kernel
    // available for a later valid root. Existing instances remain owned here.
    assert_eq!(
        kernel
            .admit_root(selection("example.transform.apply@1", "apply",
                "example.provider"
            ))
            .err()
            .unwrap()
            .code,
        Code::Denied
    );
    let admission_failure = kernel.inspect()["last_failure"].clone();
    assert_eq!(admission_failure["phase"], "admission");
    let root = kernel.admit_default_root().unwrap();
    assert_eq!(
        kernel.admit_default_root().err().unwrap().code,
        Code::ReentrancyRejected
    );
    assert_eq!(
        kernel.inspect()["last_failure"]["code"],
        "ReentrancyRejected"
    );
    assert_eq!(kernel.inspect()["last_failure"]["phase"], "admission");
    drop(root);
    let root = kernel.admit_default_root().unwrap();
    assert_eq!(
        root.invoke(&request("handled-denial")).unwrap(),
        Outcome::Success(request("handled"))
    );
    drop(root);
    assert_eq!(kernel.inspect()["roots"]["active"], serde_json::Value::Null);
    let observations = serde_json::json!({"handled":handled_failure,"replaced":replaced_failure,
        "admission":admission_failure,"terminal":kernel.inspect()});
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
        .admit_root(selection("example.transform.count@1", "count",
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
    assert_eq!(denied["last_failure"]["phase"], "invocation");
    assert_eq!(
        denied["last_failure"]["context"],
        serde_json::json!({
            "caller":"example.consumer", "provider":"example.provider",
            "operation":"example.transform.apply@1/apply",
        })
    );
    drop(kernel);
    let cycle_key=selection("example.cycle@1","run","example.cycle");
    let mut cycle=Composition {format:"phenix.composition.v2".into(),artifacts:vec![args[4].clone()],definitions:vec![],entry:cycle_key.clone(),
        slots:vec![phenix_conformance::binding("example.cycle","example.cycle@1/run","example.cycle")],
        grants:["root","example.cycle"].map(|caller|ScopedGrant {caller:caller.into(),interface:cycle_key.interface.clone(),operation:cycle_key.operation.clone(),resources:BTreeSet::new()}).into(),
        excluded:BTreeSet::new(),substitutions:vec![],authority:Authority {operations:[cycle_key.key()].into(),resources:BTreeSet::new()},resource:None};
    expect_prepare_error(cycle.clone(),Code::MissingBinding);
    let optional=mutate_package(Path::new(&args[4]),&work.join("optional-cycle"),|value| {
        let Value::Map(root)=value else {panic!()}; let Value::List(imports)=root.get_mut("requires").unwrap() else {panic!()};
        let Value::Map(import)=&mut imports[0] else {panic!()}; import.insert("cardinality".into(),Value::String("optional".into())); import.insert("minimum".into(),Value::U64(0));
    },true);
    cycle.artifacts=vec![optional.to_string_lossy().into()];
    let cycle_kernel=Prepared::prepare(cycle).unwrap().load().unwrap();
    let cycle_root=cycle_kernel.admit_default_root().unwrap();
    assert_eq!(cycle_root.invoke(&request("cycle")).unwrap_err().code,Code::ReentrancyRejected);
    drop(cycle_root);assert_eq!(cycle_kernel.inspect()["calls"],1);drop(cycle_kernel);
    for bad in &args[5..9] {
        let entry=selection("example.transform.apply@1","apply","example.provider");
        let deployment=Composition {format:"phenix.composition.v2".into(),artifacts:vec![bad.clone()],definitions:vec![],entry:entry.clone(),slots:vec![],
            grants:vec![ScopedGrant {caller:"root".into(),interface:entry.interface.clone(),operation:entry.operation.clone(),resources:BTreeSet::new()}],
            authority:Authority {operations:[entry.key()].into(),resources:BTreeSet::new()},excluded:BTreeSet::new(),substitutions:vec![],resource:None};
        let prepared = Prepared::prepare(deployment).unwrap();
        match prepared.load() {
            Err(e) => assert_eq!(e.code, Code::AbiIncompatible),
            Ok(_) => panic!("invalid loaded ABI accepted"),
        }
    }
    // Consumer starts first in canonical identity order. Foreign provider
    // allocates an instance and a reply, then returns an invalid status. Both
    // producer resources and the already started consumer must be stopped once.
    let lifecycle = work.join("lifecycle");
    std::env::set_var("PHENIX_LIFECYCLE_WITNESS", &lifecycle);
    let optional_consumer=mutate_package(Path::new(consumer),&work.join("optional-consumer"),|value| {
        let Value::Map(root)=value else {panic!()};let Value::List(imports)=root.get_mut("requires").unwrap() else {panic!()};
        let Value::Map(import)=&mut imports[0] else {panic!()};import.insert("cardinality".into(),Value::String("optional".into()));import.insert("minimum".into(),Value::U64(0));
    },true);
    let rollback = Prepared::prepare(deployment(&args[9], &optional_consumer.to_string_lossy())).unwrap();
    assert!(!lifecycle.exists(), "preparation executed lifecycle code");
    match rollback.load() {
        Err(e) => assert_eq!(e.code, Code::AbiIncompatible),
        Ok(_) => panic!("failed create admitted a kernel"),
    }
    std::env::remove_var("PHENIX_LIFECYCLE_WITNESS");
    let rollback_events = fs::read_to_string(lifecycle).unwrap();
    assert_eq!(
        rollback_events.lines().collect::<Vec<_>>(),
        [
            "consumer:create",
            "foreign:create",
            "foreign:release",
            "foreign:stop",
            "foreign:release",
            "consumer:stop",
        ]
    );
    assert!(!rollback_events.contains("foreign:invoke"));
    fs::write(output.join("native.json"),serde_json::to_vec_pretty(&serde_json::json!({"format":"phenix.conformance.v1","sdk_version":"0.1.0","contract_version":"0.1.0","successful":successful,"denied":denied,"cases":["offline-no-execution","real-nested-call","domain-error","panic-translation","poisoned-provider","missing-binding","single-provider-needs-selection","multiple-providers-still-need-selection","absent-selection","schema-mismatch","bad-abi","metadata-mismatch","wrong-owner","artifact-tamper","unknown-policy","binding-order","schema-rejection","denied-counter-unchanged","buffer-roundtrip","native-cycle","loaded-abi-major","loaded-short-table","loaded-null-entries","loaded-null-release","failed-create-rollback","unsigned-contract-revision","handled-host-denial","replaced-provider-failure","admission-failure-recovery"],"observations":observations,"rollback_events":rollback_events.lines().collect::<Vec<_>>()})).unwrap()).unwrap();
    fs::remove_dir_all(work).unwrap();
    println!("M1 native conformance passed");
}
