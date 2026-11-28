//! Real local effects through the ordinary P1 and P2 tool contract. The model
//! is deterministic, but file handles and bytes are real.

use phenix_agent::{
    AgentEvent,
    frontend::run_turn,
    harness::{self, Environment},
    local::{FILES, READ_TEXT, ReadableFiles, read_text},
    mock::{MODEL_PATH, MockModel},
};
use phenix_graph::{ResourceGrant, ValidatedResourceSet};
use phenix_kernel::host::KernelHost;
use std::{collections::BTreeMap, fs::File, path::PathBuf};

fn fixture(advanced: bool, denied: bool) -> KernelHost {
    // Use checked-in source files, opened by the host, not by the tool.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let environment = |path: &str, placement: &str| {
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
        Environment {
            tool: read_text().placement(placement),
            resources,
        }
    };
    let local = environment("Cargo.toml", "local-read");
    let additional = if advanced {
        BTreeMap::from([(
            "reference".into(),
            environment("src/local.rs", "reference-read"),
        )])
    } else {
        BTreeMap::new()
    };
    let model = MockModel {
        tool_by_prompt: [
            ("source".into(), READ_TEXT.into()),
            ("reference".into(), READ_TEXT.into()),
            ("missing".into(), READ_TEXT.into()),
        ]
        .into(),
        arguments_by_prompt: [("reference".into(), "source".into())].into(),
        environment_by_prompt: [("reference".into(), "reference".into())].into(),
        ..Default::default()
    };
    harness::compose(model, MODEL_PATH, local, additional).unwrap()
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
        assert!(turn(host, "missing").contains(&AgentEvent::Delta(
            "tool failed: file alias not granted: ".into()
        )));
    }
    assert!(turn(&p2, "reference").contains(&AgentEvent::Delta(reference.into())));
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
    assert!(turn(&host, "source").contains(&AgentEvent::Delta(
        include_str!("../Cargo.toml").into()
    )));
}
