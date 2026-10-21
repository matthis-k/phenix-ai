use crate::{request, ModelConfiguration};
use phenix_basic_contracts::ErrorCode;

#[test]
fn selected_http_install_is_consumed_once_and_never_falls_back_to_environment() {
    let mut mode = ModelConfiguration::Selected(Some(request::Config {
        endpoint: "http://127.0.0.1:11434/v1/chat/completions".into(),
        model: "controller-selected-model".into(),
        protocol: phenix_provider_protocols::Protocol::OpenaiChat,
        provider_max_tokens: None,
        credential: None,
        ca_file: None,
    }));
    let config = mode.take_for_turn().unwrap();
    assert_eq!(config.model, "controller-selected-model");
    assert_eq!(
        mode.take_for_turn().unwrap_err().code,
        ErrorCode::InvalidInput
    );
    // Even if legacy PHENIX_MODEL_HTTP_* variables are present, selected
    // mode never invokes Config::load once its one-turn grant is spent.
    assert!(matches!(mode, ModelConfiguration::Selected(None)));
}
