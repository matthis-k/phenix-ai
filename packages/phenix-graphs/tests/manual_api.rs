//! Runs with `cargo test --no-default-features`, without proc macros or any
//! dependency other than the standard library and the graph itself.
use phenix_graph::Deployment;
use phenix_graph::ReportRetention;
use phenix_graph::{
    AllSameCohort, ContractValue, DecodedInputLayout, FromSelected, InputLayout, InputSchema,
    InputSlot, InvocationCtx, LayoutError, ModuleCatalog, ModuleDefBuilder, ModuleInputTarget,
    ModuleTriggerTarget, NodeTemplate, OutputSlot, Port, SelectedValues, Store, Value, emit, input,
};

struct Values {
    number: i64,
}
impl InputSchema for Values {
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        Ok(vec![InputSlot::immediate::<i64>(0).named("number")])
    }
}

struct RenamedValue;
impl InputLayout for RenamedValue {
    type Value = i64;
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        Ok(vec![InputSlot::deferred::<i64>(0).named("custom.path")])
    }
}
impl DecodedInputLayout for RenamedValue {
    fn prepare(values: &SelectedValues<'_>) -> Result<Self::Value, String> {
        values.get_path("custom.path")
    }
}
impl FromSelected for Values {
    fn from_values(values: &SelectedValues<'_>) -> Result<Self, String> {
        Ok(Self {
            number: values.get("number")?,
        })
    }
}

#[test]
fn std_only_manual_graph_compiles_and_executes() {
    let node = NodeTemplate::with_action(
        1,
        vec![Port::for_layout::<Values>(0, "run", AllSameCohort).unwrap()],
        vec![OutputSlot::typed::<i64>(0).named("value")],
        |ctx: InvocationCtx| {
            let args = ctx.prepared::<Values>()?;
            Ok(vec![emit(0, args.number + 1)])
        },
    )
    .named("increment")
    .unwrap();
    let mut builder = ModuleDefBuilder::new("manual-only");
    let addr = node.input("run", "number").unwrap();
    assert_eq!(addr.slot_id(), 0);
    builder.add(node).unwrap();
    builder.input(
        0,
        "request",
        i64::wire_schema(),
        vec![ModuleInputTarget::Node(addr)],
    );
    builder.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
    let mut catalog = ModuleCatalog::new();
    catalog.add(builder.build()).unwrap();
    let engine = Deployment::new(catalog.compile("manual-only").unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap();
    let report = engine.fire(1, vec![Value::of(42_i64)]).unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report
            .publications()
            .next()
            .unwrap()
            .value::<i64>()
            .unwrap(),
        43
    );
}

