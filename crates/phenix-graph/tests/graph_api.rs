#![cfg(feature = "derive")]
//! External-crate coverage: the manually implementable path must not depend on
//! macro implementation details, private node state or compiler internals.
use phenix_graph::Deployment;
use phenix_graph::ReportRetention;
use phenix_graph::{
    AllSameCohort, AnyOne, BufferMode, ContractValue, Emission, FromSelected, InputSchema,
    InputSlot, InvocationCtx, LayoutError, ModuleCatalog, ModuleDefBuilder, ModuleInputTarget,
    ModuleOutputSource, ModuleTriggerTarget, NodeAction, NodeTemplate, OutputSlot, Port, PortArgs,
    SelectedValues, SlotId, Store, Value, emit,
};
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq)]
struct ManualArgs {
    left: i64,
    right: i64,
}

// This trait can be implemented manually; code generation is optional.
impl InputSchema for ManualArgs {
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        Ok(vec![
            InputSlot::deferred::<i64>(0).named("left"),
            InputSlot::deferred::<i64>(1).named("right"),
        ])
    }
}
impl FromSelected for ManualArgs {
    fn from_values(values: &SelectedValues<'_>) -> Result<Self, String> {
        Ok(Self {
            left: values.get("left")?,
            right: values.get("right")?,
        })
    }
}

#[derive(Debug, PartialEq, Eq, PortArgs)]
struct DerivedArgs {
    #[slot(id = 0, buffer = "deferred")]
    left: i64,
    #[slot(id = 1, buffer = "deferred")]
    right: i64,
}

struct ManualAdd {
    multiplier: i64,
}
impl NodeAction for ManualAdd {
    fn execute(&self, ctx: InvocationCtx) -> Result<Vec<Emission>, String> {
        let args = ctx.prepared::<ManualArgs>()?;
        Ok(vec![emit(0, (args.left + args.right) * self.multiplier)])
    }
}

struct DerivedAdd {
    multiplier: i64,
}
impl NodeAction for DerivedAdd {
    fn execute(&self, ctx: InvocationCtx) -> Result<Vec<Emission>, String> {
        let args = ctx.prepared::<DerivedArgs>()?;
        Ok(vec![emit(0, (args.left + args.right) * self.multiplier)])
    }
}

fn graph(derive: bool, wiring_macro: bool) -> Arc<phenix_graph::CompiledGraph> {
    let source = NodeTemplate::with_action(
        1,
        vec![Port::all(0, vec![InputSlot::immediate::<i64>(0).named("value")]).named("start")],
        vec![OutputSlot::typed::<i64>(0).named("value")],
        |ctx: InvocationCtx| Ok(vec![emit(0, ctx.arg::<i64>(0)?)]),
    )
    .named("source")
    .unwrap();
    // One source value, broadcast to two routes. The right branch applies its
    // own projection, preserving the original x + 2*x join semantics.
    let double = NodeTemplate::new(
        3,
        vec![Port::all(0, vec![InputSlot::immediate::<i64>(0).named("value")]).named("run")],
        vec![OutputSlot::typed::<i64>(0).named("value")],
        |ctx: InvocationCtx| Ok(vec![emit(0, ctx.arg::<i64>(0)? * 2)]),
    )
    .named("double")
    .unwrap();
    let adder = if derive {
        NodeTemplate::with_action(
            2,
            vec![Port::for_layout::<DerivedArgs>(0, "run", AllSameCohort).unwrap()],
            vec![OutputSlot::typed::<i64>(0).named("sum")],
            DerivedAdd { multiplier: 2 },
        )
    } else {
        NodeTemplate::with_action(
            2,
            vec![Port::for_layout::<ManualArgs>(0, "run", AllSameCohort).unwrap()],
            vec![OutputSlot::typed::<i64>(0).named("sum")],
            ManualAdd { multiplier: 2 },
        )
    }
    .named("adder")
    .unwrap();

    // Both named and typed handles are provided directly by ordinary structs.
    let first = source.output_handle::<i64>("value").unwrap();
    let source_to_double = double.input_handle::<i64>("run", "value").unwrap();
    let doubled = double.output_handle::<i64>("value").unwrap();
    let left = adder.input_handle::<i64>("run", "left").unwrap();
    let right = adder.input_handle::<i64>("run", "right").unwrap();
    assert!(adder.input_handle::<String>("run", "left").is_err());
    let root_input = source.input("start", "value").unwrap();
    let root_output = adder.output("sum").unwrap();
    let mut m = ModuleDefBuilder::new("same-graph");
    m.add(source).unwrap();
    m.add(double).unwrap();
    m.add(adder).unwrap();
    if wiring_macro {
        phenix_graph::wire!(m, source.value => adder.run.left).unwrap();
        phenix_graph::wire!(m, source.value => double.run.value).unwrap();
        phenix_graph::wire!(m, double.value => adder.run.right).unwrap();
    } else {
        m.connect_typed(first, left)
            .connect_typed(first, source_to_double)
            .connect_typed(doubled, right);
    }
    m.input(
        0,
        "start",
        i64::SCHEMA,
        vec![ModuleInputTarget::Node(root_input)],
    );
    m.output(0, "sum", i64::SCHEMA, ModuleOutputSource::Node(root_output));
    m.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
    let mut catalog = ModuleCatalog::new();
    catalog.add(m.build()).unwrap();
    let compiled = catalog.compile("same-graph").unwrap();
    match compiled.find("adder") {
        Some(phenix_graph::FindResult::Node(node)) => {
            assert_eq!(node.input("run", "left").unwrap().slot_id(), 0);
            assert_eq!(node.output("sum").unwrap().slot_id(), 0);
        }
        _ => panic!("named compiled node missing"),
    }
    compiled
}

