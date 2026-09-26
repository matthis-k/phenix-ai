#![forbid(unsafe_code)]

mod agent_loop;

pub use agent_loop::{
    agent_loop_component_id, agent_loop_component_manifest, agent_loop_control_service,
    agent_loop_factory, agent_loop_factory_with_policy, agent_loop_manifest,
    agent_loop_progress_service, agent_loop_service, agent_tool_execution_service,
    AgentLoopCommand, AgentLoopControlInterface, AgentLoopControlRequest, AgentLoopControlResponse,
    AgentLoopFailure, AgentLoopInterface, AgentLoopPolicy, AgentLoopProgress,
    AgentLoopProgressInterface, AgentLoopProgressRecord, AgentLoopProgressResponse,
    AgentLoopResponse, AgentLoopUsage, AgentToolExecutionInterface, AgentToolExecutionRequest,
    AgentToolExecutionResponse, AGENT_LOOP_CONTROL_SERVICE, AGENT_LOOP_PLUGIN,
    AGENT_LOOP_PROGRESS_SERVICE, AGENT_LOOP_SERVICE, AGENT_TOOL_EXECUTION_SERVICE,
    DEFAULT_MAX_MODEL_TURNS, DEFAULT_MAX_TOOL_CALLS_PER_TURN,
};

use phenix_core::{DurableSchemaRegistration, PluginManifest};
use phenix_sdk::StaticPluginResources;

pub use phenix_plugin_basic_context::{
    basic_context_component_id, basic_context_component_manifest, basic_context_factory,
    basic_context_manifest, BasicContextInterface, BASIC_CONTEXT_COMPONENT, BASIC_CONTEXT_PLUGIN,
};
pub use phenix_plugin_basic_model::{
    basic_model_component_manifest, basic_model_factory, basic_model_manifest,
    BASIC_MODEL_COMPONENT, BASIC_MODEL_PLUGIN,
};
pub use phenix_plugin_basic_skills::{
    basic_skills_component_id, basic_skills_component_manifest, basic_skills_factory,
    basic_skills_manifest, BasicSkillsInterface, BASIC_SKILLS_COMPONENT, BASIC_SKILLS_PLUGIN,
};
pub use phenix_plugin_basic_tools::{
    basic_tools_component_id, basic_tools_component_manifest, basic_tools_factory,
    basic_tools_manifest, BasicToolsInterface, BASIC_TOOLS_COMPONENT, BASIC_TOOLS_PLUGIN,
};

#[must_use]
pub fn basic_durable_schema_registrations(
    manifest: &PluginManifest,
) -> Vec<DurableSchemaRegistration> {
    let owner = &manifest.id;
    if owner.as_str() == BASIC_CONTEXT_PLUGIN {
        return <phenix_plugin_basic_context::Plugin as StaticPluginResources>::durable_schema_registrations(owner);
    }
    if owner.as_str() == BASIC_SKILLS_PLUGIN {
        return <phenix_plugin_basic_skills::Plugin as StaticPluginResources>::durable_schema_registrations(owner);
    }
    if owner.as_str() == BASIC_TOOLS_PLUGIN {
        return <phenix_plugin_basic_tools::Plugin as StaticPluginResources>::durable_schema_registrations(owner);
    }
    Vec::new()
}

