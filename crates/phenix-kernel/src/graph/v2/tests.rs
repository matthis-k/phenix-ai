use super::*;
use phenix_contract::ValueSchema;

const PUBLIC: PortSecurity = PortSecurity {
    confidentiality: Classification::Public,
    integrity: IntegrityClass::Untrusted,
};

pub(super) fn join_fixture() -> (Vec<GraphContributionV2>, TrustedCompositionPolicy) {
    let (mut d, p) = merge_fixture();
    let join = d[0]
        .nodes
        .iter_mut()
        .find(|n| n.id == "alpha/merge")
        .unwrap();
    join.kind = NodeKind::Join {
        policy: JoinPolicy::All,
    };
    join.inputs[0].name = "left".into();
    let mut right = join.inputs[0].clone();
    right.name = "right".into();
    join.inputs.push(right);
    let schema = ValueSchema::Struct(
        join.inputs
            .iter()
            .map(|p| (p.name.clone(), p.schema.clone()))
            .collect(),
    );
    join.outputs[0].schema = schema.clone();
    let sink = d[0]
        .nodes
        .iter_mut()
        .find(|n| n.id == "alpha/sink")
        .unwrap();
    sink.inputs[0].schema = schema.clone();
    sink.outputs[0].schema = schema;
    for edge in &mut d[0].edges {
        if let EdgeKind::Flow { from, to } = &mut edge.kind {
            if edge.id == "alpha/a" {
                to.port = "left".into();
            }
            if edge.id == "alpha/b" {
                from.node = "alpha/source".into();
                to.port = "right".into();
            }
        }
    }
    (d, p)
}

#[test]
fn join_sources_dormancy_order_and_candidate_limits() {
    let (mut d, p) = join_fixture();
    let graph = run(&d, &p).unwrap();
    assert_eq!(
        graph.join_satisfiability()["alpha/merge"],
        Satisfiability::ProvenSatisfiable
    );
    d[0].nodes.reverse();
    d[0].edges.reverse();
    assert_eq!(graph.identity(), run(&d, &p).unwrap().identity());
    assert_eq!(
        graph.join_satisfiability(),
        run(&d, &p).unwrap().join_satisfiability()
    );
    let (mut d, p) = join_fixture();
    let EdgeKind::Flow { from, .. } = &mut d[0].edges[1].kind else {
        panic!()
    };
    from.node = "alpha/alternative-source".into();
    d[0].edges.push(EdgeDecl {
        id: "alpha/order".into(),
        kind: EdgeKind::Order {
            before: "alpha/source".into(),
            after: "alpha/alternative-source".into(),
        },
    });
    assert!(run(&d, &p)
        .unwrap_err()
        .message
        .contains("disjoint input sources"));
    d[0].edges.remove(1);
    assert_eq!(
        run(&d, &p).unwrap().join_satisfiability()["alpha/merge"],
        Satisfiability::Dormant
    );
    let (mut d, p) = join_fixture();
    let mut duplicate = d[0].edges[0].clone();
    duplicate.id = "alpha/duplicate".into();
    d[0].edges.push(duplicate);
    assert!(run(&d, &p).unwrap_err().message.contains("at most one"));
}

