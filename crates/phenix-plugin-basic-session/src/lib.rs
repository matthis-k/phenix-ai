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

/// A new Session provider instance has no in-memory ownership of a
/// previously Running run. An interrupted Model-only turn can fail safely;
/// unresolved tool effects must retain their call IDs for reconciliation.
pub fn recover_interrupted(snapshot: &mut SessionSnapshot, live: bool) -> bool {
    if live || snapshot.status != RunStatus::Running {
        return false;
    }
    snapshot.status = if snapshot.pending.is_empty() {
        RunStatus::Failed
    } else {
        RunStatus::Indeterminate
    };
    true
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
    snapshot.history.push(Message {
        role: Role::Tool,
        text: resolution.text.clone(),
        calls: vec![],
        call_id: resolution.call_id.clone(),
        error: resolution.error.clone(),
    });
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
    use super::{empty, reconcile_snapshot, recover_interrupted, RECORD};
    use phenix_basic_contracts::{
        decode, encode, error, BasicError, ErrorCode, ReconcileTool, RecordKey, RunStatus, Scope,
        Session, SessionSnapshot, Storage, StorageEdit, StorageTransaction,
    };
    use phenix_sdk::{CallError, CallResult, Code, Failure, Host};
    use std::collections::BTreeSet;

    #[derive(Default)]
    pub struct State {
        // Only sessions actively checkpointed by this particular instance are
        // known-live. Reopening Running+pending after reload marks the tool
        // result indeterminate rather than invoking it a second time.
        live: BTreeSet<String>,
    }

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
        state: &mut State,
        host: &Host<'_>,
        request: Scope,
    ) -> CallResult<SessionSnapshot, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        let mut current = load(host, &request.session)?;
        if recover_interrupted(&mut current, state.live.contains(&request.session)) {
            current = commit(host, current)?;
        }
        Ok(current)
    }

    #[provide(Session::checkpoint)]
    fn checkpoint(
        state: &mut State,
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
        let session = request.session.clone();
        let is_live = request.status == RunStatus::Running;
        let result = commit(host, request)?;
        if is_live {
            state.live.insert(session);
        } else {
            state.live.remove(&session);
        }
        Ok(result)
    }

    #[provide(Session::reconcile)]
    fn reconcile(
        state: &mut State,
        host: &Host<'_>,
        request: ReconcileTool,
    ) -> CallResult<SessionSnapshot, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        let mut current = load(host, &request.session)?;
        reconcile_snapshot(&mut current, &request).map_err(CallError::Domain)?;
        let result = commit(host, current)?;
        if result.status == RunStatus::Running {
            state.live.insert(result.session.clone());
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_basic_contracts::ToolCall;


    #[test]
    fn orphaned_model_run_fails_but_pending_tool_calls_need_reconciliation() {
        let mut model = empty("s1");
        model.status = RunStatus::Running;
        model.run = "r1".into();
        assert!(!recover_interrupted(&mut model, true));
        assert_eq!(model.status, RunStatus::Running);
        assert!(recover_interrupted(&mut model, false));
        assert_eq!(model.status, RunStatus::Failed);
        assert!(!recover_interrupted(&mut model, false));

        let mut tool = empty("s2");
        tool.status = RunStatus::Running;
        tool.run = "r2".into();
        tool.pending.push(ToolCall {
            id: "external-call".into(),
            tool: "shell".into(),
            arguments: String::new(),
        });
        assert!(recover_interrupted(&mut tool, false));
        assert_eq!(tool.status, RunStatus::Indeterminate);
        assert_eq!(tool.pending[0].id, "external-call");
        assert!(!recover_interrupted(&mut tool, false));
    }

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
                    text: String::new(),
                    error: None
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
                    text: String::new(),
                    error: None
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
                text: "HELLO".into(),
                error: None,
            },
        )
        .unwrap();
        assert_eq!(state.status, RunStatus::Indeterminate);
        assert_eq!(state.history[0].role, Role::Tool);
        reconcile_snapshot(
            &mut state,
            &ReconcileTool {
                session: "s1".into(),
                call_id: "b".into(),
                text: String::new(),
                error: Some("tool failed".into()),
            },
        )
        .unwrap();
        assert_eq!(state.status, RunStatus::Running);
        assert_eq!(state.pending.len(), 0);
        assert_eq!(state.history[1].call_id, "b");
        assert_eq!(state.history[1].error.as_deref(), Some("tool failed"));
    }
}
