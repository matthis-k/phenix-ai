//! Reference effect boundary: result packets are not evidence of resource writes.

use phenix_agent::{
    AgentEvent, CONTRACTS_PLUGIN, EVENTS, PROMPT, TOOL_CONTRACT, ToolInvocation, ToolOutcome,
    mock::{self, MockModel},
    tool_contract,
};
use phenix_graph::{
    ComponentDef, Failure as GraphFailure, InputKey, OutputKey, Process, ProcessContext,
    ProcessEmission, ResourceGrant, Step, ValidatedResourceSet, Value,
    compose::{Edit, NetworkPatch},
    runtime::RequestStatus,
};
use phenix_kernel::{Contribution, Key, host::Generation};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::sync::{Arc, Mutex};

const EFFECT: &str = "mock-effect";

#[derive(Default)]
struct State {
    value: usize,
    evidence: Vec<ToolInvocation>,
}

// This capability, not the tool's result packet, owns the simulated effect.
struct Writer(Arc<Mutex<State>>);

impl Writer {
    fn apply(&self, invocation: &ToolInvocation) {
        let mut state = self.0.lock().unwrap();
        state.evidence.push(invocation.clone());
        state.value += 1;
    }
}

#[derive(Default)]
struct EffectProcess {
    emitted: bool,
}

impl Process for EffectProcess {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, GraphFailure> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let Some(value) = context.take("input") else {
            return Ok(if context.is_eos("input") {
                Step::Finished
            } else {
                Step::Complete
            });
        };
        let invocation = value.borrow::<ToolInvocation>().unwrap().clone();
        let prompt = invocation.call().prompt.as_str();
        let outcome = if prompt == "no-effect" {
            ToolOutcome::Succeeded {
                output: "no write".into(),
            }
        } else if let Ok(writer) = context.resource::<Writer>(EFFECT) {
            writer.apply(&invocation);
            match prompt {
                "suspend" => return Ok(Step::WaitWake),
                "fail" => ToolOutcome::Failed {
                    output: "write committed".into(),
                    reason: "later failure".into(),
                },
                _ => ToolOutcome::Succeeded {
                    output: "write committed".into(),
                },
            }
        } else {
            ToolOutcome::Failed {
                output: String::new(),
                reason: "effect not granted".into(),
            }
        };
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new(
            "output",
            Value::local(invocation.complete(outcome)),
        )))
    }
}

struct EffectTool;

impl PluginInstance for EffectTool {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let contract = tool_contract();
        let definition =
            ComponentDef::primitive(contract.required().clone(), Arc::new(EffectProcess::default))
                .claim(&contract)
                .unwrap();
        Contribution::new(
            "main",
            NetworkPatch::new([Edit::Add {
                path: mock::TOOL_PATH.into(),
                definition: Arc::new(definition),
            }]),
        )
        .offer(
            "tool",
            mock::TOOL_PATH,
            Key::new(CONTRACTS_PLUGIN, TOOL_CONTRACT),
        )
        .register(registrar)
    }
}

fn drive(generation: &Generation, prompt: &str, cancel: bool) -> Vec<AgentEvent> {
    let ingress = generation
        .compiled()
        .bind_ingress(PROMPT, InputKey::<String>::new(PROMPT))
        .unwrap();
    let capture = generation
        .compiled()
        .bind_capture(EVENTS, OutputKey::<AgentEvent>::new(EVENTS))
        .unwrap();
    generation
        .with_session(|session| {
            let request = session.begin_bound_request(None).unwrap();
            session
                .inject_bound_request(request, &ingress, prompt.to_owned())
                .unwrap();
            session.close_request(request).unwrap();
            let mut events = Vec::new();
            for _ in 0..64 {
                session.run_until_idle(1);
                assert!(session.failure().is_none());
                while let Some(packet) = session
                    .take_bound_request_output(request, &capture)
                    .unwrap()
                {
                    events.push(packet.value().borrow::<AgentEvent>().unwrap().clone());
                }
                if cancel && session.request_work(request).unwrap().waiting_wake != 0 {
                    session.cancel_request(request).unwrap();
                    assert_eq!(
                        session.request_status(request).unwrap(),
                        RequestStatus::Cancelled
                    );
                    assert!(session.failure().is_none());
                    session.release_request(request).unwrap();
                    return events;
                }
                if matches!(
                    session.request_status(request),
                    Ok(RequestStatus::Settled { .. })
                ) {
                    assert!(!cancel, "tool unexpectedly completed before cancellation");
                    session.release_request(request).unwrap();
                    return events;
                }
            }
            panic!("effect scenario did not settle");
        })
        .unwrap()
}

#[test]
fn result_outcome_and_authoritative_effect_evidence_are_independent() {
    let state = Arc::new(Mutex::new(State::default()));
    let mut options = mock::options();
    options.execution.resources.insert(
        mock::TOOL_PATH.into(),
        ValidatedResourceSet::try_new(vec![ResourceGrant::new(EFFECT, Writer(state.clone()))])
            .unwrap(),
    );
    let host = mock::host_with_options(MockModel::default(), EffectTool, options).unwrap();
    let generation = host.active().unwrap();

    let events = drive(&generation, "no-effect", false);
    assert!(events.contains(&AgentEvent::Delta("no write".into())));
    assert_eq!(state.lock().unwrap().value, 0);

    let events = drive(&generation, "fail", false);
    assert!(events.contains(&AgentEvent::Delta(
        "tool failed: later failure: write committed".into()
    )));
    assert_eq!(state.lock().unwrap().value, 1);

    let events = drive(&generation, "suspend", true);
    assert!(!events.contains(&AgentEvent::Completed));
    assert_eq!(state.lock().unwrap().value, 2);

    for _ in 0..2 {
        let events = drive(&generation, "write", false);
        assert!(events.contains(&AgentEvent::Completed));
    }
    {
        let state = state.lock().unwrap();
        assert_eq!(state.value, 4);
        assert_eq!(state.evidence.len(), state.value);
        assert_eq!(state.evidence[0].call().prompt, "fail");
        assert_eq!(state.evidence[1].call().prompt, "suspend");
        assert_eq!(state.evidence[2].call(), state.evidence[3].call());
        assert_ne!(state.evidence[2], state.evidence[3]);
    }

    let denied =
        mock::host_with_options(MockModel::default(), EffectTool, mock::options()).unwrap();
    let events = drive(&denied.active().unwrap(), "write", false);
    assert!(events.contains(&AgentEvent::Delta(
        "tool failed: effect not granted: ".into()
    )));
    assert_eq!(state.lock().unwrap().value, 4);
}
