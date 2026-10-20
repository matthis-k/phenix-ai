//! Session aggregate over selected revision-checked Storage.
//! The kernel provides admission and cancellation, not durable session logic.
use phenix_basic_contracts::{
    error, BasicError, ErrorCode, Message, ReconcileTool, Role, RunStatus, SessionSnapshot, Usage,
};

const RECORD: &str = "session.snapshot.v1";

pub fn empty(session: &str) -> SessionSnapshot {
    SessionSnapshot {
        session: session.into(),
        history: vec![],
        status: RunStatus::Idle,
        run: String::new(),
        checkpoint: 0,
        revision: 0,
        pending: vec![],
        usage: Usage::default(),
    }
}

/// No speculative retry of a non-idempotent tool call. Its recorded call ID
/// must be explicitly reconciled before the session can resume.
pub fn reconcile_snapshot(
    snapshot: &mut SessionSnapshot,
    resolution: &ReconcileTool,
) -> Result<(), BasicError> {
    if snapshot.session != resolution.session
        || snapshot.status != RunStatus::Indeterminate
        || resolution.call_id.is_empty()
    {
        return Err(error(
            ErrorCode::InvalidInput,
            "session cannot be reconciled",
        ));
    }
    let position = snapshot
        .pending
        .iter()
        .position(|call| call.id == resolution.call_id)
        .ok_or_else(|| error(ErrorCode::NotFound, "pending tool call not found"))?;
    snapshot.pending.remove(position);
    snapshot.history.push(Message::tool(
        resolution.call_id.clone(),
        resolution.outcome.clone(),
    ));
    if snapshot.pending.is_empty() {
        snapshot.status = RunStatus::Running;
    }
    Ok(())
}

#[phenix_sdk::plugin(
    id = "phenix.basic.session",
    version = "1.0.0",
    provide(phenix_basic_contracts::Session, slot = "Session"),
    require(phenix_basic_contracts::Storage, slot = "Storage")
)]
mod plugin {
    use super::{empty, reconcile_snapshot, RECORD};
    use phenix_basic_contracts::{
        decode, encode, error, BasicError, ErrorCode, ReconcileTool, RecordKey, RunStatus, Scope,
        Session, SessionSnapshot, Storage, StorageEdit, StorageTransaction,
    };
    use phenix_sdk::{CallError, CallResult, Code, Failure, Host};
    #[derive(Default)]
    pub struct State;

    fn load(host: &Host<'_>, session: &str) -> CallResult<SessionSnapshot, BasicError> {
        let result = host.invoke::<Storage::read>(
            "Storage",
            &RecordKey {
                session: session.into(),
                key: RECORD.into(),
            },
        );
        match result {
            Ok(record) => {
                let snapshot: SessionSnapshot = decode(&record.data).map_err(|_| {
                    CallError::Domain(error(ErrorCode::StorageFailed, "invalid snapshot"))
                })?;
                if snapshot.revision != record.revision || snapshot.session != session {
                    return Err(CallError::Domain(error(
                        ErrorCode::StorageFailed,
                        "snapshot revision or session mismatch",
                    )));
                }
                Ok(snapshot)
            }
            Err(CallError::Domain(err)) if err.code == ErrorCode::NotFound => Ok(empty(session)),
            Err(err) => Err(err),
        }
    }

    fn commit(
        host: &Host<'_>,
        mut snapshot: SessionSnapshot,
    ) -> CallResult<SessionSnapshot, BasicError> {
        let expected = snapshot.revision;
        snapshot.revision = expected.checked_add(1).ok_or_else(|| {
            CallError::Domain(error(ErrorCode::Conflict, "session revision overflow"))
        })?;
        let data = encode(&snapshot)?;
        let records = host.invoke::<Storage::transact>(
            "Storage",
            &StorageTransaction {
                session: snapshot.session.clone(),
                edits: vec![StorageEdit {
                    key: RECORD.into(),
                    expected,
                    data,
                }],
            },
        )?;
        if records.len() != 1 || records[0].revision != snapshot.revision {
            return Err(CallError::Kernel(Failure::new(
                Code::PluginFailed,
                "storage returned invalid session revision",
            )));
        }
        Ok(snapshot)
    }

    #[provide(Session::open)]
    fn open(
        _: &mut State,
        host: &Host<'_>,
        request: Scope,
    ) -> CallResult<SessionSnapshot, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        // A new plugin instance cannot prove that the owning process died.
        // Report the durable state unchanged. In particular, do not turn a
        // still-live run with pending tools into Indeterminate.
        load(host, &request.session)
    }

    #[provide(Session::checkpoint)]
    fn checkpoint(
        _: &mut State,
        host: &Host<'_>,
        request: SessionSnapshot,
    ) -> CallResult<SessionSnapshot, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        let prior = load(host, &request.session)?;
        if request.revision != prior.revision {
            return Err(CallError::Domain(error(
                ErrorCode::Conflict,
                "stale session checkpoint",
            )));
        }
        if prior.status == RunStatus::Indeterminate && request.status != RunStatus::Indeterminate {
            return Err(CallError::Domain(error(
                ErrorCode::Indeterminate,
                "pending tools require reconciliation",
            )));
        }
        if request.status == RunStatus::Running && request.run.is_empty() {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidInput,
                "running session has no run id",
            )));
        }
        commit(host, request)
    }

    #[provide(Session::reconcile)]
    fn reconcile(
        _: &mut State,
        host: &Host<'_>,
        request: ReconcileTool,
    ) -> CallResult<SessionSnapshot, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        let mut current = load(host, &request.session)?;
        reconcile_snapshot(&mut current, &request).map_err(CallError::Domain)?;
        commit(host, current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_basic_contracts::{ToolCall, ToolOutcome};

    #[test]
    fn interrupted_tool_recovery_requires_explicit_call_id() {
        let mut state = empty("s1");
        state.status = RunStatus::Indeterminate;
        state.run = "r1".into();
        state.pending = vec![
            ToolCall {
                id: "a".into(),
                tool: "uppercase".into(),
                arguments: "x".into(),
            },
            ToolCall {
                id: "b".into(),
                tool: "length".into(),
                arguments: "y".into(),
            },
        ];
        assert_eq!(
            reconcile_snapshot(
                &mut state,
                &ReconcileTool {
                    session: "s2".into(),
                    call_id: "a".into(),
                    outcome: ToolOutcome::Succeeded {
                        text: String::new(),
                    }
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidInput
        );
        assert_eq!(
            reconcile_snapshot(
                &mut state,
                &ReconcileTool {
                    session: "s1".into(),
                    call_id: "unknown".into(),
                    outcome: ToolOutcome::Succeeded {
                        text: String::new(),
                    }
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::NotFound
        );
        reconcile_snapshot(
            &mut state,
            &ReconcileTool {
                session: "s1".into(),
                call_id: "a".into(),
                outcome: ToolOutcome::Succeeded {
                    text: "HELLO".into(),
                },
            },
        )
        .unwrap();
        assert_eq!(state.status, RunStatus::Indeterminate);
        assert_eq!(state.history[0].role(), Role::Tool);
        reconcile_snapshot(
            &mut state,
            &ReconcileTool {
                session: "s1".into(),
                call_id: "b".into(),
                outcome: ToolOutcome::Failed {
                    output: String::new(),
                    reason: "tool failed".into(),
                },
            },
        )
        .unwrap();
        assert_eq!(state.status, RunStatus::Running);
        assert_eq!(state.pending.len(), 0);
        assert_eq!(state.history[1].call_id(), "b");
        assert_eq!(state.history[1].error(), Some("tool failed"));
    }
}
