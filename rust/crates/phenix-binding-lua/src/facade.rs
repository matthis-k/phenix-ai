use super::*;
use phenix_application_interface::{
    types::{
        Acknowledged, Content, ElicitationRequest, ElicitationResponse, ExecutionChange,
        ExecutionState, ExecutionUpdate, InteractionHandlers, ModelSelectInput, Models, PageInput,
        PermissionRequest, PermissionResponse, PromptInput, PromptResult, Provenance,
        ReviewDecision, ReviewDecisionInput, ReviewRecord, RoutingProfiles, RoutingSelectInput,
        SessionCreateInput, SessionInfo, SessionInput, SessionProjection, SessionResumeInput,
        SessionSnapshot, SessionUpdate, SetInteractionHandlersInput,
    },
    Cancel as AppCancel, CloseSession as AppCloseSession, CreateSession as AppCreateSession,
    DecideReview as AppDecideReview, GetProvenance as AppGetProvenance,
    ListModels as AppListModels, ListRoutingProfiles as AppListRoutingProfiles,
    ListSessions as AppListSessions, Prompt as AppPrompt, RenameSession as AppRenameSession,
    ResumeSession as AppResumeSession, SelectModel as AppSelectModel,
    SelectRoutingProfile as AppSelectRoutingProfile,
    SetInteractionHandlers as AppSetInteractionHandlers,
};
use phenix_core::{
    CapabilityOwnerId, ModelId, ReferenceId, RoutingProfileId, SessionId, ValueCodec,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};

const SESSION_EVENT: &str = "phenix.application.session-update@1";
const EXECUTION_EVENT: &str = "phenix.application.execution-update@1";
const DEFAULT_PUMP_BUDGET: usize = 64;
const MAX_PUMP_BUDGET: usize = 1024;

#[derive(Clone)]
pub(super) struct FacadeClient {
    core: Rc<FacadeCore>,
}

#[derive(Clone)]
struct FacadeSessions {
    core: Rc<FacadeCore>,
}

#[derive(Clone)]
struct FacadeSession {
    core: Rc<FacadeCore>,
    id: SessionId,
}

struct FacadeCore {
    raw: Client,
    state: RefCell<FacadeState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FacadePhase {
    Connecting,
    Ready,
    Failed,
    Closed,
}

impl FacadePhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Closed => "closed",
        }
    }
}

struct FacadeState {
    phase: FacadePhase,
    error: Option<BindingError>,
    bootstrap: Option<Request>,
    permission_handler: Option<RegistryKey>,
    elicitation_handler: Option<RegistryKey>,
    permission_ref: Option<ReferenceId>,
    elicitation_ref: Option<ReferenceId>,
    events: VecDeque<FacadeEvent>,
    sessions: BTreeMap<String, SessionProjection>,
    session_info: BTreeMap<String, SessionInfo>,
    repairs: BTreeMap<String, Request>,
    repair_backlog: BTreeMap<String, Vec<SessionUpdate>>,
    models: BTreeMap<String, SelectionCache>,
    routing: BTreeMap<String, SelectionCache>,
    provenance: BTreeMap<(String, String), Provenance>,
    latest_execution: BTreeMap<String, String>,
}

#[derive(Clone, Default)]
struct SelectionCache {
    selected: Option<String>,
    names: BTreeMap<String, String>,
}

#[derive(Clone)]
enum FacadeEvent {
    Status,
    SessionSnapshot {
        projection: SessionProjection,
        reason: &'static str,
    },
    SessionUpdate(SessionUpdate),
    ExecutionUpdate(ExecutionUpdate),
}

struct FacadeRequest {
    core: Rc<FacadeCore>,
    request: Request,
    projection: RequestProjection,
    result: Option<Result<FacadeOutcome, BindingError>>,
}

#[derive(Clone, Copy)]
enum RequestProjection {
    SessionCreate,
    SessionList,
    SessionResume,
    SessionRename,
    Acknowledged,
    Prompt,
    Models,
    Routing,
    Provenance,
    Review,
}

#[derive(Clone)]
enum FacadeOutcome {
    Session(SessionInfo),
    SessionPage(phenix_application_interface::types::SessionList),
    SessionSnapshot(SessionSnapshot),
    SessionInfo(SessionInfo),
    Acknowledged(Acknowledged),
    Prompt(PromptResult),
    Models(Models),
    Routing(RoutingProfiles),
    Provenance(Provenance),
    Review(ReviewRecord),
}

enum InteractionReplyKind {
    Permission,
    Elicitation(ElicitationRequest),
}

struct InteractionReply {
    callback: Option<phenix_client_acp::ExtensionCallbackRequest>,
    kind: InteractionReplyKind,
}

pub(super) fn connect(lua: &Lua, options: Table) -> LuaResult<FacadeClient> {
    let interactions = options.get::<Option<Table>>("interactions")?;
    let permission_handler = interactions
        .as_ref()
        .and_then(|table| table.get::<Option<mlua::Function>>("permission").ok())
        .flatten()
        .map(|handler| lua.create_registry_value(handler))
        .transpose()?;
    let elicitation_handler = interactions
        .as_ref()
        .and_then(|table| table.get::<Option<mlua::Function>>("elicitation").ok())
        .flatten()
        .map(|handler| lua.create_registry_value(handler))
        .transpose()?;

    let raw = super::connect_raw(options)?;
    let owner = CapabilityOwnerId::Client(raw.state.owner.clone());
    let generation = raw.state.generation.clone();

    let permission_ref = permission_handler.as_ref().map(|_| {
        ReferenceId::parse("facade-permission").expect("static facade permission reference")
    });
    let elicitation_ref = elicitation_handler.as_ref().map(|_| {
        ReferenceId::parse("facade-elicitation").expect("static facade elicitation reference")
    });

    let handlers = InteractionHandlers {
        permission: permission_ref.as_ref().map(|id| {
            phenix_application_interface::types::PermissionHandlerRef(CallableRef::new(
                ContractId::parse("phenix.application.permission@1")
                    .expect("static permission contract"),
                owner.clone(),
                generation.clone(),
                id.clone(),
            ))
        }),
        elicitation: elicitation_ref.as_ref().map(|id| {
            phenix_application_interface::types::ElicitationHandlerRef(CallableRef::new(
                ContractId::parse("phenix.application.elicitation@1")
                    .expect("static elicitation contract"),
                owner,
                generation,
                id.clone(),
            ))
        }),
    };
    let operation = ContractId::parse(AppSetInteractionHandlers::ID)
        .expect("static interaction handler operation");
    let input = SetInteractionHandlersInput { handlers }.to_value();
    let bootstrap = request_for(&raw.state, None, |reply| Command::Application {
        operation,
        input,
        reply,
    })?;

    Ok(FacadeClient {
        core: Rc::new(FacadeCore {
            raw,
            state: RefCell::new(FacadeState {
                phase: FacadePhase::Connecting,
                error: None,
                bootstrap: Some(bootstrap),
                permission_handler,
                elicitation_handler,
                permission_ref,
                elicitation_ref,
                events: VecDeque::from([FacadeEvent::Status]),
                sessions: BTreeMap::new(),
                session_info: BTreeMap::new(),
                repairs: BTreeMap::new(),
                repair_backlog: BTreeMap::new(),
                models: BTreeMap::new(),
                routing: BTreeMap::new(),
                provenance: BTreeMap::new(),
                latest_execution: BTreeMap::new(),
            }),
        }),
    })
}

