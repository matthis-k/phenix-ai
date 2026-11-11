mod support;
use phenix_graph::{
    ContractValue, Interface, PortDef, Value,
    component::ComponentDef,
    compose::{Edit, Footprint, NetworkPatch},
    contract::Contract,
    process::{LocalProcessDispatcher, Process, ProcessContext, ProcessEmission, Step},
};
use phenix_kernel::{
    CandidateArtifacts, ContractDefinition, ContractRequirement, Contribution, ImportPolicy,
    Kernel, Key, NetworkOffer,
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
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
            if self.1 {
                self.1 = false;
                return Ok(Step::Complete);
            }
            if context.is_closed("run") {
                return Ok(Step::Finished);
            }
            let request = Request::decode(context.take("run").ok_or("missing request")?.bytes())?;
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
            into.add(NetworkOffer {
                id: "service".into(),
                producer: Key::new(into.owner(), "main"),
                unit: self.path.clone(),
                contract: Key::new("definition", "transform"),
            })?;
            let activations = self.activations.clone();
            let path = if self.path == "does-not-exist" {
                "service".into()
            } else {
                self.path.clone()
            };
            into.add(Contribution {
                id: "main".into(),
                requires: vec![],
                routes: vec![],
                patch: NetworkPatch::new([Edit::Add {
                    path,
                    definition: component(self.increment, self.claim),
                }]),
                prepare: Some(Box::new(move |_| {
                    let mut artifacts = CandidateArtifacts::default();
                    let activations = activations.clone();
                    artifacts.activate(move || {
                        activations.fetch_add(1, Ordering::SeqCst);
                        Ok(Box::new(Idle))
                    })?;
                    Ok(artifacts)
                })),
            })
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
            into.add(ContractRequirement {
                id: "transform".into(),
                consumer: Key::new("consumer", "main"),
                contract: Key::new("definition", "transform"),
            })?;
            let observed = self.0.clone();
            into.add(Contribution {
                id: "main".into(),
                requires: vec![],
                routes: vec![],
                patch: NetworkPatch::new([]),
                prepare: Some(Box::new(move |context| {
                    let bound = context.import("transform")?.bind(context.compiled)?;
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
                })),
            })
        }
    }
}
fn options(snapshots: &[PluginSnapshot]) -> phenix_kernel::BuildOptions {
    let mut options = support::options(snapshots);
    support::grant(
        &mut options,
        Key::new("consumer", "main"),
        [
            Footprint::InputExport("request".into()),
            Footprint::OutputExport("reply".into()),
        ],
    );
    options
}
fn build(
    snapshots: impl IntoIterator<Item = PluginSnapshot>,
) -> Result<Kernel, phenix_kernel::ResolutionError> {
    let snapshots: Vec<_> = snapshots.into_iter().collect();
    let options = options(&snapshots);
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
    plugins.load(&a, ()).unwrap();
    let original = plugins
        .load(&b, provider(10, true, &drops, &activations))
        .unwrap();
    plugins
        .load(&c, consumer::Plugin(observed.clone()))
        .unwrap();
    let mut old = build(plugins.snapshots()).unwrap();
    assert_eq!(activations.load(Ordering::SeqCst), 0);
    old.start().unwrap();
    execute(&old, 2);
    let staged = plugins
        .prepare_reload(&b, provider(100, false, &drops, &activations))
        .unwrap();
    assert!(
        build(
            plugins
                .snapshots()
                .filter(|s| s.name() != "provider")
                .chain([staged.candidate().unwrap()])
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
    let staged = plugins.prepare_reload(&b, renamed).unwrap();
    let mut new = build(
        plugins
            .snapshots()
            .filter(|s| s.name() != "provider")
            .chain([staged.candidate().unwrap()]),
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
    let snapshot = support::snapshot("provider", p);
    assert!(
        build([snapshot.clone()])
            .err()
            .unwrap()
            .to_string()
            .contains("missing contract definition")
    );
    assert!(
        build([
            snapshot,
            support::snapshot("definition", definition::Plugin)
        ])
        .err()
        .unwrap()
        .to_string()
        .contains("does-not-exist")
    );
    assert_eq!(activations.load(Ordering::SeqCst), 0);
}
struct RequirementPlugin(Contract);
impl PluginInstance for RequirementPlugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        into.add(ContractDefinition {
            id: "requested".into(),
            contract: self.0.clone(),
        })?;
        into.add(ContractRequirement {
            id: "request".into(),
            consumer: Key::new("consumer", "main"),
            contract: Key::new("consumer", "requested"),
        })?;
        into.add(support::contribution("main", NetworkPatch::new([])))
    }
}
#[test]
fn selection_and_definition_mismatches_are_not_structural_fallbacks() {
    let drops = Arc::new(AtomicUsize::new(0));
    let activations = Arc::new(AtomicUsize::new(0));
    let base = vec![
        support::snapshot("definition", definition::Plugin),
        support::snapshot("provider", provider(1, true, &drops, &activations)),
    ];
    let mut snapshots = base.clone();
    snapshots.push(support::snapshot(
        "consumer",
        RequirementPlugin(definition::contract()),
    ));
    let mut options = options(&snapshots);
    options.imports = ImportPolicy::default();
    options.imports.selections.insert(
        Key::new("consumer", "request"),
        Key::new("provider", "absent"),
    );
    assert!(
        Kernel::build_with_options("host", snapshots, Arc::new(LocalProcessDispatcher), options)
            .err()
            .unwrap()
            .to_string()
            .contains("unknown offer")
    );
    for (contract, expected) in [
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
    ] {
        let mut snapshots = base.clone();
        snapshots.push(support::snapshot("consumer", RequirementPlugin(contract)));
        assert!(
            build(snapshots)
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
            let mut patch = support::contribution(
                "main",
                NetworkPatch::new([Edit::Replace {
                    path: "service".into(),
                    definition: provider::component(0, false),
                }]),
            );
            patch.requires.push(Key::new("provider", "main"));
            into.add(patch)
        }
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let activations = Arc::new(AtomicUsize::new(0));
    let error = build([
        support::snapshot("definition", definition::Plugin),
        support::snapshot("provider", provider(1, true, &drops, &activations)),
        support::snapshot("patch", LateEdit),
    ])
    .err()
    .unwrap();
    assert!(error.to_string().contains("claim"), "{error}");
    assert_eq!(activations.load(Ordering::SeqCst), 0);
}
