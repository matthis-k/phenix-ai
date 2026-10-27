//! Graph V2 + native Python language host: actual compiled V2 execution, ABI-3,
//! selected native provider dispatch, verified metadata and explicit failure gates.
use phenix_artifact::{digest, extract_graph_v2, VerifiedPackage};
use phenix_contract::{
    graph::v2::*, Authority, Code, ScopedRequest, Value, ValueSchema, NATIVE_SECTION_BYTES,
};
use phenix_kernel::{
    graph::{v2::{FlowGrant, OperationSecurity, SourceOwnerPolicy, TrustedCompositionPolicy},
            GraphCompletion, GraphStatus},
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Outcome,
};
use std::{collections::{BTreeMap, BTreeSet}, fs, path::{Path, PathBuf}, process::Command};

const PY: &str = "example.python-host";
const RS: &str = "example.provider";
const PY_NODE: &str = "example.python-host/query";
const RS_NODE: &str = "example.provider/uppercase";
const SOURCE: &str = "example.python-host/source";
const ACTION: &str = "python.v2-submit";
const RUN: &str = "example.consumer@1/run";
const TRANSFORM: &str = "example.transform.apply@1/apply";
const SECURITY: PortSecurity = PortSecurity {
    confidentiality: Classification::Public,
    integrity: IntegrityClass::Untrusted,
};

fn schema(package: &str, key: &str) -> OperationSchema {
    let pkg = VerifiedPackage::verify_trusted_read_only(Path::new(package)).unwrap();
    let (contract, operation) = key.rsplit_once('/').unwrap();
    pkg.metadata().definitions.iter()
        .find(|d| d.contract == contract).unwrap()
        .operations.iter().find(|o| o.name == operation).unwrap().clone()
}

fn invoke(id: &str, operation: &str, selected: &OperationSchema) -> NodeDecl {
    NodeDecl {
        id: id.into(),
        kind: NodeKind::Invoke { selector: String::new(), operation: operation.into() },
        inputs: vec![InputPort {
            name: "in".into(),
            schema: selected.request.clone(),
            readiness: InputReadiness::One,
            security: SECURITY,
        }],
        outputs: [
            OutputPort { name: "err".into(), schema: selected.error.clone(), security: SECURITY },
            OutputPort { name: "ok".into(), schema: selected.response.clone(), security: SECURITY },
        ].into(),
        execution: ExecutionPolicy { on_domain_error: DomainErrorPolicy::RouteErr },
    }
}

fn edge(id: &str, from: &str, port: &str, to: &str) -> EdgeDecl {
    EdgeDecl {
        id: format!("{PY}/{id}"),
        kind: EdgeKind::Flow {
            from: OutPortRef { node: from.into(), port: port.into() },
            to: InPortRef { node: to.into(), port: "in".into() },
        },
    }
}

fn graph_declarations(python_schema: &OperationSchema, rust_schema: &OperationSchema)
    -> [GraphContributionV2; 2]
{
    assert_eq!(python_schema.request, rust_schema.request);
    assert_eq!(python_schema.response, rust_schema.request);
    let source = SourceDecl {
        id: SOURCE.into(), owner: PY.into(),
        kind: SourceKind::ExternalAction { action: ACTION.into() },
        payload: python_schema.request.clone(), security: SECURITY,
    };
    [
        GraphContributionV2 {
            contributor: PY.into(),
            sources: vec![source],
            nodes: vec![
                NodeDecl {
                    id: SOURCE.into(),
                    kind: NodeKind::Source { source: SOURCE.into() },
                    inputs: vec![],
                    outputs: vec![OutputPort {
                        name: "out".into(), schema: python_schema.request.clone(), security: SECURITY,
                    }],
                    execution: ExecutionPolicy { on_domain_error: DomainErrorPolicy::RouteErr },
                },
                invoke(PY_NODE, RUN, python_schema),
            ],
            edges: vec![edge("start", SOURCE, "out", PY_NODE),
                        edge("python-to-rust", PY_NODE, "ok", RS_NODE)],
            patches: vec![],
        },
        GraphContributionV2 {
            contributor: RS.into(), sources: vec![],
            nodes: vec![invoke(RS_NODE, TRANSFORM, rust_schema)],
            edges: vec![], patches: vec![],
        },
    ]
}

