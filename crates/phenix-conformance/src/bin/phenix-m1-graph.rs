//! G2 external artifact gate. All three graph providers are independently built.
use phenix_contract::{Authority, Code, Value};
use phenix_kernel::resolve::{Composition, Prepared, ScopedGrant, Selection};
use std::{collections::BTreeSet, fs, path::Path};

const TASK: &str = "example.graph.task@1";
const EVENT: &str = "example.graph.start";
const KEY: &str = "example.graph.task@1/run";

fn composition(artifacts: Vec<String>) -> Composition {
    let ids = ["example.graph-a", "example.graph-b", "example.graph-c"];
    let mut grants = Vec::new();
    for caller in ["root", ids[0], ids[1], ids[2]] {
        grants.push(ScopedGrant {
            caller: caller.into(),
            interface: TASK.into(),
            operation: "run".into(),
            resources: BTreeSet::new(),
        });
    }
    Composition {
        format: "phenix.composition.v2".into(),
        artifacts,
        definitions: vec![],
        entry: Selection {
            interface: TASK.into(),
            operation: "run".into(),
            provider: ids[0].into(),
        },
        slots: vec![],
        grants,
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: [KEY.into()].into(),
            resources: BTreeSet::new(),
        },
        resource: None,
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 5, "GRAPH_A GRAPH_B GRAPH_C OUTPUT");
    let output = Path::new(&args[4]);
    fs::create_dir_all(output).unwrap();
    let witness = output.join("constructor-witness");
    std::env::set_var("PHENIX_GRAPH_CONSTRUCTOR_WITNESS", &witness);
    let packages = args[1..4].to_vec();
    for (index, package) in packages.iter().enumerate() {
        let loaded =
            phenix_artifact::VerifiedPackage::verify_trusted_read_only(Path::new(package)).unwrap();
        let graph = loaded.graph().expect("compiled graph section");
        assert_eq!(
            graph.plugin,
            format!("example.graph-{}", ["a", "b", "c"][index])
        );
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.events.len(), if index == 0 { 1 } else { 0 });
    }
    let policy = composition(packages);
    let prepared = Prepared::prepare(policy.clone()).unwrap();
    assert!(
        !witness.exists(),
        "offline resolution ran native constructor"
    );
    let graph = prepared.compiled_graph().expect("verified graph");
    assert_eq!(graph.nodes.len(), 3);
    assert_eq!(
        graph.events[EVENT].payload,
        phenix_contract::ValueSchema::Struct(std::collections::BTreeMap::from([(
            "text".into(),
            phenix_contract::ValueSchema::String
        )]))
    );
    assert_eq!(
        graph.order,
        [
            "example.graph-a/a",
            "example.graph-b/b",
            "example.graph-c/c"
        ]
    );
    assert_eq!(graph.predecessors["example.graph-c/c"].len(), 2);
    assert_eq!(
        graph
            .activation_order(&BTreeSet::from([EVENT.into()]))
            .unwrap(),
        [
            "example.graph-a/a",
            "example.graph-b/b",
            "example.graph-c/c"
        ]
    );
    assert_eq!(prepared.discover_actions().len(), 1);
    assert_eq!(prepared.discover_actions()[0].id, "example.graph.submit");
    let digest = prepared.generation_identity("test.graph").unwrap();
    let mut reversed = policy.clone();
    reversed.artifacts.reverse();
    reversed.grants.reverse();
    let repeat = Prepared::prepare(reversed).unwrap();
    assert_eq!(repeat.generation_identity("test.graph").unwrap(), digest);
    assert_eq!(repeat.compiled_graph().unwrap().identity, graph.identity);
    assert!(!witness.exists(), "graph composition activated native code");

    let mut denied = policy.clone();
    denied
        .grants
        .retain(|grant| grant.caller != "example.graph-b");
    let err = Prepared::prepare(denied)
        .err()
        .expect("must deny ungranted graph node");
    assert_eq!(err.code, Code::Denied);
    assert!(!witness.exists(), "rejected graph activated native code");

    // An attacker who controls the package record can recompute its artifact
    // checksum. The graph decoder must still reject malformed graph bytes
    // without activating the library.
    let tampered = output.join("tampered-graph");
    fs::create_dir_all(tampered.join("lib")).unwrap();
    fs::copy(
        Path::new(&policy.artifacts[0]).join("descriptor.bin"),
        tampered.join("descriptor.bin"),
    )
    .unwrap();
    let mut executable = fs::read(Path::new(&policy.artifacts[0]).join("lib/plugin.so")).unwrap();
    let old = b"phenix.graph.v1";
    let at = executable
        .windows(old.len())
        .enumerate()
        .filter(|(_, bytes)| *bytes == old)
        .map(|(index, _)| index)
        .find(|index| {
            let mut candidate = executable.clone();
            candidate[*index + old.len() - 1] = b'0';
            phenix_artifact::extract_graph(&candidate).is_err()
        })
        .expect("embedded graph section format");
    executable[at + old.len() - 1] = b'0';
    fs::write(tampered.join("lib/plugin.so"), &executable).unwrap();
    let mut record: serde_json::Value = serde_json::from_slice(
        &fs::read(Path::new(&policy.artifacts[0]).join("package.json")).unwrap(),
    )
    .unwrap();
    record["artifact_sha256"] = serde_json::Value::String(phenix_artifact::digest(&executable));
    fs::write(
        tampered.join("package.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    assert_eq!(
        phenix_artifact::VerifiedPackage::verify_trusted_read_only(&tampered)
            .unwrap_err()
            .code,
        Code::DescriptorInvalid
    );
    assert!(!witness.exists(), "tampered graph activated native code");

    let graph_identity = graph.identity.clone();
    let graph_order = graph.order.clone();
    let action_ids = prepared
        .discover_actions()
        .iter()
        .map(|a| a.id.clone())
        .collect::<Vec<_>>();

    // Existing direct invocation is unchanged. G3 will replace frontend calls
    // with authenticated event admission and graph node dispatch.
    let kernel = prepared.load().unwrap();
    let response = kernel
        .admit_default_root()
        .unwrap()
        .invoke(&Value::map([("text", Value::String("proof".into()))]))
        .unwrap();
    assert_eq!(
        response,
        phenix_kernel::Outcome::Success(Value::String("proof".into()))
    );
    kernel.shutdown().unwrap();

    fs::write(
        output.join("graph.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
        "format": "phenix.g2-evidence.v1",
        "graph_identity": graph_identity,
        "generation_identity": digest.as_str(),
        "nodes": graph_order,
        "actions": action_ids,
        "cases": [
            "three-native-artifact-contributions",
            "pre-dlopen-graph-verification",
            "multi-root-join",
            "independent-order",
            "permutation-invariant-generation",
            "authorized-action-discovery",
            "denied-graph-operation-no-constructor",
            "re-signed-graph-tamper-rejected",
            "dormant-predecessor-semantics",
            "existing-direct-invocation"
        ]
        }))
        .unwrap(),
    )
    .unwrap();
    println!("G2 native graph preparation passed");
}
