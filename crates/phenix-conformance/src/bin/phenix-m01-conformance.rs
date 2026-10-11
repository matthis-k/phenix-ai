use phenix_contract::{Authority, Code, ScopedRequest, Value};
use phenix_kernel::{
    resolve::{Composition, ImportBinding, Prepared, ScopedGrant, Selection, Substitution},
    Outcome,
};
use std::{collections::BTreeSet, fs, path::Path};
fn selection(key: &str, provider: &str) -> Selection {
    let (interface, operation) = key.split_once('/').unwrap();
    Selection {
        interface: interface.into(),
        operation: operation.into(),
        provider: provider.into(),
    }
}
fn bind(caller: &str, key: &str, provider: &str) -> ImportBinding {
    let s = selection(key, provider);
    ImportBinding {
        caller: caller.into(),
        interface: s.interface,
        operation: s.operation,
        provider: s.provider,
    }
}
fn grant(caller: &str, key: &str, resources: &[&str]) -> ScopedGrant {
    let s = selection(key, "");
    ScopedGrant {
        caller: caller.into(),
        interface: s.interface,
        operation: s.operation,
        resources: resources.iter().map(|s| (*s).into()).collect(),
    }
}
fn text(text: &str) -> Value {
    Value::map([("text", Value::String(text.into()))])
}
fn count(count: u64) -> Value {
    Value::map([("count", Value::U64(count))])
}
fn fail(c: Composition, code: Code) -> String {
    let e = Prepared::prepare_composition(c).err().unwrap();
    assert_eq!(e.code, code);
    e.message
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        10,
        "PROVIDER ALTERNATE CHOICE OTHER RESTRICTED RELAY RESOURCES CYCLE OUTPUT"
    );
    // The accepted M0.0 declarations keep their byte identity under the new SDK.
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(Path::new(&args[1]).join("package.json")).unwrap())
            .unwrap();
    assert_eq!(
        record["descriptor_sha256"],
        "0fe90725a4573bcaee5421584607bcd4915044dd0c177cd9e56f5060a1f1a261"
    );
    let output = Path::new(&args[9]);
    fs::create_dir_all(output).unwrap();
    let witness = output.join("constructor-witness");
    std::env::set_var("PHENIX_CONSTRUCTOR_WITNESS", &witness);
    let transform = "example.transform@1/apply";
    let counter = "example.transform@1/count";
    let choice = "example.choice@1/run";
    let other = "example.other@1/run";
    let mut c = Composition {
        format: "phenix.composition.v1".into(),
        artifacts: args[1..5].to_vec(),
        entry: selection(choice, "example.choice"),
        bindings: vec![
            bind("example.choice", transform, "example.alternate"),
            bind("example.other", transform, "example.provider"),
        ],
        grants: vec![
            grant("root", choice, &[]),
            grant("root", other, &[]),
            grant("root", counter, &[]),
            grant("example.choice", transform, &[]),
            grant("example.choice", counter, &[]),
            grant("example.other", transform, &[]),
        ],
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: [choice, other, transform, counter]
                .map(str::to_owned)
                .into(),
            resources: BTreeSet::new(),
        },
        resource: None,
    };
    let prepared = Prepared::prepare_composition(c.clone()).unwrap();
    let baseline = prepared.inspect();
    let mut reordered = c.clone();
    reordered.artifacts.reverse();
    reordered.bindings.reverse();
    reordered.grants.reverse();
    assert_eq!(
        Prepared::prepare_composition(reordered).unwrap().inspect(),
        baseline
    );
    let mut bad = c.clone();
    bad.bindings[1].provider = "example.alternate".into();
    fail(bad, Code::MissingBinding);
    let mut bad = c.clone();
    bad.bindings.clear();
    let missing = fail(bad.clone(), Code::AmbiguousBinding);
    bad.artifacts.reverse();
    assert_eq!(fail(bad, Code::AmbiguousBinding), missing);
    let mut bad = c.clone();
    bad.excluded.insert("example.alternate".into());
    fail(bad, Code::MissingBinding);
    let mut bad = c.clone();
    bad.substitutions = vec![Substitution {
        from: "example.provider".into(),
        to: "example.alternate".into(),
    }];
    fail(bad, Code::MissingBinding);
    let mut bad = c.clone();
    bad.substitutions = vec![
        Substitution {
            from: "example.provider".into(),
            to: "example.alternate".into(),
        },
        Substitution {
            from: "example.alternate".into(),
            to: "example.provider".into(),
        },
    ];
    fail(bad, Code::DescriptorInvalid);
    assert!(
        !witness.exists(),
        "rejected preparation executed provider constructor"
    );
    let kernel = prepared.load().unwrap();
    assert!(witness.exists());
    let root = kernel.admit_default_root().unwrap();
    assert_eq!(
        root.invoke(&text("mixed")).unwrap(),
        Outcome::Success(text("mixed"))
    );
    assert_eq!(
        root.invoke(&text("optional")).unwrap_err().code,
        Code::MissingBinding
    );
    drop(root);
    let root = kernel
        .admit_root(selection(other, "example.other"))
        .unwrap();
    assert_eq!(
        root.invoke(&text("mixed")).unwrap(),
        Outcome::Success(text("MIXED"))
    );
    drop(root);
    let root = kernel
        .admit_root(selection(counter, "example.provider"))
        .unwrap();
    assert_eq!(root.invoke(&count(0)).unwrap(), Outcome::Success(count(1)));
    drop(root);
    let choices = kernel.inspect();
    drop(kernel);
    c.bindings
        .push(bind("example.choice", counter, "example.provider"));
    let kernel = Prepared::prepare_composition(c.clone())
        .unwrap()
        .load()
        .unwrap();
    let root = kernel.admit_default_root().unwrap();
    assert_eq!(
        root.invoke(&text("optional")).unwrap(),
        Outcome::Success(text("0"))
    );
    drop(root);
    drop(kernel);
    c.bindings.retain(|b| b.key() != counter);
    c.substitutions.push(Substitution {
        from: "example.alternate".into(),
        to: "example.provider".into(),
    });
    let kernel = Prepared::prepare_composition(c.clone())
        .unwrap()
        .load()
        .unwrap();
    let root = kernel.admit_default_root().unwrap();
    assert_eq!(
        root.invoke(&text("substituted")).unwrap(),
        Outcome::Success(text("SUBSTITUTED"))
    );
    drop(root);
    drop(kernel);
    c.excluded.insert("example.alternate".into());
    let excluded = Prepared::prepare_composition(c.clone()).unwrap().inspect();
    assert_eq!(excluded["plugins"].as_array().unwrap().len(), 3);
    // No provider for the optional counter, yet the required transform is valid.
    let mut optional = c.clone();
    optional.artifacts = vec![args[2].clone(), args[3].clone()];
    optional.excluded.clear();
    optional.substitutions.clear();
    optional.bindings = vec![bind("example.choice", transform, "example.alternate")];
    optional.grants = vec![
        grant("root", choice, &[]),
        grant("example.choice", transform, &[]),
    ];
    optional.authority.operations = [choice, transform].map(str::to_owned).into();
    let kernel = Prepared::prepare_composition(optional)
        .unwrap()
        .load()
        .unwrap();
    let root = kernel.admit_default_root().unwrap();
    assert_eq!(
        root.invoke(&text("optional")).unwrap_err().code,
        Code::MissingBinding
    );
    drop(root);
    drop(kernel);
    let cycle = "example.cycle@1/run";
    let cycle_policy = Composition {
        format: "phenix.composition.v1".into(),
        artifacts: vec![args[8].clone()],
        entry: selection(cycle, "example.cycle"),
        bindings: vec![bind("example.cycle", cycle, "example.cycle")],
        grants: vec![
            grant("root", cycle, &[]),
            grant("example.cycle", cycle, &[]),
        ],
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: [cycle.into()].into(),
            resources: BTreeSet::new(),
        },
        resource: None,
    };
    let cycle_path = fail(cycle_policy, Code::MissingBinding);
    assert_eq!(
        cycle_path,
        "required dependency cycle: example.cycle -> example.cycle"
    );
    let run = "example.restricted@1/run";
    let relay = "example.relay@1/run";
    let bump = "example.resource@1/bump";
    let read = "example.resource@1/count";
    let scoped = Composition {
        format: "phenix.composition.v1".into(),
        artifacts: args[5..8].to_vec(),
        entry: selection(run, "example.restricted"),
        bindings: vec![
            bind("example.restricted", relay, "example.relay"),
            bind("example.relay", bump, "example.resources"),
        ],
        grants: vec![
            grant("root", run, &["red", "blue"]),
            grant("root", read, &["red", "blue"]),
            grant("example.restricted", relay, &["red"]),
            grant("example.relay", bump, &["red", "blue"]),
        ],
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: [run, relay, bump, read].map(str::to_owned).into(),
            resources: ["red".into(), "blue".into()].into(),
        },
        resource: None,
    };
    let kernel = Prepared::prepare_composition(scoped.clone())
        .unwrap()
        .load()
        .unwrap();
    let root = kernel.admit_default_root().unwrap();
    assert_eq!(
        root.invoke(&text("red")).unwrap(),
        Outcome::Success(count(1))
    );
    let before = kernel.inspect()["calls"].as_u64().unwrap();
    assert_eq!(root.invoke(&text("blue")).unwrap_err().code, Code::Denied);
    assert_eq!(kernel.inspect()["calls"].as_u64().unwrap(), before + 1);
    assert_eq!(root.invoke(&text("switch")).unwrap_err().code, Code::Denied);
    assert_eq!(root.invoke(&text("widen")).unwrap_err().code, Code::Denied);
    assert_eq!(
        root.invoke(&text("red")).unwrap(),
        Outcome::Success(count(2))
    );
    drop(root);
    // Read the actual protected counters without recreating plugin instances.
    for (resource, expected) in [("blue", 0), ("red", 2)] {
        let scope = ScopedRequest {
            authority: Authority {
                operations: [read.into()].into(),
                resources: [resource.into()].into(),
            },
            resource: Some(resource.into()),
        };
        let root = kernel
            .admit_scoped_root(selection(read, "example.resources"), scope)
            .unwrap();
        assert_eq!(
            root.invoke(&count(0)).unwrap(),
            Outcome::Success(count(expected))
        );
        drop(root);
    }
    let scoped_inspection = kernel.inspect();
    assert_eq!(scoped_inspection["denied_operations"], 3);
    drop(kernel);
    let mut read_policy = scoped.clone();
    // A root pinned to red also rejects a plugin request to switch to blue.
    read_policy.entry = selection(run, "example.restricted");
    read_policy.resource = Some("red".into());
    let kernel = Prepared::prepare_composition(read_policy)
        .unwrap()
        .load()
        .unwrap();
    let root = kernel.admit_default_root().unwrap();
    assert_eq!(root.invoke(&text("blue")).unwrap_err().code, Code::Denied);
    drop(root);
    drop(kernel);
    fs::write(output.join("composition.json"),serde_json::to_vec_pretty(&serde_json::json!({"format":"phenix.m01-evidence.v1","contract_version":"0.1.0","sdk_version":"0.1.0","cases":["per-caller-selection","optional-unbound","optional-selected","optional-provider-absent","exact-implementation","substitution","excluded-no-fallback","excluded-explicit-substitution","invalid-substitution","order-independent-binding-and-error","required-cycle-path","named-resource-forwarding","attenuated-grandchild","denied-sibling-resource","pinned-root-resource"],"choices":choices,"excluded":excluded,"cycle":cycle_path,"scoped":scoped_inspection})).unwrap()).unwrap();
    fs::write(
        output.join("composition-policy.json"),
        serde_json::to_vec(&c).unwrap(),
    )
    .unwrap();
    fs::write(
        output.join("scoped-policy.json"),
        serde_json::to_vec(&scoped).unwrap(),
    )
    .unwrap();
    println!("M0.1 composition conformance passed");
}
