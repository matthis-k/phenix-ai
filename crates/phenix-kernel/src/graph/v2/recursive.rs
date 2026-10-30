use super::*;
use composition::*;

/// Deterministic bounded owner/local name only; grants no authority. `kind` is
/// "node", "source", "body-edge", "wire" or "order". Owner is the ORIGINAL contributor.
pub fn recursive_effective_id(owner: &str, kind: &str, id: &LogicalId) -> Result<String, Failure> {
    if !phenix_contract::identifier(owner)
        || owner == "root"
        || !matches!(kind, "node" | "source" | "body-edge" | "wire" | "order")
        || !(phenix_contract::identifier(&id.authored)
            || phenix_contract::graph::valid_node_key(&id.authored))
    {
        return Err(invalid("invalid recursive identity"));
    }
    let mut bytes = b"phenix.graph.recursive-id.v1\0".to_vec();
    bytes.extend(Value::List(vec![text(owner), text(kind), logical_value(id)]).encode()?);
    Ok(format!("{owner}/r{}", phenix_artifact::digest(&bytes)))
}

fn logical_value(id: &LogicalId) -> Value {
    Value::List(vec![
        text(id.instance.root().as_str()),
        Value::List(id.instance.path().iter().map(|s| text(s)).collect()),
        text(&id.authored),
    ])
}

impl Prepared {
    /// Trusted-control-plane composition. Every body is package-verified; exact
    /// metadata/root authorize placement/exposure/wiring, NOT package-attested
    /// boundaries. Policy must explicitly name effective IDs; no grants inferred.
    pub fn prepare_recursive_graph_v2(
        composition: crate::resolve::Composition,
        definitions: &[GraphDefinition],
        root: &GraphDefinitionId,
        budget: ExpansionBudget,
        policy: &TrustedCompositionPolicy,
    ) -> Result<Self, Failure> {
        let mut prepared = Self::prepare_inner(composition, true)?;
        let expanded = lower_recursive(definitions, root, budget)?;
        for definition in definitions {
            let verified = prepared
                .packages
                .get(&definition.body.contributor)
                .and_then(|p| p.graph_v2());
            verify_body(&definition.body, verified)?;
        }
        let declarations = materialize(&expanded)?;
        let mut graph = prepared.compile_selected_graph_v2(&declarations, policy)?;
        let mut bytes = b"phenix.graph.recursive-plan.v1\0".to_vec();
        bytes.extend(
            Value::List(vec![text(graph.identity()), metadata(definitions, root)?]).encode()?,
        );
        graph.identity = phenix_artifact::digest(&bytes);
        graph.recursive = Some(expanded);
        prepared.graph_v2 = Some(graph);
        Ok(prepared)
    }
}

