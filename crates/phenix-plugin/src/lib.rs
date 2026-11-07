//! Generic plugin contributions and native implementation lifetimes.
mod manager;
mod native;
mod registry;
pub use manager::{ContributionHandle, PluginCatalog, PluginKey, PluginSet, PluginSnapshot};
pub use native::Native;
pub use registry::{Contribution, Contributions, PluginInstance, Registrar};
