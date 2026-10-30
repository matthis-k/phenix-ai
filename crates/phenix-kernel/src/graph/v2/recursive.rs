use super::*;
use composition::*;

/// Deterministic bounded owner/local name only; grants no authority. `kind` is
/// "node", "source", "body-edge" or "wire". Owner is the ORIGINAL contributor.
pub fn recursive_effective_id(owner: &str, kind: &str, id: &LogicalId) -> Result<String, Failure> {
    if !phenix_contract::identifier(owner)
        || owner == "root"
        || !matches!(kind, "node" | "source" | "body-edge" | "wire")
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
        };
        groups.get_mut(owner(id)).unwrap().edges.push(EdgeDecl {
            id: effective,
            kind,
        });
    }
    groups.into_values().map(|d| d.normalized()).collect()
}

// Boundary port metadata equals its body endpoint by normalization validation;
// encoding body + boundary name/endpoint therefore includes the full metadata.
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
                            Value::List(vec![
                                text(&b.port.name),
                                text(&b.target.node),
                                text(&b.target.port),
                            ])
                        })
                        .collect(),
                ),
                Value::List(
                    d.outputs
                        .iter()
                        .map(|b| {
                            Value::List(vec![
                                text(&b.port.name),
                                text(&b.source.node),
                                text(&b.source.port),
                            ])
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
                            let from = match &w.from {
                                BoundaryFrom::Local(p) => {
                                    Value::List(vec![text("local"), text(&p.node), text(&p.port)])
                                }
                                BoundaryFrom::Child { child, port } => {
                                    Value::List(vec![text("child"), text(child), text(port)])
                                }
                            };
                            let to = match &w.to {
                                BoundaryTo::Local(p) => {
                                    Value::List(vec![text("local"), text(&p.node), text(&p.port)])
                                }
                                BoundaryTo::Child { child, port } => {
                                    Value::List(vec![text("child"), text(child), text(port)])
                                }
                            };
                            Value::List(vec![text(&w.name), from, to])
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
        let (mut root, mut policy) = fixture();
        let (mut bodies, _) = super::super::tests::fixture();
        let body = bodies.remove(1);
        let child = GraphDefinition {
            id: GraphDefinitionId::new("beta/child").unwrap(),
            inputs: vec![BoundaryInput {
                port: body.nodes[0].inputs[0].clone(),
                target: InPortRef {
                    node: "beta/second".into(),
                    port: "in".into(),
                },
            }],
            body,
            outputs: vec![],
            children: vec![],
            wiring: vec![],
        };
        root.children.push(GraphChild {
            name: "nested".into(),
            definition: child.id.clone(),
        });
        root.wiring.push(BoundaryFlow {
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
        let expanded = lower_recursive(&[root.clone(), child], &root.id, budget()).unwrap();
        let flat = materialize(&expanded).unwrap();
        assert_eq!(flat[0].contributor, "alpha");
        assert_eq!(flat[1].contributor, "beta");
        assert_eq!(flat[0].edges.len(), 2);
        assert!(flat[1].edges.is_empty());
        policy
            .sources
            .insert(flat[0].sources[0].id.clone(), flat[0].sources[0].security);
        // Reconstruct exact delivery entries only; authored topology must fail.
        for edge in &flat[0].edges {
            let EdgeKind::Flow { from, to } = &edge.kind else {
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
        policy
            .topology
            .insert(("alpha".into(), flat[1].nodes[0].id.clone()));
        assert!(run(&policy).is_ok());
        policy.delivery.clear();
        assert_eq!(run(&policy).unwrap_err().code, Code::Denied);
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
    fn unused_definitions_and_exposure_change_identity_metadata_not_flat_graph() {
        let (d, _) = fixture();
        let mut exposed = d.clone();
        exposed.outputs.push(BoundaryOutput {
            port: OutputPort {
                name: "exposed".into(),
                ..d.body.nodes[0].outputs[0].clone()
            },
            source: OutPortRef {
                node: "alpha/source".into(),
                port: "out".into(),
            },
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