fn repackage(original: &str, out: &Path, graph: &GraphContributionV2) -> String {
    fs::create_dir_all(out.join("lib")).unwrap();
    let source = Path::new(original);
    let bytes = fs::read(source.join("lib/plugin.so")).unwrap();
    assert!(extract_graph_v2(&bytes).unwrap().is_none(), "native fixture already contributed V2");
    let normalized = graph.normalized().unwrap();
    let encoded = normalized.encode().unwrap();
    assert!(encoded.len() + 4 <= NATIVE_SECTION_BYTES);
    let mut section = vec![0u8; NATIVE_SECTION_BYTES];
    section[..4].copy_from_slice(&(encoded.len() as u32).to_be_bytes());
    section[4..4 + encoded.len()].copy_from_slice(&encoded);
    let section_path = out.join("graph-section.bin");
    fs::write(&section_path, section).unwrap();
    let outfile = out.join("lib/plugin.so");
    let result = Command::new("objcopy")
        .arg("--add-section")
        .arg(format!(".phenix.graph={}", section_path.display()))
        .arg(source.join("lib/plugin.so"))
        .arg(&outfile)
        .output().unwrap();
    assert!(result.status.success(), "objcopy: {}", String::from_utf8_lossy(&result.stderr));
    let final_bytes = fs::read(&outfile).unwrap();
    assert_eq!(extract_graph_v2(&final_bytes).unwrap().unwrap(), normalized);
    fs::copy(source.join("descriptor.bin"), out.join("descriptor.bin")).unwrap();
    let mut record: serde_json::Value =
        serde_json::from_slice(&fs::read(source.join("package.json")).unwrap()).unwrap();
    record["artifact_sha256"] = digest(&final_bytes).into();
    fs::write(out.join("package.json"), serde_json::to_vec(&record).unwrap()).unwrap();
    VerifiedPackage::verify_trusted_read_only(out).unwrap();
    out.to_str().unwrap().into()
}