#[test]
fn manual_input_layout_uses_the_same_erased_runtime_path_without_macros() {
    let node =
        NodeTemplate::with_input_layout::<RenamedValue, i64, _>("run", AllSameCohort, |value| {
            value + 5
        })
        .unwrap()
        .named("custom-layout")
        .unwrap();
    let mut module = ModuleDefBuilder::new("manual-layout");
    let node = module.register("custom-layout", node).unwrap();
    module
        .export_input("request", vec![node.input::<i64>("run", "custom.path")])
        .unwrap();
    module
        .export_output("result", node.output::<i64>("value"))
        .unwrap();
    module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
    let mut catalog = ModuleCatalog::new();
    catalog.add(module.build()).unwrap();
    let report = Deployment::new(catalog.compile("manual-layout").unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(1, vec![Value::of(7_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report
            .publications()
            .next()
            .unwrap()
            .value::<i64>()
            .unwrap(),
        12
    );
}

#[test]
fn wrong_named_input_type_fails_before_execution() {
    let node = NodeTemplate::with_action(
        1,
        vec![Port::for_layout::<Values>(0, "run", AllSameCohort).unwrap()],
        vec![],
        |_ctx: InvocationCtx| Ok(vec![]),
    );
    assert!(node.input_handle::<String>("run", "number").is_err());
    assert!(node.input("run", "unknown").is_err());
    assert!(node.input("unknown", "number").is_err());
    assert_eq!(input(1, 0, 0), node.input("run", "number").unwrap());
}

#[test]
fn direct_module_struct_literal_compiles_without_builder() {
    use phenix_graph::{
        ModuleDef, ModuleEntry, ModuleInput, ModuleOutput, ModuleOutputSource, ModuleTrigger,
        TriggerId, output,
    };
    let node = NodeTemplate::with_action(
        1,
        vec![Port::for_layout::<Values>(0, "run", AllSameCohort).unwrap()],
        vec![OutputSlot::typed::<i64>(0).named("value")],
        |ctx: InvocationCtx| Ok(vec![emit(0, ctx.prepared::<Values>()?.number * 2)]),
    )
    .named("double")
    .unwrap();
    let entry = ModuleEntry {
        definition: ModuleDef {
            declaration_ids: Default::default(),
            claims: vec![],
            contract_requests: vec![],
            id: "raw-def".to_owned(),
            nodes: vec![node],
            instances: vec![],
            edges: vec![],
            inputs: vec![ModuleInput {
                id: 0,
                name: "request".into(),
                port: "request".into(),
                slot: "value".into(),
                schema: i64::wire_schema(),
                local_type: None,
                targets: vec![ModuleInputTarget::Node(input(1, 0, 0))],
            }],
            outputs: vec![ModuleOutput {
                id: 0,
                name: "result".into(),
                schema: i64::wire_schema(),
                local_type: None,
                source: ModuleOutputSource::Node(output(1, 0)),
            }],
        },
        triggers: vec![ModuleTrigger {
            id: TriggerId::new(1),
            name: "1".into(),
            targets: vec![ModuleTriggerTarget::PublicInput(0)],
        }],
    };
    let mut catalog = ModuleCatalog::new();
    catalog.add(entry).unwrap();
    let report = Deployment::new(catalog.compile("raw-def").unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(1, vec![Value::of(7_i64)])
        .unwrap();
    assert!(report.success());
    assert_eq!(
        report
            .publications()
            .next()
            .unwrap()
            .value::<i64>()
            .unwrap(),
        14
    );
}

#[test]
fn failed_typed_decoder_is_atomic_and_another_wave_can_succeed() {
    use phenix_graph::{AnyOne, WaveError};
    let port = Port::new(
        0,
        vec![InputSlot::immediate::<i64>(0).named("value")],
        AnyOne,
    )
    .named("run")
    .args::<i64, _>(|selected| {
        let value = selected.get::<i64>("value")?;
        if value < 0 {
            Err("negative value rejected".into())
        } else {
            Ok(value)
        }
    });
    let node = NodeTemplate::with_action(
        1,
        vec![port],
        vec![OutputSlot::typed::<i64>(0)],
        |ctx: InvocationCtx| Ok(vec![emit(0, *ctx.prepared::<i64>()?)]),
    );
    let mut m = ModuleDefBuilder::new("atomic-decode");
    m.input(
        0,
        "request",
        i64::wire_schema(),
        vec![ModuleInputTarget::Node(input(1, 0, 0))],
    );
    m.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
    m.add(node).unwrap();
    let mut c = ModuleCatalog::new();
    c.add(m.build()).unwrap();
    let e = Deployment::new(c.compile("atomic-decode").unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap();

    let rejected = e.fire(1, vec![Value::of(-1_i64)]).unwrap();
    assert_eq!(rejected.invocation_count(1), 0);
    assert_eq!(rejected.publications().count(), 0);
    assert!(
        matches!(rejected.errors(), [WaveError::InvalidArgs(_, _, _, reason)]
        if reason == "negative value rejected")
    );

    let accepted = e.fire(1, vec![Value::of(7_i64)]).unwrap();
    assert!(accepted.success(), "{:?}", accepted.error_messages());
    assert_eq!(
        accepted
            .publications()
            .next()
            .unwrap()
            .value::<i64>()
            .unwrap(),
        7
    );
}

#[test]
fn custom_policy_can_select_by_declared_input_name() {
    use phenix_graph::{Candidate, IncompleteGroup, Pick, PortPolicy, PortSnapshot, Selection};
    struct SelectNamed {
        name: &'static str,
    }
    impl PortPolicy for SelectNamed {
        fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
            let candidate: &Candidate = snapshot.slot_named(self.name)?.candidates().first()?;
            Some(Selection::new(
                candidate.cohort_id(),
                vec![Pick::new(candidate.slot_id(), candidate.delivery_id())],
            ))
        }
        fn incomplete(&self, _: &PortSnapshot) -> Vec<IncompleteGroup> {
            vec![]
        }
    }
    let port = Port::new(
        0,
        vec![
            InputSlot::deferred::<i64>(0).named("left"),
            InputSlot::deferred::<i64>(1).named("right"),
        ],
        SelectNamed { name: "right" },
    )
    .named("run")
    .args::<i64, _>(|selected| selected.get("right"));
    let node = NodeTemplate::with_action(
        1,
        vec![port],
        vec![OutputSlot::typed::<i64>(0)],
        |ctx: InvocationCtx| Ok(vec![emit(0, *ctx.prepared::<i64>()?)]),
    );
    let mut m = ModuleDefBuilder::new("named-policy");
    m.input(
        0,
        "left",
        i64::wire_schema(),
        vec![ModuleInputTarget::Node(input(1, 0, 0))],
    );
    m.input(
        1,
        "right",
        i64::wire_schema(),
        vec![ModuleInputTarget::Node(input(1, 0, 1))],
    );
    m.trigger(
        1,
        vec![
            ModuleTriggerTarget::PublicInput(0),
            ModuleTriggerTarget::PublicInput(1),
        ],
    );
    m.add(node).unwrap();
    let mut catalog = ModuleCatalog::new();
    catalog.add(m.build()).unwrap();
    let report = Deployment::new(catalog.compile("named-policy").unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(1, vec![Value::of(1_i64), Value::of(10_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.invocation_count(1), 1);
    assert_eq!(
        report
            .publications()
            .next()
            .unwrap()
            .value::<i64>()
            .unwrap(),
        10
    );
}
