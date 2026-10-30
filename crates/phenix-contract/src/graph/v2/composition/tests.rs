use super::*;
use crate::graph::v2::*;
use crate::ValueSchema;

fn definition() -> GraphDefinition {
    let security = PortSecurity {
        confidentiality: Classification::Public,
        integrity: IntegrityClass::Untrusted,
    };
    let input = InputPort {
        name: "in".into(),
        schema: ValueSchema::Bool,
        readiness: InputReadiness::One,
        security,
    };
    let output = OutputPort {
        name: "out".into(),
        schema: ValueSchema::Bool,
        security,
    };
    GraphDefinition {
        anchors: vec![],
        orders: vec![],
        id: GraphDefinitionId::new("demo/filter").unwrap(),
        body: GraphContributionV2 {
            contributor: "demo".into(),
            sources: vec![],
            edges: vec![],
            patches: vec![],
            nodes: vec![NodeDecl {
                id: "demo/filter".into(),
                kind: NodeKind::Merge {
                    policy: MergePolicy::ExactlyOne,
                },
                inputs: vec![input.clone()],
                outputs: vec![output.clone()],
                execution: ExecutionPolicy {
                    on_domain_error: DomainErrorPolicy::FailRoot,
                },
            }],
        },
        inputs: vec![BoundaryInput {
            port: input,
            target: BoundaryTo::Local(InPortRef {
                node: "demo/filter".into(),
                port: "in".into(),
            }),
        }],
        outputs: vec![BoundaryOutput {
            port: output,
            source: BoundaryFrom::Local(OutPortRef {
                node: "demo/filter".into(),
                port: "out".into(),
            }),
        }],
        children: vec![],
        wiring: vec![],
    }
}

#[test]
fn qualified_and_structural_occurrence_identities() {
    for invalid in ["filter", "demo/", "/filter", "demo/a/b", "root/filter"] {
        assert!(GraphDefinitionId::new(invalid).is_err(), "{invalid}");
    }
    let root = GraphInstanceId::new(GraphDefinitionId::new("app/main").unwrap());
    let a = root.child("a").unwrap();
    assert_ne!(a, root.child("b").unwrap());
    assert_ne!(
        a.child("b").unwrap(),
        root.child("b").unwrap().child("a").unwrap()
    );
    assert!(root.child("a/b").is_err());
    assert_eq!(a.root().as_str(), "app/main");
    assert_eq!(a.path(), &["a"]);
    let first = GraphInstance {
        id: a.clone(),
        definition: definition().id,
    };
    let second = GraphInstance {
        id: a,
        definition: GraphDefinitionId::new("demo/other").unwrap(),
    };
    assert_eq!(first.id, second.id);
    assert_ne!(first.definition, second.definition);
}

#[test]
fn explicit_aliases_normalize_without_changing_flat_bytes() {
    let mut d = definition();
    let bytes = d.body.encode().unwrap();
    let mut alias = d.inputs[0].clone();
    alias.port.name = "alias".into();
    d.inputs.push(alias);
    let canonical = d.normalized().unwrap();
    assert_eq!(canonical.inputs[0].port.name, "alias");
    d.inputs.reverse();
    assert_eq!(canonical, d.normalized().unwrap());
    assert_eq!(bytes, canonical.body.encode().unwrap());
    assert_eq!(GraphContributionV2::decode(&bytes).unwrap(), canonical.body);
}

#[test]
fn boundaries_reject_missing_duplicate_and_mismatched_contracts() {
    let base = definition();
    base.validate().unwrap();
    let mut d = base.clone();
    d.inputs[0].target = BoundaryTo::Local(InPortRef {
        node: "demo/filter".into(),
        port: "missing".into(),
    });
    assert!(d.validate().is_err());
    let mut d = base.clone();
    d.outputs[0].source = BoundaryFrom::Local(OutPortRef {
        node: "other/filter".into(),
        port: "out".into(),
    });
    assert!(d.validate().is_err());
    let mut d = base.clone();
    d.inputs.push(d.inputs[0].clone());
    assert!(d.normalized().is_err());
    let mut d = base.clone();
    d.inputs[0].port.schema = ValueSchema::String;
    assert!(d.validate().is_err());
    let mut d = base.clone();
    d.inputs[0].port.readiness = InputReadiness::Optional;
    assert!(d.validate().is_err());
    let mut d = base.clone();
    d.outputs[0].port.security.integrity = IntegrityClass::Trusted;
    assert!(d.validate().is_err());
    let mut d = base.clone();
    d.inputs[0].port.security.confidentiality = Classification::Secret;
    assert!(d.validate().is_err());
    let mut d = base.clone();
    d.id = GraphDefinitionId::new("other/filter").unwrap();
    assert!(d.validate().is_err());
    let mut d = base;
    d.outputs[0].port.name.clear();
    assert!(d.validate().is_err());
}

#[test]
fn closed_body_rejects_foreign_flow_and_order_crossings() {
    for kind in [
        EdgeKind::Flow {
            from: OutPortRef {
                node: "demo/filter".into(),
                port: "out".into(),
            },
            to: InPortRef {
                node: "other/filter".into(),
                port: "in".into(),
            },
        },
        EdgeKind::Order {
            before: "other/filter".into(),
            after: "demo/filter".into(),
        },
    ] {
        let mut d = definition();
        d.body.edges.push(EdgeDecl {
            id: "demo/crossing".into(),
            kind,
        });
        d.body.validate().unwrap();
        assert!(d.validate().is_err());
    }
}

