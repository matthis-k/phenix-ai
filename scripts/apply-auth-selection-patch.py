from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one replacement, found {count}\n--- needle ---\n{old}")
    file.write_text(text.replace(old, new, 1))


# Fix the isolated OAuth implementation after the initial structural commit.
replace_once(
    "rust/crates/phenix-provider-sdk/src/oauth.rs",
    "    sync::Arc,\n",
    "",
)
replace_once(
    "rust/crates/phenix-provider-sdk/src/oauth.rs",
    '    form: &[("static str", String)],\n',
    "    form: &[(&str, String)],\n",
)

# Preserve the existing default invocation API and add an explicit override path.
replace_once(
    "rust/crates/phenix-sdk/src/contracts/step_runner.rs",
    "pub enum DefaultInvocationCommand {\n    Invoke { request: InvocationRequest },\n}\n",
    "pub enum DefaultInvocationCommand {\n    Invoke { request: InvocationRequest },\n    InvokeWithProfile {\n        request: InvocationRequest,\n        profile_id: RoutingProfileId,\n    },\n}\n",
)

replace_once(
    "rust/crates/phenix-plugin-step-runner/src/lib.rs",
    "            let DefaultInvocationCommand::Invoke { request } = command;\n            let resolved: InvocationDefaultsResponse = context\n                .sdk\n                .defaults\n                .invoke_projected(&InvocationDefaultsCommand::Resolve {\n                    request: request.clone(),\n                })\n                .map_err(|error| format!(\"default invocation parameters unavailable: {error}\"))?;\n            let InvocationDefaultsResponse::Params { params } = resolved;\n            return self.invoke_explicit(&context, host, request, params);\n",
    "            let (request, profile_id) = match command {\n                DefaultInvocationCommand::Invoke { request } => (request, None),\n                DefaultInvocationCommand::InvokeWithProfile { request, profile_id } => {\n                    (request, Some(profile_id))\n                }\n            };\n            let resolved: InvocationDefaultsResponse = context\n                .sdk\n                .defaults\n                .invoke_projected(&InvocationDefaultsCommand::Resolve {\n                    request: request.clone(),\n                })\n                .map_err(|error| format!(\"default invocation parameters unavailable: {error}\"))?;\n            let InvocationDefaultsResponse::Params { mut params } = resolved;\n            if let Some(profile_id) = profile_id {\n                params.profile_id = profile_id;\n            }\n            return self.invoke_explicit(&context, host, request, params);\n",
)

# Application runtime owns the fixed frontend projection of auth/model/routing.
replace_once(
    "rust/crates/phenix-harness/src/application.rs",
    "use crate::{default_suite_authority, PhenixHarness};\n",
    '#[path = "application_selection.rs"]\nmod application_selection;\n\nuse crate::{default_suite_authority, PhenixHarness};\n',
)
replace_once(
    "rust/crates/phenix-harness/src/application.rs",
    "        Acknowledged, ApplicationError, CapabilityInvokeInput, CapabilityInvokeResult, Content,\n        ElicitationHandlerRef, ExecutionChange, ExecutionState, InteractionHandlers, Message,\n",
    "        Acknowledged, ApplicationError, CapabilityInvokeInput, CapabilityInvokeResult, Content,\n        ElicitationHandlerRef, Empty, ExecutionChange, ExecutionState, InteractionHandlers, Message,\n",
)
replace_once(
    "rust/crates/phenix-harness/src/application.rs",
    "    AddClientTool, Cancel, CloseSession, CreateSession, DecideReview, GetSdk, InvokeCallable,\n    InvokeCapability, ListCallables, ListSessions, Operation, Prompt, RemoveClientTool,\n    RenameSession, ResumeSession, SetInteractionHandlers,\n",
    "    AddClientTool, Authenticate, Cancel, CloseSession, CreateSession, DecideReview,\n    DiscoverAuthentication, GetSdk, InvokeCallable, InvokeCapability, ListCallables, ListModels,\n    ListRoutingProfiles, ListSessions, Operation, Prompt, RemoveClientTool, RenameSession,\n    ResumeSession, SelectModel, SelectRoutingProfile, SetInteractionHandlers,\n",
)
replace_once(
    "rust/crates/phenix-harness/src/application.rs",
    "    PluginId, Project, RuntimeId, SessionId, SharedCapabilityRegistry, SnapshotPolicy, ValueCodec,\n",
    "    PluginId, Project, RoutingProfileId, RuntimeId, SessionId, SharedCapabilityRegistry,\n    SnapshotPolicy, ValueCodec,\n",
)