impl UserData for FacadeClient {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("status", |lua, this, ()| status_table(lua, &this.core));
        methods.add_method("features", |lua, this, ()| features_table(lua, &this.core));
        methods.add_method("sessions", |lua, this, ()| {
            lua.create_userdata(FacadeSessions {
                core: Rc::clone(&this.core),
            })
        });
        methods.add_method("pump", |lua, this, budget: Option<usize>| {
            pump(lua, &this.core, budget.unwrap_or(DEFAULT_PUMP_BUDGET))
        });
        methods.add_method(
            "decide_review",
            |lua, this, (review, decision): (Table, String)| {
                let review_id = review.get::<String>("id").map_err(|_| {
                    lua_error(BindingError::conversion("review requires string field id"))
                })?;
                let expected_revision = review.get::<u64>("revision").map_err(|_| {
                    lua_error(BindingError::conversion(
                        "review requires integer field revision",
                    ))
                })?;
                let decision = match decision.as_str() {
                    "accept" => ReviewDecision::Accept,
                    "reject" => ReviewDecision::Reject,
                    _ => {
                        return Err(lua_error(BindingError::conversion(
                            "review decision must be accept or reject",
                        )))
                    }
                };
                let request = application_request::<AppDecideReview>(
                    &this.core,
                    ReviewDecisionInput {
                        review_id,
                        expected_revision,
                        decision,
                    },
                    RequestProjection::Review,
                )?;
                lua.create_userdata(request)
            },
        );
        methods.add_method_mut("close", |_lua, this, ()| {
            let mut state = this.core.state.borrow_mut();
            state.phase = FacadePhase::Closed;
            state.events.push_back(FacadeEvent::Status);
            Ok(())
        });
    }
}

impl UserData for FacadeSessions {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("create", |lua, this, options: Table| {
            require_ready(&this.core)?;
            let working_directory = options
                .get::<String>("working_directory")
                .map_err(|_| {
                    lua_error(BindingError::conversion(
                        "session create requires working_directory",
                    ))
                })?;
            let title = options.get::<Option<String>>("title")?;
            let request = application_request::<AppCreateSession>(
                &this.core,
                SessionCreateInput {
                    working_directory,
                    title,
                },
                RequestProjection::SessionCreate,
            )?;
            lua.create_userdata(request)
        });
        methods.add_method("list", |lua, this, options: Option<Table>| {
            require_ready(&this.core)?;
            let cursor = options
                .as_ref()
                .map(|options| options.get::<Option<String>>("cursor"))
                .transpose()?
                .flatten();
            let request = application_request::<AppListSessions>(
                &this.core,
                PageInput { cursor },
                RequestProjection::SessionList,
            )?;
            lua.create_userdata(request)
        });
        methods.add_method("resume", |lua, this, session_id: String| {
            require_ready(&this.core)?;
            let session_id = parse_session_id(session_id)?;
            let request = application_request::<AppResumeSession>(
                &this.core,
                SessionResumeInput {
                    session_id,
                    after_sequence: None,
                },
                RequestProjection::SessionResume,
            )?;
            lua.create_userdata(request)
        });
        methods.add_method("cached", |lua, this, session_id: String| {
            let id = parse_session_id(session_id)?;
            let key = id.to_string();
            if !this.core.state.borrow().session_info.contains_key(&key) {
                return Ok(Value::Nil);
            }
            lua.create_userdata(FacadeSession {
                core: Rc::clone(&this.core),
                id,
            })
            .map(Value::UserData)
        });
    }
}

impl UserData for FacadeSession {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("id", |_lua, this, ()| Ok(this.id.to_string()));
        methods.add_method("info", |lua, this, ()| {
            let state = this.core.state.borrow();
            let Some(info) = state.session_info.get(&this.id.to_string()) else {
                return Ok(Value::Nil);
            };
            facade_value(lua, &info.to_value()).map_err(lua_error)
        });
        methods.add_method("projection", |lua, this, ()| {
            let state = this.core.state.borrow();
            let Some(projection) = state.sessions.get(&this.id.to_string()) else {
                return Ok(Value::Nil);
            };
            facade_value(lua, &projection.to_value()).map_err(lua_error)
        });
        methods.add_method("status", |lua, this, ()| {
            session_status_table(lua, &this.core, &this.id)
        });
        methods.add_method("prompt", |lua, this, content: Table| {
            require_ready(&this.core)?;
            let content = parse_content(content)?;
            let request = application_request::<AppPrompt>(
                &this.core,
                PromptInput {
                    session_id: this.id.clone(),
                    content,
                },
                RequestProjection::Prompt,
            )?;
            lua.create_userdata(request)
        });
        methods.add_method("cancel", |lua, this, ()| {
            require_ready(&this.core)?;
            let request = application_request::<AppCancel>(
                &this.core,
                SessionInput {
                    session_id: this.id.clone(),
                },
                RequestProjection::Acknowledged,
            )?;
            lua.create_userdata(request)
        });
        methods.add_method("close", |lua, this, ()| {
            require_ready(&this.core)?;
            let request = application_request::<AppCloseSession>(
                &this.core,
                SessionInput {
                    session_id: this.id.clone(),
                },
                RequestProjection::Acknowledged,
            )?;
            lua.create_userdata(request)
        });
        methods.add_method("rename", |lua, this, title: String| {
            require_ready(&this.core)?;
            let request = application_request::<AppRenameSession>(
                &this.core,
                phenix_application_interface::types::SessionRenameInput {
                    session_id: this.id.clone(),
                    title,
                },
                RequestProjection::SessionRename,
            )?;
            lua.create_userdata(request)
        });
        methods.add_method("models", |lua, this, ()| {
            require_ready(&this.core)?;
            let request = application_request::<AppListModels>(
                &this.core,
                SessionInput {
                    session_id: this.id.clone(),
                },
                RequestProjection::Models,
            )?;
            lua.create_userdata(request)
        });
        methods.add_method("select_model", |lua, this, model_id: String| {
            require_ready(&this.core)?;
            let model_id = ModelId::parse(model_id)
                .map_err(|error| lua_error(BindingError::conversion(error)))?;
            let request = application_request::<AppSelectModel>(
                &this.core,
                ModelSelectInput {
                    session_id: this.id.clone(),
                    model_id,
                },
                RequestProjection::Models,
            )?;
            lua.create_userdata(request)
        });
        methods.add_method("routing_profiles", |lua, this, ()| {
            require_ready(&this.core)?;
            let request = application_request::<AppListRoutingProfiles>(
                &this.core,
                SessionInput {
                    session_id: this.id.clone(),
                },
                RequestProjection::Routing,
            )?;
            lua.create_userdata(request)
        });
        methods.add_method(
            "select_routing_profile",
            |lua, this, profile_id: String| {
                require_ready(&this.core)?;
                let profile_id = RoutingProfileId::parse(profile_id)
                    .map_err(|error| lua_error(BindingError::conversion(error)))?;
                let request = application_request::<AppSelectRoutingProfile>(
                    &this.core,
                    RoutingSelectInput {
                        session_id: this.id.clone(),
                        profile_id,
                    },
                    RequestProjection::Routing,
                )?;
                lua.create_userdata(request)
            },
        );
        methods.add_method("provenance", |lua, this, execution_id: Option<String>| {
            require_ready(&this.core)?;
            let execution_id = match execution_id {
                Some(execution_id) => execution_id,
                None => this
                    .core
                    .state
                    .borrow()
                    .latest_execution
                    .get(&this.id.to_string())
                    .cloned()
                    .ok_or_else(|| {
                        lua_error(BindingError::conversion(
                            "session has no known execution for provenance",
                        ))
                    })?,
            };
            let request = application_request::<AppGetProvenance>(
                &this.core,
                phenix_application_interface::types::ExecutionInput {
                    session_id: this.id.clone(),
                    execution_id,
                },
                RequestProjection::Provenance,
            )?;
            lua.create_userdata(request)
        });
    }
}

