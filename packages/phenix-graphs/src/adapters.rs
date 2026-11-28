//! Small local function adapters on the ordinary bounded process path.
use super::ComponentDef;
use crate::{
    Failure, Value, ValueError,
    interface::{Interface, InterfaceError, PortDef},
    process::{Process, ProcessContext, ProcessEmission, Step},
};
use std::sync::Arc;

struct Adapter<F>(F);
impl<F> Process for Adapter<F>
where
    F: FnMut(&mut ProcessContext) -> Result<Step, Failure> + Send + 'static,
{
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, Failure> {
        (self.0)(context)
    }
}

impl ComponentDef {
    /// Transform local `input` into local `output`, once per activation.
    /// Cloning through the borrowed input is explicit application ownership;
    /// callers must ensure `T::clone` is safe under the input's lease contract.
    /// Captured capabilities in `handler` are explicitly shared between spawns.
    pub fn map<T, U, F>(handler: F) -> Self
    where
        T: Clone + Send + Sync + 'static,
        U: Send + Sync + 'static,
        F: Fn(T) -> U + Send + Sync + 'static,
    {
        Self::map_state(|| (), move |_, input| handler(input))
    }

    /// Transform with mutable process-private state, initialized at each spawn.
    /// Requests in the same process share state; restart or a new session
    /// constructs fresh state. This is not a durable or transactional store.
    pub fn map_state<T, U, S, F>(
        initialize: impl Fn() -> S + Send + Sync + 'static,
        handler: F,
    ) -> Self
    where
        T: Clone + Send + Sync + 'static,
        U: Send + Sync + 'static,
        S: Send + 'static,
        F: Fn(&mut S, T) -> U + Send + Sync + 'static,
    {
        Self::map_context_state(initialize, move |state, _, input| handler(state, input))
    }

    /// The same map adapter with access to the process's granted resources.
    /// The callback cannot retain a borrowed capability outside the activation.
    pub fn map_context_state<T, U, S, F>(
        initialize: impl Fn() -> S + Send + Sync + 'static,
        handler: F,
    ) -> Self
    where
        T: Clone + Send + Sync + 'static,
        U: Send + Sync + 'static,
        S: Send + 'static,
        F: Fn(&mut S, &ProcessContext, T) -> U + Send + Sync + 'static,
    {
        Self::filter_map_context_state(initialize, move |state, context, input| {
            Some(handler(state, context, input))
        })
    }

    /// Transform or intentionally suppress an input in the ordinary map path.
    pub fn filter_map<T, U, F>(handler: F) -> Self
    where
        T: Clone + Send + Sync + 'static,
        U: Send + Sync + 'static,
        F: Fn(T) -> Option<U> + Send + Sync + 'static,
    {
        Self::filter_map_context_state(|| (), move |_, _, input| handler(input))
    }

    fn filter_map_context_state<T, U, S, F>(
        initialize: impl Fn() -> S + Send + Sync + 'static,
        handler: F,
    ) -> Self
    where
        T: Clone + Send + Sync + 'static,
        U: Send + Sync + 'static,
        S: Send + 'static,
        F: Fn(&mut S, &ProcessContext, T) -> Option<U> + Send + Sync + 'static,
    {
        let handler = Arc::new(handler);
        Self::primitive(
            Arc::new(
                Interface::new(
                    [PortDef::local::<T>("input")],
                    [PortDef::local::<U>("output")],
                )
                .unwrap(),
            ),
            Arc::new(move || {
                let handler = handler.clone();
                let mut state = initialize();
                let mut emitted = false;
                Adapter(move |cx: &mut ProcessContext| {
                    if emitted {
                        emitted = false;
                        return Ok(Step::Complete);
                    }
                    let Some(value) = cx.take("input") else {
                        return Ok(if cx.is_eos("input") {
                            Step::Finished
                        } else {
                            Step::Complete
                        });
                    };
                    let output = value.map_callback(|| {
                        let input = value
                            .borrow::<T>()
                            .ok_or_else(|| {
                                ValueError::LocalType {
                                    expected: std::any::type_name::<T>(),
                                    actual: value.identity(),
                                }
                                .failure()
                            })?
                            .clone();
                        Ok(handler(&mut state, cx, input).map(Value::local))
                    });
                    // Preserve the input lease until the outcome has been
                    // converted to an owned Value, even when suppressed.
                    let output = match output {
                        Ok(Some(output)) => value.finish_owned(Ok(output))?,
                        Ok(None) => {
                            value.finish_owned(Ok(Value::local(())))?;
                            return Ok(Step::Complete);
                        }
                        Err(error) => return value.finish_owned(Err(error)).map(|_| Step::Complete),
                    };
                    emitted = true;
                    Ok(Step::Emit(ProcessEmission::new("output", output)))
                })
            }),
        )
    }

