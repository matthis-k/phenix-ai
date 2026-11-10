use phenix_graph::{
    DeclaredValue, FindResult, GraphBuilder, GraphContract, GraphUnit, InterfacePhase,
    InterfaceShape, ModuleCatalog, ModuleDefBuilder, ModulePackage, NodeTemplate,
};

fn contract() -> GraphContract {
    GraphContract::new(
        "test/transform@1",
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::local::<i64>())
            .unwrap()
            .output("value", DeclaredValue::local::<i64>())
            .unwrap(),
    )
    .unwrap()
}
fn module(name: &str) -> ModulePackage {
    let mut graph = GraphBuilder::new(name);
    let worker = graph.node("private", |x: i64| x).unwrap();
    graph
        .expose_input_field(
            "run",
            phenix_graph::SlotField::<(), i64>::new("value"),
            worker.inlet(),
        )
        .unwrap();
    graph.expose_output("value", worker.output()).unwrap();
    graph.implements_public(&contract()).unwrap();
    graph.module()
}
fn inspect(unit: &dyn GraphUnit) -> phenix_graph::UnitSnapshot {
    unit.inspect().unwrap()
}

#[test]
fn shape_has_no_semantic_identity_and_membership_requires_claim() {
    let mut graph = GraphBuilder::new("root");
    graph.node("claimed", |x: i64| x).unwrap();
    graph.node("unclaimed", |x: i64| x).unwrap();
    graph.implements("claimed", &contract()).unwrap();
    let a = inspect(&graph.inspect("claimed").unwrap());
    let b = inspect(&graph.inspect("unclaimed").unwrap());
    assert!(graph.bind_contract("unclaimed", &contract()).is_err());
    assert!(graph.bind_contract("missing", &contract()).is_err());
    assert_eq!(a.shape, b.shape);
    assert!(a.check_contract(&contract()).is_ok());
    assert!(b.check_contract(&contract()).is_err());
    let compiled = graph.compile().unwrap();
    assert!(
        compiled
            .find("claimed")
            .unwrap()
            .bind_contract(&contract())
            .is_ok()
    );
    assert!(
        compiled
            .find("unclaimed")
            .unwrap()
            .bind_contract(&contract())
            .is_err()
    );
    let other = GraphContract::new("unrelated@1", contract().required().clone()).unwrap();
    assert!(
        compiled
            .find("claimed")
            .unwrap()
            .bind_contract(&other)
            .is_err()
    );
}

#[test]
fn exact_definition_and_single_output_shape_matching_are_distinct() {
    let required = contract();
    let mut actual = InterfaceShape::new();
    for (port, slot, value) in required.required().inputs() {
        actual = actual.input_slot(port, slot, value).unwrap();
    }
    let actual = actual
        .output("value", DeclaredValue::local::<String>())
        .unwrap();
    assert!(required.required().check_shape(&actual).is_err());
    assert!(!required.required().same_shape(&actual));
    let changed = GraphContract::new(required.id(), actual).unwrap();
    assert!(required.check_definition(&changed).is_err());
    assert!(
        required
            .check_definition(
                &GraphContract::new("different", required.required().clone()).unwrap()
            )
            .is_err()
    );
    for shape in [
        InterfaceShape::new(),
        InterfaceShape::new()
            .input_slot("wrong", "value", DeclaredValue::local::<i64>())
            .unwrap(),
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::wire::<i64>())
            .unwrap(),
    ] {
        assert!(required.required().check_shape(&shape).is_err());
    }
}

#[test]
fn construction_rejects_duplicate_and_invalid_names() {
    let v = DeclaredValue::local::<i64>();
    assert!(
        InterfaceShape::new()
            .input_slot("run", "value", v.clone())
            .unwrap()
            .input_slot("run", "value", v.clone())
            .is_err()
    );
    assert!(
        InterfaceShape::new()
            .output("value", v.clone())
            .unwrap()
            .output("value", v.clone())
            .is_err()
    );
    assert!(InterfaceShape::new().input_slot("", "value", v).is_err());
    assert!(GraphContract::new("bad//id", InterfaceShape::new()).is_err());
}

#[test]
fn claim_edits_participate_in_patch_footprints_and_rollback() {
    use phenix_graph::{
        GraphAssembly, PatchBuilder, PatchError, PatchPlanner, PatchSpec, PatchTarget,
    };
    let mut assembly = GraphAssembly::new("root");
    let mut base = PatchPlanner::new();
    base.add_patch(PatchBuilder::custom(
        PatchSpec::new("base").writes(PatchTarget::Node("worker".into())),
        |graph| {
            graph
                .node("worker", |x: i64| x)
                .map_err(|e| e.to_string())?;
            Ok(())
        },
    ))
    .unwrap();
    base.apply(&mut assembly).unwrap();
    let mut planner = PatchPlanner::new();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("dishonest").writes(PatchTarget::Node("other".into())),
            |graph| {
                graph
                    .implements("worker", &contract())
                    .map_err(|e| e.to_string())
            },
        ))
        .unwrap();
    assert!(matches!(
        planner.apply(&mut assembly),
        Err(PatchError::UndeclaredWrite { .. })
    ));
    let graph = assembly.finish().unwrap().graph;
    assert!(
        graph
            .find("worker")
            .unwrap()
            .bind_contract(&contract())
            .is_err()
    );
}