fn recursive_fixture() -> (Vec<GraphDefinition>, GraphDefinitionId, ExpansionBudget) {
    let leaf = definition();
    let root_id = GraphDefinitionId::new("app/main").unwrap();
    let root = GraphDefinition {
        anchors: vec![],
        orders: vec![],
        id: root_id.clone(),
        body: GraphContributionV2 {
            contributor: "app".into(),
            sources: vec![],
            nodes: vec![],
            edges: vec![],
            patches: vec![],
        },
        inputs: vec![],
        outputs: vec![],
        children: ["left", "right"]
            .into_iter()
            .map(|name| GraphChild {
                name: name.into(),
                definition: leaf.id.clone(),
            })
            .collect(),
        wiring: vec![BoundaryFlow {
            next: None,
            name: "connect".into(),
            from: BoundaryFrom::Child {
                child: "left".into(),
                port: "out".into(),
            },
            to: BoundaryTo::Child {
                child: "right".into(),
                port: "in".into(),
            },
        }],
    };
    (
        vec![root, leaf],
        root_id,
        ExpansionBudget {
            depth: 3,
            occurrences: 8,
            items: 16,
        },
    )
}

fn nested_alias_fixture() -> (Vec<GraphDefinition>, GraphDefinitionId, ExpansionBudget) {
    let (mut defs, root, _) = recursive_fixture();
    let mut middle = defs[1].clone();
    middle.id = GraphDefinitionId::new("demo/middle").unwrap();
    middle.body.nodes.clear();
    middle.children = vec![GraphChild {
        name: "leaf".into(),
        definition: defs[1].id.clone(),
    }];
    middle.inputs[0].target = BoundaryTo::Child {
        child: "leaf".into(),
        port: "in".into(),
    };
    middle.outputs[0].source = BoundaryFrom::Child {
        child: "leaf".into(),
        port: "out".into(),
    };
    for child in &mut defs[0].children {
        child.definition = middle.id.clone();
    }
    defs[0].inputs = vec![BoundaryInput {
        port: middle.inputs[0].port.clone(),
        target: BoundaryTo::Child {
            child: "left".into(),
            port: "in".into(),
        },
    }];
    defs[0].outputs = vec![BoundaryOutput {
        port: middle.outputs[0].port.clone(),
        source: BoundaryFrom::Child {
            child: "right".into(),
            port: "out".into(),
        },
    }];
    defs.push(middle);
    (
        defs,
        root,
        ExpansionBudget {
            depth: 3,
            occurrences: 5,
            items: 21,
        },
    )
}

#[test]
fn multilevel_aliases_preserve_every_hop_and_canonical_occurrence_identity() {
    let (mut defs, root, budget) = nested_alias_fixture();
    let graph = lower_recursive(&defs, &root, budget).unwrap();
    let instance = GraphInstanceId::new(root.clone());
    for (maps, name, side) in [
        (&graph.inputs, "in", "left"),
        (&graph.outputs, "out", "right"),
    ] {
        let key = LogicalId {
            instance: instance.clone(),
            authored: name.into(),
        };
        let provenance = &maps[&key];
        assert_eq!(provenance.endpoint.node.instance.path(), [side, "leaf"]);
        assert_eq!(provenance.endpoint.node.authored, "demo/filter");
        assert_eq!(
            provenance.aliases,
            vec![
                key,
                LogicalId {
                    instance: instance.child(side).unwrap(),
                    authored: name.into()
                },
                LogicalId {
                    instance: instance.child(side).unwrap().child("leaf").unwrap(),
                    authored: name.into()
                },
            ]
        );
        assert_eq!(maps[&provenance.aliases[1]].aliases.len(), 2);
        assert_eq!(maps[&provenance.aliases[2]].aliases.len(), 1);
    }
    let origin = EdgeOrigin::Wiring(LogicalId {
        instance,
        authored: "connect".into(),
    });
    assert!(
        matches!(&graph.edges[&origin], LogicalEdge::Flow { from, to }
        if from.node.instance.path() == ["left", "leaf"] && to.node.instance.path() == ["right", "leaf"] && from.node != to.node)
    );
    for d in &mut defs {
        d.children.reverse();
        d.inputs.reverse();
        d.outputs.reverse();
        d.wiring.reverse();
    }
    defs.reverse();
    assert_eq!(graph, lower_recursive(&defs, &root, budget).unwrap());
    for limited in [
        ExpansionBudget { depth: 2, ..budget },
        ExpansionBudget {
            occurrences: 4,
            ..budget
        },
        ExpansionBudget {
            items: 20,
            ..budget
        },
    ] {
        assert!(lower_recursive(&defs, &root, limited).is_err());
    }
}

#[test]
fn nested_aliases_reject_malformed_and_unused_links_before_expansion() {
    let (base, root, budget) = nested_alias_fixture();
    for case in 0..13 {
        let mut defs = base.clone();
        match case {
            0 => {
                defs[2].inputs[0].target = BoundaryTo::Child {
                    child: "missing".into(),
                    port: "in".into(),
                }
            }
            1 => {
                defs[2].inputs[0].target = BoundaryTo::Child {
                    child: "leaf".into(),
                    port: "private".into(),
                }
            }
            2 => {
                defs[2].outputs[0].source = BoundaryFrom::Child {
                    child: "leaf".into(),
                    port: "private".into(),
                }
            }
            3 => defs[2].inputs[0].port.schema = ValueSchema::String,
            4 => defs[2].inputs[0].port.readiness = InputReadiness::Optional,
            5 => defs[2].inputs[0].port.security.integrity = IntegrityClass::Trusted,
            6 => defs[2].outputs[0].port.schema = ValueSchema::String,
            7 => defs[2].outputs[0].port.security.confidentiality = Classification::Secret,
            8 => {
                defs[2].inputs[0].target = BoundaryTo::Child {
                    child: "leaf/nested".into(),
                    port: "in".into(),
                }
            }
            9 => defs[2].children[0].definition = defs[2].id.clone(),
            10 => defs[2].children[0].definition = GraphDefinitionId::new("lost/leaf").unwrap(),
            11 => {
                // An actual body port is private unless exported under this name.
                defs[1].inputs[0].port.name = "public".into();
            }
            12 => {
                defs[2].inputs[0].target = BoundaryTo::Local(InPortRef {
                    node: "demo/filter".into(),
                    port: "in".into(),
                })
            }
            _ => unreachable!(),
        }
        assert!(
            lower_recursive(&defs, &root, budget).is_err(),
            "case {case}"
        );
        // The malformed definitions remain rejected when outside the selected closure.
        let unused_root = GraphDefinition {
            anchors: vec![],
            orders: vec![],
            id: GraphDefinitionId::new("app/unused").unwrap(),
            body: GraphContributionV2 {
                contributor: "app".into(),
                nodes: vec![],
                sources: vec![],
                edges: vec![],
                patches: vec![],
            },
            inputs: vec![],
            outputs: vec![],
            children: vec![],
            wiring: vec![],
        };
        let unused = unused_root.id.clone();
        defs.push(unused_root);
        assert!(
            lower_recursive(&defs, &unused, budget).is_err(),
            "unused case {case}"
        );
    }
}

