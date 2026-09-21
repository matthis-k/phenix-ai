use crate::{extension_meta, wire};
use phenix_application_interface::{
    application_descriptor,
    types::{
        Acknowledged, ApplicationError, Content as ApplicationContent, PageInput, PromptInput,
        SelectionPresentation, SelectionSelectInput, Selections, SessionCreateInput, SessionInput,
        SessionSnapshot, StopReason as ApplicationStopReason,
    },
    ApplicationClient, ApplicationDescriptor, ApplicationTransport, Cancel, Capabilities,
    CloseSession, CreateSession, ListSelections, ListSessions, Operation, Prompt, ResumeSession,
    SelectSelection,
};
use phenix_core::{ContractId, PhenixValue, RoutingProfileId, SessionId};
use std::path::Path;
use wire::schema::v1::{
    AgentCapabilities, CancelNotification, CloseSessionRequest, CloseSessionResponse, ContentBlock,
    Implementation, InitializeRequest, InitializeResponse, ListSessionsRequest,
    ListSessionsResponse, LoadSessionRequest, LoadSessionResponse, NewSessionRequest,
    NewSessionResponse, PromptRequest, PromptResponse, ResumeSessionRequest, ResumeSessionResponse,
    SessionCapabilities, SessionCloseCapabilities, SessionConfigOption,
    SessionConfigOptionCategory, SessionConfigSelectOption, SessionInfo, SessionListCapabilities,
    SessionResumeCapabilities, SetSessionConfigOptionRequest, SetSessionConfigOptionResponse,
    StopReason,
};

const DISCOVERY_CAPABILITY: &str = "phenix.application.capability.discovery@1";
const SESSIONS_CAPABILITY: &str = "phenix.application.capability.sessions@1";
const SESSION_LIST_CAPABILITY: &str = "phenix.application.capability.session-list@1";
const SESSION_RESUME_CAPABILITY: &str = "phenix.application.capability.session-resume@1";
const PROMPT_CAPABILITY: &str = "phenix.application.capability.prompt@1";
const ROUTING_CAPABILITY: &str = "phenix.application.capability.routing@1";
const SELECTION_CONFIG_ID: &str = "model";

pub struct ApplicationAdapter<T> {
    descriptor: ApplicationDescriptor,
    client: ApplicationClient<T>,
    capabilities: Capabilities,
}

pub struct LoadedSession {
    pub response: LoadSessionResponse,
    pub snapshot: SessionSnapshot,
}

impl<T: ApplicationTransport> ApplicationAdapter<T> {
    pub fn new(
        transport: T,
        advertised: impl IntoIterator<Item = ContractId>,
    ) -> Result<Self, ApplicationError> {
        let descriptor = application_descriptor();
        let capabilities = Capabilities::negotiate(&descriptor, advertised)?;
        require(&capabilities, DISCOVERY_CAPABILITY)?;
        require(&capabilities, SESSIONS_CAPABILITY)?;
        require(&capabilities, PROMPT_CAPABILITY)?;
        let client = ApplicationClient::new(transport, capabilities.clone());
        Ok(Self {
            descriptor,
            client,
            capabilities,
        })
    }

    #[must_use]
    pub fn application_capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    #[must_use]
    pub fn agent_capabilities(&self) -> AgentCapabilities {
        let mut session = SessionCapabilities::new();
        if self.supports(SESSION_LIST_CAPABILITY) {
            session = session.list(SessionListCapabilities::new());
        }
        if self.supports(SESSION_RESUME_CAPABILITY) {
            session = session
                .resume(SessionResumeCapabilities::new())
                .close(SessionCloseCapabilities::new());
        }

        AgentCapabilities::new()
            .load_session(self.supports(SESSION_RESUME_CAPABILITY))
            .session_capabilities(session)
    }

    #[must_use]
    pub fn initialize(&self, request: InitializeRequest) -> InitializeResponse {
        InitializeResponse::new(request.protocol_version)
            .agent_capabilities(self.agent_capabilities())
            .agent_info(Implementation::new("phenix", env!("CARGO_PKG_VERSION")).title("Phenix"))
            .meta(extension_meta(&self.descriptor, &self.capabilities))
    }

    pub fn extension_event(
        &self,
        event: &ContractId,
        payload: &PhenixValue,
    ) -> Result<wire::schema::v1::ExtNotification, ApplicationError> {
        let declaration =
            self.descriptor
                .events
                .get(event)
                .ok_or_else(|| ApplicationError::InvalidResponse {
                    message: format!("application descriptor is missing event {event}"),
                })?;
        if !self
            .capabilities
            .iter()
            .any(|capability| capability == &declaration.capability)
        {
            return Err(ApplicationError::UnsupportedCapability {
                capability: declaration.capability.clone(),
            });
        }
        crate::translate_extension_event(event, payload)
    }