fn selection(key: &str, provider: &str) -> Selection {
    let (interface, operation) = key.rsplit_once('/').unwrap();
    Selection { interface: interface.into(), operation: operation.into(), provider: provider.into() }
}
fn grant(caller: &str, key: &str) -> ScopedGrant {
    let s = selection(key, "");
    ScopedGrant { caller: caller.into(), interface: s.interface, operation: s.operation,
        resources: BTreeSet::new() }
}
fn compose(packages: [String; 2]) -> Composition {
    Composition {
        format: "phenix.composition.v3".into(),
        definitions: vec![],
        artifacts: packages.into(),
        entry: selection(RUN, PY),
        slots: vec![SlotBinding {
            caller: PY.into(), slot: "Provider".into(), index: 0,
            provider: RS.into(), export: "Transform".into(),
        }],
        grants: vec![grant("root", RUN), grant(PY, RUN), grant(PY, TRANSFORM),
                     grant(RS, TRANSFORM)],
        authority: Authority {
            operations: [RUN.into(), TRANSFORM.into()].into(),
            resources: BTreeSet::new(),
        },
        excluded: BTreeSet::new(), substitutions: vec![], resource: None,
    }
}
fn trust() -> TrustedCompositionPolicy {
    let mut trusted = TrustedCompositionPolicy::default();
    trusted.topology.insert((PY.into(), RS_NODE.into()));
    trusted.source_owners.insert(PY.into(), SourceOwnerPolicy {
        owner_ceiling: Classification::Public,
        composition_ceiling: Classification::Public,
    });
    trusted.sources.insert(SOURCE.into(), SECURITY);
    for (owner, key) in [(PY, RUN), (RS, TRANSFORM)] {
        trusted.operations.insert((owner.into(), key.into()), OperationSecurity {
            input: SECURITY, ok: SECURITY, err: SECURITY,
        });
    }
    trusted.delivery.insert(
        (PY_NODE.into(), "ok".into(), RS_NODE.into(), "in".into()),
        FlowGrant { resources: BTreeSet::new(), maximum_classification: Classification::Public },
    );
    trusted
}
fn request(s: &str) -> Value {
    Value::map([("text", Value::String(s.into()))])
}
fn verify_negative(mut composition: Composition, trusted: &TrustedCompositionPolicy, code: Code) {
    let failure = Prepared::prepare_graph_v2(composition.clone(), trusted).err().unwrap();
    assert_eq!(failure.code, code, "{failure}");
    // Keep ownership local to this probe; don't leave modified composition around.
    composition.artifacts.clear();
}
fn main() {
    let argv: Vec<String> = std::env::args().collect();
    assert_eq!(argv.len(), 4, "RUNNER PYTHON-PACKAGE RUST-PACKAGE WORKDIR");
    let work = PathBuf::from(&argv[3]);
    fs::create_dir_all(&work).unwrap();
    let py_schema = schema(&argv[1], RUN);
    let rust_schema = schema(&argv[2], TRANSFORM);
    assert_eq!(py_schema.request, ValueSchema::Struct(BTreeMap::from([
        ("text".into(), ValueSchema::String),
    ])));
    let graphs = graph_declarations(&py_schema, &rust_schema);
    let packages = [
        repackage(&argv[1], &work.join("py-v2"), &graphs[0]),
        repackage(&argv[2], &work.join("rust-v2"), &graphs[1]),
    ];
    let base = compose(packages);
    let trusted = trust();

    // Offline failures: no native plugin may be executed during preparation.
    let mut wrong = base.clone();
    wrong.slots.clear();
    verify_negative(wrong, &trusted, Code::MissingBinding);
    let mut wrong = base.clone();
    wrong.slots[0].provider = PY.into();
    verify_negative(wrong, &trusted, Code::MissingBinding);
    let mut bad_policy = trusted.clone();
    bad_policy.topology.clear();
    verify_negative(base.clone(), &bad_policy, Code::Denied);
    let mut bad_policy = trusted.clone();
    bad_policy.delivery.clear();
    verify_negative(base.clone(), &bad_policy, Code::Denied);
    let mut bad_policy = trusted.clone();
    bad_policy.sources.clear();
    verify_negative(base.clone(), &bad_policy, Code::Denied);

    // Fail closed for an altered hosted Python graph schema (not attested
    // merely because the .so is valid). Compiler checks the real ABI contract.
    let mut spoof = graphs[0].clone();
    spoof.nodes.iter_mut().find(|n| n.id == PY_NODE).unwrap().inputs[0].schema =
        ValueSchema::String;
    let bad_path = repackage(&argv[1], &work.join("schema-spoof"), &spoof);
    let mut bad = base.clone();
    bad.artifacts[0] = bad_path;
    verify_negative(bad, &trusted, Code::SchemaRejected);

    let kernel = Prepared::prepare_graph_v2(base.clone(), &trusted).unwrap().load().unwrap();
    let scope = ScopedRequest { authority: base.authority.clone(), resource: None };

    for (input, python_out, rust_out) in [
        ("one two", "one,two", "ONE,TWO"),
        ("enrich:hello world", "HELLO WORLD", "HELLO WORLD"),
        ("enrich:again", "AGAIN", "AGAIN"),
        ("one", "one", "ONE"),
    ] {
        let mut execution = kernel.begin_action_scoped(ACTION, &request(input), scope.clone()).unwrap();
        assert_eq!(execution.pump(|_| {}).unwrap(), GraphStatus::Finished, "{input}");
        assert_eq!(execution.completion(), Some(GraphCompletion::Success), "{input}");
        assert_eq!(execution.steps().len(), 2, "{input}");
        assert_eq!(execution.steps()[0].node, PY_NODE);
        assert_eq!(execution.steps()[0].outcome, Outcome::Success(request(python_out)));
        assert_eq!(execution.steps()[1].node, RS_NODE);
        assert_eq!(execution.steps()[1].outcome, Outcome::Success(request(rust_out)));
        drop(execution);
    }

    for (input, code) in [
        ("private", Code::PluginFailed),
        ("raise", Code::PluginFailed),
        ("wrong-result", Code::PluginFailed),
        ("enrich:undeclared", Code::Denied),
        ("enrich:stale", Code::Denied),
        ("panic", Code::PluginFailed),
        ("enrich:panic", Code::PluginFailed),
    ] {
        let mut execution = kernel.begin_action_scoped(ACTION, &request(input), scope.clone()).unwrap();
        assert_eq!(execution.pump(|_| {}).unwrap_err().code, code, "{input}");
        assert!(matches!(execution.completion(), Some(GraphCompletion::Failure(f)) if f.code == code));
        drop(execution);
    }
    let mut execution = kernel.begin_action_scoped(ACTION, &request(""), scope.clone()).unwrap();
    assert_eq!(execution.pump(|_| {}).unwrap(), GraphStatus::Finished);
    assert_eq!(execution.completion(), Some(GraphCompletion::Domain));
    assert!(matches!(execution.steps()[0].outcome, Outcome::Domain(_)));
    assert_eq!(execution.steps().len(), 1, "domain failure must not invoke downstream Rust node");
    drop(execution);

    assert_eq!(
        kernel.begin_action_scoped(ACTION, &Value::Bool(true), scope.clone())
            .err().unwrap().code,
        Code::SchemaRejected,
    );
    drop(kernel);
    // Independent restricted composition: nested Python -> host import denied.
    let mut denied = base.clone();
    denied.grants.retain(|g| !(g.caller == PY && g.interface == "example.transform.apply@1"));
    match Prepared::prepare_graph_v2(denied, &trusted) {
        Err(f) => assert_eq!(f.code, Code::Denied),
        Ok(prepared) => {
            let kernel = prepared.load().unwrap();
            let mut execution = kernel.begin_action_scoped(ACTION, &request("enrich:hello"), scope).unwrap();
            assert_eq!(execution.pump(|_| {}).unwrap_err().code, Code::Denied);
            drop(execution);
            kernel.shutdown().unwrap();
        }
    }
    println!("PASS: V2 Source -> Python-hosted ABI-3 Invoke -> Rust ABI-3 Invoke; nested real Host::invoke, schema, binding, delivery, errors, authority and cleanup");
}