#[test]
fn root_claim_survives_packaging_and_nested_instances() {
    let package = module("service");
    let mut wrapper = GraphBuilder::new("wrapper");
    wrapper.instance("inside", &package).unwrap();
    let output = wrapper
        .discover("inside")
        .unwrap()
        .output::<i64, phenix_graph::LocalValue>()
        .unwrap();
    wrapper.expose_output("value", output).unwrap();
    let wrapper = wrapper.module();
    let mut graph = GraphBuilder::new("root");
    graph.instance("one", &wrapper).unwrap();
    graph.instance("two", &wrapper).unwrap();
    for path in ["one/inside", "two/inside"] {
        inspect(&graph.inspect(path).unwrap())
            .check_contract(&contract())
            .unwrap();
    }
    let compiled = graph.compile().unwrap();
    for path in ["one/inside", "two/inside"] {
        let unit = compiled.find(path).unwrap();
        let snapshot = inspect(&unit);
        assert_eq!(snapshot.provenance.phase, InterfacePhase::Compiled);
        assert_eq!(snapshot.provenance.path, path);
        assert_eq!(snapshot.provenance.definition, "service");
        assert_eq!(snapshot.shape, *contract().required());
        let bound = unit.bind_contract(&contract()).unwrap();
        assert_eq!(
            bound
                .input("run", "value")
                .unwrap()
                .resolve(&compiled)
                .unwrap()
                .len(),
            1
        );
        assert!(bound.output("value").is_ok());
        assert!(bound.output("private").is_err());
        assert!(bound.input("private", "value").is_err());
    }
    match compiled.find("one/inside/private").unwrap() {
        FindResult::Node(n) => {
            inspect(&n);
        }
        _ => panic!("not a node"),
    }
    match compiled.find("one/inside").unwrap() {
        FindResult::Module(m) => {
            inspect(&m);
        }
        _ => panic!("not a module"),
    }
}

#[test]
fn nested_claim_edit_is_definition_owned_and_clone_isolated() {
    let package = module("service");
    let mut graph = GraphBuilder::new("root");
    graph.instance("one", &package).unwrap();
    graph.instance("two", &package).unwrap();
    let sibling = graph.clone();
    graph.implements("one/private", &contract()).unwrap();
    inspect(&graph.inspect("two/private").unwrap())
        .check_contract(&contract())
        .unwrap();
    assert!(
        inspect(&sibling.inspect("one/private").unwrap())
            .check_contract(&contract())
            .is_err()
    );
    graph.compile().unwrap();
}

#[test]
fn low_level_claims_and_same_id_conflicts_are_checked_finally() {
    let mut graph = GraphBuilder::new("root");
    let mut bad = NodeTemplate::function(|x: i64| x);
    bad.claims.push(contract());
    graph.add("bad", bad).unwrap();
    let error = graph.compile().err().unwrap().to_string();
    assert!(
        error.contains("test/transform@1") && error.contains("bad"),
        "{error}"
    );

    let mut graph = GraphBuilder::new("root");
    graph.node("one", |x: i64| x).unwrap();
    graph.node("two", |x: i64| x > 0).unwrap();
    graph.implements("one", &contract()).unwrap();
    let conflicting = GraphContract::new(
        contract().id(),
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::local::<i64>())
            .unwrap()
            .output("value", DeclaredValue::local::<bool>())
            .unwrap(),
    )
    .unwrap();
    graph.implements("two", &conflicting).unwrap();
    assert!(graph.compile().is_err());

    let mut builder = ModuleDefBuilder::new("direct");
    let mut entry = builder.clone().build();
    entry.definition.claims.push(contract());
    let mut catalog = ModuleCatalog::new();
    catalog.add(entry).unwrap();
    assert!(catalog.compile("direct").is_err());
    builder
        .register("n", NodeTemplate::function(|x: i64| x))
        .unwrap();
}

#[test]
fn final_requests_require_actual_claim_and_definition() {
    for claimed in [false, true] {
        let mut graph = GraphBuilder::new("root");
        graph.node("worker", |x: i64| x).unwrap();
        if claimed {
            graph.implements("worker", &contract()).unwrap();
        }
        graph.require_contract("worker", &contract()).unwrap();
        assert_eq!(graph.compile().is_ok(), claimed);
    }
    let mut graph = GraphBuilder::new("root");
    graph.require_contract("missing", &contract()).unwrap();
    assert!(graph.compile().is_err());
}

