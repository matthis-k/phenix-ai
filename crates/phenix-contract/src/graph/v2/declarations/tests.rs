use super::*;
use crate::{MAX_BYTES, MAX_COUNT, MAX_DEPTH};

const PUBLIC: PortSecurity = PortSecurity {
    confidentiality: Classification::Public,
    integrity: IntegrityClass::Untrusted,
};

fn input(name: &str, schema: ValueSchema) -> InputPort {
    InputPort {
        name: name.into(),
        schema,
        readiness: InputReadiness::One,
        security: PUBLIC,
    }
}

#[test]
fn switch_merge_roundtrip_permutations_and_security() {
    let schema = ValueSchema::Tagged(std::collections::BTreeMap::from([
        ("empty".into(), None),
        ("payload".into(), Some(ValueSchema::String)),
    ]));
    let switch = NodeDecl {
        id: "demo/switch".into(),
        kind: NodeKind::Switch {
            tagged_schema: schema.id().unwrap(),
        },
        inputs: vec![input("in", schema)],
        outputs: vec![
            output("empty", ValueSchema::Struct(Default::default())),
            output("payload", ValueSchema::String),
        ],
        execution: ExecutionPolicy {
            on_domain_error: DomainErrorPolicy::RouteErr,
        },
    };
    let merge = NodeDecl {
        id: "demo/merge".into(),
        kind: NodeKind::Merge {
            policy: MergePolicy::ExactlyOne,
        },
        inputs: vec![input("in", ValueSchema::String)],
        outputs: vec![output("out", ValueSchema::String)],
        execution: switch.execution,
    };
    let mut graph = GraphContributionV2 {
        contributor: "demo".into(),
        sources: vec![],
        nodes: vec![switch.clone(), merge.clone()],
        edges: vec![],
        patches: vec![],
    };
    let bytes = graph.encode().unwrap();
    assert_eq!(
        GraphContributionV2::decode(&bytes).unwrap(),
        graph.normalized().unwrap()
    );
    graph.nodes.reverse();
    graph.nodes[1].outputs.reverse();
    assert_eq!(bytes, graph.encode().unwrap());
    for malformed in 0..9 {
        let mut node = switch.clone();
        match malformed {
            0 => node.inputs.clear(),
            1 => node.inputs[0].name = "wrong".into(),
            2 => node.inputs[0].schema = ValueSchema::String,
            3 => {
                node.kind = NodeKind::Switch {
                    tagged_schema: SchemaId::from_bytes(&[1; 32]).unwrap(),
                }
            }
            4 => {
                node.outputs.pop();
            }
            5 => node.outputs[0].schema = ValueSchema::String,
            6 => node.outputs[0].name = "unknown".into(),
            7 => node.inputs[0].security.confidentiality = Classification::Confidential,
            _ => node.outputs[0].security.integrity = IntegrityClass::Trusted,
        }
        graph.nodes = vec![node];
        assert!(graph.encode().is_err(), "malformed {malformed}");
    }
    for malformed in 0..5 {
        let mut node = merge.clone();
        match malformed {
            0 => node.inputs[0].name = "other".into(),
            1 => node.outputs[0].name = "other".into(),
            2 => node.outputs[0].schema = ValueSchema::Bool,
            3 => node.inputs[0].security.confidentiality = Classification::Confidential,
            _ => node.outputs[0].security.integrity = IntegrityClass::Trusted,
        }
        graph.nodes = vec![node];
        assert!(graph.encode().is_err());
    }
}
fn output(name: &str, schema: ValueSchema) -> OutputPort {
    OutputPort {
        name: name.into(),
        schema,
        security: PUBLIC,
    }
}
fn sample() -> GraphContributionV2 {
    GraphContributionV2 {
        contributor: "demo".into(),
        sources: vec![SourceDecl {
            id: "demo/prompt".into(),
            owner: "demo".into(),
            kind: SourceKind::ExternalAction {
                action: "prompt".into(),
            },
            payload: ValueSchema::String,
            security: PUBLIC,
        }],
        nodes: vec![
            NodeDecl {
                id: "demo/source".into(),
                kind: NodeKind::Source {
                    source: "demo/prompt".into(),
                },
                inputs: vec![],
                outputs: vec![output("out", ValueSchema::String)],
                execution: ExecutionPolicy {
                    on_domain_error: DomainErrorPolicy::FailRoot,
                },
            },
            NodeDecl {
                id: "demo/invoke".into(),
                kind: NodeKind::Invoke {
                    selector: "model#0".into(),
                    operation: "model.api@1/run".into(),
                },
                inputs: vec![input("in", ValueSchema::String)],
                outputs: vec![
                    output("ok", ValueSchema::Bool),
                    output("err", ValueSchema::String),
                ],
                execution: ExecutionPolicy {
                    on_domain_error: DomainErrorPolicy::RouteErr,
                },
            },
        ],
        edges: vec![
            EdgeDecl {
                id: "demo/data".into(),
                kind: EdgeKind::Flow {
                    from: OutPortRef {
                        node: "demo/source".into(),
                        port: "out".into(),
                    },
                    to: InPortRef {
                        node: "demo/invoke".into(),
                        port: "in".into(),
                    },
                },
            },
            EdgeDecl {
                id: "demo/order".into(),
                kind: EdgeKind::Order {
                    before: "demo/source".into(),
                    after: "demo/invoke".into(),
                },
            },
        ],
        patches: vec![],
    }
}
fn constant() -> NodeDecl {
    NodeDecl {
        id: "demo/constant".into(),
        kind: NodeKind::Constant {
            value: Value::Bool(true),
        },
        inputs: vec![input("start", ValueSchema::Struct(Default::default()))],
        outputs: vec![output("out", ValueSchema::Bool)],
        execution: ExecutionPolicy {
            on_domain_error: DomainErrorPolicy::SkipDependents,
        },
    }
}
fn wire(g: &GraphContributionV2) -> Fields {
    fields(Value::decode(&g.encode().unwrap()).unwrap()).unwrap()
}
fn rejects(f: Fields) {
    assert!(GraphContributionV2::decode(&Value::Map(f).encode().unwrap()).is_err());
}

