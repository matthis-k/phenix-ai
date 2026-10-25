//! Native black-box B5 graph gate. Frontends submit the discovered action;
//! they do not call a special agent entry point.
use phenix_basic_contracts::{
    BasicError, ContextItem, ContextSet, ErrorCode, EventPage, JournalQuery, Limits,
    PromptSubmitted, RunResult, Scope, Skill, SkillSet,
};
use phenix_contract::{Authority, Code, Portable, ScopedRequest};
use phenix_kernel::{
    graph::{GraphCompletion, GraphStatus},
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Call, Outcome, Poll,
};
use std::{collections::BTreeSet, fs, path::Path};

fn selection(key: &str, provider: &str) -> Selection {
    let (interface, operation) = key.split_once('/').unwrap();
    Selection {
        interface: interface.into(),
        operation: operation.into(),
        provider: provider.into(),
    }
}
fn invoke<T: Portable>(
    kernel: &phenix_kernel::Kernel,
    key: &str,
    provider: &str,
    input: impl Portable,
) -> Result<Result<T, BasicError>, phenix_contract::Failure> {
    let root = kernel.admit_root(selection(key, provider))?;
    let outcome = match root.begin(&input.to_value())? {
        Call::Ready(outcome) => outcome,
        Call::Pending(ticket) => loop {
            match root.poll(&ticket)? {
                Poll::Pending | Poll::Draining | Poll::Cancelled { quiescent: false } => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Poll::Ready(result) => {
                    root.release(&ticket)?;
                    break result?;
                }
                Poll::Cancelled { quiescent: true } => {
                    root.release(&ticket)?;
                    return Err(phenix_contract::Failure::new(
                        Code::ScopeRejected,
                        "unexpected test cancellation",
                    ));
                }
            }
        },
    };
    match outcome {
        Outcome::Success(value) => Ok(Ok(T::from_value(value)?)),
        Outcome::Domain(value) => Ok(Err(BasicError::from_value(value)?)),
    }
}
fn submit(
    kernel: &phenix_kernel::Kernel,
    scope: &ScopedRequest,
    prompt: &str,
    limits: Limits,
) -> Result<RunResult, BasicError> {
    let mut execution = kernel
        .begin_action_scoped(
            "phenix.basic.submit",
            &PromptSubmitted {
                session: "s1".into(),
                text: prompt.into(),
                limits,
            }
            .to_value(),
            scope.clone(),
        )
        .expect("graph admission must have no kernel failures");
    while let GraphStatus::Pending | GraphStatus::Draining = execution
        .pump(|_| {})
        .expect("graph execution must have no kernel failures")
    {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let steps = execution.steps();
    assert!(!steps.is_empty());
    assert_eq!(steps[0].node, "phenix.basic.agent/submit");
    match &steps[0].outcome {
        Outcome::Success(value) => {
            // The completed agent run now emits a semantic success notice
            // via its separately granted, explicitly ordered graph handler.
            assert_eq!(steps.len(), 2);
            assert_eq!(steps[1].node, "phenix.basic.agent/run-finished");
            assert!(matches!(steps[1].outcome, Outcome::Success(_)));
            Ok(RunResult::from_value(value.clone()).unwrap())
        }
        Outcome::Domain(value) => {
            assert_eq!(steps.len(), 1, "failed turns must not emit completion");
            Err(BasicError::from_value(value.clone()).unwrap())
        }
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        10,
        "AGENT MODEL TOOLS MEMORY SESSION JOURNAL SQLITE DEFINITIONS OUTPUT"
    );
    let paths = args[1..8].to_vec();
    let defs = args[8].clone();
    let output = Path::new(&args[9]);
    fs::create_dir_all(output).unwrap();
    let resources = BTreeSet::from(["phenix.session.s1".to_owned()]);
    let keys = [
        ("phenix.basic.agent", "phenix.basic.input@1/submit"),
        ("phenix.basic.agent", "phenix.agent.lifecycle@1/finished"),
        ("phenix.basic.agent", "phenix.basic.output@1/read"),
        ("phenix.model.scripted", "phenix.basic.model@1/complete"),
        ("phenix.basic.tools", "phenix.basic.catalog@1/list"),
        ("phenix.basic.tools", "phenix.basic.tool@1/execute"),
        ("phenix.basic.tools", "phenix.tools.startup@1/begin"),
        ("phenix.basic.tools", "phenix.tools.settlement@1/complete"),
        ("phenix.basic.memory", "phenix.basic.skills@1/list"),
        ("phenix.basic.memory", "phenix.basic.skills@1/set"),
        ("phenix.basic.memory", "phenix.basic.context@1/list"),
        ("phenix.basic.memory", "phenix.basic.context@1/set"),
        ("phenix.basic.session", "phenix.basic.session@1/open"),
        ("phenix.basic.session", "phenix.basic.session@1/checkpoint"),
        ("phenix.basic.session", "phenix.basic.session@1/reconcile"),
        (
            "phenix.basic.event-journal",
            "phenix.basic.journal@1/append",
        ),
        ("phenix.basic.event-journal", "phenix.basic.journal@1/read"),
        ("phenix.basic.storage-sqlite", "phenix.basic.storage@1/read"),
        (
            "phenix.basic.storage-sqlite",
            "phenix.basic.storage@1/transact",
        ),
    ];
    let mut slots = Vec::new();
    for (index, (slot, provider, export)) in [
        ("Model", "phenix.model.scripted", "Model"),
        ("Catalog", "phenix.basic.tools", "Catalog"),
        ("Tools", "phenix.basic.tools", "Tools"),
        ("Skills", "phenix.basic.memory", "Skills"),
        ("Context", "phenix.basic.memory", "Context"),
        ("Session", "phenix.basic.session", "Session"),
        ("Journal", "phenix.basic.event-journal", "Journal"),
    ]
    .into_iter()
    .enumerate()
    {
        slots.push(SlotBinding {
            caller: "phenix.basic.agent".into(),
            slot: slot.into(),
            index: 0,
            provider: provider.into(),
            export: export.into(),
        });
        assert_eq!(index, slots.len() - 1);
    }
    for caller in [
        "phenix.basic.memory",
        "phenix.basic.session",
        "phenix.basic.event-journal",
    ] {
        slots.push(SlotBinding {
            caller: caller.into(),
            slot: "Storage".into(),
            index: 0,
            provider: "phenix.basic.storage-sqlite".into(),
            export: "Storage".into(),
        });
    }

    let mut permissions: Vec<(&str, &str)> = vec![
        ("root", "phenix.basic.input@1/submit"),
        ("root", "phenix.basic.output@1/read"),
        ("root", "phenix.basic.skills@1/set"),
        ("root", "phenix.basic.session@1/open"),
        ("root", "phenix.basic.context@1/set"),
        ("phenix.basic.agent", "phenix.basic.input@1/submit"),
        ("phenix.basic.agent", "phenix.agent.lifecycle@1/finished"),
        ("phenix.basic.tools", "phenix.tools.startup@1/begin"),
        ("phenix.basic.tools", "phenix.tools.settlement@1/complete"),
        ("phenix.basic.agent", "phenix.basic.journal@1/append"),
        ("phenix.basic.agent", "phenix.basic.journal@1/read"),
        ("phenix.basic.agent", "phenix.basic.model@1/complete"),
        ("phenix.basic.agent", "phenix.basic.catalog@1/list"),
        ("phenix.basic.agent", "phenix.basic.tool@1/execute"),
        ("phenix.basic.agent", "phenix.basic.skills@1/list"),
        ("phenix.basic.agent", "phenix.basic.context@1/list"),
        ("phenix.basic.agent", "phenix.basic.session@1/open"),
        ("phenix.basic.agent", "phenix.basic.session@1/checkpoint"),
        ("phenix.basic.agent", "phenix.basic.storage@1/read"),
        ("phenix.basic.agent", "phenix.basic.storage@1/transact"),
    ];
    for owner in [
        "phenix.basic.memory",
        "phenix.basic.session",
        "phenix.basic.event-journal",
    ] {
        permissions.push((owner, "phenix.basic.storage@1/read"));
        permissions.push((owner, "phenix.basic.storage@1/transact"));
    }
    let grants = permissions
        .into_iter()
        .map(|(caller, key)| {
            let (interface, operation) = key.split_once('/').unwrap();
            ScopedGrant {
                caller: caller.into(),
                interface: interface.into(),
                operation: operation.into(),
                resources: resources.clone(),
            }
        })
        .collect();
    let composition = Composition {
        format: "phenix.composition.v3".into(),
        artifacts: paths,
        definitions: vec![defs],
        entry: selection("phenix.basic.input@1/submit", "phenix.basic.agent"),
        slots,
        grants,
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: keys.into_iter().map(|(_, key)| key.to_owned()).collect(),
            resources,
        },
        resource: Some("phenix.session.s1".into()),
    };
    // Reject duplicate and undeclared grants before any native code is loaded.
    let mut invalid = composition.clone();
    invalid.grants.push(invalid.grants[0].clone());
    assert_eq!(
        Prepared::prepare(invalid).err().unwrap().code,
        Code::DescriptorInvalid
    );
    let mut invalid = composition.clone();
    invalid.grants.push(ScopedGrant {
        caller: "phenix.basic.agent".into(),
        interface: "phenix.basic.tool@1".into(),
        operation: "missing".into(),
        resources: composition.authority.resources.clone(),
    });
    assert_eq!(
        Prepared::prepare(invalid).err().unwrap().code,
        Code::DescriptorInvalid
    );

    let mut restricted = composition.authority.clone();
    restricted.operations.remove("phenix.basic.storage@1/read");
    let resource = composition.resource.clone();
    let submit_scope = ScopedRequest {
        authority: composition.authority.clone(),
        resource: resource.clone(),
    };
    let kernel = Prepared::prepare(composition).unwrap().load().unwrap();
    // This node may now yield while awaiting a Model import. Exercise the
    // pending-capable graph path before asserting authority denial.
    let mut denied_run = kernel
        .begin_action_scoped(
            "phenix.basic.submit",
            &PromptSubmitted {
                session: "s1".into(),
                text: "denied storage".into(),
                limits: Limits::default(),
            }
            .to_value(),
            ScopedRequest {
                authority: restricted,
                resource,
            },
        )
        .unwrap();
    while let GraphStatus::Pending | GraphStatus::Draining = denied_run.pump(|_| {}).unwrap() {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let denied = match denied_run.completion().unwrap() {
        GraphCompletion::Failure(failure) => failure.code.clone(),
        other => panic!("restricted storage unexpectedly completed: {other:?}"),
    };
    assert_eq!(denied, Code::Denied);
    drop(denied_run);
    let untouched = invoke::<phenix_basic_contracts::SessionSnapshot>(
        &kernel,
        "phenix.basic.session@1/open",
        "phenix.basic.session",
        Scope {
            session: "s1".into(),
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(untouched.status, phenix_basic_contracts::RunStatus::Idle);
    assert_eq!(untouched.checkpoint, 0);
    assert!(untouched.history.is_empty());

    // Catalog access is checked after the Basic agent has written its Running
    // checkpoint. A nested denial must not strand the durable session there.
    let mut no_catalog = submit_scope.authority.clone();
    no_catalog.operations.remove("phenix.basic.catalog@1/list");
    let mut failed_preparation = kernel
        .begin_action_scoped(
            "phenix.basic.submit",
            &PromptSubmitted {
                session: "s1".into(),
                text: "catalog denied".into(),
                limits: Limits::default(),
            }
            .to_value(),
            ScopedRequest {
                authority: no_catalog,
                resource: submit_scope.resource.clone(),
            },
        )
        .unwrap();
    while let GraphStatus::Pending | GraphStatus::Draining =
        failed_preparation.pump(|_| {}).unwrap()
    {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    match failed_preparation.completion().unwrap() {
        GraphCompletion::Failure(failure) => assert_eq!(failure.code, Code::Denied),
        other => panic!("restricted catalog unexpectedly completed: {other:?}"),
    }
    drop(failed_preparation);
    let aborted = invoke::<phenix_basic_contracts::SessionSnapshot>(
        &kernel,
        "phenix.basic.session@1/open",
        "phenix.basic.session",
        Scope {
            session: "s1".into(),
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(aborted.status, phenix_basic_contracts::RunStatus::Failed);
    assert_eq!(aborted.checkpoint, 1);
    // The next normal submit below proves the incomplete attempt did not
    // leave an undiscoverable in-memory worker or block subsequent runs.
    invoke::<()>(
        &kernel,
        "phenix.basic.skills@1/set",
        "phenix.basic.memory",
        SkillSet {
            session: "s1".into(),
            skills: vec![Skill {
                id: "rust".into(),
                description: "Rust".into(),
                instructions: "Use Rust".into(),
            }],
        },
    )
    .unwrap()
    .unwrap();
    invoke::<()>(
        &kernel,
        "phenix.basic.context@1/set",
        "phenix.basic.memory",
        ContextSet {
            session: "s1".into(),
            items: vec![ContextItem {
                id: "repo".into(),
                text: "Phenix".into(),
            }],
        },
    )
    .unwrap()
    .unwrap();

    let direct = submit(
        &kernel,
        &submit_scope,
        "hello",
        Limits {
            model_turns: Some(2),
            tool_calls: Some(2),
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(direct.answer, "mock:hello");
    assert_eq!(direct.model_turns, 1);
    let tools = submit(
        &kernel,
        &submit_scope,
        "tools:hello",
        Limits {
            model_turns: Some(2),
            tool_calls: Some(2),
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(tools.answer, "mock:call-1:HELLO|call-2:5");
    assert_eq!(tools.tool_calls, 2);
    assert_eq!(tools.model_turns, 2);
    assert!(tools.usage.output_tokens > 0);

    let output_events: EventPage = invoke(
        &kernel,
        "phenix.basic.output@1/read",
        "phenix.basic.agent",
        JournalQuery {
            session: "s1".into(),
            after: 0,
            limit: 100,
        },
    )
    .unwrap()
    .unwrap();
    assert!(output_events
        .events
        .iter()
        .any(|event| event.text == tools.answer));
    assert!(output_events
        .events
        .iter()
        .any(|event| event.call_id == "call-1"));
    let denied: Result<RunResult, BasicError> = invoke(
        &kernel,
        "phenix.basic.input@1/submit",
        "phenix.basic.agent",
        PromptSubmitted {
            session: "s2".into(),
            text: "bad".into(),
            limits: Limits::default(),
        },
    )
    .unwrap();
    assert_eq!(denied.unwrap_err().code, ErrorCode::Denied);

    let failed = submit(&kernel, &submit_scope, "model-error", Limits::default()).unwrap_err();
    assert_eq!(failed.code, ErrorCode::ModelFailed);
    let failed_turn = submit(
        &kernel,
        &submit_scope,
        "tools:limit",
        Limits {
            model_turns: Some(1),
            tool_calls: Some(2),
            ..Limits::default()
        },
    )
    .unwrap_err();
    assert_eq!(failed_turn.code, ErrorCode::TurnLimit);
    let persistent = invoke::<phenix_basic_contracts::SessionSnapshot>(
        &kernel,
        "phenix.basic.session@1/open",
        "phenix.basic.session",
        Scope {
            session: "s1".into(),
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(persistent.status, phenix_basic_contracts::RunStatus::Failed);

    fs::write(
        output.join("agent.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "format": "phenix.b5.graph-agent.v1",
            "cases": ["graph-discovered-action", "complete-history", "multiple-tools",
                "skills-and-context-imports", "durable-session-checkpoints",
                "journal-output", "cross-session-denial", "model-error",
                "caller-turn-limit", "usage", "invalid-grant-rejection",
                "nested-storage-attenuation-no-effect",
                "failed-preparation-does-not-strand-running-session"],
            "scripted_model_only": true,
            "pending_streaming_or_frontend_parity": false
        }))
        .unwrap(),
    )
    .unwrap();
    println!("B5 scripted native graph gate passed");
}