fn materialize(expanded: &ExpandedGraph) -> Result<Vec<GraphContributionV2>, Failure> {
    let owner = |id: &LogicalId| expanded.instances[&id.instance].owner();
    let mut groups = BTreeMap::new();
    for definition in expanded.instances.values() {
        groups
            .entry(definition.owner().to_owned())
            .or_insert_with(|| GraphContributionV2 {
                contributor: definition.owner().into(),
                sources: vec![],
                nodes: vec![],
                edges: vec![],
                patches: vec![],
            });
    }
    let node_id = |id: &LogicalId| -> Result<String, Failure> {
        if !expanded.nodes.contains_key(id) {
            return Err(invalid("missing recursive node"));
        }
        recursive_effective_id(owner(id), "node", id)
    };
    let mut ids = BTreeSet::new();
    for (id, source) in &expanded.sources {
        let mut source = source.clone();
        source.id = recursive_effective_id(owner(id), "source", id)?;
        if !ids.insert(source.id.clone()) {
            return Err(invalid("recursive identity collision"));
        }
        groups.get_mut(owner(id)).unwrap().sources.push(source);
    }
    for (id, node) in &expanded.nodes {
        let mut node = node.clone();
        node.id = node_id(id)?;
        if !ids.insert(node.id.clone()) {
            return Err(invalid("recursive identity collision"));
        }
        if let NodeKind::Source { source } = &mut node.kind {
            let source_id = LogicalId {
                instance: id.instance.clone(),
                authored: source.clone(),
            };
            if !expanded.sources.contains_key(&source_id) {
                return Err(invalid("missing recursive source"));
            }
            *source = recursive_effective_id(owner(id), "source", &source_id)?;
        }
        groups.get_mut(owner(id)).unwrap().nodes.push(node);
    }
    for (origin, edge) in &expanded.edges {
        let (id, kind) = match origin {
            EdgeOrigin::Body(id) => (id, "body-edge"),
            EdgeOrigin::Wiring(id) => (id, "wire"),
            EdgeOrigin::Order(id) => (id, "order"),
        };
        let effective = recursive_effective_id(owner(id), kind, id)?;
        if !ids.insert(effective.clone()) {
            return Err(invalid("recursive identity collision"));
        }
        let kind = match edge {
            LogicalEdge::Flow { from, to } => EdgeKind::Flow {
                from: OutPortRef {
                    node: node_id(&from.node)?,
                    port: from.port.clone(),
                },
                to: InPortRef {
                    node: node_id(&to.node)?,
                    port: to.port.clone(),
                },
            },
            LogicalEdge::Order { before, after } => EdgeKind::Order {
                before: node_id(before)?,
                after: node_id(after)?,
            },
            LogicalEdge::Deferred { from, to, next } => EdgeKind::Deferred {
                from: OutPortRef {
                    node: node_id(&from.node)?,
                    port: from.port.clone(),
                },
                to: InPortRef {
                    node: node_id(&to.node)?,
                    port: to.port.clone(),
                },
                next: *next,
            },
        };
        groups.get_mut(owner(id)).unwrap().edges.push(EdgeDecl {
            id: effective,
            kind,
        });
    }
    groups.into_values().map(|d| d.normalized()).collect()
}

fn boundary_from_value(endpoint: &BoundaryFrom) -> Value {
    match endpoint {
        BoundaryFrom::Local(p) => Value::List(vec![text("local"), text(&p.node), text(&p.port)]),
        BoundaryFrom::Child { child, port } => {
            Value::List(vec![text("child"), text(child), text(port)])
        }
    }
}

fn boundary_to_value(endpoint: &BoundaryTo) -> Value {
    match endpoint {
        BoundaryTo::Local(p) => Value::List(vec![text("local"), text(&p.node), text(&p.port)]),
        BoundaryTo::Child { child, port } => {
            Value::List(vec![text("child"), text(child), text(port)])
        }
    }
}

fn anchor_ref_value(reference: &AnchorRef) -> Value {
    match reference {
        AnchorRef::Local(name) => Value::List(vec![text("local"), text(name)]),
        AnchorRef::Child { child, anchor } => {
            Value::List(vec![text("child"), text(child), text(anchor)])
        }
    }
}