#[test]
fn intrinsic_wire_fields_ids_policies_and_security_reject() {
    let tagged = ValueSchema::Tagged(std::collections::BTreeMap::from([(
        "case".into(),
        Some(ValueSchema::Bool),
    )]));
    for switch in [false, true] {
        let mut node = constant();
        node.inputs = vec![input(
            "in",
            if switch {
                tagged.clone()
            } else {
                ValueSchema::Bool
            },
        )];
        node.kind = if switch {
            NodeKind::Switch {
                tagged_schema: tagged.id().unwrap(),
            }
        } else {
            NodeKind::Merge {
                policy: MergePolicy::ExactlyOne,
            }
        };
        node.outputs[0].name = if switch { "case" } else { "out" }.into();
        let graph = GraphContributionV2 {
            contributor: "demo".into(),
            sources: vec![],
            nodes: vec![node],
            edges: vec![],
            patches: vec![],
        };
        let key = if switch { "tagged_schema" } else { "policy" };
        for missing in ["kind", key] {
            let mut root = wire(&graph);
            let Value::List(nodes) = root.get_mut("nodes").unwrap() else {
                panic!()
            };
            let Value::Map(node) = &mut nodes[0] else {
                panic!()
            };
            let Value::Map(kind) = node.get_mut("kind").unwrap() else {
                panic!()
            };
            kind.remove(missing);
            rejects(root);
        }
        let mut replacements = vec![("extra", Value::Null), ("kind", Value::Null)];
        if switch {
            replacements.extend([
                (key, string("not-bytes")),
                (key, Value::Bytes(vec![1; 31])),
                (key, Value::Bytes(vec![1; 33])),
                (key, Value::Bytes(vec![0; 32])),
                (key, Value::Bytes(vec![1; 32])),
            ]);
        } else {
            replacements.push((key, string("first-value")));
        }
        for (key, value) in replacements {
            let mut root = wire(&graph);
            let Value::List(nodes) = root.get_mut("nodes").unwrap() else {
                panic!()
            };
            let Value::Map(node) = &mut nodes[0] else {
                panic!()
            };
            let Value::Map(kind) = node.get_mut("kind").unwrap() else {
                panic!()
            };
            kind.insert(key.into(), value);
            rejects(root);
        }
        for (ports, key, value) in [
            ("inputs", "confidentiality", "confidential"),
            ("outputs", "integrity", "trusted"),
            ("outputs", "confidentiality", "unknown"),
            ("inputs", "integrity", "unknown"),
        ] {
            let mut root = wire(&graph);
            let Value::List(nodes) = root.get_mut("nodes").unwrap() else {
                panic!()
            };
            let Value::Map(node) = &mut nodes[0] else {
                panic!()
            };
            let Value::List(ports) = node.get_mut(ports).unwrap() else {
                panic!()
            };
            let Value::Map(port) = &mut ports[0] else {
                panic!()
            };
            let Value::Map(security) = port.get_mut("security").unwrap() else {
                panic!()
            };
            security.insert(key.into(), string(value));
            rejects(root);
        }
    }
}

