mod support;
use phenix_graph::{
    Value,
    compose::{Edit, NetworkPatch},
    network::PortPath,
    process::LocalProcessDispatcher,
};
use phenix_kernel::{BuildOptions, Contribution, Kernel};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::Arc;
struct Increment;
impl PluginInstance for Increment {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let patch =
            support::input_patch("increment", "request", support::local_map(|x: i64| x + 1));
        let mut edits = patch.edits().to_vec();
        edits.push(Edit::ExposeOutput {
            name: "reply".into(),
            from: PortPath {
                process: "increment".into(),
                port: "output".into(),
            },
        });
        registrar.add(Contribution {
            id: "main".into(),
            requires: vec![],
            routes: vec![],
            patch: NetworkPatch::new(edits),
            prepare: None,
        })
    }
}
fn main() -> Result<(), phenix_kernel::CallbackError> {
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("increment", |_: ()| Ok(Increment))?;
    let mut plugins = catalog.instance();
    plugins.load(&key, ())?;
    let mut options = BuildOptions::default();
    support::allow(&mut options, "increment", "main", "increment", "request");
    options
        .composition
        .get_mut(&phenix_kernel::Key::new("increment", "main"))
        .unwrap()
        .grants
        .insert(phenix_graph::compose::Footprint::OutputExport(
            "reply".into(),
        ));
    let mut kernel = Kernel::build_with_options(
        "example",
        plugins.snapshots(),
        Arc::new(LocalProcessDispatcher),
        options,
    )?;
    drop(plugins);
    kernel.start()?;
    assert!(kernel.start().is_err());
    kernel.inject("request", Value::local(41_i64))?;
    kernel.run_steps(16)?;
    assert_eq!(
        kernel.with_session(|session| *session
            .take_output("reply")
            .unwrap()
            .unwrap()
            .value()
            .borrow::<i64>()
            .unwrap())?,
        42
    );
    Ok(())
}
