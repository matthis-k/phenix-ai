//! Host binding for the one graph implementation.
//! Domain contracts and selected provider dispatch remain independent of the
//! graph scheduler. This first cutover adapter deliberately rejects pending
//! native calls; they require the ticket-aware executor migration.
pub use phenix_graph::*;

use crate::{
    pending::Call,
    resolve::Selection,
    runtime::{Outcome, Root},
};
use phenix_contract::{Code, Failure, Value};
use std::collections::BTreeMap;

/// Trusted preparation binds symbolic node actions to selected operations.
/// A plugin-supplied action string alone never grants an effect capability.
pub struct BoundAction {
    pub declaring_owner: String,
    pub selected: Selection,
}

pub struct NativeActionHost<'r, 'k> {
    pub root: &'r Root<'k>,
    pub actions: &'r BTreeMap<String, BoundAction>,
}

impl phenix_graph::ActionHost for NativeActionHost<'_, '_> {
    fn invoke(
        &mut self, action: &str, input: BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>, Failure> {
        let selected = self.actions.get(action).ok_or_else(|| {
            Failure::new(Code::MissingBinding, "graph action is not selected")
        })?;
        // Native operations accept one value. A single-slot input is the
        // value directly; multi-slot input uses an explicitly keyed map.
        let request = if input.len() == 1 {
            input.into_values().next().expect("single value")
        } else {
            Value::Map(input)
        };
        let outcome = self.root.call_graph(
            &selected.declaring_owner, &selected.selected, &request, false,
        )?;
        let (port, value) = match outcome {
            Call::Ready(Outcome::Success(value)) => ("value", value),
            Call::Ready(Outcome::Domain(value)) => ("domain", value),
            Call::Pending(_) => return Err(Failure::new(
                Code::InfrastructureFailure,
                "native pending execution needs the graph ticket adapter",
            )),
        };
        Ok([(port.into(), value)].into())
    }
}
