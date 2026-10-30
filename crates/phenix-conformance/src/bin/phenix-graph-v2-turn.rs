//! Native acceptance for repeated graph-managed Basic turns.
//! Usage: phenix-graph-v2-turn COMPOSITION (with a fresh SQLite database).
//! Optional mode: deny-tool, pending, cancel, or fail-model. Controlled modes
//! require the scripted Model's controlled-pending fixture feature.
use phenix_basic_contracts::{
    BasicError, ErrorCode, EventKind, EventPage, JournalQuery, Limits, PromptSubmitted, RunResult,
    RunStatus, Scope, SessionSnapshot,
};
use phenix_contract::{graph::v2::*, Portable, ScopedRequest};
use phenix_kernel::{
    graph::{
        v2::{OperationSecurity, SourceOwnerPolicy, TrustedCompositionPolicy},
        GraphCompletion, GraphEvent, GraphStatus,
    },
    resolve::{Composition, Prepared, Selection},
    Call, Outcome, Poll,
};

fn main() {
    let path = std::env::args().nth(1).expect("COMPOSITION");
    let mut composition: Composition =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mode = std::env::args().nth(2);
    let deny_tool = mode.as_deref() == Some("deny-tool");
    let controlled = matches!(mode.as_deref(), Some("pending" | "cancel" | "fail-model"));
    let repeated = matches!(
        mode.as_deref(),
        Some("rounds" | "model-budget" | "tool-budget")
    );
    let budget = matches!(mode.as_deref(), Some("model-budget" | "tool-budget"));
    let prompt = match mode.as_deref() {
        Some("final") => "graph-proof",
        Some("tool-failure") => "tools-fail:graph-proof",
        _ if repeated => "rounds:graph-proof",
        _ => "tool:graph-proof",
    };
    if deny_tool {
        composition.grants.retain(|grant| {
            !(grant.caller == "phenix.basic.agent"
                && grant.interface == "phenix.basic.tool@1"
                && grant.operation == "execute")
        });
    }
    let label = PortSecurity {
        confidentiality: Classification::Confidential,
        integrity: IntegrityClass::Untrusted,
    };
    let mut policy = TrustedCompositionPolicy::default();
    policy.source_owners.insert(
        "phenix.basic.agent".into(),
        SourceOwnerPolicy {
            owner_ceiling: Classification::Confidential,
            composition_ceiling: Classification::Confidential,
        },
    );
    policy
        .sources
        .insert("phenix.basic.agent/prompt".into(), label);
    for key in [
        "phenix.basic.turn-prepare@1/prepare",
        "phenix.basic.graph-turn@1/model",
        "phenix.basic.graph-turn@1/decide",
        "phenix.basic.graph-turn@1/tool",
        "phenix.basic.graph-turn@1/finish",
    ] {
        policy.operations.insert(
            ("phenix.basic.agent".into(), key.into()),
            OperationSecurity {
                input: label,
                ok: label,
                err: label,
            },
        );
    }
    let scope = ScopedRequest {
        authority: composition.authority.clone(),
        resource: composition.resource.clone(),
    };
    let kernel = Prepared::prepare_graph_v2(composition, &policy)
        .unwrap()
        .load()
        .unwrap();
    let direct_root = kernel
        .admit_root(Selection {
            interface: "phenix.basic.input@1".into(),
            operation: "submit".into(),
            provider: "phenix.basic.agent".into(),
        })
        .unwrap();
    let direct = direct_root
        .begin(
            &PromptSubmitted {
                session: "s1".into(),
                text: "must-not-bypass-graph".into(),
                limits: Limits::default(),
            }
            .to_value(),
        )
        .unwrap();
    let direct = match direct {
        Call::Ready(outcome) => outcome,
        Call::Pending(ticket) => match direct_root.poll(&ticket).unwrap() {
            Poll::Ready(result) => result.unwrap(),
            other => panic!("direct submit did not reject immediately: {other:?}"),
        },
    };
    let Outcome::Domain(value) = direct else {
        panic!("direct submit admitted: {direct:?}")
    };
    assert_eq!(
        BasicError::from_value(value).unwrap().code,
        ErrorCode::Denied
    );
    drop(direct_root);
    if controlled {
        std::env::set_var("PHENIX_SCRIPTED_MODEL_CONTROL", "hold");
    }
    let mut run = kernel
        .begin_action_scoped(
            "phenix.basic.submit",
            &PromptSubmitted {
                session: "s1".into(),
                text: prompt.into(),
                limits: Limits {
                    model_turns: (mode.as_deref() == Some("model-budget")).then_some(2),
                    tool_calls: (mode.as_deref() == Some("tool-budget")).then_some(1),
                    ..Limits::default()
                },
            }
            .to_value(),
            scope,
        )
        .unwrap();
    let mut completions = 0;
    if controlled {
        assert_eq!(run.pump(|_| {}).unwrap(), GraphStatus::Pending);
        assert_eq!(run.steps().len(), 1);
        assert_eq!(run.steps()[0].node, "phenix.basic.agent/prepare");
        assert!(run.completion().is_none());
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 2);
        // Repeated polls must neither re-admit the stage nor start another Model.
        assert_eq!(run.pump(|_| {}).unwrap(), GraphStatus::Pending);
        assert_eq!(run.steps().len(), 1);
        if mode.as_deref() == Some("cancel") {
            run.cancel().unwrap();
            assert_eq!(run.pump(|_| {}).unwrap(), GraphStatus::Draining);
            assert!(run.completion().is_none());
            assert_eq!(kernel.inspect()["pending"]["physical_leases"], 2);
            std::env::set_var("PHENIX_SCRIPTED_MODEL_CONTROL", "drain");
            for _ in 0..8 {
                if run
                    .pump(|update| {
                        if matches!(update.event, GraphEvent::Finished(_)) {
                            completions += 1;
                        }
                    })
                    .unwrap()
                    == GraphStatus::Finished
                {
                    break;
                }
            }
            assert_eq!(run.completion(), Some(GraphCompletion::Cancelled));
            assert_eq!(completions, 1);
            assert_eq!(run.steps().len(), 1, "late Model success resumed graph");
            assert_eq!(
                run.pump(|_| panic!("duplicate completion")).unwrap(),
                GraphStatus::Finished
            );
            assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
            drop(run);
            std::env::remove_var("PHENIX_SCRIPTED_MODEL_CONTROL");
            assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
            kernel.shutdown().unwrap();
            return;
        }
        if mode.as_deref() == Some("fail-model") {
            std::env::set_var("PHENIX_SCRIPTED_MODEL_CONTROL", "fail");
            let failure = run
                .pump(|update| {
                    if matches!(update.event, GraphEvent::Finished(_)) {
                        completions += 1;
                    }
                })
                .unwrap_err();
            assert_eq!(failure.code, phenix_contract::Code::PluginFailed);
            assert_eq!(run.completion(), Some(GraphCompletion::Failure(&failure)));
            assert_eq!(run.steps().len(), 1);
            assert_eq!(completions, 1);
            assert_eq!(
                run.pump(|_| panic!("duplicate completion")).unwrap_err(),
                failure
            );
            assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
            let Outcome::Success(value) = run
                .invoke_selected(
                    &Selection {
                        interface: "phenix.basic.session@1".into(),
                        operation: "open".into(),
                        provider: "phenix.basic.session".into(),
                    },
                    &Scope {
                        session: "s1".into(),
                    }
                    .to_value(),
                )
                .unwrap()
            else {
                panic!("failed Model session read failed")
            };
            assert_eq!(
                SessionSnapshot::from_value(value).unwrap().status,
                RunStatus::Failed
            );
            drop(run);
            std::env::remove_var("PHENIX_SCRIPTED_MODEL_CONTROL");
            assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
            kernel.shutdown().unwrap();
            return;
        }
        std::env::set_var("PHENIX_SCRIPTED_MODEL_CONTROL", "release");
        // Every Model stage suspends, including both repeated continuations.
        let mut continuation_pending = std::collections::BTreeSet::new();
        for _ in 0..8 {
            let status = run
                .pump(|update| {
                    if matches!(update.event, GraphEvent::Finished(_)) {
                        completions += 1;
                    }
                })
                .unwrap();
            if status == GraphStatus::Pending && kernel.inspect()["pending"]["physical_leases"] == 2
            {
                let completed_models = run
                    .steps()
                    .iter()
                    .filter(|step| step.node == "phenix.basic.agent/model")
                    .count();
                assert_eq!(run.steps().len(), 1 + 3 * completed_models);
                if completed_models > 0 {
                    // Repeated polls of one admission are not new continuations.
                    continuation_pending.insert(completed_models);
                }
                assert!(run.completion().is_none());
            }
            if status == GraphStatus::Finished {
                break;
            }
        }
        assert_eq!(
            continuation_pending.len(),
            if repeated { 2 } else { 1 },
            "continuation did not suspend in every round"
        );
        assert_eq!(run.completion(), Some(GraphCompletion::Success));
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
        std::env::remove_var("PHENIX_SCRIPTED_MODEL_CONTROL");
    }
    let mut pumped = Ok(GraphStatus::Pending);
    for _ in 0..8 {
        pumped = run.pump(|update| {
            if matches!(update.event, GraphEvent::Finished(_)) {
                completions += 1;
            }
        });
        if matches!(pumped, Ok(GraphStatus::Finished) | Err(_)) {
            break;
        }
    }
    if deny_tool {
        assert_eq!(pumped.unwrap_err().code, phenix_contract::Code::Denied);
        assert_ne!(run.completion(), Some(GraphCompletion::Success));
        let Outcome::Success(value) = run
            .invoke_selected(
                &Selection {
                    interface: "phenix.basic.session@1".into(),
                    operation: "open".into(),
                    provider: "phenix.basic.session".into(),
                },
                &Scope {
                    session: "s1".into(),
                }
                .to_value(),
            )
            .unwrap()
        else {
            panic!("session read failed")
        };
        let session = SessionSnapshot::from_value(value).unwrap();
        assert_eq!(session.status, RunStatus::Failed);
        assert_eq!(session.pending.len(), 1);
        drop(run);
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
        kernel.shutdown().unwrap();
        return;
    }
    assert_eq!(pumped.unwrap(), GraphStatus::Finished);
    assert_eq!(completions, 1);
    if budget {
        assert_eq!(run.completion(), Some(GraphCompletion::Domain));
        let Outcome::Domain(value) = &run.steps().last().unwrap().outcome else {
            panic!("budget did not stop the graph turn")
        };
        assert_eq!(
            BasicError::from_value(value.clone()).unwrap().code,
            ErrorCode::TurnLimit
        );
        assert_eq!(
            run.steps()
                .iter()
                .filter(|step| step.node == "phenix.basic.agent/model")
                .count(),
            2
        );
        assert!(!run
            .steps()
            .iter()
            .any(|step| step.node == "phenix.basic.agent/finish"));
        let Outcome::Success(value) = run
            .invoke_selected(
                &Selection {
                    interface: "phenix.basic.session@1".into(),
                    operation: "open".into(),
                    provider: "phenix.basic.session".into(),
                },
                &Scope {
                    session: "s1".into(),
                }
                .to_value(),
            )
            .unwrap()
        else {
            panic!("budget session read failed")
        };
        let snapshot = SessionSnapshot::from_value(value).unwrap();
        assert_eq!(snapshot.status, RunStatus::Failed);
        assert!(snapshot.pending.is_empty());
        assert_eq!(
            snapshot
                .history
                .iter()
                .filter(|message| message.role() == phenix_basic_contracts::Role::Tool)
                .count(),
            if mode.as_deref() == Some("model-budget") {
                2
            } else {
                1
            }
        );
        drop(run);
        kernel.shutdown().unwrap();
        return;
    }
    assert_eq!(run.completion(), Some(GraphCompletion::Success));
    let rounds = if mode.as_deref() == Some("final") {
        0
    } else if repeated {
        2
    } else {
        1
    };
    let mut expected = vec!["phenix.basic.agent/prepare"];
    for _ in 0..rounds {
        expected.extend([
            "phenix.basic.agent/model",
            "phenix.basic.agent/decide",
            "phenix.basic.agent/tool",
        ]);
    }
    expected.extend([
        "phenix.basic.agent/model",
        "phenix.basic.agent/decide",
        "phenix.basic.agent/finish",
    ]);
    assert_eq!(
        run.steps()
            .iter()
            .map(|step| step.node.as_str())
            .collect::<Vec<_>>(),
        expected
    );
    let Outcome::Success(value) = &run.steps().last().unwrap().outcome else {
        panic!("missing final answer")
    };
    let result = RunResult::from_value(value.clone()).unwrap();
    let calls = if mode.as_deref() == Some("tool-failure") {
        2
    } else {
        rounds
    };
    if mode.as_deref() == Some("tool-failure") {
        assert!(result.answer.contains("call-1:error:"));
        assert!(result.answer.contains("call-2:11"));
    } else {
        assert_eq!(
            result.answer,
            match rounds {
                0 => "mock:graph-proof",
                2 => "mock:rounds:GRAPH-PROOF",
                _ => "mock:call-1:GRAPH-PROOF",
            }
        );
    }
    assert_eq!((result.model_turns, result.tool_calls), (rounds + 1, calls));
    let Outcome::Success(value) = run
        .invoke_selected(
            &Selection {
                interface: "phenix.basic.output@1".into(),
                operation: "read".into(),
                provider: "phenix.basic.agent".into(),
            },
            &JournalQuery {
                session: "s1".into(),
                after: 0,
                limit: 100,
            }
            .to_value(),
        )
        .unwrap()
    else {
        panic!("journal read failed")
    };
    let page = EventPage::from_value(value).unwrap();
    let count = |kind| {
        page.events
            .iter()
            .filter(|event| event.kind == kind)
            .count() as u64
    };
    assert_eq!(count(EventKind::PromptAccepted), 1);
    assert_eq!(count(EventKind::ModelStarted), rounds + 1);
    assert_eq!(count(EventKind::ToolStarted), calls);
    assert_eq!(count(EventKind::ToolFinished), calls);
    assert_eq!(count(EventKind::Answer), 1);
    assert_eq!(count(EventKind::Failure), 0);
    assert_eq!(
        run.pump(|_| panic!("duplicate completion")).unwrap(),
        GraphStatus::Finished
    );
    drop(run);
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
    kernel.shutdown().unwrap();
    println!("graph-v2 turn: explicit branch, repeated deferred turns, accounting and exactly-once completion passed");
}
