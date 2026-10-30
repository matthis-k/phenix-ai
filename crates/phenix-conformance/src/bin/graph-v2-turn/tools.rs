//! Basic-domain native acceptance over real selected pending tool providers.
use phenix_basic_contracts::*;
use phenix_contract::{Portable, ScopedRequest, Value};
use phenix_kernel::{
    graph::{GraphCompletion, GraphEvent, GraphExecution, GraphStatus},
    resolve::Selection,
    Kernel, Outcome,
};

fn selection(provider: &str, interface: &str, operation: &str) -> Selection {
    Selection {
        provider: provider.into(),
        interface: interface.into(),
        operation: operation.into(),
    }
}

fn ready(outcome: Outcome) -> Value {
    step_value(&outcome)
}

fn step_value(outcome: &Outcome) -> Value {
    let Outcome::Success(value) = outcome else {
        panic!("expected successful native domain call: {outcome:?}")
    };
    value.clone()
}

fn snapshot(run: &GraphExecution<'_>) -> SessionSnapshot {
    SessionSnapshot::from_value(ready(
        run.invoke_selected(
            &selection("phenix.basic.session", "phenix.basic.session@1", "open"),
            &Scope {
                session: "s1".into(),
            }
            .to_value(),
        )
        .unwrap(),
    ))
    .unwrap()
}

fn journal(run: &GraphExecution<'_>) -> EventPage {
    EventPage::from_value(ready(
        run.invoke_selected(
            &selection("phenix.basic.agent", "phenix.basic.output@1", "read"),
            &JournalQuery {
                session: "s1".into(),
                after: 0,
                limit: 100,
            }
            .to_value(),
        )
        .unwrap(),
    ))
    .unwrap()
}

fn call(
    kernel: &Kernel,
    provider: &str,
    interface: &str,
    operation: &str,
    value: Value,
) -> Outcome {
    let root = kernel
        .admit_root(selection(provider, interface, operation))
        .unwrap();
    root.invoke(&value).unwrap()
}

