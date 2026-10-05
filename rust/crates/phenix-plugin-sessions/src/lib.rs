#![forbid(unsafe_code)]

mod implementation;

pub use implementation::*;
pub use phenix_sdk::{
    SESSION_SERVICE, SessionCommand, SessionId, SessionInput, SessionInputKind, SessionInterface,
    SessionJournalDraft, SessionJournalEntry, SessionLifecycle, SessionRecord, SessionResponse,
    SessionTransition, session_service,
};

#[cfg(test)]
mod history_integration;