// All alias hops have exact contracts; bodies plus the complete typed alias
// topology therefore include their metadata, even for unused exports.
fn metadata(definitions: &[GraphDefinition], root: &GraphDefinitionId) -> Result<Value, Failure> {
    let mut definitions = definitions
        .iter()
        .map(GraphDefinition::normalized)
        .collect::<Result<Vec<_>, _>>()?;
    definitions.sort_by(|a, b| a.id.cmp(&b.id));
    let values = definitions
        .iter()
        .map(|d| {
            Ok(Value::List(vec![
                text(d.id.as_str()),
                Value::Bytes(d.body.encode()?),
                Value::List(
                    d.inputs
                        .iter()
                        .map(|b| {
                            Value::List(vec![text(&b.port.name), boundary_to_value(&b.target)])
                        })
                        .collect(),
                ),
                Value::List(
                    d.outputs
                        .iter()
                        .map(|b| {
                            Value::List(vec![text(&b.port.name), boundary_from_value(&b.source)])
                        })
                        .collect(),
                ),
                Value::List(
                    d.children
                        .iter()
                        .map(|c| Value::List(vec![text(&c.name), text(c.definition.as_str())]))
                        .collect(),
                ),
                Value::List(
                    d.wiring
                        .iter()
                        .map(|w| {
                            let from = boundary_from_value(&w.from);
                            let to = boundary_to_value(&w.to);
                            Value::List(vec![
                                text(&w.name),
                                from,
                                to,
                                text(match w.next {
                                    None => "flow",
                                    Some(DeferredWave::Next) => "next",
                                }),
                            ])
                        })
                        .collect(),
                ),
                Value::List(
                    d.anchors
                        .iter()
                        .map(|a| {
                            Value::List(vec![
                                text(&a.name),
                                text(match a.visibility {
                                    AnchorVisibility::Public => "public",
                                    AnchorVisibility::Private => "private",
                                }),
                                match &a.target {
                                    AnchorTarget::LocalNode(node) => {
                                        Value::List(vec![text("local-node"), text(node)])
                                    }
                                    AnchorTarget::Child { child, anchor } => {
                                        Value::List(vec![text("child"), text(child), text(anchor)])
                                    }
                                },
                            ])
                        })
                        .collect(),
                ),
                Value::List(
                    d.orders
                        .iter()
                        .map(|o| {
                            Value::List(vec![
                                text(&o.name),
                                anchor_ref_value(&o.before),
                                anchor_ref_value(&o.after),
                            ])
                        })
                        .collect(),
                ),
            ]))
        })
        .collect::<Result<Vec<_>, Failure>>()?;
    Ok(Value::List(vec![text(root.as_str()), Value::List(values)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (GraphDefinition, TrustedCompositionPolicy) {
        let (mut declarations, policy) = super::super::tests::fixture();
        let mut body = declarations.remove(0);
        body.edges.retain(
            |e| matches!(&e.kind, EdgeKind::Flow { to, .. } if to.node.starts_with("alpha/")),
        );
        let definition = GraphDefinition {
            anchors: vec![],
            orders: vec![],
            id: GraphDefinitionId::new("alpha/main").unwrap(),
            body,
            inputs: vec![],
            outputs: vec![],
            children: vec![],
            wiring: vec![],
        };
        (definition, policy)
    }

    fn budget() -> ExpansionBudget {
        ExpansionBudget {
            depth: 8,
            occurrences: 16,
            items: 128,
        }
    }

    #[test]
    fn exact_body_verification_rejects_missing_or_changed_package_metadata() {
        let (d, _) = fixture();
        assert_eq!(verify_body(&d.body, None).unwrap_err().code, Code::Denied);
        assert!(verify_body(&d.body, Some(&d.body)).is_ok());
        let mut changed = d.body.clone();
        changed.sources[0].kind = SourceKind::ExternalAction {
            action: "other".into(),
        };
        assert_eq!(
            verify_body(&changed, Some(&d.body)).unwrap_err().code,
            Code::Denied
        );
        let mut reordered = d.body.clone();
        reordered.nodes.reverse();
        assert!(verify_body(&reordered, Some(&d.body)).is_ok());
    }

    #[test]
    fn rewrites_sources_flow_and_order_and_retains_original_owner() {
        let (mut d, _) = fixture();
        let EdgeKind::Flow { from, to } = d.body.edges[0].kind.clone() else {
            panic!()
        };
        d.body.edges.push(EdgeDecl {
            id: "alpha/deferred".into(),
            kind: EdgeKind::Deferred {
                from,
                to,
                next: DeferredWave::Next,
            },
        });
        d.body.edges.push(EdgeDecl {
            id: "alpha/order".into(),
            kind: EdgeKind::Order {
                before: "alpha/source".into(),
                after: "alpha/first".into(),
            },
        });
        let expanded = lower_recursive(std::slice::from_ref(&d), &d.id, budget()).unwrap();
        let flat = materialize(&expanded).unwrap();
        assert_eq!(flat[0].contributor, "alpha");
        let instance = GraphInstanceId::new(d.id.clone());
        let logical = |authored: &str| LogicalId {
            instance: instance.clone(),
            authored: authored.into(),
        };
        let source = recursive_effective_id("alpha", "source", &logical("alpha/submit")).unwrap();
        let start = recursive_effective_id("alpha", "node", &logical("alpha/source")).unwrap();
        let invoke = recursive_effective_id("alpha", "node", &logical("alpha/first")).unwrap();
        assert_eq!(flat[0].sources[0].id, source);
        assert_eq!(flat[0].sources[0].owner, "alpha");
        assert!(flat[0].nodes.iter().any(|n| n.id == start
            && n.kind
                == NodeKind::Source {
                    source: source.clone()
                }));
        assert!(flat[0].edges.iter().any(|e| matches!(&e.kind, EdgeKind::Flow { from, to } if from.node == start && to.node == invoke)));
        let deferred_id =
            recursive_effective_id("alpha", "body-edge", &logical("alpha/deferred")).unwrap();
        assert!(flat[0].edges.iter().any(|e| e.id == deferred_id && matches!(&e.kind, EdgeKind::Deferred { from, to, next: DeferredWave::Next } if from.node == start && to.node == invoke)));
        assert!(flat[0].edges.iter().any(|e| matches!(&e.kind, EdgeKind::Order { before, after } if before == &start && after == &invoke)));
        assert_eq!(flat, materialize(&expanded).unwrap());
        assert_ne!(
            start,
            recursive_effective_id(
                "alpha",
                "node",
                &LogicalId {
                    instance: instance.child("other").unwrap(),
                    authored: "alpha/source".into(),
                }
            )
            .unwrap()
        );
        assert_ne!(start, source);
    }

    #[test]
    fn authored_source_policy_does_not_authorize_effective_source() {
        let (mut d, mut policy) = fixture();
        d.body.nodes.truncate(1);
        d.body.edges.clear();
        let expanded = lower_recursive(std::slice::from_ref(&d), &d.id, budget()).unwrap();
        let flat = materialize(&expanded).unwrap();
        let compile = |p: &TrustedCompositionPolicy| {
            super::super::compile(&flat, p, |_| true, |_, _, _| panic!("no invokes"))
        };
        assert_eq!(compile(&policy).unwrap_err().code, Code::Denied);
        policy
            .sources
            .insert(flat[0].sources[0].id.clone(), flat[0].sources[0].security);
        assert!(compile(&policy).is_ok());
        let mut malformed = expanded.clone();
        malformed.sources.clear();
        assert!(materialize(&malformed).is_err());
    }

    #[test]
    fn parent_wiring_retains_contributor_and_requires_exact_foreign_policy() {
        for next in [None, Some(DeferredWave::Next)] {
            let (mut root, mut policy) = fixture();
            let (mut bodies, _) = super::super::tests::fixture();
            let body = bodies.remove(1);
            let leaf = GraphDefinition {
                anchors: vec![],
                orders: vec![],
                id: GraphDefinitionId::new("beta/leaf").unwrap(),
                inputs: vec![BoundaryInput {
                    port: body.nodes[0].inputs[0].clone(),
                    target: BoundaryTo::Local(InPortRef {
                        node: "beta/second".into(),
                        port: "in".into(),
                    }),
                }],
                body,
                outputs: vec![],
                children: vec![],
                wiring: vec![],
            };
            let mut child = leaf.clone();
            child.id = GraphDefinitionId::new("beta/middle").unwrap();
            child.children.push(GraphChild {
                name: "leaf".into(),
                definition: leaf.id.clone(),
            });
            child.inputs[0].target = BoundaryTo::Child {
                child: "leaf".into(),
                port: "in".into(),
            };
            root.children.push(GraphChild {
                name: "nested".into(),
                definition: child.id.clone(),
            });
            root.wiring.push(BoundaryFlow {
                next,
                name: "connect".into(),
                from: BoundaryFrom::Local(OutPortRef {
                    node: "alpha/first".into(),
                    port: "ok".into(),
                }),
                to: BoundaryTo::Child {
                    child: "nested".into(),
                    port: "in".into(),
                },
            });
            let definitions = [root.clone(), child, leaf];
            let mut changed = definitions.clone();
            changed[0].wiring[0].next = if next.is_some() {
                None
            } else {
                Some(DeferredWave::Next)
            };
            assert_ne!(
                metadata(&definitions, &root.id).unwrap(),
                metadata(&changed, &root.id).unwrap()
            );
            let expanded = lower_recursive(&definitions, &root.id, budget()).unwrap();
            let origin = EdgeOrigin::Wiring(LogicalId {
                instance: GraphInstanceId::new(root.id.clone()),
                authored: "connect".into(),
            });
            assert_eq!(
                matches!(expanded.edges[&origin], LogicalEdge::Deferred { .. }),
                next.is_some()
            );
            let flat = materialize(&expanded).unwrap();
            let leaf_id = recursive_effective_id(
                "beta",
                "node",
                &LogicalId {
                    instance: GraphInstanceId::new(root.id.clone())
                        .child("nested")
                        .unwrap()
                        .child("leaf")
                        .unwrap(),
                    authored: "beta/second".into(),
                },
            )
            .unwrap();
            assert!(flat[0].edges.iter().any(|e| matches!(&e.kind,
                EdgeKind::Flow { to, .. } | EdgeKind::Deferred { to, .. } if to.node == leaf_id)));
            assert_eq!(flat[0].contributor, "alpha");
            assert_eq!(flat[1].contributor, "beta");
            assert_eq!(flat[0].edges.len(), 2);
            assert!(flat[1].edges.is_empty());
            policy
                .sources
                .insert(flat[0].sources[0].id.clone(), flat[0].sources[0].security);
            // Reconstruct exact delivery entries only; authored topology must fail.
            for edge in &flat[0].edges {
                let (EdgeKind::Flow { from, to } | EdgeKind::Deferred { from, to, .. }) =
                    &edge.kind
                else {
                    panic!()
                };
                policy.delivery.insert(
                    (
                        from.node.clone(),
                        from.port.clone(),
                        to.node.clone(),
                        to.port.clone(),
                    ),
                    FlowGrant {
                        resources: BTreeSet::from(["resource".into()]),
                        maximum_classification: Classification::Public,
                    },
                );
            }
            let run = |p: &TrustedCompositionPolicy| {
                super::super::compile(
                    &flat,
                    p,
                    |_| true,
                    |owner, _, _| {
                        assert!(owner == "alpha" || owner == "beta");
                        Ok((
                            "provider".into(),
                            OperationSchema {
                                name: "run".into(),
                                request: phenix_contract::ValueSchema::String,
                                response: phenix_contract::ValueSchema::String,
                                error: phenix_contract::ValueSchema::String,
                            },
                            BTreeSet::from(["resource".into()]),
                        ))
                    },
                )
            };
            assert_eq!(run(&policy).unwrap_err().code, Code::Denied);
            policy.topology.insert(("alpha".into(), leaf_id));
            assert!(run(&policy).is_ok());
            policy.delivery.clear();
            assert_eq!(run(&policy).unwrap_err().code, Code::Denied);
        }
    }

    #[test]
    fn repeated_action_occurrences_are_not_pretended_to_be_supported() {
        let (mut root, mut policy) = fixture();
        root.body.nodes.truncate(1);
        root.body.edges.clear();
        let mut child = root.clone();
        child.id = GraphDefinitionId::new("alpha/child").unwrap();
        root.children.push(GraphChild {
            name: "again".into(),
            definition: child.id.clone(),
        });
        let expanded = lower_recursive(&[root.clone(), child], &root.id, budget()).unwrap();
        let flat = materialize(&expanded).unwrap();
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].sources.len(), 2);
        assert_ne!(flat[0].sources[0].id, flat[0].sources[1].id);
        for source in &flat[0].sources {
            policy.sources.insert(source.id.clone(), source.security);
        }
        let error = super::super::compile(&flat, &policy, |_| true, |_, _, _| panic!("no invokes"))
            .unwrap_err();
        assert!(error.message.contains("duplicate graph-v2 action"));
    }

    #[test]
    fn anchor_orders_materialize_original_owner_exact_policy_and_identity_without_delivery() {
        let (mut root, mut policy) = fixture();
        let (mut bodies, _) = super::super::tests::fixture();
        let leaf = GraphDefinition {
            id: GraphDefinitionId::new("beta/leaf").unwrap(),
            body: bodies.remove(1),
            inputs: vec![],
            outputs: vec![],
            children: vec![],
            wiring: vec![],
            orders: vec![],
            anchors: vec![GraphAnchor {
                name: "ready".into(),
                visibility: AnchorVisibility::Public,
                target: AnchorTarget::LocalNode("beta/second".into()),
            }],
        };
        let mut middle = leaf.clone();
        middle.id = GraphDefinitionId::new("beta/middle").unwrap();
        middle.children.push(GraphChild {
            name: "leaf".into(),
            definition: leaf.id.clone(),
        });
        middle.anchors[0].target = AnchorTarget::Child {
            child: "leaf".into(),
            anchor: "ready".into(),
        };
        root.children.push(GraphChild {
            name: "nested".into(),
            definition: middle.id.clone(),
        });
        root.anchors = vec![
            GraphAnchor {
                name: "before".into(),
                visibility: AnchorVisibility::Private,
                target: AnchorTarget::LocalNode("alpha/first".into()),
            },
            GraphAnchor {
                name: "selected".into(),
                visibility: AnchorVisibility::Public,
                target: AnchorTarget::Child {
                    child: "nested".into(),
                    anchor: "ready".into(),
                },
            },
        ];
        root.orders.push(BoundaryOrder {
            name: "wait".into(),
            before: AnchorRef::Local("before".into()),
            after: AnchorRef::Child {
                child: "nested".into(),
                anchor: "ready".into(),
            },
        });
        let defs = [root.clone(), middle.clone(), leaf.clone()];
        let expanded = lower_recursive(&defs, &root.id, budget()).unwrap();
        let flat = materialize(&expanded).unwrap();
        let id = LogicalId {
            instance: GraphInstanceId::new(root.id.clone()),
            authored: "wait".into(),
        };
        let effective = recursive_effective_id("alpha", "order", &id).unwrap();
        assert_ne!(
            effective,
            recursive_effective_id("alpha", "wire", &id).unwrap()
        );
        let target = expanded
            .resolve_anchor(&id.instance, &AnchorRef::Local("selected".into()))
            .unwrap();
        let target_id = recursive_effective_id("beta", "node", &target.node).unwrap();
        assert!(flat[0].edges.iter().any(|e| e.id == effective
            && matches!(&e.kind, EdgeKind::Order { after, .. } if *after == target_id)));
        assert_eq!(flat[0].contributor, "alpha");
        assert_eq!(expanded.orders[&id].after.len(), 2);
        policy.sources.clear();
        policy
            .sources
            .insert(flat[0].sources[0].id.clone(), flat[0].sources[0].security);
        policy.delivery.clear();
        let labels = policy.operations.values().next().unwrap().clone();
        for owner in ["alpha", "beta"] {
            policy
                .operations
                .insert((owner.into(), "example.api@1/run".into()), labels.clone());
        }
        let run = |p: &TrustedCompositionPolicy| {
            super::super::compile(
                &flat,
                p,
                |_| true,
                |owner, _, _| {
                    Ok((
                        owner.into(),
                        OperationSchema {
                            name: "run".into(),
                            request: phenix_contract::ValueSchema::String,
                            response: phenix_contract::ValueSchema::String,
                            error: phenix_contract::ValueSchema::String,
                        },
                        BTreeSet::from(["resource".into()]),
                    ))
                },
            )
        };
        assert_eq!(run(&policy).unwrap_err().code, Code::Denied);
        policy.topology.insert(("alpha".into(), target_id));
        assert!(
            run(&policy).is_ok(),
            "Order must not require a delivery grant"
        );
        let identity = metadata(&defs, &root.id).unwrap();
        root.anchors[0].visibility = AnchorVisibility::Public;
        assert_ne!(
            identity,
            metadata(&[root.clone(), middle.clone(), leaf.clone()], &root.id).unwrap()
        );
        root.anchors[0].visibility = AnchorVisibility::Private;
        root.orders[0].after = AnchorRef::Local("selected".into());
        let changed = [root.clone(), middle.clone(), leaf.clone()];
        assert_eq!(
            flat,
            materialize(&lower_recursive(&changed, &root.id, budget()).unwrap()).unwrap()
        );
        assert_ne!(identity, metadata(&changed, &root.id).unwrap());
        root.orders[0].after = defs[0].orders[0].after.clone();
        root.anchors.reverse();
        root.orders.reverse();
        assert_eq!(
            identity,
            metadata(&[leaf, middle, root.clone()], &root.id).unwrap()
        );
    }

    #[test]
    fn alias_kind_and_child_name_are_identity_metadata_not_flat_authority() {
        let (mut bodies, _) = super::super::tests::fixture();
        let body = bodies.remove(1);
        let mut leaf = GraphDefinition {
            anchors: vec![],
            orders: vec![],
            id: GraphDefinitionId::new("beta/leaf").unwrap(),
            body,
            inputs: vec![],
            outputs: vec![],
            children: vec![],
            wiring: vec![],
        };
        leaf.inputs.push(BoundaryInput {
            port: leaf.body.nodes[0].inputs[0].clone(),
            target: BoundaryTo::Local(InPortRef {
                node: "beta/second".into(),
                port: "in".into(),
            }),
        });
        let mut alternate = leaf.inputs[0].clone();
        alternate.port.name = "alternate".into();
        leaf.inputs.push(alternate);
        leaf.outputs.push(BoundaryOutput {
            port: OutputPort {
                name: "value".into(),
                ..leaf.body.nodes[0]
                    .outputs
                    .iter()
                    .find(|p| p.name == "ok")
                    .unwrap()
                    .clone()
            },
            source: BoundaryFrom::Local(OutPortRef {
                node: "beta/second".into(),
                port: "ok".into(),
            }),
        });
        let mut root = leaf.clone();
        root.id = GraphDefinitionId::new("beta/root").unwrap();
        root.inputs.truncate(1);
        root.inputs[0].target = BoundaryTo::Child {
            child: "nested".into(),
            port: "in".into(),
        };
        root.outputs[0].source = BoundaryFrom::Child {
            child: "nested".into(),
            port: "value".into(),
        };
        root.children = vec![GraphChild {
            name: "nested".into(),
            definition: leaf.id.clone(),
        }];
        let defs = [root.clone(), leaf.clone()];
        let expanded = lower_recursive(&defs, &root.id, budget()).unwrap();
        let flat = materialize(&expanded).unwrap();
        let identity = metadata(&defs, &root.id).unwrap();
        root.inputs[0].target = BoundaryTo::Child {
            child: "nested".into(),
            port: "alternate".into(),
        };
        let renamed = [root.clone(), leaf.clone()];
        assert_eq!(
            flat,
            materialize(&lower_recursive(&renamed, &root.id, budget()).unwrap()).unwrap()
        );
        assert_ne!(identity, metadata(&renamed, &root.id).unwrap());
        root.inputs[0].target = BoundaryTo::Local(InPortRef {
            node: "beta/second".into(),
            port: "in".into(),
        });
        root.outputs[0].source = leaf.outputs[0].source.clone();
        let local = [root.clone(), leaf.clone()];
        assert_eq!(
            flat,
            materialize(&lower_recursive(&local, &root.id, budget()).unwrap()).unwrap()
        );
        assert_ne!(identity, metadata(&local, &root.id).unwrap());
        root.children[0].name = "other".into();
        assert_ne!(
            metadata(&local, &root.id).unwrap(),
            metadata(&[root.clone(), leaf.clone()], &root.id).unwrap()
        );
        leaf.inputs.reverse();
        assert_eq!(
            identity,
            metadata(&[leaf, defs[0].clone()], &root.id).unwrap()
        );
    }

    #[test]
    fn unused_definitions_and_exposure_change_identity_metadata_not_flat_graph() {
        let (d, _) = fixture();
        let mut exposed = d.clone();
        exposed.outputs.push(BoundaryOutput {
            port: OutputPort {
                name: "exposed".into(),
                ..d.body.nodes[0].outputs[0].clone()
            },
            source: BoundaryFrom::Local(OutPortRef {
                node: "alpha/source".into(),
                port: "out".into(),
            }),
        });
        let flat = |defs: &[GraphDefinition]| {
            materialize(&lower_recursive(defs, &d.id, budget()).unwrap()).unwrap()
        };
        assert_eq!(
            flat(std::slice::from_ref(&d)),
            flat(std::slice::from_ref(&exposed))
        );
        assert_ne!(
            metadata(std::slice::from_ref(&d), &d.id).unwrap(),
            metadata(&[exposed], &d.id).unwrap()
        );
        let mut unused = d.clone();
        unused.id = GraphDefinitionId::new("alpha/unused").unwrap();
        let defs = vec![d.clone(), unused.clone()];
        assert_eq!(flat(std::slice::from_ref(&d)), flat(&defs));
        assert_ne!(
            metadata(std::slice::from_ref(&d), &d.id).unwrap(),
            metadata(&defs, &d.id).unwrap()
        );
        assert_eq!(
            metadata(&defs, &d.id).unwrap(),
            metadata(&[unused, d.clone()], &d.id).unwrap()
        );
    }
}
