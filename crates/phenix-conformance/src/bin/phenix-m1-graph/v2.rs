//! Native V1 slice only, deliberately run after every legacy acceptance case.
use super::*;
use phenix_contract::{graph::v2::*, ValueSchema};
use phenix_kernel::graph::v2::{
    FlowGrant, OperationSecurity, SourceOwnerPolicy, TrustedCompositionPolicy,
};
use phenix_kernel::Outcome;
use std::collections::BTreeMap;

const A: &str = "example.graph-a";
const B: &str = "example.graph-b";
const FIRST: &str = "example.graph-a/producer";
const SECOND: &str = "example.graph-b/consumer";
const SOURCE: &str = "example.graph-a/source";
const ACTION: &str = "example.graph.v2-submit";
const CONSUME: &str = "example.graph.consume@1/run";
const REFERENCE_SOURCE: &str = "example.graph.reference-source@1/run";
const REFERENCE_CONSUMER: &str = "example.graph.reference-consumer@1/run";
const LABEL: PortSecurity = PortSecurity {
    confidentiality: Classification::Confidential,
    integrity: IntegrityClass::Untrusted,
};

fn declarations(slow: bool, err: bool) -> Vec<GraphContributionV2> {
    let request = ValueSchema::Struct(BTreeMap::from([("text".into(), ValueSchema::String)]));
    let execution = ExecutionPolicy {
        on_domain_error: DomainErrorPolicy::RouteErr,
    };
    let invoke = |id: &str, operation: &str, schema| NodeDecl {
        id: id.into(),
        kind: NodeKind::Invoke {
            selector: String::new(),
            operation: operation.into(),
        },
        inputs: vec![InputPort {
            name: "in".into(),
            schema,
            readiness: InputReadiness::One,
            security: LABEL,
        }],
        outputs: ["err", "ok"]
            .into_iter()
            .map(|name| OutputPort {
                name: name.into(),
                schema: ValueSchema::String,
                security: LABEL,
            })
            .collect(),
        execution,
    };
    let flow = |id: &str, from: &str, port: &str, to: &str| EdgeDecl {
        id: format!("{A}/{id}"),
        kind: EdgeKind::Flow {
            from: OutPortRef {
                node: from.into(),
                port: port.into(),
            },
            to: InPortRef {
                node: to.into(),
                port: "in".into(),
            },
        },
    };
    vec![
        GraphContributionV2 {
            contributor: A.into(),
            sources: vec![SourceDecl {
                id: SOURCE.into(),
                owner: A.into(),
                kind: SourceKind::ExternalAction {
                    action: ACTION.into(),
                },
                payload: request.clone(),
                security: LABEL,
            }],
            nodes: vec![
                invoke(
                    FIRST,
                    if slow {
                        "example.graph.slow@1/run"
                    } else {
                        KEY
                    },
                    request.clone(),
                ),
                NodeDecl {
                    id: SOURCE.into(),
                    kind: NodeKind::Source {
                        source: SOURCE.into(),
                    },
                    inputs: vec![],
                    outputs: vec![OutputPort {
                        name: "out".into(),
                        schema: request,
                        security: LABEL,
                    }],
                    execution,
                },
            ],
            edges: vec![
                flow("start", SOURCE, "out", FIRST),
                flow("next", FIRST, if err { "err" } else { "ok" }, SECOND),
            ],
            patches: vec![],
        },
        GraphContributionV2 {
            contributor: B.into(),
            sources: vec![],
            nodes: vec![invoke(SECOND, CONSUME, ValueSchema::String)],
            edges: vec![],
            patches: vec![],
        },
    ]
}

