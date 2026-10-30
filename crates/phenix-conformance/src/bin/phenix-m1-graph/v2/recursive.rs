//! Package-backed recursive preparation through the existing native dispatcher.
use super::*;
use phenix_contract::graph::v2::composition::*;
use phenix_kernel::graph::v2::recursive_effective_id;

pub(super) fn run(
    packages: &[String],
    composition: &Composition,
    policy: &TrustedCompositionPolicy,
    output: &Path,
) {
    let witness = output.join("v2-recursive-constructor");
    let consumer = output.join("v2-recursive-consumer");
    std::env::set_var("PHENIX_GRAPH_CONSTRUCTOR_WITNESS", &witness);
    std::env::set_var("PHENIX_GRAPH_B_CONSUME_WITNESS", &consumer);
    assert!(!witness.exists());
    assert!(!consumer.exists());
    let mut bodies = declarations(false, false);
    // Foreign edges belong to trusted boundary wiring, not the attested root body.
    bodies[0]
        .edges
        .retain(|edge| matches!(&edge.kind, EdgeKind::Flow { to, .. } if to.node == FIRST));
    let EdgeKind::Flow { from, to } = bodies[0].edges[0].kind.clone() else {
        panic!()
    };
    bodies[0].edges[0].kind = EdgeKind::Deferred {
        from,
        to,
        next: DeferredWave::Next,
    };
    assert!(bodies[1].sources.is_empty());
    let tail = format!("{B}/tail");
    let mut root_consumer = bodies[1].nodes[0].clone();
    root_consumer.id = tail.clone();
    bodies[1].nodes.push(root_consumer);
    let mut composition = composition.clone();
    composition.artifacts = packages
        .iter()
        .zip(&bodies)
        .enumerate()
        .map(|(i, (package, body))| {
            repack(package, &output.join(format!("v2-recursive-{i}")), body)
        })
        .collect();
    let verified: Vec<_> = composition
        .artifacts
        .iter()
        .map(|path| {
            phenix_artifact::VerifiedPackage::verify_trusted_read_only(Path::new(path)).unwrap()
        })
        .collect();
    let leaf = GraphDefinition {
        anchors: vec![],
        orders: vec![],
        id: GraphDefinitionId::new("example.graph-b/leaf").unwrap(),
        body: verified[1].graph_v2().unwrap().clone(),
        inputs: vec![BoundaryInput {
            port: InputPort {
                name: "request".into(),
                ..bodies[1].nodes[0].inputs[0].clone()
            },
            target: BoundaryTo::Local(InPortRef {
                node: SECOND.into(),
                port: "in".into(),
            }),
        }],
        outputs: vec![BoundaryOutput {
            port: OutputPort {
                name: "result".into(),
                ..bodies[1].nodes[0]
                    .outputs
                    .iter()
                    .find(|p| p.name == "ok")
                    .unwrap()
                    .clone()
            },
            source: BoundaryFrom::Local(OutPortRef {
                node: SECOND.into(),
                port: "ok".into(),
            }),
        }],
        children: vec![],
        wiring: vec![],
    };
    let middle = GraphDefinition {
        anchors: vec![],
        orders: vec![],
        id: GraphDefinitionId::new("example.graph-b/middle").unwrap(),
        body: leaf.body.clone(),
        inputs: vec![BoundaryInput {
            port: InputPort {
                name: "input".into(),
                ..leaf.inputs[0].port.clone()
            },
            target: BoundaryTo::Child {
                child: "leaf".into(),
                port: "request".into(),
            },
        }],
        outputs: vec![BoundaryOutput {
            port: OutputPort {
                name: "output".into(),
                ..leaf.outputs[0].port.clone()
            },
            source: BoundaryFrom::Child {
                child: "leaf".into(),
                port: "result".into(),
            },
        }],
        children: vec![GraphChild {
            name: "leaf".into(),
            definition: leaf.id.clone(),
        }],
        wiring: vec![],
    };
    let seed = GraphDefinition {
        anchors: vec![],
        orders: vec![],
        id: GraphDefinitionId::new("example.graph-a/seed").unwrap(),
        body: verified[0].graph_v2().unwrap().clone(),
        inputs: vec![],
        outputs: vec![BoundaryOutput {
            port: OutputPort {
                name: "value".into(),
                ..bodies[0].nodes[0]
                    .outputs
                    .iter()
                    .find(|p| p.name == "ok")
                    .unwrap()
                    .clone()
            },
            source: BoundaryFrom::Local(OutPortRef {
                node: FIRST.into(),
                port: "ok".into(),
            }),
        }],
        children: vec![],
        wiring: vec![],
    };
    let root = GraphDefinition {
        anchors: vec![],
        orders: vec![],
        id: GraphDefinitionId::new("example.graph-b/root").unwrap(),
        body: leaf.body.clone(),
        inputs: ["left", "right"]
            .map(|name| BoundaryInput {
                port: InputPort {
                    name: format!("{name}-in"),
                    ..middle.inputs[0].port.clone()
                },
                target: BoundaryTo::Child {
                    child: name.into(),
                    port: "input".into(),
                },
            })
            .into(),
        outputs: ["left", "right"]
            .map(|name| BoundaryOutput {
                port: OutputPort {
                    name: format!("{name}-out"),
                    ..middle.outputs[0].port.clone()
                },
                source: BoundaryFrom::Child {
                    child: name.into(),
                    port: "output".into(),
                },
            })
            .into(),
        children: ["left", "right", "seed"]
            .map(|name| GraphChild {
                name: name.into(),
                definition: if name == "seed" {
                    seed.id.clone()
                } else {
                    middle.id.clone()
                },
            })
            .into(),
        wiring: ["left", "right"]
            .into_iter()
            .flat_map(|name| {
                [
                    BoundaryFlow {
                        next: (name == "right").then_some(DeferredWave::Next),
                        name: format!("{name}-enter"),
                        from: BoundaryFrom::Child {
                            child: "seed".into(),
                            port: "value".into(),
                        },
                        to: BoundaryTo::Child {
                            child: name.into(),
                            port: "input".into(),
                        },
                    },
                    BoundaryFlow {
                        next: None,
                        name: format!("{name}-leave"),
                        from: BoundaryFrom::Child {
                            child: name.into(),
                            port: "output".into(),
                        },
                        to: BoundaryTo::Local(InPortRef {
                            node: if name == "left" {
                                SECOND.into()
                            } else {
                                tail.clone()
                            },
                            port: "in".into(),
                        }),
                    },
                ]
            })
            .collect(),
    };
    let instance = GraphInstanceId::new(root.id.clone());
    let effective = |owner, kind, instance: GraphInstanceId, authored: &str| {
        recursive_effective_id(
            owner,
            kind,
            &LogicalId {
                instance,
                authored: authored.into(),
            },
        )
        .unwrap()
    };
    let source = effective(A, "source", instance.child("seed").unwrap(), SOURCE);
    let first = effective(A, "node", instance.child("seed").unwrap(), FIRST);
    let children = ["left", "right"].map(|name| {
        effective(
            B,
            "node",
            instance.child(name).unwrap().child("leaf").unwrap(),
            SECOND,
        )
    });
    let consumers =
        [SECOND, tail.as_str()].map(|node| effective(B, "node", instance.clone(), node));
    assert_ne!(children[0], children[1]);
    let mut policy = policy.clone();
    policy.sources.clear();
    policy.sources.insert(source.clone(), LABEL);
    policy.topology.clear();
    policy.delivery.clear();
    for second in &children {
        policy.topology.insert((B.into(), first.clone()));
        policy.delivery.insert(
            (first.clone(), "ok".into(), second.clone(), "in".into()),
            FlowGrant {
                resources: BTreeSet::from(["example.graph.delivery".into()]),
                maximum_classification: Classification::Confidential,
            },
        );
    }
    let definitions = vec![root.clone(), middle, leaf, seed];
    let budget = ExpansionBudget {
        depth: 3,
        occurrences: 6,
        items: 128,
    };
    let expanded = lower_recursive(&definitions, &root.id, budget).unwrap();
    for (side, endpoint) in ["left", "right"].into_iter().zip(&children) {
        for (map, suffix) in [(&expanded.inputs, "in"), (&expanded.outputs, "out")] {
            let boundary = &map[&LogicalId {
                instance: instance.clone(),
                authored: format!("{side}-{suffix}"),
            }];
            assert_eq!(boundary.aliases.len(), 3);
            assert_eq!(boundary.aliases[0].instance.path().len(), 0);
            assert_eq!(boundary.aliases[1].instance.path(), [side]);
            assert_eq!(boundary.aliases[2].instance.path(), [side, "leaf"]);
            assert_eq!(
                recursive_effective_id(B, "node", &boundary.endpoint.node).unwrap(),
                *endpoint
            );
        }
    }
    let mut permuted = definitions.clone();
    for definition in &mut permuted {
        definition.children.reverse();
        definition.inputs.reverse();
        definition.outputs.reverse();
        definition.wiring.reverse();
        definition.body.nodes.reverse();
    }
    permuted.reverse();
    assert_eq!(
        expanded,
        lower_recursive(&permuted, &root.id, budget).unwrap()
    );
    let mut cases = Vec::new();
    for negative in [
        "altered-body",
        "authored-source",
        "ancestor-source",
        "authored-topology",
        "ancestor-topology",
        "delivery",
        "ancestor-delivery",
        "exact-topology",
        "deferred-delivery",
        "source-ceiling",
        "grant",
    ] {
        let mut definitions = definitions.clone();
        let mut c = composition.clone();
        let mut p = policy.clone();
        match negative {
            "altered-body" => {
                definitions[3].body.sources[0].kind = SourceKind::ExternalAction {
                    action: "example.graph.altered".into(),
                }
            }
            "authored-source" => {
                p.sources.clear();
                p.sources.insert(SOURCE.into(), LABEL);
            }
            "ancestor-source" => {
                p.sources.clear();
                p.sources
                    .insert(effective(A, "source", instance.clone(), SOURCE), LABEL);
            }
            "authored-topology" => {
                p.topology.clear();
                p.topology.insert((B.into(), FIRST.into()));
            }
            "ancestor-topology" => {
                p.topology.clear();
                p.topology
                    .insert((B.into(), effective(A, "node", instance.clone(), FIRST)));
            }
            "delivery" => p.delivery.clear(),
            "ancestor-delivery" => {
                p.delivery
                    .remove(&(first.clone(), "ok".into(), children[1].clone(), "in".into()));
                p.delivery.insert(
                    (
                        first.clone(),
                        "ok".into(),
                        effective(B, "node", instance.child("right").unwrap(), SECOND),
                        "in".into(),
                    ),
                    FlowGrant {
                        resources: BTreeSet::from(["example.graph.delivery".into()]),
                        maximum_classification: Classification::Confidential,
                    },
                );
            }
            "exact-topology" => {
                p.topology.remove(&(B.into(), first.clone()));
            }
            "deferred-delivery" => {
                p.delivery
                    .remove(&(first.clone(), "ok".into(), children[1].clone(), "in".into()));
            }
            "source-ceiling" => {
                p.source_owners.get_mut(A).unwrap().owner_ceiling = Classification::Public
            }
            "grant" => c.grants.retain(|g| g.caller != B),
            _ => unreachable!(),
        }
        assert_eq!(
            Prepared::prepare_recursive_graph_v2(c, &definitions, &root.id, budget, &p)
                .err()
                .unwrap()
                .code,
            Code::Denied,
            "{negative}"
        );
        assert!(!witness.exists(), "{negative} ran constructor");
        assert!(!consumer.exists(), "{negative} ran consumer");
        cases.push(negative);
    }
    for negative in [
        "missing-child",
        "private-input",
        "private-output",
        "alias-schema",
        "alias-readiness",
        "alias-security",
        "cycle",
        "depth",
        "occurrences",
        "alias-items",
        "unused-malformed",
    ] {
        let mut definitions = definitions.clone();
        let mut limited = budget;
        match negative {
            "missing-child" => {
                definitions[1].inputs[0].target = BoundaryTo::Child {
                    child: "missing".into(),
                    port: "request".into(),
                }
            }
            "private-input" => {
                definitions[1].inputs[0].target = BoundaryTo::Child {
                    child: "leaf".into(),
                    port: "in".into(),
                }
            }
            "private-output" => {
                definitions[1].outputs[0].source = BoundaryFrom::Child {
                    child: "leaf".into(),
                    port: "ok".into(),
                }
            }
            "alias-schema" => definitions[1].inputs[0].port.schema = ValueSchema::Bool,
            "alias-readiness" => definitions[1].inputs[0].port.readiness = InputReadiness::Optional,
            "alias-security" => {
                definitions[1].outputs[0].port.security.confidentiality = Classification::Public
            }
            "cycle" => definitions[1].children[0].definition = root.id.clone(),
            "depth" => limited.depth = 2,
            "occurrences" => limited.occurrences = 5,
            "alias-items" => limited.items = 20,
            "unused-malformed" => {
                let mut unused = definitions[1].clone();
                unused.id = GraphDefinitionId::new("example.graph-b/unused").unwrap();
                unused.outputs[0].source = BoundaryFrom::Child {
                    child: "leaf".into(),
                    port: "private".into(),
                };
                definitions.push(unused);
            }
            _ => unreachable!(),
        }
        assert!(
            Prepared::prepare_recursive_graph_v2(
                composition.clone(),
                &definitions,
                &root.id,
                limited,
                &policy
            )
            .is_err(),
            "{negative}"
        );
        assert!(!witness.exists(), "{negative} ran constructor");
        assert!(!consumer.exists(), "{negative} ran consumer");
        cases.push(negative);
    }
    let prepared = Prepared::prepare_recursive_graph_v2(
        composition.clone(),
        &definitions,
        &root.id,
        budget,
        &policy,
    )
    .unwrap();
    assert!(!witness.exists(), "preparation ran constructor");
    let kernel = prepared.load().unwrap();
    assert!(witness.exists());
    let payload = Value::map([("text", Value::String("recursive-native".into()))]);
    let mut execution = kernel
        .begin_action_scoped(
            ACTION,
            &payload,
            ScopedRequest {
                authority: composition.authority.clone(),
                resource: None,
            },
        )
        .unwrap();
    let mut terminals = 0;
    let mut status = GraphStatus::Pending;
    // Deferred admission happens at pump settlement, so later waves need pumps.
    for _ in 0..8 {
        status = execution
            .pump(|update| {
                assert!(
                    !matches!(update.event, GraphEvent::NodeFinished(_)),
                    "protected payload exposed"
                );
                if matches!(update.event, GraphEvent::Finished(_)) {
                    terminals += 1;
                    assert_eq!(terminals, 1, "duplicate recursive terminal");
                }
            })
            .unwrap();
        if status == GraphStatus::Finished {
            break;
        }
        assert_eq!(status, GraphStatus::Pending);
        assert_eq!(terminals, 0, "recursive terminal before wave settlement");
    }
    assert_eq!(
        status,
        GraphStatus::Finished,
        "recursive pump budget exhausted"
    );
    assert_eq!(terminals, 1);
    assert_eq!(execution.completion(), Some(GraphCompletion::Success));
    assert_eq!(execution.steps().len(), 5);
    assert_eq!(execution.steps()[0].node, first);
    assert_eq!(
        execution.steps()[0].outcome,
        Outcome::Success(Value::String("recursive-native".into()))
    );
    let actual: BTreeSet<_> = execution.steps()[1..]
        .iter()
        .map(|step| {
            let expected = if children.contains(&step.node) {
                "consumed:recursive-native"
            } else {
                assert!(consumers.contains(&step.node));
                "consumed:consumed:recursive-native"
            };
            assert_eq!(
                step.outcome,
                Outcome::Success(Value::String(expected.into()))
            );
            step.node.clone()
        })
        .collect();
    assert_eq!(
        actual,
        children
            .into_iter()
            .chain(consumers)
            .collect::<BTreeSet<_>>()
    );
    assert_eq!(
        fs::read_to_string(&consumer).unwrap(),
        "called\ncalled\ncalled\ncalled\n"
    );
    assert_eq!(kernel.inspect()["calls"], 5);
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
    assert_eq!(
        execution
            .pump(|_| panic!("duplicate recursive event"))
            .unwrap(),
        GraphStatus::Finished
    );
    assert_eq!(terminals, 1);
    drop(execution);
    kernel.shutdown().unwrap();
    cases.push("repeated-source-free-child-native-execution");
    cases.push("internal-and-cross-boundary-deferred-next-native-execution");
    cases.push("three-level-input-output-aliases-repeated-middle-native-execution");
    order_acceptance(packages, &composition, &policy, output);
    each_acceptance(packages, &composition, &policy, output);
    cases.extend([
        "recursive-each-original-owner-route-and-effective-grant-denials",
        "recursive-each-repeated-native-branches-collectors-and-global-provider-drain",
    ]);
    cases.extend([
        "scoped-public-private-anchor-denials-before-constructors",
        "cross-boundary-order-held-a-ready-b",
        "order-control-distinct-provider-not-busy",
        "order-without-flow-does-not-activate",
    ]);
    fs::write(output.join("graph-v2-recursive.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "format": "phenix.graph-v2-recursive.v1", "full_V2": false, "p1_parity": false,
        "cases": cases,
        "limits": ["finite roots with repeated source-free occurrences", "no repeated action sources, recursive cycles, patch application or P1 parity"]
    })).unwrap()).unwrap();
    std::env::set_var(
        "PHENIX_GRAPH_CONSTRUCTOR_WITNESS",
        output.join("v2-constructor-witness"),
    );
    std::env::set_var(
        "PHENIX_GRAPH_B_CONSUME_WITNESS",
        output.join("v2-consumer-witness"),
    );
}