old_match = '''        match operation.as_str() {
            CreateSession::ID => self
                .create_session(decode(input)?)
                .map(|value| value.to_value()),
'''
new_match = '''        match operation.as_str() {
            DiscoverAuthentication::ID => {
                decode::<Empty>(input)?;
                application_selection::discover_authentication(&mut self.harness.lock())
                    .map(|value| value.to_value())
            }
            Authenticate::ID => application_selection::authenticate(
                &mut self.harness.lock(),
                decode(input)?,
            )
            .map(|value| value.to_value()),
            ListModels::ID => {
                let request = decode(input)?;
                self.require_open_application_session(&request.session_id)?;
                application_selection::list_models(&mut self.harness.lock(), request)
                    .map(|value| value.to_value())
            }
            SelectModel::ID => {
                let request = decode(input)?;
                self.require_open_application_session(&request.session_id)?;
                application_selection::select_model(&mut self.harness.lock(), request)
                    .map(|value| value.to_value())
            }
            ListRoutingProfiles::ID => {
                let request = decode(input)?;
                self.require_open_application_session(&request.session_id)?;
                application_selection::list_routing_profiles(&mut self.harness.lock(), request)
                    .map(|value| value.to_value())
            }
            SelectRoutingProfile::ID => {
                let request = decode(input)?;
                self.require_open_application_session(&request.session_id)?;
                application_selection::select_routing_profile(&mut self.harness.lock(), request)
                    .map(|value| value.to_value())
            }
            CreateSession::ID => self
                .create_session(decode(input)?)
                .map(|value| value.to_value()),
'''
replace_once("rust/crates/phenix-harness/src/application.rs", old_match, new_match)

replace_once(
    "rust/crates/phenix-harness/src/application.rs",
    '        "discovery",\n        "sessions",\n',
    '        "discovery",\n        "authentication",\n        "sessions",\n',
)
replace_once(
    "rust/crates/phenix-harness/src/application.rs",
    '        "prompt",\n        "sdk",\n',
    '        "prompt",\n        "models",\n        "routing",\n        "sdk",\n',
)

# Resolve provider authentication and the session selection immediately before execution.
replace_once(
    "rust/crates/phenix-harness/src/application.rs",
    "    let model_input = match model_input_from_content(&request.content) {\n",
    "    let profile_id = {\n        let mut harness = worker.harness.lock();\n        if let Err(error) = application_selection::refresh_provider_authentication(&mut harness) {\n            invocation.respond(Err(error));\n            return;\n        }\n        match application_selection::selected_profile(&mut harness, &request.session_id) {\n            Ok(Some(profile_id)) => profile_id,\n            Ok(None) => {\n                invocation.respond(Err(ApplicationError::Failed {\n                    message: \"session has no model or routing profile selection\".to_owned(),\n                }));\n                return;\n            }\n            Err(error) => {\n                invocation.respond(Err(error));\n                return;\n            }\n        }\n    };\n    let model_input = match model_input_from_content(&request.content) {\n",
)
replace_once(
    "rust/crates/phenix-harness/src/application.rs",
    "                    execution_id: runtime_execution_id,\n                    input: model_input,\n",
    "                    execution_id: runtime_execution_id,\n                    profile_id,\n                    input: model_input,\n",
)
replace_once(
    "rust/crates/phenix-harness/src/application.rs",
    "struct AgentExecutionContext {\n    session_id: SessionId,\n    execution_id: String,\n    input: Bytes,\n",
    "struct AgentExecutionContext {\n    session_id: SessionId,\n    execution_id: String,\n    profile_id: RoutingProfileId,\n    input: Bytes,\n",
)
replace_once(
    "rust/crates/phenix-harness/src/application.rs",
    "        session_id,\n        execution_id,\n        input,\n",
    "        session_id,\n        execution_id,\n        profile_id,\n        input,\n",
)
replace_once(
    "rust/crates/phenix-harness/src/application.rs",
    "        let command = AgentLoopCommand::Run {\n            execution_id: execution_id.clone(),\n            parent_attempt_id: None,\n            callable_id: Some(callable_id.clone()),\n",
    "        let command = AgentLoopCommand::RunWithProfile {\n            execution_id: execution_id.clone(),\n            parent_attempt_id: None,\n            callable_id: Some(callable_id.clone()),\n            profile_id: profile_id.clone(),\n",
)

# Ensure the descriptor explicitly retains the enum used by AuthenticationMethod.
replace_once(
    "rust/crates/phenix-application-interface/src/catalog.rs",
    "        AuthenticationMethod,\n        ModelInfo,\n",
    "        AuthenticationMethod,\n        AuthenticationMethodKind,\n        ModelInfo,\n",
)

print("auth/model/routing integration patch applied")
