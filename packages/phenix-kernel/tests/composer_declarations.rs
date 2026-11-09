use phenix_graph::{DeclaredValue, GraphContract, InterfaceShape, LocalExecutor, Value};
use phenix_kernel::{
    BuildOptions, ContractDefinition, Contribution, ImportPolicy, Kernel, Key, ResolutionError,
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::{Arc, Mutex};

fn service_contract() -> GraphContract {
    GraphContract::new(
        "test/two-offers@1",
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::local::<i64>())
            .unwrap()
            .output("value", DeclaredValue::local::<i64>())
            .unwrap(),
    )
    .unwrap()
}

struct Spec;
impl PluginInstance for Spec {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        into.add(ContractDefinition {
            id: "model".into(),
            contract: service_contract(),
        })
    }
}

struct Provider;
impl PluginInstance for Provider {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        Contribution::new("serve", |cx| {
            cx.graph.node("primary", |value: i64| value + 1)?;
            cx.graph.node("backup", |value: i64| value + 100)?;
            cx.graph.implements("primary", &service_contract())?;
            cx.graph.implements("backup", &service_contract())?;
            Ok(())
        })
        .offer("primary", "primary", Key::new("spec", "model"))
        .offer("backup", "backup", Key::new("spec", "model"))
        .register(into)
    }
}

struct Consumer(Arc<Mutex<Vec<i64>>>);
impl PluginInstance for Consumer {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let results = self.0.clone();
        Contribution::new("client", move |cx| {
            let chosen = cx.import("model")?;
            let bound = chosen.bind(cx.graph)?;
            let input = bound.local_input::<i64>("run", "value")?;
            let output = bound.local_output::<i64>("value")?;
            let results = results.clone();
            let sink = cx.graph.node("sink", move |value: i64| {
                results.lock().unwrap().push(value);
                value
            })?;
            cx.graph.connect(output, sink.inlet())?;
            let public = cx.graph.expose_input("request", input)?;
            cx.graph.trigger("run", [public])?;
            Ok(())
        })
        .import("model", Key::new("spec", "model"))
        .register(into)
    }
}

#[test]
fn two_offers_from_one_composer_are_selectable_and_change_execution() {
    let mut catalog = PluginCatalog::new();
    let spec = catalog.register("spec", |_: ()| Ok(Spec)).unwrap();
    let provider = catalog.register("provider", |_: ()| Ok(Provider)).unwrap();
    let output = Arc::new(Mutex::new(Vec::new()));
    let captured = output.clone();
    let consumer = catalog
        .register("consumer", move |_: ()| Ok(Consumer(captured.clone())))
        .unwrap();
    let mut plugins = catalog.instance();
    plugins.load(&spec, ()).unwrap();
    plugins.load(&provider, ()).unwrap();
    plugins.load(&consumer, ()).unwrap();
    assert!(matches!(
        Kernel::build("host", plugins.snapshots(), Arc::new(LocalExecutor)),
        Err(ResolutionError::Ambiguous { .. })
    ));
    for (offer, expected) in [("primary", 42), ("backup", 141)] {
        let mut policy = ImportPolicy::default();
        policy.selections.insert(
            Key::new("consumer", "model"),
            Key::new("provider", offer),
        );
        let mut kernel = Kernel::build_with_options(
            "host",
            plugins.snapshots(),
            Arc::new(LocalExecutor),
            BuildOptions {
                imports: policy,
                ..Default::default()
            },
        )
        .unwrap();
        kernel.start().unwrap();
        assert!(kernel.fire("run", vec![Value::local(41_i64)]).unwrap().success());
        assert_eq!(output.lock().unwrap().pop(), Some(expected));
    }
}

#[test]
fn grouped_registration_is_failure_atomic_across_composer_and_metadata() {
    let mut registrar = Registrar::new("plugin").unwrap();
    Contribution::new("original", |_| Ok(()))
        .offer("occupied", "service", Key::new("spec", "model"))
        .register(&mut registrar)
        .unwrap();
    let result = Contribution::new("candidate", |_| Ok(()))
        .offer("fresh", "service", Key::new("spec", "model"))
        .offer("occupied", "service", Key::new("spec", "model"))
        .register(&mut registrar);
    assert!(result.is_err());
    // A failed group must not reserve its composer name or its first offer.
    Contribution::new("candidate", |_| Ok(()))
        .offer("fresh", "service", Key::new("spec", "model"))
        .register(&mut registrar)
        .unwrap();
    assert_eq!(registrar.finish().keys().count(), 4);
}

#[test]
fn duplicate_imports_across_composers_are_rejected_without_partial_insert() {
    let mut registrar = Registrar::new("plugin").unwrap();
    Contribution::new("a", |_| Ok(()))
        .import("shared", Key::new("spec", "model"))
        .register(&mut registrar)
        .unwrap();
    assert!(
        Contribution::new("b", |_| Ok(()))
            .offer("new", "unit", Key::new("spec", "model"))
            .import("shared", Key::new("spec", "model"))
            .register(&mut registrar)
            .is_err()
    );
    Contribution::new("b", |_| Ok(())).register(&mut registrar).unwrap();
    assert_eq!(registrar.finish().keys().count(), 3);
}

#[test]
fn duplicate_imports_and_offers_within_one_composer_fail_atomically() {
    for duplicate_offer in [true, false] {
        let mut registrar = Registrar::new("plugin").unwrap();
        let candidate = Contribution::new("candidate", |_| Ok(()));
        let result = if duplicate_offer {
            candidate
                .offer("x", "unit", Key::new("spec", "model"))
                .offer("x", "unit", Key::new("spec", "model"))
                .register(&mut registrar)
        } else {
            candidate
                .import("x", Key::new("spec", "model"))
                .import("x", Key::new("spec", "model"))
                .register(&mut registrar)
        };
        assert!(result.is_err());
        assert_eq!(registrar.finish().keys().count(), 0);
    }
}