    pub async fn new_session(
        &self,
        request: NewSessionRequest,
    ) -> Result<NewSessionResponse, ApplicationError> {
        reject_extra_workspace(&request.additional_directories)?;
        reject_mcp_servers(request.mcp_servers.len())?;
        let session = self
            .client
            .invoke::<CreateSession>(SessionCreateInput {
                working_directory: absolute_path(&request.cwd)?,
                title: None,
            })
            .await?;
        let config_options = self.config_options(&session.session_id).await?;
        let mut response = NewSessionResponse::new(session.session_id.to_string());
        if !config_options.is_empty() {
            response = response.config_options(config_options);
        }
        Ok(response)
    }

    pub async fn list_sessions(
        &self,
        request: ListSessionsRequest,
    ) -> Result<ListSessionsResponse, ApplicationError> {
        let cwd = request.cwd.as_deref().map(absolute_path).transpose()?;
        let page = self
            .client
            .invoke::<ListSessions>(PageInput {
                cursor: request.cursor,
            })
            .await?;
        let sessions = page
            .sessions
            .into_iter()
            .filter(|session| {
                cwd.as_ref()
                    .is_none_or(|cwd| session.working_directory == *cwd)
            })
            .map(|session| {
                SessionInfo::new(session.session_id.to_string(), session.working_directory)
                    .title(session.title)
            })
            .collect();
        Ok(ListSessionsResponse::new(sessions).next_cursor(page.next_cursor))
    }

    pub async fn resume_session(
        &self,
        request: ResumeSessionRequest,
    ) -> Result<ResumeSessionResponse, ApplicationError> {
        reject_extra_workspace(&request.additional_directories)?;
        reject_mcp_servers(request.mcp_servers.len())?;
        let expected_cwd = absolute_path(&request.cwd)?;
        let snapshot = self.resume(request.session_id.to_string(), None).await?;
        validate_cwd(&snapshot, &expected_cwd)?;
        let config_options = self.config_options(&snapshot.session.session_id).await?;
        let mut response = ResumeSessionResponse::new();
        if !config_options.is_empty() {
            response = response.config_options(config_options);
        }
        Ok(response)
    }

    pub async fn load_session(
        &self,
        request: LoadSessionRequest,
    ) -> Result<LoadedSession, ApplicationError> {
        reject_extra_workspace(&request.additional_directories)?;
        reject_mcp_servers(request.mcp_servers.len())?;
        let expected_cwd = absolute_path(&request.cwd)?;
        let snapshot = self.resume(request.session_id.to_string(), Some(0)).await?;
        validate_cwd(&snapshot, &expected_cwd)?;
        let config_options = self.config_options(&snapshot.session.session_id).await?;
        let mut response = LoadSessionResponse::new();
        if !config_options.is_empty() {
            response = response.config_options(config_options);
        }
        Ok(LoadedSession { response, snapshot })
    }

    pub async fn close_session(
        &self,
        request: CloseSessionRequest,
    ) -> Result<CloseSessionResponse, ApplicationError> {
        self.client
            .invoke::<CloseSession>(SessionInput {
                session_id: session_id(&request.session_id.to_string())?,
            })
            .await?;
        Ok(CloseSessionResponse::new())
    }

    pub async fn cancel(&self, notification: CancelNotification) -> Result<(), ApplicationError> {
        let _: Acknowledged = self
            .client
            .invoke::<Cancel>(SessionInput {
                session_id: session_id(&notification.session_id.to_string())?,
            })
            .await?;
        Ok(())
    }

    pub async fn prompt(&self, request: PromptRequest) -> Result<PromptResponse, ApplicationError> {
        let content = request
            .prompt
            .iter()
            .map(application_content)
            .collect::<Result<Vec<_>, _>>()?;
        let result = self
            .client
            .invoke::<Prompt>(PromptInput {
                session_id: session_id(&request.session_id.to_string())?,
                content,
            })
            .await?;
        let stop_reason = match result.stop_reason {
            ApplicationStopReason::EndTurn => StopReason::EndTurn,
            ApplicationStopReason::Cancelled => StopReason::Cancelled,
            ApplicationStopReason::MaxTokens => StopReason::MaxTokens,
            ApplicationStopReason::Refused => StopReason::Refusal,
        };
        let mut meta = serde_json::Map::new();
        meta.insert(
            "phenix.executionId".to_owned(),
            serde_json::Value::String(result.execution_id),
        );
        Ok(PromptResponse::new(stop_reason).meta(meta))
    }

    pub async fn set_session_config_option(
        &self,
        request: SetSessionConfigOptionRequest,
    ) -> Result<SetSessionConfigOptionResponse, ApplicationError> {
        let session_id = session_id(&request.session_id.to_string())?;
        let value = request
            .value
            .as_value_id()
            .ok_or_else(|| ApplicationError::InvalidInput {
                message: "Phenix model and routing selectors require an ACP id value".to_owned(),
            })?
            .to_string();

        match request.config_id.to_string().as_str() {
            SELECTION_CONFIG_ID => {
                let selection_id = RoutingProfileId::parse(value).map_err(|error| {
                    ApplicationError::InvalidInput {
                        message: format!("invalid ACP routing selection id: {error}"),
                    }
                })?;
                self.client
                    .invoke::<SelectSelection>(SelectionSelectInput {
                        session_id: session_id.clone(),
                        selection_id,
                    })
                    .await?;
            }
            config_id => {
                return Err(ApplicationError::InvalidInput {
                    message: format!("unsupported ACP session config option {config_id}"),
                });
            }
        }

        Ok(SetSessionConfigOptionResponse::new(
            self.config_options(&session_id).await?,
        ))
    }