#[test]
fn definitions_conflict_across_modules_and_final_requests() {
    let first = module("first");
    let conflicting = GraphContract::new(
        contract().id(),
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::local::<()>())
            .unwrap()
            .output("value", DeclaredValue::local::<bool>())
            .unwrap(),
    )
    .unwrap();
    use phenix_graph::{InputSlot, InvocationCtx, OutputSlot, Port};
    let mut second = GraphBuilder::new("second");
    second
        .add(
            "alternate",
            NodeTemplate::with_action(
                0,
                vec![Port::all(0, vec![InputSlot::local::<()>(0).named("value")]).named("run")],
                OutputSlot::local::<bool>(0).named("value"),
                |_ctx: InvocationCtx| Ok(Vec::new()),
            ),
        )
        .unwrap();
    let output = second
        .discover("alternate")
        .unwrap()
        .output::<bool, phenix_graph::LocalValue>()
        .unwrap();
    let input = second
        .discover("alternate")
        .unwrap()
        .inlets::<(), phenix_graph::LocalValue>()
        .port(phenix_graph::InletPort::<(), ()>::new("run"))
        .field(phenix_graph::SlotField::<(), ()>::new("value"))
        .target()
        .unwrap();
    second
        .expose_input_field(
            "run",
            phenix_graph::SlotField::<(), ()>::new("value"),
            input,
        )
        .unwrap();
    second.expose_output("value", output).unwrap();
    second.implements_public(&conflicting).unwrap();
    let second = second.module();
    let mut graph = GraphBuilder::new("root");
    graph.instance("first", &first).unwrap();
    graph.instance("second", &second).unwrap();
    let error = graph.compile().err().unwrap().to_string();
    assert!(error.contains(contract().id()), "{error}");

    let mut graph = GraphBuilder::new("root");
    graph.instance("first", &first).unwrap();
    graph.require_contract("first", &conflicting).unwrap();
    assert!(graph.compile().is_err());
}

#[test]
fn errors_report_contract_phase_path_endpoint_and_identities() {
    use phenix_graph::TypeIdentity;
    let mut graph = GraphBuilder::new("root");
    graph.node("worker", |x: i64| x).unwrap();
    let wrong = GraphContract::new(
        "wrong@1",
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::local::<i64>())
            .unwrap()
            .output("value", DeclaredValue::local::<bool>())
            .unwrap(),
    )
    .unwrap();
    let error = graph.implements("worker", &wrong).unwrap_err();
    assert_eq!(error.phase, InterfacePhase::Authoring);
    assert_eq!(error.path.as_ref(), "worker");
    assert_eq!(error.contract.as_deref(), Some("wrong@1"));
    assert_eq!(error.endpoint.as_deref(), Some("output value"));
    assert_eq!(
        error.expected,
        Some(TypeIdentity::Local(std::any::TypeId::of::<bool>()))
    );
    assert_eq!(
        error.actual,
        Some(TypeIdentity::Local(std::any::TypeId::of::<i64>()))
    );
    assert!(inspect(&graph.inspect("worker").unwrap()).claims.is_empty());
}

#[test]
fn replacement_requires_explicit_claims_and_complete_shape() {
    let mut graph = GraphBuilder::new("root");
    graph.node("worker", |x: i64| x).unwrap();
    graph.implements("worker", &contract()).unwrap();
    let mut replacement = NodeTemplate::local_function::<i64, i64, i64, _>(|x| x + 1);
    replacement.claims.push(contract());
    graph.replace("worker", replacement).unwrap();
    inspect(&graph.inspect("worker").unwrap())
        .check_contract(&contract())
        .unwrap();
    assert!(
        graph
            .replace("worker", NodeTemplate::function(|x: i64| x))
            .is_err()
    );
    graph.compile().unwrap();
}