#[test]
fn nested_feedback_is_deferred_not_an_instantaneous_cycle() {
    let (mut defs, root, budget) = nested_alias_fixture();
    let mut reverse = defs[0].wiring[0].clone();
    reverse.name = "reverse".into();
    reverse.from = BoundaryFrom::Child {
        child: "right".into(),
        port: "out".into(),
    };
    reverse.to = BoundaryTo::Child {
        child: "left".into(),
        port: "in".into(),
    };
    reverse.next = Some(DeferredWave::Next);
    defs[0].wiring.push(reverse);
    let budget = ExpansionBudget {
        items: 22,
        ..budget
    };
    lower_recursive(&defs, &root, budget).unwrap();
    defs[0].wiring[1].next = None;
    assert_eq!(
        lower_recursive(&defs, &root, budget).unwrap_err().message,
        "recursive dependency cycle"
    );
}

#[test]
fn retained_alias_paths_exhaust_bytes_before_depth_or_item_limits() {
    for count in [4, 32] {
        let mut defs = Vec::new();
        for i in 0..count {
            let mut d = definition();
            d.id = GraphDefinitionId::new(&format!("demo/d{i}")).unwrap();
            if i + 1 < count {
                let child = "x".repeat(256);
                d.body.nodes.clear();
                d.children.push(GraphChild {
                    name: child.clone(),
                    definition: GraphDefinitionId::new(&format!("demo/d{}", i + 1)).unwrap(),
                });
                d.inputs[0].target = BoundaryTo::Child {
                    child: child.clone(),
                    port: "in".into(),
                };
                d.outputs[0].source = BoundaryFrom::Child {
                    child,
                    port: "out".into(),
                };
            }
            defs.push(d);
        }
        let result = lower_recursive(
            &defs,
            &defs[0].id,
            ExpansionBudget {
                depth: count,
                occurrences: count,
                items: crate::MAX_COUNT,
            },
        );
        if count == 4 {
            result.unwrap();
        } else {
            assert_eq!(result.unwrap_err().message, "recursive byte budget");
        }
    }
}

fn anchor_fixture() -> (Vec<GraphDefinition>, GraphDefinitionId, ExpansionBudget) {
    let (mut defs, root, _) = nested_alias_fixture();
    for (name, visibility) in [
        ("run", AnchorVisibility::Public),
        ("hidden", AnchorVisibility::Private),
    ] {
        defs[1].anchors.push(GraphAnchor {
            name: name.into(),
            visibility,
            target: AnchorTarget::LocalNode("demo/filter".into()),
        });
    }
    defs[2].anchors.push(GraphAnchor {
        name: "run".into(),
        visibility: AnchorVisibility::Public,
        target: AnchorTarget::Child {
            child: "leaf".into(),
            anchor: "run".into(),
        },
    });
    let mut guard = defs[1].body.nodes[0].clone();
    guard.id = "app/guard".into();
    defs[0].body.nodes.push(guard);
    defs[0].anchors = vec![
        GraphAnchor {
            name: "before".into(),
            visibility: AnchorVisibility::Private,
            target: AnchorTarget::LocalNode("app/guard".into()),
        },
        GraphAnchor {
            name: "selected".into(),
            visibility: AnchorVisibility::Public,
            target: AnchorTarget::Child {
                child: "left".into(),
                anchor: "run".into(),
            },
        },
        GraphAnchor {
            name: "right-local".into(),
            visibility: AnchorVisibility::Private,
            target: AnchorTarget::Child {
                child: "right".into(),
                anchor: "run".into(),
            },
        },
    ];
    defs[0].orders = vec![
        BoundaryOrder {
            name: "left".into(),
            before: AnchorRef::Local("before".into()),
            after: AnchorRef::Local("selected".into()),
        },
        BoundaryOrder {
            name: "right".into(),
            before: AnchorRef::Local("before".into()),
            after: AnchorRef::Child {
                child: "right".into(),
                anchor: "run".into(),
            },
        },
    ];
    (
        defs,
        root,
        ExpansionBudget {
            depth: 3,
            occurrences: 5,
            items: 46,
        },
    )
}