impl UserData for FacadeRequest {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("poll", |lua, this, ()| this.poll_lua(lua));
    }
}

impl FacadeRequest {
    fn poll_lua(&mut self, lua: &Lua) -> LuaResult<MultiValue> {
        if self.result.is_none() {
            let Some(result) = self.request.poll() else {
                return Ok(MultiValue::from_vec(vec![Value::Boolean(false)]));
            };
            self.result = Some(match result {
                Ok(response) => decode_outcome(&self.core, self.projection, response),
                Err(error) => Err(error),
            });
        }

        match self.result.as_ref().expect("facade result set above") {
            Ok(outcome) => {
                let value = outcome_to_lua(lua, &self.core, outcome)?;
                Ok(MultiValue::from_vec(vec![Value::Boolean(true), value]))
            }
            Err(error) => Ok(MultiValue::from_vec(vec![
                Value::Boolean(true),
                Value::Nil,
                Value::Table(error_table(lua, error)?),
            ])),
        }
    }
}

impl UserData for InteractionReply {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("allow_once", |_lua, this, ()| {
            this.permission(PermissionResponse::AllowOnce)
        });
        methods.add_method_mut("deny", |_lua, this, ()| {
            this.permission(PermissionResponse::Deny)
        });
        methods.add_method_mut("cancel", |_lua, this, ()| this.cancel());
        methods.add_method_mut("decline", |_lua, this, ()| {
            this.elicitation(ElicitationResponse::Declined)
        });
        methods.add_method_mut("accept", |lua, this, value: Value| {
            let InteractionReplyKind::Elicitation(request) = &this.kind else {
                return Err(lua_error(BindingError::conversion(
                    "accept is only valid for elicitation replies",
                )));
            };
            let value = lua_to_phenix(lua, &request.schema, value).map_err(lua_error)?;
            let response = phenix_application_interface::types::normalize_elicitation_response(
                request,
                ElicitationResponse::Accepted { value },
            )
            .map_err(|error| lua_error(BindingError::conversion(error.to_string())))?;
            this.elicitation(response)
        });
    }
}

impl InteractionReply {
    fn permission(&mut self, response: PermissionResponse) -> LuaResult<()> {
        if !matches!(self.kind, InteractionReplyKind::Permission) {
            return Err(lua_error(BindingError::conversion(
                "permission response used for elicitation",
            )));
        }
        self.respond(response.to_value())
    }

    fn elicitation(&mut self, response: ElicitationResponse) -> LuaResult<()> {
        if !matches!(self.kind, InteractionReplyKind::Elicitation(_)) {
            return Err(lua_error(BindingError::conversion(
                "elicitation response used for permission",
            )));
        }
        self.respond(response.to_value())
    }

    fn cancel(&mut self) -> LuaResult<()> {
        match self.kind {
            InteractionReplyKind::Permission => self.permission(PermissionResponse::Cancelled),
            InteractionReplyKind::Elicitation(_) => {
                self.elicitation(ElicitationResponse::Cancelled)
            }
        }
    }

    fn respond(&mut self, output: PhenixValue) -> LuaResult<()> {
        let callback = self.callback.take().ok_or_else(|| {
            lua_error(BindingError::local(
                ErrorKind::Rejected,
                "interaction reply has already been settled",
            ))
        })?;
        callback.respond(Ok(CapabilityInvokeResult { output }.to_value()));
        Ok(())
    }

    fn fail(&mut self, message: impl Into<String>) {
        if let Some(callback) = self.callback.take() {
            callback.respond(Err(
                phenix_application_interface::types::ApplicationError::Failed {
                    message: message.into(),
                },
            ));
        }
    }
}

impl Drop for InteractionReply {
    fn drop(&mut self) {
        if self.callback.is_none() {
            return;
        }
        let output = match self.kind {
            InteractionReplyKind::Permission => PermissionResponse::Cancelled.to_value(),
            InteractionReplyKind::Elicitation(_) => ElicitationResponse::Cancelled.to_value(),
        };
        if let Some(callback) = self.callback.take() {
            callback.respond(Ok(CapabilityInvokeResult { output }.to_value()));
        }
    }
}

fn require_ready(core: &FacadeCore) -> LuaResult<()> {
    let state = core.state.borrow();
    match state.phase {
        FacadePhase::Ready => Ok(()),
        FacadePhase::Failed => Err(lua_error(
            state
                .error
                .clone()
                .unwrap_or_else(|| BindingError::transport("Phenix client failed")),
        )),
        FacadePhase::Closed => Err(lua_error(BindingError::transport("Phenix client is closed"))),
        FacadePhase::Connecting => Err(lua_error(BindingError::local(
            ErrorKind::Rejected,
            "Phenix client is still connecting",
        ))),
    }
}

