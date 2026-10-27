//! Native complete-request readiness, causal absence and finite collection acceptance.
use super::*;

const OPTIONAL: &str = "example.graph.optional@1/run";
const MANY: &str = "example.graph.many@1/run";
const NESTED: &str = "example.graph.nested-optional@1/run";
const SLOW: &str = "example.graph.slow@1/run";
const SWITCH: &str = "example.graph-b/readiness-switch";
const FAST: &str = "example.graph-b/readiness-fast";
const PREFIX: &str = "example.graph-b/readiness-prefix";
const LOCAL: &str = "example.graph-b/readiness-source";
const INACTIVE: &str = "example.graph-b/readiness-inactive";
const OTHER: &str = "example.graph.readiness-other";
const UNUSED_ACTION: &str = "example.graph.readiness-unused";

fn invoke(
    metadata: &phenix_contract::ContractMetadata,
    id: &str,
    key: &str,
    readiness: InputReadiness,
) -> NodeDecl {
    let (contract, operation) = key.rsplit_once('/').unwrap();
    let schema = metadata
        .definitions
        .iter()
        .find(|d| d.contract == contract)
        .unwrap()
        .operations
        .iter()
        .find(|s| s.name == operation)
        .unwrap();
    NodeDecl {
        id: id.into(),
        kind: NodeKind::Invoke {
            selector: String::new(),
            operation: key.into(),
        },
        inputs: vec![InputPort {
            name: "in".into(),
            schema: schema.request.clone(),
            readiness,
            security: LABEL,
        }],
        outputs: vec![
            port("err", schema.error.clone()),
            port("ok", schema.response.clone()),
        ],
        execution: ExecutionPolicy {
            on_domain_error: DomainErrorPolicy::SkipDependents,
        },
    }
}

fn port(name: &str, schema: ValueSchema) -> OutputPort {
    OutputPort {
        name: name.into(),
        schema,
        security: LABEL,
    }
}

fn source(graph: &mut GraphContributionV2, id: &str, action: &str, payload: ValueSchema) {
    graph.sources.push(SourceDecl {
        id: id.into(),
        owner: graph.contributor.clone(),
        kind: SourceKind::ExternalAction {
            action: action.into(),
        },
        payload: payload.clone(),
        security: LABEL,
    });
    graph.nodes.push(NodeDecl {
        id: id.into(),
        kind: NodeKind::Source { source: id.into() },
        inputs: vec![],
        outputs: vec![port("out", payload)],
        execution: ExecutionPolicy {
            on_domain_error: DomainErrorPolicy::RouteErr,
        },
    });
}

fn flow(graph: &mut GraphContributionV2, id: &str, from: &str, out: &str, to: &str) {
    graph.edges.push(EdgeDecl {
        id: format!("{}/{id}", graph.contributor),
        kind: EdgeKind::Flow {
            from: OutPortRef {
                node: from.into(),
                port: out.into(),
            },
            to: InPortRef {
                node: to.into(),
                port: "in".into(),
            },
        },
    });
}

