//! Plugin factories, contributions and leases; graph owns interface contracts.
mod error;
mod manager;
mod registry;
pub(crate) use error::identifier;
pub use error::{CallbackContext, CallbackError, CallbackPhase, Code, Failure};
pub use manager::{
    ContributionHandle, PluginCatalog, PluginKey, PluginSet, PluginSnapshot, StagedPlugin,
};
pub use registry::{Contribution, Contributions, PluginInstance, Registrar};
