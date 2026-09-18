use phenix_adapter_acp::{wire, ApplicationAdapter};
use phenix_application_interface::{
    application_descriptor,
    types::{
        Acknowledged, ApplicationError, Content, PageInput, PromptInput, PromptResult,
        SelectionInfo, SelectionPresentation, SelectionSelectInput, Selections, SessionInfo,
        SessionList, SessionResumeInput, SessionSnapshot, StopReason,
    },
    ApplicationTransport, Cancel, CloseSession, CreateSession, ListSelections, ListSessions,
    Operation, Prompt, ResumeSession, SelectSelection,
};
use phenix_core::{ContractId, PhenixValue, RoutingProfileId, SessionId, ValueCodec};
use std::sync::{Arc, Mutex};
use wire::schema::v1::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ResourceLink,
    SetSessionConfigOptionRequest, TextContent,
};
use wire::schema::ProtocolVersion;

#[derive(Clone)]
struct FakeTransport {
    calls: Arc<Mutex<Vec<(String, PhenixValue)>>>,
    selected: Arc<Mutex<String>>,
}

impl Default for FakeTransport {
    fn default() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            selected: Arc::new(Mutex::new("balanced".to_owned())),
        }
    }
}

impl ApplicationTransport for FakeTransport {
    fn invoke(
        &self,
        operation: &ContractId,
        input: PhenixValue,
    ) -> impl std::future::Future<Output = Result<PhenixValue, ApplicationError>> {
        let calls = self.calls.clone();
        let selected = self.selected.clone();
        let operation = operation.as_str().to_owned();
        async move {
            calls
                .lock()
                .expect("fake transport calls lock")
                .push((operation.clone(), input.clone()));
            match operation.as_str() {
                id if id == CreateSession::ID => Ok(session_info().to_value()),
                id if id == ListSessions::ID => Ok(SessionList {
                    sessions: vec![session_info()],
                    next_cursor: Some("next".to_owned()),
                }
                .to_value()),
                id if id == ResumeSession::ID => Ok(SessionSnapshot {
                    session: session_info(),
                    through_sequence: 9,
                    updates: Vec::new(),
                }
                .to_value()),
                id if id == CloseSession::ID || id == Cancel::ID => Ok(Acknowledged {}.to_value()),
                id if id == Prompt::ID => Ok(PromptResult {
                    execution_id: "execution-7".to_owned(),
                    stop_reason: StopReason::EndTurn,
                }
                .to_value()),
                id if id == ListSelections::ID => {
                    let selected = selected.lock().expect("selected route lock").clone();
                    Ok(selections(&selected).to_value())
                }
                id if id == SelectSelection::ID => {
                    let selection =
                        SelectionSelectInput::from_value(&input).expect("typed route selection");
                    let value = selection.selection_id.to_string();
                    selected
                        .lock()
                        .expect("selected route lock")
                        .clone_from(&value);
                    Ok(selections(&value).to_value())
                }
                other => Err(ApplicationError::Failed {
                    message: format!("unexpected operation {other}"),
                }),
            }
        }
    }
}

fn session_info() -> SessionInfo {
    SessionInfo {
        session_id: SessionId::parse("session-1").expect("valid session id"),
        title: Some("Example".to_owned()),
        working_directory: "/workspace".to_owned(),
    }
}

fn selections(selected: &str) -> Selections {
    Selections {
        available: vec![
            SelectionInfo {
                id: RoutingProfileId::parse("balanced").expect("valid route id"),
                name: "Balanced".to_owned(),
                description: Some("Adaptive route".to_owned()),
                presentation: SelectionPresentation::Router,
            },
            SelectionInfo {
                id: RoutingProfileId::parse("model.provider.model-a.deadbeef")
                    .expect("valid fixed route id"),
                name: "Model A".to_owned(),
                description: Some("provider".to_owned()),
                presentation: SelectionPresentation::Model,
            },
        ],
        selected: Some(RoutingProfileId::parse(selected).expect("valid selected route")),
    }
}

