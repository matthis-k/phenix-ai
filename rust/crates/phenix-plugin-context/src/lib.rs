#![forbid(unsafe_code)]

use phenix_core::{PluginId, PluginManifest};
pub use phenix_sdk::{
    ContextAdmissionRequest, ContextAdmissionResult, ContextCommand, ContextInjection,
    ContextInjectionLifetime, ContextInjectionRequester, ContextResourceKind, ContextResponse,
    ContextScope, ExactContextReference, ExecutionContextProjection, ProjectedContextEntry,
};

mod component;
mod implementation_state;
mod projection_state;
mod prompt;
mod state_service;

pub use component::*;
pub use implementation_state::context_factory;
pub use prompt::{
    assemble_prompt, PromptAssembly, PromptSection, PromptSectionKind, PromptSectionRole,
    PHENIX_HARNESS_IDENTITY,
};

#[must_use]
pub fn context_manifest() -> PluginManifest {
    let mut manifest = implementation_state::context_manifest();
    manifest.dependencies =
        vec![PluginId::parse("phenix.execution").expect("static execution plugin id is valid")];
    manifest
}

#[cfg(test)]
mod state_integration;

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