fn application_request<O: Operation>(
    core: &Rc<FacadeCore>,
    input: O::Input,
    projection: RequestProjection,
) -> LuaResult<FacadeRequest>
where
    O::Input: ValueCodec,
{
    let operation = ContractId::parse(O::ID).expect("static application operation id");
    let request = request_for(&core.raw.state, None, |reply| Command::Application {
        operation,
        input: input.to_value(),
        reply,
    })?;
    Ok(FacadeRequest {
        core: Rc::clone(core),
        request,
        projection,
        result: None,
    })
}

fn decode_outcome(
    core: &Rc<FacadeCore>,
    projection: RequestProjection,
    response: Response,
) -> Result<FacadeOutcome, BindingError> {
    let Response::Application { value, .. } = response else {
        return Err(BindingError::conversion(
            "application facade received a non-application response",
        ));
    };
    let outcome = match projection {
        RequestProjection::SessionCreate => {
            let info = SessionInfo::from_value(&value)
                .map_err(|error| BindingError::conversion(error.to_string()))?;
            install_created_session(core, info.clone());
            FacadeOutcome::Session(info)
        }
        RequestProjection::SessionList => {
            let page = phenix_application_interface::types::SessionList::from_value(&value)
                .map_err(|error| BindingError::conversion(error.to_string()))?;
            {
                let mut state = core.state.borrow_mut();
                for info in &page.sessions {
                    state.session_info.insert(info.session_id.to_string(), info.clone());
                }
            }
            FacadeOutcome::SessionPage(page)
        }
        RequestProjection::SessionResume => {
            let snapshot = SessionSnapshot::from_value(&value)
                .map_err(|error| BindingError::conversion(error.to_string()))?;
            install_snapshot(core, snapshot.clone(), "resume");
            FacadeOutcome::SessionSnapshot(snapshot)
        }
        RequestProjection::SessionRename => {
            let info = SessionInfo::from_value(&value)
                .map_err(|error| BindingError::conversion(error.to_string()))?;
            let key = info.session_id.to_string();
            let mut state = core.state.borrow_mut();
            state.session_info.insert(key.clone(), info.clone());
            if let Some(projection) = state.sessions.get_mut(&key) {
                projection.session = info.clone();
            }
            state.events.push_back(FacadeEvent::Status);
            FacadeOutcome::SessionInfo(info)
        }
        RequestProjection::Acknowledged => FacadeOutcome::Acknowledged(
            Acknowledged::from_value(&value)
                .map_err(|error| BindingError::conversion(error.to_string()))?,
        ),
        RequestProjection::Prompt => {
            let result = PromptResult::from_value(&value)
                .map_err(|error| BindingError::conversion(error.to_string()))?;
            let session = latest_session_for_execution(core, &result.execution_id);
            if let Some(session) = session {
                core.state
                    .borrow_mut()
                    .latest_execution
                    .insert(session, result.execution_id.clone());
            }
            FacadeOutcome::Prompt(result)
        }
        RequestProjection::Models => {
            let models = Models::from_value(&value)
                .map_err(|error| BindingError::conversion(error.to_string()))?;
            cache_models(core, &models);
            FacadeOutcome::Models(models)
        }
        RequestProjection::Routing => {
            let routing = RoutingProfiles::from_value(&value)
                .map_err(|error| BindingError::conversion(error.to_string()))?;
            cache_routing(core, &routing);
            FacadeOutcome::Routing(routing)
        }
        RequestProjection::Provenance => {
            let provenance = Provenance::from_value(&value)
                .map_err(|error| BindingError::conversion(error.to_string()))?;
            cache_provenance(core, provenance.clone());
            FacadeOutcome::Provenance(provenance)
        }
        RequestProjection::Review => FacadeOutcome::Review(
            ReviewRecord::from_value(&value)
                .map_err(|error| BindingError::conversion(error.to_string()))?,
        ),
    };
    Ok(outcome)
}

fn latest_session_for_execution(core: &FacadeCore, execution_id: &str) -> Option<String> {
    let state = core.state.borrow();
    for (session_id, projection) in &state.sessions {
        if projection.updates.iter().any(|update| {
            matches!(
                &update.update,
                phenix_application_interface::types::SessionChange::Execution {
                    execution_id: candidate,
                    ..
                } if candidate == execution_id
            )
        }) {
            return Some(session_id.clone());
        }
    }
    None
}

fn outcome_to_lua(lua: &Lua, core: &Rc<FacadeCore>, outcome: &FacadeOutcome) -> LuaResult<Value> {
    match outcome {
        FacadeOutcome::Session(info) => lua
            .create_userdata(FacadeSession {
                core: Rc::clone(core),
                id: info.session_id.clone(),
            })
            .map(Value::UserData),
        FacadeOutcome::SessionSnapshot(snapshot) => lua
            .create_userdata(FacadeSession {
                core: Rc::clone(core),
                id: snapshot.session.session_id.clone(),
            })
            .map(Value::UserData),
        FacadeOutcome::SessionPage(page) => facade_value(lua, &page.to_value()).map_err(lua_error),
        FacadeOutcome::SessionInfo(info) => facade_value(lua, &info.to_value()).map_err(lua_error),
        FacadeOutcome::Acknowledged(value) => facade_value(lua, &value.to_value()).map_err(lua_error),
        FacadeOutcome::Prompt(value) => facade_value(lua, &value.to_value()).map_err(lua_error),
        FacadeOutcome::Models(value) => facade_value(lua, &value.to_value()).map_err(lua_error),
        FacadeOutcome::Routing(value) => facade_value(lua, &value.to_value()).map_err(lua_error),
        FacadeOutcome::Provenance(value) => facade_value(lua, &value.to_value()).map_err(lua_error),
        FacadeOutcome::Review(value) => facade_value(lua, &value.to_value()).map_err(lua_error),
    }
}

fn install_created_session(core: &FacadeCore, info: SessionInfo) {
    let key = info.session_id.to_string();
    let projection = SessionProjection {
        session: info.clone(),
        through_sequence: 0,
        updates: Vec::new(),
    };
    let mut state = core.state.borrow_mut();
    state.session_info.insert(key.clone(), info);
    state.sessions.insert(key, projection.clone());
    state.events.push_back(FacadeEvent::SessionSnapshot {
        projection,
        reason: "create",
    });
}

