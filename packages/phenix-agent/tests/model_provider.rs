//! The model adapter drives a real executable, not a scripted graph process.
//! The cat fixture verifies transport; it is not an inference model.

use phenix_agent::{
    AgentEvent,
    frontend::run_turn,
    harness::{self, Environment},
    model::{Model, ModelProfile, command::CommandBackend},
};
use std::collections::BTreeMap;

fn fixture(advanced: bool, response_limit: usize) -> phenix_kernel::host::KernelHost {
    let backend = CommandBackend::new("cat", Vec::new(), 1024, response_limit);
    let additional = if advanced {
        BTreeMap::from([("reference".into(), Environment::new())])
    } else {
        BTreeMap::new()
    };
    harness::compose(
        |context| {
            Model::new(
                "command-provider",
                ModelProfile::new("command", "cat").capability("text"),
                context.clone(),
                move || backend.clone(),
            )
        },
        Environment::new(),
        additional,
        Vec::new(),
    )
    .unwrap()
}

#[test]
fn real_executable_streams_in_both_profiles() {
    for advanced in [false, true] {
        let host = fixture(advanced, 128);
        let generation = host.active().unwrap();
        let mut events = Vec::new();
        run_turn(&generation, "first\nsecond\n".into(), 128, |event| {
            events.push(event.clone())
        })
        .unwrap();
        assert_eq!(
            events,
            [
                AgentEvent::Delta("first\n".into()),
                AgentEvent::Delta("second\n".into()),
                AgentEvent::Completed,
            ]
        );
    }
}

#[test]
fn oversized_response_fails_and_next_turn_can_reuse_generation() {
    let host = fixture(false, 4);
    let generation = host.active().unwrap();
    let mut events = Vec::new();
    let result = run_turn(&generation, "too long".into(), 128, |event| {
        events.push(event.clone())
    });
    assert!(result.is_err());
    assert!(!events.contains(&AgentEvent::Completed));
    events.clear();
    run_turn(&generation, "ok".into(), 128, |event| {
        events.push(event.clone())
    })
    .unwrap();
    assert_eq!(
        events,
        [AgentEvent::Delta("ok".into()), AgentEvent::Completed]
    );
}
