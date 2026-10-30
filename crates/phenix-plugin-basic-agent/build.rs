use phenix_basic_contracts::{BasicGraphTurn, BasicTurnPrepare, GraphTurnDecision};
use phenix_contract::{graph::v2::*, Operation, Portable};

fn invoke<O: Operation>(id: &str) -> NodeDecl {
    NodeDecl {
        id: format!("phenix.basic.agent/{id}"),
        kind: NodeKind::Invoke {
            selector: String::new(),
            operation: O::KEY.into(),
        },
        inputs: vec![InputPort {
            name: "in".into(),
            schema: O::Request::SCHEMA.owned().unwrap(),
            readiness: InputReadiness::One,
            security: label(),
        }],
        outputs: vec![
            OutputPort {
                name: "err".into(),
                schema: O::Error::SCHEMA.owned().unwrap(),
                security: label(),
            },
            OutputPort {
                name: "ok".into(),
                schema: O::Response::SCHEMA.owned().unwrap(),
                security: label(),
            },
        ],
        execution: ExecutionPolicy {
            on_domain_error: DomainErrorPolicy::RouteErr,
        },
    }
}

fn label() -> PortSecurity {
    PortSecurity {
        confidentiality: Classification::Confidential,
        integrity: IntegrityClass::Untrusted,
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_GRAPH_V2").is_none() {
        return;
    }
    let request = <BasicTurnPrepare::prepare as Operation>::Request::SCHEMA
        .owned()
        .unwrap();
    let graph = GraphContributionV2 {
        contributor: "phenix.basic.agent".into(),
        sources: vec![SourceDecl {
            id: "phenix.basic.agent/prompt".into(),
            owner: "phenix.basic.agent".into(),
            kind: SourceKind::ExternalAction {
                action: "phenix.basic.submit".into(),
            },
            payload: request.clone(),
            security: label(),
        }],
        nodes: vec![
            NodeDecl {
                id: "phenix.basic.agent/prompt".into(),
                kind: NodeKind::Source {
                    source: "phenix.basic.agent/prompt".into(),
                },
                inputs: vec![],
                outputs: vec![OutputPort {
                    name: "out".into(),
                    schema: request,
                    security: label(),
                }],
                execution: ExecutionPolicy {
                    on_domain_error: DomainErrorPolicy::RouteErr,
                },
            },
            invoke::<BasicTurnPrepare::prepare>("prepare"),
            invoke::<BasicGraphTurn::model>("model"),
            invoke::<BasicGraphTurn::tool>("tool"),
            invoke::<BasicGraphTurn::decide>("decide"),
            NodeDecl {
                id: "phenix.basic.agent/route".into(),
                kind: NodeKind::Switch {
                    tagged_schema: GraphTurnDecision::SCHEMA.owned().unwrap().id().unwrap(),
                },
                inputs: vec![InputPort {
                    name: "in".into(),
                    schema: GraphTurnDecision::SCHEMA.owned().unwrap(),
                    readiness: InputReadiness::One,
                    security: label(),
                }],
                outputs: ["Final", "Tools"]
                    .into_iter()
                    .map(|name| OutputPort {
                        name: name.into(),
                        schema: <BasicGraphTurn::tool as Operation>::Request::SCHEMA
                            .owned()
                            .unwrap(),
                        security: label(),
                    })
                    .collect(),
                execution: ExecutionPolicy {
                    on_domain_error: DomainErrorPolicy::RouteErr,
                },
            },
            invoke::<BasicGraphTurn::finish>("finish"),
        ],
        edges: [
            ("prompt", "out", "prepare"),
            ("prepare", "ok", "model"),
            ("model", "ok", "decide"),
            ("decide", "ok", "route"),
            ("route", "Tools", "tool"),
            ("route", "Final", "finish"),
            ("tool", "ok", "model"),
        ]
        .into_iter()
        .map(|(from, port, to)| EdgeDecl {
            id: format!("phenix.basic.agent/{from}-to-{to}"),
            kind: if from == "tool" {
                EdgeKind::Deferred {
                    from: OutPortRef {
                        node: format!("phenix.basic.agent/{from}"),
                        port: port.into(),
                    },
                    to: InPortRef {
                        node: format!("phenix.basic.agent/{to}"),
                        port: "in".into(),
                    },
                    next: DeferredWave::Next,
                }
            } else {
                EdgeKind::Flow {
                    from: OutPortRef {
                        node: format!("phenix.basic.agent/{from}"),
                        port: port.into(),
                    },
                    to: InPortRef {
                        node: format!("phenix.basic.agent/{to}"),
                        port: "in".into(),
                    },
                }
            },
        })
        .collect(),
        patches: vec![],
    }
    .normalized()
    .unwrap();
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("graph-v2.bin"),
        graph.native_section().unwrap(),
    )
    .unwrap();
}