#[test]
fn bound_lookup_hides_actual_extensions_and_shapes_cover_multiple_ports() {
    use phenix_graph::{AllSameCohort, InputSlot, InvocationCtx, OutputSlot, Port};
    let template = NodeTemplate::with_action(
        1,
        vec![
            Port::new(
                0,
                vec![
                    InputSlot::local::<i64>(0).named("left"),
                    InputSlot::local::<bool>(1).named("flag"),
                ],
                AllSameCohort,
            )
            .named("run"),
            Port::new(
                1,
                vec![InputSlot::local::<String>(0).named("value")],
                AllSameCohort,
            )
            .named("extra"),
        ],
        OutputSlot::local::<i64>(0).named("reply"),
        |_ctx: InvocationCtx| Ok(Vec::new()),
    );
    let contract = GraphContract::new(
        "subset@1",
        InterfaceShape::new()
            .input_slot("run", "left", DeclaredValue::local::<i64>())
            .unwrap()
            .input_slot("run", "flag", DeclaredValue::local::<bool>())
            .unwrap()
            .input_slot("extra", "value", DeclaredValue::local::<String>())
            .unwrap()
            .output("reply", DeclaredValue::local::<i64>())
            .unwrap(),
    )
    .unwrap();
    let mut graph = GraphBuilder::new("root");
    graph.add("multi", template).unwrap();
    graph.implements("multi", &contract).unwrap();
    let actual = inspect(&graph.inspect("multi").unwrap());
    assert_eq!(actual.shape.inputs().count(), 3);
    assert_eq!(actual.shape.output_value().unwrap().0, Some("reply"));
    let bound = graph
        .inspect("multi")
        .unwrap()
        .bind_contract(&contract)
        .unwrap();
    assert!(
        bound
            .operation()
            .unwrap()
            .inlets::<(), phenix_graph::LocalValue>()
            .port(phenix_graph::InletPort::<(), i64>::new("run"))
            .field(phenix_graph::SlotField::<i64, i64>::new("left"))
            .target()
            .is_ok()
    );
    assert!(
        bound
            .operation()
            .unwrap()
            .inlets::<(), phenix_graph::LocalValue>()
            .port(phenix_graph::InletPort::<(), bool>::new("run"))
            .field(phenix_graph::SlotField::<bool, bool>::new("flag"))
            .target()
            .is_ok()
    );
    assert!(
        bound
            .operation()
            .unwrap()
            .inlets::<(), phenix_graph::LocalValue>()
            .port(phenix_graph::InletPort::<(), String>::new("extra"))
            .field(phenix_graph::SlotField::<String, String>::new("value"))
            .target()
            .is_ok()
    );
    assert!(
        bound
            .operation()
            .unwrap()
            .output::<i64, phenix_graph::LocalValue>()
            .is_ok()
    );
    assert!(
        bound
            .operation()
            .unwrap()
            .output::<String, phenix_graph::LocalValue>()
            .is_err()
    );
    assert!(
        bound
            .operation()
            .unwrap()
            .output::<bool, phenix_graph::LocalValue>()
            .is_err()
    );
    assert!(
        bound
            .operation()
            .unwrap()
            .inlets::<(), phenix_graph::LocalValue>()
            .port(phenix_graph::InletPort::<(), bool>::new("run"))
            .field(phenix_graph::SlotField::<bool, bool>::new("left"))
            .target()
            .is_err()
    );
    assert!(
        bound
            .operation()
            .unwrap()
            .inlets::<(), phenix_graph::LocalValue>()
            .port(phenix_graph::InletPort::<(), i64>::new("run"))
            .field(phenix_graph::SlotField::<i64, i64>::new("unpromised"))
            .target()
            .is_err()
    );
    assert!(
        bound
            .operation()
            .unwrap()
            .output::<i64, phenix_graph::EncodedValue>()
            .is_err()
    );
    assert!(
        graph
            .discover("multi")
            .unwrap()
            .output::<String, phenix_graph::LocalValue>()
            .is_err()
    );
    let compiled = graph.compile().unwrap();
    assert_eq!(
        actual.shape,
        inspect(&compiled.find("multi").unwrap()).shape
    );
    let bound = compiled
        .find("multi")
        .unwrap()
        .bind_contract(&contract)
        .unwrap();
    assert!(bound.input("run", "left").is_ok());
    assert!(bound.output("reply").is_ok());
    assert!(bound.output("debug").is_err());
    assert!(bound.input("extra", "value").is_ok());
    assert!(bound.input("run", "flag").is_ok());
    assert!(bound.input("run", "unpromised").is_err());
}