fn install_snapshot(core: &FacadeCore, snapshot: SessionSnapshot, reason: &'static str) {
    let key = snapshot.session.session_id.to_string();
    let projection = SessionProjection {
        session: snapshot.session.clone(),
        through_sequence: snapshot.through_sequence,
        updates: snapshot.updates.clone(),
    };
    {
        let mut state = core.state.borrow_mut();
        state.session_info.insert(key.clone(), snapshot.session);
        state.sessions.insert(key.clone(), projection.clone());
        state.events.push_back(FacadeEvent::SessionSnapshot {
            projection,
            reason,
        });
    }
    replay_backlog(core, &key);
}

fn replay_backlog(core: &FacadeCore, session_id: &str) {
    let mut backlog = core
        .state
        .borrow_mut()
        .repair_backlog
        .remove(session_id)
        .unwrap_or_default();
    backlog.sort_by_key(|update| update.sequence);
    for update in backlog {
        ingest_session_update(core, update);
    }
}

fn ingest_session_update(core: &FacadeCore, update: SessionUpdate) {
    let key = update.session_id.to_string();
    let mut repair = false;
    {
        let mut state = core.state.borrow_mut();
        let Some(projection) = state.sessions.get_mut(&key) else {
            state
                .repair_backlog
                .entry(key.clone())
                .or_default()
                .push(update);
            repair = true;
            drop(state);
            if repair {
                schedule_repair(core, &key);
            }
            return;
        };
        if update.sequence <= projection.through_sequence {
            return;
        }
        let expected = projection.through_sequence.saturating_add(1);
        if update.sequence != expected {
            state
                .repair_backlog
                .entry(key.clone())
                .or_default()
                .push(update);
            repair = true;
        } else {
            if let phenix_application_interface::types::SessionChange::Renamed { title } =
                &update.update
            {
                projection.session.title = Some(title.clone());
                state
                    .session_info
                    .insert(key.clone(), projection.session.clone());
            }
            if let phenix_application_interface::types::SessionChange::Execution {
                execution_id,
                ..
            } = &update.update
            {
                state
                    .latest_execution
                    .insert(key.clone(), execution_id.clone());
            }
            projection.through_sequence = update.sequence;
            projection.updates.push(update.clone());
            state.events.push_back(FacadeEvent::SessionUpdate(update));
        }
    }
    if repair {
        schedule_repair(core, &key);
    }
}

fn schedule_repair(core: &FacadeCore, session_id: &str) {
    if core.state.borrow().repairs.contains_key(session_id) {
        return;
    }
    let Ok(id) = SessionId::parse(session_id.to_owned()) else {
        fail_core(
            core,
            BindingError::conversion(format!("invalid session id in repair: {session_id}")),
        );
        return;
    };
    let operation = ContractId::parse(AppResumeSession::ID).expect("static resume operation");
    let input = SessionResumeInput {
        session_id: id,
        after_sequence: None,
    }
    .to_value();
    match request_for(&core.raw.state, None, |reply| Command::Application {
        operation,
        input,
        reply,
    }) {
        Ok(request) => {
            core.state
                .borrow_mut()
                .repairs
                .insert(session_id.to_owned(), request);
        }
        Err(error) => fail_core(
            core,
            BindingError::transport(format!("could not schedule session repair: {error}")),
        ),
    }
}

fn drive_repairs(core: &FacadeCore) {
    let repairs = std::mem::take(&mut core.state.borrow_mut().repairs);
    for (session_id, mut request) in repairs {
        match request.poll() {
            None => {
                core.state.borrow_mut().repairs.insert(session_id, request);
            }
            Some(Ok(Response::Application { value, .. })) => match SessionSnapshot::from_value(&value) {
                Ok(snapshot) => install_snapshot(core, snapshot, "repair"),
                Err(error) => fail_core(core, BindingError::conversion(error.to_string())),
            },
            Some(Ok(_)) => fail_core(
                core,
                BindingError::conversion("session repair returned a non-application response"),
            ),
            Some(Err(error)) => fail_core(core, error),
        }
    }
}

fn drive_bootstrap(core: &FacadeCore) {
    let bootstrap = core.state.borrow_mut().bootstrap.take();
    let Some(mut request) = bootstrap else {
        return;
    };
    match request.poll() {
        None => core.state.borrow_mut().bootstrap = Some(request),
        Some(Ok(_)) => {
            let missing = required_operations()
                .into_iter()
                .filter(|id| {
                    let id = ContractId::parse(*id).expect("static required operation");
                    !core.raw.state.supports_extension(&id).unwrap_or(false)
                })
                .collect::<Vec<_>>();
            if missing.is_empty() {
                let mut state = core.state.borrow_mut();
                state.phase = FacadePhase::Ready;
                state.events.push_back(FacadeEvent::Status);
            } else {
                fail_core(
                    core,
                    BindingError::local(
                        ErrorKind::UnsupportedCapability,
                        format!("application facade is missing operations: {}", missing.join(", ")),
                    ),
                );
            }
        }
        Some(Err(error)) => fail_core(core, error),
    }
}

fn required_operations() -> [&'static str; 8] {
    [
        AppCreateSession::ID,
        AppListSessions::ID,
        AppResumeSession::ID,
        AppCloseSession::ID,
        AppPrompt::ID,
        AppCancel::ID,
        AppSetInteractionHandlers::ID,
        AppDecideReview::ID,
    ]
}

fn pump(lua: &Lua, core: &Rc<FacadeCore>, budget: usize) -> LuaResult<Table> {
    let budget = budget.clamp(1, MAX_PUMP_BUDGET);
    drive_bootstrap(core);
    drive_repairs(core);

    if let Some(error) = core.raw.state.terminal_error.lock().ok().and_then(|error| error.clone()) {
        if core.state.borrow().phase != FacadePhase::Failed {
            fail_core(core, error);
        }
    }

    for _ in 0..budget {
        let notification = core
            .raw
            .state
            .updates
            .lock()
            .map_err(|_| lua_error(BindingError::transport("ACP update queue lock is poisoned")))?
            .try_recv();
        match notification {
            Ok(_) | Err(std_mpsc::TryRecvError::Empty) => {}
            Err(std_mpsc::TryRecvError::Disconnected) => break,
        }

        let extension = core
            .raw
            .state
            .extension_updates
            .lock()
            .map_err(|_| {
                lua_error(BindingError::transport(
                    "ACP extension event queue lock is poisoned",
                ))
            })?
            .try_recv();
        match extension {
            Ok(event) => ingest_application_event(core, event),
            Err(std_mpsc::TryRecvError::Empty) => {}
            Err(std_mpsc::TryRecvError::Disconnected) => break,
        }

        let callback = core
            .raw
            .state
            .callbacks
            .lock()
            .map_err(|_| lua_error(BindingError::transport("callback queue lock is poisoned")))?
            .try_recv();
        match callback {
            Ok(callback) => dispatch_callback(lua, core, callback)?,
            Err(std_mpsc::TryRecvError::Empty) => {}
            Err(std_mpsc::TryRecvError::Disconnected) => break,
        }
    }

    drive_repairs(core);
    let events = lua.create_table()?;
    let mut state = core.state.borrow_mut();
    for index in 1..=budget {
        let Some(event) = state.events.pop_front() else {
            break;
        };
        events.set(index, event_to_lua(lua, core, &event)?)?;
    }
    Ok(events)
}

