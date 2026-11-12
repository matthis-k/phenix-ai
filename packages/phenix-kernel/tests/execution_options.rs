mod support;
use phenix_graph::{
    Value,
    compose::{Edit, NetworkPatch},
    network::ConnectionDef,
    process::LocalProcessDispatcher,
    runtime::SessionState,
};
use phenix_kernel::{Kernel, host::KernelHost};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::Arc;
struct Dag(usize);
impl PluginInstance for Dag {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
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
        into.add(support::contribution("dag", NetworkPatch::new(edits)))
    }
}
#[test]
fn large_valid_dag_uses_bounded_steps_through_kernel_and_host() {
    let length = 1025;
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("dag", |dag: Dag| Ok(dag)).unwrap();
    let mut plugins = catalog.instance();
    let snapshot = plugins.load(&key, Dag(length)).unwrap();
    let mut options = support::options(std::slice::from_ref(&snapshot));
    options
        .execution
        .discard_outputs
        .extend((1..length - 1).map(|index| (format!("step-{index}"), "value".into())));
    let mut host_options = support::options(std::slice::from_ref(&snapshot));
    host_options.execution.discard_outputs = options.execution.discard_outputs.clone();
    let mut kernel = Kernel::build_with_options(
        "bounded",
        [snapshot.clone()],
        Arc::new(LocalProcessDispatcher),
        options,
    )
    .unwrap();
    kernel.start().unwrap();
    kernel.inject("input", Value::local(0_i64)).unwrap();
    let report = kernel.run_steps(8).unwrap();
    assert!(
        report.budget_exhausted,
        "{report:?}; failure={:?}",
        kernel
            .with_session(|session| session.failure().map(str::to_owned))
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
    let mut host = KernelHost::new(plugins);
    let candidate = host
        .prepare(
            vec![],
            host_options,
            Arc::new(LocalProcessDispatcher),
            "configured",
            |_| Ok(()),
        )
        .unwrap();
    let generation = host.publish(candidate).unwrap();
    generation.inject("input", Value::local(0_i64)).unwrap();
    assert_eq!(generation.run_steps(length * 3).unwrap().steps, length * 2);
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
