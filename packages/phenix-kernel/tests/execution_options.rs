use phenix_graph::{DEFAULT_MAX_ACTIVATIONS, ExecutionLimits, LocalExecutor, Value};
use phenix_kernel::{BuildOptions, Contribution, Kernel, host::KernelHost};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::Arc;

struct Dag(usize);

impl PluginInstance for Dag {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let length = self.0;
        into.add(Contribution {
            id: "dag".into(),
            requires: vec![],
            routes: vec![],
            compose: Box::new(move |context| {
                let first = context.graph.node("first", |value: i64| value + 1)?;
                let input = context.graph.expose_input("input", first.inlet())?;
                context.graph.trigger("start", [input])?;
                for index in 1..length {
                    let node = context
                        .graph
                        .node(format!("step-{index}"), |value: i64| value + 1)?;
                    context.graph.connect(first.output(), node.inlet())?;
                }
                Ok(())
            }),
        })
    }
}

#[test]
fn large_valid_dag_uses_execution_limits_through_kernel_and_host() {
    let length = DEFAULT_MAX_ACTIVATIONS + 1;
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("dag", |dag: Dag| Ok(dag)).unwrap();
    let mut plugins = catalog.instance();
    let snapshot = plugins.load(&key, Dag(length)).unwrap();

    let mut default =
        Kernel::build("default", [snapshot.clone()], Arc::new(LocalExecutor)).unwrap();
    default.start().unwrap();
    let report = default.fire("start", vec![Value::local(0_i64)]).unwrap();
    assert!(report.budget_exhausted());
    assert_eq!(report.activation_count(), DEFAULT_MAX_ACTIVATIONS);

    let mut host = KernelHost::new(plugins);
    let candidate = host
        .prepare(
            vec![],
            BuildOptions {
                imports: Default::default(),
                execution: ExecutionLimits {
                    max_activations: length,
                },
            },
            Arc::new(LocalExecutor),
            "configured",
            |_| Ok(()),
        )
        .unwrap();
    let generation = host.publish(candidate).unwrap();
    let report = generation.fire("start", vec![Value::local(0_i64)]).unwrap();
    assert!(report.success(), "{:?}", report.errors());
    assert_eq!(report.activation_count(), length);
    assert_eq!(
        report.publications().last().unwrap().borrow::<i64>(),
        Some(&2)
    );
}