fn ingest_application_event(core: &FacadeCore, event: ApplicationEvent) {
    match event.event.as_str() {
        SESSION_EVENT => match SessionUpdate::from_value(&event.payload) {
            Ok(update) => ingest_session_update(core, update),
            Err(error) => fail_core(core, BindingError::conversion(error.to_string())),
        },
        EXECUTION_EVENT => match ExecutionUpdate::from_value(&event.payload) {
            Ok(update) => core
                .state
                .borrow_mut()
                .events
                .push_back(FacadeEvent::ExecutionUpdate(update)),
            Err(error) => fail_core(core, BindingError::conversion(error.to_string())),
        },
        _ => {}
    }
}

fn dispatch_callback(
    lua: &Lua,
    core: &Rc<FacadeCore>,
    callback: phenix_client_acp::ExtensionCallbackRequest,
) -> LuaResult<()> {
    let invocation = CapabilityInvokeInput::from_value(&callback.input)
        .map_err(|error| lua_error(BindingError::conversion(error.to_string())))?;
    let callable = invocation
        .callable
        .callable()
        .map_err(|error| lua_error(BindingError::conversion(error.to_string())))?;
    if callable.owner() != &CapabilityOwnerId::Client(core.raw.state.owner.clone())
        || callable.generation() != &core.raw.state.generation
    {
        callback.respond(Err(
            phenix_application_interface::types::ApplicationError::Failed {
                message: "callback refers to a stale or foreign facade callable".to_owned(),
            },
        ));
        return Ok(());
    }

    let (permission_ref, elicitation_ref) = {
        let state = core.state.borrow();
        (state.permission_ref.clone(), state.elicitation_ref.clone())
    };
    if permission_ref.as_ref() == Some(callable.id()) {
        return dispatch_permission(lua, core, callback, invocation.input);
    }
    if elicitation_ref.as_ref() == Some(callable.id()) {
        return dispatch_elicitation(lua, core, callback, invocation.input);
    }
    dispatch_local_callback(
        lua,
        &core.raw.state,
        &core.raw.local_callables,
        callback,
    )
}

fn dispatch_permission(
    lua: &Lua,
    core: &FacadeCore,
    callback: phenix_client_acp::ExtensionCallbackRequest,
    value: PhenixValue,
) -> LuaResult<()> {
    let request = PermissionRequest::from_value(&value)
        .map_err(|error| lua_error(BindingError::conversion(error.to_string())))?;
    let handler = {
        let state = core.state.borrow();
        let key = state.permission_handler.as_ref().ok_or_else(|| {
            lua_error(BindingError::conversion("permission handler is not installed"))
        })?;
        lua.registry_value::<mlua::Function>(key)?
    };
    let argument = facade_value(lua, &request.to_value()).map_err(lua_error)?;
    let reply = lua.create_userdata(InteractionReply {
        callback: Some(callback),
        kind: InteractionReplyKind::Permission,
    })?;
    let result: LuaResult<()> = handler.call((argument, reply.clone()));
    if let Err(error) = result {
        reply.borrow_mut::<InteractionReply>()?.fail(error.to_string());
        return Err(error);
    }
    Ok(())
}

fn dispatch_elicitation(
    lua: &Lua,
    core: &FacadeCore,
    callback: phenix_client_acp::ExtensionCallbackRequest,
    value: PhenixValue,
) -> LuaResult<()> {
    let request = ElicitationRequest::from_value(&value)
        .map_err(|error| lua_error(BindingError::conversion(error.to_string())))?;
    let handler = {
        let state = core.state.borrow();
        let key = state.elicitation_handler.as_ref().ok_or_else(|| {
            lua_error(BindingError::conversion("elicitation handler is not installed"))
        })?;
        lua.registry_value::<mlua::Function>(key)?
    };
    let argument = lua.create_table()?;
    argument.set("session_id", request.session_id.to_string())?;
    argument.set("message", request.message.as_str())?;
    argument.set("form", form_schema(lua, &request.schema).map_err(lua_error)?)?;
    let reply = lua.create_userdata(InteractionReply {
        callback: Some(callback),
        kind: InteractionReplyKind::Elicitation(request),
    })?;
    let result: LuaResult<()> = handler.call((argument, reply.clone()));
    if let Err(error) = result {
        reply.borrow_mut::<InteractionReply>()?.fail(error.to_string());
        return Err(error);
    }
    Ok(())
}

fn form_schema(lua: &Lua, schema: &Type) -> Result<Table, BindingError> {
    let result = lua
        .create_table()
        .map_err(|error| BindingError::conversion(error.to_string()))?;
    match schema {
        Type::String => result.set("kind", "string"),
        Type::Bool => result.set("kind", "boolean"),
        Type::I64 => {
            result.set("kind", "integer")?;
            result.set("signed", true)
        }
        Type::U64 => {
            result.set("kind", "integer")?;
            result.set("signed", false)
        }
        Type::F64 => result.set("kind", "number"),
        Type::Option(item) if is_form_scalar(item) => {
            let inner = form_schema(lua, item)?;
            for pair in inner.pairs::<Value, Value>() {
                let (key, value) = pair
                    .map_err(|error| BindingError::conversion(error.to_string()))?;
                result
                    .set(key, value)
                    .map_err(|error| BindingError::conversion(error.to_string()))?;
            }
            result.set("optional", true)
        }
        Type::Table(fields) => {
            result.set("kind", "object")?;
            let output = lua
                .create_table()
                .map_err(|error| BindingError::conversion(error.to_string()))?;
            for (index, (name, schema)) in fields.iter().enumerate() {
                let field = lua
                    .create_table()
                    .map_err(|error| BindingError::conversion(error.to_string()))?;
                field.set("name", name.as_str())?;
                field.set("schema", form_schema(lua, schema)?)?;
                output.set(index + 1, field)?;
            }
            result.set("fields", output)
        }
        Type::Variant(variants)
            if variants.values().all(|schema| matches!(schema, Type::Unit)) =>
        {
            result.set("kind", "enum")?;
            let options = lua
                .create_table()
                .map_err(|error| BindingError::conversion(error.to_string()))?;
            for (index, name) in variants.keys().enumerate() {
                options.set(index + 1, name.as_str())?;
            }
            result.set("options", options)
        }
        Type::List(item) if is_form_scalar(item) || is_unit_variant(item) => {
            result.set("kind", "list")?;
            result.set("item", form_schema(lua, item)?)
        }
        _ => {
            return Err(BindingError::conversion(format!(
                "unsupported_schema: elicitation schema {} is not supported by the facade",
                schema.kind()
            )))
        }
    }
    .map_err(|error| BindingError::conversion(error.to_string()))?;
    Ok(result)
}

