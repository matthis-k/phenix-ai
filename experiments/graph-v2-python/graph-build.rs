use phenix_contract::{graph::v2::*, Operation, ValueSchema};

fn main() {
    println!("cargo:rerun-if-changed=../graph-build.rs");
    let python = std::env::var("CARGO_PKG_NAME").unwrap() == "phenix-python-host-fixture";
    let (owner, node, operation, definition) = if python {
        (
            "example.python-host",
            "example.python-host/query",
            example_contracts::Run::KEY,
            example_contracts::Run::DEFINITION,
        )
    } else {
        (
            "example.provider",
            "example.provider/uppercase",
            example_contracts::Transform::KEY,
            example_contracts::Transform::DEFINITION,
        )
    };
    let security = PortSecurity {
        confidentiality: Classification::Public,
        integrity: IntegrityClass::Untrusted,
    };
    let request = ValueSchema::decode(definition.request.encode().bytes()).unwrap();
    let response = ValueSchema::decode(definition.response.encode().bytes()).unwrap();
    let error = ValueSchema::decode(definition.error.encode().bytes()).unwrap();
    let mut graph = GraphContributionV2 {
        contributor: owner.into(),
        sources: vec![],
        edges: vec![],
        patches: vec![],
        nodes: vec![NodeDecl {
            id: node.into(),
            kind: NodeKind::Invoke {
                selector: String::new(),
                operation: operation.into(),
            },
            inputs: vec![InputPort {
                name: "in".into(),
                schema: request.clone(),
                readiness: InputReadiness::One,
                security,
            }],
            outputs: vec![
                OutputPort {
                    name: "ok".into(),
                    schema: response,
                    security,
                },
                OutputPort {
                    name: "err".into(),
                    schema: error,
                    security,
                },
            ],
            execution: ExecutionPolicy {
                on_domain_error: DomainErrorPolicy::RouteErr,
            },
        }],
    };
    if python {
        let source = "example.python-host/source";
        graph.sources.push(SourceDecl {
            id: source.into(),
            owner: owner.into(),
            kind: SourceKind::ExternalAction {
                action: "python.v2-submit".into(),
            },
            payload: request.clone(),
            security,
        });
        graph.nodes.push(NodeDecl {
            id: source.into(),
            kind: NodeKind::Source {
                source: source.into(),
            },
            inputs: vec![],
            outputs: vec![OutputPort {
                name: "out".into(),
                schema: request,
                security,
            }],
            execution: ExecutionPolicy {
                on_domain_error: DomainErrorPolicy::RouteErr,
            },
        });
        for (id, from, port, to) in [
            ("start", source, "out", node),
            ("python-to-rust", node, "ok", "example.provider/uppercase"),
        ] {
            graph.edges.push(EdgeDecl {
                id: format!("{owner}/{id}"),
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
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(out.join("graph.bin"), graph.native_section().unwrap()).unwrap();
}
