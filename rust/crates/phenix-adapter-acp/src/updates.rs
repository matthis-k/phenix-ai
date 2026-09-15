use crate::{extension_name, wire};
use phenix_application_interface::{
    application_descriptor,
    types::{
        ApplicationError, Content as ApplicationContent, ExecutionChange,
        ExecutionUpdate as ApplicationExecutionUpdate, MessageRole as ApplicationMessageRole,
        SessionChange, SessionUpdate as ApplicationSessionUpdate,
    },
    ApplicationDescriptor,
};
use phenix_core::{PhenixValue, SessionId, ValueCodec};
use serde_json::{Map, Value};
use std::sync::Arc;
use wire::schema::v1::{
    ContentBlock, ContentChunk, ExtNotification, SessionNotification,
    SessionUpdate as AcpSessionUpdate, TextContent, ToolCall, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields,
};

const SESSION_UPDATE_EVENT: &str = "phenix.application.session-update@1";
const EXECUTION_UPDATE_EVENT: &str = "phenix.application.execution-update@1";

#[derive(Clone, Debug)]
pub enum TranslatedSessionUpdate {
    Standard(Box<SessionNotification>),
    Extension(ExtNotification),
}

pub fn translate_session_update(
    update: &ApplicationSessionUpdate,
) -> Result<Vec<TranslatedSessionUpdate>, ApplicationError> {
    let descriptor = application_descriptor();
    translate_session_with_descriptor(&descriptor, update)
}

pub fn translate_execution_update(
    update: &ApplicationExecutionUpdate,
) -> Result<Vec<TranslatedSessionUpdate>, ApplicationError> {
    let descriptor = application_descriptor();
    translate_execution_change(
        &update.session_id,
        update.sequence,
        &update.execution_id,
        &update.update,
        || extension_fallback(&descriptor, EXECUTION_UPDATE_EVENT, update),
    )
}

pub fn translate_extension_event(
    event: &phenix_core::ContractId,
    payload: &PhenixValue,
) -> Result<ExtNotification, ApplicationError> {
    let descriptor = application_descriptor();
    let declaration =
        descriptor
            .events
            .get(event)
            .ok_or_else(|| ApplicationError::InvalidResponse {
                message: format!("application descriptor is missing event {event}"),
            })?;
    let schema = descriptor.types.get(&declaration.payload).ok_or_else(|| {
        ApplicationError::InvalidResponse {
            message: format!(
                "application descriptor is missing event payload type {}",
                declaration.payload
            ),
        }
    })?;
    schema
        .parse(payload)
        .map_err(|error| ApplicationError::InvalidResponse {
            message: format!("application event violates descriptor schema: {error}"),
        })?;
    let params = serde_json::value::to_raw_value(payload).map_err(|error| {
        ApplicationError::InvalidResponse {
            message: format!("cannot encode application event for ACP: {error}"),
        }
    })?;
    Ok(ExtNotification::new(
        extension_name(event),
        Arc::from(params),
    ))
}

fn translate_session_with_descriptor(
    descriptor: &ApplicationDescriptor,
    update: &ApplicationSessionUpdate,
) -> Result<Vec<TranslatedSessionUpdate>, ApplicationError> {
    match &update.update {
        SessionChange::Message { message } => {
            let mut notifications = Vec::with_capacity(message.content.len());
            for content in &message.content {
                let ApplicationContent::Text { text } = content else {
                    return extension_fallback(descriptor, SESSION_UPDATE_EVENT, update)
                        .map(|item| vec![item]);
                };
                let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(text.clone())));
                let acp_update = match &message.role {
                    ApplicationMessageRole::User => AcpSessionUpdate::UserMessageChunk(chunk),
                    ApplicationMessageRole::Assistant => AcpSessionUpdate::AgentMessageChunk(chunk),
                };
                notifications.push(standard(
                    &update.session_id,
                    update.sequence,
                    None,
                    acp_update,
                ));
            }
            if notifications.is_empty() {
                return extension_fallback(descriptor, SESSION_UPDATE_EVENT, update)
                    .map(|item| vec![item]);
            }
            Ok(notifications)
        }
        SessionChange::TextDelta { execution_id, text } => Ok(vec![standard(
            &update.session_id,
            update.sequence,
            Some(execution_id),
            AcpSessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new(text.clone()),
            ))),
        )]),
        SessionChange::Execution {
            execution_id,
            update: execution,
        } => translate_execution_change(
            &update.session_id,
            update.sequence,
            execution_id,
            execution,
            || extension_fallback(descriptor, SESSION_UPDATE_EVENT, update),
        ),
        SessionChange::Renamed { .. }
        | SessionChange::Closed
        | SessionChange::Diagnostic { .. }
        | SessionChange::Review { .. } => {
            extension_fallback(descriptor, SESSION_UPDATE_EVENT, update).map(|item| vec![item])
        }
    }
}

