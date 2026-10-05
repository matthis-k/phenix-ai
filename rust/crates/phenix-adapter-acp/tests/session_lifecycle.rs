use phenix_adapter_acp::{ApplicationAdapter, wire};
use phenix_application_interface::{
    ApplicationTransport, Cancel, CloseSession, Operation, ResumeSession,
    types::{
        Acknowledged, ApplicationError, SessionInfo, SessionInput, SessionResumeInput,
        SessionSnapshot,
    },
};
use phenix_core::{ContractId, PhenixValue, SessionId, ValueCodec};
use std::sync::{Arc, Mutex};
use wire::schema::v1::{CancelNotification, CloseSessionRequest, LoadSessionRequest};

#[derive(Clone, Default)]
struct LifecycleTransport {
    calls: Arc<Mutex<Vec<(String, PhenixValue)>>>,
}

impl ApplicationTransport for LifecycleTransport {
    fn invoke(
        &self,
        operation: &ContractId,
        input: PhenixValue,
    ) -> impl std::future::Future<Output = Result<PhenixValue, ApplicationError>> {
        let calls = self.calls.clone();
        let operation = operation.as_str().to_owned();
        async move {
            calls
                .lock()
                .expect("calls lock")
                .push((operation.clone(), input));
            match operation.as_str() {
                id if id == ResumeSession::ID => Ok(SessionSnapshot {
                    session: SessionInfo {
                        session_id: SessionId::parse("session-1").expect("session id"),
                        title: Some("Example".to_owned()),
                        working_directory: "/workspace".to_owned(),
                    },
                    through_sequence: 11,
                    updates: Vec::new(),
                }
                .to_value()),
                id if id == CloseSession::ID || id == Cancel::ID => Ok(Acknowledged {}.to_value()),
                other => Err(ApplicationError::Failed {
                    message: format!("unexpected operation {other}"),
                }),
            }
        }
    }
}

fn capability(value: &str) -> ContractId {
    ContractId::parse(value).expect("valid capability")
}

#[tokio::test]
async fn load_close_and_cancel_use_the_canonical_session_operations() {
    let transport = LifecycleTransport::default();
    let calls = transport.calls.clone();
    let adapter = ApplicationAdapter::new(
        transport,
        [
            "phenix.application.capability.discovery@1",
            "phenix.application.capability.sessions@1",
            "phenix.application.capability.prompt@1",
            "phenix.application.capability.session-resume@1",
        ]
        .into_iter()
        .map(capability),
    )
    .expect("lifecycle capabilities");

    let loaded = adapter
        .load_session(LoadSessionRequest::new("session-1", "/workspace"))
        .await
        .expect("load session");
    assert_eq!(loaded.snapshot.through_sequence, 11);

    adapter
        .close_session(CloseSessionRequest::new("session-1"))
        .await
        .expect("close session");
    adapter
        .cancel(CancelNotification::new("session-1"))
        .await
        .expect("cancel session execution");

    let calls = calls.lock().expect("calls lock");
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].0, ResumeSession::ID);
    let resume = SessionResumeInput::from_value(&calls[0].1).expect("resume input");
    assert_eq!(resume.session_id.as_str(), "session-1");
    assert_eq!(resume.after_sequence, Some(0));

    assert_eq!(calls[1].0, CloseSession::ID);
    let close = SessionInput::from_value(&calls[1].1).expect("close input");
    assert_eq!(close.session_id.as_str(), "session-1");

    assert_eq!(calls[2].0, Cancel::ID);
    let cancel = SessionInput::from_value(&calls[2].1).expect("cancel input");
    assert_eq!(cancel.session_id.as_str(), "session-1");
}
