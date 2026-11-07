//! Generic plugin contributions and native implementation lifetimes.
mod native;
mod registry;
mod manager;
pub use native::Native;
pub use registry::{Contribution, Contributions, PluginInstance, Registrar};
pub use manager::{ContributionHandle, PluginCatalog, PluginKey, PluginSet, PluginSnapshot};
