use phenix_contract::{graph::v2::*, Operation, ValueSchema};
const OWNER: &str = "experiment.fixture";
const SEC: PortSecurity = PortSecurity {
    confidentiality: Classification::Public,
    integrity: IntegrityClass::Untrusted,
};
fn graph() -> GraphContributionV2 {
    let schema =
        |s: phenix_contract::StaticSchema| ValueSchema::decode(s.encode().bytes()).unwrap();
    let text = schema(dataflow_native_contracts::Service::run::DEFINITION.request);
    let branch = schema(dataflow_native_contracts::Prepare::run::DEFINITION.response);
    let error = schema(dataflow_native_contracts::Service::run::DEFINITION.error);
    let exec = ExecutionPolicy {
        on_domain_error: DomainErrorPolicy::RouteErr,
    };
    let input = |s: ValueSchema| {
        vec![InputPort {
            name: "in".into(),
            schema: s,
            security: SEC,
            readiness: InputReadiness::One,
        }]
    };
    let output = |name: &str, s: ValueSchema| OutputPort {
        name: name.into(),
        schema: s,
        security: SEC,
    };
    let invoke = |name: &str, key: &str, request: ValueSchema, response: ValueSchema| NodeDecl {
        id: format!("{OWNER}/{name}"),
        kind: NodeKind::Invoke {
            selector: String::new(),
            operation: key.into(),
        },
        inputs: input(request),
        outputs: vec![output("ok", response), output("err", error.clone())],
        execution: exec,
    };
    let mut nodes = vec![
        NodeDecl {
            id: format!("{OWNER}/Trigger"),
            kind: NodeKind::Source {
                source: format!("{OWNER}/submit"),
            },
            inputs: vec![],
            outputs: vec![output("out", text.clone())],
            execution: exec,
        },
        invoke(
            "Prepare",
            dataflow_native_contracts::Prepare::run::KEY,
            text.clone(),
            branch.clone(),
        ),
        NodeDecl {
            id: format!("{OWNER}/Switch"),
            kind: NodeKind::Switch {
                tagged_schema: branch.id().unwrap(),
            },
            inputs: input(branch),
            outputs: vec![output("Model", text.clone()), output("Tool", text.clone())],
            execution: exec,
        },
        invoke(
            "Model",
            dataflow_native_contracts::Service::run::KEY,
            text.clone(),
            text.clone(),
        ),
        invoke(
            "Tool",
            dataflow_native_contracts::Service::run::KEY,
            text.clone(),
            text.clone(),
        ),
        NodeDecl {
            id: format!("{OWNER}/Aggregate"),
            kind: NodeKind::Merge {
                policy: MergePolicy::ExactlyOne,
            },
            inputs: input(text.clone()),
            outputs: vec![output("out", text.clone())],
            execution: exec,
        },
        invoke(
            "Complete",
            dataflow_native_contracts::Complete::run::KEY,
            text.clone(),
            text.clone(),
        ),
    ];
    let edges = [
        ("Trigger", "out", "Prepare"),
        ("Prepare", "ok", "Switch"),
        ("Switch", "Model", "Model"),
        ("Switch", "Tool", "Tool"),
        ("Model", "ok", "Aggregate"),
        ("Tool", "ok", "Aggregate"),
        ("Aggregate", "out", "Complete"),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (f, p, t))| EdgeDecl {
        id: format!("{OWNER}/e{i}"),
        kind: EdgeKind::Flow {
            from: OutPortRef {
                node: format!("{OWNER}/{f}"),
                port: p.into(),
            },
            to: InPortRef {
                node: format!("{OWNER}/{t}"),
                port: "in".into(),
            },
        },
    })
    .collect();
    GraphContributionV2 {
        contributor: OWNER.into(),
        sources: vec![SourceDecl {
            id: format!("{OWNER}/submit"),
            owner: OWNER.into(),
            kind: SourceKind::ExternalAction {
                action: "experiment.submit".into(),
            },
            payload: text,
            security: SEC,
        }],
        nodes: std::mem::take(&mut nodes),
        edges,
        patches: vec![],
    }
}
