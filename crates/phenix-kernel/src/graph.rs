//! Trusted host integration for the single graph engine.
//! No alternate graph scheduler, app-specific executor or V1/V2 facade.
pub use phenix_graph::*;

use crate::resolve::Selection;

/// Assembly binds node paths to selected provider operations through the
/// trusted composition. Untrusted node action strings are not capabilities.
pub struct BoundAction {
    pub declaring_owner: String,
    pub selected: Selection,
}

mod native_run;
pub use native_run::{NativeGraphRun, NativeRunStatus};

use crate::runtime::Kernel;
use phenix_contract::{Code, Failure, ScopedRequest, Value};

impl Kernel {
    /// Admit one selected root and one trigger occurrence on the sole graph
    /// executor. All node bindings come from offline-verified composition.
    pub fn begin_graph(
        &self,
        trigger: &str,
        payload: Value,
        scope: ScopedRequest,
    ) -> Result<NativeGraphRun<'_, '_>, Failure> {
        let graph = self.prepared.graph.as_ref().ok_or_else(|| {
            Failure::new(Code::MissingBinding, "composition has no compiled graph")
        })?;
        let actions = self.prepared.graph_action_bindings()?;
        let root = self.admit_scoped_root(self.prepared.entry.clone(), scope)?;
        NativeGraphRun::start(graph, root, actions, trigger, payload)
    }
}
