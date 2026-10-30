use phenix_basic_contracts::{
    GraphToolCollection, GraphToolOutcome, RoutedToolInvocation, ToolGraph, ToolGraphExecutor,
};
use phenix_contract::{graph::v2::*, Operation, Portable};

fn label() -> PortSecurity {
    PortSecurity {
        confidentiality: Classification::Confidential,
        integrity: IntegrityClass::Untrusted,
    }
}

fn invoke<O: Operation>(id: &str) -> NodeDecl {
    NodeDecl {
        id: format!("phenix.basic.tools/{id}"),
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

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_GRAPH_V2").is_none() {
        return;
    }
    let phenix_contract::ValueSchema::Struct(fields) = GraphToolCollection::SCHEMA.owned().unwrap()
    else {
        unreachable!()
    };
    let graph = GraphContributionV2 {
        contributor: "phenix.basic.tools".into(),
        sources: vec![],
        patches: vec![],
        nodes: vec![
            invoke::<ToolGraph::plan>("plan"),
            invoke::<ToolGraph::collect>("collect"),
            NodeDecl {
                id: "phenix.basic.tools/each".into(),
                kind: NodeKind::InvokeEach {
                    collection: "GraphContributors".into(),
                    operation: <ToolGraphExecutor::invoke as Operation>::KEY.into(),
                    include_self: true,
                    max: 256,
                },
                inputs: vec![InputPort {
                    name: "in".into(),
                    schema: Vec::<RoutedToolInvocation>::SCHEMA.owned().unwrap(),
                    readiness: InputReadiness::One,
                    security: label(),
                }],
                outputs: vec![OutputPort {
                    name: "out".into(),
                    schema: Vec::<GraphToolOutcome>::SCHEMA.owned().unwrap(),
                    security: label(),
                }],
                execution: ExecutionPolicy {
                    on_domain_error: DomainErrorPolicy::RouteErr,
                },
            },
            NodeDecl {
                id: "phenix.basic.tools/join".into(),
                kind: NodeKind::Join {
                    policy: JoinPolicy::All,
                },
                inputs: fields
                    .into_iter()
                    .map(|(name, schema)| InputPort {
                        name,
                        schema,
                        readiness: InputReadiness::One,
                        security: label(),
                    })
                    .collect(),
                outputs: vec![OutputPort {
                    name: "out".into(),
                    schema: GraphToolCollection::SCHEMA.owned().unwrap(),
                    security: label(),
                }],
                execution: ExecutionPolicy {
                    on_domain_error: DomainErrorPolicy::RouteErr,
                },
            },
        ],
        edges: [
            ("plan", "ok", "each", "in"),
            ("each", "out", "join", "outcomes"),
            ("join", "out", "collect", "in"),
        ]
        .into_iter()
        .map(|(from, port, to, input)| EdgeDecl {
            id: format!("phenix.basic.tools/{from}-to-{to}"),
            kind: EdgeKind::Flow {
                from: OutPortRef {
                    node: format!("phenix.basic.tools/{from}"),
                    port: port.into(),
                },
                to: InPortRef {
                    node: format!("phenix.basic.tools/{to}"),
                    port: input.into(),
                },
            },
        })
        .collect(),
    }
    .normalized()
    .unwrap();
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("graph-v2.bin"),
        graph.native_section().unwrap(),
    )
    .unwrap();
}
