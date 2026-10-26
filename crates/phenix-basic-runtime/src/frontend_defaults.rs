//! Shared Basic frontend defaults. Frontends submit input intent, not
//! authentication or session orchestration logic. This layer owns only
//! selection policy; Basic Storage and providers own persistence and auth.

/// Conventional session for sessionless Terminal and JSONL input.
pub const INPUT_SESSION: &str = "s1";

/// Non-conversational user preference scope. Only non-secret selected-model
/// intent is stored here; OAuth/API tokens remain in ProviderCredentials.
pub const MODEL_PREFERENCE_SCOPE: &str = "phenix_user_defaults";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionDecision<'a> {
    Existing(&'a str),
    Create,
}

/// Use a requested session, else current session, else automatically create
/// (ACP) or use the conventional default (Terminal/JSONL).
pub fn session_for_input<'a>(
    explicit: Option<&'a str>,
    active: Option<&'a str>,
    create_when_missing: bool,
) -> SessionDecision<'a> {
    if let Some(value) = explicit.filter(|value| !value.is_empty()) {
        return SessionDecision::Existing(value);
    }
    if let Some(value) = active.filter(|value| !value.is_empty()) {
        return SessionDecision::Existing(value);
    }
    if create_when_missing {
        SessionDecision::Create
    } else {
        SessionDecision::Existing(INPUT_SESSION)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_does_not_need_explicit_session_creation() {
        assert_eq!(session_for_input(None, None, true), SessionDecision::Create);
        assert_eq!(
            session_for_input(None, None, false),
            SessionDecision::Existing("s1")
        );
        assert_eq!(
            session_for_input(None, Some("existing"), true),
            SessionDecision::Existing("existing"),
        );
        assert_eq!(
            session_for_input(Some("chosen"), Some("existing"), true),
            SessionDecision::Existing("chosen"),
        );
    }

    #[test]
    fn default_preference_storage_is_not_a_chat_session() {
        assert_ne!(INPUT_SESSION, MODEL_PREFERENCE_SCOPE);
        assert!(phenix_basic_contracts::session_resource(MODEL_PREFERENCE_SCOPE).is_ok());
    }
}
