use phenix_graph::{
    AllSameCohort, DeclaredValue, DecodedInputLayout, Deployment, GraphBuilder, GraphContract,
    InputLayout, InputSlot, InterfaceShape, LayoutError, NodeTemplate, ReportRetention,
    SelectedValues, SlotField, Value,
};

const LEFT: SlotField<(i64, i64), i64> = SlotField::new("pair.left");
const RIGHT: SlotField<(i64, i64), i64> = SlotField::new("pair.right");

#[test]
fn descriptor_copy_and_clone_do_not_require_cloneable_domain_values() {
    struct Root;
    struct Branch;
    struct Leaf;
    let branch = SlotField::<Root, Branch>::new("branch");
    let leaf = SlotField::<Branch, Leaf>::new("leaf");
    let path = branch.then(leaf);
    assert_eq!(branch.then(leaf).name(), path.clone().name());
    assert_eq!(path.name(), "branch.leaf");
}

struct PairLayout;
impl InputLayout for PairLayout {
    type Value = (i64, i64);
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        Ok(vec![
            InputSlot::deferred::<i64>(0).named(LEFT.name()),
            InputSlot::deferred::<i64>(1).named(RIGHT.name()),
        ])
    }
}
impl DecodedInputLayout for PairLayout {
    fn prepare(values: &SelectedValues<'_>) -> Result<Self::Value, String> {
        Ok((
            values.get_path(LEFT.name())?,
            values.get_path(RIGHT.name())?,
        ))
    }
}

#[test]
fn manual_wire_grouped_fields_use_the_same_contract_projection_and_execution() {
    let contract = GraphContract::new(
        "manual/subtract@1",
        InterfaceShape::new()
            .input_slot("request", LEFT.name(), DeclaredValue::wire::<i64>())
            .unwrap()
            .input_slot("request", RIGHT.name(), DeclaredValue::wire::<i64>())
            .unwrap()
            .output("result", DeclaredValue::wire::<i64>())
            .unwrap(),
    )
    .unwrap();
    let mut provider = GraphBuilder::new("provider");
    provider
        .add(
            "private",
            NodeTemplate::with_input_layout::<PairLayout, i64, _>(
                "run",
                AllSameCohort,
                |(left, right)| left - right,
            )
            .unwrap(),
        )
        .unwrap();
    for key in [LEFT, RIGHT] {
        let input = provider
            .discover("private")
            .unwrap()
            .inlets::<(), phenix_graph::EncodedValue>()
            .port(phenix_graph::InletPort::<(), (i64, i64)>::new("run"))
            .field(phenix_graph::SlotField::<(i64, i64), i64>::new(key.name()))
            .target()
            .unwrap();
        provider.expose_input_field("request", key, input).unwrap();
    }
    let output = provider
        .discover("private")
        .unwrap()
        .output::<i64, phenix_graph::EncodedValue>()
        .unwrap();
    provider.expose_output("result", output).unwrap();
    provider.implements_public(&contract).unwrap();

    let mut wrapper = GraphBuilder::new("wrapper");
    let inner = wrapper.instance("inner", &provider.module()).unwrap();
    for key in [LEFT, RIGHT] {
        let input = inner
            .operation()
            .unwrap()
            .inlets::<(), phenix_graph::EncodedValue>()
            .port(phenix_graph::InletPort::<(), (i64, i64)>::new("request"))
            .field(key)
            .target()
            .unwrap();
        wrapper.expose_input_field("request", key, input).unwrap();
    }
    wrapper
        .expose_output(
            "result",
            wrapper
                .discover("inner")
                .unwrap()
                .output::<i64, phenix_graph::EncodedValue>()
                .unwrap(),
        )
        .unwrap();
    wrapper.implements_public(&contract).unwrap();

    let mut graph = GraphBuilder::new("consumer");
    graph.instance("service", &wrapper.module()).unwrap();
    let bound = graph.bind_contract("service", &contract).unwrap();
    let operation = bound.operation().unwrap();
    let request = operation
        .inlets::<(), phenix_graph::EncodedValue>()
        .port(phenix_graph::InletPort::<(), (i64, i64)>::new("request"));
    let left = request.field(LEFT).target().unwrap();
    let right = request.field(RIGHT).target().unwrap();
    let result = operation
        .output::<i64, phenix_graph::EncodedValue>()
        .unwrap();
    assert!(
        operation
            .inlets::<(), phenix_graph::LocalValue>()
            .port(phenix_graph::InletPort::<(), (i64, i64)>::new("request"))
            .field(LEFT)
            .target()
            .is_err()
    );
    assert!(
        request
            .field(SlotField::<(i64, i64), bool>::new("pair.left"))
            .target()
            .is_err()
    );
    assert!(
        request
            .field(SlotField::<(i64, i64), i64>::new("pair..left"))
            .target()
            .is_err()
    );
    let left = graph.expose_input_field("request", LEFT, left).unwrap();
    let right = graph.expose_input_field("request", RIGHT, right).unwrap();
    graph.expose_output("result", result).unwrap();
    let trigger = graph.trigger("run", [left, right]).unwrap();
    let report = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(trigger.get(), vec![Value::of(13_i64), Value::of(4_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().last().unwrap().value::<i64>(), Ok(9));
}
