//! G2 external artifact gate. All three graph providers are independently built.
use phenix_contract::{Authority, Code, ScopedRequest, Value};
use phenix_kernel::graph::{GraphCompletion, GraphEvent, GraphStatus};
use phenix_kernel::resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding};
use std::{collections::BTreeSet, fs, path::Path};

const TASK: &str = "example.graph.task@1";
const EVENT: &str = "example.graph.start";
const KEY: &str = "example.graph.task@1/run";

#[path = "phenix-m1-graph/v2.rs"]
mod v2;

fn composition(artifacts: Vec<String>) -> Composition {
    let ids = ["example.graph-a", "example.graph-b", "example.graph-c"];
    let mut resources: BTreeSet<_> = ids[..2]
        .iter()
        .map(|id| {
            phenix_contract::property_contract(id, "result")
                .unwrap()
                .trim_end_matches("@1")
                .to_owned()
        })
        .collect();
    let signal = phenix_contract::property_contract("example.graph-a", "signal").unwrap();
    resources.insert(signal.trim_end_matches("@1").to_owned());
    let pending_signal =
        phenix_contract::property_contract("example.graph-a", "pending_signal").unwrap();
    resources.insert(pending_signal.trim_end_matches("@1").to_owned());
    let receipt = phenix_contract::property_contract("example.graph-c", "receipt").unwrap();
    resources.insert(receipt.trim_end_matches("@1").to_owned());
    let cancel_probe =
        phenix_contract::property_contract("example.graph-c", "cancel_probe").unwrap();
    resources.insert(cancel_probe.trim_end_matches("@1").to_owned());
    let mut grants = Vec::new();
    for caller in ["root", ids[0], ids[1], ids[2]] {
        grants.push(ScopedGrant {
            caller: caller.into(),
            interface: TASK.into(),
            operation: "run".into(),
            resources: resources.clone(),
        });
    }
    // The graph nodes use the existing state host API. A and B commit their
    // own properties; C reads those values through selected property imports.
    let mut operations = BTreeSet::from([
        KEY.into(),
        "example.graph.slow@1/run".into(),
        "example.graph.changed@1/run".into(),
        "example.graph.pending-change@1/run".into(),
    ]);
    grants.push(ScopedGrant {
        caller: "example.graph-c".into(),
        interface: "example.graph.changed@1".into(),
        operation: "run".into(),
        resources: resources.clone(),
    });
    grants.push(ScopedGrant {
        caller: "example.graph-c".into(),
        interface: "example.graph.pending-change@1".into(),
        operation: "run".into(),
        resources: resources.clone(),
    });
    for caller in ["root", "example.graph-a"] {
        grants.push(ScopedGrant {
            caller: caller.into(),
            interface: "example.graph.slow@1".into(),
            operation: "run".into(),
            resources: resources.clone(),
        });
    }
    for (owner, caller, actions) in [
        ("example.graph-a", "example.graph-a", &["get", "set"][..]),
        ("example.graph-b", "example.graph-b", &["get", "set"][..]),
        ("example.graph-a", "example.graph-c", &["get"][..]),
        ("example.graph-b", "example.graph-c", &["get"][..]),
    ] {
        let interface = phenix_contract::property_contract(owner, "result").unwrap();
        for operation in actions {
            grants.push(ScopedGrant {
                caller: caller.into(),
                interface: interface.clone(),
                operation: (*operation).into(),
                resources: resources.clone(),
            });
            operations.insert(format!("{interface}/{operation}"));
        }
    }
    for (caller, operation) in [
        ("example.graph-a", "get"),
        ("example.graph-a", "set"),
        ("example.graph-c", "watch"),
    ] {
        grants.push(ScopedGrant {
            caller: caller.into(),
            interface: signal.clone(),
            operation: operation.into(),
            resources: resources.clone(),
        });
        operations.insert(format!("{signal}/{operation}"));
    }
    for (caller, operation) in [
        ("example.graph-a", "get"),
        ("example.graph-a", "set"),
        ("example.graph-c", "watch"),
    ] {
        grants.push(ScopedGrant {
            caller: caller.into(),
            interface: pending_signal.clone(),
            operation: operation.into(),
            resources: resources.clone(),
        });
        operations.insert(format!("{pending_signal}/{operation}"));
    }
    for operation in ["get", "set", "watch"] {
        grants.push(ScopedGrant {
            caller: "example.graph-c".into(),
            interface: receipt.clone(),
            operation: operation.into(),
            resources: resources.clone(),
        });
        operations.insert(format!("{receipt}/{operation}"));
    }
    for operation in ["get", "set"] {
        grants.push(ScopedGrant {
            caller: "example.graph-c".into(),
            interface: cancel_probe.clone(),
            operation: operation.into(),
            resources: resources.clone(),
        });
        operations.insert(format!("{cancel_probe}/{operation}"));
    }
    Composition {
        format: "phenix.composition.v3".into(),
        artifacts,
        definitions: vec![],
        entry: Selection {
            interface: TASK.into(),
            operation: "run".into(),
            provider: ids[0].into(),
        },
        slots: vec![
            SlotBinding {
                caller: "example.graph-c".into(),
                slot: "a".into(),
                index: 0,
                provider: "example.graph-a".into(),
                export: "property.result".into(),
            },
            SlotBinding {
                caller: "example.graph-c".into(),
                slot: "b".into(),
                index: 0,
                provider: "example.graph-b".into(),
                export: "property.result".into(),
            },
            SlotBinding {
                caller: "example.graph-c".into(),
                slot: "signal".into(),
                index: 0,
                provider: "example.graph-a".into(),
                export: "property.signal".into(),
            },
            SlotBinding {
                caller: "example.graph-c".into(),
                slot: "pending_signal".into(),
                index: 0,
                provider: "example.graph-a".into(),
                export: "property.pending_signal".into(),
            },
        ],
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

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 6, "GRAPH_A GRAPH_B GRAPH_C DEFINITIONS OUTPUT");
    let output = Path::new(&args[5]);
    fs::create_dir_all(output).unwrap();
    let witness = output.join("constructor-witness");
    std::env::set_var("PHENIX_GRAPH_CONSTRUCTOR_WITNESS", &witness);
    let successor_witness = output.join("graph-c-invoked");
    std::env::set_var("PHENIX_GRAPH_C_WITNESS", &successor_witness);
    let hook_witness = output.join("graph-c-state-hook");
    std::env::set_var("PHENIX_GRAPH_HOOK_WITNESS", &hook_witness);
    let receipt_witness = output.join("graph-c-receipt-hook");
    std::env::set_var("PHENIX_GRAPH_RECEIPT_WITNESS", &receipt_witness);
    let packages = args[1..4].to_vec();
    for (index, package) in packages.iter().enumerate() {
        let loaded =
            phenix_artifact::VerifiedPackage::verify_trusted_read_only(Path::new(package)).unwrap();
        let graph = loaded.graph().expect("compiled graph section");
        assert_eq!(
            graph.plugin,
            format!("example.graph-{}", ["a", "b", "c"][index])
        );
        assert_eq!(
            graph.nodes.len(),
            if index == 1 {
                1
            } else if index == 2 {
                5
            } else {
                2
            }
        );
        assert!(graph.events.is_empty());
    }
    let mut policy = composition(packages.clone());
    policy.definitions.push(args[4].clone());
    let prepared = Prepared::prepare(policy.clone()).unwrap();
    assert!(
        !witness.exists(),
        "offline resolution ran native constructor"
    );
    let graph = prepared.compiled_graph().expect("verified graph");
    assert_eq!(graph.nodes.len(), 8);
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
            "example.graph-a/slow",
            "example.graph-b/b",
            "example.graph-c/c",
            "example.graph-c/d",
            "example.graph-c/pending",
            "example.graph-c/watch",
            "example.graph-c/receipt"
        ]
    );
    assert_eq!(graph.predecessors["example.graph-c/c"].len(), 3);
    assert_eq!(
        graph
            .activation_order(&BTreeSet::from([EVENT.into()]))
            .unwrap(),
        [
            "example.graph-a/a",
            "example.graph-b/b",
            "example.graph-c/c",
            "example.graph-c/d"
        ]
    );
    assert_eq!(prepared.discover_actions().len(), 3);
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

    let mut executable = fs::read(Path::new(&policy.artifacts[0]).join("lib/plugin.so")).unwrap();
    let original = phenix_artifact::extract_graph(&executable)
        .unwrap()
        .unwrap();
    let original_bytes = original.encode().unwrap();
    let mut forged = original;
    forged.plugin = "example.graph-z".into();
    let forged_bytes = forged.encode().unwrap();
    assert_eq!(original_bytes.len(), forged_bytes.len());
    let at = executable
        .windows(original_bytes.len())
        .position(|bytes| bytes == original_bytes)
        .unwrap();
    executable[at..at + forged_bytes.len()].copy_from_slice(&forged_bytes);
    fs::write(tampered.join("lib/plugin.so"), &executable).unwrap();
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
    assert!(
        !witness.exists(),
        "re-signed owner mismatch activated native code"
    );

    // These declarations came from three independently packaged native plugins.
    // Mutating the requested event schema or a declared write must reject
    // during preparation, before any code from a selected plugin is loaded.
    let mut declarations = packages
        .iter()
        .map(|package| {
            phenix_artifact::VerifiedPackage::verify_trusted_read_only(Path::new(package))
                .unwrap()
                .graph()
                .unwrap()
                .clone()
        })
        .collect::<Vec<_>>();
    let definitions =
        phenix_artifact::VerifiedContractPackage::verify_trusted_read_only(Path::new(&args[4]))
            .unwrap();
    let definition_graph = definitions.graph().expect("definition-only graph").clone();
    declarations.push(definition_graph.clone());
    let bad_definition = output.join("bad-definition");
    fs::create_dir_all(&bad_definition).unwrap();
    fs::copy(
        Path::new(&args[4]).join("descriptor.bin"),
        bad_definition.join("descriptor.bin"),
    )
    .unwrap();
    let mut record: serde_json::Value =
        serde_json::from_slice(&fs::read(Path::new(&args[4]).join("package.json")).unwrap())
            .unwrap();
    for case in 0..3 {
        let mut bad = definition_graph.clone();
        if case == 1 {
            bad.plugin = "wrong.owner".into();
        }
        if case == 2 {
            bad.nodes = declarations[0].nodes.clone();
        }
        let bytes = bad.encode().unwrap();
        fs::write(bad_definition.join("graph.bin"), &bytes).unwrap();
        record["graph"]["sha256"] = serde_json::Value::String(if case == 0 {
            "0".repeat(64)
        } else {
            phenix_artifact::digest(&bytes)
        });
        fs::write(
            bad_definition.join("package.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert_eq!(
            phenix_artifact::VerifiedContractPackage::verify_trusted_read_only(&bad_definition)
                .unwrap_err()
                .code,
            Code::DescriptorInvalid
        );
        assert!(
            !witness.exists(),
            "rejected definition graph activated native code"
        );
    }
    let mut bad_schema = declarations.clone();
    bad_schema[3]
        .events
        .iter_mut()
        .find(|event| event.id == EVENT)
        .unwrap()
        .payload = phenix_contract::ValueSchema::String;
    assert_eq!(
        prepared.compile_graph(&bad_schema).unwrap_err().code,
        Code::SchemaRejected
    );
    assert!(!witness.exists(), "rejected schema activated native code");

    let mut missing_property = declarations.clone();
    missing_property[0].nodes[0]
        .writes
        .push("example.graph-a/missing".into());
    assert_eq!(
        prepared.compile_graph(&missing_property).unwrap_err().code,
        Code::MissingBinding
    );
    assert!(!witness.exists(), "rejected property activated native code");

    let mut denied_write = declarations.clone();
    denied_write[2].nodes[0].writes = vec!["example.graph-a/result".into()];
    assert_eq!(
        prepared.compile_graph(&denied_write).unwrap_err().code,
        Code::Denied
    );
    assert!(!witness.exists(), "ungranted writer activated native code");

    let graph_identity = graph.identity.clone();
    let graph_order = graph.order.clone();
    let action_ids = prepared
        .discover_actions()
        .iter()
        .map(|a| a.id.clone())
        .collect::<Vec<_>>();

    // Observe results during execution, without invoking a handler from the host.
    let kernel = prepared.load().unwrap();
    let scope = ScopedRequest {
        authority: policy.authority.clone(),
        resource: None,
    };
    let mut trace = Vec::new();
    let submitted = kernel
        .submit_action_observed(
            "example.graph.submit",
            &Value::map([("text", Value::String("triggered".into()))]),
            scope.clone(),
            |update| {
                let label = match &update.event {
                    GraphEvent::Admitted {
                        action,
                        event,
                        graph,
                        resource,
                    } => {
                        assert_eq!(*action, "example.graph.submit");
                        assert_eq!(*event, EVENT);
                        assert_eq!(*graph, graph_identity);
                        assert!(resource.is_none());
                        assert_eq!(
                            kernel.admit_default_root().err().unwrap().code,
                            Code::ReentrancyRejected,
                            "observer must retain the admitted root"
                        );
                        "admitted".to_owned()
                    }
                    GraphEvent::NodeFinished(step) => {
                        if step.node != "example.graph-c/c" && step.node != "example.graph-c/d" {
                            assert!(
                                !successor_witness.exists(),
                                "result delivered after successor ran"
                            );
                        }
                        step.node.clone()
                    }
                    GraphEvent::Finished(completion) => {
                        assert_eq!(*completion, GraphCompletion::Success);
                        assert!(successor_witness.exists());
                        "success".to_owned()
                    }
                    GraphEvent::NodeSkipped { .. } | GraphEvent::NodePending { .. } => {
                        panic!("synchronous action skipped or suspended a node")
                    }
                };
                trace.push((update.root, label));
            },
        )
        .unwrap();
    assert_eq!(trace.len(), 6);
    assert!(trace.iter().all(|(root, _)| *root == trace[0].0));
    assert_eq!(trace[0].1, "admitted");
    assert_eq!(trace[5].1, "success");
    assert_eq!(submitted.len(), 4);
    for (index, name) in [
        "example.graph-a/a",
        "example.graph-b/b",
        "example.graph-c/c",
        "example.graph-c/d",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(submitted[index].node, *name);
        let value = if index >= 2 {
            "A:triggered|B:triggered"
        } else {
            "triggered"
        };
        assert_eq!(
            submitted[index].outcome,
            phenix_kernel::Outcome::Success(Value::String(value.into()))
        );
    }
    assert_eq!(
        fs::read_to_string(&successor_witness).unwrap(),
        "A:triggered|B:triggered",
        "C must read both committed predecessor values"
    );
    fs::remove_file(&successor_witness).unwrap();
    let mut skipped = Vec::new();
    let mut domain_root = None;
    let mut domain_terminal = false;
    let domain = kernel
        .submit_action_observed(
            "example.graph.submit",
            &Value::map([("text", Value::String("domain".into()))]),
            scope.clone(),
            |update| {
                assert_ne!(update.root, trace[0].0, "actions must have distinct roots");
                assert_eq!(*domain_root.get_or_insert(update.root), update.root);
                match &update.event {
                    GraphEvent::NodeSkipped { node } => skipped.push((*node).to_owned()),
                    GraphEvent::Finished(completion) => {
                        assert_eq!(*completion, GraphCompletion::Domain);
                        domain_terminal = true;
                    }
                    _ => {}
                }
            },
        )
        .unwrap();
    assert_eq!(skipped, ["example.graph-c/c", "example.graph-c/d"]);
    assert!(domain_terminal);
    assert_eq!(domain.len(), 2);
    assert_eq!(domain[0].node, "example.graph-a/a");
    assert_eq!(
        domain[0].outcome,
        phenix_kernel::Outcome::Domain(Value::String("domain fixture".into()))
    );
    assert_eq!(domain[1].node, "example.graph-b/b");
    assert_eq!(
        domain[1].outcome,
        phenix_kernel::Outcome::Success(Value::String("domain".into()))
    );
    assert!(
        !successor_witness.exists(),
        "A skip must suppress C and its descendant while B succeeds"
    );
    let mut fail_root_terminal = false;
    let later_failure = kernel
        .submit_action_observed(
            "example.graph.submit",
            &Value::map([("text", Value::String("domain-b".into()))]),
            scope.clone(),
            |update| {
                if let GraphEvent::Finished(completion) = &update.event {
                    assert_eq!(*completion, GraphCompletion::Domain);
                    fail_root_terminal = true;
                }
            },
        )
        .unwrap();
    assert!(fail_root_terminal);
    assert_eq!(later_failure.len(), 2, "B failure must stop downstream C");
    assert_eq!(later_failure[0].node, "example.graph-a/a");
    assert_eq!(
        later_failure[0].outcome,
        phenix_kernel::Outcome::Success(Value::String("domain-b".into()))
    );
    assert_eq!(later_failure[1].node, "example.graph-b/b");
    assert_eq!(
        later_failure[1].outcome,
        phenix_kernel::Outcome::Domain(Value::String("domain fixture b".into()))
    );
    assert!(
        !successor_witness.exists(),
        "B failure must have zero C invocations"
    );
    // Skip handles domain results only. Resource denial must stop the action
    // before A commits anything, without invoking B or either successor.
    let calls_before = kernel.inspect()["calls"].as_u64().unwrap();
    let partial = kernel
        .submit_action(
            "example.graph.submit-partial",
            &Value::map([("text", Value::String("partial".into()))]),
        )
        .unwrap();
    assert_eq!(partial.len(), 2);
    assert_eq!(partial[0].node, "example.graph-a/a");
    assert_eq!(partial[1].node, "example.graph-c/c");
    assert_eq!(
        partial[1].outcome,
        phenix_kernel::Outcome::Success(Value::String("A:partial|B:domain".into()))
    );
    assert_eq!(
        kernel.inspect()["calls"].as_u64().unwrap(),
        calls_before + 2
    );
    fs::remove_file(&successor_witness).unwrap();
    let before = kernel.inspect()["live_state"].clone();
    let mut restricted = scope.clone();
    restricted
        .authority
        .resources
        .remove("example.graph-a.property.result");
    let mut failed_terminal = false;
    let mut denied_updates = 0;
    assert_eq!(
        kernel
            .submit_action_observed(
                "example.graph.submit",
                &Value::map([("text", Value::String("denied-resource".into()))]),
                restricted,
                |update| {
                    denied_updates += 1;
                    match &update.event {
                        GraphEvent::Admitted { .. } => {}
                        GraphEvent::Finished(GraphCompletion::Failure(failure)) => {
                            assert_eq!(failure.code, Code::PluginFailed);
                            failed_terminal = true;
                        }
                        _ => panic!("resource denial must stop all remaining nodes"),
                    }
                },
            )
            .unwrap_err()
            .code,
        Code::PluginFailed
    );
    assert!(failed_terminal);
    assert_eq!(denied_updates, 2);
    assert_eq!(
        kernel.inspect()["live_state"],
        before,
        "denied scope committed state"
    );
    assert!(!successor_witness.exists());
    let calls_before = kernel.inspect()["calls"].as_u64().unwrap();
    let mut rejected_updates = 0;
    let mut invalid_resource = scope.clone();
    invalid_resource.resource = Some("ungranted.resource".into());
    assert_eq!(
        kernel
            .submit_action_observed(
                "example.graph.submit",
                &Value::map([("text", Value::String("invalid-resource".into()))]),
                invalid_resource,
                |_| rejected_updates += 1,
            )
            .unwrap_err()
            .code,
        Code::Denied
    );
    assert_eq!(rejected_updates, 0);
    assert_eq!(kernel.inspect()["calls"].as_u64().unwrap(), calls_before);
    // A discovered action without the submitting root's authority must not
    // admit a root, invoke A, or write either predecessor's state.
    let calls_before = kernel.inspect()["calls"].as_u64().unwrap();
    assert_eq!(
        kernel
            .submit_action_scoped(
                "example.graph.submit",
                &Value::map([("text", Value::String("denied-scope".into()))]),
                ScopedRequest {
                    authority: Authority::default(),
                    resource: None,
                },
            )
            .unwrap_err()
            .code,
        Code::Denied
    );
    assert_eq!(kernel.inspect()["calls"].as_u64().unwrap(), calls_before);
    assert!(!successor_witness.exists());
    assert_eq!(
        kernel
            .submit_action("example.graph.unknown", &Value::String("x".into()))
            .unwrap_err()
            .code,
        Code::MissingBinding
    );
    assert_eq!(
        kernel
            .submit_action("example.graph.submit", &Value::String("x".into()))
            .unwrap_err()
            .code,
        Code::SchemaRejected
    );
    let response = kernel
        .admit_default_root()
        .unwrap()
        .invoke(&Value::map([("text", Value::String("proof".into()))]))
        .unwrap();
    assert_eq!(
        response,
        phenix_kernel::Outcome::Success(Value::String("proof".into()))
    );
    let mut resource_scope = scope.clone();
    resource_scope.resource = Some("example.graph-a.property.result".into());
    let mut observed_resource = false;
    let contextual = kernel
        .submit_action_observed(
            "example.graph.submit",
            &Value::map([("text", Value::String("resource".into()))]),
            resource_scope,
            |update| {
                if let GraphEvent::Admitted { resource, .. } = &update.event {
                    assert_eq!(*resource, Some("example.graph-a.property.result"));
                    observed_resource = true;
                }
            },
        )
        .unwrap();
    assert!(observed_resource);
    assert_eq!(contextual.len(), 4);
    fs::remove_file(&successor_witness).unwrap();
    {
        let descriptors_before = fs::read_dir("/proc/self/fd").unwrap().count();
        let mut execution = kernel
            .begin_action_scoped(
                "example.graph.submit-slow",
                &Value::map([("text", Value::String("slow".into()))]),
                scope.clone(),
            )
            .unwrap();
        let (before_epoch, _) = execution.committed_changes_since(0);
        let mut pending_root = None;
        assert_eq!(
            execution
                .pump(|update| {
                    assert_eq!(*pending_root.get_or_insert(update.root), update.root);
                })
                .unwrap(),
            GraphStatus::Pending
        );
        assert_eq!(execution.steps().len(), 1);
        assert_eq!(execution.steps()[0].node, "example.graph-b/b");
        // B commits before the pending A branch settles. Observe the
        // committed epoch without admitting or reentering any new node.
        let (after_b_epoch, after_b) = execution.committed_changes_since(before_epoch);
        assert_eq!(after_b, ["example.graph-b/result"]);
        assert!(
            !successor_witness.exists(),
            "join ran before slow source settled"
        );
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
        assert_eq!(
            fs::read_dir("/proc/self/fd").unwrap().count(),
            descriptors_before + 1
        );
        assert_eq!(execution.pump(|_| {}).unwrap(), GraphStatus::Pending);
        assert_eq!(execution.pump(|_| {}).unwrap(), GraphStatus::Pending);
        assert!(!successor_witness.exists());
        assert_eq!(execution.pump(|_| {}).unwrap(), GraphStatus::Finished);
        let (after_a_epoch, after_a) = execution.committed_changes_since(after_b_epoch);
        assert!(after_a_epoch > after_b_epoch);
        assert_eq!(after_a, ["example.graph-a/result"]);
        assert!(execution
            .committed_changes_since(after_a_epoch)
            .1
            .is_empty());
        assert_eq!(execution.completion(), Some(GraphCompletion::Success));
        assert_eq!(execution.steps().len(), 4);
        assert_eq!(execution.steps()[1].node, "example.graph-a/slow");
        assert_eq!(
            execution.steps()[2].outcome,
            phenix_kernel::Outcome::Success(Value::String("A:slow|B:slow".into()))
        );
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
        assert_eq!(
            fs::read_dir("/proc/self/fd").unwrap().count(),
            descriptors_before
        );
    }
    fs::remove_file(&successor_witness).unwrap();
    for input in ["slow-cancel", "slow-drain"] {
        let descriptors_before = fs::read_dir("/proc/self/fd").unwrap().count();
        let mut execution = kernel
            .begin_action_scoped(
                "example.graph.submit-slow",
                &Value::map([("text", Value::String(input.into()))]),
                scope.clone(),
            )
            .unwrap();
        assert_eq!(execution.pump(|_| {}).unwrap(), GraphStatus::Pending);
        let committed_before_cancel = kernel.inspect()["live_state"].clone();
        execution.cancel().unwrap();
        let mut terminal_updates = 0;
        let first = execution
            .pump(|update| {
                if let GraphEvent::Finished(completion) = &update.event {
                    assert_eq!(*completion, GraphCompletion::Cancelled);
                    terminal_updates += 1;
                }
            })
            .unwrap();
        if input == "slow-drain" {
            assert_eq!(first, GraphStatus::Draining);
            assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
            assert_eq!(
                fs::read_dir("/proc/self/fd").unwrap().count(),
                descriptors_before + 1
            );
            assert!(
                kernel.shutdown().is_err(),
                "physical worker lease allowed shutdown"
            );
            assert_eq!(execution.pump(|_| {}).unwrap(), GraphStatus::Draining);
            assert_eq!(execution.pump(|_| {}).unwrap(), GraphStatus::Finished);
        } else {
            assert_eq!(first, GraphStatus::Finished);
        }
        assert_eq!(terminal_updates, 1);
        assert_eq!(
            execution
                .pump(|_| panic!("terminal update repeated"))
                .unwrap(),
            GraphStatus::Finished
        );
        assert_eq!(execution.completion(), Some(GraphCompletion::Cancelled));
        assert_eq!(
            execution.steps().len(),
            1,
            "cancelled source or successors ran"
        );
        assert_eq!(kernel.inspect()["live_state"], committed_before_cancel);
        assert!(
            !successor_witness.exists(),
            "cancelled graph invoked successor"
        );
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
        assert_eq!(
            fs::read_dir("/proc/self/fd").unwrap().count(),
            descriptors_before
        );
    }
    for input in ["domain-b", "kernel-b"] {
        let mut execution = kernel
            .begin_action_scoped(
                "example.graph.submit-slow",
                &Value::map([("text", Value::String(input.into()))]),
                scope.clone(),
            )
            .unwrap();
        assert_eq!(execution.pump(|_| {}).unwrap(), GraphStatus::Finished);
        if input == "domain-b" {
            assert_eq!(execution.completion(), Some(GraphCompletion::Domain));
        } else {
            assert!(
                matches!(execution.completion(), Some(GraphCompletion::Failure(failure)) if failure.code == Code::PluginFailed)
            );
        }
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
        assert!(
            !successor_witness.exists(),
            "failed branch allowed successor"
        );
    }
    assert!(!hook_witness.exists());
    assert!(!receipt_witness.exists());
    let mut hook_root = None;
    let mut hook_terminal = 0;
    let changed = kernel
        .submit_action_observed(
            "example.graph.submit",
            &Value::map([("text", Value::String("g4-hook".into()))]),
            scope.clone(),
            |update| {
                assert_eq!(*hook_root.get_or_insert(update.root), update.root);
                if let GraphEvent::Finished(GraphCompletion::Success) = &update.event {
                    hook_terminal += 1;
                }
            },
        )
        .unwrap();
    assert_eq!(
        hook_terminal, 1,
        "reactive waves emitted multiple terminal events"
    );
    assert_eq!(changed.len(), 6);
    assert_eq!(changed[4].node, "example.graph-c/watch");
    assert_eq!(changed[5].node, "example.graph-c/receipt");
    let hook_text = fs::read_to_string(&hook_witness).unwrap();
    let (hook_epoch, hook_property) = hook_text.split_once(':').unwrap();
    assert!(hook_epoch.parse::<u64>().unwrap() > 0);
    assert_eq!(hook_property, "example.graph-a/signal");
    let receipt_text = fs::read_to_string(&receipt_witness).unwrap();
    let (receipt_epoch, receipt_property) = receipt_text.split_once(':').unwrap();
    assert!(receipt_epoch.parse::<u64>().unwrap() > hook_epoch.parse::<u64>().unwrap());
    assert_eq!(receipt_property, "example.graph-c/receipt");
    fs::remove_file(&hook_witness).unwrap();
    fs::remove_file(&receipt_witness).unwrap();

    // The synchronous helper cannot be cancelled from another thread. An
    // intentionally self-triggering hook must fail with a bounded typed error,
    // release the original root, and leave later action admission possible.
    std::env::set_var("PHENIX_GRAPH_SELF_TRIGGER", "1");
    let cycle = kernel
        .submit_action(
            "example.graph.submit",
            &Value::map([("text", Value::String("g4-cycle".into()))]),
        )
        .unwrap_err();
    assert_eq!(cycle.code, Code::InfrastructureFailure);
    assert!(cycle.message.contains("reactive wave limit"));
    std::env::remove_var("PHENIX_GRAPH_SELF_TRIGGER");
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
    let recovered = kernel
        .submit_action(
            "example.graph.submit",
            &Value::map([("text", Value::String("g4-recovered".into()))]),
        )
        .unwrap();
    assert_eq!(recovered.len(), 4);

    // Start a true pending state hook after the action wave has committed.
    // Both cancellation paths must retain the original root and revoke
    // writes before the worker's physical file lease is released.
    for (input, drain) in [("g4-pending-hook", false), ("g4-pending-drain", true)] {
        let descriptors_before = fs::read_dir("/proc/self/fd").unwrap().count();
        let mut execution = kernel
            .begin_action_scoped(
                "example.graph.submit",
                &Value::map([("text", Value::String(input.into()))]),
                scope.clone(),
            )
            .unwrap();
        let mut correlated = None;
        assert_eq!(
            execution
                .pump(|update| {
                    assert_eq!(*correlated.get_or_insert(update.root), update.root);
                })
                .unwrap(),
            GraphStatus::Pending
        );
        assert_eq!(execution.steps().len(), 4);
        assert_eq!(execution.pump(|_| {}).unwrap(), GraphStatus::Pending);
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
        assert_eq!(
            fs::read_dir("/proc/self/fd").unwrap().count(),
            descriptors_before + 1
        );
        let before_cancel = kernel.inspect()["live_state"].clone();
        if drain {
            std::env::set_var("PHENIX_GRAPH_PENDING_DRAIN", "1");
        }
        execution.cancel().unwrap();
        let mut terminals = 0;
        let first = execution
            .pump(|update| {
                if let GraphEvent::Finished(GraphCompletion::Cancelled) = &update.event {
                    terminals += 1;
                }
            })
            .unwrap();
        if drain {
            assert_eq!(first, GraphStatus::Draining);
            assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
            assert!(kernel.shutdown().is_err());
            assert_eq!(execution.pump(|_| {}).unwrap(), GraphStatus::Draining);
            assert_eq!(execution.pump(|_| {}).unwrap(), GraphStatus::Finished);
            std::env::remove_var("PHENIX_GRAPH_PENDING_DRAIN");
        } else {
            assert_eq!(first, GraphStatus::Finished);
        }
        assert_eq!(terminals, 1);
        assert_eq!(
            execution
                .pump(|_| panic!("pending hook terminal emitted twice"))
                .unwrap(),
            GraphStatus::Finished
        );
        assert_eq!(execution.completion(), Some(GraphCompletion::Cancelled));
        assert_eq!(execution.steps().len(), 4);
        assert_eq!(kernel.inspect()["live_state"], before_cancel);
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
        assert_eq!(
            fs::read_dir("/proc/self/fd").unwrap().count(),
            descriptors_before
        );
    }

    // A selected watch permission is necessary even for an offline hook.
    let mut denied_watch = policy.clone();
    denied_watch.grants.retain(|grant| {
        !(grant.caller == "example.graph-c"
            && grant.interface
                == phenix_contract::property_contract("example.graph-a", "signal").unwrap()
            && grant.operation == "watch")
    });
    assert_eq!(
        Prepared::prepare(denied_watch)
            .err()
            .expect("state hooks require watch grants")
            .code,
        Code::Denied
    );

    // A pending state hook must pass the same offline watch-grant gate.
    let mut denied_pending_watch = policy.clone();
    denied_pending_watch.grants.retain(|grant| {
        !(grant.caller == "example.graph-c"
            && grant.interface
                == phenix_contract::property_contract("example.graph-a", "pending_signal").unwrap()
            && grant.operation == "watch")
    });
    assert_eq!(
        Prepared::prepare(denied_pending_watch)
            .err()
            .expect("pending state hooks require watch grants")
            .code,
        Code::Denied
    );

    kernel.shutdown().unwrap();

    // The graph caller must inherit the root requirement's resource ceiling.
    // Giving node owners broader resources must not restore root permissions.
    let mut restricted_policy = policy.clone();
    restricted_policy
        .grants
        .iter_mut()
        .filter(|grant| grant.caller == "root")
        .for_each(|grant| grant.resources.clear());
    let restricted_kernel = Prepared::prepare(restricted_policy)
        .unwrap()
        .load()
        .unwrap();
    let before = restricted_kernel.inspect()["live_state"].clone();
    assert_eq!(
        restricted_kernel
            .submit_action(
                "example.graph.submit",
                &Value::map([("text", Value::String("root-resource-ceiling".into()))]),
            )
            .unwrap_err()
            .code,
        Code::PluginFailed
    );
    assert_eq!(restricted_kernel.inspect()["live_state"], before);
    assert_eq!(restricted_kernel.inspect()["calls"], 1);
    restricted_kernel.shutdown().unwrap();

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
            "definition-only-event-action-contributor",
            "definition-graph-digest-owner-and-executable-node-rejection",
            "pre-dlopen-graph-verification",
            "multi-root-join",
            "independent-order",
            "permutation-invariant-generation",
            "authorized-action-discovery",
            "denied-graph-operation-no-constructor",
            "re-signed-graph-tamper-rejected",
            "re-signed-native-graph-owner-rejected",
            "dormant-predecessor-semantics",
            "native-dormant-b-does-not-block-c-and-after-does-not-trigger-d",
            "existing-direct-invocation",
            "serial-action-admission",
            "four-external-nodes-dispatched",
            "schema-and-action-rejection",
            "successful-fanin-runs-c",
            "two-committed-property-values-read-by-successor",
            "narrow-action-scope-zero-effects",
            "predecessor-b-domain-prevents-c-effect",
            "denied-property-writer-before-activation",
            "skip-domain-continues-independent-node",
            "skip-suppresses-transitive-successors",
            "correlated-host-results-before-successor-effects",
            "observer-retains-root-and-terminal-outcomes",
            "resource-denial-zero-commits-and-no-successor",
            "ungranted-event-resource-zero-invocations",
            "root-resource-ceiling-bounds-graph-delegation",
            "action-resource-inherited-by-four-native-nodes",
            "pending-a-independent-b-and-delayed-c",
            "native-state-hook-same-root-after-commit",
            "native-state-hook-one-terminal-event",
            "native-state-hook-committed-second-epoch",
            "native-state-hook-nonrecursive-receipt",
            "native-state-hook-bounded-sync-self-cycle",
            "native-state-hook-recovers-fresh-root-after-cycle",
            "native-pending-hook-cancel-no-late-effects",
            "native-pending-hook-drain-holds-physical-lease",
            "native-pending-hook-cancel-revokes-state-authority",
            "native-pending-hook-one-terminal-event",
            "native-state-hook-grant-denied-before-load",
            "native-pending-hook-grant-denied-before-load",
            "native-state-hook-receives-only-matching-property",
            "watched-b-commit-projected-before-pending-a-settles",
            "watched-a-commit-projected-after-pending-completes",
            "watched-change-cursor-does-not-replay-commits",
            "same-root-cancellation-before-successor-effects",
            "retained-worker-lease-draining-and-shutdown-rejection",
            "cancelled-cleanup-state-write-rejected",
            "one-terminal-update-and-final-ticket-release"
            ,"failed-independent-branch-cancels-pending-source"
            ,"real-worker-file-descriptor-held-until-drain"
        ]
        }))
        .unwrap(),
    )
    .unwrap();
    println!("G2/G3 native graph preparation and serial execution passed");
    v2::run(&packages[..2], &args[4], output);
}