    /// Explicit semantic conversion between two *wire* schemas. The input and
    /// output keep their own complete versioned identities; an incompatible
    /// direct connection is never made compatible by a matching Rust layout,
    /// a contract claim, or an implicit schema-version rule.
    ///
    /// The caller supplies the conversion code and must consciously place
    /// this ordinary, bounded process in the network. Both schemas are
    /// validated before returning the definition. A failing conversion fails
    /// the activation instead of silently dropping or reinterpreting a value.
    /// Input leases remain held during decode and handler execution, and are
    /// retained by the encoded output. A panic is converted to a process
    /// failure through the same callback cleanup path as local adapters.
    ///
    /// The handler borrows `T` rather than requiring `Clone` or granting bare
    /// ownership of a decoded leased value.
    pub fn adapt_wire<T, U, F>(handler: F) -> Result<Self, InterfaceError>
    where
        T: crate::ContractValue,
        U: crate::ContractValue,
        F: Fn(&T) -> Result<U, Failure> + Send + Sync + 'static,
    {
        let interface = Arc::new(Interface::new(
            [PortDef::wire::<T>("input")?],
            [PortDef::wire::<U>("output")?],
        )?);
        let handler = Arc::new(handler);
        Ok(Self::primitive(
            interface,
            Arc::new(move || {
                let handler = handler.clone();
                let mut emitted = false;
                Adapter(move |cx: &mut ProcessContext| {
                    if emitted {
                        emitted = false;
                        return Ok(Step::Complete);
                    }
                    let Some(value) = cx.take("input") else {
                        return Ok(if cx.is_eos("input") {
                            Step::Finished
                        } else {
                            Step::Complete
                        });
                    };
                    let output = value.map_callback(|| {
                        let input = value.decoded::<T>()?;
                        let converted = handler(&input)?;
                        Ok(Value::of(converted))
                    });
                    let output = value.finish_owned(output)?;
                    emitted = true;
                    Ok(Step::Emit(ProcessEmission::new("output", output)))
                })
            }),
        ))
    }

    /// Emit one local `output` per step until the fresh per-spawn iterator ends.
    /// Only immutable construction code is shared; iteration is process-private.
    /// Register its instance with `NetworkBuilder::start` to start it automatically.
    pub fn source<T, I, F>(factory: F) -> Self
    where
        T: Send + Sync + 'static,
        I: Iterator<Item = T> + Send + 'static,
        F: Fn() -> I + Send + Sync + 'static,
    {
        Self::primitive(
            Arc::new(Interface::new([], [PortDef::local::<T>("output")]).unwrap()),
            Arc::new(move || {
                let mut iterator = factory();
                Adapter(move |_: &mut ProcessContext| {
                    Ok(match iterator.next() {
                        Some(value) => {
                            Step::Emit(ProcessEmission::new("output", Value::local(value)))
                        }
                        None => Step::Finished,
                    })
                })
            }),
        )
    }

    /// Consume borrowed local `input`, with no outputs or bare lease extraction.
    /// Captured capabilities in `handler` are explicitly shared between spawns.
    pub fn sink<T, F>(handler: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&T) + Send + Sync + 'static,
    {
        let handler = Arc::new(handler);
        Self::primitive(
            Arc::new(Interface::new([PortDef::local::<T>("input")], []).unwrap()),
            Arc::new(move || {
                let handler = handler.clone();
                Adapter(move |cx: &mut ProcessContext| {
                    let Some(value) = cx.take("input") else {
                        return Ok(if cx.is_eos("input") {
                            Step::Finished
                        } else {
                            Step::Complete
                        });
                    };
                    let result = value.map_callback(|| {
                        handler(value.borrow::<T>().ok_or_else(|| {
                            ValueError::LocalType {
                                expected: std::any::type_name::<T>(),
                                actual: value.identity(),
                            }
                            .failure()
                        })?);
                        Ok(Value::local(()))
                    });
                    value.finish_owned(result)?;
                    Ok(Step::Complete)
                })
            }),
        )
    }
}
