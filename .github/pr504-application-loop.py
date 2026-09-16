from pathlib import Path

path = Path('rust/crates/phenix-harness/src/application.rs')
text = path.read_text()
if 'model tool turn limit exceeded' in text:
    raise SystemExit(0)


def replace(old: str, new: str, count: int = 1) -> None:
    global text
    if old not in text:
        raise SystemExit(f'application.rs source shape changed: {old[:160]!r}')
    text = text.replace(old, new, count)


replace(
    '''use phenix_acp_stdio::{
    model_tool_surface, serve_stdio_with_events_and_callbacks, ApplicationEvent,
    ApplicationInvocation, ChannelTransport, ClientCapabilityCallbacks, ClientCapabilityIdentity,
    SdkApplicationService,
};''',
    '''use phenix_acp_stdio::{
    execute_admitted_client_tool_call, model_tool_surface, serve_stdio_with_events_and_callbacks,
    ApplicationEvent, ApplicationInvocation, ChannelTransport, ClientCapabilityCallbacks,
    ClientCapabilityIdentity, SdkApplicationService,
};''',
)
replace(
    '''        Acknowledged, ApplicationError, Content, ElicitationHandlerRef, ExecutionChange,
        ExecutionState, InteractionHandlers, Message, MessageRole, PageInput, PermissionHandlerRef,
        PromptInput, PromptResult, ReviewDecisionInput, ReviewRecord, SessionChange,''',
    '''        Acknowledged, ApplicationError, CapabilityInvokeInput, CapabilityInvokeResult, Content,
        ElicitationHandlerRef, ExecutionChange, ExecutionState, InteractionHandlers, Message,
        MessageRole, PageInput, PermissionHandlerRef, PermissionRequest, PermissionResponse,
        PromptInput, PromptResult, ReviewDecisionInput, ReviewRecord, SessionChange,''',
)
replace(
    '''    HasPhenixSchema, LocalPersistence, ModelToolDescriptor, ObservableError,
    ObservableRegistration, ObservableStore, PhenixContract, PhenixValue, PluginId, Project,''',
    '''    HasPhenixSchema, LocalPersistence, ModelToolDescriptor, ModelToolResult, ModelToolTurn,
    ObservableError, ObservableRegistration, ObservableStore, PhenixContract, PhenixValue, PluginId,
    Project,''',
)

replace(
    '''struct ExecutionCompletion {
    session_id: SessionId,
    execution_id: String,
    result: Result<String, ApplicationError>,
}
''',
    '''struct ExecutionProgress {
    session_id: SessionId,
    execution_id: String,
    change: ExecutionChange,
}

struct ExecutionCompletion {
    session_id: SessionId,
    execution_id: String,
    result: Result<String, ApplicationError>,
}

enum ExecutionWorkerEvent {
    Progress(ExecutionProgress),
    Complete(ExecutionCompletion),
}
''',
)

replace(
    '''    let (completion_sender, mut completions) =
        mpsc::channel::<ExecutionCompletion>(APPLICATION_EXECUTION_CAPACITY);''',
    '''    let (execution_sender, mut execution_events) =
        mpsc::channel::<ExecutionWorkerEvent>(APPLICATION_EXECUTION_CAPACITY);''',
)
replace(
    '''                    start_prompt(&mut worker, &service, &completion_sender, &mut active, invocation);''',
    '''                    start_prompt(&mut worker, &service, &execution_sender, &mut active, invocation);''',
)
replace(
    '''            completion = completions.recv(), if !active.is_empty() => {
                if let Some(completion) = completion {
                    finish_prompt(&mut worker, &mut active, completion);
                }
            }''',
    '''            event = execution_events.recv(), if !active.is_empty() => {
                if let Some(event) = event {
                    match event {
                        ExecutionWorkerEvent::Progress(progress) => {
                            handle_execution_progress(&mut worker, &mut active, progress);
                        }
                        ExecutionWorkerEvent::Complete(completion) => {
                            finish_prompt(&mut worker, &mut active, completion);
                        }
                    }
                }
            }''',
)

