use phenix_adapter_acp::{ApplicationAdapter, wire};
use phenix_application_interface::{
    ApplicationTransport, CreateSession, Operation,
    types::{ApplicationError, SessionInfo},
};
use phenix_core::{ContractId, PhenixValue, SessionId, ValueCodec};
use std::sync::{Arc, Mutex};
use wire::schema::v1::NewSessionRequest;

#[derive(Clone, Default)]
struct RecordingTransport {
    calls: Arc<Mutex<Vec<String>>>,
}

impl ApplicationTransport for RecordingTransport {
    fn invoke(
        &self,
        operation: &ContractId,
        _input: PhenixValue,
    ) -> impl std::future::Future<Output = Result<PhenixValue, ApplicationError>> {
        let calls = self.calls.clone();
        let operation = operation.as_str().to_owned();
        async move {
            calls.lock().expect("calls lock").push(operation.clone());
            if operation == CreateSession::ID {
                return Ok(SessionInfo {
                    session_id: SessionId::parse("session-1").expect("session id"),
                    title: None,
                    working_directory: "/workspace".to_owned(),
                }
                .to_value());
            }
            Err(ApplicationError::Failed {
                message: format!("unexpected operation {operation}"),
            })
        }
    }
}

fn capability(value: &str) -> ContractId {
    ContractId::parse(value).expect("valid capability")
}

#[tokio::test]
async fn dropping_adapter_does_not_close_the_durable_session() {
    let transport = RecordingTransport::default();
    let calls = transport.calls.clone();
    let adapter = ApplicationAdapter::new(
        transport,
        [
            "phenix.application.capability.discovery@1",
            "phenix.application.capability.sessions@1",
            "phenix.application.capability.prompt@1",
        ]
        .into_iter()
        .map(capability),
    )
    .expect("baseline ACP capabilities");

    adapter
        .new_session(NewSessionRequest::new("/workspace"))
        .await
        .expect("create session");
    drop(adapter);

    assert_eq!(
        calls.lock().expect("calls lock").as_slice(),
        [CreateSession::ID]
    );
}
