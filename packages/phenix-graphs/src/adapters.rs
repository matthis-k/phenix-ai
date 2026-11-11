//! Small local function adapters on the ordinary bounded process path.
use super::ComponentDef;
use crate::{
    Value,
    interface::{Interface, PortDef},
    process::{Process, ProcessContext, ProcessEmission, Step},
};
use std::sync::Arc;

struct Adapter<F>(F);
impl<F> Process for Adapter<F>
where
    F: FnMut(&mut ProcessContext) -> Result<Step, String> + Send + 'static,
{
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
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
                    let input = value
                        .borrow::<T>()
                        .ok_or("expected local adapter input")?
                        .clone();
                    let output = handler(input);
                    emitted = true;
                    Ok(Step::Emit(ProcessEmission::new(
                        "output",
                        Value::local(output),
                    )))
                })
            }),
        )
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
                    handler(value.borrow::<T>().ok_or("expected local adapter input")?);
                    Ok(Step::Complete)
                })
            }),
        )
    }
}