fn order_acceptance(
    packages: &[String],
    composition: &Composition,
    policy: &TrustedCompositionPolicy,
    output: &Path,
) {
    const SLOW: &str = "example.graph.slow@1/run";
    let held = format!("{A}/held");
    for mode in ["ordered", "unordered", "no-flow"] {
        let control = output.join(format!("recursive-order-{mode}.control"));
        let witness = output.join(format!("recursive-order-{mode}.constructor"));
        let consumer = output.join(format!("recursive-order-{mode}.consumer"));
        fs::write(&control, "hold").unwrap();
        std::env::set_var("PHENIX_GRAPH_A_EACH_CONTROL", &control);
        std::env::set_var("PHENIX_GRAPH_CONSTRUCTOR_WITNESS", &witness);
        std::env::set_var("PHENIX_GRAPH_B_CONSUME_WITNESS", &consumer);
        assert!(!witness.exists());
        assert!(!consumer.exists());
        let mut bodies = declarations(false, false);
        bodies[0]
            .edges
            .retain(|e| matches!(&e.kind, EdgeKind::Flow { to, .. } if to.node == FIRST));
        let mut slow = bodies[0].nodes[0].clone();
        slow.id = held.clone();
        slow.kind = NodeKind::Invoke {
            selector: String::new(),
            operation: SLOW.into(),
        };
        bodies[0].nodes.push(slow);
        bodies[0].edges.extend([
            EdgeDecl {
                id: format!("{A}/slow-input"),
                kind: EdgeKind::Flow {
                    from: OutPortRef {
                        node: SOURCE.into(),
                        port: "out".into(),
                    },
                    to: InPortRef {
                        node: held.clone(),
                        port: "in".into(),
                    },
                },
            },
            EdgeDecl {
                id: format!("{A}/task-before-slow"),
                kind: EdgeKind::Order {
                    before: FIRST.into(),
                    after: held.clone(),
                },
            },
        ]);
        let leaf = GraphDefinition {
            id: GraphDefinitionId::new("example.graph-b/order-leaf").unwrap(),
            body: bodies[1].clone(),
            inputs: vec![BoundaryInput {
                port: bodies[1].nodes[0].inputs[0].clone(),
                target: BoundaryTo::Local(InPortRef {
                    node: SECOND.into(),
                    port: "in".into(),
                }),
            }],
            outputs: vec![],
            children: vec![],
            wiring: vec![],
            orders: vec![],
            anchors: vec![
                GraphAnchor {
                    name: "ready".into(),
                    visibility: AnchorVisibility::Public,
                    target: AnchorTarget::LocalNode(SECOND.into()),
                },
                GraphAnchor {
                    name: "hidden".into(),
                    visibility: AnchorVisibility::Private,
                    target: AnchorTarget::LocalNode(SECOND.into()),
                },
            ],
        };
        let middle = GraphDefinition {
            id: GraphDefinitionId::new("example.graph-b/order-middle").unwrap(),
            body: leaf.body.clone(),
            inputs: vec![BoundaryInput {
                port: leaf.inputs[0].port.clone(),
                target: BoundaryTo::Child {
                    child: "leaf".into(),
                    port: "in".into(),
                },
            }],
            outputs: vec![],
            children: vec![GraphChild {
                name: "leaf".into(),
                definition: leaf.id.clone(),
            }],
            wiring: vec![],
            orders: vec![],
            anchors: vec![GraphAnchor {
                name: "ready".into(),
                visibility: AnchorVisibility::Public,
                target: AnchorTarget::Child {
                    child: "leaf".into(),
                    anchor: "ready".into(),
                },
            }],
        };
        let root = GraphDefinition {
            id: GraphDefinitionId::new("example.graph-a/order-root").unwrap(),
            body: bodies[0].clone(),
            inputs: vec![],
            outputs: vec![],
            children: ["left", "right"]
                .map(|name| GraphChild {
                    name: name.into(),
                    definition: middle.id.clone(),
                })
                .into(),
            wiring: if mode == "no-flow" {
                vec![]
            } else {
                vec![BoundaryFlow {
                    name: "input".into(),
                    next: None,
                    from: BoundaryFrom::Local(OutPortRef {
                        node: FIRST.into(),
                        port: "ok".into(),
                    }),
                    to: BoundaryTo::Child {
                        child: "left".into(),
                        port: "in".into(),
                    },
                }]
            },
            anchors: vec![
                GraphAnchor {
                    name: "task".into(),
                    visibility: AnchorVisibility::Private,
                    target: AnchorTarget::LocalNode(FIRST.into()),
                },
                GraphAnchor {
                    name: "held".into(),
                    visibility: AnchorVisibility::Private,
                    target: AnchorTarget::LocalNode(held.clone()),
                },
                GraphAnchor {
                    name: "selected".into(),
                    visibility: AnchorVisibility::Public,
                    target: AnchorTarget::Child {
                        child: "left".into(),
                        anchor: "ready".into(),
                    },
                },
            ],
            orders: std::iter::once(BoundaryOrder {
                name: "local".into(),
                before: AnchorRef::Local("task".into()),
                after: AnchorRef::Local("held".into()),
            })
            .chain(if mode == "unordered" {
                vec![]
            } else {
                vec![
                    BoundaryOrder {
                        name: "wait-left".into(),
                        before: AnchorRef::Local("held".into()),
                        after: AnchorRef::Local("selected".into()),
                    },
                    BoundaryOrder {
                        name: "wait-right".into(),
                        before: AnchorRef::Local("held".into()),
                        after: AnchorRef::Child {
                            child: "right".into(),
                            anchor: "ready".into(),
                        },
                    },
                ]
            })
            .collect(),
        };
        let definitions = vec![root.clone(), middle, leaf];
        let budget = ExpansionBudget {
            depth: 3,
            occurrences: 5,
            items: 128,
        };
        let expanded = lower_recursive(&definitions, &root.id, budget).unwrap();
        let scope = GraphInstanceId::new(root.id.clone());
        let selected = expanded
            .resolve_anchor(&scope, &AnchorRef::Local("selected".into()))
            .unwrap();
        assert_eq!(selected.aliases.len(), 3);
        assert_eq!(selected.node.instance.path(), ["left", "leaf"]);
        let second = recursive_effective_id(B, "node", &selected.node).unwrap();
        let private = expanded
            .resolve_anchor(&scope, &AnchorRef::Local("held".into()))
            .unwrap();
        assert_eq!(private.visibility, AnchorVisibility::Private);
        let first = recursive_effective_id(
            A,
            "node",
            &LogicalId {
                instance: scope.clone(),
                authored: FIRST.into(),
            },
        )
        .unwrap();
        let before = recursive_effective_id(A, "node", &private.node).unwrap();
        let right = expanded
            .resolve_anchor(
                &scope,
                &AnchorRef::Child {
                    child: "right".into(),
                    anchor: "ready".into(),
                },
            )
            .unwrap();
        assert_ne!(selected.node, right.node);
        if mode != "unordered" {
            let provenance = &expanded.orders[&LogicalId {
                instance: scope.clone(),
                authored: "wait-left".into(),
            }];
            assert_eq!(provenance.before, private.aliases);
            assert_eq!(provenance.after, selected.aliases);
        }
        let mut c = composition.clone();
        c.artifacts = bodies
            .iter()
            .enumerate()
            .map(|(i, body)| {
                repack(
                    &packages[i],
                    &output.join(format!("recursive-order-{mode}-{i}")),
                    body,
                )
            })
            .collect();
        c.authority.operations.insert(SLOW.into());
        c.grants.push(ScopedGrant {
            caller: A.into(),
            interface: "example.graph.slow@1".into(),
            operation: "run".into(),
            resources: c.authority.resources.clone(),
        });
        let mut p = policy.clone();
        p.sources.clear();
        p.topology.clear();
        p.delivery.clear();
        p.sources.insert(
            recursive_effective_id(
                A,
                "source",
                &LogicalId {
                    instance: scope.clone(),
                    authored: SOURCE.into(),
                },
            )
            .unwrap(),
            LABEL,
        );
        for node in [&selected.node, &right.node] {
            p.topology
                .insert((A.into(), recursive_effective_id(B, "node", node).unwrap()));
        }
        p.delivery.insert(
            (first.clone(), "ok".into(), second.clone(), "in".into()),
            FlowGrant {
                resources: BTreeSet::from(["example.graph.delivery".into()]),
                maximum_classification: Classification::Confidential,
            },
        );
        p.operations.insert(
            (A.into(), SLOW.into()),
            OperationSecurity {
                input: LABEL,
                ok: LABEL,
                err: LABEL,
            },
        );
        if mode == "ordered" {
            for negative in [
                "private-child",
                "hidden-reexport",
                "missing-anchor",
                "raw-node",
                "foreign-node",
                "unknown-child",
                "descendant-path",
                "owner",
                "reference-cycle",
                "order-cycle",
                "depth",
                "items",
                "topology",
                "order-only-topology",
                "delivery",
                "operation",
                "body",
            ] {
                let mut d = definitions.clone();
                let mut denied = p.clone();
                let mut limited = budget;
                match negative {
                    "private-child" => d[1].orders.push(BoundaryOrder {
                        name: "hidden".into(),
                        before: AnchorRef::Child {
                            child: "leaf".into(),
                            anchor: "hidden".into(),
                        },
                        after: AnchorRef::Local("ready".into()),
                    }),
                    "hidden-reexport" => {
                        d[1].anchors[0].target = AnchorTarget::Child {
                            child: "leaf".into(),
                            anchor: "hidden".into(),
                        }
                    }
                    "missing-anchor" => d[0].orders[1].after = AnchorRef::Local("missing".into()),
                    "raw-node" => d[0].orders[1].after = AnchorRef::Local(SECOND.into()),
                    "foreign-node" => {
                        d[0].anchors[0].target = AnchorTarget::LocalNode(SECOND.into())
                    }
                    "unknown-child" => {
                        d[0].anchors[2].target = AnchorTarget::Child {
                            child: "missing".into(),
                            anchor: "ready".into(),
                        }
                    }
                    "descendant-path" => {
                        d[0].anchors[2].target = AnchorTarget::Child {
                            child: "left/leaf".into(),
                            anchor: "ready".into(),
                        }
                    }
                    "owner" => {
                        d[0].id = GraphDefinitionId::new("example.graph-b/wrong-owner").unwrap()
                    }
                    "reference-cycle" => d[1].children[0].definition = root.id.clone(),
                    "order-cycle" => d[0].orders.push(BoundaryOrder {
                        name: "cycle".into(),
                        before: AnchorRef::Local("selected".into()),
                        after: AnchorRef::Local("task".into()),
                    }),
                    "depth" => limited.depth = 2,
                    "items" => limited.items = 1,
                    "topology" => denied.topology.clear(),
                    "order-only-topology" => {
                        denied.topology.remove(&(
                            A.into(),
                            recursive_effective_id(B, "node", &right.node).unwrap(),
                        ));
                    }
                    "delivery" => denied.delivery.clear(),
                    "operation" => {
                        denied.operations.remove(&(A.into(), SLOW.into()));
                    }
                    "body" => d[0]
                        .body
                        .edges
                        .retain(|e| !matches!(e.kind, EdgeKind::Order { .. })),
                    _ => unreachable!(),
                }
                assert!(
                    Prepared::prepare_recursive_graph_v2(c.clone(), &d, &root.id, limited, &denied)
                        .is_err(),
                    "{negative}"
                );
                assert!(!witness.exists(), "{negative} ran constructor");
                assert!(!consumer.exists(), "{negative} ran consumer");
                assert!(
                    !control.with_extension("calls").exists(),
                    "{negative} ran SlowTask"
                );
            }
        }
        let prepared =
            Prepared::prepare_recursive_graph_v2(c.clone(), &definitions, &root.id, budget, &p)
                .unwrap();
        assert!(!witness.exists());
        let kernel = prepared.load().unwrap();
        assert!(witness.exists());
        let mut execution = kernel
            .begin_action_scoped(
                ACTION,
                &Value::map([("text", Value::String("order-native".into()))]),
                ScopedRequest {
                    authority: c.authority.clone(),
                    resource: None,
                },
            )
            .unwrap();
        let mut terminals = 0;
        let mut observe = |u: &phenix_kernel::graph::GraphUpdate<'_>| {
            assert!(
                !matches!(u.event, GraphEvent::NodeFinished(_)),
                "protected order payload exposed"
            );
            if matches!(u.event, GraphEvent::Finished(_)) {
                terminals += 1;
                assert_eq!(terminals, 1);
            }
        };
        for _ in 0..8 {
            assert_eq!(execution.pump(&mut observe).unwrap(), GraphStatus::Pending);
        }
        assert_eq!(
            fs::read_to_string(control.with_extension("calls")).unwrap(),
            "order-native\n"
        );
        assert!(execution.steps().iter().any(|s| s.node == first));
        assert!(!execution.steps().iter().any(|s| s.node == before));
        assert!(
            kernel.inspect()["pending"]["physical_leases"]
                .as_u64()
                .unwrap()
                > 0
        );
        if mode == "unordered" {
            assert_eq!(fs::read_to_string(&consumer).unwrap(), "called\n");
        } else {
            assert!(!consumer.exists(), "Order activated or failed to block B");
        }
        fs::write(&control, "release").unwrap();
        let mut status = GraphStatus::Pending;
        for _ in 0..16 {
            status = execution.pump(&mut observe).unwrap();
            if status == GraphStatus::Finished {
                break;
            }
            assert_eq!(status, GraphStatus::Pending);
        }
        assert_eq!(status, GraphStatus::Finished);
        assert_eq!(execution.completion(), Some(GraphCompletion::Success));
        assert_eq!(terminals, 1);
        assert_eq!(
            execution.steps().len(),
            if mode == "no-flow" { 2 } else { 3 }
        );
        let held_index = execution
            .steps()
            .iter()
            .position(|s| s.node == before)
            .unwrap();
        if mode == "no-flow" {
            assert!(!consumer.exists());
        } else {
            assert_eq!(fs::read_to_string(&consumer).unwrap(), "called\n");
            let consumer_index = execution
                .steps()
                .iter()
                .position(|s| s.node == second)
                .unwrap();
            assert_eq!(held_index < consumer_index, mode == "ordered");
            assert_eq!(
                execution.steps()[consumer_index].outcome,
                Outcome::Success(Value::String("consumed:order-native".into()))
            );
        }
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
        assert_eq!(
            execution
                .pump(|_| panic!("duplicate Order terminal"))
                .unwrap(),
            GraphStatus::Finished
        );
        drop(execution);
        kernel.shutdown().unwrap();
    }
    std::env::remove_var("PHENIX_GRAPH_A_EACH_CONTROL");
}

