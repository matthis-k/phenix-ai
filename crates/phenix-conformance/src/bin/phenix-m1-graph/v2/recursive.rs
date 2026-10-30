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
                authority: composition.authority,
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
    fs::write(output.join("graph-v2-recursive.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "format": "phenix.graph-v2-recursive.v1", "full_V2": false, "p1_parity": false,
        "cases": cases,
        "limits": ["finite root with two repeated middle/leaf occurrences", "no repeated action sources, recursive cycles, recursive InvokeEach or P1 parity"]
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
