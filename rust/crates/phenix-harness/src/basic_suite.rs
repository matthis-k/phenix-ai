use crate::{default_suite_authority, HarnessBuildError, HarnessBuilder, PhenixHarness};
use phenix_core::{KernelError, PersistenceBackend};
use phenix_plugin_catalog::{
    basic_context_component_manifest, basic_context_factory, basic_context_manifest,
    basic_model_component_manifest, basic_model_factory, basic_model_manifest,
    basic_skills_component_manifest, basic_skills_factory, basic_skills_manifest,
    basic_tools_component_manifest, basic_tools_factory, basic_tools_manifest,
    session_component_manifest, session_factory, session_manifest,
};

impl HarnessBuilder {
    pub fn with_basic_suite() -> Result<Self, KernelError> {
        let mut builder = Self::new();
        builder.set_component_authority(default_suite_authority());
        builder.add_embedded(session_manifest(), session_factory)?;
        builder.add_embedded(basic_model_manifest(), basic_model_factory)?;
        builder.add_embedded(basic_tools_manifest(), basic_tools_factory)?;
        builder.add_embedded(basic_skills_manifest(), basic_skills_factory)?;
        builder.add_embedded(basic_context_manifest(), basic_context_factory)?;
        for component in [
            session_component_manifest(),
            basic_model_component_manifest(),
            basic_tools_component_manifest(),
            basic_skills_component_manifest(),
            basic_context_component_manifest(),
        ] {
            builder.add_component(component);
        }
        Ok(builder)
    }
}

impl PhenixHarness {
    pub fn basic_suite() -> Result<Self, HarnessBuildError> {
        HarnessBuilder::with_basic_suite()?.build()
    }