#[test]
fn manual_builder_derive_and_wiring_macro_have_identical_execution() {
    for derive in [false, true] {
        for wiring_macro in [false, true] {
            let engine = Deployment::new(graph(derive, wiring_macro))
                .store(Store::new())
                .report_retention(ReportRetention::RetainPayloads)
                .build()
                .unwrap();
            for value in [0, 3, -5] {
                let report = engine.fire(1, vec![Value::of(value)]).unwrap();
                assert!(report.success(), "{:?}", report.error_messages());
                let result = report
                    .publications()
                    .find(|p| p.source_node_id() == 2)
                    .unwrap()
                    .value::<i64>()
                    .unwrap();
                assert_eq!(result, 6 * value);
            }
        }
    }
}

#[test]
fn closure_decoder_and_struct_literal_port_are_supported_without_derive() {
    let port = Port {
        id: phenix_graph::PortId::new(0),
        name: Some("run".into()),
        slots: ManualArgs::slots().unwrap(),
        policy: Arc::new(AllSameCohort),
        args: None,
    }
    .args::<ManualArgs, _>(|values| {
        Ok(ManualArgs {
            left: values.get("left")?,
            right: values.get("right")?,
        })
    });
    let node = NodeTemplate::with_action(
        1,
        vec![port],
        vec![OutputSlot::typed::<i64>(0).named("sum")],
        ManualAdd { multiplier: 1 },
    )
    .named("adder")
    .unwrap();
    let mut m = ModuleDefBuilder::new("manual");
    m.input(
        0,
        "left",
        i64::SCHEMA,
        vec![ModuleInputTarget::Node(node.input("run", "left").unwrap())],
    );
    m.input(
        1,
        "right",
        i64::SCHEMA,
        vec![ModuleInputTarget::Node(node.input("run", "right").unwrap())],
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
    let report = Deployment::new(catalog.compile("manual").unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(1, vec![Value::of(8_i64), Value::of(4_i64)])
        .unwrap();
    assert!(report.success());
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

#[derive(Debug, PartialEq, Eq, PortArgs)]
struct OptionalArgs {
    #[slot(id = 3)]
    left: Option<i64>,
    #[slot(id = 5, name = "alternative")]
    right: Option<i64>,
}

#[test]
fn derived_optional_fields_respect_selected_deliveries() {
    let run = Port::for_layout::<OptionalArgs>(0, "run", AnyOne).unwrap();
    let node = NodeTemplate::with_action(1, vec![run], vec![], |ctx: InvocationCtx| {
        let args = ctx.prepared::<OptionalArgs>()?;
        assert_eq!(
            *args,
            OptionalArgs {
                left: None,
                right: Some(17)
            }
        );
        Ok(vec![])
    });
    let mut m = ModuleDefBuilder::new("optional");
    m.input(
        0,
        "event",
        i64::SCHEMA,
        vec![ModuleInputTarget::Node(phenix_graph::input(1, 0, 5))],
    );
    m.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
    m.add(node).unwrap();
    let mut c = ModuleCatalog::new();
    c.add(m.build()).unwrap();
    let report = Deployment::new(c.compile("optional").unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(1, vec![Value::of(17_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.invocation_count(1), 1);
}

#[derive(Debug, Clone, PartialEq, Eq, ContractValue)]
#[contract(schema = "phenix.poc.record@1")]
struct Record {
    count: i64,
    note: Option<String>,
}

#[test]
fn derived_contract_codec_roundtrips_and_rejects_malformed_bytes() {
    for record in [
        Record {
            count: 12,
            note: Some("hello".into()),
        },
        Record {
            count: 0,
            note: Some(String::new()),
        },
        Record {
            count: -1,
            note: None,
        },
    ] {
        let encoded = Value::of(record.clone());
        assert_eq!(encoded.get::<Record>().unwrap(), record);
        let reconstructed = Value::from_wire_parts(encoded.schema(), encoded.bytes().to_vec());
        assert_eq!(reconstructed.get::<Record>().unwrap(), record);
        assert!(
            Value::from_wire_parts("not.the.record", encoded.bytes().to_vec())
                .get::<Record>()
                .is_err()
        );
        let bytes = record.encode();
        assert!(Record::decode(&bytes[..bytes.len() - 1]).is_err());
        let mut trailing = bytes;
        trailing.push(1);
        assert!(Record::decode(&trailing).is_err());
    }
}

#[test]
fn compiler_rejects_duplicate_names_but_unambiguous_literal_definitions_work() {
    let node = NodeTemplate::with_action(
        1,
        vec![
            Port::all(
                0,
                vec![
                    InputSlot::deferred::<i64>(0).named("same"),
                    InputSlot::deferred::<i64>(1).named("same"),
                ],
            )
            .named("run"),
        ],
        vec![OutputSlot::typed::<()>(0)],
        |_ctx| Ok(vec![emit(0, ())]),
    );
    let mut m = ModuleDefBuilder::new("bad-names");
    m.add(node).unwrap();
    let mut c = ModuleCatalog::new();
    c.add(m.build()).unwrap();
    let failure = match c.compile("bad-names") {
        Ok(_) => panic!("expected compile failure"),
        Err(e) => e,
    };
    assert!(
        failure.message.contains("duplicate input slot name"),
        "{failure}"
    );

    // Public low-level construction does not require either derive or builder
    // methods. The compiler is still the single validation authority.
    let raw_slot = InputSlot {
        id: SlotId::new(0),
        name: Some("request".into()),
        schema: i64::SCHEMA,
        local_type: None,
        buffer: BufferMode::Deferred,
    };
    assert_eq!(raw_slot.name.as_deref(), Some("request"));
}

#[test]
fn named_module_boundaries_are_wired_without_numeric_ports() {
    use phenix_graph::{ModuleInstance, ModuleOutputSource};
    let child_node = NodeTemplate::with_action(
        1,
        vec![Port::all(0, vec![InputSlot::immediate::<i64>(0).named("number")]).named("run")],
        vec![OutputSlot::typed::<i64>(0).named("value")],
        |ctx: InvocationCtx| Ok(vec![emit(0, ctx.arg::<i64>(0)? * 2)]),
    )
    .named("double")
    .unwrap();
    let mut child = ModuleDefBuilder::new("child");
    child.input(
        0,
        "incoming",
        i64::SCHEMA,
        vec![ModuleInputTarget::Node(
            child_node.input("run", "number").unwrap(),
        )],
    );
    child.output(
        0,
        "response",
        i64::SCHEMA,
        ModuleOutputSource::Node(child_node.output("value").unwrap()),
    );
    child.add(child_node).unwrap();

    let root_node = NodeTemplate::with_action(
        1,
        vec![Port::all(0, vec![InputSlot::immediate::<i64>(0).named("value")]).named("run")],
        vec![OutputSlot::typed::<i64>(0).named("value")],
        |ctx: InvocationCtx| Ok(vec![emit(0, ctx.arg::<i64>(0)?)]),
    )
    .named("source")
    .unwrap();
    let mut root = ModuleDefBuilder::new("root");
    root.input(
        0,
        "start",
        i64::SCHEMA,
        vec![ModuleInputTarget::Node(
            root_node.input("run", "value").unwrap(),
        )],
    );
    root.output(
        0,
        "result",
        i64::SCHEMA,
        ModuleOutputSource::InstanceNamed {
            name: "worker".into(),
            port: "response".into(),
        },
    );
    root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
    root.add(root_node).unwrap();
    root.add(ModuleInstance::new("worker", "child").unwrap())
        .unwrap();
    phenix_graph::wire!(root, source.value => worker.incoming).unwrap();
    assert!(phenix_graph::wire!(root, source.missing => worker.incoming).is_err());

    let mut catalog = ModuleCatalog::new();
    catalog.add(child.build()).unwrap();
    catalog.add(root.build()).unwrap();
    let graph = catalog.compile("root").unwrap();
    assert!(matches!(
        graph.find("worker/double"),
        Some(phenix_graph::FindResult::Node(_))
    ));
    let report = Deployment::new(graph)
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(1, vec![Value::of(9_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert!(report.publications().any(|p| p.value::<i64>() == Ok(18)));
}
