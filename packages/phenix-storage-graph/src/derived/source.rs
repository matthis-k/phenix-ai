use super::*;
use phenix_graph::{
    CallbackPhase, Failure, FailureContext, FailureKind, InputKey, Interface, PortDef, Value,
    process::{Process, ProcessContext, ProcessEmission, Step},
};
use phenix_storage::snapshot::SnapshotReader;

type Compute = dyn Fn(&mut ProcessContext) -> Result<Value, Failure> + Send + Sync;

/// One emission followed by explicit activation completion. The callback is
/// immutable; each physical instance owns its continuation state.
pub(crate) struct Function {
    pub function: Arc<Compute>,
    pub emitted: bool,
    pub inputs: Vec<String>,
}
impl Process for Function {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, Failure> {
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
                    return Err(missing_input(input));
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
    function: impl Fn(&mut ProcessContext) -> Result<Value, Failure> + Send + Sync + 'static,
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
            InputKey::<V>::new("run")
                .map_shared_input(ctx, "", |_, value| Ok(Value::local(f(value))))
        },
    )
}
pub(crate) fn missing_input(field: &str) -> Failure {
    Failure::external(
        "missing source input",
        phenix_graph::InputError::Missing {
            field: field.into(),
        },
    )
    .with_kind(FailureKind::Framework)
    .with_context(FailureContext {
        field: Some(field.into()),
        ..Default::default()
    })
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
        fn step(&mut self, ctx: &mut ProcessContext) -> Result<Step, Failure> {
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
    let configuration = Arc::new((inputs, reader));
    let function: Arc<Compute> = Arc::new(move |ctx| {
        InputKey::<RefreshCause>::new("run").map_shared_input(ctx, "", |_, _| {
            let snapshot = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                configuration.1.read()
            })) {
                Ok(result) => result.map_err(|error| {
                    Failure::external("storage source read failed", error)
                        .with_context(FailureContext {
                            phase: Some(CallbackPhase::Read),
                            ..Default::default()
                        })
                        .with_lease(configuration.clone())
                }),
                Err(payload) => Err(phenix_graph::process::callback_panicked(
                    CallbackPhase::Read,
                    payload,
                )
                .with_lease(configuration.clone())),
            }?;
            let values = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                I::decode(&configuration.0, snapshot.entries())
            })) {
                Ok(result) => result.map_err(|error| {
                    Failure::external("storage source decode failed", error)
                        .with_context(FailureContext {
                            phase: Some(CallbackPhase::Decode),
                            ..Default::default()
                        })
                        .with_lease(configuration.clone())
                }),
                Err(payload) => Err(phenix_graph::process::callback_panicked(
                    CallbackPhase::Decode,
                    payload,
                )
                .with_lease(configuration.clone())),
            }?;
            Ok(Value::local(values).with_lease(configuration.clone()))
        })
    });
    Ok(Arc::new(ComponentDef::primitive(
        Arc::new(
            Interface::new(
                [PortDef::local::<RefreshCause>("run")],
                [
                    PortDef::local::<I::Values>("value"),
                    PortDef::local::<Failure>("error"),
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
    g.expose_output(&root.errors, component.output::<Failure>("error")?)?;
    Ok(DerivedRef {
        output: component.output("value")?,
        component,
        root,
    })
}