    pub fn basic_suite_with_persistence(
        persistence: impl PersistenceBackend + 'static,
    ) -> Result<Self, HarnessBuildError> {
        HarnessBuilder::with_basic_suite()?.build_with_persistence(persistence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        context_service, model_inference_service, skill_service, tool_service, CallableId,
        ComponentManifest, ContextCommand, ContextResourceId, ContextResourceKind, ContextResponse,
        ContextScope, InterfaceId, LocalPersistence, ModelId, ModelInferenceRequest,
        ModelInferenceResponse, PhenixSchema, PhenixValue, Project, SessionId, SkillCommand,
        SkillDefinition, SkillId, SkillResponse, ToolCommand, ToolDefinition, ToolResponse,
    };
    use phenix_plugin_catalog::{
        context_component_id, memory_component_id, session_service, SessionCommand, SessionResponse,
    };
    use std::{
        collections::{BTreeMap, BTreeSet},
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
            "phenix-basic-suite-{}-{nonce}.sqlite",
            std::process::id()
        ))
    }

    fn invoke<T, R>(harness: &mut PhenixHarness, service: &phenix_core::ServiceId, request: &T) -> R
    where
        for<'value> PhenixValue: From<&'value T>,
        for<'value> R: TryFrom<Project<&'value PhenixValue>, Error = phenix_core::ValueError>,
    {
        let input = serde_json::to_vec(&PhenixValue::from(request)).unwrap();
        let output = harness
            .invoke(service, &input, &default_suite_authority(), None)
            .unwrap();
        let value: PhenixValue = serde_json::from_slice(&output).unwrap();
        R::try_from(Project(&value)).unwrap()
    }

    fn invoke_component<T, R>(
        harness: &mut PhenixHarness,
        component: ComponentManifest,
        service: &phenix_core::ServiceId,
        request: &T,
    ) -> R
    where
        for<'value> PhenixValue: From<&'value T>,
        for<'value> R: TryFrom<Project<&'value PhenixValue>, Error = phenix_core::ValueError>,
    {
        let input = serde_json::to_vec(&PhenixValue::from(request)).unwrap();
        let output = harness
            .kernel_mut()
            .invoke_component(
                &component.id,
                service,
                &input,
                &default_suite_authority(),
                &component.owner,
            )
            .unwrap();
        let value: PhenixValue = serde_json::from_slice(&output).unwrap();
        R::try_from(Project(&value)).unwrap()
    }

    #[test]
    fn selected_suite_accepts_each_basic_plugin_id() {
        for plugin in [
            "phenix.basic-model",
            "phenix.basic-tools",
            "phenix.basic-skills",
            "phenix.basic-context",
        ] {
            let selected = BTreeSet::from([plugin.to_owned()]);
            let harness = HarnessBuilder::with_selected_suite(&selected)
                .unwrap()
                .build()
                .unwrap();
            assert_eq!(
                harness
                    .kernel()
                    .config()
                    .manifests()
                    .map(|manifest| manifest.id.as_str())
                    .collect::<Vec<_>>(),
                vec![plugin]
            );
        }
    }

    #[test]
    fn default_suite_includes_memory_while_basic_suite_remains_memory_free() {
        let default = HarnessBuilder::with_default_suite()
            .unwrap()
            .build()
            .unwrap();
        assert!(default
            .kernel()
            .config()
            .manifests()
            .any(|manifest| manifest.id.as_str() == "phenix.memory"));
        for interface in ["context.compact@1", "context.expand@1"] {
            let handle = default
                .component_graph()
                .import_handle(
                    &context_component_id(),
                    &InterfaceId::parse(interface).unwrap(),
                )
                .unwrap()
                .unwrap();
            assert_eq!(handle.exporter(), &memory_component_id());
        }

        let basic = HarnessBuilder::with_basic_suite().unwrap().build().unwrap();
        assert!(!basic
            .kernel()
            .config()
            .manifests()
            .any(|manifest| manifest.id.as_str() == "phenix.memory"));
    }

    #[test]
    fn minimal_agent_journey_uses_public_services_and_restores_plugin_owned_state() {
        let path = temp_db();
        {
            let persistence = LocalPersistence::open(&path).unwrap();
            let mut harness = PhenixHarness::basic_suite_with_persistence(persistence).unwrap();
            harness.activate().unwrap();

            let _: SessionResponse = invoke(
                &mut harness,
                &session_service(),
                &SessionCommand::Create {
                    id: SessionId::parse("root").unwrap(),
                },
            );
            let _: SkillResponse = invoke_component(
                &mut harness,
                basic_skills_component_manifest(),
                &skill_service(),
                &SkillCommand::Register {
                    skill: SkillDefinition {
                        id: SkillId::parse("review").unwrap(),
                        content: b"review carefully".to_vec().into(),
                    },
                },
            );
            let _: ToolResponse = invoke_component(
                &mut harness,
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
            let _: ContextResponse = invoke_component(
                &mut harness,
                basic_context_component_manifest(),
                &context_service(),
                &ContextCommand::Register {
                    resource_id: ContextResourceId::parse("project").unwrap(),
                    kind: ContextResourceKind::ProjectDocument,
                    source: "README.md".into(),
                    scope: ContextScope::Workspace,
                    content: b"project context".to_vec().into(),
                },
            );
            let model: ModelInferenceResponse = invoke_component(
                &mut harness,
                basic_model_component_manifest(),
                &model_inference_service(),
                &ModelInferenceRequest {
                    model: ModelId::parse("direct").unwrap(),
                    input: b"hello".to_vec().into(),
                    options: BTreeMap::new(),
                    tools: Vec::new(),
                },
            );
            assert_eq!(model.output.as_ref(), b"hello");
        }

        let persistence = LocalPersistence::open(&path).unwrap();
        let mut restored = PhenixHarness::basic_suite_with_persistence(persistence).unwrap();
        restored.activate().unwrap();
        let sessions: SessionResponse =
            invoke(&mut restored, &session_service(), &SessionCommand::List);
        assert!(
            matches!(sessions, SessionResponse::Sessions { sessions } if sessions[0].id.as_str() == "root")
        );
        let skills: SkillResponse = invoke_component(
            &mut restored,
            basic_skills_component_manifest(),
            &skill_service(),
            &SkillCommand::List,
        );
        assert!(
            matches!(skills, SkillResponse::Skills { skills } if skills[0].id.as_str() == "review")
        );
        let tools: ToolResponse = invoke_component(
            &mut restored,
            basic_tools_component_manifest(),
            &tool_service(),
            &ToolCommand::List,
        );
        assert!(matches!(tools, ToolResponse::Tools { tools } if tools[0].id.as_str() == "echo"));
        let context: ContextResponse = invoke_component(
            &mut restored,
            basic_context_component_manifest(),
            &context_service(),
            &ContextCommand::List,
        );
        assert!(
            matches!(context, ContextResponse::Resources { descriptors } if descriptors[0].resource_id.as_str() == "project")
        );
        let _ = fs::remove_file(path);
    }
}