#[test]
fn authoring_wire_binding_limits_endpoints_and_connects_nodes_and_nested_modules() {
    use phenix_graph::{
        AllSameCohort, ContractValue, InputSlot, InvocationCtx, Node, OutputSlot, Port, Schema,
    };
    struct Number(i64);
    impl ContractValue for Number {
        fn schema() -> Schema {
            Schema::record("test/number", &[("value", false, i64::schema())])
        }
        fn encode(&self) -> Vec<u8> {
            self.0.encode()
        }
        fn decode(bytes: &[u8]) -> Result<Self, String> {
            i64::decode(bytes).map(Self)
        }
    }
    let required = GraphContract::new(
        "wire/transform@1",
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::wire::<Number>())
            .unwrap()
            .output("value", DeclaredValue::wire::<Number>())
            .unwrap(),
    )
    .unwrap();
    let template = NodeTemplate::with_action(
        1,
        vec![
            Port::new(
                0,
                vec![InputSlot::deferred::<Number>(0).named("value")],
                AllSameCohort,
            )
            .named("run"),
        ],
        OutputSlot::typed::<Number>(0).named("value"),
        |_ctx: InvocationCtx| Ok(Vec::new()),
    );
    let mut service = GraphBuilder::new("service");
    service.add("private", template.clone()).unwrap();
    service.implements("private", &required).unwrap();
    let input = service
        .discover("private")
        .unwrap()
        .inlets::<(), phenix_graph::EncodedValue>()
        .port(phenix_graph::InletPort::<(), Number>::new("run"))
        .field(phenix_graph::SlotField::<Number, Number>::new("value"))
        .target()
        .unwrap();
    let output = service
        .discover("private")
        .unwrap()
        .output::<Number, phenix_graph::EncodedValue>()
        .unwrap();
    service
        .expose_input_field(
            "run",
            phenix_graph::SlotField::<(), Number>::new("value"),
            input,
        )
        .unwrap();
    service.expose_output("value", output).unwrap();
    service.implements_public(&required).unwrap();
    let mut wrapper = GraphBuilder::new("wrapper");
    wrapper.instance("inside", &service.module()).unwrap();
    let (input, output) = {
        let bound = wrapper.bind_contract("inside", &required).unwrap();
        assert!(
            bound
                .operation()
                .unwrap()
                .output::<bool, phenix_graph::EncodedValue>()
                .is_err()
        );
        (
            bound
                .operation()
                .unwrap()
                .inlets::<(), phenix_graph::EncodedValue>()
                .port(phenix_graph::InletPort::<(), Number>::new("run"))
                .field(phenix_graph::SlotField::<Number, Number>::new("value"))
                .target()
                .unwrap(),
            bound
                .operation()
                .unwrap()
                .output::<Number, phenix_graph::EncodedValue>()
                .unwrap(),
        )
    };
    wrapper
        .expose_input_field(
            "run",
            phenix_graph::SlotField::<(), Number>::new("value"),
            input,
        )
        .unwrap();
    wrapper.expose_output("value", output).unwrap();
    wrapper.implements_public(&required).unwrap();

    let mut graph = GraphBuilder::new("root");
    graph.add("node", template).unwrap();
    graph.implements("node", &required).unwrap();
    graph.instance("nested", &wrapper.module()).unwrap();
    for path in ["node", "nested"] {
        let source = graph
            .node(format!("source-{path}"), Node::wire(|x: Number| x))
            .unwrap();
        let sink = graph
            .node(format!("sink-{path}"), Node::wire(|x: Number| x))
            .unwrap();
        let (input, output) = {
            let bound = graph.bind_contract(path, &required).unwrap();
            let error = bound
                .operation()
                .unwrap()
                .output::<bool, phenix_graph::EncodedValue>()
                .err()
                .unwrap();
            assert_eq!(error.phase, InterfacePhase::Authoring);
            assert_eq!(error.path.as_ref(), path);
            assert_eq!(error.contract.as_deref(), Some(required.id()));
            assert_eq!(error.endpoint.as_deref(), Some("output value"));
            assert!(
                bound
                    .operation()
                    .unwrap()
                    .inlets::<(), phenix_graph::EncodedValue>()
                    .port(phenix_graph::InletPort::<(), Number>::new("run"))
                    .field(phenix_graph::SlotField::<Number, Number>::new("other"))
                    .target()
                    .is_err()
            );
            assert!(
                bound
                    .operation()
                    .unwrap()
                    .inlets::<(), phenix_graph::EncodedValue>()
                    .port(phenix_graph::InletPort::<(), bool>::new("run"))
                    .field(phenix_graph::SlotField::<bool, bool>::new("value"))
                    .target()
                    .is_err()
            );
            assert!(
                bound
                    .operation()
                    .unwrap()
                    .output::<bool, phenix_graph::EncodedValue>()
                    .is_err()
            );
            assert!(
                bound
                    .operation()
                    .unwrap()
                    .inlets::<(), phenix_graph::LocalValue>()
                    .port(phenix_graph::InletPort::<(), i64>::new("run"))
                    .field(phenix_graph::SlotField::<i64, i64>::new("value"))
                    .target()
                    .is_err()
            );
            assert!(
                bound
                    .operation()
                    .unwrap()
                    .output::<i64, phenix_graph::LocalValue>()
                    .is_err()
            );
            (
                bound
                    .operation()
                    .unwrap()
                    .inlets::<(), phenix_graph::EncodedValue>()
                    .port(phenix_graph::InletPort::<(), Number>::new("run"))
                    .field(phenix_graph::SlotField::<Number, Number>::new("value"))
                    .target()
                    .unwrap(),
                bound
                    .operation()
                    .unwrap()
                    .output::<Number, phenix_graph::EncodedValue>()
                    .unwrap(),
            )
        };
        assert!(
            graph
                .discover(path)
                .unwrap()
                .output::<bool, phenix_graph::EncodedValue>()
                .is_err()
        );
        graph.connect(source.output(), input).unwrap();
        graph.connect(output, sink.inlet()).unwrap();
    }
    // Inspection may see nested declarations; binding must not bypass authoring privacy.
    assert!(
        graph
            .bind_contract("nested/inside", &required)
            .unwrap()
            .operation()
            .is_err()
    );
    assert!(
        graph
            .bind_contract("nested/inside/private", &required)
            .unwrap()
            .operation()
            .is_err()
    );
    graph.compile().unwrap();
}

#[test]
fn authoring_bound_handles_retain_declaration_revision_checks() {
    let mut graph = GraphBuilder::new("root");
    graph.node("worker", |x: i64| x).unwrap();
    graph.implements("worker", &contract()).unwrap();
    let (input, output) = {
        let bound = graph.bind_contract("worker", &contract()).unwrap();
        (
            bound
                .operation()
                .unwrap()
                .inlets::<(), phenix_graph::LocalValue>()
                .port(phenix_graph::InletPort::<(), i64>::new("run"))
                .field(phenix_graph::SlotField::<i64, i64>::new("value"))
                .target()
                .unwrap(),
            bound
                .operation()
                .unwrap()
                .output::<i64, phenix_graph::LocalValue>()
                .unwrap(),
        )
    };
    let mut replacement = NodeTemplate::local_function::<i64, i64, i64, _>(|x| x);
    replacement.claims.push(contract());
    graph.replace("worker", replacement).unwrap();
    let fresh_input = graph
        .discover("worker")
        .unwrap()
        .inlets::<(), phenix_graph::LocalValue>()
        .port(phenix_graph::InletPort::<(), i64>::new("run"))
        .field(phenix_graph::SlotField::<i64, i64>::new("value"))
        .target()
        .unwrap();
    let fresh_output = graph
        .discover("worker")
        .unwrap()
        .output::<i64, phenix_graph::LocalValue>()
        .unwrap();
    assert!(graph.connect(output, fresh_input).is_err());
    assert!(graph.connect(fresh_output, input).is_err());
}