replace(
    '''    completion_sender: &mpsc::Sender<ExecutionCompletion>,''',
    '''    execution_sender: &mpsc::Sender<ExecutionWorkerEvent>,''',
)
replace(
    '''    let harness = Arc::clone(&worker.harness);
    let authority = worker.authority.clone();
    let session_id = request.session_id;
    let execution_id = prompt.execution_id;
    let runtime_execution_id = execution_id.clone();
    let sender = completion_sender.clone();
    tokio::spawn(async move {
        let blocking_cancellation = Arc::clone(&cancellation);
        let result = tokio::task::spawn_blocking(move || {
            run_agent_execution(
                harness,
                authority,
                runtime_execution_id,
                model_input,
                tools,
                blocking_cancellation,
            )
        })
        .await
        .map_err(|error| ApplicationError::Failed {
            message: format!("application execution worker failed: {error}"),
        })
        .and_then(|result| result);
        let _ = sender
            .send(ExecutionCompletion {
                session_id,
                execution_id,
                result,
            })
            .await;
    });''',
    '''    let harness = Arc::clone(&worker.harness);
    let authority = worker.authority.clone();
    let permission_handler = worker.interaction_handlers().permission.clone();
    let application_service = service.clone();
    let session_id = request.session_id;
    let execution_id = prompt.execution_id;
    let runtime_execution_id = execution_id.clone();
    let sender = execution_sender.clone();
    tokio::spawn(async move {
        let blocking_cancellation = Arc::clone(&cancellation);
        let progress_sender = sender.clone();
        let execution_session = session_id.clone();
        let result = tokio::task::spawn_blocking(move || {
            run_agent_execution(
                harness,
                authority,
                application_service,
                permission_handler,
                execution_session,
                runtime_execution_id,
                model_input,
                tools,
                blocking_cancellation,
                progress_sender,
            )
        })
        .await
        .map_err(|error| ApplicationError::Failed {
            message: format!("application execution worker failed: {error}"),
        })
        .and_then(|result| result);
        let _ = sender
            .send(ExecutionWorkerEvent::Complete(ExecutionCompletion {
                session_id,
                execution_id,
                result,
            }))
            .await;
    });''',
)

marker = '''fn finish_prompt(
    worker: &mut ApplicationWorker,
'''
insert = '''fn handle_execution_progress(
    worker: &mut ApplicationWorker,
    active: &mut BTreeMap<String, ActiveExecution>,
    progress: ExecutionProgress,
) {
    let key = progress.session_id.as_str().to_owned();
    let Some(execution) = active.get(&key) else {
        return;
    };
    if execution.execution_id != progress.execution_id {
        return;
    }
    if let Err(error) = worker.append_execution_change(
        &progress.session_id,
        &progress.execution_id,
        progress.change,
    ) {
        let Some(execution) = active.remove(&key) else {
            return;
        };
        execution.cancellation.store(true, Ordering::Release);
        let _ = worker.finish_root_execution(&execution.execution_id, false);
        execution.prompt.respond(Err(error));
    }
}

'''
if marker not in text:
    raise SystemExit('finish_prompt marker changed')
text = text.replace(marker, insert + marker, 1)