#[cfg(test)]
mod component_regression;

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        context_service, skill_service, tool_service, Authority, CallableId, ComponentInterface,
        ComponentManifest, ContextCommand, ContextResourceId, ContextResourceKind, ContextResponse,
        ContextScope, Kernel, KernelConfig, LocalPersistence, ModelId, PhenixSchema, PhenixValue,
        ResolvedHarness, ResolvedHarnessActivation, SkillCommand, SkillDefinition, SkillId,
        SkillResponse, ToolCommand, ToolDefinition, ToolResponse,
    };
    use phenix_sdk::{
        model_inference_service, ModelInferenceInterface, ModelInferenceRequest,
        ModelInferenceResponse,
    };
    use std::{
        collections::BTreeMap,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_db() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "phenix-basic-agent-{}-{nonce}.sqlite",
            std::process::id()
        ))
    }

    fn authority() -> Authority {
        let manifests = [
            basic_model_manifest(),
            basic_tools_manifest(),
            basic_skills_manifest(),
            basic_context_manifest(),
        ];
        Authority::new(
            manifests
                .iter()
                .flat_map(|manifest| manifest.maximum_authority.capabilities().cloned()),
        )
    }

    fn kernel(path: &PathBuf) -> Kernel {
        let manifests = [
            basic_model_manifest(),
            basic_tools_manifest(),
            basic_skills_manifest(),
            basic_context_manifest(),
        ];
        let components = [
            basic_model_component_manifest(),
            basic_tools_component_manifest(),
            basic_skills_component_manifest(),
            basic_context_component_manifest(),
        ];
        let ceiling = authority();
        let durable_schemas = manifests
            .iter()
            .flat_map(basic_durable_schema_registrations)
            .collect::<Vec<_>>();
        let resolved = ResolvedHarness::resolve_with_durable_schemas(
            manifests.clone(),
            components,
            durable_schemas,
            [],
            &ceiling,
        )
        .unwrap();
        let mut kernel = Kernel::with_persistence(
            KernelConfig::new(manifests.clone()).unwrap(),
            LocalPersistence::open(path).unwrap(),
        );
        kernel.activate_resolved_harness(&resolved).unwrap();
        kernel
            .register_embedded_factory(manifests[0].id.clone(), basic_model_factory)
            .unwrap();
        kernel
            .register_embedded_factory(manifests[1].id.clone(), basic_tools_factory)
            .unwrap();
        kernel
            .register_embedded_factory(manifests[2].id.clone(), basic_skills_factory)
            .unwrap();
        kernel
            .register_embedded_factory(manifests[3].id.clone(), basic_context_factory)
            .unwrap();
        kernel.activate_all().unwrap();
        kernel
    }

    fn invoke<T, R>(
        kernel: &mut Kernel,
        component: ComponentManifest,
        service: &phenix_core::ServiceId,
        request: &T,
    ) -> R
    where
        for<'value> PhenixValue: From<&'value T>,
        for<'value> R:
            TryFrom<phenix_core::Project<&'value PhenixValue>, Error = phenix_core::ValueError>,
    {
        let input = serde_json::to_vec(&PhenixValue::from(request)).unwrap();
        let output = kernel
            .invoke_component(
                &component.id,
                service,
                &input,
                &authority(),
                &component.owner,
            )
            .unwrap();
        let value: PhenixValue = serde_json::from_slice(&output).unwrap();
        R::try_from(phenix_core::Project(&value)).unwrap()
    }

    #[test]
    fn basic_components_are_independently_named_and_export_canonical_interfaces() {
        let manifests = [
            basic_model_manifest(),
            basic_tools_manifest(),
            basic_skills_manifest(),
            basic_context_manifest(),
        ];
        assert_eq!(
            manifests
                .iter()
                .map(|manifest| manifest.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                BASIC_MODEL_PLUGIN,
                BASIC_TOOLS_PLUGIN,
                BASIC_SKILLS_PLUGIN,
                BASIC_CONTEXT_PLUGIN,
            ]
        );
        assert_eq!(
            ModelInferenceInterface::interface_id().as_str(),
            model_inference_service().as_str()
        );
        assert_eq!(
            BasicToolsInterface::interface_id().as_str(),
            tool_service().as_str()
        );
        assert_eq!(
            BasicSkillsInterface::interface_id().as_str(),
            skill_service().as_str()
        );
        assert_eq!(
            BasicContextInterface::interface_id().as_str(),
            context_service().as_str()
        );
    }

    #[test]
    fn basic_model_is_direct_and_policy_light() {
        let path = temp_db();
        let mut kernel = kernel(&path);
        let response: ModelInferenceResponse = invoke(
            &mut kernel,
            basic_model_component_manifest(),
            &model_inference_service(),
            &ModelInferenceRequest {
                model: ModelId::parse("direct").unwrap(),
                input: b"hello".to_vec().into(),
                options: BTreeMap::new(),
                cache: Default::default(),
                tools: Vec::new(),
                continuation: Vec::new(),
            },
        );
        assert_eq!(response.output.as_ref(), b"hello");
        assert_eq!(
            response.provider_metadata.get("provider"),
            Some(&PhenixValue::String(BASIC_MODEL_PLUGIN.to_owned()))
        );
        assert_eq!(
            response.provider_metadata.get("implementation"),
            Some(&PhenixValue::String("deterministic-echo".to_owned()))
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn basic_tool_skill_and_context_state_survives_restart() {
        let path = temp_db();
        {
            let mut first = kernel(&path);
            let _: ToolResponse = invoke(
                &mut first,
                basic_tools_component_manifest(),
                &tool_service(),
                &ToolCommand::Register {
                    tool: ToolDefinition {
                        id: CallableId::parse("echo").unwrap(),
                        input_schema: PhenixSchema::Any,
                        output_schema: PhenixSchema::Any,
                        output_prefix: b"tool:".to_vec().into(),
                    },
                },
            );
            let _: SkillResponse = invoke(
                &mut first,
                basic_skills_component_manifest(),
                &skill_service(),
                &SkillCommand::Register {
                    skill: SkillDefinition {
                        id: SkillId::parse("review").unwrap(),
                        content: b"review carefully".to_vec().into(),
                    },
                },
            );
            let registered: ContextResponse = invoke(
                &mut first,
                basic_context_component_manifest(),
                &context_service(),
                &ContextCommand::Register {
                    resource_id: ContextResourceId::parse("readme").unwrap(),
                    kind: ContextResourceKind::ProjectDocument,
                    source: "README.md".into(),
                    scope: ContextScope::Workspace,
                    content: b"project".to_vec().into(),
                },
            );
            assert!(matches!(registered, ContextResponse::Registered { .. }));
        }

        let mut restored = kernel(&path);
        let tool: ToolResponse = invoke(
            &mut restored,
            basic_tools_component_manifest(),
            &tool_service(),
            &ToolCommand::Invoke {
                id: CallableId::parse("echo").unwrap(),
                input: b"hello".to_vec().into(),
            },
        );
        assert_eq!(
            tool,
            ToolResponse::Output {
                output: b"tool:hello".to_vec().into()
            }
        );
        let skills: SkillResponse = invoke(
            &mut restored,
            basic_skills_component_manifest(),
            &skill_service(),
            &SkillCommand::List,
        );
        assert!(
            matches!(skills, SkillResponse::Skills { skills } if skills[0].id.as_str() == "review")
        );
        let context: ContextResponse = invoke(
            &mut restored,
            basic_context_component_manifest(),
            &context_service(),
            &ContextCommand::List,
        );
        assert!(
            matches!(context, ContextResponse::Resources { descriptors } if descriptors[0].resource_id.as_str() == "readme")
        );
        let _ = fs::remove_file(path);
    }
}
