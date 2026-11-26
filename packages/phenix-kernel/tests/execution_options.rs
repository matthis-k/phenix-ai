mod support;
use phenix_graph::{
    Value,
    compose::{Edit, NetworkPatch},
    network::ConnectionDef,
    process::{ActivationJob, DispatchResult, LocalProcessDispatcher, ProcessDispatcher},
    runtime::SessionState,
};
use phenix_kernel::{BuildOptions, Kernel, Key, host::KernelHost};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
struct CountingDispatcher(AtomicUsize);
impl ProcessDispatcher for CountingDispatcher {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        self.0.fetch_add(1, Ordering::SeqCst);
        LocalProcessDispatcher.dispatch(job)
    }
}
struct Dag(usize);
impl PluginInstance for Dag {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        support::contribution("dag", self.patch()).register(into)
    }
}
impl Dag {
    fn patch(&self) -> NetworkPatch {
        let mut edits = vec![
            Edit::Add {
                path: "first".into(),
                definition: support::function(|value| value + 1),
            },
            Edit::ExposeInput {
                name: "input".into(),
                targets: vec![support::path("first", "run")],
                capacity: support::capacity(),
            },
        ];
        for index in 1..self.0 {
            let name = format!("step-{index}");
            edits.push(Edit::Add {
                path: name.clone(),
                definition: support::function(|value| value + 1),
            });
            edits.push(Edit::Connect(ConnectionDef {
                from: support::path("first", "value"),
                to: support::path(&name, "run"),
                capacity: support::capacity(),
            }));
        }
        edits.push(Edit::ExposeOutput {
            name: "result".into(),
            from: support::path(&format!("step-{}", self.0 - 1), "value"),
        });
        NetworkPatch::new(edits)
    }
}
#[test]
fn large_valid_dag_uses_bounded_steps_through_kernel_and_host() {
    let length = 1025;
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("dag", |dag: Dag| Ok(dag)).unwrap();
    let mut plugins = catalog.instance();
    let mut options = BuildOptions::default();
    support::grant(
        &mut options,
        Key::new("dag", "dag"),
        Dag(length).patch().footprints(),
    );
    let mut host_options = BuildOptions::default();
    support::grant(
        &mut host_options,
        Key::new("dag", "dag"),
        Dag(length).patch().footprints(),
    );
    let snapshot = plugins.load(&key, Dag(length)).unwrap();
    options
        .execution
        .discard_outputs
        .extend((1..length - 1).map(|index| (format!("step-{index}"), "value".into())));
    host_options.execution.discard_outputs = options.execution.discard_outputs.clone();
    let dispatcher = Arc::new(CountingDispatcher::default());
    options.execution.dispatcher = dispatcher.clone();
    let mut kernel = Kernel::build_with_options("bounded", [snapshot.clone()], options).unwrap();
    kernel.start().unwrap();
    kernel.inject("input", Value::local(0_i64)).unwrap();
    let report = kernel.run_steps(8).unwrap();
    assert!(
        report.budget_exhausted,
        "{report:?}; failure={:?}",
        kernel
            .with_session(|session| session
                .failure()
                .map(phenix_graph::Failure::summary)
                .map(str::to_owned))
            .unwrap()
    );
    assert_eq!(report.steps, 8);
    assert_eq!(
        kernel.run_steps(length * 3).unwrap().state,
        SessionState::Waiting
    );
    assert_eq!(
        kernel
            .with_session(|session| *session
                .take_output("result")
                .unwrap()
                .unwrap()
                .value()
                .borrow::<i64>()
                .unwrap())
            .unwrap(),
        2
    );
    assert_eq!(dispatcher.0.load(Ordering::SeqCst), length * 2);
    let mut host = KernelHost::new(plugins);
    let host_dispatcher = Arc::new(CountingDispatcher::default());
    host_options.execution.dispatcher = host_dispatcher.clone();
    let candidate = host
        .prepare(vec![], host_options, "configured", |_| Ok(()))
        .unwrap();
    let generation = host.publish(candidate).unwrap();
    generation.inject("input", Value::local(0_i64)).unwrap();
    assert_eq!(generation.run_steps(length * 3).unwrap().steps, length * 2);
    assert_eq!(host_dispatcher.0.load(Ordering::SeqCst), length * 2);
    assert_eq!(
        generation
            .with_session(|session| *session
                .take_output("result")
                .unwrap()
                .unwrap()
                .value()
                .borrow::<i64>()
                .unwrap())
            .unwrap(),
        2
    );
}