#[test]
fn scoped_anchors_and_orders_preserve_visibility_every_hop_and_isolated_nodes() {
    let (mut defs, root, budget) = anchor_fixture();
    let graph = lower_recursive(&defs, &root, budget).unwrap();
    let scope = GraphInstanceId::new(root.clone());
    assert!(graph
        .resolve_anchor(
            &scope.child("missing").unwrap(),
            &AnchorRef::Local("before".into())
        )
        .is_err());
    let selected = graph
        .resolve_anchor(&scope, &AnchorRef::Local("selected".into()))
        .unwrap();
    assert_eq!(
        selected
            .aliases
            .iter()
            .map(|a| a.authored.as_str())
            .collect::<Vec<_>>(),
        ["selected", "run", "run"]
    );
    assert_eq!(selected.node.instance.path(), ["left", "leaf"]);
    let right = graph
        .resolve_anchor(
            &scope,
            &AnchorRef::Child {
                child: "right".into(),
                anchor: "run".into(),
            },
        )
        .unwrap();
    assert_eq!(right.node.instance.path(), ["right", "leaf"]);
    assert_ne!(selected.node, right.node);
    let private = graph
        .resolve_anchor(&scope, &AnchorRef::Local("before".into()))
        .unwrap();
    assert_eq!(private.visibility, AnchorVisibility::Private);
    let id = LogicalId {
        instance: scope.clone(),
        authored: "left".into(),
    };
    assert_eq!(graph.orders[&id].before, private.aliases);
    assert_eq!(graph.orders[&id].after, selected.aliases);
    assert_eq!(
        graph.edges[&EdgeOrigin::Order(id)],
        LogicalEdge::Order {
            before: private.node.clone(),
            after: selected.node.clone()
        }
    );
    let middle = scope.child("left").unwrap();
    assert!(graph
        .resolve_anchor(
            &middle,
            &AnchorRef::Child {
                child: "leaf".into(),
                anchor: "hidden".into()
            }
        )
        .is_err());
    assert!(graph
        .resolve_anchor(
            &middle.child("leaf").unwrap(),
            &AnchorRef::Local("hidden".into())
        )
        .is_ok());
    for r in [
        AnchorRef::Local("demo/filter".into()),
        AnchorRef::Child {
            child: "left/leaf".into(),
            anchor: "run".into(),
        },
        AnchorRef::Child {
            child: "left".into(),
            anchor: "hidden".into(),
        },
    ] {
        assert!(graph.resolve_anchor(&scope, &r).is_err());
    }
    for d in &mut defs {
        d.anchors.reverse();
        d.orders.reverse();
        d.children.reverse();
    }
    defs.reverse();
    assert_eq!(graph, lower_recursive(&defs, &root, budget).unwrap());
    for limited in [
        ExpansionBudget {
            items: budget.items - 1,
            ..budget
        },
        ExpansionBudget { depth: 2, ..budget },
        ExpansionBudget {
            occurrences: 4,
            ..budget
        },
    ] {
        assert!(lower_recursive(&defs, &root, limited).is_err());
    }
}

#[test]
fn anchor_and_order_links_reject_hidden_raw_foreign_cyclic_and_oversized_metadata() {
    let (base, root, budget) = anchor_fixture();
    for case in 0..15 {
        let mut defs = base.clone();
        match case {
            0 => {
                defs[2].anchors[0].target = AnchorTarget::Child {
                    child: "leaf".into(),
                    anchor: "hidden".into(),
                }
            }
            1 => {
                defs[2].anchors[0].target = AnchorTarget::Child {
                    child: "leaf".into(),
                    anchor: "missing".into(),
                }
            }
            2 => {
                defs[2].anchors[0].target = AnchorTarget::Child {
                    child: "missing".into(),
                    anchor: "run".into(),
                }
            }
            3 => defs[0].anchors[0].target = AnchorTarget::LocalNode("demo/filter".into()),
            4 => defs[0].orders[0].after = AnchorRef::Local("demo/filter".into()),
            5 => {
                defs[0].orders[0].after = AnchorRef::Child {
                    child: "left/leaf".into(),
                    anchor: "run".into(),
                }
            }
            6 => {
                defs[0].orders[0].after = AnchorRef::Child {
                    child: "left".into(),
                    anchor: "missing".into(),
                }
            }
            7 => defs[2].orders.push(BoundaryOrder {
                name: "hidden".into(),
                before: AnchorRef::Child {
                    child: "leaf".into(),
                    anchor: "hidden".into(),
                },
                after: AnchorRef::Local("run".into()),
            }),
            8 => {
                let a = defs[1].anchors[0].clone();
                defs[1].anchors.push(a);
            }
            9 => {
                let o = defs[0].orders[0].clone();
                defs[0].orders.push(o);
            }
            10 => defs[1].children.push(GraphChild {
                name: "back".into(),
                definition: root.clone(),
            }),
            11 => defs[0].anchors[0].name = "x".repeat(crate::MAX_BYTES + 1),
            12 => defs[0].id = GraphDefinitionId::new("foreign/root").unwrap(),
            13 => defs[2].anchors[0].visibility = AnchorVisibility::Private,
            14 => defs[0].orders[0].name = "x".repeat(crate::MAX_BYTES + 1),
            _ => unreachable!(),
        }
        assert!(
            lower_recursive(&defs, &root, budget).is_err(),
            "case {case}"
        );
        // Link validation cannot be bypassed by placing the malformed definition
        // outside the selected closure.
        let mut unused = definition();
        unused.id = GraphDefinitionId::new("demo/unused").unwrap();
        let unused_root = unused.id.clone();
        defs.push(unused);
        assert!(
            lower_recursive(&defs, &unused_root, budget).is_err(),
            "unused case {case}"
        );
    }
    let (mut defs, root, _) = anchor_fixture();
    defs[0].orders.push(BoundaryOrder {
        name: "cycle".into(),
        before: AnchorRef::Child {
            child: "right".into(),
            anchor: "run".into(),
        },
        after: AnchorRef::Local("before".into()),
    });
    assert_eq!(
        lower_recursive(
            &defs,
            &root,
            ExpansionBudget {
                items: 128,
                ..budget
            }
        )
        .unwrap_err()
        .message,
        "recursive dependency cycle"
    );
}

