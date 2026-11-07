//! Generic plugin contributions and native implementation lifetimes.
mod native;
mod registry;
pub use native::Native;
pub use registry::{Contribution, Contributions, PluginFactory, PluginInstance, Registrar};
