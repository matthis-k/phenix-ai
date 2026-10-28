//! Native intrinsic routing, after the legacy and V1 acceptance suites.
use super::*;

pub(super) fn run(packages: &[String], output: &Path) {
    const SWITCH: &str = "example.graph-a/switch";
    const CONSTANT: &str = "example.graph-a/constant";
    const MERGE: &str = "example.graph-a/merge";
    const OTHER: &str = "example.graph-a/other";
    const OTHER_ACTION: &str = "example.graph.v2-other";
    let public = PortSecurity {
        confidentiality: Classification::Public,
        integrity: IntegrityClass::Untrusted,
    };
    let execution = ExecutionPolicy {
        on_domain_error: DomainErrorPolicy::RouteErr,
    };
    let tagged = ValueSchema::Tagged(BTreeMap::from([
        ("empty".into(), None),
        ("text".into(), Some(ValueSchema::String)),
    ]));
    let unit = ValueSchema::Struct(BTreeMap::new());
    let input = |name: &str, schema| InputPort {
        name: name.into(),
        schema,
        readiness: InputReadiness::One,
        security: public,
    };
    let port = |name: &str, schema| OutputPort {
        name: name.into(),
        schema,
        security: public,
    };
    let package =
        phenix_artifact::VerifiedPackage::verify_trusted_read_only(Path::new(&packages[1]))
            .unwrap();
    let schema = package
        .metadata()
        .definitions
        .iter()
        .find(|d| d.contract == "example.graph.consume@1")
        .unwrap()
        .operations
        .iter()
        .find(|s| s.name == "run")
        .unwrap();
    assert_eq!(schema.request, ValueSchema::String);
    let mut a = GraphContributionV2 {
        contributor: A.into(),
        sources: vec![],
        nodes: vec![],
        edges: vec![],
        patches: vec![],
    };
    for (id, action, payload) in [
        (SOURCE, ACTION, tagged.clone()),
        (OTHER, OTHER_ACTION, ValueSchema::String),
    ] {
        a.sources.push(SourceDecl {
            id: id.into(),
            owner: A.into(),
            kind: SourceKind::ExternalAction {
                action: action.into(),
            },
            payload: payload.clone(),
            security: public,
        });
        a.nodes.push(NodeDecl {
            id: id.into(),
            kind: NodeKind::Source { source: id.into() },
            inputs: vec![],
            outputs: vec![port("out", payload)],
            execution,
        });
    }
    a.nodes.extend([
        NodeDecl {
            id: SWITCH.into(),
            kind: NodeKind::Switch {
                tagged_schema: tagged.id().unwrap(),
            },
            inputs: vec![input("in", tagged)],
            outputs: vec![
                port("empty", unit.clone()),
                port("text", ValueSchema::String),
            ],
            execution,
        },
        NodeDecl {
            id: CONSTANT.into(),
            kind: NodeKind::Constant {
                value: Value::String("unit-case".into()),
            },
            inputs: vec![input("start", unit)],
            outputs: vec![port("out", ValueSchema::String)],
            execution,
        },
        NodeDecl {
            id: MERGE.into(),
            kind: NodeKind::Merge {
                policy: MergePolicy::ExactlyOne,
            },
            inputs: vec![input("in", ValueSchema::String)],
            outputs: vec![port("out", ValueSchema::String)],
            execution,
        },
    ]);
    for (index, (from, out, to, into)) in [
        (SOURCE, "out", SWITCH, "in"),
        (SWITCH, "text", MERGE, "in"),
        (SWITCH, "empty", CONSTANT, "start"),
        (CONSTANT, "out", MERGE, "in"),
        (OTHER, "out", MERGE, "in"),
        (MERGE, "out", SECOND, "in"),
    ]
    .into_iter()
    .enumerate()
    {
        a.edges.push(EdgeDecl {
            id: format!("{A}/operator-flow-{index}"),
            kind: EdgeKind::Flow {
                from: OutPortRef {
                    node: from.into(),
                    port: out.into(),
                },
                to: InPortRef {
                    node: to.into(),
                    port: into.into(),
                },
            },
        });
    }
    let b = GraphContributionV2 {
        contributor: B.into(),
        sources: vec![],
        nodes: vec![NodeDecl {
            id: SECOND.into(),
            kind: NodeKind::Invoke {
                selector: String::new(),
                operation: CONSUME.into(),
            },
            inputs: vec![input("in", schema.request.clone())],
            outputs: vec![
                port("err", schema.error.clone()),
                port("ok", schema.response.clone()),
            ],
            execution,
        }],
        edges: vec![],
        patches: vec![],
    };
    let witness = output.join("v2-operators-constructors");
    let consumer = output.join("v2-operators-consumer");
    std::env::set_var("PHENIX_GRAPH_CONSTRUCTOR_WITNESS", &witness);
    std::env::set_var("PHENIX_GRAPH_B_CONSUME_WITNESS", &consumer);
    assert!(!witness.exists());
    // Offline rejection keeps the whole tagged source and Switch input confidential.
    let mut secret = a.clone();
    secret
        .sources
        .iter_mut()
        .find(|s| s.id == SOURCE)
        .unwrap()
        .security = LABEL;
    secret
        .nodes
        .iter_mut()
        .find(|n| n.id == SOURCE)
        .unwrap()
        .outputs[0]
        .security = LABEL;
    secret
        .nodes
        .iter_mut()
        .find(|n| n.id == SWITCH)
        .unwrap()
        .inputs[0]
        .security = LABEL;
    assert!(
        secret.encode().is_err(),
        "offline Switch downgrade accepted"
    );
    assert!(!witness.exists());
    let resources = BTreeSet::from(["example.graph.delivery".to_owned()]);
    let composition = Composition {
        format: "phenix.composition.v3".into(),
        artifacts: vec![
            repack(&packages[0], &output.join("v2-operators-a"), &a),
            repack(&packages[1], &output.join("v2-operators-b"), &b),
        ],
        definitions: vec![],
        entry: Selection {
            interface: TASK.into(),
            operation: "run".into(),
            provider: A.into(),
        },
        slots: vec![],
        grants: [("root", KEY), (B, CONSUME)]
            .into_iter()
            .map(|(caller, key)| {
                let (interface, operation) = key.rsplit_once('/').unwrap();
                ScopedGrant {
                    caller: caller.into(),
                    interface: interface.into(),
                    operation: operation.into(),
                    resources: resources.clone(),
                }
            })
            .collect(),
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: BTreeSet::from([KEY.into(), CONSUME.into()]),
            resources: resources.clone(),
        },
        resource: None,
    };
    let mut policy = TrustedCompositionPolicy::default();
    policy.topology.insert((A.into(), SECOND.into()));
    policy.source_owners.insert(
        A.into(),
        SourceOwnerPolicy {
            owner_ceiling: Classification::Public,
            composition_ceiling: Classification::Public,
        },
    );
    for source in [SOURCE, OTHER] {
        policy.sources.insert(source.into(), public);
    }
    policy.operations.insert(
        (B.into(), CONSUME.into()),
        OperationSecurity {
            input: public,
            ok: public,
            err: public,
        },
    );
    policy.delivery.insert(
        (MERGE.into(), "out".into(), SECOND.into(), "in".into()),
        FlowGrant {
            resources,
            maximum_classification: Classification::Public,
        },
    );
    for negative in ["topology", "delivery", "consume-grant"] {
        let mut c = composition.clone();
        let mut p = policy.clone();
        match negative {
            "topology" => p.topology.clear(),
            "delivery" => p.delivery.clear(),
            "consume-grant" => c.grants.retain(|g| g.caller != B),
            _ => unreachable!(),
        }
        assert!(Prepared::prepare_graph_v2(c, &p).is_err(), "{negative}");
        assert!(!witness.exists(), "{negative} invoked constructors");
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
    for malformed in [
        Value::map([("text", Value::Bool(true))]),
        Value::map([("unknown", Value::map([]))]),
        Value::map([("empty", Value::String("not-unit".into()))]),
    ] {
        assert!(kernel
            .begin_action_scoped(ACTION, &malformed, scope.clone())
            .is_err());
        assert!(!consumer.exists());
        assert_eq!(kernel.inspect()["calls"], 0);
    }
    let mut roots = BTreeSet::new();
    for (index, (action, payload, expected)) in [
        (
            ACTION,
            Value::map([("text", Value::String("text".into()))]),
            "text",
        ),
        (ACTION, Value::map([("empty", Value::Null)]), "unit-case"),
        (OTHER_ACTION, Value::String("other".into()), "other"),
    ]
    .into_iter()
    .enumerate()
    {
        let before = fs::read_to_string(&consumer).unwrap_or_default();
        let mut run = kernel
            .begin_action_scoped(action, &payload, scope.clone())
            .unwrap();
        let mut root = None;
        let mut terminals = 0;
        assert_eq!(
            run.pump(|u| {
                assert_eq!(*root.get_or_insert(u.root), u.root);
                if matches!(u.event, GraphEvent::Finished(_)) {
                    terminals += 1;
                }
            })
            .unwrap(),
            GraphStatus::Finished,
            "inactive source candidates did not close"
        );
        assert!(roots.insert(root.unwrap()));
        assert_eq!(terminals, 1);
        assert_eq!(run.completion(), Some(GraphCompletion::Success));
        assert_eq!(
            run.steps().len(),
            1,
            "intrinsics must not invoke native operations"
        );
        assert_eq!(run.steps()[0].node, SECOND);
        assert_eq!(
            run.steps()[0].outcome,
            Outcome::Success(Value::String(format!("consumed:{expected}")))
        );
        assert_eq!(
            run.pump(|_| panic!("duplicate event")).unwrap(),
            GraphStatus::Finished
        );
        assert_eq!(
            fs::read_to_string(&consumer).unwrap(),
            format!("{before}called\n")
        );
        assert_eq!(kernel.inspect()["calls"], index + 1);
    }
    kernel.shutdown().unwrap();
    terminal_reduction(packages, output);
    std::env::remove_var("PHENIX_GRAPH_B_CONSUME_WITNESS");
    std::env::remove_var("PHENIX_GRAPH_CONSTRUCTOR_WITNESS");
    fs::write(output.join("graph-v2-operators.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "format": "phenix.graph-v2-operators.v1", "full_V2": false, "p1_parity": false,
        "cases": { "tagged_text": true, "unit_constant": true, "cross_source_alternative": true, "inactive_source_closure": true, "separate_roots": true, "native_consumer_once": true, "no_duplicate_delivery": true, "independent_topology_delivery_grants": true, "malformed_tagged_admission": true, "offline_confidential_public_switch_rejected_before_constructors": true, "native_merge_waits_for_pending_candidate": true, "native_merge_two_values_infrastructure_failure": true, "native_merge_value_failed_domain": true, "native_merge_all_closed_success": true, "native_merge_terminal_after_lease_drain": true, "native_merge_protected_observers": true, "native_merge_repeat_pumps_inert": true },
        "remaining": ["Join", "readiness", "P1 parity"]
    })).unwrap()).unwrap();
}

