#![allow(dead_code)]
use phenix_graph::{
    Interface, PortDef, Value,
    component::ComponentDef,
    compose::{Edit, Footprint, NetworkPatch},
    network::PortPath,
    process::{Process, ProcessContext, ProcessEmission, Step},
};
use phenix_kernel::{BuildOptions, CompositionPolicy, Key};
use std::{num::NonZeroUsize, sync::Arc};

pub fn capacity() -> NonZeroUsize {
    NonZeroUsize::new(16).unwrap()
}
pub fn local_map<T: Clone + Send + Sync + 'static, U: Send + Sync + 'static>(
    f: impl Fn(T) -> U + Send + Sync + 'static,
) -> Arc<ComponentDef> {
    struct Map<T, U> {
        f: Arc<dyn Fn(T) -> U + Send + Sync>,
        emitted: bool,
    }
    impl<T: Clone + Send + Sync + 'static, U: Send + Sync + 'static> Process for Map<T, U> {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
            if self.emitted {
                self.emitted = false;
                return Ok(Step::Complete);
            }
            let Some(value) = context.take("input") else {
                return Ok(Step::Finished);
            };
            let input = value.borrow::<T>().ok_or("wrong local input type")?.clone();
            self.emitted = true;
            Ok(Step::Emit(ProcessEmission::new(
                "output",
                Value::local((self.f)(input)),
            )))
        }
    }
    let f: Arc<dyn Fn(T) -> U + Send + Sync> = Arc::new(f);
    Arc::new(ComponentDef::primitive(
        Arc::new(
            Interface::new(
                [PortDef::local::<T>("input")],
                [PortDef::local::<U>("output")],
            )
            .unwrap(),
        ),
        Arc::new(move || Map {
            f: f.clone(),
            emitted: false,
        }),
    ))
}
pub fn input_patch(path: &str, input: &str, definition: Arc<ComponentDef>) -> NetworkPatch {
    NetworkPatch::new([
        Edit::Add {
            path: path.into(),
            definition,
        },
        Edit::ExposeInput {
            name: input.into(),
            targets: vec![PortPath {
                process: path.into(),
                port: "input".into(),
            }],
            capacity: capacity(),
        },
    ])
}
pub fn local_sink<T: Clone + Send + Sync + 'static>(
    f: impl Fn(T) + Send + Sync + 'static,
) -> Arc<ComponentDef> {
    struct Sink<T>(Arc<dyn Fn(T) + Send + Sync>);
    impl<T: Clone + Send + Sync + 'static> Process for Sink<T> {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
            let Some(value) = context.take("input") else {
                return Ok(Step::Finished);
            };
            (self.0)(value.borrow::<T>().ok_or("wrong local input type")?.clone());
            Ok(Step::Complete)
        }
    }
    let f: Arc<dyn Fn(T) + Send + Sync> = Arc::new(f);
    Arc::new(ComponentDef::primitive(
        Arc::new(Interface::new([PortDef::local::<T>("input")], []).unwrap()),
        Arc::new(move || Sink(f.clone())),
    ))
}
pub fn allow(options: &mut BuildOptions, owner: &str, id: &str, path: &str, input: &str) {
    options.composition.insert(
        Key::new(owner, id),
        CompositionPolicy {
            priority: 0,
            grants: [
                Footprint::Component(path.into()),
                Footprint::InputExport(input.into()),
            ]
            .into(),
        },
    );
}