#[test]
fn canonical_roundtrip_and_declaration_order_invariance() {
    let mut graph = sample();
    graph.nodes.push(constant());
    graph.sources.push(SourceDecl {
        id: "demo/other".into(),
        ..graph.sources[0].clone()
    });
    let bytes = graph.encode().unwrap();
    assert_eq!(
        GraphContributionV2::decode(&bytes).unwrap(),
        graph.normalized().unwrap()
    );
    graph.sources.reverse();
    graph.nodes.reverse();
    graph.edges.reverse();
    for node in &mut graph.nodes {
        node.inputs.reverse();
        node.outputs.reverse();
    }
    assert_eq!(graph.encode().unwrap(), bytes);
    let f = wire(&graph);
    assert_eq!(
        f.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "contributor",
            "edges",
            "format",
            "nodes",
            "patches",
            "sources"
        ]
    );
    assert_eq!(f["format"], string("phenix.graph.v2"));
}

#[test]
fn source_kinds_and_policies_roundtrip() {
    for kind in [
        SourceKind::ExternalAction {
            action: "prompt".into(),
        },
        SourceKind::CustomEvent {
            event: "updated".into(),
        },
        SourceKind::KernelEvent {
            event: "ready".into(),
        },
        SourceKind::CommittedWatch {
            property: "store/items".into(),
        },
    ] {
        for policy in [
            DomainErrorPolicy::RouteErr,
            DomainErrorPolicy::SkipDependents,
            DomainErrorPolicy::FailRoot,
        ] {
            let mut graph = sample();
            graph.sources[0].kind = kind.clone();
            graph.nodes[1].execution.on_domain_error = policy;
            assert_eq!(
                GraphContributionV2::decode(&graph.encode().unwrap()).unwrap(),
                graph.normalized().unwrap()
            );
        }
    }
}

#[test]
fn malformed_unknown_and_noncanonical_wire_rejects() {
    for key in [
        "format",
        "contributor",
        "sources",
        "nodes",
        "edges",
        "patches",
    ] {
        let mut f = wire(&sample());
        f.remove(key);
        rejects(f);
    }
    let mut f = wire(&sample());
    f.insert("unknown".into(), Value::Null);
    rejects(f);
    let mut f = wire(&sample());
    f.insert("format".into(), string("phenix.graph.v3"));
    rejects(f);
    let mut f = wire(&sample());
    f.insert("patches".into(), Value::List(vec![Value::Null]));
    rejects(f);
    for collection in ["sources", "nodes", "edges"] {
        let mut f = wire(&sample());
        let Value::List(items) = f.get_mut(collection).unwrap() else {
            panic!()
        };
        let Value::Map(item) = &mut items[0] else {
            panic!()
        };
        item.insert("unknown".into(), Value::Null);
        rejects(f);
    }
    for collection in ["nodes", "edges"] {
        let mut f = wire(&sample());
        let Value::List(items) = f.get_mut(collection).unwrap() else {
            panic!()
        };
        items.reverse();
        rejects(f);
    }
    for collection in ["sources", "nodes", "edges"] {
        let mut f = wire(&sample());
        let Value::List(items) = f.get_mut(collection).unwrap() else {
            panic!()
        };
        let Value::Map(item) = &mut items[0] else {
            panic!()
        };
        let Value::Map(kind) = item.get_mut("kind").unwrap() else {
            panic!()
        };
        kind.insert("kind".into(), string("unknown"));
        rejects(f);
    }
    let bytes = sample().encode().unwrap();
    assert!(GraphContributionV2::decode(&bytes[..bytes.len() - 1]).is_err());
    let mut trailing = bytes;
    trailing.push(0);
    assert!(GraphContributionV2::decode(&trailing).is_err());
    // Duplicate raw map keys must fail before collecting into a BTreeMap.
    let mut duplicate = crate::value::V2_PREFIX.to_vec();
    duplicate.extend([8, 0, 0, 0, 2]);
    for _ in 0..2 {
        duplicate.extend([0, 0, 0, 1, b'x', 0]);
    }
    assert!(GraphContributionV2::decode(&duplicate).is_err());
}

