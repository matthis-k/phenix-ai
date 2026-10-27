//! Native Join All acceptance through ordinary domain contracts.
use super::*;

pub(super) fn run(packages: &[String], output: &Path) {
    const SLOW: &str = "example.graph.slow@1/run";
    const FAST: &str = "example.graph-b/join-task";
    const JOIN: &str = "example.graph-b/join";
    const CONSUMER: &str = "example.graph.join-consumer@1/run";
    const UNUSED: &str = "example.graph-a/join-unused";
    const UNUSED_ACTION: &str = "example.graph.v2-join-unused";
    let consumer = output.join("v2-join-consumer");
    let witness = output.join("v2-join-constructors");
    std::env::set_var("PHENIX_GRAPH_B_JOIN_WITNESS", &consumer);
    std::env::set_var("PHENIX_GRAPH_CONSTRUCTOR_WITNESS", &witness);
    assert!(!consumer.exists());
    assert!(!witness.exists());
    let mut d = declarations(true, false);
    let mut fast = d[1].nodes[0].clone();
    fast.id = FAST.into();
    fast.kind = NodeKind::Invoke {
        selector: String::new(),
        operation: KEY.into(),
    };
    d[1].nodes.push(fast);
    for (index, id, key) in [(0, FIRST, SLOW), (1, FAST, KEY), (1, SECOND, CONSUMER)] {
        let package =
            phenix_artifact::VerifiedPackage::verify_trusted_read_only(Path::new(&packages[index]))
                .unwrap();
        let (contract, operation) = key.rsplit_once('/').unwrap();
        let schema = package
            .metadata()
            .definitions
            .iter()
            .find(|s| s.contract == contract)
            .unwrap()
            .operations
            .iter()
            .find(|s| s.name == operation)
            .unwrap();
        let node = d[index].nodes.iter_mut().find(|n| n.id == id).unwrap();
        node.kind = NodeKind::Invoke {
            selector: String::new(),
            operation: key.into(),
        };
        node.inputs[0].schema = schema.request.clone();
        node.outputs[0].schema = schema.error.clone();
        node.outputs[1].schema = schema.response.clone();
    }
    let assembled = ValueSchema::Struct(BTreeMap::from([
        ("fast".into(), ValueSchema::String),
        ("slow".into(), ValueSchema::String),
    ]));
    assert_eq!(d[1].nodes[0].inputs[0].schema, assembled);
    d[0].nodes[0].execution.on_domain_error = DomainErrorPolicy::SkipDependents;
    d[0].sources.push(SourceDecl {
        id: UNUSED.into(),
        owner: A.into(),
        kind: SourceKind::ExternalAction {
            action: UNUSED_ACTION.into(),
        },
        payload: ValueSchema::Struct(BTreeMap::new()),
        security: LABEL,
    });
    d[1].nodes.push(NodeDecl {
        id: JOIN.into(),
        kind: NodeKind::Join {
            policy: JoinPolicy::All,
        },
        inputs: ["fast", "slow"]
            .into_iter()
            .map(|name| InputPort {
                name: name.into(),
                schema: ValueSchema::String,
                readiness: InputReadiness::One,
                security: LABEL,
            })
            .collect(),
        outputs: vec![OutputPort {
            name: "out".into(),
            schema: assembled,
            security: LABEL,
        }],
        execution: ExecutionPolicy {
            on_domain_error: DomainErrorPolicy::RouteErr,
        },
    });
    let resources = BTreeSet::from([
        "example.graph-a.property.result".to_owned(),
        "example.graph-b.property.result".to_owned(),
        "example.graph.delivery".to_owned(),
    ]);
    let mut grants = Vec::new();
    for (caller, key) in [("root", KEY), (A, SLOW), (B, KEY), (B, CONSUMER)] {
        let (interface, operation) = key.rsplit_once('/').unwrap();
        grants.push(ScopedGrant {
            caller: caller.into(),
            interface: interface.into(),
            operation: operation.into(),
            resources: resources.clone(),
        });
    }
    for owner in [A, B] {
        for operation in ["get", "set"] {
            grants.push(ScopedGrant {
                caller: owner.into(),
                interface: phenix_contract::property_contract(owner, "result").unwrap(),
                operation: operation.into(),
                resources: BTreeSet::from([format!("{owner}.property.result")]),
            });
        }
    }
    for closed in [false, true] {
        for graph in &mut d {
            graph.edges.clear();
        }
        let routes = [
            (0, SOURCE, "out", FIRST, "in"),
            (0, SOURCE, "out", FAST, "in"),
            (0, FIRST, "ok", JOIN, "slow"),
            (1, FAST, if closed { "err" } else { "ok" }, JOIN, "fast"),
            (0, JOIN, "out", SECOND, "in"),
        ];
        for (index, from, port, to, into) in routes {
            let graph = &mut d[index];
            graph.edges.push(EdgeDecl {
                id: format!("{}/join-flow-{}", graph.contributor, graph.edges.len()),
                kind: EdgeKind::Flow {
                    from: OutPortRef {
                        node: from.into(),
                        port: port.into(),
                    },
                    to: InPortRef {
                        node: to.into(),
                        port: into.into(),
                    },
                },
            });
        }
        let composition = Composition {
            format: "phenix.composition.v3".into(),
            artifacts: d
                .iter()
                .enumerate()
                .map(|(i, graph)| {
                    repack(
                        &packages[i],
                        &output.join(format!("v2-join-{closed}-{i}")),
                        graph,
                    )
                })
                .collect(),
            definitions: vec![],
            entry: Selection {
                interface: TASK.into(),
                operation: "run".into(),
                provider: A.into(),
            },
            slots: vec![],
            authority: Authority {
                operations: grants
                    .iter()
                    .map(|g| format!("{}/{}", g.interface, g.operation))
                    .collect(),
                resources: resources.clone(),
            },
            grants: grants.clone(),
            excluded: BTreeSet::new(),
            substitutions: vec![],
            resource: None,
        };
        let mut policy = TrustedCompositionPolicy {
            topology: BTreeSet::from([
                (A.into(), FAST.into()),
                (A.into(), JOIN.into()),
                (A.into(), SECOND.into()),
            ]),
            ..TrustedCompositionPolicy::default()
        };
        policy.source_owners.insert(
            A.into(),
            SourceOwnerPolicy {
                owner_ceiling: Classification::Confidential,
                composition_ceiling: Classification::Confidential,
            },
        );
        for source in [SOURCE, UNUSED] {
            policy.sources.insert(source.into(), LABEL);
        }
        for (owner, key) in [(A, SLOW), (B, KEY), (B, CONSUMER)] {
            policy.operations.insert(
                (owner.into(), key.into()),
                OperationSecurity {
                    input: LABEL,
                    ok: LABEL,
                    err: LABEL,
                },
            );
        }
        for (_, from, port, to, into) in routes {
            policy.delivery.insert(
                (from.into(), port.into(), to.into(), into.into()),
                FlowGrant {
                    resources: if from == FIRST && to == JOIN {
                        BTreeSet::from(["example.graph.delivery".into()])
                    } else {
                        resources.clone()
                    },
                    maximum_classification: Classification::Confidential,
                },
            );
        }
        if !closed {
            let mut wrong = policy.clone();
            wrong
                .operations
                .get_mut(&(B.into(), CONSUMER.into()))
                .unwrap()
                .input
                .confidentiality = Classification::Public;
            assert!(Prepared::prepare_graph_v2(composition.clone(), &wrong).is_err());
            assert!(!witness.exists(), "wrong label ran native constructor");
        }
        let kernel = Prepared::prepare_graph_v2(composition.clone(), &policy)
            .unwrap()
            .load()
            .unwrap();
        assert!(witness.exists());
        let scope = ScopedRequest {
            authority: composition.authority,
            resource: None,
        };
        for text in if closed {
            vec!["domain", "late-success", "unused"]
        } else {
            vec!["join-value"]
        } {
            let before = fs::read_to_string(&consumer).unwrap_or_default();
            let calls = kernel.inspect()["calls"].as_u64().unwrap();
            let fds = fs::read_dir("/proc/self/fd").unwrap().count();
            let payload = if text == "unused" {
                Value::map([])
            } else {
                Value::map([("text", Value::String(text.into()))])
            };
            let mut action_scope = scope.clone();
            if text == "late-success" {
                // The Join is already non-runnable. A discarded value must not
                // attempt a delivery outside this narrowed destination grant.
                action_scope
                    .authority
                    .resources
                    .remove("example.graph.delivery");
            }
            let mut run = kernel
                .begin_action_scoped(
                    if text == "unused" {
                        UNUSED_ACTION
                    } else {
                        ACTION
                    },
                    &payload,
                    action_scope,
                )
                .unwrap();
            let terminals = std::cell::Cell::new(0);
            let mut root = None;
            let mut observe = |u: &phenix_kernel::graph::GraphUpdate<'_>| {
                assert_eq!(*root.get_or_insert(u.root), u.root);
                assert!(!matches!(u.event, GraphEvent::NodeFinished(_)));
                if matches!(u.event, GraphEvent::Finished(_)) {
                    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
                    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds);
                    terminals.set(terminals.get() + 1);
                }
            };
            if text != "unused" {
                for _ in 0..2 {
                    assert_eq!(run.pump(&mut observe).unwrap(), GraphStatus::Pending);
                    assert!(run.completion().is_none());
                    assert_eq!(terminals.get(), 0);
                    assert_eq!(run.steps().len(), 1);
                    assert_eq!(run.steps()[0].node, FAST);
                    assert_eq!(
                        run.steps()[0].outcome,
                        Outcome::Success(Value::String(text.into()))
                    );
                    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
                    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds + 1);
                    assert_eq!(fs::read_to_string(&consumer).unwrap_or_default(), before);
                }
            }
            for _ in 0..4 {
                if run.pump(&mut observe).unwrap() == GraphStatus::Finished {
                    break;
                }
            }
            assert_eq!(terminals.get(), 1);
            assert_eq!(
                run.completion(),
                Some(if text == "domain" {
                    GraphCompletion::Domain
                } else {
                    GraphCompletion::Success
                })
            );
            assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
            if text == "join-value" {
                assert_eq!(run.steps().len(), 3);
                assert!(run.steps().iter().any(|s| s.node == FIRST
                    && s.outcome == Outcome::Success(Value::String(text.into()))));
                let expected = "joined:fast=join-value;slow=join-value";
                assert_eq!(run.steps()[2].node, SECOND);
                assert_eq!(
                    run.steps()[2].outcome,
                    Outcome::Success(Value::String(expected.into()))
                );
                assert_eq!(
                    fs::read_to_string(&consumer).unwrap(),
                    format!("{before}{expected}\n")
                );
            } else {
                assert_eq!(fs::read_to_string(&consumer).unwrap_or_default(), before);
                if text == "domain" {
                    assert_eq!(run.steps().len(), 2);
                    // A cancelled SlowTask cannot return this domain result: its cancel
                    // callback discards input and its cancelled poll returns "drained".
                    assert!(run.steps().iter().any(|s| s.node == FIRST
                        && s.outcome == Outcome::Domain(Value::String("domain fixture".into()))));
                } else if text == "late-success" {
                    assert_eq!(run.steps().len(), 2);
                    assert!(run.steps().iter().any(|s| s.node == FIRST
                        && s.outcome == Outcome::Success(Value::String(text.into()))));
                } else {
                    assert!(run.steps().is_empty());
                }
            }
            assert_eq!(
                kernel.inspect()["calls"].as_u64().unwrap(),
                calls
                    + match text {
                        "unused" => 0,
                        "domain" => 5,
                        "late-success" => 5,
                        _ => 6,
                    }
            );
            for _ in 0..2 {
                assert_eq!(
                    run.pump(|_| panic!("duplicate Join event")).unwrap(),
                    GraphStatus::Finished
                );
            }
        }
        kernel.shutdown().unwrap();
    }
    std::env::remove_var("PHENIX_GRAPH_B_JOIN_WITNESS");
    std::env::remove_var("PHENIX_GRAPH_CONSTRUCTOR_WITNESS");
    fs::write(output.join("graph-v2-join.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "format": "phenix.graph-v2-join.v1", "full_V2": false, "p1_parity": false,
        "cases": { "native_join_all": true, "one_value_pending_waits": true, "assembled_fields": true,
             "native_consumer_once": true, "closed_plus_late_failure": true, "closed_plus_late_success": true,
             "unrelated_producer_not_cancelled": true, "discarded_delivery_needs_no_destination_access": true,
            "all_closed_success": true, "terminal_after_worker_drain": true, "wrong_labels_before_constructors": true,
            "protected_observers": true, "repeat_pumps_inert": true },
        "remaining": ["full V2", "P1 parity"]
    })).unwrap()).unwrap();
}
