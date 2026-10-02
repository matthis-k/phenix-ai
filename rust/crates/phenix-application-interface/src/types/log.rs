use super::*;

record!(LogQueryInput, "phenix.application.type.log-query-input@1", {
    cursor: Option<String>,
    limit: Option<u64>,
    session_id: Option<SessionId>,
    execution_id: Option<String>,
});
record!(LogRecord, "phenix.application.type.log-record@1", {
    cursor: String,
    timestamp_ms: u64,
    pid: u64,
    kind: String,
    payload: PhenixValue,
});
record!(LogPage, "phenix.application.type.log-page@1", {
    records: Vec<LogRecord>,
    next_cursor: Option<String>,
});
record!(LogReferenceInput, "phenix.application.type.log-reference-input@1", {
    reference: ContentReference,
});
record!(LogReferenceContent, "phenix.application.type.log-reference-content@1", {
    reference: ContentReference,
    content: Bytes,
});