fn is_form_scalar(schema: &Type) -> bool {
    matches!(schema, Type::String | Type::Bool | Type::I64 | Type::U64 | Type::F64)
}

fn is_unit_variant(schema: &Type) -> bool {
    matches!(schema, Type::Variant(variants) if variants.values().all(|schema| matches!(schema, Type::Unit)))
}

fn event_to_lua(lua: &Lua, core: &FacadeCore, event: &FacadeEvent) -> LuaResult<Table> {
    let result = lua.create_table()?;
    match event {
        FacadeEvent::Status => {
            result.set("kind", "status")?;
            result.set("data", status_table(lua, core)?)?;
        }
        FacadeEvent::SessionSnapshot { projection, reason } => {
            result.set("kind", "session_snapshot")?;
            let data = facade_value(lua, &projection.to_value()).map_err(lua_error)?;
            if let Value::Table(data) = &data {
                data.set("reason", *reason)?;
            }
            result.set("data", data)?;
        }
        FacadeEvent::SessionUpdate(update) => {
            result.set("kind", "session_update")?;
            result.set(
                "data",
                facade_value(lua, &update.to_value()).map_err(lua_error)?,
            )?;
        }
        FacadeEvent::ExecutionUpdate(update) => {
            result.set("kind", "execution_update")?;
            result.set(
                "data",
                facade_value(lua, &update.to_value()).map_err(lua_error)?,
            )?;
        }
    }
    Ok(result)
}

fn status_table(lua: &Lua, core: &FacadeCore) -> LuaResult<Table> {
    let state = core.state.borrow();
    let result = lua.create_table()?;
    result.set("state", state.phase.as_str())?;
    if let Some(error) = &state.error {
        result.set("error", error_table(lua, error)?)?;
    }
    Ok(result)
}

fn session_status_table(lua: &Lua, core: &FacadeCore, id: &SessionId) -> LuaResult<Table> {
    let key = id.to_string();
    let state = core.state.borrow();
    let result = lua.create_table()?;
    result.set("session_id", key.as_str())?;
    if let Some(info) = state.session_info.get(&key) {
        if let Some(title) = &info.title {
            result.set("title", title.as_str())?;
        }
        result.set("working_directory", info.working_directory.as_str())?;
    }

    let (execution_id, execution_state) = state
        .sessions
        .get(&key)
        .map(latest_execution_state)
        .unwrap_or((None, None));
    let execution_id = execution_id.or_else(|| state.latest_execution.get(&key).cloned());
    if let Some(execution_id) = &execution_id {
        result.set("execution_id", execution_id.as_str())?;
    }
    if let Some(execution_state) = execution_state {
        result.set("execution_state", execution_state_label(execution_state))?;
        result.set("settled", execution_settled(execution_state))?;
    } else {
        result.set("settled", execution_id.is_none())?;
    }

    let provenance = execution_id
        .as_ref()
        .and_then(|execution_id| state.provenance.get(&(key.clone(), execution_id.clone())));
    let selected_model = state.models.get(&key).and_then(|cache| cache.selected.as_ref());
    let effective_model = provenance.and_then(|provenance| provenance.model_id.as_ref());
    let model_id = effective_model
        .map(ToString::to_string)
        .or_else(|| selected_model.cloned());
    if let Some(model_id) = model_id {
        result.set("model_id", model_id.as_str())?;
        if let Some(name) = state
            .models
            .get(&key)
            .and_then(|cache| cache.names.get(&model_id))
        {
            result.set("model_name", name.as_str())?;
        }
        result.set("model_effective", effective_model.is_some())?;
    }

    let selected_routing = state.routing.get(&key).and_then(|cache| cache.selected.as_ref());
    let effective_routing = provenance.and_then(|provenance| provenance.routing_profile.as_ref());
    let routing_id = effective_routing
        .map(ToString::to_string)
        .or_else(|| selected_routing.cloned());
    if let Some(routing_id) = routing_id {
        result.set("routing_profile_id", routing_id.as_str())?;
        if let Some(name) = state
            .routing
            .get(&key)
            .and_then(|cache| cache.names.get(&routing_id))
        {
            result.set("routing_profile_name", name.as_str())?;
        }
        result.set("routing_effective", effective_routing.is_some())?;
    }
    Ok(result)
}

fn latest_execution_state(
    projection: &SessionProjection,
) -> (Option<String>, Option<&ExecutionState>) {
    let mut execution_id = None;
    for update in projection.updates.iter().rev() {
        let phenix_application_interface::types::SessionChange::Execution {
            execution_id: candidate,
            update,
        } = &update.update
        else {
            continue;
        };
        if execution_id.is_none() {
            execution_id = Some(candidate.clone());
        }
        if execution_id.as_deref() == Some(candidate.as_str()) {
            if let ExecutionChange::State { state } = update {
                return (execution_id, Some(state));
            }
        }
    }
    (execution_id, None)
}

fn execution_state_label(state: &ExecutionState) -> &'static str {
    match state {
        ExecutionState::Pending => "pending",
        ExecutionState::Running => "running",
        ExecutionState::Completed => "completed",
        ExecutionState::Cancelled => "cancelled",
        ExecutionState::Failed { .. } => "failed",
    }
}

fn execution_settled(state: &ExecutionState) -> bool {
    !matches!(state, ExecutionState::Pending | ExecutionState::Running)
}

fn features_table(lua: &Lua, core: &FacadeCore) -> LuaResult<Table> {
    let result = lua.create_table()?;
    for (name, operation) in [
        ("models", AppListModels::ID),
        ("routing", AppListRoutingProfiles::ID),
        ("provenance", AppGetProvenance::ID),
        ("review", AppDecideReview::ID),
    ] {
        let operation = ContractId::parse(operation).expect("static feature operation");
        result.set(
            name,
            core.raw
                .state
                .supports_extension(&operation)
                .map_err(lua_error)?,
        )?;
    }
    Ok(result)
}

fn cache_models(core: &FacadeCore, models: &Models) {
    let mut state = core.state.borrow_mut();
    let Some(session_id) = most_recent_session(&state) else {
        return;
    };
    state.models.insert(
        session_id,
        SelectionCache {
            selected: models.selected.as_ref().map(ToString::to_string),
            names: models
                .available
                .iter()
                .map(|model| (model.id.to_string(), model.name.clone()))
                .collect(),
        },
    );
    state.events.push_back(FacadeEvent::Status);
}

