//! Binding of a selected external operation to an ordinary graph node.
//!
//! The graph kernel knows only `NodeAction`. A plugin loader/resolver owns the
//! selection of providers, permissions, lifetimes and native ABI. The resulting
//! binding has no special scheduler treatment: it is one ordinary node action.
//! This adapter demonstrates the integration seam; it is NOT Phenix's ABI.

use phenix_graph::{
    AllSameCohort, Atomic, ContractValue, InvocationCtx, NodeTemplate, OutputSlot, Port, Value,
    emit,
};
use std::sync::Arc;

/// Implemented by the surrounding host; not by the graph scheduler.
///
/// The host must validate the selected binding and the caller's authority.
/// Requests and results are explicit portable envelopes, never Rust references
/// or trait objects crossing the native plugin boundary.
pub trait SelectedOperationDispatcher: Send + Sync {
    fn invoke(&self, selected: &str, operation: &str, request: Value) -> Result<Value, String>;
}

/// A *resolved* operation reference. No plugin discovery, policy decisions,
/// loading, or selection takes place inside graph execution.
#[derive(Clone)]
pub struct SelectedOperation {
    selected: String,
    operation: String,
    dispatcher: Arc<dyn SelectedOperationDispatcher>,
}

impl SelectedOperation {
    pub fn new(
        selected: impl Into<String>,
        operation: impl Into<String>,
        dispatcher: Arc<dyn SelectedOperationDispatcher>,
    ) -> Result<Self, String> {
        let selected = selected.into();
        let operation = operation.into();
        if selected.trim().is_empty() || operation.trim().is_empty() {
            return Err("selected provider and operation must be nonempty".into());
        }
        Ok(Self {
            selected,
            operation,
            dispatcher,
        })
    }

    /// Lower a resolved external operation to the existing typed node action.
    /// The input/output contract is verified using the graph's usual wire
    /// identity, without introducing an Invoke node or a second executor.
    pub fn into_node<A: ContractValue, O: ContractValue>(self) -> NodeTemplate {
        NodeTemplate::with_action(
            0,
            vec![Port::for_layout::<Atomic<A>>(0, "run", AllSameCohort).unwrap()],
            OutputSlot::typed::<O>(0).named("value"),
            move |mut ctx: InvocationCtx| {
                let input = ctx.take_prepared::<A>()?;
                let value =
                    self.dispatcher
                        .invoke(&self.selected, &self.operation, Value::of(input))?;
                Ok(vec![emit(0, value.get::<O>()?)])
            },
        )
    }
}