#[test]
fn h07_shape_matching_requires_exact_named_inputs_and_exactly_one_output() {
    let required = contract();
    let inputs = required.required();
    let extra_port = inputs
        .clone()
        .input_slot("extra", "value", DeclaredValue::local::<i64>())
        .unwrap();
    let extra_slot = inputs
        .clone()
        .input_slot("run", "extra", DeclaredValue::local::<i64>())
        .unwrap();
    let missing_slot = InterfaceShape::new()
        .input_slot("run", "other", DeclaredValue::local::<i64>())
        .unwrap()
        .output("value", DeclaredValue::local::<i64>())
        .unwrap();
    let missing_port = InterfaceShape::new()
        .output("value", DeclaredValue::local::<i64>())
        .unwrap();
    for actual in [extra_port, extra_slot, missing_slot, missing_port] {
        assert!(inputs.check_shape(&actual).is_err());
        assert!(!inputs.same_shape(&actual));
    }
    assert!(
        inputs
            .clone()
            .output("debug", DeclaredValue::local::<bool>())
            .is_err()
    );
    let mut changed = InterfaceShape::new();
    for (port, slot, value) in inputs.inputs() {
        changed = changed.input_slot(port, slot, value).unwrap();
    }
    let changed = changed
        .output("value", DeclaredValue::local::<bool>())
        .unwrap();
    assert!(inputs.check_shape(&changed).is_err());
    assert!(!inputs.same_shape(&changed));
}

#[test]
fn h07_actual_node_checks_and_final_claims_reject_extra_inputs() {
    use phenix_graph::{AllSameCohort, InputSlot, InvocationCtx, OutputSlot, Port};
    for extra_port in [false, true] {
        let mut ports = vec![
            Port::new(
                0,
                vec![InputSlot::local::<i64>(0).named("value")],
                AllSameCohort,
            )
            .named("run"),
        ];
        if extra_port {
            ports.push(
                Port::new(
                    1,
                    vec![InputSlot::local::<bool>(0).named("value")],
                    AllSameCohort,
                )
                .named("extra"),
            );
        } else {
            ports[0] = Port::new(
                0,
                vec![
                    InputSlot::local::<i64>(0).named("value"),
                    InputSlot::local::<bool>(1).named("extra"),
                ],
                AllSameCohort,
            )
            .named("run");
        }
        let template = NodeTemplate::with_action(
            1,
            ports,
            OutputSlot::local::<i64>(0).named("value"),
            |_ctx: InvocationCtx| Ok(Vec::new()),
        );
        let mut graph = GraphBuilder::new("root");
        graph.add("worker", template.clone()).unwrap();
        assert!(
            contract()
                .check_shape(&graph.inspect("worker").unwrap())
                .is_err()
        );
        assert!(graph.implements("worker", &contract()).is_err());
        let compiled = graph.compile().unwrap();
        assert!(
            contract()
                .check_shape(&compiled.find("worker").unwrap())
                .is_err()
        );

        let mut template = template;
        template.claims.push(contract());
        let mut graph = GraphBuilder::new("root");
        graph.add("worker", template).unwrap();
        assert!(graph.compile().is_err());
    }
}

#[test]
fn h07_actual_module_checks_reject_extra_public_ports_and_slots() {
    for extra_port in [false, true] {
        let mut module = GraphBuilder::new("implementation");
        let first = module.node("first", |x: i64| x).unwrap();
        let second = module.node("second", |x: i64| x).unwrap();
        module
            .expose_input_field(
                "run",
                phenix_graph::SlotField::<(), i64>::new("value"),
                first.inlet(),
            )
            .unwrap();
        module
            .expose_input_field(
                if extra_port { "extra" } else { "run" },
                phenix_graph::SlotField::<(), i64>::new(if extra_port { "value" } else { "extra" }),
                second.inlet(),
            )
            .unwrap();
        module.expose_output("value", first.output()).unwrap();
        assert!(
            contract()
                .check_shape(&module.inspect("").unwrap())
                .is_err()
        );
        assert!(module.implements_public(&contract()).is_err());
        let mut graph = GraphBuilder::new("root");
        graph.instance("service", &module.module()).unwrap();
        assert!(
            contract()
                .check_shape(&graph.inspect("service").unwrap())
                .is_err()
        );
        let compiled = graph.compile().unwrap();
        assert!(
            contract()
                .check_shape(&compiled.find("service").unwrap())
                .is_err()
        );
    }
}

#[test]
fn graph_contract_structural_and_semantic_checks_are_distinct_on_real_units() {
    let mut graph = GraphBuilder::new("root");
    graph.node("worker", |x: i64| x).unwrap();
    let required = contract();
    let view = graph.inspect("worker").unwrap();
    required.check_shape(&view).unwrap();
    assert!(required.check_implementation(&view).is_err());
    graph.implements("worker", &required).unwrap();
    required
        .check_implementation(&graph.inspect("worker").unwrap())
        .unwrap();
    let compiled = graph.compile().unwrap();
    required
        .check_implementation(&compiled.find("worker").unwrap())
        .unwrap();
    let mut different_shape = InterfaceShape::new();
    for (port, slot, value) in required.required().inputs() {
        different_shape = different_shape.input_slot(port, slot, value).unwrap();
    }
    let different_shape = different_shape
        .output("value", DeclaredValue::local::<bool>())
        .unwrap();
    let different_definition = GraphContract::new(required.id(), different_shape).unwrap();
    assert!(
        different_definition
            .check_implementation(&compiled.find("worker").unwrap())
            .is_err()
    );
}