pub(super) fn run(kernel: &Kernel, scope: ScopedRequest, mode: &str) {
    std::env::set_var("PHENIX_TOOL_A_CONTROL", "hold");
    std::env::set_var("PHENIX_TOOL_B_CONTROL", "hold");
    let prompt = match mode {
        "fanout-mixed" => "fanout-fail:proof",
        "fanout-duplicate" => "duplicate-tools:proof",
        "fanout-invalid-json" => "invalid-tool-json",
        "fanout-invalid-type" => "invalid-tool-type",
        _ => "fanout:proof",
    };
    let rejected = matches!(
        mode,
        "fanout-duplicate" | "fanout-budget" | "fanout-invalid-json" | "fanout-invalid-type"
    );
    let mut run = kernel
        .begin_action_scoped(
            "phenix.basic.submit",
            &PromptSubmitted {
                session: "s1".into(),
                text: prompt.into(),
                limits: Limits {
                    tool_calls: (mode == "fanout-budget").then_some(1),
                    ..Limits::default()
                },
            }
            .to_value(),
            scope.clone(),
        )
        .unwrap();
    let mut pending = std::collections::BTreeSet::new();
    let mut finished = 0;
    for _ in 0..32 {
        let status = run
            .pump(|update| {
                if let GraphEvent::NodePending { node } = &update.event {
                    assert_eq!(*node, "phenix.basic.tools/each");
                    assert!(
                        pending.insert((update.root, update.activation, update.branch)),
                        "duplicate pending observation"
                    );
                }
                if matches!(update.event, GraphEvent::Finished(_)) {
                    finished += 1;
                }
            })
            .unwrap();
        if rejected && status == GraphStatus::Finished {
            break;
        }
        if pending.len() == 2 {
            break;
        }
    }
    if rejected {
        assert_eq!(run.completion(), Some(GraphCompletion::Domain));
        assert_eq!(finished, 1);
        assert!(pending.is_empty());
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
        assert!(!run
            .steps()
            .iter()
            .any(|step| step.node.starts_with("phenix.basic.tools/")));
        let Outcome::Domain(problem) = &run.steps().last().unwrap().outcome else {
            panic!("missing admission rejection")
        };
        assert_eq!(
            BasicError::from_value(problem.clone()).unwrap().code,
            if mode == "fanout-budget" {
                ErrorCode::TurnLimit
            } else if mode.starts_with("fanout-invalid-") {
                ErrorCode::InvalidInput
            } else {
                ErrorCode::InvalidReply
            }
        );
        let snapshot = snapshot(&run);
        assert_eq!(snapshot.status, RunStatus::Failed);
        assert!(snapshot.pending.is_empty());
        assert!(!snapshot
            .history
            .iter()
            .any(|message| message.role() == Role::Assistant));
        assert!(!journal(&run)
            .events
            .iter()
            .any(|event| event.kind == EventKind::ToolStarted));
        drop(run);
        std::env::remove_var("PHENIX_TOOL_A_CONTROL");
        std::env::remove_var("PHENIX_TOOL_B_CONTROL");
        return;
    }
    assert_eq!(
        pending.len(),
        2,
        "B must start while A is still physically pending"
    );
    assert_eq!(
        pending
            .iter()
            .map(|(_, _, branch)| *branch)
            .collect::<Vec<_>>(),
        [Some(0), Some(1)]
    );
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 2);
    assert_eq!(
        snapshot(&run).pending.len(),
        2,
        "durable admission must precede both effects"
    );
    assert_eq!(
        journal(&run)
            .events
            .iter()
            .filter(|event| event.kind == EventKind::ToolStarted)
            .count(),
        2
    );
    assert!(!journal(&run)
        .events
        .iter()
        .any(|event| event.kind == EventKind::ToolFinished));
    assert!(!run
        .steps()
        .iter()
        .any(|step| step.node == "phenix.basic.tools/each"));

    let batch = GraphToolBatch::from_value(step_value(
        &run.steps()
            .iter()
            .find(|step| step.node == "phenix.basic.agent/admit")
            .unwrap()
            .outcome,
    ))
    .unwrap();
    let unchanged = snapshot(&run);
    assert_eq!(unchanged.run, batch.run);
    assert_eq!(unchanged.pending, batch.calls);
    let fabricated = GraphToolCollection {
        batch: batch.clone(),
        outcomes: batch
            .invocations()
            .into_iter()
            .map(|invocation| GraphToolOutcome::Ok {
                invocation,
                text: "fabricated".into(),
            })
            .collect(),
    };
    for kind in 0..3 {
        let mut invalid = fabricated.clone();
        if kind == 0 {
            invalid.outcomes[1] = invalid.outcomes[0].clone();
        }
        if kind == 1 {
            invalid.outcomes.pop();
        }
        // Case 2 has exact active correlation but lacks durable receipts.
        for (provider, interface, operation) in [
            (
                "phenix.basic.tools",
                "phenix.basic.graph-tools@1",
                "collect",
            ),
            (
                "phenix.basic.agent",
                "phenix.basic.graph-tool-turn@1",
                "reconcile",
            ),
        ] {
            let result = run
                .invoke_selected(
                    &selection(provider, interface, operation),
                    &invalid.to_value(),
                )
                .unwrap();
            let Outcome::Domain(problem) = result else {
                panic!("active malformed collection accepted")
            };
            assert_eq!(
                BasicError::from_value(problem).unwrap().code,
                if kind < 2 {
                    ErrorCode::InvalidReply
                } else {
                    ErrorCode::Denied
                }
            );
            assert_eq!(
                snapshot(&run),
                unchanged,
                "active malformed collection mutated Session"
            );
        }
    }
    let partial = mode.starts_with("fanout-partial-");
    if partial {
        std::env::set_var("PHENIX_TOOL_B_CONTROL", "release");
        for _ in 0..3 {
            assert_eq!(run.pump(|_| {}).unwrap(), GraphStatus::Pending);
        }
        let saved = run
            .invoke_selected(
                &selection(
                    "phenix.basic.tool-receipts",
                    "phenix.basic.tool-receipts@1",
                    "read",
                ),
                &batch.invocations()[1].to_value(),
            )
            .unwrap();
        assert!(Option::<GraphToolOutcome>::from_value(ready(saved))
            .unwrap()
            .is_some());
        assert_eq!(snapshot(&run).pending.len(), 2);
    }
    if matches!(
        mode,
        "fanout-cancel"
            | "fanout-infrastructure"
            | "fanout-partial-cancel"
            | "fanout-partial-infrastructure"
    ) {
        let cancelling = mode.ends_with("cancel");
        let admitted_steps = run.steps().len();
        let mut failure = None;
        if cancelling {
            run.cancel().unwrap();
            run.cancel().unwrap();
            for _ in 0..3 {
                assert_eq!(
                    run.pump(|_| panic!("finished before drain")).unwrap(),
                    GraphStatus::Draining
                );
                assert_eq!(
                    kernel.inspect()["pending"]["physical_leases"],
                    if partial { 1 } else { 2 }
                );
            }
            if !partial {
                std::env::set_var("PHENIX_TOOL_B_CONTROL", "drain");
            }
        } else if partial {
            std::env::set_var("PHENIX_TOOL_A_CONTROL", "fail");
            failure = Some(
                run.pump(|update| {
                    if matches!(update.event, GraphEvent::Finished(_)) {
                        finished += 1;
                    }
                })
                .unwrap_err(),
            );
        } else {
            std::env::set_var("PHENIX_TOOL_B_CONTROL", "fail");
        }
        if failure.is_none() {
            for _ in 0..3 {
                assert_eq!(
                    run.pump(|_| panic!("published before sibling drain"))
                        .unwrap(),
                    GraphStatus::Draining
                );
            }
            assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
        }
        assert_eq!(snapshot(&run).pending.len(), 2);
        assert!(!run
            .steps()
            .iter()
            .any(|step| step.node == "phenix.basic.agent/tool"));
        std::env::set_var("PHENIX_TOOL_A_CONTROL", "drain");
        if failure.is_none() {
            for _ in 0..32 {
                match run.pump(|update| {
                    if matches!(update.event, GraphEvent::Finished(_)) {
                        finished += 1;
                    }
                }) {
                    Ok(GraphStatus::Finished) => break,
                    Err(problem) => {
                        failure = Some(problem);
                        break;
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(finished, 1);
        if cancelling {
            assert!(failure.is_none());
            assert_eq!(run.completion(), Some(GraphCompletion::Cancelled));
        } else {
            let failure = failure.unwrap();
            assert_eq!(failure.code, phenix_contract::Code::PluginFailed);
            assert_eq!(run.completion(), Some(GraphCompletion::Failure(&failure)));
        }
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
        assert_eq!(
            run.steps().len(),
            admitted_steps,
            "late tool success must not become an observable completion"
        );
        let saved = snapshot(&run);
        assert_eq!(saved.pending.len(), 2);
        assert!(!saved
            .history
            .iter()
            .any(|message| message.role() == Role::Tool));
        assert!(!journal(&run)
            .events
            .iter()
            .any(|event| matches!(event.kind, EventKind::ToolFinished | EventKind::Answer)));
        // A separately authorized fresh root imports receipts only after drain.
        drop(run);
        let recovery = GraphToolRecovery {
            session: "s1".into(),
            run: batch.run.clone(),
            status: if cancelling {
                RunStatus::Cancelled
            } else {
                RunStatus::Failed
            },
        };
        let saved = SessionSnapshot::from_value(ready(call(
            kernel,
            "phenix.basic.tool-receipts",
            "phenix.basic.tool-receipts@1",
            "recover",
            recovery.to_value(),
        )))
        .unwrap();
        assert_eq!(saved.pending.len(), if partial { 1 } else { 2 });
        let repeated = SessionSnapshot::from_value(ready(call(
            kernel,
            "phenix.basic.tool-receipts",
            "phenix.basic.tool-receipts@1",
            "recover",
            recovery.to_value(),
        )))
        .unwrap();
        assert_eq!(repeated, saved, "post-drain recovery must be idempotent");
        if partial {
            assert_eq!(saved.pending[0].id, "call-1");
            let known = saved
                .history
                .iter()
                .filter(|message| message.role() == Role::Tool)
                .collect::<Vec<_>>();
            assert_eq!(known.len(), 1);
            assert_eq!(
                (known[0].call_id(), known[0].text()),
                ("call-2", "pending-b:proof")
            );
            let conflicting = GraphToolOutcome::Err {
                invocation: batch.invocations()[1].clone(),
                error: error(ErrorCode::ToolFailed, "must not erase B"),
            };
            assert!(matches!(
                call(
                    kernel,
                    "phenix.basic.tool-receipts",
                    "phenix.basic.tool-receipts@1",
                    "resolve",
                    conflicting.to_value()
                ),
                Outcome::Domain(_)
            ));
        }
        let late = GraphToolOutcome::Ok {
            invocation: batch.invocations()[0].clone(),
            text: "forbidden late write".into(),
        };
        assert!(matches!(
            call(
                kernel,
                "phenix.basic.tool-receipts",
                "phenix.basic.tool-receipts@1",
                "record",
                late.to_value()
            ),
            Outcome::Domain(_)
        ));
        for pending in &saved.pending {
            let invocation = batch
                .invocations()
                .into_iter()
                .find(|invocation| invocation.call == *pending)
                .unwrap();
            ready(call(
                kernel,
                "phenix.basic.tool-receipts",
                "phenix.basic.tool-receipts@1",
                "resolve",
                GraphToolOutcome::Err {
                    invocation,
                    error: error(ErrorCode::ToolFailed, "operator resolved A"),
                }
                .to_value(),
            ));
        }
        let resolved = SessionSnapshot::from_value(ready(call(
            kernel,
            "phenix.basic.tool-receipts",
            "phenix.basic.tool-receipts@1",
            "recover",
            recovery.to_value(),
        )))
        .unwrap();
        assert!(resolved.pending.is_empty());
        assert_eq!(
            resolved
                .history
                .iter()
                .filter(|message| message.role() == Role::Tool)
                .map(|message| message.call_id())
                .collect::<Vec<_>>(),
            ["call-1", "call-2"]
        );
        if partial {
            assert!(resolved.history.iter().any(
                |message| message.call_id() == "call-2" && message.text() == "pending-b:proof"
            ));
        }
        let mut next = kernel
            .begin_action_scoped(
                "phenix.basic.submit",
                &PromptSubmitted {
                    session: "s1".into(),
                    text: "after-tool-drain".into(),
                    limits: Limits::default(),
                }
                .to_value(),
                scope,
            )
            .unwrap();
        for _ in 0..32 {
            if next.pump(|_| {}).unwrap() == GraphStatus::Finished {
                break;
            }
        }
        assert_eq!(next.completion(), Some(GraphCompletion::Success));
        let answer =
            RunResult::from_value(step_value(&next.steps().last().unwrap().outcome)).unwrap();
        assert_eq!(answer.answer, "mock:after-tool-drain");
        assert_eq!(answer.tool_calls, 0);
        drop(next);
    } else {
        std::env::set_var("PHENIX_TOOL_B_CONTROL", "release");
        for _ in 0..3 {
            assert_eq!(run.pump(|_| {}).unwrap(), GraphStatus::Pending);
        }
        let branches = run
            .steps()
            .iter()
            .filter(|step| step.node == "phenix.basic.tools/each")
            .collect::<Vec<_>>();
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].branch, Some(1));
        assert_eq!(snapshot(&run).pending.len(), 2);
        assert!(!journal(&run)
            .events
            .iter()
            .any(|event| event.kind == EventKind::ToolFinished));
        std::env::set_var("PHENIX_TOOL_A_CONTROL", "release");
        for _ in 0..64 {
            if run
                .pump(|update| {
                    if matches!(update.event, GraphEvent::Finished(_)) {
                        finished += 1;
                    }
                })
                .unwrap()
                == GraphStatus::Finished
            {
                break;
            }
        }
        assert_eq!(finished, 1);
        assert_eq!(
            run.completion(),
            Some(if mode == "fanout-mixed" {
                GraphCompletion::Domain
            } else {
                GraphCompletion::Success
            })
        );
        assert_eq!(
            run.steps()
                .iter()
                .filter(|step| step.node == "phenix.basic.tools/each")
                .map(|step| step.branch)
                .collect::<Vec<_>>(),
            [Some(1), Some(0)]
        );
        let saved = snapshot(&run);
        assert_eq!(saved.status, RunStatus::Completed);
        assert!(saved.pending.is_empty());
        assert_eq!(
            saved
                .history
                .iter()
                .filter(|message| message.role() == Role::Tool)
                .map(|message| message.call_id())
                .collect::<Vec<_>>(),
            ["call-1", "call-2"]
        );
        let result =
            RunResult::from_value(step_value(&run.steps().last().unwrap().outcome)).unwrap();
        assert_eq!((result.model_turns, result.tool_calls), (2, 2));
        assert!(result
            .answer
            .starts_with("mock:call-1:pending-a:proof|call-2:"));
        assert_eq!(
            result.answer.contains("error:controlled domain failure"),
            mode == "fanout-mixed"
        );
        let collection = run
            .steps()
            .iter()
            .find(|step| step.node == "phenix.basic.tools/collect")
            .unwrap();
        let value = step_value(&collection.outcome);
        let mut duplicate = GraphToolCollection::from_value(value.clone()).unwrap();
        duplicate.outcomes[1] = duplicate.outcomes[0].clone();
        for (provider, interface, operation, value) in [
            (
                "phenix.basic.tools",
                "phenix.basic.graph-tools@1",
                "collect",
                value.clone(),
            ),
            (
                "phenix.basic.agent",
                "phenix.basic.graph-tool-turn@1",
                "reconcile",
                value,
            ),
            (
                "phenix.basic.agent",
                "phenix.basic.graph-tool-turn@1",
                "reconcile",
                duplicate.to_value(),
            ),
        ] {
            assert!(matches!(
                run.invoke_selected(&selection(provider, interface, operation), &value)
                    .unwrap(),
                Outcome::Domain(_)
            ));
        }
        assert_eq!(
            snapshot(&run),
            saved,
            "duplicate results must not mutate the session"
        );
        assert_eq!(
            journal(&run)
                .events
                .iter()
                .filter(|event| event.kind == EventKind::ToolFinished)
                .map(|event| event.call_id.as_str())
                .collect::<Vec<_>>(),
            ["call-1", "call-2"]
        );
        assert_eq!(
            run.pump(|_| panic!("duplicate completion")).unwrap(),
            GraphStatus::Finished
        );
        drop(run);
    }
    std::env::remove_var("PHENIX_TOOL_A_CONTROL");
    std::env::remove_var("PHENIX_TOOL_B_CONTROL");
    println!("{mode}: native selected providers, correlated ordered reconciliation and physical drain passed");
}