#[test]
fn anchor_provenance_bytes_are_bounded_independently_of_depth_and_items() {
    for count in [4, 32] {
        let mut defs = Vec::new();
        for i in 0..count {
            let mut d = definition();
            d.id = GraphDefinitionId::new(&format!("demo/anchor{i}")).unwrap();
            d.inputs.clear();
            d.outputs.clear();
            let target = if i + 1 == count {
                AnchorTarget::LocalNode("demo/filter".into())
            } else {
                d.body.nodes.clear();
                let name = "x".repeat(256);
                d.children.push(GraphChild {
                    name: name.clone(),
                    definition: GraphDefinitionId::new(&format!("demo/anchor{}", i + 1)).unwrap(),
                });
                AnchorTarget::Child {
                    child: name,
                    anchor: "public".into(),
                }
            };
            d.anchors.push(GraphAnchor {
                name: "public".into(),
                visibility: AnchorVisibility::Public,
                target,
            });
            defs.push(d);
        }
        let result = lower_recursive(
            &defs,
            &defs[0].id,
            ExpansionBudget {
                depth: count,
                occurrences: count,
                items: crate::MAX_COUNT,
            },
        );
        if count == 4 {
            result.unwrap();
        } else {
            assert_eq!(result.unwrap_err().message, "recursive byte budget");
        }
    }
}

#[test]
fn recursive_expansion_is_canonical_and_preserves_occurrence_provenance() {
    let (mut defs, root, budget) = recursive_fixture();
    let graph = lower_recursive(&defs, &root, budget).unwrap();
    assert_eq!(graph.instances.len(), 3);
    assert_eq!(graph.nodes.len(), 2);
    assert_eq!(graph.edges.len(), 1);
    let origin = EdgeOrigin::Wiring(LogicalId {
        instance: GraphInstanceId::new(root.clone()),
        authored: "connect".into(),
    });
    let LogicalEdge::Flow { from, to } = &graph.edges[&origin] else {
        panic!()
    };
    assert_eq!(from.node.instance.path(), &["left"]);
    assert_eq!(to.node.instance.path(), &["right"]);
    assert_eq!(from.node.authored, "demo/filter");
    assert_ne!(from.node, to.node);
    assert_eq!(graph.instances[&from.node.instance], defs[1].id);
    assert_eq!(graph.nodes[&from.node], defs[1].body.nodes[0]);
    defs[0].children.reverse();
    defs.reverse();
    assert_eq!(graph, lower_recursive(&defs, &root, budget).unwrap());
}

#[test]
fn invoke_each_preserves_original_owner_complete_schema_and_scoped_provenance() {
    let (mut definitions, root, budget) = recursive_fixture();
    let batch = ValueSchema::List(Box::new(ValueSchema::Struct(
        std::collections::BTreeMap::from([
            ("route".into(), ValueSchema::U64),
            ("input".into(), ValueSchema::Bool),
        ]),
    )));
    let outcomes = ValueSchema::List(Box::new(ValueSchema::Tagged(
        std::collections::BTreeMap::from([
            ("Ok".into(), Some(ValueSchema::Bool)),
            ("Err".into(), Some(ValueSchema::Bool)),
        ]),
    )));
    let leaf = &mut definitions[1];
    leaf.body.nodes[0].kind = NodeKind::InvokeEach {
        collection: "Workers".into(),
        operation: "demo@1/run".into(),
        include_self: true,
        max: 3,
    };
    leaf.body.nodes[0].execution.on_domain_error = DomainErrorPolicy::RouteErr;
    leaf.body.nodes[0].inputs[0].schema = batch.clone();
    leaf.inputs[0].port.schema = batch;
    leaf.body.nodes[0].outputs[0].schema = outcomes.clone();
    leaf.outputs[0].port.schema = outcomes;
    leaf.anchors.push(GraphAnchor {
        name: "each".into(),
        visibility: AnchorVisibility::Public,
        target: AnchorTarget::LocalNode("demo/filter".into()),
    });
    let input = leaf.inputs[0].port.clone();
    let output = leaf.outputs[0].port.clone();
    definitions[0].wiring.clear();
    definitions[0].inputs.push(BoundaryInput {
        port: input,
        target: BoundaryTo::Child {
            child: "left".into(),
            port: "in".into(),
        },
    });
    definitions[0].outputs.push(BoundaryOutput {
        port: output,
        source: BoundaryFrom::Child {
            child: "right".into(),
            port: "out".into(),
        },
    });
    definitions[0].anchors.push(GraphAnchor {
        name: "selected".into(),
        visibility: AnchorVisibility::Public,
        target: AnchorTarget::Child {
            child: "left".into(),
            anchor: "each".into(),
        },
    });
    definitions[0].orders.push(BoundaryOrder {
        name: "ordered".into(),
        before: AnchorRef::Local("selected".into()),
        after: AnchorRef::Child {
            child: "right".into(),
            anchor: "each".into(),
        },
    });
    let budget = ExpansionBudget {
        items: 32,
        ..budget
    };
    let expanded = lower_recursive(&definitions, &root, budget).unwrap();
    assert_eq!(expanded.nodes.len(), 2);
    for (id, node) in &expanded.nodes {
        assert_eq!(expanded.instances[&id.instance].owner(), "demo");
        assert_eq!(node, &definitions[1].body.nodes[0]);
        assert_eq!(id.authored, "demo/filter");
    }
    let scope = GraphInstanceId::new(root.clone());
    let selected = expanded
        .resolve_anchor(&scope, &AnchorRef::Local("selected".into()))
        .unwrap();
    assert_eq!(selected.aliases.len(), 2);
    assert_eq!(selected.node.instance.path(), ["left"]);
    assert_eq!(
        expanded.orders[&LogicalId {
            instance: scope.clone(),
            authored: "ordered".into()
        }]
            .before,
        selected.aliases
    );
    assert_eq!(
        expanded.inputs[&LogicalId {
            instance: scope.clone(),
            authored: "in".into()
        }]
            .endpoint
            .node,
        selected.node
    );
    assert_eq!(
        expanded.outputs[&LogicalId {
            instance: scope,
            authored: "out".into()
        }]
            .endpoint
            .node
            .instance
            .path(),
        ["right"]
    );
    for case in 0..4 {
        let mut malformed = definitions.clone();
        let NodeKind::InvokeEach {
            collection,
            operation,
            max,
            ..
        } = &mut malformed[1].body.nodes[0].kind
        else {
            panic!()
        };
        match case {
            0 => *collection = "x".repeat(crate::MAX_BYTES + 1),
            1 => *operation = "x".repeat(crate::MAX_BYTES + 1),
            2 => *max = 0,
            3 => *max = 257,
            _ => unreachable!(),
        }
        let failure = lower_recursive(&malformed, &root, budget).unwrap_err();
        if case < 2 {
            assert_eq!(failure.message, "recursive byte budget");
        }
    }
    definitions[0].children.reverse();
    definitions.reverse();
    assert_eq!(
        expanded,
        lower_recursive(&definitions, &root, budget).unwrap()
    );
}