#[test]
fn contradictory_same_id_claims_cannot_pass_authoring_implementation_checks() {
    let required = contract();
    let different = GraphContract::new(
        required.id(),
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::local::<i64>())
            .unwrap()
            .output("value", DeclaredValue::local::<bool>())
            .unwrap(),
    )
    .unwrap();
    for claims in [
        vec![required.clone(), different.clone()],
        vec![different.clone(), required.clone()],
    ] {
        let mut graph = GraphBuilder::new("root");
        let mut template = NodeTemplate::local_function::<i64, i64, i64, _>(|x| x);
        template.claims = claims;
        graph.add("worker", template).unwrap();
        required
            .check_shape(&graph.inspect("worker").unwrap())
            .unwrap();
        assert!(
            required
                .check_implementation(&graph.inspect("worker").unwrap())
                .is_err()
        );
        assert!(graph.implements("worker", &required).is_err());
        assert!(graph.compile().is_err());
    }
}

#[test]
fn endpoint_names_follow_graph_rules_instead_of_contract_id_rules() {
    use phenix_graph::{AllSameCohort, InputSlot, InvocationCtx, OutputSlot, Port};
    let shape = InterfaceShape::new()
        .input_slot("run port", "request:field", DeclaredValue::local::<i64>())
        .unwrap()
        .output("reply value", DeclaredValue::local::<i64>())
        .unwrap();
    let required = GraphContract::new("names@1", shape).unwrap();
    let template = NodeTemplate::with_action(
        1,
        vec![
            Port::new(
                0,
                vec![InputSlot::local::<i64>(0).named("request:field")],
                AllSameCohort,
            )
            .named("run port"),
        ],
        OutputSlot::local::<i64>(0).named("reply value"),
        |_ctx: InvocationCtx| Ok(Vec::new()),
    );
    let mut graph = GraphBuilder::new("root");
    graph.add("worker", template).unwrap();
    graph.implements("worker", &required).unwrap();
    let compiled = graph.compile().unwrap();
    required
        .check_implementation(&compiled.find("worker").unwrap())
        .unwrap();
    for name in [".", "..", "a/b", "bad\nname"] {
        assert!(
            InterfaceShape::new()
                .output(name, DeclaredValue::local::<i64>())
                .is_err()
        );
        assert!(
            InterfaceShape::new()
                .input_slot(name, "value", DeclaredValue::local::<i64>())
                .is_err()
        );
    }
}

