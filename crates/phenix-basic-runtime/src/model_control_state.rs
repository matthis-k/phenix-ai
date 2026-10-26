//! Connection-level P1 control-plane admission, independent of ACP sessions.
//!
//! This is a product state machine, not a permission grant. Any action which
//! passes this gate must still pass the exact provider/account/kernel checks.
//! Report reasons for unavailable actions instead of presenting an inert UI.
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlAction {
    ConnectionInitialize,
    ConnectionClose,
    ProvidersList,
    AuthenticationStatus,
    AuthenticationBegin,
    AuthenticationPoll,
    AuthenticationCancel,
    CredentialsCapabilities,
    CredentialsImport,
    CredentialsBindEnvironment,
    CredentialsRevoke,
    ModelsList,
    ModelsSelect,
    SessionNew,
    SessionLoad,
    SessionModelsSelect,
    SessionPrompt,
    SessionCancel,
}

pub const ALL_ACTIONS: [ControlAction; 18] = [
    ControlAction::ConnectionInitialize,
    ControlAction::ConnectionClose,
    ControlAction::ProvidersList,
    ControlAction::AuthenticationStatus,
    ControlAction::AuthenticationBegin,
    ControlAction::AuthenticationPoll,
    ControlAction::AuthenticationCancel,
    ControlAction::CredentialsCapabilities,
    ControlAction::CredentialsImport,
    ControlAction::CredentialsBindEnvironment,
    ControlAction::CredentialsRevoke,
    ControlAction::ModelsList,
    ControlAction::ModelsSelect,
    ControlAction::SessionNew,
    ControlAction::SessionLoad,
    ControlAction::SessionModelsSelect,
    ControlAction::SessionPrompt,
    ControlAction::SessionCancel,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DenialCode {
    NotInitialized,
    AlreadyInitialized,
    CredentialManagementNotInstalled,
    ConnectionClosed,
    ProviderManagementNotInstalled,
    ModelControlNotInstalled,
    NoSelectableModels,
    SessionRequired,
}

impl DenialCode {
    pub const fn message(self) -> &'static str {
        match self {
            Self::NotInitialized => "Initialize the ACP connection first",
            Self::AlreadyInitialized => "This ACP connection is already initialized",
            Self::CredentialManagementNotInstalled => {
                "The launcher did not grant credential management for this connection"
            },
            Self::ConnectionClosed => "The ACP connection is closed",
            Self::ProviderManagementNotInstalled => {
                "The launcher did not install an authorized provider management composition"
            }
            Self::ModelControlNotInstalled => {
                "The launcher did not grant model selection and session installation"
            }
            Self::NoSelectableModels => {
                "No authorized provider models are currently selectable; authenticate and refresh model discovery"
            }
            Self::SessionRequired => {
                "Create or load a session before submitting a prompt, cancelling it, or selecting a session override"
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ControlState {
    pub initialized: bool,
    pub closed: bool,
    pub provider_management: bool,
    pub credential_management: bool,
    pub model_control: bool,
    pub available_models: usize,
    pub default_model_selected: bool,
    pub session_active: bool,
}

impl ControlState {
    /// Pure state transition guard: these are the product-level actions that
    /// can be attempted, before checking exact provider/credential authority.
    pub fn deny(self, action: ControlAction) -> Option<DenialCode> {
        if self.closed {
            return Some(DenialCode::ConnectionClosed);
        }
        if action == ControlAction::ConnectionClose {
            return None;
        }
        if action == ControlAction::ConnectionInitialize {
            return self.initialized.then_some(DenialCode::AlreadyInitialized);
        }
        if !self.initialized {
            return Some(DenialCode::NotInitialized);
        }
        match action {
            ControlAction::ConnectionInitialize | ControlAction::ConnectionClose => None,
            ControlAction::ProvidersList
            | ControlAction::AuthenticationStatus
            | ControlAction::AuthenticationBegin
            | ControlAction::AuthenticationPoll
            | ControlAction::AuthenticationCancel => {
                (!self.provider_management).then_some(DenialCode::ProviderManagementNotInstalled)
            }
            ControlAction::CredentialsCapabilities
            | ControlAction::CredentialsImport
            | ControlAction::CredentialsBindEnvironment
            | ControlAction::CredentialsRevoke => (!self.credential_management)
                .then_some(DenialCode::CredentialManagementNotInstalled),
            ControlAction::ModelsList => {
                (!self.model_control).then_some(DenialCode::ModelControlNotInstalled)
            }
            ControlAction::ModelsSelect => {
                if !self.model_control {
                    Some(DenialCode::ModelControlNotInstalled)
                } else if self.available_models == 0 {
                    Some(DenialCode::NoSelectableModels)
                } else {
                    None
                }
            }
            ControlAction::SessionNew | ControlAction::SessionLoad => None,
            ControlAction::SessionModelsSelect => {
                if !self.session_active {
                    Some(DenialCode::SessionRequired)
                } else if !self.model_control {
                    Some(DenialCode::ModelControlNotInstalled)
                } else if self.available_models == 0 {
                    Some(DenialCode::NoSelectableModels)
                } else {
                    None
                }
            }
            ControlAction::SessionPrompt | ControlAction::SessionCancel => {
                (!self.session_active).then_some(DenialCode::SessionRequired)
            }
        }
    }

    pub fn phase(self) -> &'static str {
        if self.closed {
            "closed"
        } else if !self.initialized {
            "uninitialized"
        } else if self.session_active {
            "session_active"
        } else if self.default_model_selected {
            "model_selected"
        } else if self.available_models > 0 {
            "models_available"
        } else {
            "ready"
        }
    }

    pub fn describe(self) -> serde_json::Value {
        let actions = ALL_ACTIONS
            .iter()
            .map(|action| {
                let denied = self.deny(*action);
                serde_json::json!({
                    "action": action,
                    "allowed": denied.is_none(),
                    "denial": denied.map(|code| serde_json::json!({
                        "code": code,
                        "message": code.message(),
                    })),
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "format": "phenix.control.state.v1",
            "phase": self.phase(),
            "available_models": self.available_models,
            "default_model_selected": self.default_model_selected,
            "session_active": self.session_active,
            "actions": actions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> ControlState {
        ControlState {
            initialized: true,
            closed: false,
            provider_management: true,
            credential_management: true,
            model_control: true,
            available_models: 2,
            default_model_selected: false,
            session_active: false,
        }
    }

    #[test]
    fn pre_session_model_selection_is_valid_but_prompts_need_sessions() {
        let mut state = ready();
        assert!(state.deny(ControlAction::ModelsList).is_none());
        assert!(state.deny(ControlAction::ModelsSelect).is_none());
        assert_eq!(
            state.deny(ControlAction::SessionPrompt),
            Some(DenialCode::SessionRequired)
        );
        assert_eq!(state.phase(), "models_available");
        state.default_model_selected = true;
        assert_eq!(state.phase(), "model_selected");
        state.session_active = true;
        assert_eq!(state.phase(), "session_active");
        assert!(state.deny(ControlAction::SessionPrompt).is_none());
    }

    #[test]
    fn deny_each_invalid_transition_with_distinct_reason() {
        let mut state = ready();
        state.available_models = 0;
        assert_eq!(
            state.deny(ControlAction::ModelsSelect),
            Some(DenialCode::NoSelectableModels)
        );
        state.model_control = false;
        assert_eq!(
            state.deny(ControlAction::ModelsList),
            Some(DenialCode::ModelControlNotInstalled)
        );
        state.provider_management = false;
        assert_eq!(
            state.deny(ControlAction::AuthenticationBegin),
            Some(DenialCode::ProviderManagementNotInstalled)
        );
        state.initialized = false;
        assert_eq!(
            state.deny(ControlAction::SessionNew),
            Some(DenialCode::NotInitialized)
        );
        state.closed = true;
        assert_eq!(
            state.deny(ControlAction::SessionNew),
            Some(DenialCode::ConnectionClosed)
        );
        assert!(state.describe()["actions"]
            .as_array()
            .is_some_and(|a| a.len() == ALL_ACTIONS.len()));
    }
}
