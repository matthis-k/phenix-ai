include!("../../native_graph.rs");
use phenix_contract::{Authority, ScopedRequest, Value};
use phenix_kernel::{
    graph::{
        v2::{OperationSecurity, SourceOwnerPolicy, TrustedCompositionPolicy},
        GraphCompletion, GraphStatus,
    },
    resolve::{Composition, Prepared, ScopedGrant, Selection},
    Outcome,
};
use std::collections::BTreeSet;
fn main() {
    let mut args = std::env::args().skip(1);
    let mut package = args
        .next()
        .expect("native-host PACKAGE | --package LIB OUTPUT");
    if package == "--package" {
        let source = args.next().unwrap();
        let dest = std::path::PathBuf::from(args.next().unwrap());
        std::fs::create_dir_all(dest.join("lib")).unwrap();
        let bytes = std::fs::read(source).unwrap();
        let descriptor = phenix_artifact::extract(&bytes).unwrap();
        std::fs::write(dest.join("lib/plugin.so"), &bytes).unwrap();
        std::fs::write(dest.join("descriptor.bin"), &descriptor).unwrap();
        let record = serde_json::json!({"format":"phenix.native-package.v2","target":"x86_64-unknown-linux-gnu","artifact":"lib/plugin.so","descriptor":"descriptor.bin","artifact_sha256":phenix_artifact::digest(&bytes),"descriptor_sha256":phenix_artifact::digest(&descriptor)});
        std::fs::write(
            dest.join("package.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        package = dest.to_str().unwrap().into();
    }
    let keys = [
        dataflow_native_contracts::Prepare::run::KEY,
        dataflow_native_contracts::Service::run::KEY,
        dataflow_native_contracts::Complete::run::KEY,
    ];
    let select = |key: &str| {
        let (interface, operation) = key.rsplit_once('/').unwrap();
        Selection {
            interface: interface.into(),
            operation: operation.into(),
            provider: OWNER.into(),
        }
    };
    let c = Composition {
        format: "phenix.composition.v3".into(),
        definitions: vec![],
        artifacts: vec![package],
        entry: select(keys[0]),
        slots: vec![],
        grants: ["root", OWNER]
            .into_iter()
            .flat_map(|caller| {
                keys.iter().map(move |key| {
                    let (interface, operation) = key.rsplit_once('/').unwrap();
                    ScopedGrant {
                        caller: caller.into(),
                        interface: interface.into(),
                        operation: operation.into(),
                        resources: BTreeSet::from(["fixture".into()]),
                    }
                })
            })
            .collect(),
        authority: Authority {
            operations: keys.into_iter().map(Into::into).collect(),
            resources: BTreeSet::from(["fixture".into()]),
        },
        excluded: BTreeSet::new(),
        substitutions: vec![],
        resource: None,
    };
    let mut p = TrustedCompositionPolicy::default();
    p.source_owners.insert(
        OWNER.into(),
        SourceOwnerPolicy {
            owner_ceiling: Classification::Public,
            composition_ceiling: Classification::Public,
        },
    );
    p.sources.insert(format!("{OWNER}/submit"), SEC);
    for key in keys {
        p.operations.insert(
            (OWNER.into(), key.into()),
            OperationSecurity {
                input: SEC,
                ok: SEC,
                err: SEC,
            },
        );
    }
    let prepared = Prepared::prepare_graph_v2(c.clone(), &p).unwrap();
    let k = prepared.load().unwrap();
    let n = std::env::var("PHENIX_BENCH_N")
        .ok()
        .map(|s| s.parse::<u64>().unwrap())
        .unwrap_or(32);
    let start = std::time::Instant::now();
    for i in 1..=n {
        let m = dataflow_fixture::message(i, i % 2 == 0, false);
        let payload = Value::map([("text", Value::String(serde_json::to_string(&m).unwrap()))]);
        let mut e = k
            .begin_action_scoped(
                "experiment.submit",
                &payload,
                ScopedRequest {
                    authority: c.authority.clone(),
                    resource: None,
                },
            )
            .unwrap();
        assert_eq!(e.pump(|_| {}).unwrap(), GraphStatus::Finished);
        assert_eq!(e.completion(), Some(GraphCompletion::Success));
        let steps = e.steps();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].node, format!("{OWNER}/Prepare"));
        assert_eq!(
            steps[1].node,
            format!("{OWNER}/{}", if m.tool { "Tool" } else { "Model" })
        );
        if let Outcome::Success(Value::Map(fields)) = &steps[2].outcome {
            let Value::String(wire) = &fields["text"] else {
                panic!()
            };
            let (id, text): (dataflow_fixture::Identity, String) =
                serde_json::from_str(wire).unwrap();
            assert_eq!(id, m.id);
            assert_eq!(
                text,
                format!("{}:HELLO", if m.tool { "tool" } else { "model" })
            );
        } else {
            panic!("{:?}", steps[2].outcome)
        }
    }
    println!(
        "native_graph_v2 n={n} elapsed_ns={}",
        start.elapsed().as_nanos()
    );
    assert_eq!(graph().nodes.len(), 7);
}
