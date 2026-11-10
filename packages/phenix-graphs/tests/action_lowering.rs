//! Manual layouts keep these lowering probes independent of the derive feature.
use phenix_graph::{
    AllSameCohort, ContractValue, DecodedInputLayout, Deployment, Emission, FromSelected,
    InputLayout, InputSchema, InputSlot, IntoEmission, InvocationCtx, LayoutError, ModuleCatalog,
    ModuleDefBuilder, ModuleInputTarget, ModuleTriggerTarget, NodeTemplate, OutputSchema,
    OutputSlot, ReportRetention, SelectedValues, Store, Value, emit,
};
use std::cell::Cell;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct SendOnlyLayout;
impl InputLayout for SendOnlyLayout {
    type Value = Cell<i64>;
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        Ok(vec![InputSlot::deferred::<i64>(0).named("custom.value")])
    }
}
impl DecodedInputLayout for SendOnlyLayout {
    fn prepare(values: &SelectedValues<'_>) -> Result<Self::Value, String> {
        let value = values.get_path::<i64>("custom.value")?;
        if value < 0 {
            return Err("negative input rejected before admission".into());
        }
        Ok(Cell::new(value))
    }
}

struct Scalar(i64);
impl OutputSchema for Scalar {
    fn output_slot() -> OutputSlot {
        OutputSlot::typed::<i64>(0).named("value")
    }
}
impl IntoEmission for Scalar {
    fn into_emission(self) -> Emission {
        emit(0, self.0)
    }
}

#[test]
fn send_only_prepared_argument_preserves_custom_context_and_grants() {
    let calls = Arc::new(AtomicUsize::new(0));
    let handler_calls = Arc::clone(&calls);
    let node = NodeTemplate::with_layout::<SendOnlyLayout, Scalar, _>(
        "custom",
        AllSameCohort,
        move |value, ctx| {
            handler_calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(ctx.source_port_id(), Some(0));
            assert!(ctx.prepared::<Cell<i64>>().is_err());
            assert!(!ctx.has_arg(0));
            // Metadata and resource authority remain usable after mutable extraction.
            let _metadata = (ctx.wave_id(), ctx.invocation_id(), ctx.cohort_id());
            assert!(ctx.read::<i64, _>("denied", |n| *n).is_err());
            ctx.write::<i64, _>("total", |total| *total += value.get())?;
            Ok(Scalar(ctx.read::<i64, _>("total", |total| *total)?))
        },
    )
    .unwrap()
    .writes("total");
    let mut module = ModuleDefBuilder::new("send-only");
    module.register("custom", node).unwrap();
    module.input(
        0,
        "request",
        i64::wire_schema(),
        vec![ModuleInputTarget::NodeNamed {
            node: "custom".into(),
            port: "custom".into(),
            slot: "custom.value".into(),
        }],
    );
    module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
    let mut catalog = ModuleCatalog::new();
    catalog.add(module.build()).unwrap();
    let mut store = Store::new();
    store.insert("total", 10_i64);
    let engine = Deployment::new(catalog.compile("send-only").unwrap())
        .store(store)
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap();
    let rejected = engine.fire(1, vec![Value::of(-1_i64)]).unwrap();
    assert!(!rejected.success());
    assert_eq!(rejected.publication_events().len(), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for expected in [17, 24] {
        let report = engine.fire(1, vec![Value::of(7_i64)]).unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        assert_eq!(
            report.publications().next().unwrap().value::<i64>(),
            Ok(expected)
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

struct Args(i64);
impl InputSchema for Args {
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        Ok(vec![InputSlot::deferred::<i64>(0).named("value")])
    }
}
impl FromSelected for Args {
    fn from_values(values: &SelectedValues<'_>) -> Result<Self, String> {
        Ok(Self(values.get_path("value")?))
    }
}

#[test]
fn typed_factory_is_fresh_for_two_physical_nodes_and_two_engines() {
    let creations = Arc::new(AtomicUsize::new(0));
    let factory_creations = Arc::clone(&creations);
    let template = NodeTemplate::typed_factory("run", AllSameCohort, move || {
        factory_creations.fetch_add(1, Ordering::SeqCst);
        let count = AtomicUsize::new(0);
        move |args: Args, _: &InvocationCtx| {
            Ok(Scalar(args.0 + count.fetch_add(1, Ordering::SeqCst) as i64))
        }
    })
    .unwrap();
    assert_eq!(creations.load(Ordering::SeqCst), 0);
    let mut module = ModuleDefBuilder::new("factory-lowering");
    module.register("left", template.clone()).unwrap();
    module.register("right", template).unwrap();
    module.input(
        0,
        "request",
        i64::wire_schema(),
        ["left", "right"]
            .map(|name| ModuleInputTarget::NodeNamed {
                node: name.into(),
                port: "run".into(),
                slot: "value".into(),
            })
            .to_vec(),
    );
    module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
    let mut catalog = ModuleCatalog::new();
    catalog.add(module.build()).unwrap();
    let graph = catalog.compile("factory-lowering").unwrap();
    let first = Deployment::new(Arc::clone(&graph))
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap();
    let second = Deployment::new(graph)
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap();
    assert_eq!(creations.load(Ordering::SeqCst), 4);
    for (engine, expected) in [(&first, 10), (&first, 11), (&second, 10), (&second, 11)] {
        let report = engine.fire(1, vec![Value::of(10_i64)]).unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        let values: Vec<_> = report
            .publications()
            .map(|p| p.value::<i64>().unwrap())
            .collect();
        assert_eq!(values, vec![expected, expected]);
    }
}
