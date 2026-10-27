//! End-to-end *production* Prepared/Kernel/Root/Host ABI 3 validation.
//! No fake host callbacks or direct so-to-so dynamic symbol calls are used.
use phenix_contract::{Authority, Code, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Outcome,
};
use std::{collections::BTreeSet, fs, path::Path};

const RUN: &str = "example.consumer@1/run";
const TRANSFORM: &str = "example.transform.apply@1/apply";
const ADAPTER: &str = "example.python-host";
const PROVIDER: &str = "example.provider";

fn selection(key: &str, provider: &str) -> Selection {
    let (interface, operation) = key.split_once('/').unwrap();
    Selection {
        interface: interface.into(),
        operation: operation.into(),
        provider: provider.into(),
    }
}
fn grant(caller: &str, key: &str) -> ScopedGrant {
    let s = selection(key, "");
    ScopedGrant {
        caller: caller.into(),
        interface: s.interface,
        operation: s.operation,
        resources: BTreeSet::new(),
    }
}
fn text(s: &str) -> Value {
    Value::map([("text", Value::String(s.into()))])
}
fn composition(python: &str, rust: &str) -> Composition {
    Composition {
        format: "phenix.composition.v3".into(),
        definitions: vec![],
        artifacts: vec![python.into(), rust.into()],
        entry: selection(RUN, ADAPTER),
        slots: vec![SlotBinding {
            caller: ADAPTER.into(),
            slot: "Provider".into(),
            index: 0,
            provider: PROVIDER.into(),
            export: "Transform".into(),
        }],
        grants: vec![grant("root", RUN), grant(ADAPTER, TRANSFORM)],
        authority: Authority {
            operations: [RUN.into(), TRANSFORM.into()].into(),
            resources: BTreeSet::new(),
        },
        excluded: BTreeSet::new(),
        substitutions: vec![],
        resource: None,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(
        args.len(),
        3,
        "usage: RUNNER PYTHON-PACKAGE RUST-PROVIDER-PACKAGE"
    );
    let policy = composition(&args[1], &args[2]);

    // Missing declared provider binding rejects before native code can load.
    let mut unbound = policy.clone();
    unbound.slots.clear();
    assert_eq!(
        Prepared::prepare(unbound).err().unwrap().code,
        Code::MissingBinding
    );

    // A bound export from the wrong package must also fail at preparation.
    let mut wrong = policy.clone();
    wrong.slots[0].provider = ADAPTER.into();
    assert_eq!(
        Prepared::prepare(wrong).err().unwrap().code,
        Code::MissingBinding
    );

    // Exact packaged artifact hashes are checked offline, before invocation.
    let work = std::env::temp_dir().join(format!(
        "phenix-python-kernel-verify-{}",
        std::process::id()
    ));
    fs::create_dir_all(work.join("tampered/lib")).unwrap();
    let original = Path::new(&args[1]);
    fs::copy(
        original.join("package.json"),
        work.join("tampered/package.json"),
    )
    .unwrap();
    fs::copy(
        original.join("descriptor.bin"),
        work.join("tampered/descriptor.bin"),
    )
    .unwrap();
    let mut bytes = fs::read(original.join("lib/plugin.so")).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(work.join("tampered/lib/plugin.so"), bytes).unwrap();
    let mut tampered = policy.clone();
    tampered.artifacts[0] = work.join("tampered").to_string_lossy().into();
    assert_eq!(
        Prepared::prepare(tampered).err().unwrap().code,
        Code::ArtifactInvalid
    );

    let prepared =
        Prepared::prepare(policy.clone()).expect("offline ABI-3 composition verification");
    let kernel = prepared
        .load()
        .expect("load compiled native and Python runtime plugin");
    let root = kernel.admit_default_root().unwrap();

    for (input, result) in [
        ("one two", "one,two"),
        ("enrich:hello world", "HELLO WORLD"),
        ("enrich:again", "AGAIN"),
        ("one", "one"),
    ] {
        assert_eq!(
            root.invoke(&text(input)).unwrap(),
            Outcome::Success(text(result)),
            "kernel selected dispatch failed: {input}"
        );
    }
    // An undeclared method is never callable, even though it exists in Python.
    for input in ["private", "raise", "wrong-result", "enrich:stale"] {
        let failure = root.invoke(&text(input)).unwrap_err();
        assert_eq!(failure.code, Code::PluginFailed, "{input}: {failure}");
    }
    // Request cannot bypass the ABI's declared Text schema.
    assert_eq!(
        root.invoke(&Value::Bool(true)).unwrap_err().code,
        Code::SchemaRejected
    );
    // Python tries to invoke an unprovided operation. The adapter refuses it.
    assert_eq!(
        root.invoke(&text("enrich:undeclared")).unwrap_err().code,
        Code::Denied
    );
    // The real Rust provider rejects an empty request with a domain outcome.
    assert!(matches!(
        root.invoke(&text("enrich:")),
        Ok(Outcome::Domain(_))
    ));
    // The Rust provider's deliberate panic crosses the real native ABI as
    // a kernel failure, not a successful Python result or uncaught unwind.
    assert_eq!(
        root.invoke(&text("enrich:panic")).unwrap_err().code,
        Code::PluginFailed
    );
    drop(root);
    kernel.shutdown().unwrap();

    // An unavailable grant is never converted to a successful Python result.
    let mut denied = policy.clone();
    denied.grants.retain(|grant| grant.caller != ADAPTER);
    match Prepared::prepare(denied) {
        Err(failure) => assert_eq!(failure.code, Code::Denied),
        Ok(prepared) => {
            let denied_kernel = prepared.load().unwrap();
            let root = denied_kernel.admit_default_root().unwrap();
            assert_eq!(
                root.invoke(&text("enrich:hello")).unwrap_err().code,
                Code::Denied
            );
            drop(root);
            denied_kernel.shutdown().unwrap();
        }
    }
    let _ = fs::remove_dir_all(&work);
    println!("PASS: real Phenix kernel -> Python-hosting .so -> annotated Python -> real kernel Host::invoke -> independently compiled Rust provider; positive and negative");
}
