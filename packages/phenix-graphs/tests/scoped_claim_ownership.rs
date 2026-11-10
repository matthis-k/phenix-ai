use phenix_graph::{DeclaredValue, GraphBuilder, GraphContract, GraphOrigin, InterfaceShape};

fn contract() -> GraphContract {
    GraphContract::new(
        "example/owned-service@1",
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::local::<i64>())
            .unwrap()
            .output("value", DeclaredValue::local::<i64>())
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn scoped_contributor_cannot_claim_another_contributors_direct_node() {
    let provider = GraphOrigin::default();
    let foreign = GraphOrigin::default();
    let mut graph = GraphBuilder::new("host");

    graph.with_origin(&provider, |g| {
        g.node("service", |v: i64| v + 1).unwrap();
    });

    let error = graph
        .with_origin(&foreign, |g| g.implements("service", &contract()))
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("another contribution generation")
    );

    graph
        .with_origin(&provider, |g| g.implements("service", &contract()))
        .unwrap();
    graph.compile().unwrap();
}

#[test]
fn scoped_claims_on_imported_definitions_must_be_declared_on_the_package() {
    let provider = GraphOrigin::default();
    let mut child = GraphBuilder::new("child");
    let node = child.node("private", |v: i64| v + 1).unwrap();
    child
        .expose_input_field(
            "run",
            phenix_graph::SlotField::<(), i64>::new("value"),
            node.inlet(),
        )
        .unwrap();
    child.expose_output("value", node.output()).unwrap();
    child.implements_public(&contract()).unwrap();

    let mut graph = GraphBuilder::new("host");
    graph.with_origin(&provider, |g| {
        g.instance("service", &child.module()).unwrap();
    });

    for path in ["service", "service/private", ""] {
        let result = graph.with_origin(&provider, |g| g.implements(path, &contract()));
        assert!(result.is_err(), "scoped mutation of shared path {path:?}");
    }
    contract()
        .check_implementation(&graph.inspect("service").unwrap())
        .unwrap();
    graph.compile().unwrap();
}

#[test]
fn manual_graph_authoring_retains_definition_level_claim_edits() {
    let mut child = GraphBuilder::new("child");
    let node = child.node("private", |v: i64| v + 1).unwrap();
    child
        .expose_input_field(
            "run",
            phenix_graph::SlotField::<(), i64>::new("value"),
            node.inlet(),
        )
        .unwrap();
    child.expose_output("value", node.output()).unwrap();
    let mut graph = GraphBuilder::new("host");
    graph.instance("one", &child.module()).unwrap();
    graph.implements("one", &contract()).unwrap();
    contract()
        .check_implementation(&graph.inspect("one").unwrap())
        .unwrap();
    graph.compile().unwrap();
}
