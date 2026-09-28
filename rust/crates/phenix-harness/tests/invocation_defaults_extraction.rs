use phenix_core::{Bytes, PhenixValue, Project};
use phenix_harness::{default_suite_authority, HarnessBuilder};
use phenix_plugin_invocation_defaults::INVOCATION_DEFAULTS_PLUGIN;
use phenix_sdk::{
    invocation_defaults_service, InvocationDefaultsCommand, InvocationDefaultsResponse,
    InvocationRequest,
};
use std::collections::BTreeSet;

#[test]
fn extracted_defaults_work_without_options_plugin() {
    let enabled = BTreeSet::from([INVOCATION_DEFAULTS_PLUGIN.to_owned()]);
    let mut harness = HarnessBuilder::with_selected_suite(&enabled)
        .unwrap()
        .build()
        .unwrap();
    harness.activate().unwrap();

    assert!(!harness
        .kernel()
        .config()
        .manifests()
        .any(|manifest| manifest.id.as_str() == "phenix.options"));

    let command = InvocationDefaultsCommand::Resolve {
        request: InvocationRequest {
            execution_id: "execution-basic".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
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
