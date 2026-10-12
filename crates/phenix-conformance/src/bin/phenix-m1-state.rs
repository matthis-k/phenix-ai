//! Independent trusted host assertions. Does not link either fixture's code.
use phenix_contract::{Authority, Code, ScopedRequest, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Call, Generations, Kernel, Outcome, Poll, Retirement,
};
use std::{collections::BTreeSet, fs, path::Path};
const PROVIDER: &str = "test.state-provider";
const CLIENT: &str = "test.state-client";
const COUNTER: &str = "test.state-provider.property.counter@1";
const NOTES: &str = "test.state-provider.property.notes@1";
fn selection(key: &str, provider: &str) -> Selection {
    let (interface, operation) = key.split_once('/').unwrap();
    Selection {
        interface: interface.into(),
        operation: operation.into(),
        provider: provider.into(),
    }
}
fn composition(provider: &str, client: &str, alternative: &str) -> Composition {
    let resources: BTreeSet<_> = [COUNTER, NOTES]
        .map(|s| s.trim_end_matches("@1").to_owned())
        .into();
    let mut grants = vec![];
    let mut operations = BTreeSet::new();
    for (caller, keys) in [
        (
            "root",
            vec![
                "test.state@1/run".into(),
                "test.state@1/read".into(),
                "test.refs@1/get".into(),
                "test.provider@1/run".into(),
                format!("{COUNTER}/get"),
            ],
        ),
        (
            CLIENT,
            vec![
                "test.refs@1/get".into(),
                "test.refs@1/notes".into(),
                format!("{COUNTER}/get"),
                format!("{COUNTER}/set"),
                format!("{COUNTER}/watch"),
                format!("{NOTES}/get"),
                format!("{NOTES}/set"),
                format!("{NOTES}/watch"),
            ],
        ),
    ] {
        for key in keys {
            let s = selection(&key, "");
            operations.insert(key);
            grants.push(ScopedGrant {
                caller: caller.into(),
                interface: s.interface,
                operation: s.operation,
                resources: resources.clone(),
            });
        }
    }
    let slot = |slot: &str, export: &str| SlotBinding {
        caller: CLIENT.into(),
        slot: slot.into(),
        index: 0,
        provider: PROVIDER.into(),
        export: export.into(),
    };
    Composition {
        format: "phenix.composition.v2".into(),
        definitions: vec![],
        slots: vec![
            slot("refs", "refs"),
            slot("counter", "property.counter"),
            slot("notes", "property.notes"),
            slot("many", "refs"),
            SlotBinding {
                caller: CLIENT.into(),
                slot: "many".into(),
                index: 1,
                provider: "test.state-alternative".into(),
                export: "refs".into(),
            },
        ],
        
            artifacts: vec![provider.into(), client.into(), alternative.into()],
            entry: selection("test.state@1/run", CLIENT),
            grants,
            excluded: BTreeSet::new(),
            substitutions: vec![],
            authority: Authority {
                operations,
                resources,
            },
            resource: None,
    }
}
fn run(kernel: &Kernel, command: &str) -> Outcome {
    kernel
        .admit_default_root()
        .unwrap()
        .invoke(&Value::String(command.into()))
        .unwrap()
}
fn success(values: &[u64]) -> Outcome {
    Outcome::Success(Value::List(
        values.iter().copied().map(Value::U64).collect(),
    ))
}
fn domain(name: &str) -> Outcome {
    Outcome::Domain(Value::String(name.into()))
}
fn reference(kernel: &Kernel) -> Value {
    let Outcome::Success(r) = kernel
        .admit_root(selection("test.refs@1/get", PROVIDER))
        .unwrap()
        .invoke(&Value::String(String::new()))
        .unwrap()
    else {
        panic!("ref");
    };
    r
}
fn revision(kernel: &Kernel, property: &str) -> u64 {
    kernel.inspect()["live_state"]["revisions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["property"] == property)
        .unwrap()["revision"]
        .as_u64()
        .unwrap()
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "--process-ref") {
        let c: Composition = serde_json::from_slice(&fs::read(&args[2]).unwrap()).unwrap();
        let kernel = Prepared::prepare(c).unwrap().load().unwrap();
        let value = reference(&kernel);
        let bytes: String = value
            .encode()
            .unwrap()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        println!("{bytes}");
        kernel.shutdown().unwrap();
        return;
    }
    assert_eq!(
        args.len(),
        8,
        "PROVIDER CLIENT ALTERNATIVE BAD_INIT BAD_SCHEMA BAD_TABLE OUTPUT"
    );
    let output = Path::new(&args[7]);
    fs::create_dir_all(output).unwrap();
    let mut c = composition(&args[1], &args[2], &args[3]);
    let mut definitions = phenix_contract::ContractMetadata::decode(
        &fs::read(Path::new(&args[1]).join("descriptor.bin")).unwrap(),
    )
    .unwrap();
    definitions.plugin = "test.state-definitions".into();
    definitions.execution = phenix_contract::MetadataExecution::None;
    definitions.provides.clear();
    definitions.requires.clear();
    definitions.properties.clear();
    let package = output.join("definitions");
    fs::create_dir_all(&package).unwrap();
    let bytes = definitions.encode().unwrap();
    fs::write(package.join("descriptor.bin"), &bytes).unwrap();
    fs::write(
        package.join("package.json"),
        serde_json::to_vec(&phenix_artifact::ContractPackageRecord {
            format: "phenix.contract-package.v2".into(),
            descriptor: "descriptor.bin".into(),
            descriptor_sha256: phenix_artifact::digest(&bytes),
        })
        .unwrap(),
    )
    .unwrap();
    c.definitions.push(package.to_str().unwrap().into());
    fs::write(
        output.join("composition.json"),
        serde_json::to_vec_pretty(&c).unwrap(),
    )
    .unwrap();
    let prepared = Prepared::prepare(c.clone()).unwrap();
    let identity = prepared.generation_identity("test").unwrap();
    let offline = prepared.inspect();
    assert_eq!(offline["state"]["slots"].as_array().unwrap().len(), 4);
    let mut reversed = c.clone();
    reversed.artifacts.reverse();
    reversed.grants.reverse();
    reversed.slots.reverse();
    assert_eq!(
        Prepared::prepare(reversed)
            .unwrap()
            .generation_identity("test")
            .unwrap(),
        identity
    );
    let kernel = prepared.load().unwrap();
    assert_eq!(kernel.inspect()["live_state"]["cells"], 4);
    let mut cases = vec![];
    let mut check = |name: &str, actual: Outcome, expected: Outcome| {
        assert_eq!(actual, expected, "{name}");
        cases.push(serde_json::json!({"case":name,"passed":true}));
    };
    check(
        "identity-snapshots-cas",
        run(&kernel, "basics"),
        success(&[1, 0, 0, 1, 1, 1, 0]),
    );
    check(
        "watch-bootstrap-coalescing",
        run(&kernel, "watch"),
        success(&[1, 1, 3, 3, 1, 1]),
    );
    check(
        "batch-one-epoch",
        run(&kernel, "batch"),
        success(&[4, 4, 4, 4]),
    );
    check(
        "consistent-epoch-snapshot",
        run(&kernel, "snapshot"),
        success(&[4, 4, 4]),
    );
    let gap = Prepared::prepare(c.clone()).unwrap().load().unwrap();
    check(
        "read-subscribe-gap",
        run(&gap, "bootstrap-gap"),
        success(&[1, 1, 3, 3, 1, 1]),
    );
    gap.shutdown().unwrap();
    let counter = revision(&kernel, "counter");
    let notes = revision(&kernel, "notes");
    let epoch = kernel.inspect()["live_state"]["epoch"].clone();
    check(
        "batch-conflict",
        run(&kernel, "bad-batch"),
        domain("Conflict"),
    );
    assert_eq!(revision(&kernel, "counter"), counter);
    assert_eq!(revision(&kernel, "notes"), notes);
    assert_eq!(kernel.inspect()["live_state"]["epoch"], epoch);
    check(
        "value-schema",
        run(&kernel, "schema"),
        domain("SchemaMismatch"),
    );
    for (command, error) in [
        ("owner", "OwnerGone"),
        ("cell", "StaleReference"),
        ("generation", "StaleReference"),
        ("ref-schema", "SchemaMismatch"),
    ] {
        check(command, run(&kernel, command), domain(error));
    }
    check("indexed-many-slot", run(&kernel, "many"), success(&[2, 1]));
    check(
        "selected-ref-grants-no-authority",
        run(&kernel, "many-ref-denied"),
        domain("Denied"),
    );
    let _ = run(&kernel, "leak");
    assert_eq!(kernel.inspect()["live_state"]["subscriptions"], 0);
    check(
        "root-cursor-cleanup",
        run(&kernel, "leaked"),
        domain("StaleReference"),
    );
    let ref_before = reference(&kernel);
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--process-ref",
            output.join("composition.json").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    let text = String::from_utf8(child.stdout).unwrap();
    let text = text.trim();
    let bytes = (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect::<Vec<_>>();
    let foreign_ref = Value::decode(&bytes).unwrap();
    let foreign_read = kernel
        .admit_root(selection("test.state@1/read", CLIENT))
        .unwrap();
    assert_eq!(
        foreign_read.invoke(&foreign_ref).unwrap(),
        domain("OwnerGone")
    );
    drop(foreign_read);
    assert_ne!(foreign_ref, ref_before);
    let read = kernel
        .admit_root(selection("test.state@1/read", CLIENT))
        .unwrap();
    assert_eq!(
        read.invoke(&ref_before).unwrap(),
        Outcome::Success(Value::U64(4))
    );
    drop(read);
    let property = kernel
        .admit_root(selection(&format!("{COUNTER}/get"), PROVIDER))
        .unwrap();
    assert_eq!(
        property.invoke(&ref_before).unwrap(),
        Outcome::Success(Value::U64(4))
    );
    drop(property);
    for command in ["thread", "expired"] {
        let root = kernel
            .admit_root(selection("test.provider@1/run", PROVIDER))
            .unwrap();
        check(
            command,
            root.invoke(&Value::String(command.into())).unwrap(),
            success(&[1]),
        );
    }
    let pending_root = kernel
        .admit_root(selection("test.provider@1/run", PROVIDER))
        .unwrap();
    let Call::Pending(ticket) = pending_root
        .begin(&Value::String("pending".into()))
        .unwrap()
    else {
        panic!("ticket");
    };
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
    let Poll::Ready(result) = pending_root.poll(&ticket).unwrap() else {
        panic!("ready");
    };
    check("abi3-pending", result.unwrap(), success(&[7]));
    pending_root.release(&ticket).unwrap();
    drop(pending_root);
    let cancelled = kernel
        .admit_root(selection("test.provider@1/run", PROVIDER))
        .unwrap();
    let Call::Pending(ticket) = cancelled.begin(&Value::String("pending".into())).unwrap() else {
        panic!("ticket");
    };
    assert!(cancelled.cancel(&ticket).unwrap());
    cancelled.release(&ticket).unwrap();
    drop(cancelled);
    cases.push(serde_json::json!({"case":"cross-process-incarnation-ref-rejection","passed":true}));
    cases.push(serde_json::json!({"case":"abi3-cancel","passed":true}));
    let mut denied = c.clone();
    denied
        .grants
        .retain(|g| !(g.caller == CLIENT && g.key() == format!("{COUNTER}/set")));
    let denied = Prepared::prepare(denied).unwrap().load().unwrap();
    assert_eq!(run(&denied, "denied"), domain("Denied"));
    assert_eq!(revision(&denied, "counter"), 0);
    denied.shutdown().unwrap();
    cases.push(serde_json::json!({"case":"denied-write-zero-effects","passed":true}));
    let mut scope = c.authority.clone();
    scope.operations.remove(&format!("{COUNTER}/set"));
    let narrow = kernel
        .admit_scoped_root(
            c.entry.clone(),
            ScopedRequest {
                authority: scope,
                resource: None,
            },
        )
        .unwrap();
    assert_eq!(
        narrow.invoke(&Value::String("denied".into())).unwrap(),
        domain("Denied")
    );
    drop(narrow);
    assert_eq!(revision(&kernel, "counter"), counter);
    cases.push(serde_json::json!({"case":"root-intersection-zero-effects","passed":true}));
    let mut no_resources = c.clone();
    no_resources.authority.resources.clear();
    let no_resources = Prepared::prepare(no_resources)
        .unwrap()
        .load()
        .unwrap();
    assert_eq!(run(&no_resources, "count"), domain("Denied"));
    no_resources.shutdown().unwrap();
    cases.push(serde_json::json!({"case":"resource-grant-required","passed":true}));
    // A fresh incarnation from identical packages cannot resurrect a ref.
    let fresh = Prepared::prepare(c.clone()).unwrap().load().unwrap();
    let read = fresh
        .admit_root(selection("test.state@1/read", CLIENT))
        .unwrap();
    assert_eq!(read.invoke(&ref_before).unwrap(), domain("OwnerGone"));
    drop(read);
    fresh.shutdown().unwrap();
    cases.push(serde_json::json!({"case":"incarnation-ref-rejection","passed":true}));
    let mut session_context = c.clone();
    session_context.resource = Some("session.root".into());
    session_context
        .authority
        .resources
        .insert("session.root".into());
    for grant in &mut session_context.grants {
        grant.resources.insert("session.root".into());
    }
    let session_context = Prepared::prepare(session_context)
        .unwrap()
        .load()
        .unwrap();
    assert_eq!(run(&session_context, "batch"), success(&[1, 1, 1, 1]));
    session_context.shutdown().unwrap();
    cases.push(serde_json::json!({"case":"resource-context-retained-across-batch","passed":true}));
    let mut negatives = vec![];
    for (name, change) in [
        ("missing-slot", 0),
        ("wrong-export", 1),
        ("noncontiguous-index", 2),
        ("duplicate-slot", 3),
    ] {
        let mut invalid = c.clone();
        match change {
            0 => {
                invalid.slots.remove(0);
            }
            1 => invalid.slots[0].export = "missing".into(),
            2 => invalid.slots[4].index = 2,
            _ => invalid.slots.push(invalid.slots[0].clone()),
        };
        assert!(Prepared::prepare(invalid).is_err(), "{name}");
        negatives.push(name);
    }
    for (name, index, code) in [
        ("missing-property-init", 4, Code::PluginFailed),
        ("invalid-property-init", 5, Code::SchemaRejected),
        ("short-state-table", 6, Code::AbiIncompatible),
    ] {
        let bad = composition(&args[index], &args[2], &args[3]);
        let err = Prepared::prepare(bad).unwrap().load().err().unwrap();
        assert_eq!(err.code, code, "{name}");
        negatives.push(name);
    }
    let mut invalid = c.clone();
    let bad_package = output.join("conflicting-definitions");
    fs::create_dir_all(&bad_package).unwrap();
    definitions.plugin = "test.conflicting-definitions".into();
    definitions.definitions[0].operations[0].response = phenix_contract::ValueSchema::String;
    let bytes = definitions.encode().unwrap();
    fs::write(bad_package.join("descriptor.bin"), &bytes).unwrap();
    fs::write(
        bad_package.join("package.json"),
        serde_json::to_vec(&phenix_artifact::ContractPackageRecord {
            format: "phenix.contract-package.v2".into(),
            descriptor: "descriptor.bin".into(),
            descriptor_sha256: phenix_artifact::digest(&bytes),
        })
        .unwrap(),
    )
    .unwrap();
    invalid
        .definitions
        .push(bad_package.to_str().unwrap().into());
    assert!(Prepared::prepare(invalid).is_err());
    negatives.push("conflicting-definitions");
    let mut duplicate = c.clone();
    let mut repeated_binding = duplicate.slots[3].clone();
    repeated_binding.index = 2;
    duplicate.slots.push(repeated_binding);
    assert!(Prepared::prepare(duplicate).is_err());
    negatives.push("duplicate-provider-selection");
    let Outcome::Domain(order_error) = run(&kernel, "snapshot-order") else {
        panic!("snapshot order");
    };
    assert!(matches!(order_error,Value::String(s) if s.contains("SchemaRejected")));
    negatives.push("snapshot-order");
    let mut missing_get = c.clone();
    missing_get
        .grants
        .retain(|g| !(g.caller == CLIENT && g.key() == format!("{NOTES}/get")));
    let missing_get = Prepared::prepare(missing_get)
        .unwrap()
        .load()
        .unwrap();
    let Outcome::Domain(denied_snapshot) = run(&missing_get, "snapshot") else {
        panic!("snapshot authority");
    };
    assert!(matches!(denied_snapshot,Value::String(s) if s.contains("Denied")));
    missing_get.shutdown().unwrap();
    cases.push(serde_json::json!({"case":"snapshot-all-get-grants","passed":true}));
    let before = kernel.inspect();
    kernel.shutdown().unwrap();
    assert_eq!(kernel.inspect()["live_state"]["cells"], 0);
    let mut generations = Generations::default();
    let old = generations
        .install_candidate(Prepared::prepare(c.clone()).unwrap(), "old")
        .unwrap();
    generations.promote(&old).unwrap();
    let pinned = generations
        .admit_root_on(&old, selection("test.refs@1/get", PROVIDER))
        .unwrap();
    let Outcome::Success(Value::Ref(location)) =
        pinned.invoke(&Value::String(String::new())).unwrap()
    else {
        panic!("generation ref");
    };
    let hex: String = location
        .generation
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hex, old.as_str());
    let new = generations
        .install_candidate(Prepared::prepare(c).unwrap(), "new")
        .unwrap();
    generations.promote(&new).unwrap();
    assert!(matches!(
        generations.retire(&old).unwrap(),
        Retirement::Blocked { .. }
    ));
    drop(pinned);
    assert!(matches!(
        generations.retire(&old).unwrap(),
        Retirement::Retired
    ));
    cases.push(serde_json::json!({"case":"generation-pin-and-retirement","passed":true}));
    fs::write(output.join("state.json"),serde_json::to_vec_pretty(&serde_json::json!({"format":"phenix.m1-state-evidence.v1","cases":cases,"negative_cases":negatives,"identity":identity,"offline":offline,"before_stop":before,"after_stop":kernel.inspect()})).unwrap()).unwrap();
    println!("S2 live state native gate passed");
}