#[test]
fn nested_local_wiring_and_owner_relative_invokes_are_preserved() {
    let (mut defs, root, budget) = recursive_fixture();
    // Make the shared leaf an Invoke owned by demo, placed under app twice.
    let leaf = &mut defs[1];
    leaf.body.nodes[0].kind = NodeKind::Invoke {
        selector: "".into(),
        operation: "demo@1/run".into(),
    };
    let mut err = leaf.body.nodes[0].outputs[0].clone();
    err.name = "err".into();
    leaf.body.nodes[0].outputs[0].name = "ok".into();
    leaf.body.nodes[0].outputs.push(err);
    leaf.outputs[0].source = BoundaryFrom::Local(OutPortRef {
        node: "demo/filter".into(),
        port: "ok".into(),
    });
    let mut middle = definition();
    middle.id = GraphDefinitionId::new("demo/middle").unwrap();
    middle.children = vec![GraphChild {
        name: "nested".into(),
        definition: leaf.id.clone(),
    }];
    middle.wiring = vec![BoundaryFlow {
        next: None,
        name: "into".into(),
        from: middle.outputs[0].source.clone(),
        to: BoundaryTo::Child {
            child: "nested".into(),
            port: "in".into(),
        },
    }];
    defs[0].children[0].definition = middle.id.clone();
    defs.push(middle);
    let graph = lower_recursive(&defs, &root, budget).unwrap();
    let key = LogicalId {
        instance: GraphInstanceId::new(root)
            .child("left")
            .unwrap()
            .child("nested")
            .unwrap(),
        authored: "demo/filter".into(),
    };
    assert_eq!(graph.instances[&key.instance].owner(), "demo");
    assert_eq!(graph.nodes[&key].kind, defs[1].body.nodes[0].kind);
    assert_eq!(graph.edges.len(), 2);
}

#[test]
fn recursive_budgets_have_exact_boundaries() {
    let (defs, root, _) = recursive_fixture();
    let exact = ExpansionBudget {
        depth: 2,
        occurrences: 3,
        items: 7,
    };
    lower_recursive(&defs, &root, exact).unwrap();
    for budget in [
        ExpansionBudget { depth: 1, ..exact },
        ExpansionBudget {
            occurrences: 2,
            ..exact
        },
        ExpansionBudget { items: 6, ..exact },
        ExpansionBudget { depth: 65, ..exact },
        ExpansionBudget {
            occurrences: crate::MAX_COUNT + 1,
            ..exact
        },
        ExpansionBudget {
            items: crate::MAX_COUNT + 1,
            ..exact
        },
    ] {
        assert!(lower_recursive(&defs, &root, budget).is_err(), "{budget:?}");
    }
    let oversized = vec![defs[0].clone(); crate::MAX_COUNT + 1];
    assert!(lower_recursive(&oversized, &root, exact).is_err());
}

#[test]
fn recursive_metadata_and_literal_bytes_are_bounded_before_normalization() {
    let (base, root, budget) = recursive_fixture();
    for case in 0..5 {
        let mut defs = base.clone();
        let huge = "x".repeat(crate::MAX_BYTES + 1);
        match case {
            0 => defs[0].children[0].name = huge,
            1 => defs[1].inputs[0].port.name = huge,
            2 => defs[0].wiring[0].name = huge,
            3 => {
                defs[0].wiring[0].from = BoundaryFrom::Child {
                    child: "left".into(),
                    port: huge,
                }
            }
            4 => {
                defs[1].body.nodes[0].kind = NodeKind::Constant {
                    value: crate::Value::String(huge),
                }
            }
            _ => unreachable!(),
        }
        assert_eq!(
            lower_recursive(&defs, &root, budget).unwrap_err().message,
            "recursive byte budget",
            "case {case}"
        );
    }
}

#[test]
fn repeated_payloads_exhaust_bytes_independently_of_items() {
    let (mut defs, root, _) = recursive_fixture();
    let leaf = &mut defs[1];
    leaf.body.nodes[0].inputs[0].name = "start".into();
    leaf.body.nodes[0].inputs[0].schema = ValueSchema::Struct(Default::default());
    leaf.body.nodes[0].inputs[0].readiness = InputReadiness::One;
    leaf.inputs[0].port = leaf.body.nodes[0].inputs[0].clone();
    leaf.inputs[0].target = BoundaryTo::Local(InPortRef {
        node: "demo/filter".into(),
        port: "start".into(),
    });
    leaf.body.nodes[0].outputs[0].schema = ValueSchema::Bytes;
    leaf.outputs[0].port.schema = ValueSchema::Bytes;
    leaf.body.nodes[0].kind = NodeKind::Constant {
        value: crate::Value::Bytes(vec![0; crate::MAX_BYTES / 16]),
    };
    defs[0].wiring.clear();
    let budget = ExpansionBudget {
        depth: 2,
        occurrences: 33,
        items: 96,
    };
    lower_recursive(&defs, &root, budget).unwrap();
    defs[0].children = (0..32)
        .map(|i| GraphChild {
            name: format!("c{i}"),
            definition: defs[1].id.clone(),
        })
        .collect();
    assert_eq!(
        lower_recursive(&defs, &root, budget).unwrap_err().message,
        "recursive byte budget"
    );
}