fn terminal_reduction(packages: &[String], output: &Path) {
    const SLOW: &str = "example.graph.slow@1/run";
    const FAST: &str = "example.graph-b/task";
    const MERGE: &str = "example.graph-a/terminal-merge";
    const UNUSED: &str = "example.graph-a/unused";
    const UNUSED_ACTION: &str = "example.graph.v2-unused";
    let mut declarations = declarations(true, false);
    // All Invoke ports come from the selected native operation definitions.
    let mut fast = declarations[1].nodes[0].clone();
    fast.id = FAST.into();
    fast.kind = NodeKind::Invoke {
        selector: String::new(),
        operation: KEY.into(),
    };
    declarations[1].nodes.push(fast);
    for (index, id, key) in [(0, FIRST, SLOW), (1, FAST, KEY), (1, SECOND, CONSUME)] {
        let package =
            phenix_artifact::VerifiedPackage::verify_trusted_read_only(Path::new(&packages[index]))
                .unwrap();
        let (contract, operation) = key.rsplit_once('/').unwrap();
        let schema = package
            .metadata()
            .definitions
            .iter()
            .find(|d| d.contract == contract)
            .unwrap()
            .operations
            .iter()
            .find(|s| s.name == operation)
            .unwrap();
        let node = declarations[index]
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .unwrap();
        node.inputs[0].schema = schema.request.clone();
        node.outputs[0].schema = schema.error.clone();
        node.outputs[1].schema = schema.response.clone();
    }
    declarations[0]
        .nodes
        .iter_mut()
        .find(|n| n.id == FIRST)
        .unwrap()
        .execution
        .on_domain_error = DomainErrorPolicy::SkipDependents;
    declarations[0].sources.push(SourceDecl {
        id: UNUSED.into(),
        owner: A.into(),
        kind: SourceKind::ExternalAction {
            action: UNUSED_ACTION.into(),
        },
        payload: ValueSchema::Struct(BTreeMap::new()),
        security: LABEL,
    });
    declarations[0].nodes.push(NodeDecl {
        id: MERGE.into(),
        kind: NodeKind::Merge {
            policy: MergePolicy::ExactlyOne,
        },
        inputs: vec![InputPort {
            name: "in".into(),
            schema: ValueSchema::String,
            readiness: InputReadiness::One,
            security: LABEL,
        }],
        outputs: vec![OutputPort {
            name: "out".into(),
            schema: ValueSchema::String,
            security: LABEL,
        }],
        execution: ExecutionPolicy {
            on_domain_error: DomainErrorPolicy::RouteErr,
        },
    });
    for d in &mut declarations {
        d.edges.clear();
    }
    for (index, from, port, to) in [
        (0, SOURCE, "out", FIRST),
        (0, SOURCE, "out", FAST),
        (0, FIRST, "ok", MERGE),
        (1, FAST, "ok", MERGE),
        (0, MERGE, "out", SECOND),
    ] {
        let d = &mut declarations[index];
        d.edges.push(EdgeDecl {
            id: format!("{}/terminal-flow-{}", d.contributor, d.edges.len()),
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
        });
    }
    let result_a = "example.graph-a.property.result".to_owned();
    let result_b = "example.graph-b.property.result".to_owned();
    let delivery = "example.graph.delivery".to_owned();
    let mut grants = Vec::new();
    for (caller, key, resources) in [
        (
            "root",
            KEY.to_owned(),
            BTreeSet::from([result_a.clone(), result_b.clone(), delivery.clone()]),
        ),
        (A, SLOW.to_owned(), BTreeSet::from([result_a.clone()])),
        (
            B,
            KEY.to_owned(),
            BTreeSet::from([result_b.clone(), delivery.clone()]),
        ),
        (B, CONSUME.to_owned(), BTreeSet::from([delivery.clone()])),
        (
            A,
            format!(
                "{}/set",
                phenix_contract::property_contract(A, "result").unwrap()
            ),
            BTreeSet::from([result_a.clone()]),
        ),
        (
            B,
            format!(
                "{}/set",
                phenix_contract::property_contract(B, "result").unwrap()
            ),
            BTreeSet::from([result_b.clone()]),
        ),
    ] {
        let (interface, operation) = key.rsplit_once('/').unwrap();
        grants.push(ScopedGrant {
            caller: caller.into(),
            interface: interface.into(),
            operation: operation.into(),
            resources,
        });
    }
    // Native property edit reads the current value before committing its write.
    for (owner, resource) in [(A, &result_a), (B, &result_b)] {
        grants.push(ScopedGrant {
            caller: owner.into(),
            interface: phenix_contract::property_contract(owner, "result").unwrap(),
            operation: "get".into(),
            resources: BTreeSet::from([resource.clone()]),
        });
    }
    let composition = Composition {
        format: "phenix.composition.v3".into(),
        artifacts: declarations
            .iter()
            .enumerate()
            .map(|(i, d)| repack(&packages[i], &output.join(format!("v2-terminal-{i}")), d))
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
            resources: BTreeSet::from([result_a, result_b.clone(), delivery.clone()]),
        },
        grants,
        excluded: BTreeSet::new(),
        substitutions: vec![],
        resource: None,
    };
    let mut policy = TrustedCompositionPolicy {
        topology: BTreeSet::from([
            (A.into(), FAST.into()),
            (B.into(), MERGE.into()),
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
    for id in [SOURCE, UNUSED] {
        policy.sources.insert(id.into(), LABEL);
    }
    for (owner, key) in [(A, SLOW), (B, KEY), (B, CONSUME)] {
        policy.operations.insert(
            (owner.into(), key.into()),
            OperationSecurity {
                input: LABEL,
                ok: LABEL,
                err: LABEL,
            },
        );
    }
    for (from, port, to, resources) in [
        (
            SOURCE,
            "out",
            FAST,
            BTreeSet::from([result_b.clone(), delivery.clone()]),
        ),
        (FAST, "ok", MERGE, BTreeSet::from([delivery.clone()])),
        (MERGE, "out", SECOND, BTreeSet::from([delivery])),
    ] {
        policy.delivery.insert(
            (from.into(), port.into(), to.into(), "in".into()),
            FlowGrant {
                resources,
                maximum_classification: Classification::Confidential,
            },
        );
    }
    let consumer = output.join("v2-terminal-consumer");
    std::env::set_var("PHENIX_GRAPH_B_CONSUME_WITNESS", &consumer);
    let kernel = Prepared::prepare_graph_v2(composition.clone(), &policy)
        .unwrap()
        .load()
        .unwrap();
    let scope = ScopedRequest {
        authority: composition.authority,
        resource: None,
    };
    for text in ["normal", "domain", "unused"] {
        let calls = kernel.inspect()["calls"].as_u64().unwrap();
        let fds = fs::read_dir("/proc/self/fd").unwrap().count();
        let payload = if text == "unused" {
            Value::map([])
        } else {
            Value::map([("text", Value::String(text.into()))])
        };
        let mut run = kernel
            .begin_action_scoped(
                if text == "unused" {
                    UNUSED_ACTION
                } else {
                    ACTION
                },
                &payload,
                scope.clone(),
            )
            .unwrap();
        let mut terminals = 0;
        let mut root = None;
        let mut observe = |u: &phenix_kernel::graph::GraphUpdate<'_>| {
            assert_eq!(*root.get_or_insert(u.root), u.root);
            assert!(
                !matches!(u.event, GraphEvent::NodeFinished(_)),
                "protected callback exposed payload"
            );
            if matches!(u.event, GraphEvent::Finished(_)) {
                assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
                assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds);
                terminals += 1;
            }
        };
        if text != "unused" {
            for _ in 0..2 {
                assert_eq!(run.pump(&mut observe).unwrap(), GraphStatus::Pending);
                assert_eq!(run.steps().len(), 1);
                assert_eq!(run.steps()[0].node, FAST);
                assert_eq!(
                    run.steps()[0].outcome,
                    Outcome::Success(Value::String(text.into()))
                );
                assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
                assert!(
                    !consumer.exists(),
                    "Merge delivered before all candidates settled"
                );
            }
        }
        let mut settled = false;
        for _ in 0..4 {
            match run.pump(&mut observe) {
                Err(failure) => {
                    assert_eq!(text, "normal");
                    assert_eq!(failure.code, Code::InfrastructureFailure);
                    settled = true;
                    break;
                }
                Ok(GraphStatus::Finished) => {
                    assert_ne!(text, "normal");
                    settled = true;
                    break;
                }
                Ok(status) => assert_eq!(status, GraphStatus::Pending),
            }
        }
        assert!(settled, "terminal reduction did not settle");
        assert_eq!(terminals, 1);
        assert!(!consumer.exists());
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
        // The call counter includes SlowTask's three native poll callbacks.
        assert_eq!(
            kernel.inspect()["calls"].as_u64().unwrap(),
            calls + if text == "unused" { 0 } else { 5 }
        );
        if text == "normal" {
            assert!(
                matches!(run.completion(), Some(GraphCompletion::Failure(f)) if f.code == Code::InfrastructureFailure)
            );
            assert_eq!(run.steps().len(), 2);
            assert!(run
                .steps()
                .iter()
                .all(|s| s.outcome == Outcome::Success(Value::String(text.into()))));
        } else if text == "domain" {
            assert_eq!(run.completion(), Some(GraphCompletion::Domain));
            assert_eq!(run.steps().len(), 2);
            assert!(run.steps().iter().any(|s| s.node == FIRST
                && s.outcome == Outcome::Domain(Value::String("domain fixture".into()))));
        } else {
            assert_eq!(run.completion(), Some(GraphCompletion::Success));
            assert!(run.steps().is_empty());
        }
        for _ in 0..2 {
            let result = run.pump(|_| panic!("duplicate terminal observer"));
            if text == "normal" {
                assert_eq!(result.unwrap_err().code, Code::InfrastructureFailure);
            } else {
                assert_eq!(result.unwrap(), GraphStatus::Finished);
            }
        }
    }
    kernel.shutdown().unwrap();
}
