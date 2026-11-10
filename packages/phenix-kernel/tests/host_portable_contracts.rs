// This feature belongs to the standalone fixture package, not phenix-kernel.
#![allow(unexpected_cfgs)]

use phenix_graph::{
    AllSameCohort, ContractValue, DeclaredValue, GraphBuilder, GraphContract, InputSlot,
    InterfaceShape, InvocationCtx, LocalExecutor, NodeTemplate, OutputSlot, Port, Schema, Value,
    emit,
};
use phenix_kernel::host::{Generation, KernelHost, LifecycleError};
use phenix_kernel::{ContractDefinition, Contribution, GraphOffer, ImportPolicy, Key};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::{Arc, Mutex};

#[cfg(not(feature = "independently-compiled-consumer"))]
#[path = "fixtures/host_portable_consumer.rs"]
mod consumer;
#[cfg(feature = "independently-compiled-consumer")]
use host_portable_consumer as consumer;

// A and B have distinct Rust types, as does the separately compilable C fixture.
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
    pub fn contract() -> GraphContract {
        GraphContract::new(
            "host-portable/transform@1",
            InterfaceShape::new()
                .input_slot("run", "left", DeclaredValue::wire::<Request>())
                .unwrap()
                .input_slot("run", "right", DeclaredValue::wire::<Request>())
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
    portable!(Request, "host-portable/request", "i64-le-v1");
    portable!(Response, "host-portable/response", "i64-le-v1");
    portable!(WrongCodec, "host-portable/request", "incompatible-codec");

    #[derive(Clone, Copy, Debug)]
    pub enum Fault {
        None,
        MissingClaim,
        Codec,
        ExtraInput,
    }
    #[derive(Clone)]
    pub struct Plugin {
        pub path: &'static str,
        pub nested: bool,
        pub increment: i64,
        pub fault: Fault,
    }

    fn worker<T: ContractValue>(increment: i64, extra: bool) -> NodeTemplate {
        let mut inputs = vec![
            InputSlot::deferred::<T>(0).named("left"),
            InputSlot::deferred::<Request>(1).named("right"),
        ];
        if extra {
            inputs.push(InputSlot::deferred::<Request>(2).named("extra"));
        }
        NodeTemplate::with_action(
            500,
            vec![Port::new(0, inputs, AllSameCohort).named("run")],
            OutputSlot::typed::<Response>(0).named("value"),
            move |ctx: InvocationCtx| {
                // Noncommutative arithmetic detects swapped named slots.
                let left = ctx.arg::<Request>(0)?.0;
                let right = ctx.arg::<Request>(1)?.0;
                Ok(vec![emit(0, Response(left * 10 + right + increment))])
            },
        )
    }

    fn expose(graph: &mut GraphBuilder, path: &str, extra: bool) -> Result<(), String> {
        for slot in ["left", "right"]
            .into_iter()
            .chain(extra.then_some("extra"))
        {
            let input = graph
                .discover(path)?
                .inlets::<Request, phenix_graph::EncodedValue>()
                .port(phenix_graph::InletPort::<Request, Request>::new("run"))
                .field(phenix_graph::SlotField::<Request, Request>::new(slot))
                .target()?;
            graph.expose_input_field(
                "run",
                phenix_graph::SlotField::<(), Request>::new(slot),
                input,
            )?;
        }
        let output = graph
            .discover(path)?
            .output::<Response, phenix_graph::EncodedValue>()?;
        graph.expose_output("value", output)?;
        Ok(())
    }

    impl PluginInstance for Plugin {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            into.add(GraphOffer {
                id: "service".into(),
                producer: Key::new(into.owner(), "main"),
                unit: self.path.into(),
                contract: Key::new("definition", "transform"),
            })?;
            let config = self.clone();
            into.add(Contribution {
                id: "main".into(),
                requires: vec![],
                routes: vec![],
                compose: Box::new(move |context| {
                    let graph = &mut *context.graph;
                    let extra = matches!(config.fault, Fault::ExtraInput);
                    let template = if matches!(config.fault, Fault::Codec) {
                        worker::<WrongCodec>(config.increment, extra)
                    } else {
                        worker::<Request>(config.increment, extra)
                    };
                    if config.nested {
                        let mut child = GraphBuilder::new("private-module");
                        child.add("worker", template)?;
                        let diagnostic = child.node(
                            "diagnostic",
                            phenix_graph::Node::wire(|_: Response| Response(-1)),
                        )?;
                        child.connect(
                            child
                                .discover("worker")?
                                .output::<Response, phenix_graph::EncodedValue>()?,
                            diagnostic.inlet(),
                        )?;
                        expose(&mut child, "worker", extra)?;
                        let mut wrapper = GraphBuilder::new("wrapper");
                        wrapper.instance("inside", &child.module())?;
                        expose(&mut wrapper, "inside", extra)?;
                        if !matches!(config.fault, Fault::MissingClaim) {
                            wrapper.implements_public(&definition::contract())?;
                        }
                        graph.instance(config.path, &wrapper.module())?;
                    } else {
                        graph.add(config.path, template)?;
                        if !matches!(config.fault, Fault::MissingClaim) {
                            graph.implements(config.path, &definition::contract())?;
                        }
                    }
                    Ok(())
                }),
            })
        }
    }
}

