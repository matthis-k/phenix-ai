use phenix_graph::{DeclaredValue, GraphContract, InterfaceShape, LocalExecutor, Node, Value};
use phenix_kernel::{
    ContractDefinition, ContractRequirement, Contribution, GraphOffer, ImportPolicy, Kernel, Key,
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

// Each expansion defines a distinct Rust type and its own portable codec.
macro_rules! portable {
    ($name:ident, $schema:literal) => {
        pub struct $name(pub i64);
        impl phenix_graph::ContractValue for $name {
            fn schema() -> phenix_graph::Schema {
                phenix_graph::Schema {
                    name: $schema.into(),
                    revision: 1,
                    structure: "record{value:i64}".into(),
                    codec: "i64-le-v1".into(),
                }
            }
            fn encode(&self) -> Vec<u8> {
                self.0.to_le_bytes().to_vec()
            }
            fn decode(bytes: &[u8]) -> Result<Self, String> {
                Ok(Self(i64::from_le_bytes(
                    bytes.try_into().map_err(|_| "bad i64 length")?,
                )))
            }
        }
    };
}

mod definition {
    use super::*;
    portable!(Request, "example/request");
    portable!(Response, "example/response");
    pub fn contract() -> GraphContract {
        GraphContract::new(
            "example/transform@1",
            InterfaceShape::new()
                .input_slot("run", "value", DeclaredValue::wire::<Request>())
                .unwrap()
                .output("value", DeclaredValue::wire::<Response>())
                .unwrap(),
        )
        .unwrap()
    }
    pub struct Plugin;
    impl PluginInstance for Plugin {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            into.add(ContractDefinition {
                id: "transform".into(),
                contract: contract(),
            })
        }
    }
}

mod provider {
    use super::*;
    portable!(Request, "example/request");
    portable!(Response, "example/response");
    pub struct Plugin {
        pub increment: i64,
        pub claim: bool,
        pub path: String,
        pub drops: Arc<AtomicUsize>,
        pub activations: Arc<AtomicUsize>,
    }
    impl Drop for Plugin {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl PluginInstance for Plugin {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            into.add(GraphOffer {
                id: "service".into(),
                producer: Key::new(into.owner(), "main"),
                unit: self.path.clone(),
                contract: Key::new("definition", "transform"),
            })?;
            let increment = self.increment;
            let claim = self.claim;
            let activations = self.activations.clone();
            let path = if self.path == "does-not-exist" {
                "service".into()
            } else {
                self.path.clone()
            };
            Contribution::new("main", move |context| {
                    let graph = &mut *context.graph;
                    graph.node(
                        &path,
                        Node::wire(move |request: Request| Response(request.0 + increment)),
                    )?;
                    if claim {
                        graph.implements(&path, &definition::contract())?;
                    }
                    let activations = activations.clone();
                    context.activate(move || {
                        activations.fetch_add(1, Ordering::SeqCst);
                        Ok(Box::new(Idle))
                    })
                })
.register(into)
        }
    }
    struct Idle;
    impl phenix_kernel::Producer for Idle {
        fn poll(&mut self) -> Result<Option<phenix_kernel::Notice>, phenix_kernel::CallbackError> {
            Ok(None)
        }
    }
}

mod consumer {
    use super::*;
    portable!(Request, "example/request");
    portable!(Response, "example/response");
    pub struct Plugin {
        pub observed: Arc<Mutex<Vec<i64>>>,
    }
    impl PluginInstance for Plugin {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            into.add(ContractRequirement {
                id: "transform".into(),
                consumer: Key::new("consumer", "main"),
                contract: Key::new("definition", "transform"),
            })?;
            let observed = self.observed.clone();
            Contribution::new("main", move |context| {
                    let selected = context.import("transform")?;
                    let graph = &mut *context.graph;
                    let source = graph.node("source", Node::wire(|request: Request| request))?;
                    let sink = graph.node(
                        "sink",
                        Node::wire({
                            let observed = observed.clone();
                            move |response: Response| {
                                observed.lock().unwrap().push(response.0);
                                response
                            }
                        }),
                    )?;
                    // Lookup uses A's contract types, never B's implementation types.
                    // C's independently defined types cross both portable edges.
                    let (input, output) = {
                        let bound = selected.bind(graph)?;
                        (
                            bound.wire_input::<definition::Request>("run", "value")?,
                            bound.wire_output::<definition::Response>("value")?,
                        )
                    };
                    graph.connect_wire(source.output(), input)?;
                    graph.connect_wire(output, sink.inlet())?;
                    let input = graph.expose_input("request", source.inlet())?;
                    graph.trigger("run", [input])?;
                    Ok(())
                })
.register(into)
        }
    }
}