#[test]
fn invalid_claim_never_activates_an_action_factory() {
    use phenix_graph::{AllSameCohort, InputSlot, InvocationCtx, OutputSlot, Port};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let mut template = NodeTemplate::with_action_factory(
        1,
        vec![
            Port::new(
                0,
                vec![InputSlot::deferred::<i64>(0).named("value")],
                AllSameCohort,
            )
            .named("run"),
        ],
        OutputSlot::typed::<i64>(0).named("value"),
        move || {
            counter.fetch_add(1, Ordering::SeqCst);
            |_ctx: InvocationCtx| Ok(Vec::new())
        },
    );
    template.claims.push(contract());
    let mut graph = GraphBuilder::new("root");
    graph.add("bad", template).unwrap();
    assert!(graph.compile().is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn equal_repeated_definitions_accept_and_distinct_local_types_reject() {
    struct First;
    struct Second;
    let a = InterfaceShape::new()
        .output("value", DeclaredValue::local::<First>())
        .unwrap();
    let b = InterfaceShape::new()
        .output("value", DeclaredValue::local::<Second>())
        .unwrap();
    assert!(a.check_shape(&b).is_err());
    let mut graph = GraphBuilder::new("root");
    for name in ["one", "two"] {
        graph.node(name, |x: i64| x).unwrap();
        graph.implements(name, &contract()).unwrap();
    }
    graph.compile().unwrap();
}

#[test]
fn module_replacement_rejects_claim_removal_without_mutation() {
    let old = module("old");
    let mut replacement = GraphBuilder::new("replacement");
    let n = replacement.node("different", |x: i64| x).unwrap();
    replacement
        .expose_input_field(
            "run",
            phenix_graph::SlotField::<(), i64>::new("value"),
            n.inlet(),
        )
        .unwrap();
    replacement.expose_output("value", n.output()).unwrap();
    let mut graph = GraphBuilder::new("root");
    graph.instance("service", &old).unwrap();
    let before = inspect(&graph.inspect("service").unwrap());
    assert!(graph.replace("service", replacement.module()).is_err());
    assert_eq!(before, inspect(&graph.inspect("service").unwrap()));
    graph
        .replace("service", module("claimed-replacement"))
        .unwrap();
    graph.compile().unwrap();
}

#[cfg(feature = "derive")]
#[test]
fn invalid_pending_module_boundary_returns_contextual_error() {
    use phenix_graph::{Layout, LocalPortLayout};
    #[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
    struct Pair {
        left: i64,
        right: i64,
    }
    let mut graph = GraphBuilder::new("root");
    let node = graph.node("worker", |x: Pair| x.left + x.right).unwrap();
    graph.expose_input("request", node.inlet()).unwrap();
    let _node = node.input(Layout::fields()).unwrap();
    let error = match graph.inspect("") {
        Ok(_) => panic!("invalid boundary accepted"),
        Err(e) => e,
    };
    assert_eq!(error.phase, InterfacePhase::Authoring);
    assert!(graph.compile().is_err());
}

#[cfg(feature = "derive")]
#[test]
fn inspection_reads_pending_layout_and_final_compile_rechecks_claim() {
    use phenix_graph::{Layout, LocalPortLayout};
    #[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
    struct Pair {
        left: i64,
        right: i64,
    }
    let mut graph = GraphBuilder::new("root");
    let n = graph.node("worker", |x: Pair| x.left + x.right).unwrap();
    let contract =
        GraphContract::new("pair@1", inspect(&graph.inspect("worker").unwrap()).shape).unwrap();
    graph.implements("worker", &contract).unwrap();
    let _n = n.input(Layout::fields()).unwrap();
    let snapshot = inspect(&graph.inspect("worker").unwrap());
    assert_eq!(snapshot.shape.inputs().count(), 2);
    assert_eq!(snapshot.claims, vec![contract]);
    let error = graph.compile().err().unwrap().to_string();
    assert!(
        error.contains("pair@1") && error.contains("worker"),
        "{error}"
    );
}

#[cfg(feature = "derive")]
#[test]
fn retained_authoring_binding_rechecks_full_contract_after_layout_change() {
    use phenix_graph::{Layout, LocalPortLayout};
    #[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
    struct Pair {
        left: i64,
        right: i64,
    }
    let mut graph = GraphBuilder::new("root");
    let node = graph.node("worker", |x: Pair| x.left + x.right).unwrap();
    let contract =
        GraphContract::new("pair@1", inspect(&graph.inspect("worker").unwrap()).shape).unwrap();
    graph.implements("worker", &contract).unwrap();
    let view = graph.inspect("worker").unwrap();
    let bound = graph.bind_contract("worker", &contract).unwrap();
    assert!(
        bound
            .operation()
            .unwrap()
            .inlets::<(), phenix_graph::LocalValue>()
            .port(phenix_graph::InletPort::<(), Pair>::new("run"))
            .field(phenix_graph::SlotField::<Pair, Pair>::new("value"))
            .target()
            .is_ok()
    );
    assert!(
        bound
            .operation()
            .unwrap()
            .output::<i64, phenix_graph::LocalValue>()
            .is_ok()
    );

    let _node = node.input(Layout::fields()).unwrap();
    assert_eq!(inspect(&view).shape.inputs().count(), 2);
    // The output itself still exists, but the complete input contract is broken.
    let error = bound
        .operation()
        .and_then(|operation| operation.output::<i64, phenix_graph::LocalValue>())
        .err()
        .unwrap();
    assert_eq!(error.phase, InterfacePhase::Authoring);
    assert_eq!(error.path.as_ref(), "worker");
    assert_eq!(error.contract.as_deref(), Some("pair@1"));
    assert!(bound.operation().is_err());
    assert!(graph.bind_contract("worker", &contract).is_err());
    assert!(view.bind_contract(&contract).is_err());
}

#[test]
fn same_shape_node_replacement_requires_author_claim_without_mutating_on_failure() {
    let mut graph = GraphBuilder::new("root");
    let worker = graph.node("worker", |value: i64| value).unwrap();
    let required = contract();
    graph.implements("worker", &required).unwrap();
    let before = inspect(&graph.inspect("worker").unwrap());

    let replacement = NodeTemplate::local_function::<i64, i64, i64, _>(|value| value + 1);
    let error = graph.replace("worker", replacement.clone()).unwrap_err();
    assert!(
        error.contains("replacement removes contract claim test/transform@1"),
        "{error}"
    );
    assert_eq!(inspect(&graph.inspect("worker").unwrap()), before);
    required
        .check_implementation(&graph.inspect("worker").unwrap())
        .unwrap();

    // Rejection must leave the original declaration handle and implementation usable.
    let input = graph.expose_input("request", worker.inlet()).unwrap();
    graph.trigger("run", [input]).unwrap();
    let unchanged = graph.clone().compile().unwrap();
    let report = phenix_graph::Deployment::new(unchanged)
        .report_retention(phenix_graph::ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire("run", vec![phenix_graph::Value::local(7_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report.publications().last().unwrap().borrow::<i64>(),
        Some(&7)
    );

    let mut replacement = replacement;
    replacement.claims.push(required.clone());
    graph.replace("worker", replacement).unwrap();
    let compiled = graph.compile().unwrap();
    required
        .check_implementation(&compiled.find("worker").unwrap())
        .unwrap();
    let report = phenix_graph::Deployment::new(compiled)
        .report_retention(phenix_graph::ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire("run", vec![phenix_graph::Value::local(7_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report.publications().last().unwrap().borrow::<i64>(),
        Some(&8)
    );
}