#[test]
fn duplicate_declarations_ports_and_owner_spoofing_reject() {
    let graph = sample();
    let mut g = graph.clone();
    g.sources.push(g.sources[0].clone());
    assert!(g.encode().is_err());
    let mut g = graph.clone();
    g.nodes.push(g.nodes[0].clone());
    assert!(g.encode().is_err());
    let mut g = graph.clone();
    g.edges.push(g.edges[0].clone());
    assert!(g.encode().is_err());
    let mut g = graph.clone();
    g.nodes[1].inputs.push(graph.nodes[1].inputs[0].clone());
    assert!(g.encode().is_err());
    let mut g = graph.clone();
    g.nodes[1].outputs.push(graph.nodes[1].outputs[0].clone());
    assert!(g.encode().is_err());
    for id in ["other/name", "local", "demo/a/b", "root/name", "demo/"] {
        let mut g = graph.clone();
        g.sources[0].id = id.into();
        assert!(g.encode().is_err());
        let mut g = graph.clone();
        g.nodes[0].id = id.into();
        assert!(g.encode().is_err());
        let mut g = graph.clone();
        g.edges[0].id = id.into();
        assert!(g.encode().is_err());
    }
    let mut g = graph.clone();
    g.sources[0].owner = "other".into();
    assert!(g.encode().is_err());
    let mut g = graph;
    g.contributor = "root".into();
    assert!(g.encode().is_err());
}

#[test]
fn invoke_requires_exact_full_request_and_result_ports() {
    for inputs in [
        vec![],
        vec![input("request", ValueSchema::String)],
        vec![
            input("in", ValueSchema::String),
            input("extra", ValueSchema::Bool),
        ],
    ] {
        let mut g = sample();
        g.nodes[1].inputs = inputs;
        assert!(g.encode().is_err());
    }
    for outputs in [
        vec![],
        vec![output("ok", ValueSchema::Bool)],
        vec![
            output("ok", ValueSchema::Bool),
            output("error", ValueSchema::String),
        ],
        vec![
            output("ok", ValueSchema::Bool),
            output("err", ValueSchema::String),
            output("extra", ValueSchema::Bool),
        ],
    ] {
        let mut g = sample();
        g.nodes[1].outputs = outputs;
        assert!(g.encode().is_err());
    }
    for selector in [
        "model#01",
        "model#-1",
        "model#",
        "model#1#2",
        "model#4294967296",
    ] {
        let mut g = sample();
        g.nodes[1].kind = NodeKind::Invoke {
            selector: selector.into(),
            operation: "model.api@1/run".into(),
        };
        assert!(g.encode().is_err());
    }
}

#[test]
fn constants_require_unit_one_start_and_public_untrusted_literal() {
    let mut graph = sample();
    graph.nodes.push(constant());
    assert!(graph.encode().is_ok()); // Dormancy is valid; no implicit activation.
    for bad_input in [
        input("in", ValueSchema::Struct(Default::default())),
        input("start", ValueSchema::Bool),
    ] {
        let mut g = graph.clone();
        g.nodes[2].inputs = vec![bad_input];
        assert!(g.encode().is_err());
    }
    let mut g = graph.clone();
    g.nodes[2].inputs.clear();
    assert!(g.encode().is_err());
    let mut g = graph.clone();
    g.nodes[2].kind = NodeKind::Constant {
        value: Value::String("wrong".into()),
    };
    assert!(g.encode().is_err());
    for security in [
        PortSecurity {
            confidentiality: Classification::Secret,
            ..PUBLIC
        },
        PortSecurity {
            integrity: IntegrityClass::Trusted,
            ..PUBLIC
        },
    ] {
        let mut g = graph.clone();
        g.nodes[2].outputs[0].security = security;
        assert!(g.encode().is_err());
    }
    let mut g = graph;
    g.nodes[2].inputs[0].security.confidentiality = Classification::Secret;
    assert!(g.encode().is_err());
}