fn provider(
    increment: i64,
    claim: bool,
    drops: &Arc<AtomicUsize>,
    activations: &Arc<AtomicUsize>,
) -> provider::Plugin {
    provider::Plugin {
        increment,
        claim,
        path: "service".into(),
        drops: drops.clone(),
        activations: activations.clone(),
    }
}

#[test]
fn three_plugins_execute_portable_contracts_and_pin_old_generations() {
    let mut catalog = PluginCatalog::new();
    let a = catalog
        .register("definition", |_: ()| Ok(definition::Plugin))
        .unwrap();
    let b = catalog
        .register("provider", |plugin: provider::Plugin| Ok(plugin))
        .unwrap();
    let c = catalog
        .register("consumer", |plugin: consumer::Plugin| Ok(plugin))
        .unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    let activations = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(Mutex::new(Vec::new()));
    let mut plugins = catalog.instance();
    plugins.load(&a, ()).unwrap();
    let original = plugins
        .load(&b, provider(10, true, &drops, &activations))
        .unwrap();
    plugins
        .load(
            &c,
            consumer::Plugin {
                observed: observed.clone(),
            },
        )
        .unwrap();
    let mut old = Kernel::build("old", plugins.snapshots(), Arc::new(LocalExecutor)).unwrap();
    assert_eq!(activations.load(Ordering::SeqCst), 0);
    old.start().unwrap();
    assert!(
        old.fire("run", vec![Value::of(consumer::Request(2))])
            .unwrap()
            .success()
    );
    assert_eq!(*observed.lock().unwrap(), [12]);

    // A real candidate build fails before deployment/producer activation.
    let staged = plugins
        .prepare_reload(&b, provider(100, false, &drops, &activations))
        .unwrap();
    let candidate = staged.candidate().unwrap();
    let snapshots = plugins
        .snapshots()
        .filter(|s| s.name() != "provider")
        .chain([candidate]);
    let error = Kernel::build("rejected", snapshots, Arc::new(LocalExecutor))
        .err()
        .unwrap();
    assert!(
        error.to_string().contains("service") && error.to_string().contains("claim"),
        "{error}"
    );
    assert_eq!(activations.load(Ordering::SeqCst), 1);
    assert_eq!(
        plugins.snapshot(&b).unwrap().revision(),
        original.revision()
    );
    drop(staged);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(
        old.fire("run", vec![Value::of(consumer::Request(3))])
            .unwrap()
            .success()
    );

    // Build a valid candidate before committing. This is host sequencing, not
    // an atomic plugin/kernel coordinator or an automatic generation redirect.
    let mut renamed = provider(20, true, &drops, &activations);
    renamed.path = "renamed-service".into();
    let staged = plugins.prepare_reload(&b, renamed).unwrap();
    let candidate = staged.candidate().unwrap();
    let mut new = Kernel::build(
        "new",
        plugins
            .snapshots()
            .filter(|s| s.name() != "provider")
            .chain([candidate]),
        Arc::new(LocalExecutor),
    )
    .unwrap();
    plugins.commit(staged).unwrap();
    plugins.unload(&c).unwrap();
    plugins.unload(&b).unwrap();
    plugins.unload(&a).unwrap();
    drop(original);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    new.start().unwrap();
    assert!(
        new.fire("run", vec![Value::of(consumer::Request(4))])
            .unwrap()
            .success()
    );
    assert!(
        old.fire("run", vec![Value::of(consumer::Request(5))])
            .unwrap()
            .success()
    );
    assert_eq!(*observed.lock().unwrap(), [12, 13, 24, 15]);
    drop(old);
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    drop(new);
    assert_eq!(drops.load(Ordering::SeqCst), 3);
}

#[test]
fn missing_paths_and_definitions_fail_closed() {
    let mut catalog = PluginCatalog::new();
    let a = catalog
        .register("definition", |_: ()| Ok(definition::Plugin))
        .unwrap();
    let b = catalog
        .register("provider", |plugin: provider::Plugin| Ok(plugin))
        .unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    let activations = Arc::new(AtomicUsize::new(0));
    let mut plugins = catalog.instance();
    let mut missing = provider(1, true, &drops, &activations);
    missing.path = "does-not-exist".into();
    plugins.load(&b, missing).unwrap();
    let error = Kernel::build("host", plugins.snapshots(), Arc::new(LocalExecutor))
        .err()
        .unwrap();
    assert!(
        error.to_string().contains("missing contract definition"),
        "{error}"
    );
    plugins.load(&a, ()).unwrap();
    let error = Kernel::build("host", plugins.snapshots(), Arc::new(LocalExecutor))
        .err()
        .unwrap();
    assert!(error.to_string().contains("does-not-exist"), "{error}");
    assert_eq!(activations.load(Ordering::SeqCst), 0);
}

