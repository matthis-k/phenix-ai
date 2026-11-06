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