#[test]
fn join_direct_switch_exclusion_and_nested_unknown() {
    let (mut d, mut p) = join_fixture();
    let tagged = ValueSchema::Tagged(BTreeMap::from([
        ("one".into(), Some(ValueSchema::String)),
        ("two".into(), Some(ValueSchema::String)),
    ]));
    d[0].sources[0].payload = tagged.clone();
    d[0].nodes[0].outputs[0].schema = tagged.clone();
    let mut switch = d[0].nodes[2].clone();
    switch.id = "alpha/switch".into();
    switch.kind = NodeKind::Switch {
        tagged_schema: tagged.id().unwrap(),
    };
    switch.inputs = vec![InputPort {
        name: "in".into(),
        schema: tagged,
        readiness: InputReadiness::One,
        security: PUBLIC,
    }];
    switch.outputs = ["one", "two"]
        .into_iter()
        .map(|name| OutputPort {
            name: name.into(),
            schema: ValueSchema::String,
            security: PUBLIC,
        })
        .collect();
    d[0].nodes.push(switch);
    for edge in &mut d[0].edges[..2] {
        let EdgeKind::Flow { from, to } = &mut edge.kind else {
            panic!()
        };
        from.node = "alpha/switch".into();
        from.port = if to.port == "left" { "one" } else { "two" }.into();
    }
    d[0].edges.push(EdgeDecl {
        id: "alpha/start".into(),
        kind: EdgeKind::Flow {
            from: OutPortRef {
                node: "alpha/source".into(),
                port: "out".into(),
            },
            to: InPortRef {
                node: "alpha/switch".into(),
                port: "in".into(),
            },
        },
    });
    assert!(run(&d, &p)
        .unwrap_err()
        .message
        .contains("Switch case exclusion"));
    let mut merge = d[0].nodes[3].clone();
    merge.id = "alpha/intermediate".into();
    merge.inputs[0].schema = ValueSchema::String;
    merge.outputs[0].schema = ValueSchema::String;
    d[0].nodes.push(merge);
    let EdgeKind::Flow { to, .. } = &mut d[0].edges[1].kind else {
        panic!()
    };
    to.node = "alpha/intermediate".into();
    to.port = "in".into();
    d[0].edges.push(EdgeDecl {
        id: "alpha/nested".into(),
        kind: EdgeKind::Flow {
            from: OutPortRef {
                node: "alpha/intermediate".into(),
                port: "out".into(),
            },
            to: InPortRef {
                node: "alpha/merge".into(),
                port: "right".into(),
            },
        },
    });
    let graph = run(&d, &p).unwrap();
    assert_eq!(
        graph.join_satisfiability()["alpha/merge"],
        Satisfiability::Unknown
    );
    d[0].nodes.reverse();
    d[0].edges.reverse();
    p.topology.clear();
    assert_eq!(
        graph.join_satisfiability(),
        run(&d, &p).unwrap().join_satisfiability()
    );
}
pub(super) fn fixture() -> (Vec<GraphContributionV2>, TrustedCompositionPolicy) {
    let execution = ExecutionPolicy {
        on_domain_error: DomainErrorPolicy::RouteErr,
    };
    let source = SourceDecl {
        id: "alpha/submit".into(),
        owner: "alpha".into(),
        kind: SourceKind::ExternalAction {
            action: "submit".into(),
        },
        payload: ValueSchema::String,
        security: PUBLIC,
    };
    let start = NodeDecl {
        id: "alpha/source".into(),
        kind: NodeKind::Source {
            source: source.id.clone(),
        },
        inputs: vec![],
        outputs: vec![OutputPort {
            name: "out".into(),
            schema: ValueSchema::String,
            security: PUBLIC,
        }],
        execution,
    };
    let invoke = |id: &str| NodeDecl {
        id: id.into(),
        kind: NodeKind::Invoke {
            selector: "selected".into(),
            operation: "example.api@1/run".into(),
        },
        inputs: vec![InputPort {
            name: "in".into(),
            schema: ValueSchema::String,
            readiness: InputReadiness::One,
            security: PUBLIC,
        }],
        outputs: ["err", "ok"]
            .into_iter()
            .map(|name| OutputPort {
                name: name.into(),
                schema: ValueSchema::String,
                security: PUBLIC,
            })
            .collect(),
        execution,
    };
    let flow = |id: &str, from: &str, port: &str, to: &str| EdgeDecl {
        id: id.into(),
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
    let declarations = vec![
        GraphContributionV2 {
            contributor: "alpha".into(),
            sources: vec![source],
            nodes: vec![start, invoke("alpha/first")],
            edges: vec![
                flow("alpha/start", "alpha/source", "out", "alpha/first"),
                flow("alpha/next", "alpha/first", "ok", "beta/second"),
            ],
            patches: vec![],
        },
        GraphContributionV2 {
            contributor: "beta".into(),
            sources: vec![],
            nodes: vec![invoke("beta/second")],
            edges: vec![],
            patches: vec![],
        },
    ];
    let mut policy = TrustedCompositionPolicy::default();
    policy
        .topology
        .insert(("alpha".into(), "beta/second".into()));
    policy.source_owners.insert(
        "alpha".into(),
        SourceOwnerPolicy {
            owner_ceiling: Classification::Public,
            composition_ceiling: Classification::Public,
        },
    );
    policy.sources.insert("alpha/submit".into(), PUBLIC);
    policy.operations.insert(
        ("provider".into(), "example.api@1/run".into()),
        OperationSecurity {
            input: PUBLIC,
            ok: PUBLIC,
            err: PUBLIC,
        },
    );
    for (from, port, to) in [
        ("alpha/source", "out", "alpha/first"),
        ("alpha/first", "ok", "beta/second"),
    ] {
        policy.delivery.insert(
            (from.into(), port.into(), to.into(), "in".into()),
            FlowGrant {
                resources: BTreeSet::from(["resource".into()]),
                maximum_classification: Classification::Public,
            },
        );
    }
    (declarations, policy)
}

pub(super) fn merge_fixture() -> (Vec<GraphContributionV2>, TrustedCompositionPolicy) {
    let (mut declarations, mut policy) = fixture();
    declarations.truncate(1);
    let d = &mut declarations[0];
    d.nodes.truncate(1);
    d.edges.clear();
    let mut second = d.sources[0].clone();
    second.id = "alpha/alternative".into();
    second.kind = SourceKind::ExternalAction {
        action: "alternative".into(),
    };
    policy.sources.insert(second.id.clone(), second.security);
    d.sources.push(second);
    let mut source = d.nodes[0].clone();
    source.id = "alpha/alternative-source".into();
    source.kind = NodeKind::Source {
        source: "alpha/alternative".into(),
    };
    d.nodes.push(source);
    for id in ["alpha/merge", "alpha/sink"] {
        d.nodes.push(NodeDecl {
            id: id.into(),
            kind: NodeKind::Merge {
                policy: MergePolicy::ExactlyOne,
            },
            inputs: vec![InputPort {
                name: "in".into(),
                schema: ValueSchema::String,
                readiness: InputReadiness::One,
                security: PUBLIC,
            }],
            outputs: vec![OutputPort {
                name: "out".into(),
                schema: ValueSchema::String,
                security: PUBLIC,
            }],
            execution: d.nodes[0].execution,
        });
    }
    for (id, from, to) in [
        ("alpha/a", "alpha/source", "alpha/merge"),
        ("alpha/b", "alpha/alternative-source", "alpha/merge"),
        ("alpha/c", "alpha/merge", "alpha/sink"),
    ] {
        d.edges.push(EdgeDecl {
            id: id.into(),
            kind: EdgeKind::Flow {
                from: OutPortRef {
                    node: from.into(),
                    port: "out".into(),
                },
                to: InPortRef {
                    node: to.into(),
                    port: "in".into(),
                },
            },
        });
    }
    (declarations, policy)
}

pub(super) fn switch_merge_fixture(
    classification: Classification,
) -> (Vec<GraphContributionV2>, TrustedCompositionPolicy) {
    let (mut d, mut p) = merge_fixture();
    let label = PortSecurity {
        confidentiality: classification,
        integrity: IntegrityClass::Untrusted,
    };
    let tagged = ValueSchema::Tagged(BTreeMap::from([
        ("one".into(), Some(ValueSchema::String)),
        ("two".into(), Some(ValueSchema::String)),
    ]));
    d[0].sources.truncate(1);
    d[0].sources[0].payload = tagged.clone();
    d[0].sources[0].security = label;
    p.sources.insert("alpha/submit".into(), label);
    let ceilings = p.source_owners.get_mut("alpha").unwrap();
    ceilings.owner_ceiling = classification;
    ceilings.composition_ceiling = classification;
    d[0].nodes.retain(|n| n.id != "alpha/alternative-source");
    let mut switch = d[0].nodes[1].clone();
    switch.id = "alpha/switch".into();
    switch.kind = NodeKind::Switch {
        tagged_schema: tagged.id().unwrap(),
    };
    switch.inputs[0].schema = tagged.clone();
    switch.outputs = ["one", "two"]
        .into_iter()
        .map(|name| OutputPort {
            name: name.into(),
            schema: ValueSchema::String,
            security: label,
        })
        .collect();
    d[0].nodes[0].outputs[0].schema = tagged;
    let mut sink = d[0].nodes.pop().unwrap();
    sink.id = "beta/sink".into();
    d[0].nodes.push(switch);
    for node in d[0].nodes.iter_mut().chain(std::iter::once(&mut sink)) {
        for port in &mut node.inputs {
            port.security = label;
        }
        for port in &mut node.outputs {
            port.security = label;
        }
    }
    d.push(GraphContributionV2 {
        contributor: "beta".into(),
        sources: vec![],
        nodes: vec![sink],
        edges: vec![],
        patches: vec![],
    });
    d[0].edges.clear();
    for (id, from, port, to) in [
        ("alpha/start", "alpha/source", "out", "alpha/switch"),
        ("alpha/one", "alpha/switch", "one", "alpha/merge"),
        ("alpha/two", "alpha/switch", "two", "alpha/merge"),
        ("alpha/two-duplicate", "alpha/switch", "two", "alpha/merge"),
        ("alpha/export", "alpha/merge", "out", "beta/sink"),
    ] {
        d[0].edges.push(EdgeDecl {
            id: id.into(),
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
    p.topology.insert(("alpha".into(), "beta/sink".into()));
    p.delivery.insert(
        (
            "alpha/merge".into(),
            "out".into(),
            "beta/sink".into(),
            "in".into(),
        ),
        FlowGrant {
            resources: BTreeSet::from(["a".into(), "b".into()]),
            maximum_classification: classification,
        },
    );
    (d, p)
}

#[test]
fn confidential_switch_control_cannot_export_merge_outcome_to_public_sink() {
    let (mut d, p) = switch_merge_fixture(Classification::Confidential);
    // one produces one candidate; two produces two candidates and a cardinality failure.
    assert!(run(&d, &p).is_ok());
    let graph = run(&d, &p).unwrap();
    for id in ["alpha/switch", "alpha/merge"] {
        let node = graph.declarations()[0]
            .nodes
            .iter()
            .find(|n| n.id == id)
            .unwrap();
        assert!(node
            .outputs
            .iter()
            .all(|port| port.security.confidentiality == Classification::Confidential));
    }
    let sink = &mut d[1].nodes[0];
    sink.inputs[0].security = PUBLIC;
    sink.outputs[0].security = PUBLIC;
    assert_eq!(run(&d, &p).unwrap_err().code, Code::Denied);
}

#[test]
fn cross_source_merge_is_canonical_and_security_joins_every_candidate() {
    let (mut d, mut p) = merge_fixture();
    let graph = run(&d, &p).unwrap();
    assert_eq!(graph.source_reachability()["alpha/merge"].len(), 2);
    d[0].nodes.reverse();
    d[0].edges.reverse();
    assert_eq!(graph.identity(), run(&d, &p).unwrap().identity());
    let label = PortSecurity {
        confidentiality: Classification::Confidential,
        integrity: IntegrityClass::Untrusted,
    };
    d[0].sources[0].security = label;
    d[0].nodes
        .iter_mut()
        .find(|n| n.id == "alpha/source")
        .unwrap()
        .outputs[0]
        .security = label;
    p.sources.insert("alpha/submit".into(), label);
    let ceilings = p.source_owners.get_mut("alpha").unwrap();
    ceilings.owner_ceiling = Classification::Secret;
    ceilings.composition_ceiling = Classification::Secret;
    assert_eq!(run(&d, &p).unwrap_err().code, Code::Denied);
    for node in &mut d[0].nodes {
        if matches!(node.kind, NodeKind::Merge { .. }) {
            node.inputs[0].security = label;
            node.outputs[0].security = label;
        }
    }
    assert!(run(&d, &p).is_ok());
    d[0].nodes
        .iter_mut()
        .find(|n| n.id == "alpha/merge")
        .unwrap()
        .inputs[0]
        .security
        .integrity = IntegrityClass::Trusted;
    assert_eq!(run(&d, &p).unwrap_err().code, Code::Denied);
}
pub(super) fn run(
    d: &[GraphContributionV2],
    p: &TrustedCompositionPolicy,
) -> Result<CompiledGraph, Failure> {
    compile(
        d,
        p,
        |_| true,
        |_, _, _| {
            Ok((
                "provider".into(),
                OperationSchema {
                    name: "run".into(),
                    request: ValueSchema::String,
                    response: ValueSchema::String,
                    error: ValueSchema::String,
                },
                BTreeSet::from(["resource".into()]),
            ))
        },
    )
}
#[test]
fn chain_permutation_and_identity() {
    let (mut d, mut p) = fixture();
    let first = run(&d, &p).unwrap();
    assert_eq!(
        first.source_reachability()["beta/second"],
        BTreeSet::from(["alpha/submit".into()])
    );
    d.reverse();
    d[1].nodes.reverse();
    d[1].edges.reverse();
    assert_eq!(first.identity(), run(&d, &p).unwrap().identity());
    p.delivery
        .values_mut()
        .next()
        .unwrap()
        .resources
        .insert("other".into());
    assert_ne!(first.identity(), run(&d, &p).unwrap().identity());
}
#[test]
fn schemas_delivery_and_operation_authority() {
    let (mut d, p) = fixture();
    d[1].nodes[0].inputs[0].schema = ValueSchema::Bool;
    assert_eq!(run(&d, &p).unwrap_err().code, Code::SchemaRejected);
    let (d, mut p) = fixture();
    p.delivery.clear();
    assert_eq!(run(&d, &p).unwrap_err().code, Code::Denied);
    let (_, p) = fixture();
    assert!(compile(
        &d,
        &p,
        |_| true,
        |_, _, _| Err(denied("no operation grant"))
    )
    .is_err());
    assert!(compile(&d, &p, |_| false, |_, _, _| unreachable!()).is_err());
}
#[test]
fn cycles_missing_endpoints_and_candidate_counts() {
    let (mut d, mut p) = fixture();
    p.topology.insert(("beta".into(), "alpha/source".into()));
    d[1].edges.push(EdgeDecl {
        id: "beta/cycle".into(),
        kind: EdgeKind::Order {
            before: "beta/second".into(),
            after: "alpha/source".into(),
        },
    });
    assert!(run(&d, &p).unwrap_err().message.contains("cycle"));
    d[1].edges[0].kind = EdgeKind::Order {
        before: "absent/node".into(),
        after: "alpha/source".into(),
    };
    assert!(run(&d, &p).is_err());
    let (mut d, p) = fixture();
    d[0].edges.pop();
    assert!(run(&d, &p).unwrap().source_reachability()["beta/second"].is_empty());
    let (mut d, p) = fixture();
    let mut duplicate = d[0].edges[0].clone();
    duplicate.id = "alpha/duplicate".into();
    d[0].edges.push(duplicate);
    assert!(run(&d, &p).unwrap_err().message.contains("at most one"));
}
#[test]
fn order_does_not_connect_disconnected_sources() {
    let (mut d, mut p) = fixture();
    let mut independent = d[0].sources[0].clone();
    independent.id = "alpha/independent".into();
    independent.kind = SourceKind::ExternalAction {
        action: "independent".into(),
    };
    p.sources.insert(independent.id.clone(), PUBLIC);
    d[0].sources.push(independent);
    let mut dormant = d[0].nodes[0].clone();
    dormant.id = "alpha/disconnected".into();
    dormant.kind = NodeKind::Source {
        source: "alpha/independent".into(),
    };
    d[0].nodes.push(dormant);
    d[0].edges.push(EdgeDecl {
        id: "alpha/order".into(),
        kind: EdgeKind::Order {
            before: "beta/second".into(),
            after: "alpha/disconnected".into(),
        },
    });
    let graph = run(&d, &p).unwrap();
    assert_eq!(graph.source_reachability()["alpha/disconnected"].len(), 1);
    assert!(!graph.source_reachability()["alpha/disconnected"].contains("alpha/submit"));
    assert!(graph.predecessors()["alpha/disconnected"].contains("beta/second"));
}

#[test]
fn unsupported_sources_and_constant_control_reject() {
    let (mut d, p) = fixture();
    d[0].sources[0].kind = SourceKind::CustomEvent {
        event: "event".into(),
    };
    assert!(run(&d, &p)
        .unwrap_err()
        .message
        .contains("admission authorization"));
    let (mut d, p) = fixture();
    d[0].nodes[1].kind = NodeKind::Constant {
        value: Value::String("literal".into()),
    };
    d[0].nodes[1].inputs[0].name = "start".into();
    d[0].nodes[1].inputs[0].schema = ValueSchema::Struct(BTreeMap::new());
    d[0].nodes[1].inputs[0].security.confidentiality = Classification::Secret;
    d[0].nodes[1].outputs.remove(0);
    d[0].nodes[1].outputs[0].name = "out".into();
    assert!(run(&d, &p).is_err());
}
#[test]
fn attestation_ceilings_and_transformations_fail_closed() {
    let (d, mut p) = fixture();
    p.operations.values_mut().next().unwrap().ok.integrity = IntegrityClass::Trusted;
    assert!(run(&d, &p).unwrap_err().message.contains("attestation"));
    let (mut d, mut p) = fixture();
    let secret = PortSecurity {
        confidentiality: Classification::Secret,
        ..PUBLIC
    };
    d[0].sources[0].security = secret;
    d[0].nodes[0].outputs[0].security = secret;
    p.sources.insert("alpha/submit".into(), secret);
    assert!(run(&d, &p).unwrap_err().message.contains("owner ceiling"));
    p.source_owners.get_mut("alpha").unwrap().owner_ceiling = Classification::Secret;
    assert!(run(&d, &p)
        .unwrap_err()
        .message
        .contains("composition ceiling"));
    let (mut d, mut p) = fixture();
    for declaration in &mut d {
        for node in &mut declaration.nodes {
            if matches!(node.kind, NodeKind::Invoke { .. }) {
                node.inputs[0].security = secret;
            }
        }
    }
    p.operations.values_mut().next().unwrap().input = secret;
    assert!(run(&d, &p)
        .unwrap_err()
        .message
        .contains("declassification"));
}

#[test]
fn third_party_topology_requires_each_endpoint_independently_of_delivery() {
    for order in [false, true] {
        let (mut d, mut p) = fixture();
        let mut edge = d[0].edges.pop().unwrap();
        edge.id = "gamma/edge".into();
        if order {
            edge.kind = EdgeKind::Order {
                before: "alpha/first".into(),
                after: "beta/second".into(),
            };
        }
        d.push(GraphContributionV2 {
            contributor: "gamma".into(),
            sources: vec![],
            nodes: vec![],
            edges: vec![edge],
            patches: vec![],
        });
        // Existing exact delivery grants do not authorize gamma's topology.
        assert!(run(&d, &p).unwrap_err().message.contains("topology"));
        p.topology.insert(("gamma".into(), "alpha/first".into()));
        assert!(run(&d, &p).unwrap_err().message.contains("topology"));
        p.topology.insert(("gamma".into(), "beta/second".into()));
        let graph = run(&d, &p).unwrap();
        p.topology.insert(("gamma".into(), "alpha/source".into()));
        assert_ne!(graph.identity(), run(&d, &p).unwrap().identity());
        if !order {
            p.delivery.clear();
            assert!(run(&d, &p).unwrap_err().message.contains("delivery"));
        }
    }
    let (mut d, mut p) = fixture();
    d[0].edges.pop();
    p.topology.clear();
    run(&d, &p).unwrap(); // Local topology needs no grant.
}

fn constant(id: &str) -> NodeDecl {
    NodeDecl {
        id: id.into(),
        kind: NodeKind::Constant {
            value: Value::String("literal".into()),
        },
        inputs: vec![InputPort {
            name: "start".into(),
            schema: ValueSchema::Struct(BTreeMap::new()),
            readiness: InputReadiness::One,
            security: PUBLIC,
        }],
        outputs: vec![OutputPort {
            name: "out".into(),
            schema: ValueSchema::String,
            security: PUBLIC,
        }],
        execution: ExecutionPolicy {
            on_domain_error: DomainErrorPolicy::RouteErr,
        },
    }
}

#[test]
fn dormant_invokes_constants_order_only_and_flow_chains_are_valid() {
    let (mut d, mut p) = fixture();
    d[0].edges.clear();
    p.topology.clear();
    let mut chained = d[0].nodes[1].clone();
    chained.id = "alpha/chained".into();
    d[0].nodes.extend([
        constant("alpha/constant"),
        constant("alpha/order-only"),
        chained,
    ]);
    d[0].edges = vec![
        EdgeDecl {
            id: "alpha/order".into(),
            kind: EdgeKind::Order {
                before: "alpha/source".into(),
                after: "alpha/order-only".into(),
            },
        },
        EdgeDecl {
            id: "alpha/dormant-chain".into(),
            kind: EdgeKind::Flow {
                from: OutPortRef {
                    node: "alpha/constant".into(),
                    port: "out".into(),
                },
                to: InPortRef {
                    node: "alpha/chained".into(),
                    port: "in".into(),
                },
            },
        },
    ];
    p.delivery.insert(
        (
            "alpha/constant".into(),
            "out".into(),
            "alpha/chained".into(),
            "in".into(),
        ),
        FlowGrant {
            resources: BTreeSet::from(["resource".into()]),
            maximum_classification: Classification::Public,
        },
    );
    let graph = run(&d, &p).unwrap();
    for id in [
        "alpha/first",
        "beta/second",
        "alpha/constant",
        "alpha/order-only",
        "alpha/chained",
    ] {
        assert!(graph.source_reachability()[id].is_empty(), "{id}");
    }
}

#[test]
fn aggregate_counts_and_bytes_reject_before_resolution() {
    let (d, p) = fixture();
    let reject = |declarations: &[GraphContributionV2]| {
        compile(
            declarations,
            &p,
            |_| panic!("bounds must precede attestation"),
            |_, _, _| unreachable!(),
        )
        .unwrap_err()
    };
    let empty = GraphContributionV2 {
        contributor: "alpha".into(),
        sources: vec![],
        nodes: vec![],
        edges: vec![],
        patches: vec![],
    };
    assert!(reject(&vec![empty; MAX_COUNT + 1])
        .message
        .contains("count limit"));
    let mut many = d.clone();
    many[0].nodes = vec![d[0].nodes[0].clone(); MAX_COUNT];
    assert!(reject(&many).message.contains("count limit"));
    let mut many = d.clone();
    many[0].edges = vec![d[0].edges[0].clone(); MAX_COUNT + 1];
    assert!(reject(&many).message.contains("count limit"));
    let mut large = Vec::new();
    for owner in ["alpha", "beta"] {
        let mut node = constant(&format!("{owner}/large"));
        node.kind = NodeKind::Constant {
            value: Value::Bytes(vec![0; MAX_BYTES / 2]),
        };
        node.outputs[0].schema = ValueSchema::Bytes;
        large.push(GraphContributionV2 {
            contributor: owner.into(),
            sources: vec![],
            nodes: vec![node],
            edges: vec![],
            patches: vec![],
        });
    }
    assert!(reject(&large).message.contains("byte limit"));
}

#[test]
fn graph_depth_boundary_is_checked_before_shared_recursive_sort() {
    let (mut d, p) = fixture();
    d.truncate(1);
    d[0].sources.clear();
    d[0].nodes.clear();
    d[0].edges.clear();
    // Reverse dependency direction forces the shared DFS down the whole path.
    for i in 0..MAX_GRAPH_DEPTH {
        d[0].nodes.push(constant(&format!("alpha/n{i:04}")));
        if i > 0 {
            d[0].edges.push(EdgeDecl {
                id: format!("alpha/e{i:04}"),
                kind: EdgeKind::Order {
                    before: format!("alpha/n{i:04}"),
                    after: format!("alpha/n{:04}", i - 1),
                },
            });
        }
    }
    assert_eq!(run(&d, &p).unwrap().order().len(), MAX_GRAPH_DEPTH);
    d[0].nodes.push(constant("alpha/overflow"));
    d[0].edges.push(EdgeDecl {
        id: "alpha/overflow-edge".into(),
        kind: EdgeKind::Order {
            before: "alpha/overflow".into(),
            after: format!("alpha/n{:04}", MAX_GRAPH_DEPTH - 1),
        },
    });
    assert!(run(&d, &p).unwrap_err().message.contains("depth limit"));
}