fn adapter() -> (ApplicationAdapter<FakeTransport>, FakeTransport) {
    let transport = FakeTransport::default();
    let descriptor = application_descriptor();
    let advertised = descriptor.capabilities.keys().cloned();
    let adapter =
        ApplicationAdapter::new(transport.clone(), advertised).expect("full capabilities");
    (adapter, transport)
}

#[test]
fn initialize_advertises_only_implemented_standard_and_descriptor_extensions() {
    let (adapter, _) = adapter();
    let response = adapter.initialize(InitializeRequest::new(ProtocolVersion::V1));
    let value = serde_json::to_value(response).expect("serialize initialize response");

    assert_eq!(value["agentCapabilities"]["loadSession"], true);
    assert!(value["agentCapabilities"]["sessionCapabilities"]["list"].is_object());
    assert!(value["agentCapabilities"]["sessionCapabilities"]["resume"].is_object());
    assert!(value["agentCapabilities"]["sessionCapabilities"]["close"].is_object());

    let extensions = &value["_meta"]["phenix.extensions"];
    assert_eq!(extensions["interface"], "phenix.application@1");
    let methods = extensions["methods"].as_array().expect("extension methods");
    let skill_list = methods
        .iter()
        .find(|method| method["method"] == "_phenix/skill-list@1")
        .expect("skill list extension");
    assert_eq!(skill_list["operation"], "phenix.application.skill-list@1");
    assert_eq!(
        skill_list["capability"],
        "phenix.application.capability.skills@1"
    );
    assert!(skill_list["input"].is_object());
    assert!(skill_list["output"].is_object());

    for mapped in [
        "_phenix/selection-list@1",
        "_phenix/selection-select@1",
    ] {
        assert!(methods.iter().all(|method| method["method"] != mapped));
    }
    assert!(methods
        .iter()
        .any(|method| method["method"] == "_phenix/authentication-list@1"));

    for lane in ["methods", "events", "callbacks"] {
        for extension in extensions[lane]
            .as_array()
            .unwrap_or_else(|| panic!("{lane} extension lane"))
        {
            let method = extension["method"].as_str().expect("extension method name");
            assert!(method.starts_with("_phenix/"));
            assert!(!method.contains("client/envelope"));
        }
    }
}

#[tokio::test]
async fn standard_session_and_prompt_requests_use_typed_application_operations() {
    let (adapter, transport) = adapter();

    let created = adapter
        .new_session(NewSessionRequest::new("/workspace"))
        .await
        .expect("create session");
    assert_eq!(created.session_id.to_string(), "session-1");
    assert_eq!(created.config_options.as_ref().map(Vec::len), Some(1));

    let prompt = PromptRequest::new(
        "session-1",
        vec![
            ContentBlock::Text(TextContent::new("hello")),
            ContentBlock::ResourceLink(ResourceLink::new("README", "file:///workspace/README.md")),
        ],
    );
    let response = adapter.prompt(prompt).await.expect("prompt");
    assert_eq!(response.stop_reason, wire::schema::v1::StopReason::EndTurn);
    assert_eq!(
        response.meta.expect("prompt meta")["phenix.executionId"],
        "execution-7"
    );

    let calls = transport.calls.lock().expect("calls lock");
    assert_eq!(calls[0].0, CreateSession::ID);
    let created_input =
        phenix_application_interface::types::SessionCreateInput::from_value(&calls[0].1)
            .expect("typed create input");
    assert_eq!(created_input.working_directory, "/workspace");

    let prompt_call = calls
        .iter()
        .find(|(operation, _)| operation == Prompt::ID)
        .expect("prompt call");
    let prompt_input = PromptInput::from_value(&prompt_call.1).expect("typed prompt input");
    assert_eq!(prompt_input.session_id.as_str(), "session-1");
    assert_eq!(
        prompt_input.content,
        vec![
            Content::Text {
                text: "hello".to_owned()
            },
            Content::Resource {
                uri: "file:///workspace/README.md".to_owned(),
                mime_type: None,
                text: None,
            },
        ]
    );
}

