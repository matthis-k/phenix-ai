use phenix_graph::{LocalExecutor, Value};
use phenix_kernel::{Contribution, Kernel};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::Arc;

struct Increment;
impl PluginInstance for Increment {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        registrar.add(Contribution {
            id: "main".into(),
            requires: vec![],
            routes: vec![],
            compose: Box::new(|graph| {
                let node = graph
                    .node("increment", |x: i64| x + 1)
                    .map_err(|e| e.to_string())?;
                let input = graph.expose_input("request", node.inlet())?;
                graph.expose_output("reply", node.output())?;
                graph.trigger("run", [input])?;
                Ok(())
            }),
            activate: None, // Event sources can supply a zero-argument producer factory.
        })
    }
}
fn main() -> Result<(), String> {
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("increment", |_: ()| Ok(Increment))
        .map_err(|e| e.to_string())?;
    let mut plugins = catalog.instance();
    plugins.load(&key, ()).map_err(|e| e.to_string())?;
    let mut kernel = Kernel::build("example", plugins.snapshots(), Arc::new(LocalExecutor))?;
    drop(plugins); // The kernel retains exact-revision contribution leases.
    kernel.start()?;
    assert!(kernel.start().is_err()); // Even producer-free kernels start only once.
    let report = kernel.fire("run", vec![Value::local(41_i64)])?;
    assert!(report.success());
    Ok(())
}