fn cache_routing(core: &FacadeCore, routing: &RoutingProfiles) {
    let mut state = core.state.borrow_mut();
    let Some(session_id) = most_recent_session(&state) else {
        return;
    };
    state.routing.insert(
        session_id,
        SelectionCache {
            selected: routing.selected.as_ref().map(ToString::to_string),
            names: routing
                .available
                .iter()
                .map(|profile| (profile.id.to_string(), profile.name.clone()))
                .collect(),
        },
    );
    state.events.push_back(FacadeEvent::Status);
}

fn most_recent_session(state: &FacadeState) -> Option<String> {
    state
        .latest_execution
        .keys()
        .next_back()
        .cloned()
        .or_else(|| state.sessions.keys().next_back().cloned())
        .or_else(|| state.session_info.keys().next_back().cloned())
}

fn cache_provenance(core: &FacadeCore, provenance: Provenance) {
    let mut state = core.state.borrow_mut();
    let session_id = state
        .latest_execution
        .iter()
        .find_map(|(session_id, execution_id)| {
            (execution_id == &provenance.execution_id).then(|| session_id.clone())
        });
    if let Some(session_id) = session_id {
        state.provenance.insert(
            (session_id, provenance.execution_id.clone()),
            provenance,
        );
        state.events.push_back(FacadeEvent::Status);
    }
}

fn parse_session_id(value: String) -> LuaResult<SessionId> {
    SessionId::parse(value).map_err(|error| lua_error(BindingError::conversion(error)))
}

fn parse_content(content: Table) -> LuaResult<Vec<Content>> {
    content
        .sequence_values::<Table>()
        .map(|item| {
            let item = item?;
            let kind = item.get::<String>("kind").map_err(|_| {
                lua_error(BindingError::conversion(
                    "prompt content item requires string field kind",
                ))
            })?;
            match kind.as_str() {
                "text" => Ok(Content::Text {
                    text: item.get::<String>("text")?,
                }),
                "resource" => Ok(Content::Resource {
                    uri: item.get::<String>("uri")?,
                    mime_type: item.get::<Option<String>>("mime_type")?,
                    text: item.get::<Option<String>>("text")?,
                }),
                "image" => {
                    let data = item.get::<mlua::String>("data")?;
                    Ok(Content::Image {
                        mime_type: item.get::<String>("mime_type")?,
                        data: data.as_bytes().to_vec(),
                    })
                }
                _ => Err(lua_error(BindingError::conversion(format!(
                    "unsupported prompt content kind {kind}"
                )))),
            }
        })
        .collect()
}

fn error_table(lua: &Lua, error: &BindingError) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("kind", error.kind.as_str())?;
    table.set("code", error.code.as_str())?;
    table.set("message", error.message.as_str())?;
    if let Some(details) = error.details.as_ref().as_ref().filter(|value| !value.is_null()) {
        table.set("details", lua.to_value(details)?)?;
    }
    Ok(table)
}

fn fail_core(core: &FacadeCore, error: BindingError) {
    let mut state = core.state.borrow_mut();
    state.phase = FacadePhase::Failed;
    state.error = Some(error);
    state.events.push_back(FacadeEvent::Status);
}

fn facade_value<'lua>(lua: &'lua Lua, value: &PhenixValue) -> Result<Value<'lua>, BindingError> {
    match value {
        PhenixValue::Unit => Ok(Value::Nil),
        PhenixValue::Bool(value) => Ok(Value::Boolean(*value)),
        PhenixValue::I64(value) => Ok(Value::Integer(*value)),
        PhenixValue::U64(value) if *value <= i64::MAX as u64 => Ok(Value::Integer(*value as i64)),
        PhenixValue::U64(_) => Err(BindingError::conversion(
            "u64 value exceeds Lua 5.1 integer range",
        )),
        PhenixValue::F64(value) if value.is_finite() => Ok(Value::Number(*value)),
        PhenixValue::F64(_) => Err(BindingError::conversion(
            "non-finite float cannot cross the Lua boundary",
        )),
        PhenixValue::String(value) => lua
            .create_string(value)
            .map(Value::String)
            .map_err(|error| BindingError::conversion(error.to_string())),
        PhenixValue::Bytes(value) => lua
            .create_string(value)
            .map(Value::String)
            .map_err(|error| BindingError::conversion(error.to_string())),
        PhenixValue::Option(None) => Ok(Value::Nil),
        PhenixValue::Option(Some(value)) => facade_value(lua, value),
        PhenixValue::List(values) => {
            let table = lua
                .create_table()
                .map_err(|error| BindingError::conversion(error.to_string()))?;
            for (index, value) in values.iter().enumerate() {
                table
                    .set(index + 1, facade_value(lua, value)?)
                    .map_err(|error| BindingError::conversion(error.to_string()))?;
            }
            Ok(Value::Table(table))
        }
        PhenixValue::Map(values) => {
            let table = lua
                .create_table()
                .map_err(|error| BindingError::conversion(error.to_string()))?;
            for (key, value) in values {
                table
                    .set(key.as_str(), facade_value(lua, value)?)
                    .map_err(|error| BindingError::conversion(error.to_string()))?;
            }
            Ok(Value::Table(table))
        }
        PhenixValue::Table(values) => {
            let table = lua
                .create_table()
                .map_err(|error| BindingError::conversion(error.to_string()))?;
            for (key, value) in values {
                table
                    .set(key.as_str(), facade_value(lua, value)?)
                    .map_err(|error| BindingError::conversion(error.to_string()))?;
            }
            Ok(Value::Table(table))
        }
        PhenixValue::Variant { tag, value } => {
            let table = lua
                .create_table()
                .map_err(|error| BindingError::conversion(error.to_string()))?;
            table
                .set("kind", snake_case(tag.as_str()))
                .map_err(|error| BindingError::conversion(error.to_string()))?;
            match value.as_ref() {
                PhenixValue::Unit => {}
                PhenixValue::Table(fields) => {
                    for (key, value) in fields {
                        table
                            .set(key.as_str(), facade_value(lua, value)?)
                            .map_err(|error| BindingError::conversion(error.to_string()))?;
                    }
                }
                value => table
                    .set("value", facade_value(lua, value)?)
                    .map_err(|error| BindingError::conversion(error.to_string()))?,
            }
            Ok(Value::Table(table))
        }
        PhenixValue::Callable(_) | PhenixValue::Object(_) => Err(BindingError::conversion(
            "opaque capability references are not exposed by the application facade",
        )),
    }
}

fn snake_case(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 4);
    for (index, character) in value.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index != 0 {
                output.push('_');
            }
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}