fn each_acceptance(
    packages: &[String],
    composition: &Composition,
    policy: &TrustedCompositionPolicy,
    output: &Path,
) {
    const SLOW: &str = "example.graph.slow@1/run";
    const COLLECT: &str = "example.graph.each-collect@1/run";
    let each_node = format!("{B}/each");
    let controls = [
        output.join("recursive-each-a.control"),
        output.join("recursive-each-b.control"),
    ];
    let witness = output.join("recursive-each.constructor");
    std::env::set_var("PHENIX_GRAPH_CONSTRUCTOR_WITNESS", &witness);
    for (key, path) in ["PHENIX_GRAPH_A_EACH_CONTROL", "PHENIX_GRAPH_B_EACH_CONTROL"]
        .into_iter()
        .zip(&controls)
    {
        std::env::set_var(key, path);
        fs::write(path, "hold").unwrap();
        assert!(!path.with_extension("calls").exists());
    }
    assert!(!witness.exists());
    let (batch, collect) = super::each::profile_schemas(&packages[1]);
    let mut bodies = declarations(false, false);
    bodies[0].sources[0].payload = batch.clone();
    bodies[0]
        .nodes
        .retain(|n| matches!(n.kind, NodeKind::Source { .. }));
    bodies[0].nodes[0].outputs[0].schema = batch.clone();
    bodies[0].edges.clear();
    let mut collector = bodies[1].nodes[0].clone();
    collector.kind = NodeKind::Invoke {
        selector: String::new(),
        operation: COLLECT.into(),
    };
    collector.inputs[0].schema = collect.request.clone();
    for port in &mut collector.outputs {
        port.schema = if port.name == "ok" {
            collect.response.clone()
        } else {
            collect.error.clone()
        };
    }
    let input = InputPort {
        name: "in".into(),
        schema: batch.clone(),
        readiness: InputReadiness::One,
        security: LABEL,
    };
    let each = NodeDecl {
        id: each_node.clone(),
        kind: NodeKind::InvokeEach {
            collection: "Workers".into(),
            operation: SLOW.into(),
            include_self: true,
            max: 2,
        },
        inputs: vec![input.clone()],
        outputs: vec![OutputPort {
            name: "out".into(),
            schema: collect.request.clone(),
            security: LABEL,
        }],
        execution: collector.execution,
    };
    bodies[1].nodes = vec![each, collector];
    bodies[1].edges = vec![EdgeDecl {
        id: format!("{B}/each-exit"),
        kind: EdgeKind::Flow {
            from: OutPortRef {
                node: each_node.clone(),
                port: "out".into(),
            },
            to: InPortRef {
                node: SECOND.into(),
                port: "in".into(),
            },
        },
    }];
    let child = GraphDefinition {
        id: GraphDefinitionId::new("example.graph-b/each-child").unwrap(),
        body: bodies[1].clone(),
        inputs: vec![BoundaryInput {
            port: InputPort {
                name: "batch".into(),
                ..input
            },
            target: BoundaryTo::Local(InPortRef {
                node: each_node.clone(),
                port: "in".into(),
            }),
        }],
        outputs: vec![BoundaryOutput {
            port: OutputPort {
                name: "result".into(),
                schema: collect.response.clone(),
                security: LABEL,
            },
            source: BoundaryFrom::Local(OutPortRef {
                node: SECOND.into(),
                port: "ok".into(),
            }),
        }],
        anchors: vec![
            GraphAnchor {
                name: "each".into(),
                visibility: AnchorVisibility::Public,
                target: AnchorTarget::LocalNode(each_node.clone()),
            },
            GraphAnchor {
                name: "private-each".into(),
                visibility: AnchorVisibility::Private,
                target: AnchorTarget::LocalNode(each_node.clone()),
            },
            GraphAnchor {
                name: "collect".into(),
                visibility: AnchorVisibility::Public,
                target: AnchorTarget::LocalNode(SECOND.into()),
            },
        ],
        children: vec![],
        wiring: vec![],
        orders: vec![],
    };
    let root = GraphDefinition {
        id: GraphDefinitionId::new("example.graph-a/each-root").unwrap(),
        body: bodies[0].clone(),
        inputs: vec![BoundaryInput {
            port: child.inputs[0].port.clone(),
            target: BoundaryTo::Child {
                child: "right".into(),
                port: "batch".into(),
            },
        }],
        outputs: vec![BoundaryOutput {
            port: child.outputs[0].port.clone(),
            source: BoundaryFrom::Child {
                child: "left".into(),
                port: "result".into(),
            },
        }],
        children: ["left", "right"]
            .map(|name| GraphChild {
                name: name.into(),
                definition: child.id.clone(),
            })
            .into(),
        wiring: ["left", "right"]
            .map(|name| BoundaryFlow {
                name: name.into(),
                next: None,
                from: BoundaryFrom::Local(OutPortRef {
                    node: SOURCE.into(),
                    port: "out".into(),
                }),
                to: BoundaryTo::Child {
                    child: name.into(),
                    port: "batch".into(),
                },
            })
            .into(),
        anchors: vec![GraphAnchor {
            name: "selected".into(),
            visibility: AnchorVisibility::Public,
            target: AnchorTarget::Child {
                child: "left".into(),
                anchor: "each".into(),
            },
        }],
        orders: vec![
            BoundaryOrder {
                name: "collect-order".into(),
                before: AnchorRef::Child {
                    child: "left".into(),
                    anchor: "collect".into(),
                },
                after: AnchorRef::Child {
                    child: "right".into(),
                    anchor: "collect".into(),
                },
            },
            BoundaryOrder {
                name: "each-order".into(),
                before: AnchorRef::Local("selected".into()),
                after: AnchorRef::Child {
                    child: "right".into(),
                    anchor: "collect".into(),
                },
            },
        ],
    };
    let definitions = vec![root.clone(), child];
    let budget = ExpansionBudget {
        depth: 2,
        occurrences: 3,
        items: 128,
    };
    let expanded = lower_recursive(&definitions, &root.id, budget).unwrap();
    let scope = GraphInstanceId::new(root.id.clone());
    let effective = |owner, kind, instance: GraphInstanceId, authored: &str| {
        recursive_effective_id(
            owner,
            kind,
            &LogicalId {
                instance,
                authored: authored.into(),
            },
        )
        .unwrap()
    };
    let source = effective(A, "node", scope.clone(), SOURCE);
    let each_ids =
        ["left", "right"].map(|name| effective(B, "node", scope.child(name).unwrap(), &each_node));
    let collectors =
        ["left", "right"].map(|name| effective(B, "node", scope.child(name).unwrap(), SECOND));
    assert_ne!(each_ids[0], each_ids[1]);
    assert_ne!(collectors[0], collectors[1]);
    let selected = expanded
        .resolve_anchor(&scope, &AnchorRef::Local("selected".into()))
        .unwrap();
    assert_eq!(selected.aliases.len(), 2);
    assert_eq!(
        expanded.orders[&LogicalId {
            instance: scope.clone(),
            authored: "each-order".into()
        }]
            .before,
        selected.aliases
    );
    assert_eq!(
        recursive_effective_id(B, "node", &selected.node).unwrap(),
        each_ids[0]
    );
    assert_eq!(
        expanded.inputs[&LogicalId {
            instance: scope.clone(),
            authored: "batch".into()
        }]
            .endpoint
            .node
            .instance
            .path(),
        ["right"]
    );
    assert_eq!(
        expanded.outputs[&LogicalId {
            instance: scope.clone(),
            authored: "result".into()
        }]
            .endpoint
            .node
            .instance
            .path(),
        ["left"]
    );
    let mut permuted = definitions.clone();
    permuted.reverse();
    for d in &mut permuted {
        d.children.reverse();
        d.wiring.reverse();
        d.anchors.reverse();
        d.body.nodes.reverse();
    }
    assert_eq!(
        expanded,
        lower_recursive(&permuted, &root.id, budget).unwrap()
    );
    let mut c = composition.clone();
    c.artifacts = bodies
        .iter()
        .enumerate()
        .map(|(i, body)| {
            repack(
                &packages[i],
                &output.join(format!("recursive-each-{i}")),
                body,
            )
        })
        .collect();
    // A required Many self-binding would form a startup cycle. Self is instead
    // the explicit attested include_self route, after the selected A route.
    c.slots = vec![SlotBinding {
        caller: B.into(),
        slot: "Workers".into(),
        index: 0,
        provider: A.into(),
        export: "slow".into(),
    }];
    for key in [SLOW, COLLECT] {
        c.authority.operations.insert(key.into());
        let (interface, operation) = key.rsplit_once('/').unwrap();
        c.grants.push(ScopedGrant {
            caller: B.into(),
            interface: interface.into(),
            operation: operation.into(),
            resources: c.authority.resources.clone(),
        });
    }
    let mut p = policy.clone();
    p.sources.clear();
    p.topology.clear();
    p.delivery.clear();
    p.sources
        .insert(effective(A, "source", scope.clone(), SOURCE), LABEL);
    for (owner, key) in [(A, SLOW), (B, SLOW), (B, COLLECT)] {
        p.operations.insert(
            (owner.into(), key.into()),
            OperationSecurity {
                input: LABEL,
                ok: LABEL,
                err: LABEL,
            },
        );
    }
    for i in 0..2 {
        for node in [&each_ids[i], &collectors[i]] {
            p.topology.insert((A.into(), node.clone()));
        }
        for (from, to) in [(&source, &each_ids[i]), (&each_ids[i], &collectors[i])] {
            p.delivery.insert(
                (from.clone(), "out".into(), to.clone(), "in".into()),
                FlowGrant {
                    resources: c.authority.resources.clone(),
                    maximum_classification: Classification::Confidential,
                },
            );
        }
    }
    for (i, key) in p.delivery.keys().enumerate() {
        let mut missing = p.clone();
        missing.delivery.remove(key);
        let failure = Prepared::prepare_recursive_graph_v2(
            c.clone(),
            &definitions,
            &root.id,
            budget,
            &missing,
        )
        .err()
        .unwrap();
        assert_eq!(
            failure.code,
            Code::Denied,
            "Each effective entry/exit {i}: {key:?}"
        );
        assert_eq!(
            failure.message,
            "cross-plugin flow lacks exact delivery grant"
        );
        assert!(!witness.exists());
    }
    for negative in [
        "foreign-owner-grant",
        "collector-owner-grant",
        "route-a-attestation",
        "route-b-attestation",
        "route-security",
        "route-schema",
        "collection-namespace",
        "source-namespace",
        "topology-namespace",
        "delivery-namespace",
        "body",
        "self-binding-cycle",
        "dag-cycle",
        "depth",
        "occurrences",
        "items",
    ] {
        let mut composition = c.clone();
        let mut defs = definitions.clone();
        let mut policy = p.clone();
        let mut limited = budget;
        match negative {
            "foreign-owner-grant" | "collector-owner-grant" => {
                let key = if negative == "foreign-owner-grant" {
                    SLOW
                } else {
                    COLLECT
                };
                let (interface, operation) = key.rsplit_once('/').unwrap();
                composition.grants.retain(|g| {
                    !(g.caller == B && g.interface == interface && g.operation == operation)
                });
                if key == SLOW {
                    composition.grants.push(ScopedGrant {
                        caller: A.into(),
                        interface: interface.into(),
                        operation: operation.into(),
                        resources: c.authority.resources.clone(),
                    });
                }
            }
            "route-a-attestation" => {
                policy.operations.remove(&(A.into(), SLOW.into()));
            }
            "route-b-attestation" => {
                policy.operations.remove(&(B.into(), SLOW.into()));
            }
            "route-security" => {
                policy
                    .operations
                    .get_mut(&(A.into(), SLOW.into()))
                    .unwrap()
                    .input
                    .confidentiality = Classification::Secret
            }
            "route-schema" => {
                let wrong = ValueSchema::List(Box::new(ValueSchema::Struct(BTreeMap::from([
                    ("route".into(), ValueSchema::U64),
                    ("input".into(), ValueSchema::Bool),
                ]))));
                defs[0].body.sources[0].payload = wrong.clone();
                defs[0].body.nodes[0].outputs[0].schema = wrong.clone();
                defs[0].inputs[0].port.schema = wrong.clone();
                defs[1].inputs[0].port.schema = wrong.clone();
                defs[1]
                    .body
                    .nodes
                    .iter_mut()
                    .find(|n| n.id == each_node)
                    .unwrap()
                    .inputs[0]
                    .schema = wrong;
                composition.artifacts = defs
                    .iter()
                    .enumerate()
                    .map(|(i, d)| {
                        repack(
                            &packages[i],
                            &output.join(format!("recursive-each-schema-{i}")),
                            &d.body,
                        )
                    })
                    .collect();
            }
            "collection-namespace" => {
                let NodeKind::InvokeEach { collection, .. } = &mut defs[1]
                    .body
                    .nodes
                    .iter_mut()
                    .find(|n| n.id == each_node)
                    .unwrap()
                    .kind
                else {
                    panic!()
                };
                *collection = "ParentWorkers".into();
                composition.artifacts[1] = repack(
                    &packages[1],
                    &output.join("recursive-each-namespace-b"),
                    &defs[1].body,
                );
            }
            "source-namespace" => {
                policy.sources.clear();
                policy.sources.insert(SOURCE.into(), LABEL);
            }
            "topology-namespace" => {
                policy.topology.clear();
                policy.topology.insert((A.into(), each_node.clone()));
            }
            "delivery-namespace" => {
                policy.delivery.clear();
                policy.delivery.insert(
                    (SOURCE.into(), "out".into(), each_node.clone(), "in".into()),
                    FlowGrant {
                        resources: c.authority.resources.clone(),
                        maximum_classification: Classification::Confidential,
                    },
                );
            }
            "body" => {
                let NodeKind::InvokeEach { max, .. } = &mut defs[1]
                    .body
                    .nodes
                    .iter_mut()
                    .find(|n| n.id == each_node)
                    .unwrap()
                    .kind
                else {
                    panic!()
                };
                *max = 3;
            }
            "self-binding-cycle" => composition.slots.push(SlotBinding {
                caller: B.into(),
                slot: "Workers".into(),
                index: 1,
                provider: B.into(),
                export: "slow".into(),
            }),
            "dag-cycle" => defs[0].orders.push(BoundaryOrder {
                name: "cycle".into(),
                before: AnchorRef::Child {
                    child: "right".into(),
                    anchor: "collect".into(),
                },
                after: AnchorRef::Local("selected".into()),
            }),
            "depth" => limited.depth = 1,
            "occurrences" => limited.occurrences = 2,
            "items" => limited.items = 1,
            _ => unreachable!(),
        }
        let failure =
            Prepared::prepare_recursive_graph_v2(composition, &defs, &root.id, limited, &policy)
                .err()
                .unwrap();
        if matches!(negative, "foreign-owner-grant" | "collector-owner-grant") {
            assert_eq!(failure.code, Code::Denied);
            assert_eq!(failure.message, "graph-v2 operation lacks prepared grant");
        }
        if negative == "body" {
            assert_eq!(
                failure.message,
                "graph-v2 declaration differs from verified package metadata"
            );
        }
        if matches!(negative, "route-a-attestation" | "route-b-attestation") {
            assert_eq!(failure.message, "missing InvokeEach operation attestation");
        }
        if negative == "route-security" {
            assert_eq!(failure.message, "InvokeEach operation security mismatch");
        }
        if negative == "dag-cycle" {
            assert_eq!(failure.message, "recursive dependency cycle");
        }
        if negative == "route-schema" {
            assert_eq!(failure.code, Code::SchemaRejected);
            assert_eq!(failure.message, "InvokeEach route schemas differ");
        }
        if negative == "collection-namespace" {
            assert_eq!(
                failure.message,
                "InvokeEach requires a declared Many collection"
            );
        }
        assert!(!witness.exists(), "{negative} ran constructor");
        for control in &controls {
            assert!(
                !control.with_extension("calls").exists(),
                "{negative} ran native work"
            );
        }
    }
    let prepared =
        Prepared::prepare_recursive_graph_v2(c.clone(), &permuted, &root.id, budget, &p).unwrap();
    assert!(!witness.exists());
    let kernel = prepared.load().unwrap();
    assert!(witness.exists());
    let fds = fs::read_dir("/proc/self/fd").unwrap().count();
    let payload = Value::List(
        ["first", "domain"]
            .into_iter()
            .enumerate()
            .map(|(route, text)| {
                Value::map([
                    ("route", Value::U64(route as u64)),
                    ("input", Value::map([("text", Value::String(text.into()))])),
                ])
            })
            .collect(),
    );
    let mut run = kernel
        .begin_action_scoped(
            ACTION,
            &payload,
            ScopedRequest {
                authority: c.authority.clone(),
                resource: None,
            },
        )
        .unwrap();
    let calls =
        |i: usize| fs::read_to_string(controls[i].with_extension("calls")).unwrap_or_default();
    let terminals = std::cell::Cell::new(0);
    let mut observe = |u: &phenix_kernel::graph::GraphUpdate<'_>| {
        assert!(
            !matches!(u.event, GraphEvent::NodeFinished(_)),
            "protected Each payload exposed"
        );
        if matches!(u.event, GraphEvent::Finished(_)) {
            assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
            terminals.set(terminals.get() + 1);
        }
    };
    for _ in 0..4 {
        assert_eq!(run.pump(&mut observe).unwrap(), GraphStatus::Pending);
    }
    assert_eq!([calls(0), calls(1)], ["first\n", "domain\n"]);
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 2);
    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds + 2);
    assert!(run.steps().is_empty());
    assert!(kernel.shutdown().is_err());
    fs::write(&controls[1], "release").unwrap();
    for _ in 0..16 {
        assert_eq!(run.pump(&mut observe).unwrap(), GraphStatus::Pending);
        assert!(
            kernel.inspect()["pending"]["physical_leases"]
                .as_u64()
                .unwrap()
                <= 2
        );
        if run.steps().len() == 2 && calls(1) == "domain\ndomain\n" {
            break;
        }
    }
    assert_eq!([calls(0), calls(1)], ["first\n", "domain\ndomain\n"]);
    assert_eq!(run.steps().len(), 2);
    assert_eq!(
        run.steps()
            .iter()
            .map(|s| s.node.clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(each_ids.clone())
    );
    assert!(run.steps().iter().all(|s| s.branch == Some(1)
        && s.outcome == Outcome::Domain(Value::String("domain fixture b".into()))));
    assert_eq!(terminals.get(), 0);
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds + 1);
    assert!(kernel.shutdown().is_err());
    fs::write(&controls[0], "release").unwrap();
    let mut status = GraphStatus::Pending;
    for _ in 0..32 {
        status = run.pump(&mut observe).unwrap();
        if status == GraphStatus::Finished {
            break;
        }
        assert_eq!(status, GraphStatus::Pending);
    }
    assert_eq!(status, GraphStatus::Finished);
    assert_eq!(run.completion(), Some(GraphCompletion::Domain));
    assert_eq!(run.steps().len(), 6);
    let identities: BTreeSet<_> = run
        .steps()
        .iter()
        .map(|s| (s.root, s.activation, s.node.clone(), s.branch))
        .collect();
    assert_eq!(identities.len(), 6);
    assert!(run
        .steps()
        .iter()
        .all(|s| s.root == run.steps()[0].root && s.activation == Some(0)));
    for id in &each_ids {
        let branches: BTreeSet<_> = run
            .steps()
            .iter()
            .filter(|s| &s.node == id)
            .map(|s| s.branch)
            .collect();
        assert_eq!(branches, BTreeSet::from([Some(0), Some(1)]));
    }
    assert!(run
        .steps()
        .iter()
        .filter(|s| s.branch == Some(0))
        .all(|s| s.outcome == Outcome::Success(Value::String("first".into()))));
    let expected = Value::List(vec![
        Value::map([("Ok", Value::String("first".into()))]),
        Value::map([("Err", Value::String("domain fixture b".into()))]),
    ]);
    for id in &collectors {
        let step = run.steps().iter().find(|s| &s.node == id).unwrap();
        assert_eq!(step.branch, None);
        assert_eq!(step.outcome, Outcome::Success(expected.clone()));
    }
    assert!(
        run.steps()
            .iter()
            .position(|s| s.node == collectors[0])
            .unwrap()
            < run
                .steps()
                .iter()
                .position(|s| s.node == collectors[1])
                .unwrap()
    );
    assert_eq!([calls(0), calls(1)], ["first\nfirst\n", "domain\ndomain\n"]);
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds);
    assert_eq!(terminals.get(), 1);
    for _ in 0..3 {
        assert_eq!(
            run.pump(|_| panic!("duplicate recursive Each terminal"))
                .unwrap(),
            GraphStatus::Finished
        );
    }
    assert_eq!(run.steps().len(), 6);
    assert_eq!([calls(0), calls(1)], ["first\nfirst\n", "domain\ndomain\n"]);
    drop(run);
    kernel.shutdown().unwrap();
    for key in ["PHENIX_GRAPH_A_EACH_CONTROL", "PHENIX_GRAPH_B_EACH_CONTROL"] {
        std::env::remove_var(key);
    }
}
