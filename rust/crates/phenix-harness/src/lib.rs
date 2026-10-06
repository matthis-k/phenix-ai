#![forbid(unsafe_code)]

#[cfg(test)]
use phenix_core::{
    Authority, GenerationId, LayerPolicy, PluginExecution, PluginId, PluginInstance,
    PluginManifest, ResolvedGeneration,
};
#[cfg(test)]
use phenix_plugin_catalog::{
    artifact_component_manifest, benchmark_outcome_manifest, context_factory,
    efficiency_evaluation_manifest, execution_factory, execution_manifest,
    planning_component_manifest, session_factory,
};
#[cfg(test)]
use std::collections::{BTreeMap, BTreeSet};

pub mod application;
mod authority;
mod basic_suite;
pub mod model_surface_fixture;
mod persistence;
mod runtime;
mod runtime_builder;
pub mod runtime_config;
pub mod workspace_discovery;

pub use authority::{
    default_application_root_authority, default_suite_authority, runtime_orchestration_authority,
};
pub use phenix_plugin_invocation_defaults::{
    INVOCATION_DEFAULTS_PLUGIN, invocation_defaults_manifest,
};
pub use runtime::PhenixRuntime;
pub use runtime_builder::{PhenixRuntimeBuildError, PhenixRuntimeBuilder};

#[cfg(test)]
mod exact_selected_suite_tests;
#[cfg(test)]
mod tests;
