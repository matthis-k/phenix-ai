//! Typed plugin factories, contributions and exact revision leases.
mod error;
mod manager;
mod registry;
pub(crate) use error::identifier;
pub use error::{
    CallbackContext, CallbackError, CallbackPhase, Code, Failure, lease_callback_error,
};
pub use manager::{
    ContributionHandle, LifecycleFault, LifecycleOperation, PluginCatalog, PluginKey, PluginSet,
    PluginSnapshot, StagedPlugin,
};
pub use registry::{Contribution, Contributions, PluginInstance, Registrar, RegistryFault};
