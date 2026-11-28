//! Real local effects through the ordinary P1 and P2 tool contract. The model
//! is deterministic, but file handles and bytes are real.

use phenix_agent::{
    AgentEvent,
    frontend::run_turn,
    harness::{self, Environment},
    local::{FILES, READ_TEXT, ReadableFiles, read_text},
    mock::MockModel,
    model::context::ModelContext,
    tools::Tool,
};
use phenix_graph::{ResourceGrant, ValidatedResourceSet};
use phenix_kernel::host::KernelHost;
use std::{collections::BTreeMap, fs::File, path::PathBuf};

fn fixture(advanced: bool, denied: bool) -> KernelHost {
    // Use checked-in source files, opened by the host, not by the tool.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let environment = |path: &str| {
        let file = File::open(root.join(path)).unwrap();
        let resources = if denied {
            ValidatedResourceSet::default()
        } else {
            ValidatedResourceSet::try_new(vec![ResourceGrant::new(
                FILES,
                ReadableFiles::new(BTreeMap::from([("source".into(), file)]), 128 * 1024),
            )])
            .unwrap()
        };
        Environment::new().with_resources(read_text(), resources)
    };
    let local = environment("Cargo.toml").with_tool(Tool::new("uppercase", |text: String| {
        Ok(text.to_uppercase())
    }));
    let additional = if advanced {
        BTreeMap::from([(
            "reference".into(),
            environment("src/local.rs").with_tool(Tool::new("lowercase", |text: String| {
                Ok(text.to_lowercase())
            })),
        )])
    } else {
        BTreeMap::new()
    };
    let model = |context: &ModelContext| {
        let catalog = &context.tools;
        assert_eq!(catalog["local"].len(), 2);
        assert!(catalog["local"].contains_key(READ_TEXT));
        assert!(catalog["local"].contains_key("uppercase"));
        assert_eq!(catalog.contains_key("reference"), advanced);
        if advanced {
            assert!(catalog["reference"].contains_key("lowercase"));
            assert!(catalog["reference"].contains_key(READ_TEXT));
            assert!(!catalog["reference"].contains_key("uppercase"));
        }
        MockModel {
            tool_by_prompt: [
                ("source".into(), READ_TEXT.into()),
                ("reference".into(), READ_TEXT.into()),
                ("missing".into(), READ_TEXT.into()),
                ("upper".into(), "uppercase".into()),
                ("lower".into(), "lowercase".into()),
                ("cross-env".into(), "uppercase".into()),
                ("unknown".into(), "unknown".into()),
            ]
            .into(),
            arguments_by_prompt: [
                ("reference".into(), "source".into()),
                ("lower".into(), "ABC".into()),
            ]
            .into(),
            environment_by_prompt: [
                ("reference".into(), "reference".into()),
                ("lower".into(), "reference".into()),
                ("cross-env".into(), "reference".into()),
            ]
            .into(),
            available_tools: catalog["local"].clone(),
            ..Default::default()
        }
    };
    harness::compose(model, local, additional, Vec::new()).unwrap()
}

fn turn(host: &KernelHost, prompt: &str) -> Vec<AgentEvent> {
    let generation = host.active().unwrap();
    let mut events = Vec::new();
    run_turn(&generation, prompt.into(), 256, |event| {
        events.push(event.clone())
    })
    .unwrap();
    assert_eq!(events.last(), Some(&AgentEvent::Completed));
    events
}

#[test]
fn same_real_tool_runs_with_distinct_host_grants_in_p1_and_p2() {
    let p1 = fixture(false, false);
    let p2 = fixture(true, false);
    let local = include_str!("../Cargo.toml");
    let reference = include_str!("../src/local.rs");
    for host in [&p1, &p2] {
        assert!(turn(host, "source").contains(&AgentEvent::Delta(local.into())));
        assert!(turn(host, "upper").contains(&AgentEvent::Delta("UPPER".into())));
        assert!(
            turn(host, "unknown")
                .contains(&AgentEvent::Delta("tool failed: tool unavailable: ".into()))
        );
        assert!(turn(host, "missing").contains(&AgentEvent::Delta(
            "tool failed: file alias not granted: ".into()
        )));
    }
    assert!(turn(&p2, "reference").contains(&AgentEvent::Delta(reference.into())));
    assert!(turn(&p2, "lower").contains(&AgentEvent::Delta("abc".into())));
    assert!(
        turn(&p2, "cross-env")
            .contains(&AgentEvent::Delta("tool failed: tool unavailable: ".into()))
    );
    assert!(turn(&p1, "reference").contains(&AgentEvent::Delta(
        "tool failed: environment unavailable: ".into()
    )));
}

#[test]
fn missing_capability_is_a_domain_failure_and_budget_cleanup_allows_reuse() {
    let denied = fixture(false, true);
    assert!(turn(&denied, "source").iter().any(|event| {
        matches!(event, AgentEvent::Delta(text) if text.starts_with("tool failed: "))
    }));
    let host = fixture(false, false);
    let generation = host.active().unwrap();
    assert!(run_turn(&generation, "source".into(), 0, |_| {}).is_err());
    assert!(run_turn(&generation, "source".into(), 1, |_| {}).is_err());
    assert!(
        turn(&host, "source").contains(&AgentEvent::Delta(include_str!("../Cargo.toml").into()))
    );
}

/// No default tool is needed to build an environment or loop.
#[test]
fn empty_and_duplicate_tool_sets_have_explicit_behavior() {
    let host = harness::compose(
        |context| {
            assert!(context.tools["local"].is_empty());
            MockModel::default()
        },
        Environment::new(),
        BTreeMap::new(),
        Vec::new(),
    )
    .unwrap();
    assert!(
        turn(&host, "anything")
            .contains(&AgentEvent::Delta("tool failed: tool unavailable: ".into()))
    );

    let duplicate = Environment::new()
        .with_tool(Tool::new("echo", Ok::<String, String>))
        .with_tool(Tool::new("echo", Ok::<String, String>));
    let result = harness::compose(
        |_| MockModel::default(),
        duplicate,
        BTreeMap::new(),
        Vec::new(),
    );
    assert!(result.is_err());
}