    pub(crate) async fn invoke_application<O: Operation>(
        &self,
        input: O::Input,
    ) -> Result<O::Output, ApplicationError> {
        self.client.invoke::<O>(input).await
    }

    async fn resume(
        &self,
        id: String,
        after_sequence: Option<u64>,
    ) -> Result<SessionSnapshot, ApplicationError> {
        self.client
            .invoke::<ResumeSession>(phenix_application_interface::types::SessionResumeInput {
                session_id: session_id(&id)?,
                after_sequence,
            })
            .await
    }

    async fn config_options(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<SessionConfigOption>, ApplicationError> {
        let mut options = Vec::new();
        if self.supports(ROUTING_CAPABILITY) {
            let selections = self
                .client
                .invoke::<ListSelections>(SessionInput {
                    session_id: session_id.clone(),
                })
                .await?;
            if let Some(selection) = selection_config(selections)? {
                options.push(selection);
            }
        }
        Ok(options)
    }

    fn supports(&self, capability: &str) -> bool {
        self.capabilities
            .iter()
            .any(|candidate| candidate.as_str() == capability)
    }
}

fn selection_config(
    selections: Selections,
) -> Result<Option<SessionConfigOption>, ApplicationError> {
    let Some(selected) = selections.selected else {
        return Ok(None);
    };
    if !selections
        .available
        .iter()
        .any(|selection| selection.id == selected)
    {
        return Err(ApplicationError::InvalidResponse {
            message: "selected Phenix route is missing from the available selection list"
                .to_owned(),
        });
    }
    let choices = selections
        .available
        .into_iter()
        .map(|selection| {
            let mut meta = serde_json::Map::new();
            meta.insert(
                "phenix.provider".into(),
                serde_json::Value::String(selection.provider.to_string()),
            );
            let prefix = match selection.presentation {
                SelectionPresentation::Router => "router",
                SelectionPresentation::Model => "model",
            };
            SessionConfigSelectOption::new(
                selection.id.to_string(),
                format!("[{prefix}] {}", selection.name),
            )
            .description(selection.description)
            .meta(meta)
        })
        .collect::<Vec<_>>();
    Ok(Some(
        SessionConfigOption::select(
            SELECTION_CONFIG_ID,
            "Model / routing",
            selected.to_string(),
            choices,
        )
        .category(SessionConfigOptionCategory::Model),
    ))
}

fn application_content(content: &ContentBlock) -> Result<ApplicationContent, ApplicationError> {
    match content {
        ContentBlock::Text(text) => Ok(ApplicationContent::Text {
            text: text.text.clone(),
        }),
        ContentBlock::ResourceLink(resource) => Ok(ApplicationContent::Resource {
            uri: resource.uri.clone(),
            mime_type: resource.mime_type.clone(),
            text: None,
        }),
        _ => Err(ApplicationError::InvalidInput {
            message: "ACP content type is not advertised by this adapter".to_owned(),
        }),
    }
}

fn validate_cwd(snapshot: &SessionSnapshot, expected: &str) -> Result<(), ApplicationError> {
    if snapshot.session.working_directory == expected {
        return Ok(());
    }
    Err(ApplicationError::Conflict {
        message: "ACP cwd does not match the durable session working directory".to_owned(),
    })
}

fn reject_extra_workspace(paths: &[std::path::PathBuf]) -> Result<(), ApplicationError> {
    if paths.is_empty() {
        return Ok(());
    }
    Err(ApplicationError::InvalidInput {
        message: "ACP additionalDirectories are not represented by the application contract"
            .to_owned(),
    })
}

fn reject_mcp_servers(count: usize) -> Result<(), ApplicationError> {
    if count == 0 {
        return Ok(());
    }
    Err(ApplicationError::InvalidInput {
        message: "ACP MCP server provisioning is not represented by the application contract"
            .to_owned(),
    })
}

fn absolute_path(path: &Path) -> Result<String, ApplicationError> {
    if !path.is_absolute() {
        return Err(ApplicationError::InvalidInput {
            message: "ACP working directory must be absolute".to_owned(),
        });
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| ApplicationError::InvalidInput {
            message: "ACP working directory must be valid UTF-8".to_owned(),
        })
}

fn session_id(value: &str) -> Result<SessionId, ApplicationError> {
    SessionId::parse(value).map_err(|error| ApplicationError::InvalidInput {
        message: format!("invalid ACP session id: {error}"),
    })
}

fn require(capabilities: &Capabilities, value: &str) -> Result<(), ApplicationError> {
    let capability = ContractId::parse(value).expect("static application capability id is valid");
    capabilities.require(&capability)
}
