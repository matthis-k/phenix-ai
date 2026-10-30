//! Native acceptance for the bounded graph-v2 Basic turn.
//! Usage: phenix-graph-v2-turn COMPOSITION (with a fresh SQLite database).
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
    let deny_tool = std::env::args().nth(2).as_deref() == Some("deny-tool");
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
    let mut run = kernel
        .begin_action_scoped(
            "phenix.basic.submit",
            &PromptSubmitted {
                session: "s1".into(),
                text: "tool:graph-proof".into(),
                limits: Limits::default(),
            }
            .to_value(),
            scope,
        )
        .unwrap();
    let mut completions = 0;
    let pumped = run.pump(|update| {
        if matches!(update.event, GraphEvent::Finished(_)) {
            completions += 1;
        }
    });
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
    assert_eq!(run.completion(), Some(GraphCompletion::Success));
    assert_eq!(completions, 1);
    assert_eq!(
        run.steps()
            .iter()
            .map(|step| step.node.as_str())
            .collect::<Vec<_>>(),
        [
            "phenix.basic.agent/prepare",
            "phenix.basic.agent/model",
            "phenix.basic.agent/tool",
            "phenix.basic.agent/continuation",
            "phenix.basic.agent/finish"
        ]
    );
    let Outcome::Success(value) = &run.steps().last().unwrap().outcome else {
        panic!("missing final answer")
    };
    let result = RunResult::from_value(value.clone()).unwrap();
    assert_eq!(result.answer, "mock:call-1:GRAPH-PROOF");
    assert_eq!((result.model_turns, result.tool_calls), (2, 1));
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
    assert_eq!(
        page.events
            .iter()
            .map(|event| &event.kind)
            .collect::<Vec<_>>(),
        [
            &EventKind::PromptAccepted,
            &EventKind::ModelStarted,
            &EventKind::ToolStarted,
            &EventKind::ToolFinished,
            &EventKind::ModelStarted,
            &EventKind::TextDelta,
            &EventKind::Answer
        ]
    );
    assert_eq!(
        run.pump(|_| panic!("duplicate completion")).unwrap(),
        GraphStatus::Finished
    );
    drop(run);
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
    kernel.shutdown().unwrap();
    println!("graph-v2 turn: five ordered stages, two model turns, one tool, journal ordering, answer and exactly-once completion passed");
}