pub(super) fn run(packages: &[String], output: &Path) {
    let witness = output.join("v2-readiness-consumer");
    let observer = output.join("v2-readiness-observer");
    std::env::set_var("PHENIX_GRAPH_B_READINESS_WITNESS", &witness);
    assert!(!witness.exists());
    assert!(!observer.exists());
    let verified: Vec<_> = packages
        .iter()
        .map(|p| phenix_artifact::VerifiedPackage::verify_trusted_read_only(Path::new(p)).unwrap())
        .collect();
    let mut cases = Vec::new();
    for profile in [
        "switch-optional",
        "switch-empty",
        "local-optional",
        "local-many",
        "nested",
        "pending-optional",
        "many",
        "underflow",
        "overflow",
        "failed-min0",
        "failed-min3",
        "denied-none",
        "denied-empty",
    ] {
        let pending = matches!(
            profile,
            "pending-optional" | "many" | "underflow" | "overflow" | "failed-min0" | "failed-min3"
        );
        let many = matches!(
            profile,
            "switch-empty"
                | "local-many"
                | "many"
                | "underflow"
                | "overflow"
                | "failed-min0"
                | "failed-min3"
                | "denied-empty"
        );
        let denied = profile.starts_with("denied-");
        let switch = matches!(
            profile,
            "switch-optional"
                | "switch-empty"
                | "denied-none"
                | "denied-empty"
                | "local-optional"
                | "local-many"
        );
        let foreign = profile.starts_with("switch-") || denied;
        let nested = profile == "nested";
        let local = if foreign { SOURCE } else { LOCAL };
        let switch_id = if foreign {
            "example.graph-a/readiness-switch"
        } else {
            SWITCH
        };
        let min = if matches!(profile, "underflow" | "failed-min3") {
            3
        } else {
            0
        };
        let max = if profile == "overflow" { 1 } else { 3 };
        let readiness = if many {
            InputReadiness::StaticMany { min, max }
        } else {
            InputReadiness::Optional
        };
        let key = if nested {
            NESTED
        } else if many {
            MANY
        } else {
            OPTIONAL
        };
        let mut d = vec![
            GraphContributionV2 {
                contributor: A.into(),
                sources: vec![],
                nodes: vec![],
                edges: vec![],
                patches: vec![],
            },
            GraphContributionV2 {
                contributor: B.into(),
                sources: vec![],
                nodes: vec![invoke(verified[1].metadata(), SECOND, key, readiness)],
                edges: vec![],
                patches: vec![],
            },
        ];
        assert_eq!(
            d[1].nodes[0].inputs[0].schema,
            if nested {
                ValueSchema::Option(Box::new(ValueSchema::Option(Box::new(ValueSchema::String))))
            } else if many {
                ValueSchema::List(Box::new(ValueSchema::String))
            } else {
                ValueSchema::Option(Box::new(ValueSchema::String))
            }
        );
        // Neither zero candidates nor Order establishes causal source relevance.
        for (id, op, r) in [
            (
                "example.graph-b/dormant-optional",
                OPTIONAL,
                InputReadiness::Optional,
            ),
            (
                "example.graph-b/dormant-many",
                MANY,
                InputReadiness::StaticMany { min: 0, max: 3 },
            ),
            (
                "example.graph-b/order-only",
                OPTIONAL,
                InputReadiness::Optional,
            ),
        ] {
            d[1].nodes.push(invoke(verified[1].metadata(), id, op, r));
        }
        let request = ValueSchema::Struct(BTreeMap::from([("text".into(), ValueSchema::String)]));
        if pending {
            source(&mut d[0], SOURCE, ACTION, request.clone());
            d[0].nodes.push(invoke(
                verified[0].metadata(),
                FIRST,
                SLOW,
                InputReadiness::One,
            ));
            flow(&mut d[0], "start-slow", SOURCE, "out", FIRST);
            flow(&mut d[0], "z-slow", FIRST, "ok", SECOND);
            if many {
                source(&mut d[1], INACTIVE, OTHER, ValueSchema::String);
                d[1].nodes.push(invoke(
                    verified[1].metadata(),
                    FAST,
                    KEY,
                    InputReadiness::One,
                ));
                d[1].nodes.push(invoke(
                    verified[1].metadata(),
                    PREFIX,
                    CONSUME,
                    InputReadiness::One,
                ));
                flow(&mut d[0], "start-fast", SOURCE, "out", FAST);
                flow(&mut d[0], "prefix", FAST, "ok", PREFIX);
                // All candidate IDs have the same contributor; completion order is irrelevant.
                flow(&mut d[0], "a-fast", PREFIX, "ok", SECOND);
                flow(&mut d[0], "m-inactive", INACTIVE, "out", SECOND);
            }
        } else {
            let payload = if nested {
                ValueSchema::Option(Box::new(ValueSchema::String))
            } else if switch {
                ValueSchema::Tagged(BTreeMap::from([
                    ("empty".into(), None),
                    ("text".into(), Some(ValueSchema::String)),
                ]))
            } else {
                ValueSchema::String
            };
            let owner = if foreign { 0 } else { 1 };
            source(&mut d[owner], local, ACTION, payload.clone());
            if switch {
                d[owner].nodes.push(NodeDecl {
                    id: switch_id.into(),
                    kind: NodeKind::Switch {
                        tagged_schema: payload.id().unwrap(),
                    },
                    inputs: vec![InputPort {
                        name: "in".into(),
                        schema: payload,
                        readiness: InputReadiness::One,
                        security: LABEL,
                    }],
                    outputs: vec![
                        port("empty", ValueSchema::Struct(BTreeMap::new())),
                        port("text", ValueSchema::String),
                    ],
                    execution: ExecutionPolicy {
                        on_domain_error: DomainErrorPolicy::RouteErr,
                    },
                });
                flow(&mut d[owner], "switch", local, "out", switch_id);
                flow(&mut d[owner], "candidate", switch_id, "text", SECOND);
            } else {
                flow(&mut d[1], "candidate", local, "out", SECOND);
            }
        }
        let active = if pending { SOURCE } else { local };
        source(
            &mut d[1],
            "example.graph-b/readiness-unused",
            UNUSED_ACTION,
            ValueSchema::Struct(BTreeMap::new()),
        );
        d[1].edges.push(EdgeDecl {
            id: format!("{B}/order"),
            kind: EdgeKind::Order {
                before: active.into(),
                after: "example.graph-b/order-only".into(),
            },
        });
        let resources = if pending {
            BTreeSet::from([
                "example.graph-a.property.result".into(),
                "example.graph-b.property.result".into(),
                "example.graph.delivery".into(),
            ])
        } else if foreign {
            BTreeSet::from(["example.graph.delivery".into()])
        } else {
            BTreeSet::new()
        };
        let mut grants = Vec::new();
        for (owner, op) in [
            ("root", KEY),
            (A, SLOW),
            (B, KEY),
            (B, CONSUME),
            (B, OPTIONAL),
            (B, MANY),
            (B, NESTED),
        ] {
            let (interface, operation) = op.rsplit_once('/').unwrap();
            grants.push(ScopedGrant {
                caller: owner.into(),
                interface: interface.into(),
                operation: operation.into(),
                resources: resources.clone(),
            });
        }
        if pending {
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
        }
        let composition = Composition {
            format: "phenix.composition.v3".into(),
            artifacts: d
                .iter()
                .enumerate()
                .map(|(i, g)| {
                    repack(
                        &packages[i],
                        &output.join(format!("v2-readiness-{profile}-{i}")),
                        g,
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
            grants,
            excluded: BTreeSet::new(),
            substitutions: vec![],
            resource: None,
        };
        let mut policy = TrustedCompositionPolicy::default();
        for graph in &d {
            policy.source_owners.insert(
                graph.contributor.clone(),
                SourceOwnerPolicy {
                    owner_ceiling: Classification::Confidential,
                    composition_ceiling: Classification::Confidential,
                },
            );
            for s in &graph.sources {
                policy.sources.insert(s.id.clone(), LABEL);
            }
            for node in &graph.nodes {
                if let NodeKind::Invoke { operation, .. } = &node.kind {
                    policy.operations.insert(
                        (graph.contributor.clone(), operation.clone()),
                        OperationSecurity {
                            input: LABEL,
                            ok: LABEL,
                            err: LABEL,
                        },
                    );
                }
            }
            for edge in &graph.edges {
                let endpoints = match &edge.kind {
                    EdgeKind::Flow { from, to } => vec![&from.node, &to.node],
                    EdgeKind::Order { before, after } => vec![before, after],
                    _ => unreachable!(),
                };
                for endpoint in endpoints {
                    if !endpoint.starts_with(&format!("{}/", graph.contributor)) {
                        policy
                            .topology
                            .insert((graph.contributor.clone(), endpoint.clone()));
                    }
                }
                if let EdgeKind::Flow { from, to } = &edge.kind {
                    if pending || (foreign && to.node == SECOND) {
                        policy.delivery.insert(
                            (
                                from.node.clone(),
                                from.port.clone(),
                                to.node.clone(),
                                to.port.clone(),
                            ),
                            FlowGrant {
                                resources: if to.node == SECOND {
                                    BTreeSet::from(["example.graph.delivery".into()])
                                } else {
                                    resources.clone()
                                },
                                maximum_classification: Classification::Confidential,
                            },
                        );
                    }
                }
            }
        }
        let kernel = Prepared::prepare_graph_v2(composition.clone(), &policy)
            .unwrap()
            .load()
            .unwrap();
        let calls = kernel.inspect()["calls"].clone();
        assert_eq!(
            kernel
                .begin_action_scoped(
                    ACTION,
                    &Value::Bool(false),
                    ScopedRequest {
                        authority: composition.authority.clone(),
                        resource: None,
                    }
                )
                .err()
                .unwrap()
                .code,
            Code::SchemaRejected
        );
        assert_eq!(
            kernel.inspect()["calls"],
            calls,
            "malformed complete source request invoked native code"
        );
        let mut previous_root = None;
        let variants = if switch && !denied { 2 } else { 1 };
        for variant in 0..variants {
            let before = fs::read_to_string(&witness).unwrap_or_default();
            let fds = fs::read_dir("/proc/self/fd").unwrap().count();
            let payload = if pending {
                Value::map([(
                    "text",
                    Value::String(
                        if profile.starts_with("failed-") {
                            "domain"
                        } else {
                            "x"
                        }
                        .into(),
                    ),
                )])
            } else if nested {
                Value::Null
            } else if switch {
                if variant == 0 && !denied {
                    Value::map([("text", Value::String("x".into()))])
                } else {
                    Value::map([("empty", Value::Null)])
                }
            } else {
                Value::String("x".into())
            };
            let mut authority = composition.authority.clone();
            if denied {
                authority.resources.clear();
            }
            let mut run = kernel
                .begin_action_scoped(
                    ACTION,
                    &payload,
                    ScopedRequest {
                        authority,
                        resource: None,
                    },
                )
                .unwrap();
            let mut root = None;
            let mut terminals = 0;
            let mut observe = |u: &phenix_kernel::graph::GraphUpdate<'_>| {
                use std::io::Write;
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&observer)
                    .unwrap()
                    .write_all(b"observed\n")
                    .unwrap();
                assert_eq!(*root.get_or_insert(u.root), u.root);
                assert_ne!(previous_root, Some(u.root));
                assert!(!matches!(u.event, GraphEvent::NodeFinished(_)));
                if matches!(u.event, GraphEvent::Finished(_)) {
                    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
                    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds);
                    terminals += 1;
                }
            };
            let first = run.pump(&mut observe);
            if pending {
                assert_eq!(first.unwrap(), GraphStatus::Pending, "{profile}");
                assert_eq!(fs::read_to_string(&witness).unwrap_or_default(), before);
                assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
                assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds + 1);
                for _ in 0..2 {
                    assert_eq!(run.pump(&mut observe).unwrap(), GraphStatus::Pending);
                    assert!(run.completion().is_none());
                    assert_eq!(fs::read_to_string(&witness).unwrap_or_default(), before);
                    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
                    assert_eq!(run.steps().len(), if many { 2 } else { 0 });
                }
                for _ in 0..8 {
                    if run.pump(&mut observe).unwrap() == GraphStatus::Finished {
                        break;
                    }
                }
            } else if denied {
                assert_eq!(first.unwrap_err().code, Code::Denied);
            } else {
                assert_eq!(first.unwrap(), GraphStatus::Finished);
            }
            assert_eq!(terminals, 1);
            previous_root = root;
            assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
            assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds);
            let result = run.steps().iter().find(|s| s.node == SECOND);
            let suppressed = denied
                || matches!(
                    profile,
                    "underflow" | "overflow" | "failed-min0" | "failed-min3"
                );
            if suppressed {
                assert!(result.is_none(), "{profile}");
                assert_eq!(fs::read_to_string(&witness).unwrap_or_default(), before);
            } else {
                let expected = if nested {
                    "nested:Some(None)"
                } else if pending && many {
                    "many:[\"consumed:x\", \"x\"]"
                } else if many && switch && variant == 1 {
                    "many:[]"
                } else if many {
                    "many:[\"x\"]"
                } else if switch && variant == 1 {
                    "optional:None"
                } else {
                    "optional:Some(\"x\")"
                };
                assert_eq!(
                    result.unwrap().outcome,
                    Outcome::Success(Value::String(expected.into())),
                    "{profile}"
                );
                assert_eq!(
                    fs::read_to_string(&witness).unwrap(),
                    format!("{before}called\n")
                );
            }
            assert!(run
                .steps()
                .iter()
                .all(|s| !s.node.contains("dormant-") && !s.node.ends_with("order-only")));
            if denied {
                assert_eq!(
                    run.pump(|_| panic!("duplicate observer event"))
                        .unwrap_err()
                        .code,
                    Code::Denied
                );
            } else {
                assert_eq!(
                    run.pump(|_| panic!("duplicate observer event")).unwrap(),
                    GraphStatus::Finished
                );
                assert_eq!(
                    run.completion(),
                    Some(if profile.starts_with("failed-") {
                        GraphCompletion::Domain
                    } else {
                        GraphCompletion::Success
                    }),
                    "{profile}"
                );
                if pending {
                    assert!(
                        run.steps().iter().any(|s| s.node == FIRST
                            && s.outcome
                                == if profile.starts_with("failed-") {
                                    Outcome::Domain(Value::String("domain fixture".into()))
                                } else {
                                    Outcome::Success(Value::String("x".into()))
                                }),
                        "pending work was implicitly cancelled: {profile}"
                    );
                }
            }
            cases.push(format!("{profile}-{variant}"));
        }
        let before = fs::read_to_string(&witness).unwrap_or_default();
        let calls = kernel.inspect()["calls"].clone();
        let mut unrelated = kernel
            .begin_action_scoped(
                UNUSED_ACTION,
                &Value::map([]),
                ScopedRequest {
                    authority: composition.authority,
                    resource: None,
                },
            )
            .unwrap();
        assert_eq!(
            unrelated
                .pump(|u| {
                    assert_ne!(previous_root, Some(u.root));
                    assert!(!matches!(u.event, GraphEvent::NodeFinished(_)));
                })
                .unwrap(),
            GraphStatus::Finished
        );
        assert!(
            unrelated.steps().is_empty(),
            "inactive sources synthesized readiness: {profile}"
        );
        assert_eq!(kernel.inspect()["calls"], calls);
        assert_eq!(fs::read_to_string(&witness).unwrap_or_default(), before);
        assert_eq!(
            unrelated
                .pump(|_| panic!("duplicate unrelated event"))
                .unwrap(),
            GraphStatus::Finished
        );
        drop(unrelated);
        cases.push(format!("{profile}-unrelated-root-dormant"));
        kernel.shutdown().unwrap();
    }
    std::env::remove_var("PHENIX_GRAPH_B_READINESS_WITNESS");
    let observed = fs::read_to_string(&observer).unwrap();
    assert!(!observed.is_empty());
    assert!(observed.lines().all(|line| line == "observed"));
    fs::write(
        output.join("graph-v2-readiness.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "format": "phenix.graph-v2-readiness.v1", "cases": cases, "full_V2": false,
            "p1_parity": false, "ci": "pending", "acceptance": "focused native readiness only",
            "remaining": ["single-store CI final gate", "full V2", "P1 parity"]
        }))
        .unwrap(),
    )
    .unwrap();
}