#[tokio::test]
async fn model_and_router_choices_share_one_standard_acp_config_option() {
    let (adapter, transport) = adapter();
    let created = adapter
        .new_session(NewSessionRequest::new("/workspace"))
        .await
        .expect("create session");
    let created = serde_json::to_value(created).expect("new session JSON");
    let options = created["configOptions"]
        .as_array()
        .expect("initial config options");
    assert_eq!(options.len(), 1);
    let selection = options
        .iter()
        .find(|option| option["id"] == "model")
        .expect("unified model/routing config");
    assert_eq!(selection["category"], "model");
    assert_eq!(selection["currentValue"], "balanced");
    let choices = selection["options"].as_array().expect("selection choices");
    assert!(choices.iter().any(|choice| choice["name"] == "[router] Balanced"));
    assert!(choices.iter().any(|choice| choice["name"] == "[model] Model A"));

    let updated = adapter
        .set_session_config_option(SetSessionConfigOptionRequest::new(
            "session-1",
            "model",
            "model.provider.model-a.deadbeef",
        ))
        .await
        .expect("select fixed route");
    let updated = serde_json::to_value(updated).expect("selection response JSON");
    let selection = updated["configOptions"]
        .as_array()
        .expect("updated config options")
        .iter()
        .find(|option| option["id"] == "model")
        .expect("updated unified selection");
    assert_eq!(
        selection["currentValue"],
        "model.provider.model-a.deadbeef"
    );

    let calls = transport.calls.lock().expect("calls lock");
    let selection_call = calls
        .iter()
        .find(|(operation, _)| operation == SelectSelection::ID)
        .expect("selection call");
    let selection_input =
        SelectionSelectInput::from_value(&selection_call.1).expect("selection input");
    assert_eq!(selection_input.session_id.as_str(), "session-1");
    assert_eq!(
        selection_input.selection_id.as_str(),
        "model.provider.model-a.deadbeef"
    );
}

#[tokio::test]
async fn unsupported_security_relevant_session_inputs_fail_before_runtime_dispatch() {
    let (adapter, transport) = adapter();
    let error = adapter
        .new_session(
            NewSessionRequest::new("/workspace").additional_directories(vec!["/outside".into()]),
        )
        .await
        .expect_err("additional directory must fail");
    assert!(matches!(error, ApplicationError::InvalidInput { .. }));
    assert!(transport.calls.lock().expect("calls lock").is_empty());
}

#[tokio::test]
async fn list_and_resume_preserve_durable_session_identity_and_cwd() {
    let (adapter, transport) = adapter();
    let listed = adapter
        .list_sessions(wire::schema::v1::ListSessionsRequest::new().cwd("/workspace"))
        .await
        .expect("list sessions");
    assert_eq!(listed.sessions.len(), 1);
    assert_eq!(listed.sessions[0].session_id.to_string(), "session-1");
    assert_eq!(listed.next_cursor.as_deref(), Some("next"));

    let resumed = adapter
        .resume_session(wire::schema::v1::ResumeSessionRequest::new(
            "session-1",
            "/workspace",
        ))
        .await
        .expect("resume session");
    assert_eq!(resumed.config_options.as_ref().map(Vec::len), Some(1));

    let calls = transport.calls.lock().expect("calls lock");
    assert_eq!(calls[0].0, ListSessions::ID);
    let page = PageInput::from_value(&calls[0].1).expect("typed page input");
    assert_eq!(page.cursor, None);
    assert_eq!(calls[1].0, ResumeSession::ID);
    let resume = SessionResumeInput::from_value(&calls[1].1).expect("typed resume input");
    assert_eq!(resume.session_id.as_str(), "session-1");
    assert_eq!(resume.after_sequence, None);
}
