mod support;
use phenix_graph::{
    ContractValue, Interface, PortDef, Value,
    component::ComponentDef,
    compose::{Edit, Footprint, NetworkPatch},
    contract::Contract,
    process::{LocalProcessDispatcher, Process, ProcessContext, ProcessEmission, Step},
};
use phenix_kernel::{
    BuildOptions, CandidateArtifacts, ContractDefinition, Contribution, ImportPolicy, Kernel, Key,
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, PluginSnapshot, Registrar};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
macro_rules! portable {
    ($name:ident, $schema:literal) => {
        pub struct $name(pub i64);
        impl ContractValue for $name {
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
            fn decode(bytes: &[u8]) -> Result<Self, phenix_graph::Failure> {
                Ok(Self(i64::from_le_bytes(bytes.try_into().map_err(
                    |cause| phenix_graph::Failure::external("bad i64 length", cause),
                )?)))
            }
        }
    };
}
mod definition {
    use super::*;
    portable!(Request, "example/request");
    portable!(Response, "example/response");
    pub fn contract() -> Contract {
        Contract::new(
            "example/transform@1",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<Request>("run").unwrap()],
                    [PortDef::wire::<Response>("value").unwrap()],
                )
                .unwrap(),
            ),
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
    struct Worker(i64, bool);
    impl Process for Worker {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
            if self.1 {
                self.1 = false;
                return Ok(Step::Complete);
            }
            if context.is_closed("run") {
                return Ok(Step::Finished);
            }
            let request = Request::decode(
                context
                    .take("run")
                    .ok_or_else(|| phenix_graph::Failure::application_message("missing request"))?
                    .bytes(),
            )?;
            self.1 = true;
            Ok(Step::Emit(ProcessEmission::new(
                "value",
                Value::of(Response(request.0 + self.0)),
            )))
        }
    }
    pub fn component(increment: i64, claim: bool) -> Arc<ComponentDef> {
        let mut component = ComponentDef::primitive(
            definition::contract().required().clone(),
            Arc::new(move || Worker(increment, false)),
        );
        if claim {
            component = component.claim(&definition::contract()).unwrap();
        }
        Arc::new(component)
    }
    impl PluginInstance for Plugin {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            let activations = self.activations.clone();
            let path = if self.path == "does-not-exist" {
                "service".into()
            } else {
                self.path.clone()
            };
            Contribution::new(
                "main",
                NetworkPatch::new([Edit::Add {
                    path,
                    definition: component(self.increment, self.claim),
                }]),
            )
            .offer(
                "service",
                self.path.clone(),
                Key::new("definition", "transform"),
            )
            .prepare(move |_| {
                let mut artifacts = CandidateArtifacts::default();
                let activations = activations.clone();
                artifacts.activate(move || {
                    activations.fetch_add(1, Ordering::SeqCst);
                    Ok(Box::new(Idle))
                })?;
                Ok(artifacts)
            })
            .register(into)
        }
    }
    struct Idle;
    impl phenix_kernel::Producer for Idle {
        fn poll(
            &mut self,
        ) -> Result<Option<phenix_kernel::InputEvent>, phenix_kernel::CallbackError> {
            Ok(None)
        }
    }
}
mod consumer {
    use super::*;
    portable!(Request, "example/request");
    portable!(Response, "example/response");
    pub struct Plugin(pub Arc<Mutex<Vec<i64>>>);
    impl PluginInstance for Plugin {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            let observed = self.0.clone();
            Contribution::new("main", NetworkPatch::new([]))
                .import("transform", Key::new("definition", "transform"))
                .prepare(move |context| {
                    let bound = context.bind_import("transform")?;
                    bound.wire_input::<Request>("run")?;
                    bound.wire_output::<Response>("value")?;
                    let mut artifacts = CandidateArtifacts::default();
                    artifacts.export(observed.clone())?;
                    artifacts.patch = Some(NetworkPatch::new([
                        Edit::ExposeInput {
                            name: "request".into(),
                            targets: vec![support::path(bound.path(), "run")],
                            capacity: support::capacity(),
                        },
                        Edit::ExposeOutput {
                            name: "reply".into(),
                            from: support::path(bound.path(), "value"),
                        },
                    ]));
                    Ok(artifacts)
                })
                .register(into)
        }
    }
}
fn options(path: &str, consumer: bool, late: bool) -> BuildOptions {
    let mut options = BuildOptions::default();
    support::grant(
        &mut options,
        Key::new("provider", "main"),
        [Footprint::Component(path.into())],
    );
    if late {
        support::grant(
            &mut options,
            Key::new("patch", "main"),
            [Footprint::Component("service".into())],
        );
    }
    if consumer {
        support::grant(
            &mut options,
            Key::new("consumer", "main"),
            [
                Footprint::InputExport("request".into()),
                Footprint::OutputExport("reply".into()),
            ],
        );
    }
    options
}
fn build(
    snapshots: impl IntoIterator<Item = PluginSnapshot>,
    options: BuildOptions,
) -> Result<Kernel, phenix_kernel::ResolutionError> {
    let snapshots: Vec<_> = snapshots.into_iter().collect();
    Kernel::build_with_options(
        "portable",
        snapshots,
        Arc::new(LocalProcessDispatcher),
        options,
    )
}
fn execute(kernel: &Kernel, value: i64) {
    kernel
        .inject("request", Value::of(consumer::Request(value)))
        .unwrap();
    kernel.run_steps(100).unwrap();
    let response = kernel
        .with_session(|session| {
            session
                .take_output("reply")
                .unwrap()
                .unwrap()
                .value()
                .decoded::<consumer::Response>()
                .unwrap()
        })
        .unwrap();
    kernel
        .export::<Arc<Mutex<Vec<i64>>>>(&Key::new("consumer", "main"))
        .unwrap()
        .lock()
        .unwrap()
        .push(response.0);
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
        .register("provider", |p: provider::Plugin| Ok(p))
        .unwrap();
    let c = catalog
        .register("consumer", |p: consumer::Plugin| Ok(p))
        .unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    let activations = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(Mutex::new(vec![]));
    let mut plugins = catalog.instance();
    let initial_options = options("service", true, false);
    plugins.load(&a, ()).unwrap();
    let original = plugins
        .load(&b, provider(10, true, &drops, &activations))
        .unwrap();
    plugins
        .load(&c, consumer::Plugin(observed.clone()))
        .unwrap();
    let mut old = build(plugins.snapshots(), initial_options).unwrap();
    assert_eq!(activations.load(Ordering::SeqCst), 0);
    old.start().unwrap();
    execute(&old, 2);
    let rejected_options = options("service", true, false);
    let staged = plugins
        .prepare_reload(&b, provider(100, false, &drops, &activations))
        .unwrap();
    assert!(
        build(
            plugins
                .snapshots()
                .filter(|s| s.name() != "provider")
                .chain([staged.candidate().unwrap()]),
            rejected_options,
        )
        .is_err()
    );
    assert_eq!(activations.load(Ordering::SeqCst), 1);
    assert_eq!(
        plugins.snapshot(&b).unwrap().revision(),
        original.revision()
    );
    drop(staged);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    execute(&old, 3);
    let mut renamed = provider(20, true, &drops, &activations);
    renamed.path = "renamed-service".into();
    let renamed_options = options("renamed-service", true, false);
    let staged = plugins.prepare_reload(&b, renamed).unwrap();
    let mut new = build(
        plugins
            .snapshots()
            .filter(|s| s.name() != "provider")
            .chain([staged.candidate().unwrap()]),
        renamed_options,
    )
    .unwrap();
    plugins.commit(staged).unwrap();
    plugins.unload(&c).unwrap();
    plugins.unload(&b).unwrap();
    plugins.unload(&a).unwrap();
    drop(original);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    new.start().unwrap();
    execute(&new, 4);
    execute(&old, 5);
    assert_eq!(*observed.lock().unwrap(), [12, 13, 24, 15]);
    drop(old);
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    drop(new);
    assert_eq!(drops.load(Ordering::SeqCst), 3);
}
#[test]
fn missing_paths_and_definitions_fail_closed() {
    let drops = Arc::new(AtomicUsize::new(0));
    let activations = Arc::new(AtomicUsize::new(0));
    let mut p = provider(1, true, &drops, &activations);
    p.path = "does-not-exist".into();
    let missing_definition_options = options("service", false, false);
    let missing_path_options = options("service", false, false);
    let snapshot = support::snapshot("provider", p);
    assert!(
        build([snapshot.clone()], missing_definition_options)
            .err()
            .unwrap()
            .to_string()
            .contains("missing contract definition")
    );
    let error = build(
        [
            snapshot,
            support::snapshot("definition", definition::Plugin),
        ],
        missing_path_options,
    )
    .err()
    .unwrap();
    let phenix_kernel::ResolutionError::MissingUnit { offer, unit, cause } = &error else {
        panic!("{error}");
    };
    assert_eq!(*offer, Key::new("provider", "service"));
    assert_eq!(unit, "does-not-exist");
    assert!(
        matches!(cause.as_ref(), phenix_graph::NetworkError::UnknownProcess(path) if path == unit)
    );
    assert!(std::ptr::eq(
        std::error::Error::source(&error)
            .unwrap()
            .downcast_ref::<phenix_graph::NetworkError>()
            .unwrap(),
        cause.as_ref()
    ));
    assert!(error.to_string().contains("does-not-exist"));
    assert_eq!(activations.load(Ordering::SeqCst), 0);
}
struct RequirementPlugin(Contract);
impl PluginInstance for RequirementPlugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        into.add(ContractDefinition {
            id: "requested".into(),
            contract: self.0.clone(),
        })?;
        support::contribution("main", NetworkPatch::new([]))
            .import("request", Key::new("consumer", "requested"))
            .register(into)
    }
}
#[test]
fn selection_and_definition_mismatches_are_not_structural_fallbacks() {
    let drops = Arc::new(AtomicUsize::new(0));
    let activations = Arc::new(AtomicUsize::new(0));
    let mut build_options = options("service", false, false);
    let mismatch_options = [
        options("service", false, false),
        options("service", false, false),
    ];
    let base = vec![
        support::snapshot("definition", definition::Plugin),
        support::snapshot("provider", provider(1, true, &drops, &activations)),
    ];
    let mut snapshots = base.clone();
    snapshots.push(support::snapshot(
        "consumer",
        RequirementPlugin(definition::contract()),
    ));
    build_options.imports = ImportPolicy::default();
    build_options.imports.selections.insert(
        Key::new("consumer", "request"),
        Key::new("provider", "absent"),
    );
    assert!(
        Kernel::build_with_options(
            "host",
            snapshots,
            Arc::new(LocalProcessDispatcher),
            build_options
        )
        .err()
        .unwrap()
        .to_string()
        .contains("unknown offer")
    );
    for ((contract, expected), options) in [
        (
            Contract::new(
                "example/unrelated@1",
                definition::contract().required().clone(),
            )
            .unwrap(),
            "missing eligible offer",
        ),
        (
            Contract::new(
                definition::contract().id(),
                Arc::new(Interface::new([], []).unwrap()),
            )
            .unwrap(),
            "conflicting contract definition",
        ),
    ]
    .into_iter()
    .zip(mismatch_options)
    {
        let mut snapshots = base.clone();
        snapshots.push(support::snapshot("consumer", RequirementPlugin(contract)));
        assert!(
            build(snapshots, options)
                .err()
                .unwrap()
                .to_string()
                .contains(expected)
        );
    }
    assert_eq!(activations.load(Ordering::SeqCst), 0);
}
#[test]
fn plugin_request_checks_final_selected_unit_after_late_composition() {
    struct LateEdit;
    impl PluginInstance for LateEdit {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            support::contribution(
                "main",
                NetworkPatch::new([Edit::Replace {
                    path: "service".into(),
                    definition: provider::component(0, false),
                }]),
            )
            .after([Key::new("provider", "main")])
            .register(into)
        }
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let activations = Arc::new(AtomicUsize::new(0));
    let options = options("service", false, true);
    let error = build(
        [
            support::snapshot("definition", definition::Plugin),
            support::snapshot("provider", provider(1, true, &drops, &activations)),
            support::snapshot("patch", LateEdit),
        ],
        options,
    )
    .err()
    .unwrap();
    let phenix_kernel::ResolutionError::Compilation { cause } = &error else {
        panic!("{error}");
    };
    let phenix_graph::NetworkError::Interface(cause) = cause else {
        panic!("{error}");
    };
    assert!(matches!(
        cause.reason.as_ref(),
        phenix_graph::interface::InterfaceReason::MissingExplicitClaim { contract }
            if contract.as_ref() == "example/transform@1"
    ));
    assert!(error.to_string().contains("claim"), "{error}");
    assert_eq!(activations.load(Ordering::SeqCst), 0);
}
