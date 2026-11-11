#![allow(unexpected_cfgs)]
mod support;
use phenix_graph::{
    ContractValue, Interface, PortDef, Schema, Value,
    component::ComponentDef,
    compose::{ConnectionKey, Edit, Footprint, NetworkPatch},
    contract::Contract,
    network::NetworkBuilder,
    process::{LocalProcessDispatcher, Process, ProcessContext, ProcessEmission, Step},
    runtime::Readiness,
};
use phenix_kernel::{
    BuildOptions, ContractDefinition, Contribution, Key, NetworkOffer,
    host::{Generation, KernelHost, LifecycleError},
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar, StagedPlugin};
use std::sync::{Arc, Mutex};

#[cfg(not(feature = "independently-compiled-consumer"))]
#[path = "fixtures/host_portable_consumer.rs"]
mod consumer;
#[cfg(feature = "independently-compiled-consumer")]
use host_portable_consumer as consumer;

macro_rules! portable {
    ($name:ident, $schema:literal, $codec:literal) => {
        struct $name(i64);
        impl ContractValue for $name {
            fn schema() -> Schema {
                Schema {
                    name: $schema.into(),
                    revision: 1,
                    structure: "record{value:i64}".into(),
                    codec: $codec.into(),
                }
            }
            fn encode(&self) -> Vec<u8> {
                self.0.to_le_bytes().to_vec()
            }
            fn decode(bytes: &[u8]) -> Result<Self, String> {
                Ok(Self(i64::from_le_bytes(
                    bytes.try_into().map_err(|_| "invalid i64")?,
                )))
            }
        }
    };
}
mod definition {
    use super::*;
    portable!(Request, "host-portable/request", "i64-le-v1");
    portable!(Response, "host-portable/response", "i64-le-v1");
    pub fn contract() -> Contract {
        Contract::new(
            "host-portable/transform@1",
            Arc::new(
                Interface::new(
                    [
                        PortDef::wire::<Request>("left").unwrap(),
                        PortDef::wire::<Request>("right").unwrap(),
                    ],
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
    portable!(Request, "host-portable/request", "i64-le-v1");
    portable!(Response, "host-portable/response", "i64-le-v1");
    portable!(WrongCodec, "host-portable/request", "incompatible-codec");
    #[derive(Clone, Copy, Debug)]
    pub enum Fault {
        None,
        MissingClaim,
        Codec,
        MissingInput,
    }
    #[derive(Clone)]
    pub struct Plugin {
        pub path: &'static str,
        pub nested: bool,
        pub increment: i64,
        pub fault: Fault,
    }
    struct Worker {
        increment: i64,
        emitted: bool,
    }
    impl Process for Worker {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
            if self.emitted {
                self.emitted = false;
                return Ok(Step::Complete);
            }
            if context.is_closed("left") || context.is_closed("right") {
                return Ok(Step::Finished);
            }
            let left =
                Request::decode(context.take("left").ok_or("missing left input")?.bytes())?.0;
            let right =
                Request::decode(context.take("right").ok_or("missing right input")?.bytes())?.0;
            self.emitted = true;
            Ok(Step::Emit(ProcessEmission::new(
                "value",
                Value::of(Response(left * 10 + right + self.increment)),
            )))
        }
    }
    impl PluginInstance for Plugin {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            into.add(NetworkOffer {
                id: "service".into(),
                producer: Key::new(into.owner(), "main"),
                unit: self.path.into(),
                contract: Key::new("definition", "transform"),
            })?;
            let left = if matches!(self.fault, Fault::Codec) {
                PortDef::wire::<WrongCodec>("left").unwrap()
            } else {
                PortDef::wire::<Request>("left").unwrap()
            };
            let mut inputs = vec![left];
            if !matches!(self.fault, Fault::MissingInput) {
                inputs.push(PortDef::wire::<Request>("right").unwrap());
            }
            let interface = Arc::new(
                Interface::new(inputs, [PortDef::wire::<Response>("value").unwrap()]).unwrap(),
            );
            let increment = self.increment;
            let mut component = ComponentDef::primitive(
                interface.clone(),
                Arc::new(move || Worker {
                    increment,
                    emitted: false,
                }),
            );
            if !matches!(self.fault, Fault::MissingClaim) {
                // A malformed advertised definition is independently rejected by
                // the host's contract catalog, not by sharing A's Rust types.
                let claim = Contract::new(definition::contract().id(), interface).unwrap();
                component = component.claim(&claim).unwrap();
            }
            if self.nested {
                for name in ["worker", "inside"] {
                    let mut child = NetworkBuilder::new();
                    let claims = component.claims().to_vec();
                    let node = child.add(name, Arc::new(component)).unwrap();
                    child
                        .expose_input_dynamic(
                            "left",
                            &node.input_endpoint("left").unwrap(),
                            support::capacity(),
                        )
                        .unwrap();
                    if !matches!(self.fault, Fault::MissingInput) {
                        child
                            .expose_input(
                                "right",
                                node.wire_input::<Request>("right").unwrap(),
                                support::capacity(),
                            )
                            .unwrap();
                    }
                    child
                        .expose_output("value", node.wire_output::<Response>("value").unwrap())
                        .unwrap();
                    for claim in claims {
                        child.claim(&claim).unwrap();
                    }
                    component = ComponentDef::composite(child.build().unwrap());
                }
            }
            into.add(Contribution {
                id: "main".into(),
                requires: vec![],
                routes: vec![],
                patch: NetworkPatch::new([Edit::Add {
                    path: self.path.into(),
                    definition: Arc::new(component),
                }]),
                prepare: None,
            })
        }
    }
}

fn options(host: &KernelHost, stages: &[StagedPlugin], owner: &str) -> BuildOptions {
    let snapshots = host.plugins().candidate_snapshots(stages).unwrap();
    let mut options = support::options(&snapshots);
    options.imports.selections.insert(
        Key::new("consumer", "transform"),
        Key::new(owner, "service"),
    );
    let mut grants = vec![
        Footprint::Component("consumer-sink".into()),
        Footprint::InputExport("left".into()),
        Footprint::InputExport("right".into()),
    ];
    for snapshot in &snapshots {
        if let Some(contribution) = snapshot.get::<Contribution>("main") {
            for edit in contribution.patch.edits() {
                if let Edit::Add { path, definition } = edit {
                    grants.push(Footprint::Connection(ConnectionKey::new(
                        support::path(path, "value"),
                        support::path("consumer-sink", "value"),
                    )));
                    let physical = if matches!(
                        definition.body(),
                        phenix_graph::component::Body::Composite(_)
                    ) {
                        format!("{path}/inside/worker")
                    } else {
                        path.clone()
                    };
                    options.execution.readiness.insert(physical, Readiness::All);
                }
            }
        }
    }
    support::grant(&mut options, Key::new("consumer", "main"), grants);
    options
}
fn prepare(
    host: &KernelHost,
    stages: Vec<StagedPlugin>,
    owner: &str,
) -> Result<phenix_kernel::host::Candidate, LifecycleError> {
    let options = options(host, &stages, owner);
    host.prepare(
        stages,
        options,
        Arc::new(LocalProcessDispatcher),
        "portable",
        |_| Ok(()),
    )
}
fn execute(generation: &Generation, left: i64, right: i64) {
    generation
        .inject("left", Value::of(consumer::Request(left)))
        .unwrap();
    generation
        .inject("right", Value::of(consumer::Request(right)))
        .unwrap();
    generation.run_steps(100).unwrap();
    let failure = generation
        .with_session(|session| session.failure().map(str::to_owned))
        .unwrap();
    assert!(failure.is_none(), "{failure:?}");
}
#[test]
fn host_selects_portable_node_and_nested_module_and_preserves_active_on_bad_replacements() {
    let mut catalog = PluginCatalog::new();
    let a = catalog
        .register("definition", |_: ()| Ok(definition::Plugin))
        .unwrap();
    let b1 = catalog
        .register("node-provider", |plugin: provider::Plugin| Ok(plugin))
        .unwrap();
    let b2 = catalog
        .register("module-provider", |plugin: provider::Plugin| Ok(plugin))
        .unwrap();
    let c = catalog
        .register("consumer", |plugin: consumer::Plugin| Ok(plugin))
        .unwrap();
    let observed = Arc::new(Mutex::new(vec![]));
    let mut host = KernelHost::new(catalog.instance());
    let node = provider::Plugin {
        path: "initial-node",
        nested: false,
        increment: 100,
        fault: provider::Fault::None,
    };
    let module = provider::Plugin {
        path: "initial-module",
        nested: true,
        increment: 200,
        fault: provider::Fault::None,
    };
    let stages = vec![
        host.plugins()
            .prepare_load(&c, consumer::Plugin(observed.clone()))
            .unwrap(),
        host.plugins().prepare_load(&b2, module.clone()).unwrap(),
        host.plugins().prepare_load(&a, ()).unwrap(),
        host.plugins().prepare_load(&b1, node.clone()).unwrap(),
    ];
    let candidate = prepare(&host, stages, "node-provider").unwrap();
    assert!(host.active().is_none());
    assert_eq!(host.plugins().snapshots().count(), 0);
    let original = host.publish(candidate).unwrap();
    execute(&original, 2, 3);
    assert_eq!(*observed.lock().unwrap(), [123]);
    let switched = host
        .publish(prepare(&host, vec![], "module-provider").unwrap())
        .unwrap();
    assert_eq!(switched.epoch(), 2);
    execute(&switched, 2, 3);
    execute(&original, 3, 2);
    assert_eq!(*observed.lock().unwrap(), [123, 223, 132]);
    for (handle, owner, mut replacement) in [
        (&b1, "node-provider", node),
        (&b2, "module-provider", module),
    ] {
        let active = host
            .publish(prepare(&host, vec![], owner).unwrap())
            .unwrap();
        let revision = host.plugins().snapshot(handle).unwrap().revision();
        for fault in [
            provider::Fault::MissingClaim,
            provider::Fault::Codec,
            provider::Fault::MissingInput,
        ] {
            let mut broken = replacement.clone();
            broken.path = "rejected-path";
            broken.increment = 9000;
            broken.fault = fault;
            let stage = host.plugins().prepare_reload(handle, broken).unwrap();
            assert!(
                matches!(
                    prepare(&host, vec![stage], owner),
                    Err(LifecycleError::Build(_))
                ),
                "{fault:?}"
            );
            assert!(Arc::ptr_eq(&active, &host.active().unwrap()));
            assert_eq!(
                host.plugins().snapshot(handle).unwrap().revision(),
                revision
            );
            execute(&active, 4, 5);
            assert_eq!(
                *observed.lock().unwrap().last().unwrap(),
                45 + replacement.increment
            );
        }
        replacement.path = if replacement.nested {
            "renamed-module"
        } else {
            "renamed-node"
        };
        replacement.increment += 1000;
        let stage = host
            .plugins()
            .prepare_reload(handle, replacement.clone())
            .unwrap();
        let candidate_revision = stage.candidate().unwrap().revision();
        let current = host
            .publish(prepare(&host, vec![stage], owner).unwrap())
            .unwrap();
        assert_eq!(current.epoch(), active.epoch() + 1);
        assert_eq!(
            host.plugins().snapshot(handle).unwrap().revision(),
            candidate_revision
        );
        execute(&current, 5, 6);
        assert_eq!(
            *observed.lock().unwrap().last().unwrap(),
            56 + replacement.increment
        );
        execute(&active, 5, 6);
        assert_eq!(
            *observed.lock().unwrap().last().unwrap(),
            56 + replacement.increment - 1000
        );
    }
}
#[cfg(not(feature = "independently-compiled-consumer"))]
#[test]
fn independently_compiled_consumer_executes_through_host_publication() {
    let output_dir = tempfile::tempdir().unwrap();
    let source = std::path::Path::new(file!());
    let fixtures = if source.is_absolute() {
        source.parent().unwrap().join("fixtures")
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    };
    let output = std::process::Command::new(env!("CARGO"))
        .args(["test", "--offline", "--manifest-path"])
        .arg(fixtures.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(output_dir.path())
        .args(["--test", "host_portable_contracts"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
