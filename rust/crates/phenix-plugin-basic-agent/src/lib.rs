#![forbid(unsafe_code)]

mod agent_loop;

pub use agent_loop::{
    AGENT_LOOP_CONTROL_SERVICE, AGENT_LOOP_PLUGIN, AGENT_LOOP_PROGRESS_SERVICE, AGENT_LOOP_SERVICE,
    AGENT_TOOL_EXECUTION_SERVICE, AgentLoopCommand, AgentLoopControlInterface,
    AgentLoopControlRequest, AgentLoopControlResponse, AgentLoopFailure, AgentLoopInterface,
    AgentLoopPolicy, AgentLoopProgress, AgentLoopProgressInterface, AgentLoopProgressRecord,
    AgentLoopProgressResponse, AgentLoopResponse, AgentLoopUsage, AgentToolExecutionInterface,
    AgentToolExecutionRequest, AgentToolExecutionResponse, agent_loop_component_id,
    agent_loop_component_manifest, agent_loop_control_service, agent_loop_factory,
    agent_loop_factory_with_policy, agent_loop_manifest, agent_loop_progress_authority,
    agent_loop_progress_service, agent_loop_service, agent_tool_execution_service,
};

#[cfg(test)]
use phenix_core::{DurableSchemaRegistration, PluginManifest};
#[cfg(test)]
use phenix_sdk::StaticPluginResources;

#[cfg(test)]
pub use phenix_plugin_basic_context::{
    BASIC_CONTEXT_COMPONENT, BASIC_CONTEXT_PLUGIN, BasicContextInterface,
    basic_context_component_id, basic_context_component_manifest, basic_context_factory,
    basic_context_manifest,
};
#[cfg(test)]
pub use phenix_plugin_basic_model::{
    BASIC_MODEL_COMPONENT, BASIC_MODEL_PLUGIN, basic_model_component_manifest, basic_model_factory,
    basic_model_manifest,
};
#[cfg(test)]
pub use phenix_plugin_basic_skills::{
    BASIC_SKILLS_COMPONENT, BASIC_SKILLS_PLUGIN, BasicSkillsInterface, basic_skills_component_id,
    basic_skills_component_manifest, basic_skills_factory, basic_skills_manifest,
};
#[cfg(test)]
pub use phenix_plugin_basic_tools::{
    BASIC_TOOLS_COMPONENT, BASIC_TOOLS_PLUGIN, BasicToolsInterface, basic_tools_component_id,
    basic_tools_component_manifest, basic_tools_factory, basic_tools_manifest,
};

