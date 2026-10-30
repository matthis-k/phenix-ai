//! Session aggregate over selected revision-checked Storage.
//! The kernel provides admission and cancellation, not durable session logic.
use phenix_basic_contracts::{
    error, BasicError, ErrorCode, Message, ReconcileTool, RunStatus, SessionSnapshot, Usage,
};

const RECORD: &str = "session.snapshot.v1";

/// Validate the same structural invariants at the serialized Storage boundary
/// and before a new checkpoint. A decodable snapshot is not necessarily a
/// valid durable session or an unambiguous set of pending tool effects.
fn well_formed_snapshot(snapshot: &SessionSnapshot) -> bool {
    if snapshot.status == RunStatus::Running && snapshot.run.is_empty() {
        return false;
    }
    let mut ids = std::collections::BTreeSet::new();
    snapshot
        .pending
        .iter()
        .all(|call| !call.id.is_empty() && ids.insert(call.id.as_str()))
}

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

fn recover_graph_snapshot(
    snapshot: &mut SessionSnapshot,
    request: &phenix_basic_contracts::GraphToolRecoveryState,
) -> Result<(), BasicError> {
    use phenix_basic_contracts::Role;
    request.validate()?;
    if snapshot.session != request.batch.session
        || snapshot.run != request.batch.run
        || !matches!(
            snapshot.status,
            RunStatus::Running
                | RunStatus::Indeterminate
                | RunStatus::Failed
                | RunStatus::Cancelled
        )
    {
        return Err(error(
            ErrorCode::Conflict,
            "receipt recovery does not own this session run",
        ));
    }
    let assistant = snapshot
        .history
        .iter()
        .rposition(|message| message.role() == Role::Assistant)
        .filter(|index| snapshot.history[*index].calls() == request.batch.calls)
        .ok_or_else(|| {
            error(
                ErrorCode::Conflict,
                "receipt recovery assistant batch changed",
            )
        })?;
    let tail = &snapshot.history[assistant + 1..];
    let mut seen = std::collections::BTreeSet::new();
    for message in tail {
        let outcome = request
            .outcomes
            .iter()
            .flatten()
            .find(|outcome| outcome.invocation().call.id == message.call_id())
            .ok_or_else(|| {
                error(
                    ErrorCode::Conflict,
                    "existing tool result has no matching immutable receipt",
                )
            })?;
        if !seen.insert(message.call_id())
            || *message != Message::tool(message.call_id(), outcome.tool_outcome())
        {
            return Err(error(
                ErrorCode::Conflict,
                "receipt recovery cannot replace a tool result",
            ));
        }
    }
    if snapshot
        .pending
        .iter()
        .any(|call| !request.batch.calls.contains(call))
        || request.batch.calls.iter().any(|call| {
            snapshot.pending.iter().any(|pending| pending == call)
                == seen.contains(call.id.as_str())
        })
    {
        return Err(error(
            ErrorCode::Conflict,
            "receipt recovery pending membership changed",
        ));
    }
    let mut history = snapshot.history[..assistant + 1].to_vec();
    let mut pending = Vec::new();
    for (call, outcome) in request.batch.calls.iter().zip(&request.outcomes) {
        if let Some(outcome) = outcome {
            history.push(Message::tool(call.id.clone(), outcome.tool_outcome()));
        } else {
            pending.push(call.clone());
        }
    }
    snapshot.history = history;
    snapshot.pending = pending;
    snapshot.status = if snapshot.pending.is_empty() {
        request.status.clone()
    } else {
        RunStatus::Indeterminate
    };
    Ok(())
}

#[phenix_sdk::plugin(
    id = "phenix.basic.session",
    version = "1.0.0",
    provide(phenix_basic_contracts::Session, slot = "Session"),
    provide(phenix_basic_contracts::GraphToolSession, slot = "GraphRecovery"),
    require(phenix_basic_contracts::Storage, slot = "Storage")
)]
mod plugin {
    use super::{empty, reconcile_snapshot, well_formed_snapshot, RECORD};
    use phenix_basic_contracts::{
        decode, encode, error, BasicError, ErrorCode, ReconcileTool, RecordKey, RunStatus, Scope,
        Session, SessionSnapshot, Storage,
    };
    use phenix_sdk::{CallError, CallResult, Host};
    use std::collections::BTreeMap;

