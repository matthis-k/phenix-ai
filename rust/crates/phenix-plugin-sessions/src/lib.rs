#![forbid(unsafe_code)]

mod implementation;

pub use implementation::*;
pub use phenix_sdk::{
    session_service, SessionCommand, SessionId, SessionInput, SessionInputKind, SessionInterface,
    SessionJournalDraft, SessionJournalEntry, SessionLifecycle, SessionRecord, SessionResponse,
    SessionTransition, SESSION_SERVICE,
};

#[cfg(test)]
mod history_integration;
