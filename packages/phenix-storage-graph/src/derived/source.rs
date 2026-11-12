use super::*;
use phenix_graph::{
    Interface, PortDef, Value,
    process::{Process, ProcessContext, ProcessEmission, Step},
};
use phenix_storage::snapshot::SnapshotReader;

type Compute = dyn Fn(&mut ProcessContext) -> Result<Value, String> + Send + Sync;

/// One emission followed by explicit activation completion. The callback is
/// immutable; each physical instance owns its continuation state.
pub(crate) struct Function {
    pub function: Arc<Compute>,
    pub emitted: bool,
    pub inputs: Vec<String>,
}
impl Process for Function {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        if context.inputs().next().is_none() {
            return Ok(Step::Finished);
        }
        for input in &self.inputs {
            if context.input(input).is_none() {
                if context.is_closed(input) {
                    return Err(format!("missing join input {input}"));
                }
                return Ok(Step::WaitInput(input.clone()));
            }
        }
        let value = (self.function)(context)?;
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new("value", value)))
    }
}
pub(crate) fn function(
    inputs: impl IntoIterator<Item = PortDef>,
    output: PortDef,
    function: impl Fn(&mut ProcessContext) -> Result<Value, String> + Send + Sync + 'static,
) -> Result<Arc<ComponentDef>, Error> {
    let interface = Arc::new(Interface::new(inputs, [output]).map_err(NetworkError::from)?);
    let function: Arc<Compute> = Arc::new(function);
    let inputs: Vec<_> = interface.inputs().keys().cloned().collect();
    Ok(Arc::new(ComponentDef::primitive(
        interface,
        Arc::new(move || Function {
            function: function.clone(),
            emitted: false,
            inputs: inputs.clone(),
        }),
    )))
}
pub(crate) fn transform<V, O>(
    f: impl Fn(V) -> O + Send + Sync + 'static,
) -> Result<Arc<ComponentDef>, Error>
where
    V: Clone + Send + Sync + 'static,
    O: Clone + Send + Sync + 'static,
{
    function(
        [PortDef::local::<V>("run")],
        PortDef::local::<O>("value"),
        move |ctx| {
            let value = ctx.take("run").ok_or("missing input")?;
            Ok(Value::local(f(value
                .borrow::<V>()
                .ok_or("wrong input type")?
                .clone())))
        },
    )
}
fn diagnostic(error: phenix_storage::Error) -> String {
    let category = match &error {
        phenix_storage::Error::MissingInput { .. } => "MissingInput",
        _ => "Storage",
    };
    format!("source[{category}]: {error}")
}
pub(crate) fn definition<I: StoredInputs>(
    inputs: I::Owned,
    reader: Arc<SnapshotReader>,
) -> Result<Arc<ComponentDef>, Error> {
    struct Read {
        function: Arc<Compute>,
        emitted: bool,
    }
    impl Process for Read {
        fn step(&mut self, ctx: &mut ProcessContext) -> Result<Step, String> {
            if self.emitted {
                self.emitted = false;
                return Ok(Step::Complete);
            }
            if ctx.input("run").is_none() {
                return Ok(Step::Finished);
            }
            let (output, value) = match (self.function)(ctx) {
                Ok(value) => ("value", value),
                Err(error) => ("error", Value::local(error)),
            };
            self.emitted = true;
            Ok(Step::Emit(ProcessEmission::new(output, value)))
        }
    }
    let function: Arc<Compute> = Arc::new(move |ctx| {
        let cause = ctx.take("run").ok_or("missing refresh")?;
        cause.borrow::<RefreshCause>().ok_or("wrong refresh type")?;
        let snapshot = reader.read().map_err(diagnostic)?;
        Ok(Value::local(
            I::decode(&inputs, snapshot.entries()).map_err(diagnostic)?,
        ))
    });
    Ok(Arc::new(ComponentDef::primitive(
        Arc::new(
            Interface::new(
                [PortDef::local::<RefreshCause>("run")],
                [
                    PortDef::local::<I::Values>("value"),
                    PortDef::local::<String>("error"),
                ],
            )
            .map_err(NetworkError::from)?,
        ),
        Arc::new(move || Read {
            function: function.clone(),
            emitted: false,
        }),
    )))
}
pub(crate) fn lower<I: StoredInputs>(
    g: &mut NetworkBuilder,
    name: &str,
    root: Arc<Root>,
) -> Result<DerivedRef<I::Values>, Error> {
    let component = g.add(name, root.definition.clone())?;
    g.expose_input(
        &root.trigger,
        component.input::<RefreshCause>("run")?,
        capacity(),
    )?;
    g.expose_output(&root.capture, component.output::<I::Values>("value")?)?;
    g.expose_output(&root.errors, component.output::<String>("error")?)?;
    Ok(DerivedRef {
        output: component.output("value")?,
        component,
        root,
    })
}