    /// Live sessions belong to the plugin instance. Storage holds recovery
    /// snapshots; a validated snapshot is restored only on first use.
    #[derive(Default)]
    pub struct State {
        sessions: BTreeMap<String, SessionSnapshot>,
    }

    impl State {
        fn current(
            &mut self,
            host: &Host<'_>,
            session: &str,
        ) -> CallResult<&SessionSnapshot, BasicError> {
            if !self.sessions.contains_key(session) {
                let restored = load(host, session)?;
                self.sessions.insert(session.into(), restored);
            }
            Ok(&self.sessions[session])
        }

        fn committed(&mut self, snapshot: SessionSnapshot) -> SessionSnapshot {
            self.sessions
                .insert(snapshot.session.clone(), snapshot.clone());
            snapshot
        }

        fn invalidate(&mut self, session: &str) {
            self.sessions.remove(session);
        }
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
                if snapshot.revision != record.revision
                    || snapshot.session != session
                    || !well_formed_snapshot(&snapshot)
                {
                    return Err(CallError::Domain(error(
                        ErrorCode::StorageFailed,
                        "snapshot revision, session or state is invalid",
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
        phenix_basic_contracts::transact_one(
            host,
            "Storage",
            &snapshot.session,
            RECORD,
            expected,
            data,
        )?;
        Ok(snapshot)
    }

    #[provide(Session::open)]
    fn open(
        state: &mut State,
        host: &Host<'_>,
        request: Scope,
    ) -> CallResult<SessionSnapshot, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        // Opening the session cannot prove that a previous process died.
        // Preserve the saved Running/Indeterminate status without speculation.
        Ok(state.current(host, &request.session)?.clone())
    }

    #[provide(Session::checkpoint)]
    fn checkpoint(
        state: &mut State,
        host: &Host<'_>,
        request: SessionSnapshot,
    ) -> CallResult<SessionSnapshot, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        let prior = state.current(host, &request.session)?;
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
        if !well_formed_snapshot(&request) {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidInput,
                "session checkpoint has invalid running state or pending calls",
            )));
        }
        let session = request.session.clone();
        match commit(host, request) {
            Ok(committed) => Ok(state.committed(committed)),
            Err(failure) => {
                // A failed/ambiguous CAS may mean a different instance wrote,
                // or storage committed but failed to acknowledge. Reload next
                // time rather than advancing stale private state.
                state.invalidate(&session);
                Err(failure)
            }
        }
    }

    #[provide(Session::reconcile)]
    fn reconcile(
        state: &mut State,
        host: &Host<'_>,
        request: ReconcileTool,
    ) -> CallResult<SessionSnapshot, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        let mut current = state.current(host, &request.session)?.clone();
        reconcile_snapshot(&mut current, &request).map_err(CallError::Domain)?;
        match commit(host, current) {
            Ok(committed) => Ok(state.committed(committed)),
            Err(failure) => {
                state.invalidate(&request.session);
                Err(failure)
            }
        }
    }

    #[provide(phenix_basic_contracts::GraphToolSession::recover)]
    fn recover_graph(
        state: &mut State,
        host: &Host<'_>,
        request: phenix_basic_contracts::GraphToolRecoveryState,
    ) -> CallResult<SessionSnapshot, BasicError> {
        phenix_basic_contracts::check_session(host, &request.batch.session)?;
        let before = state.current(host, &request.batch.session)?.clone();
        let mut current = before.clone();
        super::recover_graph_snapshot(&mut current, &request).map_err(CallError::Domain)?;
        if current == before {
            return Ok(before);
        }
        match commit(host, current) {
            Ok(committed) => Ok(state.committed(committed)),
            Err(failure) => {
                state.invalidate(&request.batch.session);
                Err(failure)
            }
        }
    }
    #[cfg(test)]
    mod state_tests {
        use super::*;

        #[test]
        fn live_sessions_are_isolated_and_ambiguous_commit_can_be_invalidated() {
            let mut state = State::default();
            let mut one = empty("one");
            one.revision = 2;
            let two = empty("two");
            state.committed(one.clone());
            state.committed(two.clone());
            assert_eq!(state.sessions["one"], one);
            assert_eq!(state.sessions["two"], two);
            state.invalidate("one");
            assert!(!state.sessions.contains_key("one"));
            assert_eq!(state.sessions["two"], two);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_receipts_preserve_known_b_and_resolve_only_a_in_original_order() {
        use phenix_basic_contracts::{
            GraphToolBatch, GraphToolOutcome, GraphToolRecoveryState, ToolCall,
        };
        let batch = GraphToolBatch {
            session: "one".into(),
            run: "run-1".into(),
            batch: 1,
            calls: vec![
                ToolCall {
                    id: "a".into(),
                    tool: "pending-a".into(),
                    arguments: "\"input\"".into(),
                },
                ToolCall {
                    id: "b".into(),
                    tool: "pending-b".into(),
                    arguments: "\"input\"".into(),
                },
            ],
        };
        let invocations = batch.invocations();
        let b = GraphToolOutcome::Ok {
            invocation: invocations[1].clone(),
            text: "known B".into(),
        };
        let mut snapshot = empty("one");
        snapshot.status = RunStatus::Running;
        snapshot.run = batch.run.clone();
        snapshot.history = vec![
            Message::user("prompt"),
            Message::assistant(String::new(), batch.calls.clone()),
        ];
        snapshot.pending = batch.calls.clone();
        let mut request = GraphToolRecoveryState {
            batch,
            outcomes: vec![None, Some(b.clone())],
            status: RunStatus::Cancelled,
        };
        recover_graph_snapshot(&mut snapshot, &request).unwrap();
        assert_eq!(
            snapshot
                .pending
                .iter()
                .map(|call| call.id.as_str())
                .collect::<Vec<_>>(),
            ["a"]
        );
        assert_eq!(snapshot.status, RunStatus::Indeterminate);
        assert_eq!(
            snapshot.history.last().unwrap(),
            &Message::tool("b", b.tool_outcome())
        );
        let retained = snapshot.clone();
        let mut forgetting_b = request.clone();
        forgetting_b.outcomes[1] = None;
        assert!(recover_graph_snapshot(&mut snapshot, &forgetting_b).is_err());
        assert_eq!(snapshot, retained);
        request.outcomes[0] = Some(GraphToolOutcome::Err {
            invocation: invocations[0].clone(),
            error: error(ErrorCode::ToolFailed, "operator resolved A"),
        });
        recover_graph_snapshot(&mut snapshot, &request).unwrap();
        assert!(snapshot.pending.is_empty());
        assert_eq!(snapshot.status, RunStatus::Cancelled);
        assert_eq!(snapshot.history[2].call_id(), "a");
        assert_eq!(snapshot.history[3], Message::tool("b", b.tool_outcome()));
        let resolved = snapshot.clone();
        recover_graph_snapshot(&mut snapshot, &request).unwrap();
        assert_eq!(snapshot, resolved);
    }
    use phenix_basic_contracts::{Role, ToolCall, ToolOutcome};

    #[test]
    fn persistent_session_rejects_running_without_run_and_duplicate_pending_ids() {
        let mut state = empty("s1");
        assert!(well_formed_snapshot(&state));
        state.status = RunStatus::Running;
        assert!(!well_formed_snapshot(&state));
        state.run = "run-1".into();
        assert!(well_formed_snapshot(&state));
        let pending = ToolCall {
            id: "call-1".into(),
            tool: "uppercase".into(),
            arguments: r#""hello""#.into(),
        };
        state.pending = vec![pending.clone(), pending];
        assert!(!well_formed_snapshot(&state));
        state.pending[1].id = "call-2".into();
        assert!(well_formed_snapshot(&state));
        state.pending[0].id.clear();
        assert!(!well_formed_snapshot(&state));
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
