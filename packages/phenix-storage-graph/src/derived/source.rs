use super::*;
use phenix_graph::{
    Emission, InputSlot, InvocationCtx, NodeAction, NodeTemplate, OutputSlot, Port, emit_local,
};
use phenix_storage::snapshot::SnapshotReader;

struct ReadAction<V, I> {
    inputs: I,
    reader: Arc<SnapshotReader>,
    decode: fn(&I, &[phenix_storage::snapshot::SnapshotEntry]) -> phenix_storage::Result<V>,
}
fn diagnostic(error: phenix_storage::Error) -> String {
    let category = match &error {
        phenix_storage::Error::MissingInput { .. } => "MissingInput",
        _ => "Storage",
    };
    format!("source[{category}]: {error}")
}
impl<V: Send + Sync + 'static, I: Send + Sync> NodeAction for ReadAction<V, I> {
    fn execute(&self, mut ctx: InvocationCtx) -> Result<Vec<Emission>, String> {
        let _ = ctx.take_local::<RefreshCause>(0)?;
        let snapshot = self.reader.read().map_err(diagnostic)?;
        let values = (self.decode)(&self.inputs, snapshot.entries()).map_err(diagnostic)?;
        Ok(vec![emit_local(0, values)])
    }
}
pub(crate) fn action<I: StoredInputs>(
    inputs: I::Owned,
    reader: Arc<SnapshotReader>,
) -> Arc<dyn NodeAction> {
    Arc::new(ReadAction {
        inputs,
        reader,
        decode: I::decode,
    })
}

pub(crate) fn lower<I: StoredInputs>(
    g: &mut phenix_graph::authoring::GraphAdditions<'_>,
    name: &str,
    root: Arc<Root>,
) -> Result<DerivedRef<I::Values>, Error> {
    g.add(
        name,
        NodeTemplate::with_dyn_action(
            0,
            vec![
                Port::all(0, vec![InputSlot::local::<RefreshCause>(0).named("value")]).named("run"),
            ],
            OutputSlot::local::<I::Values>(0).named("value"),
            root.action.clone(),
        ),
    )
    .map_err(|e| Error::Graph(e.to_string()))?;
    let input = g
        .discover(name)?
        .inlets::<RefreshCause, phenix_graph::LocalValue>()
        .port(phenix_graph::InletPort::<RefreshCause, RefreshCause>::new(
            "run",
        ))
        .target()?;
    let public = g.expose_input(&root.trigger, input)?;
    g.trigger(&root.trigger, [public])?;
    Ok(DerivedRef {
        output: g
            .discover(name)?
            .output::<I::Values, phenix_graph::LocalValue>()?,
        root,
    })
}
