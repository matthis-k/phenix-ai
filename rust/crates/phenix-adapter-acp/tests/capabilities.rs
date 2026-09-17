use phenix_adapter_acp::{wire, ApplicationAdapter};
use phenix_application_interface::{types::ApplicationError, ApplicationTransport};
use phenix_core::{ContractId, PhenixValue};
use std::future::ready;
use wire::schema::{v1::InitializeRequest, ProtocolVersion};

struct NoopTransport;

impl ApplicationTransport for NoopTransport {
    fn invoke(
        &self,
        _operation: &ContractId,
        _input: PhenixValue,
    ) -> impl std::future::Future<Output = Result<PhenixValue, ApplicationError>> {
        ready(Err(ApplicationError::Failed {
            message: "initialize must not invoke application transport".to_owned(),
        }))
    }
}

fn capability(value: &str) -> ContractId {
    ContractId::parse(value).expect("valid capability")
}

#[test]
fn initialize_hides_unavailable_optional_capabilities_and_extensions() {
    let adapter = ApplicationAdapter::new(
        NoopTransport,
        [
            "phenix.application.capability.discovery@1",
            "phenix.application.capability.sessions@1",
            "phenix.application.capability.prompt@1",
        ]
        .into_iter()
        .map(capability),
    )
    .expect("baseline ACP capabilities");

    let response = adapter.initialize(InitializeRequest::new(ProtocolVersion::V1));
    let value = serde_json::to_value(response).expect("initialize JSON");

    let advertised = value["_meta"]["phenix.extensions"]["capabilities"]
        .as_array()
        .expect("advertised capabilities");
    assert_eq!(advertised.len(), 3);
    assert!(advertised
        .iter()
        .any(|capability| capability == "phenix.application.capability.prompt@1"));

    assert_eq!(value["agentCapabilities"]["loadSession"], false);
    let session = &value["agentCapabilities"]["sessionCapabilities"];
    assert!(session["list"].is_null());
    assert!(session["resume"].is_null());
    assert!(session["close"].is_null());

    let methods = value["_meta"]["phenix.extensions"]["methods"]
        .as_array()
        .expect("extension methods");
    for unavailable in [
        "_phenix/authentication-list@1",
        "_phenix/session-rename@1",
        "_phenix/session-lineage@1",
        "_phenix/skill-list@1",
        "_phenix/callable-list@1",
        "_phenix/diagnostics@1",
    ] {
        assert!(
            methods.iter().all(|method| method["method"] != unavailable),
            "unexpected extension {unavailable}"
        );
    }
}
