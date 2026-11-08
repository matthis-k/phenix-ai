use phenix_graph::{
    BoundInput, BoundOutput, CompiledGraph, DeclaredValue, GraphBuilder, GraphContract, GraphUnit,
    InterfacePhase, InterfaceShape, NodeTemplate,
};

fn contract() -> GraphContract {
    GraphContract::new(
        "test/compiled@1",
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::local::<i64>())
            .unwrap()
            .output("value", DeclaredValue::local::<i64>())
            .unwrap(),
    )
    .unwrap()
}

fn endpoints<'g>(graph: &'g CompiledGraph, path: &str) -> (BoundInput<'g>, BoundOutput<'g>) {
    let bound = graph
        .find(path)
        .unwrap()
        .bind_contract(&contract())
        .unwrap();
    (
        bound.input("run", "value").unwrap(),
        bound.output("value").unwrap(),
    )
}

#[test]
fn same_paths_and_numeric_addresses_do_not_transfer_between_generations() {
    let mut builder = GraphBuilder::new("root");
    builder.node("worker", |x: i64| x).unwrap();
    builder.implements("worker", &contract()).unwrap();
    let old = builder.clone().compile().unwrap();
    let (input, output) = endpoints(&old, "worker");
    let mut template = NodeTemplate::local_function::<i64, i64, i64, _>(|x| x + 1);
    template.claims.push(contract());
    builder.replace("worker", template).unwrap();
    let replacement = builder.compile().unwrap();
    let (fresh_input, fresh_output) = endpoints(&replacement, "worker");
    assert_eq!(
        input.resolve(&old).unwrap(),
        fresh_input.resolve(&replacement).unwrap()
    );
    assert_eq!(
        output.resolve(&old).unwrap(),
        fresh_output.resolve(&replacement).unwrap()
    );

    for (error, endpoint) in [
        (input.resolve(&replacement).unwrap_err(), "input run.value"),
        (output.resolve(&replacement).unwrap_err(), "output value"),
    ] {
        assert_eq!(error.phase, InterfacePhase::Compiled);
        assert_eq!(error.path.as_ref(), "worker");
        assert_eq!(error.contract.as_deref(), Some(contract().id()));
        assert_eq!(error.endpoint.as_deref(), Some(endpoint));
        assert!(
            error
                .message
                .contains("different compiled graph generation")
        );
    }
    assert!(fresh_input.resolve(&old).is_err());
    assert!(fresh_output.resolve(&old).is_err());
    // A cloned Arc still identifies the same immutable compiled graph.
    let retained = old.clone();
    assert_eq!(input.resolve(&retained).unwrap().len(), 1);
    assert_eq!(
        output.resolve(&retained).unwrap(),
        output.resolve(&old).unwrap()
    );
}

#[test]
fn nested_module_endpoints_keep_the_owning_graph_after_binding_is_dropped() {
    let mut service = GraphBuilder::new("service");
    let worker = service.node("private", |x: i64| x).unwrap();
    service
        .expose_input_slot("run", "value", worker.inlet())
        .unwrap();
    service.expose_output("value", worker.output()).unwrap();
    service.implements_public(&contract()).unwrap();
    let mut wrapper = GraphBuilder::new("wrapper");
    wrapper.instance("inner", &service.module()).unwrap();
    let mut builder = GraphBuilder::new("root");
    builder.instance("outer", &wrapper.module()).unwrap();
    let old = builder.clone().compile().unwrap();
    let other = builder.compile().unwrap();
    let (input, output) = endpoints(&old, "outer/inner");
    let (other_input, other_output) = endpoints(&other, "outer/inner");
    assert_eq!(
        input.resolve(&old).unwrap(),
        other_input.resolve(&other).unwrap()
    );
    assert_eq!(
        output.resolve(&old).unwrap(),
        other_output.resolve(&other).unwrap()
    );
    assert!(input.resolve(&other).is_err());
    assert!(output.resolve(&other).is_err());
    assert_eq!(input.resolve(&old).unwrap().len(), 1);
    assert!(output.resolve(&old).is_ok());
}

#[test]
fn compiled_module_root_supports_uniform_inspection_and_checked_binding() {
    let mut module = GraphBuilder::new("service");
    let worker = module.node("private", |x: i64| x).unwrap();
    module
        .expose_input_slot("run", "value", worker.inlet())
        .unwrap();
    module.expose_output("value", worker.output()).unwrap();
    module.expose_output("debug", worker.output()).unwrap();
    module.implements_public(&contract()).unwrap();
    let compiled = module.clone().module().compile().unwrap();
    let replacement = module.module().compile().unwrap();
    let required = contract();
    required.check_shape(&*compiled).unwrap();
    required.check_implementation(&*compiled).unwrap();
    let snapshot = compiled.inspect().unwrap();
    assert_eq!(snapshot.provenance.phase, InterfacePhase::Compiled);
    assert_eq!(snapshot.provenance.path, "");
    assert_eq!(snapshot.provenance.definition, "service");
    assert!(snapshot.provenance.instance_path.is_empty());
    assert!(compiled.find("").is_none());

    let bound = compiled.bind_contract(&required).unwrap();
    let input = bound.input("run", "value").unwrap();
    let output = bound.output("value").unwrap();
    assert!(bound.input("run", "private").is_err());
    assert!(bound.output("debug").is_err());
    let phenix_graph::FindResult::Node(private) = compiled.find("private").unwrap() else {
        panic!("expected private node");
    };
    assert_eq!(
        input.resolve(&compiled).unwrap(),
        &[private.input("run", "value").unwrap()]
    );
    assert_eq!(
        output.resolve(&compiled).unwrap(),
        private.output("value").unwrap()
    );
    let fresh = replacement.bind_contract(&required).unwrap();
    assert_eq!(
        input.resolve(&compiled).unwrap(),
        fresh
            .input("run", "value")
            .unwrap()
            .resolve(&replacement)
            .unwrap()
    );
    for error in [
        input.resolve(&replacement).unwrap_err(),
        output.resolve(&replacement).unwrap_err(),
    ] {
        assert_eq!(error.phase, InterfacePhase::Compiled);
        assert_eq!(error.path.as_ref(), "");
        assert_eq!(error.contract.as_deref(), Some(required.id()));
        assert!(
            error
                .message
                .contains("different compiled graph generation")
        );
    }
}

#[test]
fn compiled_root_shape_does_not_imply_semantic_membership() {
    let mut module = GraphBuilder::new("unclaimed");
    let worker = module.node("private", |x: i64| x).unwrap();
    module
        .expose_input_slot("run", "value", worker.inlet())
        .unwrap();
    module.expose_output("value", worker.output()).unwrap();
    let compiled = module.module().compile().unwrap();
    contract().check_shape(&*compiled).unwrap();
    assert!(contract().check_implementation(&*compiled).is_err());
    let error = compiled.bind_contract(&contract()).err().unwrap();
    assert_eq!(error.phase, InterfacePhase::Compiled);
    assert_eq!(error.path.as_ref(), "");
    assert_eq!(error.contract.as_deref(), Some(contract().id()));
}