struct RequirementPlugin {
    contract: GraphContract,
}
impl PluginInstance for RequirementPlugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        into.add(ContractDefinition {
            id: "requested".into(),
            contract: self.contract.clone(),
        })?;
        into.add(ContractRequirement {
            id: "request".into(),
            consumer: Key::new(into.owner(), "main"),
            contract: Key::new(into.owner(), "requested"),
        })?;
        Contribution::new("main", |_| Ok(()))
.register(into)
    }
}

#[test]
fn selection_and_definition_mismatches_are_not_structural_fallbacks() {
    let mut catalog = PluginCatalog::new();
    let a = catalog
        .register("definition", |_: ()| Ok(definition::Plugin))
        .unwrap();
    let b = catalog
        .register("provider", |plugin: provider::Plugin| Ok(plugin))
        .unwrap();
    let c = catalog
        .register("consumer", |plugin: RequirementPlugin| Ok(plugin))
        .unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    let activations = Arc::new(AtomicUsize::new(0));
    let mut plugins = catalog.instance();
    plugins.load(&a, ()).unwrap();
    plugins
        .load(&b, provider(1, true, &drops, &activations))
        .unwrap();
    plugins
        .load(
            &c,
            RequirementPlugin {
                contract: definition::contract(),
            },
        )
        .unwrap();
    let mut policy = ImportPolicy::default();
    policy.selections.insert(
        Key::new("consumer", "request"),
        Key::new("provider", "absent"),
    );
    let error = Kernel::build_with_options(
        "host",
        plugins.snapshots(),
        Arc::new(LocalExecutor),
        phenix_kernel::BuildOptions {
            imports: policy,
            ..Default::default()
        },
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("unknown offer"), "{error}");

    let unrelated = GraphContract::new(
        "example/unrelated@1",
        definition::contract().required().clone(),
    )
    .unwrap();
    plugins
        .reload(
            &c,
            RequirementPlugin {
                contract: unrelated,
            },
        )
        .unwrap();
    let error = Kernel::build("host", plugins.snapshots(), Arc::new(LocalExecutor))
        .err()
        .unwrap();
    assert!(
        error.to_string().contains("missing eligible offer"),
        "{error}"
    );

    let conflicting =
        GraphContract::new(definition::contract().id(), InterfaceShape::new()).unwrap();
    plugins
        .reload(
            &c,
            RequirementPlugin {
                contract: conflicting,
            },
        )
        .unwrap();
    let error = Kernel::build("host", plugins.snapshots(), Arc::new(LocalExecutor))
        .err()
        .unwrap();
    assert!(
        error
            .to_string()
            .contains("conflicting contract definition"),
        "{error}"
    );
    assert_eq!(activations.load(Ordering::SeqCst), 0);
}

struct LateEdit;
impl PluginInstance for LateEdit {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        Contribution::new("main", |context| {
                let graph = &mut *context.graph;
                // A later contributor can replace the entire builder. An early
                // successful claim/inspection must not certify this final unit.
                *graph = phenix_graph::GraphBuilder::new("late-edit");
                graph.node(
                    "service",
                    Node::wire(|request: definition::Request| definition::Response(request.0)),
                )?;
                Ok(())
            })
.after(vec![Key::new("provider", "main")])
.register(into)
    }
}

#[test]
fn plugin_request_checks_final_selected_unit_after_late_composition() {
    let mut catalog = PluginCatalog::new();
    let a = catalog
        .register("definition", |_: ()| Ok(definition::Plugin))
        .unwrap();
    let b = catalog
        .register("provider", |plugin: provider::Plugin| Ok(plugin))
        .unwrap();
    let patch = catalog.register("patch", |_: ()| Ok(LateEdit)).unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    let activations = Arc::new(AtomicUsize::new(0));
    let mut plugins = catalog.instance();
    plugins.load(&a, ()).unwrap();
    plugins
        .load(&b, provider(1, true, &drops, &activations))
        .unwrap();
    plugins.load(&patch, ()).unwrap();
    let error = Kernel::build("host", plugins.snapshots(), Arc::new(LocalExecutor))
        .err()
        .unwrap();
    assert!(
        error.to_string().contains("service") && error.to_string().contains("claim"),
        "{error}"
    );
    assert_eq!(activations.load(Ordering::SeqCst), 0);
}