fn repack(package: &str, destination: &Path, graph: &GraphContributionV2) -> String {
    fs::create_dir_all(destination.join("lib")).unwrap();
    fs::copy(
        Path::new(package).join("descriptor.bin"),
        destination.join("descriptor.bin"),
    )
    .unwrap();
    let mut bytes = fs::read(Path::new(package).join("lib/plugin.so")).unwrap();
    let original = phenix_artifact::extract_graph(&bytes)
        .unwrap()
        .unwrap()
        .encode()
        .unwrap();
    let at = bytes
        .windows(original.len())
        .position(|b| b == original)
        .unwrap();
    let replacement = graph.normalized().unwrap().encode().unwrap();
    assert_eq!(&bytes[at - 4..at], &(original.len() as u32).to_be_bytes());
    bytes[at - 4..at].copy_from_slice(&(replacement.len() as u32).to_be_bytes());
    // The generated fixed-size graph section is zero padded. Never move ELF data.
    assert!(
        bytes[at + original.len()..at + replacement.len().max(original.len())]
            .iter()
            .all(|b| *b == 0)
    );
    bytes[at..at + original.len().max(replacement.len())].fill(0);
    bytes[at..at + replacement.len()].copy_from_slice(&replacement);
    assert_eq!(
        phenix_artifact::extract_graph_v2(&bytes).unwrap().unwrap(),
        graph.normalized().unwrap()
    );
    fs::write(destination.join("lib/plugin.so"), &bytes).unwrap();
    let mut record: serde_json::Value =
        serde_json::from_slice(&fs::read(Path::new(package).join("package.json")).unwrap())
            .unwrap();
    record["artifact_sha256"] = phenix_artifact::digest(&bytes).into();
    fs::write(
        destination.join("package.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    destination.to_str().unwrap().into()
}

pub(super) fn run(packages: &[String], definitions: &str, output: &Path) {
    let witness = output.join("v2-constructor-witness");
    let consumer = output.join("v2-consumer-witness");
    std::env::set_var("PHENIX_GRAPH_CONSTRUCTOR_WITNESS", &witness);
    std::env::set_var("PHENIX_GRAPH_B_CONSUME_WITNESS", &consumer);
    assert!(!witness.exists());
    let mut cases = Vec::new();
    for (name, slow, err) in [
        ("success", false, false),
        ("pending", true, false),
        ("domain", false, true),
        ("cancel", true, false),
        ("reference-delivery-without-read-authority", false, false),
        ("kernel-failure-not-domain-err", false, true),
    ] {
        let reference = name == "reference-delivery-without-read-authority";
        let kernel_failure = name == "kernel-failure-not-domain-err";
        let producer = if reference {
            REFERENCE_SOURCE
        } else if slow {
            "example.graph.slow@1/run"
        } else {
            KEY
        };
        let consume = if reference {
            REFERENCE_CONSUMER
        } else {
            CONSUME
        };
        let mut d = declarations(slow, err);
        if reference {
            // Use the actual SDK-generated selected operation schemas, never a
            // hand-built reference or a guessed schema identity.
            for (index, key) in [(0, producer), (1, consume)] {
                let package = phenix_artifact::VerifiedPackage::verify_trusted_read_only(
                    Path::new(&packages[index]),
                )
                .unwrap();
                let (contract, operation) = key.rsplit_once('/').unwrap();
                let schema = package
                    .metadata()
                    .definitions
                    .iter()
                    .find(|definition| definition.contract == contract)
                    .unwrap()
                    .operations
                    .iter()
                    .find(|schema| schema.name == operation)
                    .unwrap();
                let node = &mut d[index].nodes[0];
                node.kind = NodeKind::Invoke {
                    selector: String::new(),
                    operation: key.into(),
                };
                node.inputs[0].schema = schema.request.clone();
                node.outputs[0].schema = schema.error.clone();
                node.outputs[1].schema = schema.response.clone();
                if index == 0 {
                    assert!(matches!(schema.response, ValueSchema::Ref(_)));
                } else {
                    assert_eq!(schema.request, d[0].nodes[0].outputs[1].schema);
                }
            }
        }
        let artifacts = packages
            .iter()
            .zip(&d)
            .enumerate()
            .map(|(i, (package, graph))| {
                repack(package, &output.join(format!("v2-{name}-{i}")), graph)
            })
            .collect();
        let result = "example.graph-a.property.result".to_owned();
        let resources = BTreeSet::from([result.clone(), "example.graph.delivery".into()]);
        let set = format!(
            "{}/set",
            phenix_contract::property_contract(A, "result").unwrap()
        );
        let get = format!(
            "{}/get",
            phenix_contract::property_contract(A, "result").unwrap()
        );
        let operations = BTreeSet::from([
            KEY.into(),
            producer.into(),
            consume.into(),
            set.clone(),
            get.clone(),
        ]);
        let grants = [
            ("root", KEY),
            (A, producer),
            (B, consume),
            (A, set.as_str()),
            (A, get.as_str()),
        ]
        .into_iter()
        .map(|(caller, key)| {
            let (interface, operation) = key.rsplit_once('/').unwrap();
            ScopedGrant {
                caller: caller.into(),
                interface: interface.into(),
                operation: operation.into(),
                resources: if key == set || key == get {
                    BTreeSet::from([result.clone()])
                } else if caller == B {
                    BTreeSet::from([if reference {
                        result.clone()
                    } else {
                        "example.graph.delivery".into()
                    }])
                } else {
                    resources.clone()
                },
            }
        })
        .collect();
        let composition = Composition {
            format: "phenix.composition.v3".into(),
            artifacts,
            definitions: vec![],
            entry: Selection {
                interface: TASK.into(),
                operation: "run".into(),
                provider: A.into(),
            },
            slots: vec![],
            grants,
            excluded: BTreeSet::new(),
            substitutions: vec![],
            authority: Authority {
                operations,
                resources: resources.clone(),
            },
            resource: None,
        };
        if reference {
            assert!(composition.slots.is_empty());
            assert!(composition
                .grants
                .iter()
                .filter(|grant| grant.caller == B)
                .all(
                    |grant| grant.interface == "example.graph.reference-consumer@1"
                        && grant.operation == "run"
                ));
            let b =
                phenix_artifact::VerifiedPackage::verify_trusted_read_only(Path::new(&packages[1]))
                    .unwrap();
            assert!(
                b.metadata().requires.is_empty(),
                "B must have no selected property import"
            );
        }
        let mut policy = TrustedCompositionPolicy::default();
        policy.topology.insert((A.into(), SECOND.into()));
        policy.source_owners.insert(
            A.into(),
            SourceOwnerPolicy {
                owner_ceiling: Classification::Confidential,
                composition_ceiling: Classification::Confidential,
            },
        );
        policy.sources.insert(SOURCE.into(), LABEL);
        for (owner, key) in [(A, producer), (B, consume)] {
            policy.operations.insert(
                (owner.into(), key.into()),
                OperationSecurity {
                    input: LABEL,
                    ok: LABEL,
                    err: LABEL,
                },
            );
        }
        policy.delivery.insert(
            (
                FIRST.into(),
                if err { "err" } else { "ok" }.into(),
                SECOND.into(),
                "in".into(),
            ),
            FlowGrant {
                resources: BTreeSet::from([if reference {
                    result.clone()
                } else {
                    "example.graph.delivery".into()
                }]),
                maximum_classification: Classification::Confidential,
            },
        );
        if name == "success" {
            for negative in [
                "grant",
                "topology",
                "source-ceiling",
                "composition-ceiling",
                "unselected",
                "mixed",
            ] {
                let mut c = composition.clone();
                let mut p = policy.clone();
                match negative {
                    "grant" => c.grants.retain(|g| g.caller != B),
                    "topology" => p.topology.clear(),
                    "source-ceiling" => {
                        p.source_owners.get_mut(A).unwrap().owner_ceiling = Classification::Public
                    }
                    "composition-ceiling" => {
                        p.source_owners.get_mut(A).unwrap().composition_ceiling =
                            Classification::Public
                    }
                    "unselected" => {
                        c.excluded.insert(B.into());
                    }
                    "mixed" => c.definitions.push(definitions.into()),
                    _ => unreachable!(),
                }
                assert!(Prepared::prepare_graph_v2(c, &p).is_err(), "{negative}");
                assert!(!witness.exists(), "{negative} ran constructor");
                cases.push(negative.to_owned());
            }
            let mut spoof = d[0].clone();
            spoof.nodes[0].inputs[0].schema = ValueSchema::String;
            let mut c = composition.clone();
            c.artifacts[0] = repack(&packages[0], &output.join("v2-schema-spoof"), &spoof);
            assert_eq!(
                Prepared::prepare_graph_v2(c, &policy).err().unwrap().code,
                Code::SchemaRejected
            );
            assert!(!witness.exists());
            cases.push("schema-spoof".into());
        }
        let kernel = Prepared::prepare_graph_v2(composition.clone(), &policy)
            .unwrap()
            .load()
            .unwrap();
        assert!(
            witness.exists(),
            "positive load did not exercise constructor witness"
        );
        let scope = ScopedRequest {
            authority: composition.authority.clone(),
            resource: None,
        };
        let input = if kernel_failure {
            "kernel-a"
        } else if err {
            "domain"
        } else if name == "cancel" {
            "slow-drain"
        } else {
            "protected-v2"
        };
        let payload = Value::map([("text", Value::String(input.into()))]);
        let before = fs::read_to_string(&consumer).unwrap_or_default();
        let fds = fs::read_dir("/proc/self/fd").unwrap().count();
        let mut execution = kernel
            .begin_action_scoped(ACTION, &payload, scope.clone())
            .unwrap();
        let mut root = None;
        let mut terminals = 0;
        let mut observe = |update: &phenix_kernel::graph::GraphUpdate<'_>| {
            assert_eq!(*root.get_or_insert(update.root), update.root);
            assert!(
                !matches!(update.event, GraphEvent::NodeFinished(_)),
                "protected payload exposed"
            );
            if matches!(update.event, GraphEvent::Finished(_)) {
                assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
                assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds);
                terminals += 1;
            }
        };
        let first = execution.pump(&mut observe);
        if kernel_failure {
            assert_eq!(first.unwrap_err().code, Code::PluginFailed);
            assert!(
                matches!(execution.completion(), Some(GraphCompletion::Failure(failure))
                if failure.code == Code::PluginFailed && failure.message == "kernel failure fixture a")
            );
            assert!(
                execution.steps().is_empty(),
                "kernel failure became a domain result"
            );
            assert_eq!(
                fs::read_to_string(&consumer).unwrap_or_default(),
                before,
                "kernel failure invoked the err consumer"
            );
            assert_eq!(terminals, 1);
            assert_eq!(kernel.inspect()["calls"], 1);
            assert_eq!(kernel.inspect()["live_state"]["epoch"], 0);
            assert_eq!(
                execution
                    .pump(|_| panic!("duplicate failure event"))
                    .unwrap_err()
                    .code,
                Code::PluginFailed
            );
            drop(execution);
            kernel.shutdown().unwrap();
            cases.push(name.into());
            continue;
        }
        let first = first.unwrap();
        if slow {
            assert_eq!(first, GraphStatus::Pending);
            assert!(execution.steps().is_empty());
            assert_eq!(fs::read_to_string(&consumer).unwrap_or_default(), before);
            assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
            assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds + 1);
            if name == "cancel" {
                execution.cancel().unwrap();
                assert_eq!(execution.pump(&mut observe).unwrap(), GraphStatus::Draining);
                assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
                assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds + 1);
                assert!(kernel.shutdown().is_err());
            } else {
                assert_eq!(execution.pump(&mut observe).unwrap(), GraphStatus::Pending);
                assert_eq!(fs::read_to_string(&consumer).unwrap_or_default(), before);
            }
            for _ in 0..8 {
                if execution.pump(&mut observe).unwrap() == GraphStatus::Finished {
                    break;
                }
            }
        } else {
            assert_eq!(
                first,
                GraphStatus::Finished,
                "{name}: synchronous native tickets must settle in the admitting pump"
            );
        }
        assert_eq!(
            execution
                .pump(|_| panic!("duplicate observer event"))
                .unwrap(),
            GraphStatus::Finished
        );
        assert_eq!(terminals, 1);
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds);
        if name == "cancel" {
            assert_eq!(execution.completion(), Some(GraphCompletion::Cancelled));
            assert!(execution.steps().is_empty());
            assert_eq!(fs::read_to_string(&consumer).unwrap_or_default(), before);
        } else if reference {
            assert_eq!(execution.completion(), Some(GraphCompletion::Success));
            assert_eq!(execution.steps().len(), 2);
            assert_eq!(execution.steps()[0].node, FIRST);
            assert!(
                matches!(
                    execution.steps()[0].outcome,
                    Outcome::Success(Value::Ref(_))
                ),
                "producer did not return a live reference"
            );
            assert_eq!(execution.steps()[1].node, SECOND);
            assert_eq!(
                execution.steps()[1].outcome,
                Outcome::Success(Value::String("denied-read".into()))
            );
            assert_eq!(
                fs::read_to_string(&consumer).unwrap(),
                format!("{before}denied-read\n")
            );
            assert_eq!(kernel.inspect()["calls"], 2);
            assert_eq!(kernel.inspect()["live_state"]["epoch"], 1);
        } else {
            assert_eq!(execution.steps().len(), 2);
            let produced = if err { "domain fixture" } else { input };
            assert_eq!(execution.steps()[0].node, FIRST);
            assert_eq!(
                execution.steps()[0].outcome,
                if err {
                    Outcome::Domain(Value::String(produced.into()))
                } else {
                    Outcome::Success(Value::String(produced.into()))
                }
            );
            assert_eq!(execution.steps()[1].node, SECOND);
            assert_eq!(
                execution.steps()[1].outcome,
                Outcome::Success(Value::String(format!("consumed:{produced}")))
            );
            assert_eq!(
                fs::read_to_string(&consumer).unwrap(),
                format!("{before}called\n")
            );
        }
        drop(execution);
        if name == "success" {
            assert_eq!(
                kernel
                    .admit_default_root()
                    .unwrap()
                    .invoke(&payload)
                    .unwrap(),
                Outcome::Success(Value::String(input.into()))
            );
            let mut next = kernel
                .begin_action_scoped(ACTION, &payload, scope.clone())
                .unwrap();
            next.pump(|u| assert_ne!(Some(u.root), root)).unwrap();
            drop(next);
            let mut narrow = scope;
            narrow.authority.resources.remove("example.graph.delivery");
            let before = fs::read_to_string(&consumer).unwrap();
            let calls_before = kernel.inspect()["calls"].as_u64().unwrap();
            let epoch_before = kernel.inspect()["live_state"]["epoch"].as_u64().unwrap();
            let mut denied = kernel
                .begin_action_scoped(ACTION, &payload, narrow)
                .unwrap();
            assert_eq!(
                denied
                    .pump(|u| assert!(!matches!(u.event, GraphEvent::NodeFinished(_))))
                    .unwrap_err()
                    .code,
                Code::Denied
            );
            assert_eq!(fs::read_to_string(&consumer).unwrap(), before);
            assert_eq!(
                kernel.inspect()["calls"].as_u64().unwrap(),
                calls_before + 1
            );
            assert!(kernel.inspect()["live_state"]["epoch"].as_u64().unwrap() > epoch_before);
            cases.extend(
                [
                    "direct-task-parity",
                    "separate-roots",
                    "narrowed-delivery-after-producer",
                    "payload-free-observers",
                    "no-duplicate-delivery",
                ]
                .map(str::to_owned),
            );
        }
        kernel.shutdown().unwrap();
        cases.push(name.into());
    }
    std::env::remove_var("PHENIX_GRAPH_B_CONSUME_WITNESS");
    fs::write(output.join("graph-v2-evidence.json"), serde_json::to_vec_pretty(&serde_json::json!({"format":"phenix.graph-v2-evidence.v1", "acceptance":"V1 partial", "p1_parity":false, "cases":cases, "limits":["initial finite wave only", "no operators, patches or deferred waves", "no SDK or P1 migration parity"]})).unwrap()).unwrap();
}
