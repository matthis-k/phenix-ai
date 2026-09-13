#![forbid(unsafe_code)]

use phenix_core::{PluginId, PluginManifest};
pub use phenix_sdk::{
    ContextInjection, ContextInjectionLifetime, ContextInjectionRequester, ContextResourceKind,
    ContextScope, ExactContextReference, ExecutionContextProjection, ProjectedContextEntry,
};

mod component;
mod implementation;
mod projection_state;
mod prompt;
mod state_service;

pub use component::*;
pub use implementation::context_factory;
pub use prompt::{
    assemble_prompt, PromptAssembly, PromptSection, PromptSectionKind, PromptSectionRole,
    PHENIX_HARNESS_IDENTITY,
};

/// Context loads validate execution liveness through `phenix.execution`.
/// Activation therefore depends on the execution plugin rather than failing at
/// the first load request when that service is absent.
#[must_use]
pub fn context_manifest() -> PluginManifest {
    let mut manifest = implementation::context_manifest();
    manifest.dependencies =
        vec![PluginId::parse("phenix.execution").expect("static execution plugin id is valid")];
    manifest
}

#[cfg(test)]
mod manifest_tests {
    use super::*;

    #[test]
    fn context_declares_execution_dependency() {
        assert_eq!(
            context_manifest().dependencies,
            vec![PluginId::parse("phenix.execution").unwrap()]
        );
    }
}
