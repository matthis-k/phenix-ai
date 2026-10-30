use phenix_artifact::VerifiedPackage;
use phenix_contract::{graph::v2::*, Authority, Code, ScopedRequest, Value, ValueSchema};
use phenix_kernel::{
    graph::{
        v2::{OperationSecurity, SourceOwnerPolicy, TrustedCompositionPolicy},
        GraphStatus,
    },
    resolve::{Composition, Prepared, ScopedGrant, Selection},
    Outcome,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

const OWNER: &str = "example.python-greeter";
const CONTRACT: &str = "example.greeter@1";
const OP: &str = "example.greeter@1/greet";
fn text(value: &str) -> Value {
    Value::map([("text", Value::String(value.into()))])
}
fn main() {
    let package = std::env::args().nth(1).expect("runner PACKAGE");
    let verified = VerifiedPackage::verify_trusted_read_only(Path::new(&package)).unwrap();
    let schema = &verified.metadata().definitions[0].operations[0];
    let expected = ValueSchema::Struct(BTreeMap::from([("text".into(), ValueSchema::String)]));
    assert_eq!(schema.request, expected);
    assert_eq!(schema.response, expected);
    let rust_package = std::env::args()
        .nth(2)
        .expect("runner PYTHON-PACKAGE RUST-PACKAGE");
    let rust = VerifiedPackage::verify_trusted_read_only(Path::new(&rust_package)).unwrap();
    assert_eq!(verified.metadata().definitions, rust.metadata().definitions);
    println!(
        "PASS Python-generated and Rust-authored canonical contract definitions are identical"
    );
    let graph = verified.graph_v2().expect("link-time graph");
    assert_eq!(graph.nodes.len(), 2);
    assert_eq!(graph.sources[0].payload, expected);
    let composition = Composition {
        format: "phenix.composition.v3".into(),
        definitions: vec![],
        artifacts: vec![package],
        entry: Selection {
            interface: CONTRACT.into(),
            operation: "greet".into(),
            provider: OWNER.into(),
        },
        slots: vec![],
        grants: ["root", OWNER]
            .into_iter()
            .map(|caller| ScopedGrant {
                caller: caller.into(),
                interface: CONTRACT.into(),
                operation: "greet".into(),
                resources: BTreeSet::from(["greeting".into()]),
            })
            .collect(),
        authority: Authority {
            operations: BTreeSet::from([OP.into()]),
            resources: BTreeSet::from(["greeting".into()]),
        },
        excluded: BTreeSet::new(),
        substitutions: vec![],
        resource: None,
    };
    let security = PortSecurity {
        confidentiality: Classification::Public,
        integrity: IntegrityClass::Untrusted,
    };
    let mut trusted = TrustedCompositionPolicy::default();
    trusted.source_owners.insert(
        OWNER.into(),
        SourceOwnerPolicy {
            owner_ceiling: Classification::Public,
            composition_ceiling: Classification::Public,
        },
    );
    trusted.sources.insert(format!("{OWNER}/source"), security);
    trusted.operations.insert(
        (OWNER.into(), OP.into()),
        OperationSecurity {
            input: security,
            ok: security,
            err: security,
        },
    );

    // Two live kernels from the same native image must not share Python objects.
    let first = Prepared::prepare_graph_v2(composition.clone(), &trusted)
        .unwrap()
        .load()
        .unwrap();
    let second = Prepared::prepare_graph_v2(composition.clone(), &trusted)
        .unwrap()
        .load()
        .unwrap();
    assert_eq!(
        first
            .admit_default_root()
            .unwrap()
            .invoke(&text("Ada"))
            .unwrap(),
        Outcome::Success(text("Hello Ada (1)"))
    );
    assert_eq!(
        first
            .admit_default_root()
            .unwrap()
            .invoke(&text("Ada"))
            .unwrap(),
        Outcome::Success(text("Hello Ada (2)"))
    );
    assert_eq!(
        second
            .admit_default_root()
            .unwrap()
            .invoke(&text("Ada"))
            .unwrap(),
        Outcome::Success(text("Hello Ada (1)"))
    );
    println!("PASS direct ABI-3 dispatcher: Hello Ada (1), Hello Ada (2); independent instance: Hello Ada (1)");
    let mut execution = first
        .begin_action_scoped(
            "example.python-greeter.submit",
            &text("Graph"),
            ScopedRequest {
                authority: composition.authority.clone(),
                resource: None,
            },
        )
        .unwrap();
    assert_eq!(execution.pump(|_| {}).unwrap(), GraphStatus::Finished);
    assert_eq!(execution.steps().len(), 1);
    assert_eq!(
        execution.steps()[0].outcome,
        Outcome::Success(text("Hello Graph (3)"))
    );
    drop(execution);
    println!("PASS V2 Source -> generated Invoke: Hello Graph (3)");
    assert_eq!(
        first
            .admit_default_root()
            .unwrap()
            .invoke(&text("malformed"))
            .unwrap_err()
            .code,
        Code::PluginFailed
    );
    assert_eq!(
        first
            .admit_default_root()
            .unwrap()
            .invoke(&Value::Bool(true))
            .unwrap_err()
            .code,
        Code::SchemaRejected
    );
    assert_eq!(
        second
            .admit_default_root()
            .unwrap()
            .invoke(&text("Still isolated"))
            .unwrap(),
        Outcome::Success(text("Hello Still isolated (2)"))
    );
    println!("PASS malformed Python result -> PluginFailed; malformed request -> SchemaRejected; state remains isolated");
    first.shutdown().unwrap();
    second.shutdown().unwrap();
}