old_start = text.index('fn run_agent_execution(')
old_end = text.index('\nfn is_sdk_operation(', old_start)
new_run = '''fn run_agent_execution(
    harness: Arc<Mutex<PhenixHarness>>,
    authority: Authority,
    service: SdkApplicationService,
    permission_handler: Option<PermissionHandlerRef>,
    session_id: SessionId,
    execution_id: String,
    input: Bytes,
    tools: Vec<ModelToolDescriptor>,
    cancellation: Arc<AtomicBool>,
    progress_sender: mpsc::Sender<ExecutionWorkerEvent>,
) -> Result<String, ApplicationError> {
    let callable_id =
        CallableId::parse(DEFAULT_APPLICATION_AGENT).map_err(|error| ApplicationError::Failed {
            message: format!("invalid application agent id: {error}"),
        })?;
    let mut continuation = Vec::<ModelToolTurn>::new();

    // The root execution budget also caps provider attempts at 16. Keep the
    // application loop bounded explicitly so a backend cannot evade that
    // invariant by repeatedly returning tool calls without terminal output.
    for _ in 0..16 {
        if cancellation.load(Ordering::Acquire) {
            return Err(ApplicationError::Cancelled);
        }
        let command = AgentLoopCommand::Run {
            execution_id: execution_id.clone(),
            parent_attempt_id: None,
            callable_id: Some(callable_id.clone()),
            input: input.clone(),
            tools: tools.clone(),
            continuation: continuation.clone(),
        };
        let encoded = serde_json::to_vec(&PhenixValue::from(&command)).map_err(|error| {
            ApplicationError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        let output = harness
            .lock()
            .invoke(&agent_loop_service(), &encoded, &authority, None)
            .map_err(|error| ApplicationError::Failed {
                message: error.to_string(),
            })?;
        if cancellation.load(Ordering::Acquire) {
            return Err(ApplicationError::Cancelled);
        }
        let value: PhenixValue =
            serde_json::from_slice(&output).map_err(|error| ApplicationError::InvalidResponse {
                message: error.to_string(),
            })?;
        let response = AgentLoopResponse::try_from(Project(&value)).map_err(|error| {
            ApplicationError::InvalidResponse {
                message: error.to_string(),
            }
        })?;
        let AgentLoopResponse::Completed {
            output,
            tool_calls,
            ..
        } = response;
        if tool_calls.is_empty() {
            return String::from_utf8(output.as_ref().to_vec()).map_err(|error| {
                ApplicationError::InvalidResponse {
                    message: format!("agent output is not UTF-8: {error}"),
                }
            });
        }
        if tool_calls.len() > 10 {
            return Err(ApplicationError::Conflict {
                message: format!(
                    "agent returned {} tool calls; the per-turn limit is 10",
                    tool_calls.len()
                ),
            });
        }

        let mut tool_results = Vec::with_capacity(tool_calls.len());
        for call in &tool_calls {
            if cancellation.load(Ordering::Acquire) {
                return Err(ApplicationError::Cancelled);
            }
            send_execution_progress(
                &progress_sender,
                &session_id,
                &execution_id,
                ExecutionChange::ToolCall {
                    call_id: call.call_id.clone(),
                    callable_id: call.callable_id.clone(),
                    input: call.input.clone(),
                },
            )?;
            let change = execute_admitted_client_tool_call(
                &service,
                &session_id,
                &execution_id,
                call.clone(),
                |request| {
                    invoke_permission_handler(&service, permission_handler.as_ref(), request)
                },
            );
            let result = match &change {
                ExecutionChange::ToolResult { call_id, output } => ModelToolResult {
                    call_id: call_id.clone(),
                    callable_id: call.callable_id.clone(),
                    output: output.clone(),
                    is_error: false,
                },
                ExecutionChange::ToolFailed { call_id, error } => {
                    if matches!(error, ApplicationError::Cancelled) {
                        return Err(ApplicationError::Cancelled);
                    }
                    ModelToolResult {
                        call_id: call_id.clone(),
                        callable_id: call.callable_id.clone(),
                        output: error.to_value(),
                        is_error: true,
                    }
                }
                _ => {
                    return Err(ApplicationError::InvalidResponse {
                        message: "client tool executor returned a non-terminal tool change".to_owned(),
                    });
                }
            };
            send_execution_progress(
                &progress_sender,
                &session_id,
                &execution_id,
                change,
            )?;
            tool_results.push(result);
        }
        continuation.push(ModelToolTurn {
            assistant_output: output,
            tool_calls,
            tool_results,
        });
    }

    Err(ApplicationError::Conflict {
        message: "model tool turn limit exceeded".to_owned(),
    })
}

fn send_execution_progress(
    sender: &mpsc::Sender<ExecutionWorkerEvent>,
    session_id: &SessionId,
    execution_id: &str,
    change: ExecutionChange,
) -> Result<(), ApplicationError> {
    sender
        .blocking_send(ExecutionWorkerEvent::Progress(ExecutionProgress {
            session_id: session_id.clone(),
            execution_id: execution_id.to_owned(),
            change,
        }))
        .map_err(|_| ApplicationError::Disconnected)
}

fn invoke_permission_handler(
    service: &SdkApplicationService,
    handler: Option<&PermissionHandlerRef>,
    request: PermissionRequest,
) -> Result<PermissionResponse, ApplicationError> {
    let handler = handler.ok_or_else(|| ApplicationError::PermissionDenied {
        message: "client tool requires permission but no permission handler is registered".to_owned(),
    })?;
    let operation = ContractId::parse(InvokeCapability::ID)
        .expect("static application capability invocation id is valid");
    let output = service.invoke(
        &operation,
        CapabilityInvokeInput {
            callable: handler.to_value(),
            input: request.to_value(),
        }
        .to_value(),
    )?;
    let result = CapabilityInvokeResult::from_value(&output).map_err(|error| {
        ApplicationError::InvalidResponse {
            message: error.to_string(),
        }
    })?;
    PermissionResponse::from_value(&result.output).map_err(|error| {
        ApplicationError::InvalidResponse {
            message: error.to_string(),
        }
    })
}
'''
text = text[:old_start] + new_run + text[old_end:]
path.write_text(text)