#[test]
fn recursive_api_is_publicly_exported() {
    let (defs, root, budget) = recursive_fixture();
    let graph: crate::graph::v2::composition::ExpandedGraph =
        crate::graph::v2::composition::lower_recursive(&defs, &root, budget).unwrap();
    assert_eq!(graph.instances.len(), 3);
}

#[test]
fn deferred_internal_and_boundary_feedback_preserve_origins_without_same_wave_cycles() {
    let (mut defs, root, budget) = recursive_fixture();
    let BoundaryFrom::Local(from) = defs[1].outputs[0].source.clone() else {
        panic!()
    };
    let BoundaryTo::Local(to) = defs[1].inputs[0].target.clone() else {
        panic!()
    };
    let feedback = EdgeDecl {
        id: "demo/feedback".into(),
        kind: EdgeKind::Deferred {
            from,
            to,
            next: DeferredWave::Next,
        },
    };
    defs[1].body.edges.push(feedback);
    let mut reverse = defs[0].wiring[0].clone();
    reverse.name = "feedback".into();
    // Reverse the sibling direction while retaining the original forward Flow.
    reverse.from = BoundaryFrom::Child {
        child: "right".into(),
        port: "out".into(),
    };
    reverse.to = BoundaryTo::Child {
        child: "left".into(),
        port: "in".into(),
    };
    reverse.next = Some(DeferredWave::Next);
    defs[0].wiring.push(reverse);
    let graph = lower_recursive(&defs, &root, budget).unwrap();
    for child in ["left", "right"] {
        let origin = EdgeOrigin::Body(LogicalId {
            instance: GraphInstanceId::new(root.clone()).child(child).unwrap(),
            authored: "demo/feedback".into(),
        });
        assert!(
            matches!(&graph.edges[&origin], LogicalEdge::Deferred { from, to, next: DeferredWave::Next } if from.node == to.node && from.node.instance.path() == [child])
        );
    }
    let origin = EdgeOrigin::Wiring(LogicalId {
        instance: GraphInstanceId::new(root.clone()),
        authored: "feedback".into(),
    });
    assert!(
        matches!(&graph.edges[&origin], LogicalEdge::Deferred { from, to, .. } if from.node.instance.path() == ["right"] && to.node.instance.path() == ["left"])
    );
    defs.reverse();
    assert_eq!(graph, lower_recursive(&defs, &root, budget).unwrap());
    defs.reverse();
    defs[0].wiring[1].next = None;
    assert_eq!(
        lower_recursive(&defs, &root, budget).unwrap_err().message,
        "recursive dependency cycle"
    );
}

