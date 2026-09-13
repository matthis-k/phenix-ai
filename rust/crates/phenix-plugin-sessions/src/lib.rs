#![forbid(unsafe_code)]

mod implementation;

pub use implementation::*;
pub use phenix_sdk::{
    session_service, SessionCommand, SessionId, SessionInput, SessionInputKind, SessionInterface,
    SessionLifecycle, SessionRecord, SessionResponse, SESSION_SERVICE,
};

#[cfg(test)]
mod history_integration;