#[cfg(test)]
#[must_use]
fn basic_durable_schema_registrations(manifest: &PluginManifest) -> Vec<DurableSchemaRegistration> {
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
        Authority, CallableId, ComponentInterface, ComponentManifest, ContextCommand,
        ContextResourceId, ContextResourceKind, ContextResponse, ContextScope, InvocationOutcome,
        Kernel, KernelConfig, LocalPersistence, ModelId, PhenixSchema, PhenixValue,
        ResolvedGeneration, ResolvedGenerationActivation, SkillCommand, SkillDefinition, SkillId,
        SkillResponse, ToolCommand, ToolDefinition, ToolResponse, context_service, skill_service,
        tool_service,
    };
    use phenix_sdk::{
        ModelInferenceInterface, ModelInferenceRequest, ModelInferenceResponse,
        model_inference_service,
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
                .flat_map(|manifest| manifest.maximum_authority.permissions().cloned()),
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
        let resolved = ResolvedGeneration::resolve_with_durable_schemas(
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
        kernel.activate_resolved_generation(&resolved).unwrap();
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

    fn invoke_error<T>(
        kernel: &mut Kernel,
        component: ComponentManifest,
        service: &phenix_core::ServiceId,
        request: &T,
    ) -> String
    where
        for<'value> PhenixValue: From<&'value T>,
    {
        let input = serde_json::to_vec(&PhenixValue::from(request)).unwrap();
        match kernel.invoke_component(
            &component.id,
            service,
            &input,
            &authority(),
            &component.owner,
        ) {
            Ok(output) => {
                let value: PhenixValue = serde_json::from_slice(&output).unwrap();
                match InvocationOutcome::from_transport_value(value) {
                    InvocationOutcome::DomainError(PhenixValue::String(message)) => message,
                    InvocationOutcome::DomainError(error) => format!("{error:?}"),
                    InvocationOutcome::Success(value) => {
                        panic!("request must fail, got success: {value:?}")
                    }
                }
            }
            Err(error) => error.to_string(),
        }
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
                session_id: None,
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
    fn tool_catalog_revisions_and_cursors_fail_closed() {
        let path = temp_db();
        let mut kernel = kernel(&path);
        for (id, description) in [("alpha", "Alpha tool"), ("beta", "Beta tool")] {
            let _: ToolResponse = invoke(
                &mut kernel,
                basic_tools_component_manifest(),
                &tool_service(),
                &ToolCommand::Register {
                    tool: ToolDefinition {
                        id: CallableId::parse(id).unwrap(),
                        description: description.into(),
                        input_schema: PhenixSchema::Any,
                        output_schema: PhenixSchema::Any,
                        output_prefix: Vec::new().into(),
                    },
                },
            );
        }

        let catalog: ToolResponse = invoke(
            &mut kernel,
            basic_tools_component_manifest(),
            &tool_service(),
            &ToolCommand::Search {
                query: String::new(),
                cursor: None,
                limit: 1,
            },
        );
        let (catalog_revision, cursor) = match catalog {
            ToolResponse::Catalog {
                catalog_revision,
                next_cursor: Some(cursor),
                ..
            } => (catalog_revision, cursor),
            response => panic!("expected paginated tool catalog, got {response:?}"),
        };

        let cursor_error = invoke_error(
            &mut kernel,
            basic_tools_component_manifest(),
            &tool_service(),
            &ToolCommand::Search {
                query: "beta".into(),
                cursor: Some(cursor),
                limit: 1,
            },
        );
        assert!(cursor_error.contains("different query"), "{cursor_error}");

        let _: ToolResponse = invoke(
            &mut kernel,
            basic_tools_component_manifest(),
            &tool_service(),
            &ToolCommand::Register {
                tool: ToolDefinition {
                    id: CallableId::parse("gamma").unwrap(),
                    description: "Gamma tool".into(),
                    input_schema: PhenixSchema::Any,
                    output_schema: PhenixSchema::Any,
                    output_prefix: Vec::new().into(),
                },
            },
        );
        let revision_error = invoke_error(
            &mut kernel,
            basic_tools_component_manifest(),
            &tool_service(),
            &ToolCommand::LoadSchemas {
                ids: vec![CallableId::parse("alpha").unwrap()],
                catalog_revision,
            },
        );
        assert!(
            revision_error.contains("stale tool catalog revision"),
            "{revision_error}"
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
                        description: "Echo input bytes".into(),
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
        let catalog: ToolResponse = invoke(
            &mut restored,
            basic_tools_component_manifest(),
            &tool_service(),
            &ToolCommand::Search {
                query: "echo".into(),
                cursor: None,
                limit: 10,
            },
        );
        let catalog_revision = match catalog {
            ToolResponse::Catalog {
                descriptors,
                next_cursor,
                catalog_revision,
            } => {
                assert_eq!(descriptors.len(), 1);
                assert_eq!(descriptors[0].id.as_str(), "echo");
                assert_eq!(descriptors[0].description, "Echo input bytes");
                assert!(next_cursor.is_none());
                assert_eq!(descriptors[0].catalog_revision, catalog_revision);
                catalog_revision
            }
            response => panic!("unexpected tool catalog response: {response:?}"),
        };
        let schemas: ToolResponse = invoke(
            &mut restored,
            basic_tools_component_manifest(),
            &tool_service(),
            &ToolCommand::LoadSchemas {
                ids: vec![CallableId::parse("echo").unwrap()],
                catalog_revision,
            },
        );
        assert!(matches!(
            schemas,
            ToolResponse::Schemas { tools, .. }
                if tools.len() == 1 && tools[0].id.as_str() == "echo"
        ));

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
