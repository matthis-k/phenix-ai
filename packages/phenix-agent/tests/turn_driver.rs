//! Exercise the public request driver, including owned work that outlives
//! cancellation. These are real scheduler jobs, not fake lifecycle callbacks.

use phenix_agent::{
    AgentEvent,
    frontend::{Turn, TurnPoll, run_turn},
    harness::{self, Composition, Environment},
    mock::MockModel,
};
use std::collections::BTreeMap;

fn fixture() -> phenix_kernel::host::KernelHost {
    harness::compose(
        |_| MockModel {
            use_tool: false,
            ..Default::default()
        },
        Environment::new(),
        BTreeMap::new(),
        Composition::default(),
    )
    .unwrap()
}

#[test]
fn cancel_owned_work_and_reuse_generation() {
    let host = fixture();
    let generation = host.active().unwrap();
    let mut turn = Turn::begin(&generation, "cancelled".into()).unwrap();
    let TurnPoll::Job(job) = turn.poll().unwrap() else {
        panic!("prompt adapter job");
    };
    // The returned job no longer owns the session lease.
    generation.with_session(|_| ()).unwrap();
    let ticket = job.ticket();
    let completion = std::thread::spawn(move || job.run()).join().unwrap();
    let (error, refused) = generation
        .with_session(|_| turn.complete_job(completion))
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("reentrant"));
    assert_eq!(refused.ticket(), ticket);
    turn.cancel().unwrap();
    let (error, refused) = turn.complete_job(*refused).unwrap_err();
    assert!(!error.to_string().is_empty());
    assert_eq!(refused.ticket(), ticket);
    drop(refused);
    let mut events = Vec::new();
    run_turn(&generation, "next".into(), 128, |event| {
        events.push(event.clone())
    })
    .unwrap();
    assert_eq!(
        events,
        [
            AgentEvent::Delta("mock: ".into()),
            AgentEvent::Delta("next".into()),
            AgentEvent::Completed,
        ]
    );
}

#[test]
fn manual_handoff_and_synchronous_convenience_have_same_events() {
    let host = fixture();
    let generation = host.active().unwrap();
    let mut turn = Turn::begin(&generation, "same".into()).unwrap();
    let mut manual = Vec::new();
    let mut done = false;
    for _ in 0..128 {
        match turn.poll().unwrap() {
            TurnPoll::Job(job) => {
                generation.with_session(|_| ()).unwrap();
                turn.complete_job(job.run()).unwrap();
            }
            TurnPoll::Events(events) => manual.extend(events),
            TurnPoll::Complete => {
                done = true;
                break;
            }
            TurnPoll::Waiting => panic!("immediate fixture must not wait"),
        }
    }
    assert!(done);
    let mut convenience = Vec::new();
    run_turn(&generation, "same".into(), 128, |event| {
        convenience.push(event.clone())
    })
    .unwrap();
    assert_eq!(manual, convenience);
}