#[test]
fn deferred_rejects_invalid_ports_schemas_security_and_unexposed_crossings() {
    let (base, root, budget) = recursive_fixture();
    for case in 0..5 {
        let mut defs = base.clone();
        defs[0].wiring[0].next = Some(DeferredWave::Next);
        match case {
            0 => {
                defs[0].wiring[0].to = BoundaryTo::Child {
                    child: "right".into(),
                    port: "missing".into(),
                }
            }
            1 => {
                defs[0].wiring[0].to = BoundaryTo::Child {
                    child: "right/nested".into(),
                    port: "in".into(),
                }
            }
            2 => {
                defs[1].body.nodes[0].outputs[0].schema = ValueSchema::String;
                defs[1].outputs[0].port.schema = ValueSchema::String;
            }
            3 => {
                defs[1].body.nodes[0].outputs[0].security.confidentiality = Classification::Secret;
                defs[1].outputs[0].port.security.confidentiality = Classification::Secret;
            }
            4 => {
                let BoundaryFrom::Local(from) = defs[1].outputs[0].source.clone() else {
                    panic!()
                };
                defs[1].body.edges.push(EdgeDecl {
                    id: "demo/crossing".into(),
                    kind: EdgeKind::Deferred {
                        from,
                        to: InPortRef {
                            node: "other/filter".into(),
                            port: "in".into(),
                        },
                        next: DeferredWave::Next,
                    },
                });
            }
            _ => unreachable!(),
        }
        assert!(
            lower_recursive(&defs, &root, budget).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn recursive_adversarial_shapes_and_crossings_fail_deterministically() {
    let (base, root, budget) = recursive_fixture();
    for case in 0..11 {
        let mut defs = base.clone();
        match case {
            0 => defs.push(defs[1].clone()),
            1 => {
                let child = defs[0].children[0].clone();
                defs[0].children.push(child);
            }
            2 => {
                let wire = defs[0].wiring[0].clone();
                defs[0].wiring.push(wire);
            }
            3 => defs[0].children[0].definition = GraphDefinitionId::new("lost/graph").unwrap(),
            4 => defs[0].children[0].definition = root.clone(),
            5 => defs[1].children.push(GraphChild {
                name: "back".into(),
                definition: root.clone(),
            }),
            6 => {
                defs[0].wiring[0].from = BoundaryFrom::Child {
                    child: "left/nested".into(),
                    port: "out".into(),
                }
            }
            7 => {
                defs[0].wiring[0].to = BoundaryTo::Child {
                    child: "right".into(),
                    port: "missing".into(),
                }
            }
            8 => {
                defs[0].wiring[0].from = BoundaryFrom::Local(OutPortRef {
                    node: "demo/filter".into(),
                    port: "out".into(),
                })
            }
            9 => {
                let mut reverse = defs[0].wiring[0].clone();
                reverse.name = "reverse".into();
                reverse.from = BoundaryFrom::Child {
                    child: "right".into(),
                    port: "out".into(),
                };
                reverse.to = BoundaryTo::Child {
                    child: "left".into(),
                    port: "in".into(),
                };
                defs[0].wiring.push(reverse);
            }
            10 => defs[1].body.edges.push(EdgeDecl {
                id: "demo/cycle".into(),
                kind: EdgeKind::Order {
                    before: "demo/filter".into(),
                    after: "demo/filter".into(),
                },
            }),
            _ => unreachable!(),
        }
        let error = lower_recursive(&defs, &root, budget).unwrap_err();
        defs.reverse();
        let again = lower_recursive(&defs, &root, budget).unwrap_err();
        assert_eq!(error.code, again.code, "case {case}");
        assert_eq!(error.message, again.message, "case {case}");
    }
}

#[test]
fn body_and_boundary_flows_check_real_ports_schemas_and_security() {
    let (base, root, budget) = recursive_fixture();
    for case in 0..4 {
        let mut defs = base.clone();
        let mut other = definition();
        other.id = GraphDefinitionId::new("demo/other").unwrap();
        match case {
            0 => {
                other.body.nodes[0].inputs[0].schema = ValueSchema::String;
                other.body.nodes[0].outputs[0].schema = ValueSchema::String;
                other.inputs[0].port.schema = ValueSchema::String;
                other.outputs[0].port.schema = ValueSchema::String;
            }
            1 => {
                other.body.nodes[0].inputs[0].security.integrity = IntegrityClass::Trusted;
                other.inputs[0].port.security.integrity = IntegrityClass::Trusted;
            }
            2 => {
                defs[1].body.nodes[0].outputs[0].security.confidentiality = Classification::Secret;
                defs[1].outputs[0].port.security.confidentiality = Classification::Secret;
            }
            3 => {
                let BoundaryTo::Local(to) = defs[1].inputs[0].target.clone() else {
                    panic!()
                };
                defs[1].body.edges.push(EdgeDecl {
                    id: "demo/badport".into(),
                    kind: EdgeKind::Flow {
                        from: OutPortRef {
                            node: "demo/filter".into(),
                            port: "missing".into(),
                        },
                        to,
                    },
                });
            }
            _ => unreachable!(),
        }
        defs[0].children[1].definition = other.id.clone();
        defs.push(other);
        assert!(
            lower_recursive(&defs, &root, budget).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn sources_local_order_and_both_boundary_directions_survive_lowering() {
    let (mut defs, root, budget) = recursive_fixture();
    let mut sink = defs[1].body.nodes[0].clone();
    sink.id = "app/sink".into();
    let source = SourceDecl {
        id: "app/events".into(),
        kind: SourceKind::CustomEvent {
            event: "ready".into(),
        },
        payload: ValueSchema::Bool,
        security: sink.outputs[0].security,
        owner: "app".into(),
    };
    let node = NodeDecl {
        id: "app/source".into(),
        kind: NodeKind::Source {
            source: source.id.clone(),
        },
        inputs: vec![],
        outputs: sink.outputs.clone(),
        execution: sink.execution,
    };
    defs[0].body.sources.push(source.clone());
    defs[0].body.nodes = vec![node, sink];
    defs[0].body.edges = vec![
        EdgeDecl {
            id: "app/order".into(),
            kind: EdgeKind::Order {
                before: "app/source".into(),
                after: "app/sink".into(),
            },
        },
        EdgeDecl {
            id: "app/flow".into(),
            kind: EdgeKind::Flow {
                from: OutPortRef {
                    node: "app/source".into(),
                    port: "out".into(),
                },
                to: InPortRef {
                    node: "app/sink".into(),
                    port: "in".into(),
                },
            },
        },
    ];
    defs[0].wiring.extend([
        BoundaryFlow {
            next: None,
            name: "enter".into(),
            from: BoundaryFrom::Local(OutPortRef {
                node: "app/source".into(),
                port: "out".into(),
            }),
            to: BoundaryTo::Child {
                child: "left".into(),
                port: "in".into(),
            },
        },
        BoundaryFlow {
            next: None,
            name: "leave".into(),
            from: BoundaryFrom::Child {
                child: "right".into(),
                port: "out".into(),
            },
            to: BoundaryTo::Local(InPortRef {
                node: "app/sink".into(),
                port: "in".into(),
            }),
        },
    ]);
    let graph = lower_recursive(&defs, &root, budget).unwrap();
    assert_eq!(graph.sources.values().collect::<Vec<_>>(), vec![&source]);
    assert_eq!(graph.nodes.len(), 4);
    assert_eq!(graph.edges.len(), 5);
    lower_recursive(
        &defs,
        &root,
        ExpansionBudget {
            items: 14,
            ..budget
        },
    )
    .unwrap();
    assert!(lower_recursive(
        &defs,
        &root,
        ExpansionBudget {
            items: 13,
            ..budget
        }
    )
    .is_err());
    defs[0].body.nodes.reverse();
    defs[0].body.edges.reverse();
    defs[0].wiring.reverse();
    assert_eq!(graph, lower_recursive(&defs, &root, budget).unwrap());
}

#[test]
fn empty_occurrences_still_consume_depth_and_occurrence_budgets() {
    let (base, _, _) = recursive_fixture();
    let ids: Vec<_> = (0..64)
        .map(|i| GraphDefinitionId::new(&format!("app/d{i}")).unwrap())
        .collect();
    let defs: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let mut d = base[0].clone();
            d.id = id.clone();
            d.wiring.clear();
            d.children = ids
                .get(i + 1)
                .map(|next| {
                    vec![GraphChild {
                        name: "next".into(),
                        definition: next.clone(),
                    }]
                })
                .unwrap_or_default();
            d
        })
        .collect();
    let budget = ExpansionBudget {
        depth: 64,
        occurrences: 64,
        items: 0,
    };
    let graph = lower_recursive(&defs, &ids[0], budget).unwrap();
    assert_eq!(graph.instances.len(), 64);
    assert!(graph.nodes.is_empty());
    assert!(lower_recursive(
        &defs,
        &ids[0],
        ExpansionBudget {
            depth: 63,
            ..budget
        }
    )
    .is_err());
    assert!(lower_recursive(
        &defs,
        &ids[0],
        ExpansionBudget {
            occurrences: 63,
            ..budget
        }
    )
    .is_err());
    assert!(lower_recursive(
        &defs,
        &GraphDefinitionId::new("app/missing").unwrap(),
        budget
    )
    .is_err());
}