fn translate_execution_change(
    session_id: &SessionId,
    sequence: u64,
    execution_id: &str,
    execution: &ExecutionChange,
    fallback: impl FnOnce() -> Result<TranslatedSessionUpdate, ApplicationError>,
) -> Result<Vec<TranslatedSessionUpdate>, ApplicationError> {
    match execution {
        ExecutionChange::ToolCall {
            call_id,
            callable_id,
            input,
        } => {
            let call = ToolCall::new(call_id.clone(), callable_id.to_string())
                .status(ToolCallStatus::InProgress)
                .raw_input(encode_json(input)?);
            Ok(vec![standard(
                session_id,
                sequence,
                Some(execution_id),
                AcpSessionUpdate::ToolCall(call),
            )])
        }
        ExecutionChange::ToolResult { call_id, output } => {
            let fields = ToolCallUpdateFields::new()
                .status(ToolCallStatus::Completed)
                .raw_output(encode_json(output)?);
            Ok(vec![standard(
                session_id,
                sequence,
                Some(execution_id),
                AcpSessionUpdate::ToolCallUpdate(ToolCallUpdate::new(call_id.clone(), fields)),
            )])
        }
        ExecutionChange::ToolFailed { call_id, error } => {
            let fields = ToolCallUpdateFields::new()
                .status(ToolCallStatus::Failed)
                .raw_output(encode_json(&error.to_value())?);
            Ok(vec![standard(
                session_id,
                sequence,
                Some(execution_id),
                AcpSessionUpdate::ToolCallUpdate(ToolCallUpdate::new(call_id.clone(), fields)),
            )])
        }
        ExecutionChange::State { .. } | ExecutionChange::Progress { .. } => {
            fallback().map(|item| vec![item])
        }
    }
}

fn standard(
    session_id: &SessionId,
    sequence: u64,
    execution_id: Option<&str>,
    acp_update: AcpSessionUpdate,
) -> TranslatedSessionUpdate {
    TranslatedSessionUpdate::Standard(Box::new(
        SessionNotification::new(session_id.to_string(), acp_update)
            .meta(correlation_meta(sequence, execution_id)),
    ))
}

fn correlation_meta(sequence: u64, execution_id: Option<&str>) -> Map<String, Value> {
    let mut meta = Map::new();
    meta.insert("phenix.sequence".to_owned(), Value::from(sequence));
    if let Some(execution_id) = execution_id {
        meta.insert(
            "phenix.executionId".to_owned(),
            Value::String(execution_id.to_owned()),
        );
    }
    meta
}

fn extension_fallback<T: ValueCodec>(
    descriptor: &ApplicationDescriptor,
    event_id: &str,
    update: &T,
) -> Result<TranslatedSessionUpdate, ApplicationError> {
    let (event, _) = descriptor
        .events
        .iter()
        .find(|(event, _)| event.as_str() == event_id)
        .ok_or_else(|| ApplicationError::InvalidResponse {
            message: format!("application descriptor is missing event {event_id}"),
        })?;
    let value = update.to_value();
    translate_extension_event(event, &value).map(TranslatedSessionUpdate::Extension)
}