fn policy(owner: &str) -> ImportPolicy {
    let mut policy = ImportPolicy::default();
    policy.selections.insert(
        Key::new("consumer", "transform"),
        Key::new(owner, "service"),
    );
    policy
}

fn fire(generation: &Generation, left: i64, right: i64) {
    let report = generation
        .fire(
            "run",
            vec![
                Value::of(consumer::Request(left)),
                Value::of(consumer::Request(right)),
            ],
        )
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
}

#[test]
fn host_selects_portable_node_and_nested_module_and_preserves_active_on_bad_replacements() {
    let mut catalog = PluginCatalog::new();
    let a = catalog
        .register("definition", |_: ()| Ok(definition::Plugin))
        .unwrap();
    let b1 = catalog
        .register("node-provider", |p: provider::Plugin| Ok(p))
        .unwrap();
    let b2 = catalog
        .register("module-provider", |p: provider::Plugin| Ok(p))
        .unwrap();
    let c = catalog
        .register("consumer", |p: consumer::Plugin| Ok(p))
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
    let candidate = host
        .prepare(
            stages,
            phenix_kernel::BuildOptions {
                imports: policy("node-provider"),
                ..Default::default()
            },
            Arc::new(LocalExecutor),
            "initial",
            |_| Ok(()),
        )
        .unwrap();
    assert!(host.active().is_none());
    assert_eq!(host.plugins().snapshots().count(), 0);
    let original = host.publish(candidate).unwrap();
    fire(&original, 2, 3);
    assert_eq!(*observed.lock().unwrap(), [123]);

    let candidate = host
        .prepare(
            vec![],
            phenix_kernel::BuildOptions {
                imports: policy("module-provider"),
                ..Default::default()
            },
            Arc::new(LocalExecutor),
            "switch",
            |_| Ok(()),
        )
        .unwrap();
    assert!(Arc::ptr_eq(&original, &host.active().unwrap()));
    let switched = host.publish(candidate).unwrap();
    assert_eq!(switched.epoch(), 2);
    fire(&switched, 2, 3);
    fire(&original, 3, 2);
    assert_eq!(*observed.lock().unwrap(), [123, 223, 132]);

    for (handle, owner, mut replacement) in [
        (&b1, "node-provider", node),
        (&b2, "module-provider", module),
    ] {
        let candidate = host
            .prepare(
                vec![],
                phenix_kernel::BuildOptions {
                    imports: policy(owner),
                    ..Default::default()
                },
                Arc::new(LocalExecutor),
                "select",
                |_| Ok(()),
            )
            .unwrap();
        host.publish(candidate).unwrap();
        let active = host.active().unwrap();
        let revision = host.plugins().snapshot(handle).unwrap().revision();
        for fault in [
            provider::Fault::MissingClaim,
            provider::Fault::Codec,
            provider::Fault::ExtraInput,
        ] {
            let mut broken = replacement.clone();
            broken.path = "rejected-path";
            broken.increment = 9000;
            broken.fault = fault;
            let staged = host.plugins().prepare_reload(handle, broken).unwrap();
            let error = host
                .prepare(
                    vec![staged],
                    phenix_kernel::BuildOptions {
                        imports: policy(owner),
                        ..Default::default()
                    },
                    Arc::new(LocalExecutor),
                    "rejected",
                    |_| Ok(()),
                )
                .err()
                .unwrap();
            assert!(
                matches!(error, LifecycleError::Build(_)),
                "{fault:?}: {error}"
            );
            let message = error.to_string();
            let expected = match fault {
                provider::Fault::MissingClaim => "claim",
                provider::Fault::Codec => "schema",
                provider::Fault::ExtraInput => "input",
                provider::Fault::None => unreachable!(),
            };
            assert!(message.contains(expected), "{fault:?}: {message}");
            assert!(Arc::ptr_eq(&active, &host.active().unwrap()));
            assert_eq!(
                host.plugins().snapshot(handle).unwrap().revision(),
                revision
            );
            fire(&host.active().unwrap(), 4, 5);
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
        assert!(candidate_revision > revision);
        let candidate = host
            .prepare(
                vec![stage],
                phenix_kernel::BuildOptions {
                    imports: policy(owner),
                    ..Default::default()
                },
                Arc::new(LocalExecutor),
                "renamed",
                |_| Ok(()),
            )
            .unwrap();
        assert!(Arc::ptr_eq(&active, &host.active().unwrap()));
        let current = host.publish(candidate).unwrap();
        assert_eq!(current.epoch(), active.epoch() + 1);
        assert_eq!(
            host.plugins().snapshot(handle).unwrap().revision(),
            candidate_revision
        );
        fire(&current, 5, 6);
        assert_eq!(
            *observed.lock().unwrap().last().unwrap(),
            56 + replacement.increment
        );
        fire(&active, 5, 6);
        assert_eq!(
            *observed.lock().unwrap().last().unwrap(),
            56 + replacement.increment - 1000
        );
    }
}

#[cfg(not(feature = "independently-compiled-consumer"))]
#[test]
fn independently_compiled_consumer_executes_through_host_publication() {
    use std::process::Command;
    let output_dir = tempfile::tempdir().unwrap();
    let kernel = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = kernel.join("tests/fixtures");
    // Preserve registry versions while allowing Cargo to add the standalone package.
    std::fs::copy(kernel.join("../../Cargo.lock"), fixtures.join("Cargo.lock")).unwrap();
    let output = Command::new(env!("CARGO"))
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
