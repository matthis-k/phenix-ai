//! Deterministic native agent processes shared by the M8 and private A1 workloads.
use phenix_graph::{
    ComponentDef, Value,
    interface::{Interface, PortDef},
    process::{Process, ProcessContext, ProcessEmission, Step},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Memory {
    history: Vec<String>,
    emitted: bool,
}
impl Process for Memory {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let Some(input) = cx.take("input") else {
            return Ok(Step::Finished);
        };
        let latest = input
            .borrow::<String>()
            .ok_or_else(|| {
                phenix_graph::Failure::application_message("memory expects a local String")
            })?
            .clone();
        self.history.push(latest.clone());
        self.emitted = true;
        // Each process instance owns its own history. The most recent message
        // drives the decision; earlier tool feedback does not short-circuit a
        // later independent user turn.
        Ok(Step::Emit(ProcessEmission::new(
            "output",
            Value::local(format!(
                "memory#{}[{}];latest={latest}",
                self.history.len(),
                self.history.join("|")
            )),
        )))
    }
}
pub(crate) fn memory() -> Arc<ComponentDef> {
    Arc::new(ComponentDef::primitive(
        Arc::new(
            Interface::new(
                [PortDef::local::<String>("input")],
                [PortDef::local::<String>("output")],
            )
            .unwrap(),
        ),
        Arc::new(|| Memory {
            history: Vec::new(),
            emitted: false,
        }),
    ))
}

struct Model {
    provider: &'static str,
    activations: usize,
    emitted: bool,
    effects: Arc<AtomicUsize>,
}
impl Process for Model {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let Some(input) = cx.take("input") else {
            return Ok(Step::Finished);
        };
        let context = input
            .borrow::<String>()
            .ok_or_else(|| {
                phenix_graph::Failure::application_message("model expects a local String")
            })?
            .clone();
        self.activations += 1;
        self.effects.fetch_add(1, Ordering::SeqCst);
        self.emitted = true;
        if context.contains(";latest=observation:") {
            Ok(Step::Emit(ProcessEmission::new(
                "final",
                Value::local(format!(
                    "{}:answer#{}:{context}",
                    self.provider, self.activations
                )),
            )))
        } else {
            Ok(Step::Emit(ProcessEmission::new(
                "tool",
                Value::local(format!("lookup:{}", self.provider)),
            )))
        }
    }
}
pub(crate) fn model(provider: &'static str, effects: Arc<AtomicUsize>) -> Arc<ComponentDef> {
    Arc::new(ComponentDef::primitive(
        Arc::new(
            Interface::new(
                [PortDef::local::<String>("input")],
                [
                    PortDef::local::<String>("tool"),
                    PortDef::local::<String>("final"),
                ],
            )
            .unwrap(),
        ),
        Arc::new(move || Model {
            provider,
            activations: 0,
            emitted: false,
            effects: effects.clone(),
        }),
    ))
}

pub(crate) fn tool(effects: Arc<AtomicUsize>) -> Arc<ComponentDef> {
    Arc::new(ComponentDef::map::<String, String, _>(move |input| {
        effects.fetch_add(1, Ordering::SeqCst);
        format!("observation:{input}")
    }))
}
