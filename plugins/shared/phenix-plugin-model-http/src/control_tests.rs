use crate::{request, ModelConfiguration};
use phenix_basic_contracts::ErrorCode;

#[test]
fn selected_http_install_is_per_session_consumed_once_and_never_falls_back() {
    let config = |model: &str| request::Config {
        endpoint: "http://127.0.0.1:11434/v1/chat/completions".into(),
        model: model.into(),
        protocol: phenix_provider_protocols::Protocol::OpenaiChat,
        provider_max_tokens: None,
        credential: None,
        presentation: phenix_provider_contracts::ApiKeyPresentation::Bearer,
        ca_file: None,
    };
    let mut mode = ModelConfiguration::default();
    mode.install("s1".into(), config("first-session-model"));
    mode.install("s2".into(), config("second-session-model"));

    // A foreign session cannot consume either session's selected model.
    assert_eq!(
        mode.take_for_turn("s3").unwrap_err().code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        mode.take_for_turn("s1").unwrap().model,
        "first-session-model"
    );
    assert_eq!(
        mode.take_for_turn("s1").unwrap_err().code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        mode.take_for_turn("s2").unwrap().model,
        "second-session-model"
    );
    assert_eq!(
        mode.take_for_turn("s2").unwrap_err().code,
        ErrorCode::InvalidInput
    );
    // Once selected mode is active, the environment preset is never consulted
    // for any missing or consumed session.
    assert!(matches!(mode, ModelConfiguration::Selected(ref pending) if pending.is_empty()));
}