fn encode_json(value: &PhenixValue) -> Result<Value, ApplicationError> {
    serde_json::to_value(value).map_err(|error| ApplicationError::InvalidResponse {
        message: format!("cannot encode application value for ACP: {error}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_catalog;
    use phenix_application_interface::{
        types::{
            Content, ExecutionChange, ExecutionUpdate, Message, MessageRole, SessionChange,
            SessionUpdate,
        },
        Capabilities,
    };
    use phenix_core::{CallableId, ContractId, SessionId};

    fn session_id() -> SessionId {
        SessionId::parse("session-1").expect("valid session id")
    }

    #[test]
    fn standard_text_updates_preserve_sequence_and_execution_identity() {
        let message = SessionUpdate {
            session_id: session_id(),
            sequence: 7,
            update: SessionChange::Message {
                message: Message {
                    role: MessageRole::Assistant,
                    content: vec![Content::Text {
                        text: "hello".to_owned(),
                    }],
                },
            },
        };
        let translated = translate_session_update(&message).expect("translate message");
        let TranslatedSessionUpdate::Standard(notification) = &translated[0] else {
            panic!("text message should use standard ACP");
        };
        assert_eq!(notification.session_id.to_string(), "session-1");
        assert_eq!(
            notification.meta.as_ref().expect("correlation meta")["phenix.sequence"],
            7
        );
        match &notification.update {
            AcpSessionUpdate::AgentMessageChunk(chunk) => match &chunk.content {
                ContentBlock::Text(text) => assert_eq!(text.text, "hello"),
                _ => panic!("expected text content"),
            },
            _ => panic!("expected agent message chunk"),
        }

        let delta = SessionUpdate {
            session_id: session_id(),
            sequence: 8,
            update: SessionChange::TextDelta {
                execution_id: "execution-7".to_owned(),
                text: " world".to_owned(),
            },
        };
        let translated = translate_session_update(&delta).expect("translate delta");
        let TranslatedSessionUpdate::Standard(notification) = &translated[0] else {
            panic!("text delta should use standard ACP");
        };
        let meta = notification.meta.as_ref().expect("correlation meta");
        assert_eq!(meta["phenix.sequence"], 8);
        assert_eq!(meta["phenix.executionId"], "execution-7");
    }

    #[test]
    fn tool_updates_use_standard_acp_tool_call_updates() {
        let update = SessionUpdate {
            session_id: session_id(),
            sequence: 9,
            update: SessionChange::Execution {
                execution_id: "execution-7".to_owned(),
                update: ExecutionChange::ToolCall {
                    call_id: "call-1".to_owned(),
                    callable_id: CallableId::parse("tools.read").expect("valid callable id"),
                    input: PhenixValue::String("README.md".to_owned()),
                },
            },
        };
        let translated = translate_session_update(&update).expect("translate tool call");
        let TranslatedSessionUpdate::Standard(notification) = &translated[0] else {
            panic!("tool call should use standard ACP");
        };
        let meta = notification.meta.as_ref().expect("correlation meta");
        assert_eq!(meta["phenix.sequence"], 9);
        assert_eq!(meta["phenix.executionId"], "execution-7");
        match &notification.update {
            AcpSessionUpdate::ToolCall(call) => {
                assert_eq!(call.tool_call_id.to_string(), "call-1");
                assert_eq!(call.status, ToolCallStatus::InProgress);
                assert!(call.raw_input.is_some());
            }
            _ => panic!("expected tool call"),
        }
    }

    #[test]
    fn standalone_execution_updates_keep_execution_ordering_metadata() {
        let update = ExecutionUpdate {
            session_id: session_id(),
            execution_id: "execution-7".to_owned(),
            sequence: 3,
            update: ExecutionChange::ToolResult {
                call_id: "call-1".to_owned(),
                output: PhenixValue::String("done".to_owned()),
            },
        };
        let translated = translate_execution_update(&update).expect("translate execution update");
        let TranslatedSessionUpdate::Standard(notification) = &translated[0] else {
            panic!("tool result should use standard ACP");
        };
        let meta = notification.meta.as_ref().expect("correlation meta");
        assert_eq!(meta["phenix.sequence"], 3);
        assert_eq!(meta["phenix.executionId"], "execution-7");
        match &notification.update {
            AcpSessionUpdate::ToolCallUpdate(tool) => {
                assert_eq!(tool.tool_call_id.to_string(), "call-1");
                assert_eq!(
                    tool.fields.status.as_ref(),
                    Some(&ToolCallStatus::Completed)
                );
            }
            _ => panic!("expected tool call update"),
        }
    }

    #[test]
    fn progress_falls_back_losslessly_to_descriptor_owned_extension() {
        let update = SessionUpdate {
            session_id: session_id(),
            sequence: 10,
            update: SessionChange::Execution {
                execution_id: "execution-7".to_owned(),
                update: ExecutionChange::Progress {
                    message: "working".to_owned(),
                    fraction: Some(0.5),
                },
            },
        };
        let translated = translate_session_update(&update).expect("translate progress");
        let TranslatedSessionUpdate::Extension(notification) = &translated[0] else {
            panic!("progress should use the Phenix extension");
        };
        assert_eq!(notification.method.as_ref(), "_phenix/session-update@1");
        let params: Value =
            serde_json::from_str(notification.params.get()).expect("extension JSON");
        assert_eq!(params, serde_json::to_value(update.to_value()).unwrap());

        let execution = ExecutionUpdate {
            session_id: session_id(),
            execution_id: "execution-7".to_owned(),
            sequence: 4,
            update: ExecutionChange::Progress {
                message: "still working".to_owned(),
                fraction: Some(0.75),
            },
        };
        let translated = translate_execution_update(&execution).expect("translate progress");
        let TranslatedSessionUpdate::Extension(notification) = &translated[0] else {
            panic!("execution progress should use the Phenix extension");
        };
        assert_eq!(notification.method.as_ref(), "_phenix/execution-update@1");
        let params: Value =
            serde_json::from_str(notification.params.get()).expect("extension JSON");
        assert_eq!(params, serde_json::to_value(execution.to_value()).unwrap());
    }

    #[test]
    fn update_fallback_schemas_are_advertised_from_the_descriptor() {
        let descriptor = application_descriptor();
        let advertised = descriptor.capabilities.keys().cloned().collect::<Vec<_>>();
        let capabilities = Capabilities::negotiate(&descriptor, advertised).expect("capabilities");
        let catalog = extension_catalog(&descriptor, &capabilities);
        for expected_id in [
            "phenix.application.session-update@1",
            "phenix.application.execution-update@1",
        ] {
            let expected_id = ContractId::parse(expected_id).unwrap();
            let method = extension_name(&expected_id);
            let event = catalog
                .events
                .iter()
                .find(|event| event.method == method)
                .expect("update extension");
            let expected = &descriptor.events[&expected_id];
            assert_eq!(event.event, expected_id);
            assert_eq!(event.payload, descriptor.types[&expected.payload]);
        }
    }

    #[test]
    fn descriptor_extension_events_encode_application_payloads() {
        let update = SessionUpdate {
            session_id: session_id(),
            sequence: 11,
            update: SessionChange::Renamed {
                title: "Renamed".to_owned(),
            },
        };
        let event = ContractId::parse("phenix.application.session-update@1")
            .expect("static event id is valid");
        let notification = translate_extension_event(&event, &update.to_value())
            .expect("descriptor event is encodable");

        assert_eq!(notification.method.as_ref(), "_phenix/session-update@1");
        let payload: Value =
            serde_json::from_str(notification.params.get()).expect("extension event JSON");
        assert_eq!(payload, serde_json::to_value(update.to_value()).unwrap());
    }
}
