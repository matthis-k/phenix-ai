use crate::{request, ModelConfiguration};
use phenix_basic_contracts::ErrorCode;

#[test]
fn selected_http_install_is_per_session_and_run_and_never_falls_back() {
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
    mode.install("s1".into(), config("first-session-model"), 1)
        .unwrap();
    // Bypass controller sequencing: an unused install must not be replaced either.
    assert_eq!(
        mode.install("s1".into(), config("replacement"), 2)
            .unwrap_err()
            .code,
        phenix_provider_contracts::ProviderErrorCode::Conflict
    );
    mode.install("s2".into(), config("second-session-model"), 2)
        .unwrap();

    // A foreign session cannot consume either session's selected model.
    assert_eq!(
        mode.take_for_turn("s3", "r1").unwrap_err().code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        mode.take_for_turn("s1", "r1").unwrap().model,
        "first-session-model"
    );
    assert_eq!(
        mode.take_for_turn("s1", "r2").unwrap_err().code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        mode.take_for_turn("s2", "r1").unwrap().model,
        "second-session-model"
    );
    assert_eq!(
        mode.take_for_turn("s2", "").unwrap_err().code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        mode.take_for_turn("s1", "r1").unwrap().model,
        "first-session-model"
    );
    let original = mode.take_for_turn("s1", "r1").unwrap();
    assert_eq!(
        mode.install("s1".into(), config("fresh-model"), 3)
            .unwrap_err()
            .code,
        phenix_provider_contracts::ProviderErrorCode::Conflict
    );
    assert!(std::sync::Arc::ptr_eq(
        &original,
        &mode.take_for_turn("s1", "r1").unwrap()
    ));
    // An unrelated run cannot replace or release the admitted run's install.
    mode.release_run("s1", "r2");
    mode.release("s1", 3);
    assert!(mode.take_for_turn("s1", "r1").is_ok());
    assert!(mode.take_for_turn("s1", "r2").is_err());
    mode.release("s1", 1);
    assert!(mode.take_for_turn("s1", "r1").is_err());
    mode.install("s1".into(), config("fresh-model"), 3).unwrap();
    assert_eq!(mode.take_for_turn("s1", "r2").unwrap().model, "fresh-model");
    assert_eq!(
        mode.take_for_turn("s1", "r1").unwrap_err().code,
        ErrorCode::InvalidInput
    );
    mode.release("s1", 1);
    let worker = mode.take_for_turn("s1", "r2").unwrap();
    let weak = std::sync::Arc::downgrade(&worker);
    mode.release("s1", 3);
    assert!(mode.take_for_turn("s1", "r2").is_err());
    assert!(mode.take_for_turn("s1", "r3").is_err());
    assert!(weak.upgrade().is_some());
    drop(worker);
    assert!(weak.upgrade().is_none());
    mode.release_run("s2", "foreign");
    assert!(mode.take_for_turn("s2", "r1").is_ok());
    mode.release_run("s2", "r1");
    assert!(mode.take_for_turn("s2", "r1").is_err());
}