#[test]
fn unsupported_profiles_fail_explicitly() {
    {
        let mut g = sample();
        g.nodes[1].kind = NodeKind::Join {
            policy: JoinPolicy::All,
        };
        assert!(g.encode().unwrap_err().message.contains("unsupported"));
    }
    for readiness in [
        InputReadiness::Optional,
        InputReadiness::StaticMany { min: 0, max: 2 },
    ] {
        let mut g = sample();
        g.nodes[1].inputs[0].readiness = readiness;
        assert!(g.encode().unwrap_err().message.contains("unsupported"));
    }
    let mut g = sample();
    g.sources[0].kind = SourceKind::DeferredEntry;
    assert!(g.encode().unwrap_err().message.contains("unsupported"));
    let mut g = sample();
    let EdgeKind::Flow { from, to } = g.edges[0].kind.clone() else {
        panic!()
    };
    g.edges[0].kind = EdgeKind::Deferred {
        from,
        to,
        next: DeferredWave::Next,
    };
    assert!(g.encode().unwrap_err().message.contains("unsupported"));
}

#[test]
fn explicit_ports_source_projection_and_security_fields() {
    let mut g = sample();
    g.nodes[0].outputs[0].security.integrity = IntegrityClass::Trusted;
    assert!(g.encode().is_err());
    let mut g = sample();
    g.nodes[0].outputs[0].schema = ValueSchema::Bool;
    assert!(g.encode().is_err());
    let mut g = sample();
    g.nodes[0].inputs.push(input("in", ValueSchema::String));
    assert!(g.encode().is_err());
    let mut g = sample();
    let EdgeKind::Flow { from, .. } = &mut g.edges[0].kind else {
        panic!()
    };
    from.port = "missing".into();
    assert!(g.encode().is_err());
    let mut f = fields(security(PUBLIC)).unwrap();
    assert_eq!(f["confidentiality"], string("public"));
    f.insert("classification".into(), string("public"));
    assert!(decode_security(Value::Map(f)).is_err());
}

#[test]
fn existing_byte_count_and_depth_limits_apply() {
    let mut g = sample();
    g.sources = vec![g.sources[0].clone(); MAX_COUNT + 1];
    assert!(g.encode().is_err());
    let mut g = sample();
    g.nodes[1].inputs = vec![g.nodes[1].inputs[0].clone(); MAX_COUNT + 1];
    assert!(g.encode().is_err());
    let mut g = sample();
    g.nodes.push(constant());
    g.nodes[2].kind = NodeKind::Constant {
        value: Value::Bytes(vec![0; MAX_BYTES]),
    };
    g.nodes[2].outputs[0].schema = ValueSchema::Bytes;
    assert!(g.encode().is_err());
    let mut g = sample();
    let mut schema = ValueSchema::Bool;
    for _ in 0..MAX_DEPTH + 1 {
        schema = ValueSchema::List(Box::new(schema));
    }
    g.nodes[1].inputs[0].schema = schema;
    assert!(g.encode().is_err());
    let mut deep = Value::Null;
    for _ in 0..MAX_DEPTH + 1 {
        deep = Value::List(vec![deep]);
    }
    let mut g = sample();
    g.nodes.push(constant());
    g.nodes[2].kind = NodeKind::Constant { value: deep };
    assert!(g.encode().is_err());
    assert!(GraphContributionV2::decode(&vec![0; MAX_BYTES + 1]).is_err());
    // Each schema fits individually; the complete declaration must also fit.
    let mut g = sample();
    let large = ValueSchema::Variants(
        (0..800)
            .map(|i| format!("case{i:04}{}", "x".repeat(80)))
            .collect(),
    );
    for i in 0..20 {
        let mut node = g.nodes[1].clone();
        node.id = format!("demo/n{i:02}");
        node.inputs[0].schema = large.clone();
        g.nodes.push(node);
    }
    assert!(g.encode().is_err());
}
