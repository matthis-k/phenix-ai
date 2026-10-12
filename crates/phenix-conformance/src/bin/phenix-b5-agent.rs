//! Native black-box B5 graph gate. Frontends submit the discovered action;
//! they do not call a special agent entry point.
use phenix_basic_contracts::{
    BasicError, ContextItem, ContextSet, ErrorCode, EventPage, JournalQuery, Limits,
    PromptSubmitted, RunResult, Scope, Skill, SkillSet,
};
use phenix_contract::{Authority, Portable};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Outcome,
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
    match root.invoke(&input.to_value())? {
        Outcome::Success(value) => Ok(Ok(T::from_value(value)?)),
        Outcome::Domain(value) => Ok(Err(BasicError::from_value(value)?)),
    }
}
fn submit(kernel: &phenix_kernel::Kernel, prompt: &str, limits: Limits) -> Result<RunResult, BasicError> {
    let steps = kernel.submit_action(
        "phenix.basic.submit",
        &PromptSubmitted {
            session: "s1".into(),
            text: prompt.into(),
            limits,
        }.to_value(),
    ).expect("graph execution must have no kernel failures");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].node, "phenix.basic.agent/submit");
    match &steps[0].outcome {
        Outcome::Success(value) => Ok(RunResult::from_value(value.clone()).unwrap()),
        Outcome::Domain(value) => Err(BasicError::from_value(value.clone()).unwrap()),
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 10, "AGENT MODEL TOOLS MEMORY SESSION JOURNAL SQLITE DEFINITIONS OUTPUT");
    let paths = args[1..8].to_vec();
    let defs = args[8].clone();
    let output = Path::new(&args[9]);
    fs::create_dir_all(output).unwrap();
    let resources = BTreeSet::from(["phenix.session.s1".to_owned()]);
    let keys = [
        ("phenix.basic.agent", "phenix.basic.input@1/submit"),
        ("phenix.basic.agent", "phenix.basic.output@1/read"),
        ("phenix.model.scripted", "phenix.basic.model@1/complete"),
        ("phenix.basic.tools", "phenix.basic.catalog@1/list"),
        ("phenix.basic.tools", "phenix.basic.tool@1/execute"),
        ("phenix.basic.memory", "phenix.basic.skills@1/list"),
        ("phenix.basic.memory", "phenix.basic.skills@1/set"),
        ("phenix.basic.memory", "phenix.basic.context@1/list"),
        ("phenix.basic.memory", "phenix.basic.context@1/set"),
        ("phenix.basic.session", "phenix.basic.session@1/open"),
        ("phenix.basic.session", "phenix.basic.session@1/checkpoint"),
        ("phenix.basic.session", "phenix.basic.session@1/reconcile"),
        ("phenix.basic.event-journal", "phenix.basic.journal@1/append"),
        ("phenix.basic.event-journal", "phenix.basic.journal@1/read"),
        ("phenix.basic.storage-sqlite", "phenix.basic.storage@1/read"),
        ("phenix.basic.storage-sqlite", "phenix.basic.storage@1/transact"),
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
    ].into_iter().enumerate() {
        slots.push(SlotBinding {
            caller: "phenix.basic.agent".into(),
            slot: slot.into(),
            index: 0,
            provider: provider.into(),
            export: export.into(),
        });
        assert_eq!(index, slots.len() - 1);
    }
    for caller in ["phenix.basic.memory", "phenix.basic.session", "phenix.basic.event-journal"] {
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
        ("phenix.basic.agent", "phenix.basic.journal@1/append"),
        ("phenix.basic.agent", "phenix.basic.journal@1/read"),
        ("phenix.basic.agent", "phenix.basic.model@1/complete"),
        ("phenix.basic.agent", "phenix.basic.catalog@1/list"),
        ("phenix.basic.agent", "phenix.basic.tool@1/execute"),
        ("phenix.basic.agent", "phenix.basic.skills@1/list"),
        ("phenix.basic.agent", "phenix.basic.context@1/list"),
        ("phenix.basic.agent", "phenix.basic.session@1/open"),
        ("phenix.basic.agent", "phenix.basic.session@1/checkpoint"),
    ];
    for owner in ["phenix.basic.memory", "phenix.basic.session", "phenix.basic.event-journal"] {
        permissions.push((owner, "phenix.basic.storage@1/read"));
        permissions.push((owner, "phenix.basic.storage@1/transact"));
    }
    let grants = permissions.into_iter().map(|(caller, key)| {
        let (interface, operation) = key.split_once('/').unwrap();
        ScopedGrant {
            caller: caller.into(),
            interface: interface.into(),
            operation: operation.into(),
            resources: resources.clone(),
        }
    }).collect();
    let composition = Composition {
        format: "phenix.composition.v2".into(),
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
    let kernel = Prepared::prepare(composition).unwrap().load().unwrap();
    invoke::<()>(
        &kernel, "phenix.basic.skills@1/set", "phenix.basic.memory",
        SkillSet {
            session: "s1".into(),
            skills: vec![Skill {
                id: "rust".into(),
                description: "Rust".into(),
                instructions: "Use Rust".into(),
            }],
        },
    ).unwrap().unwrap();
    invoke::<()>(
        &kernel, "phenix.basic.context@1/set", "phenix.basic.memory",
        ContextSet {
            session: "s1".into(),
            items: vec![ContextItem {
                id: "repo".into(),
                text: "Phenix".into(),
            }],
        },
    ).unwrap().unwrap();

    let direct = submit(&kernel, "hello", Limits {
        model_turns: Some(2),
        tool_calls: Some(2),
        ..Limits::default()
    }).unwrap();
    assert_eq!(direct.answer, "mock:hello");
    assert_eq!(direct.model_turns, 1);
    let tools = submit(&kernel, "tools:hello", Limits {
        model_turns: Some(2),
        tool_calls: Some(2),
        ..Limits::default()
    }).unwrap();
    assert_eq!(tools.answer, "mock:call-1:HELLO|call-2:5");
    assert_eq!(tools.tool_calls, 2);
    assert_eq!(tools.model_turns, 2);
    assert!(tools.usage.output_tokens > 0);

    let output_events: EventPage = invoke(
        &kernel, "phenix.basic.output@1/read", "phenix.basic.agent",
        JournalQuery { session: "s1".into(), after: 0, limit: 100 }
    ).unwrap().unwrap();
    assert!(output_events.events.iter().any(|event| event.text == tools.answer));
    assert!(output_events.events.iter().any(|event| event.call_id == "call-1"));
    let denied: Result<RunResult, BasicError> = invoke(
        &kernel, "phenix.basic.input@1/submit", "phenix.basic.agent",
        PromptSubmitted { session: "s2".into(), text: "bad".into(), limits: Limits::default() }
    ).unwrap();
    assert_eq!(denied.unwrap_err().code, ErrorCode::Denied);

    let failed = submit(&kernel, "model-error", Limits::default()).unwrap_err();
    assert_eq!(failed.code, ErrorCode::ModelFailed);
    let failed_turn = submit(&kernel, "tools:limit", Limits {
        model_turns: Some(1),
        tool_calls: Some(2),
        ..Limits::default()
    }).unwrap_err();
    assert_eq!(failed_turn.code, ErrorCode::TurnLimit);
    let persistent = invoke::<phenix_basic_contracts::SessionSnapshot>(
        &kernel, "phenix.basic.session@1/open", "phenix.basic.session",
        Scope { session: "s1".into() }
    ).unwrap().unwrap();
    assert_eq!(persistent.status, phenix_basic_contracts::RunStatus::Failed);

    fs::write(output.join("agent.json"), serde_json::to_vec_pretty(
        &serde_json::json!({
            "format": "phenix.b5.graph-agent.v1",
            "cases": ["graph-discovered-action", "complete-history", "multiple-tools",
                "skills-and-context-imports", "durable-session-checkpoints",
                "journal-output", "cross-session-denial", "model-error",
                "caller-turn-limit", "usage"],
            "scripted_model_only": true,
            "pending_streaming_or_frontend_parity": false
        })
    ).unwrap()).unwrap();
    println!("B5 scripted native graph gate passed");
}
