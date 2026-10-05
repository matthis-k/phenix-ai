use phenix_core::{Bytes, CallableId, PhenixValue, Project, SessionId};
use phenix_harness::{
    INVOCATION_DEFAULTS_PLUGIN, PhenixRuntime, PhenixRuntimeBuilder, default_suite_authority,
};
use phenix_sdk::{
    InvocationDefaultsCommand, InvocationDefaultsResponse, InvocationRequest, OptionCommand,
    OptionKey, OptionResponse, OptionScope, OptionSubjectId, OptionValue,
    invocation_defaults_service, options_service,
};
use std::collections::BTreeSet;

#[test]
fn extracted_defaults_work_without_options_plugin() {
    let enabled = BTreeSet::from([INVOCATION_DEFAULTS_PLUGIN.to_owned()]);
    let mut harness = PhenixRuntimeBuilder::with_selected_suite(&enabled)
        .unwrap()
        .build()
        .unwrap();
    harness.activate().unwrap();

    assert!(
        !harness
            .kernel()
            .config()
            .manifests()
            .any(|manifest| manifest.id.as_str() == "phenix.options")
    );

    let command = InvocationDefaultsCommand::Resolve {
        request: InvocationRequest {
            execution_id: "execution-basic".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: Some(CallableId::parse("agent.basic").unwrap()),
            input: Bytes::from(b"prompt".to_vec()),
            tools: Vec::new(),
            continuation: Vec::new(),
        },
    };
    let output = harness
        .invoke(
            &invocation_defaults_service(),
            &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
            &default_suite_authority(),
            None,
        )
        .unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    let InvocationDefaultsResponse::Params { params } =
        InvocationDefaultsResponse::try_from(Project(&output)).unwrap();

    assert_eq!(params.profile_id.as_str(), "default");
}

#[test]
fn extracted_defaults_keep_session_route_precedence() {
    let mut harness = PhenixRuntime::default_suite().unwrap();
    harness.activate().unwrap();

    let key = OptionKey::parse("model.default").unwrap();
    for (scope, value) in [
        (
            OptionScope::Agent(OptionSubjectId::parse("agent.coordinator").unwrap()),
            "router.agent",
        ),
        (
            OptionScope::Session(OptionSubjectId::parse("session-1").unwrap()),
            "router.session",
        ),
    ] {
        let command = OptionCommand::Set {
            key: key.clone(),
            scope,
            value: OptionValue::String(value.into()),
        };
        let output = harness
            .invoke(
                &options_service(),
                &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
                &default_suite_authority(),
                None,
            )
            .unwrap();
        let output: PhenixValue = serde_json::from_slice(&output).unwrap();
        assert!(matches!(
            OptionResponse::try_from(Project(&output)).unwrap(),
            OptionResponse::Updated { .. }
        ));
    }

    let command = InvocationDefaultsCommand::Resolve {
        request: InvocationRequest {
            execution_id: "execution-1".into(),
            session_id: Some(SessionId::parse("session-1").unwrap()),
            parent_attempt_id: None,
            callable_id: Some(CallableId::parse("agent.coordinator").unwrap()),
            input: Bytes::from(b"prompt".to_vec()),
            tools: Vec::new(),
            continuation: Vec::new(),
        },
    };
    let output = harness
        .invoke(
            &invocation_defaults_service(),
            &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
            &default_suite_authority(),
            None,
        )
        .unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    let InvocationDefaultsResponse::Params { params } =
        InvocationDefaultsResponse::try_from(Project(&output)).unwrap();

    assert_eq!(params.profile_id.as_str(), "router.session");
}
