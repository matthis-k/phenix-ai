#![allow(dead_code)]
use phenix_graph::{
    Interface, PortDef, Value,
    component::ComponentDef,
    compose::{Edit, Footprint, NetworkPatch},
    contract::Contract,
    network::{NetworkBuilder, PortPath},
    process::{Process, ProcessContext, ProcessEmission, Step},
};
use phenix_kernel::{BuildOptions, CompositionPolicy, Contribution, Key};
use phenix_plugin::{PluginCatalog, PluginInstance, PluginSnapshot};
use std::{num::NonZeroUsize, sync::Arc};

pub fn capacity() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}
pub fn path(process: &str, port: &str) -> PortPath {
    PortPath {
        process: process.into(),
        port: port.into(),
    }
}
pub fn contract(id: &str) -> Contract {
    Contract::new(
        id,
        Arc::new(
            Interface::new(
                [PortDef::local::<i64>("run")],
                [PortDef::local::<i64>("value")],
            )
            .unwrap(),
        ),
    )
    .unwrap()
}
pub struct Transform {
    pub increment: i64,
    pub emitted: bool,
}
impl Process for Transform {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let Some(value) = context.take("run") else {
            return Ok(Step::Finished);
        };
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new(
            "value",
            Value::local(value.borrow::<i64>().ok_or("wrong input")? + self.increment),
        )))
    }
}
pub fn definition(module: bool, claim: bool, increment: i64, semantic: &str) -> Arc<ComponentDef> {
    let contract = contract(semantic);
    let mut definition = ComponentDef::primitive(
        contract.required().clone(),
        Arc::new(move || Transform {
            increment,
            emitted: false,
        }),
    );
    if claim {
        definition = definition.claim(&contract).unwrap();
    }
    if module {
        let mut child = NetworkBuilder::new();
        let node = child.add("private", Arc::new(definition)).unwrap();
        child
            .expose_input("run", node.input::<i64>("run").unwrap(), capacity())
            .unwrap();
        child
            .expose_output("value", node.output::<i64>("value").unwrap())
            .unwrap();
        if claim {
            child.claim(&contract).unwrap();
        }
        definition = ComponentDef::composite(child.build().unwrap());
    }
    Arc::new(definition)
}
pub fn patch(
    process: &str,
    module: bool,
    claim: bool,
    increment: i64,
    semantic: &str,
) -> NetworkPatch {
    NetworkPatch::new([Edit::Add {
        path: process.into(),
        definition: definition(module, claim, increment, semantic),
    }])
}
pub fn contribution(id: &str, patch: NetworkPatch) -> Contribution {
    Contribution {
        id: id.into(),
        patch,
        requires: vec![],
        routes: vec![],
        prepare: None,
    }
}
pub fn function(f: impl Fn(i64) -> i64 + Send + Sync + 'static) -> Arc<ComponentDef> {
    struct Function {
        f: Arc<dyn Fn(i64) -> i64 + Send + Sync>,
        emitted: bool,
    }
    impl Process for Function {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
            if self.emitted {
                self.emitted = false;
                return Ok(Step::Complete);
            }
            let Some(value) = context.take("run") else {
                return Ok(Step::Finished);
            };
            self.emitted = true;
            Ok(Step::Emit(ProcessEmission::new(
                "value",
                Value::local((self.f)(*value.borrow::<i64>().ok_or("wrong type")?)),
            )))
        }
    }
    let f: Arc<dyn Fn(i64) -> i64 + Send + Sync> = Arc::new(f);
    Arc::new(ComponentDef::primitive(
        contract("test/function@1").required().clone(),
        Arc::new(move || Function {
            f: f.clone(),
            emitted: false,
        }),
    ))
}
pub fn sink(f: impl Fn(i64) + Send + Sync + 'static) -> Arc<ComponentDef> {
    struct Sink(Arc<dyn Fn(i64) + Send + Sync>);
    impl Process for Sink {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
            let Some(value) = context.take("run") else {
                return Ok(Step::Finished);
            };
            (self.0)(*value.borrow::<i64>().ok_or("wrong type")?);
            Ok(Step::Complete)
        }
    }
    let f: Arc<dyn Fn(i64) + Send + Sync> = Arc::new(f);
    Arc::new(ComponentDef::primitive(
        Arc::new(Interface::new([PortDef::local::<i64>("run")], []).unwrap()),
        Arc::new(move || Sink(f.clone())),
    ))
}
pub fn snapshot<P: PluginInstance + 'static>(owner: &str, plugin: P) -> PluginSnapshot {
    let mut catalog = PluginCatalog::new();
    let key = catalog.register(owner, |plugin: P| Ok(plugin)).unwrap();
    catalog.instance().load(&key, plugin).unwrap()
}
/// Test host deliberately approves the immutable fixture patches, not runtime callbacks.
pub fn options(snapshots: &[PluginSnapshot]) -> BuildOptions {
    let mut options = BuildOptions::default();
    for snapshot in snapshots {
        for (_, kind, id) in snapshot.keys() {
            if kind == <Contribution as phenix_plugin::Contribution>::KIND {
                let contribution = snapshot.get::<Contribution>(id).unwrap();
                options.composition.insert(
                    Key::new(snapshot.name(), id),
                    CompositionPolicy {
                        grants: contribution.patch.footprints(),
                        priority: 0,
                    },
                );
            }
        }
    }
    options
}
pub fn grant(options: &mut BuildOptions, key: Key, grants: impl IntoIterator<Item = Footprint>) {
    options
        .composition
        .entry(key)
        .or_default()
        .grants
        .extend(grants);
}
pub fn run(kernel: &phenix_kernel::Kernel, input: &str, value: i64) {
    kernel.inject(input, Value::local(value)).unwrap();
    kernel.run_steps(100).unwrap();
}
