//! Exact replacement diagnostics and an explicitly adapted versioned agent provider.
use phenix_graph::{
    CompiledNetwork, ComponentDef, ContractValue, Failure, GraphOrigin, Interface, PortDef, Schema,
    Value,
    compose::{ComposeError, Composer, Edit, Footprint, NetworkPatch, Registration},
    contract::Contract,
    interface::{InterfaceError, InterfacePhase, InterfaceReason},
    network::{NetworkBuilder, NetworkDef, NetworkError},
    process::{Process, ProcessContext, ProcessEmission, Step},
    runtime::{
        NetworkSession, RequestError, RequestId, RequestQueuePolicy, RequestStatus, SessionOptions,
    },
};
use std::{
    error::Error,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const PROVIDER: &str = "agent/loop/provider";
fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

#[derive(Debug, PartialEq, Eq)]
struct Legacy(String);
impl ContractValue for Legacy {
    fn schema() -> Schema {
        Schema {
            name: "agent.message".into(),
            revision: 1,
            structure: "text:utf8".into(),
            codec: "utf8-v1".into(),
        }
    }
    fn encode(&self) -> Vec<u8> {
        self.0.as_bytes().to_vec()
    }
    fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        String::from_utf8(bytes.to_vec())
            .map(Self)
            .map_err(|e| Failure::external("legacy text", e))
    }
}

// V2 changes actual framing and semantics, not just the schema's revision label.
#[derive(Debug, PartialEq, Eq)]
struct Frame {
    kind: u8,
    text: String,
}
impl ContractValue for Frame {
    fn schema() -> Schema {
        Schema {
            name: "provider.dialogue".into(),
            revision: 2,
            structure: "{kind:u8,text:utf8}".into(),
            codec: "tagged-v2".into(),
        }
    }
    fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![2, self.kind];
        bytes.extend_from_slice(self.text.as_bytes());
        bytes
    }
    fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        if bytes.len() < 2 || bytes[0] != 2 {
            return Err(Failure::application_message("invalid V2 frame"));
        }
        Ok(Self {
            kind: bytes[1],
            text: String::from_utf8(bytes[2..].to_vec())
                .map_err(|e| Failure::external("V2 text", e))?,
        })
    }
}

fn boundary() -> Arc<Interface> {
    Arc::new(
        Interface::new(
            [PortDef::wire::<Legacy>("context").unwrap()],
            [
                PortDef::wire::<Legacy>("tool").unwrap(),
                PortDef::wire::<Legacy>("final").unwrap(),
            ],
        )
        .unwrap(),
    )
}
fn promise() -> Contract {
    Contract::new("agent-provider@1", boundary()).unwrap()
}

#[derive(Clone, Default)]
struct Effects {
    decisions: Arc<AtomicUsize>,
    tools: Arc<AtomicUsize>,
    usage: Arc<AtomicUsize>,
}
impl Effects {
    fn counts(&self) -> (usize, usize, usize) {
        (
            self.decisions.load(Ordering::SeqCst),
            self.tools.load(Ordering::SeqCst),
            self.usage.load(Ordering::SeqCst),
        )
    }
}

struct Provider {
    v2: bool,
    effects: Effects,
    pending: Option<ProcessEmission>,
    emitted: bool,
}
impl Process for Provider {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, Failure> {
        if let Some(branch) = self.pending.take() {
            return Ok(Step::Emit(branch));
        }
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let Some(value) = cx.take(if self.v2 { "dialogue" } else { "context" }) else {
            return Ok(Step::Finished);
        };
        let text = if self.v2 {
            let frame = value.decoded::<Frame>()?;
            assert_eq!(frame.kind, 0);
            frame.text.clone()
        } else {
            value.decoded::<Legacy>()?.0.clone()
        };
        self.effects.decisions.fetch_add(1, Ordering::SeqCst);
        self.emitted = true;
        let final_branch = text.starts_with("observation:") || text.starts_with("stop:");
        let version = if self.v2 { "v2" } else { "v1" };
        let output = if final_branch {
            format!("answer:{version}:{text}")
        } else {
            format!("lookup:{version}:{text}")
        };
        if self.v2 {
            let kind = if text.contains("bad-")
                || (text.starts_with("observation:") && text.contains("fail-after-feedback"))
            {
                255
            } else if final_branch {
                2
            } else {
                1
            };
            self.pending = Some(ProcessEmission::new(
                if final_branch {
                    "completion"
                } else {
                    "invocation"
                },
                Value::of(Frame { kind, text: output }),
            ));
            Ok(Step::Emit(ProcessEmission::new("usage", Value::of(1_i64))))
        } else {
            Ok(Step::Emit(ProcessEmission::new(
                if final_branch { "final" } else { "tool" },
                Value::of(Legacy(output)),
            )))
        }
    }
}
fn provider(v2: bool, effects: &Effects) -> Arc<ComponentDef> {
    let interface = if v2 {
        Arc::new(
            Interface::new(
                [PortDef::wire::<Frame>("dialogue").unwrap()],
                [
                    PortDef::wire::<Frame>("invocation").unwrap(),
                    PortDef::wire::<Frame>("completion").unwrap(),
                    PortDef::wire::<i64>("usage").unwrap(),
                ],
            )
            .unwrap(),
        )
    } else {
        boundary()
    };
    let effects = effects.clone();
    let definition = ComponentDef::primitive(
        interface,
        Arc::new(move || Provider {
            v2,
            effects: effects.clone(),
            pending: None,
            emitted: false,
        }),
    );
    Arc::new(if v2 {
        definition
    } else {
        definition.claim(&promise()).unwrap()
    })
}

struct Usage(Arc<AtomicUsize>);
impl Process for Usage {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, Failure> {
        let Some(value) = cx.take("input") else {
            return Ok(Step::Finished);
        };
        assert_eq!(*value.decoded::<i64>()?, 1);
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Step::Complete)
    }
}

fn facade(effects: &Effects, claim: bool, wider: bool) -> Arc<ComponentDef> {
    let mut b = NetworkBuilder::new();
    let input = b
        .add(
            "input",
            Arc::new(
                ComponentDef::adapt_wire::<Legacy, Frame, _>(|old| {
                    if old.0 == "reject-input" {
                        return Err(Failure::external(
                            "input conversion refused",
                            std::io::Error::other("unsupported legacy context"),
                        ));
                    }
                    Ok(Frame {
                        kind: 0,
                        text: old.0.clone(),
                    })
                })
                .unwrap(),
            ),
        )
        .unwrap();
    let raw = b.add("v2", provider(true, effects)).unwrap();
    let tool = b
        .add(
            "tool",
            Arc::new(
                ComponentDef::adapt_wire::<Frame, Legacy, _>(|frame| {
                    if frame.kind != 1 {
                        return Err(Failure::application_message("tool conversion refused"));
                    }
                    Ok(Legacy(frame.text.clone()))
                })
                .unwrap(),
            ),
        )
        .unwrap();
    let final_output = b
        .add(
            "final",
            Arc::new(
                ComponentDef::adapt_wire::<Frame, Legacy, _>(|frame| {
                    if frame.kind != 2 {
                        return Err(Failure::application_message("final conversion refused"));
                    }
                    Ok(Legacy(frame.text.clone()))
                })
                .unwrap(),
            ),
        )
        .unwrap();
    let usage = effects.usage.clone();
    let sink = b
        .add(
            "usage",
            Arc::new(ComponentDef::primitive(
                Arc::new(Interface::new([PortDef::wire::<i64>("input").unwrap()], []).unwrap()),
                Arc::new(move || Usage(usage.clone())),
            )),
        )
        .unwrap();
    b.connect(
        input.wire_output::<Frame>("output").unwrap(),
        raw.wire_input::<Frame>("dialogue").unwrap(),
        one(),
    )
    .unwrap();
    b.connect(
        raw.wire_output::<Frame>("invocation").unwrap(),
        tool.wire_input::<Frame>("input").unwrap(),
        one(),
    )
    .unwrap();
    b.connect(
        raw.wire_output::<Frame>("completion").unwrap(),
        final_output.wire_input::<Frame>("input").unwrap(),
        one(),
    )
    .unwrap();
    b.connect(
        raw.wire_output::<i64>("usage").unwrap(),
        sink.wire_input::<i64>("input").unwrap(),
        one(),
    )
    .unwrap();
    b.expose_input(
        "context",
        input.wire_input::<Legacy>("input").unwrap(),
        one(),
    )
    .unwrap();
    b.expose_output("tool", tool.wire_output::<Legacy>("output").unwrap())
        .unwrap();
    b.expose_output(
        "final",
        final_output.wire_output::<Legacy>("output").unwrap(),
    )
    .unwrap();
    if wider {
        b.expose_output("extra", raw.wire_output::<i64>("usage").unwrap())
            .unwrap();
    }
    if claim {
        b.claim(&promise()).unwrap();
    }
    Arc::new(ComponentDef::composite(b.build().unwrap()))
}

fn agent(definition: Arc<ComponentDef>, effects: &Effects) -> NetworkDef {
    let mut b = NetworkBuilder::new();
    let context = b
        .add(
            "context",
            Arc::new(
                ComponentDef::adapt_wire::<Legacy, Legacy, _>(|v| Ok(Legacy(v.0.clone()))).unwrap(),
            ),
        )
        .unwrap();
    let model = b.add("provider", definition).unwrap();
    let tools = effects.tools.clone();
    let tool = b
        .add(
            "tool",
            Arc::new(
                ComponentDef::adapt_wire::<Legacy, Legacy, _>(move |v| {
                    tools.fetch_add(1, Ordering::SeqCst);
                    Ok(Legacy(format!("observation:{}", v.0)))
                })
                .unwrap(),
            ),
        )
        .unwrap();
    b.connect(
        context.wire_output::<Legacy>("output").unwrap(),
        model.wire_input::<Legacy>("context").unwrap(),
        one(),
    )
    .unwrap();
    b.connect(
        model.wire_output::<Legacy>("tool").unwrap(),
        tool.wire_input::<Legacy>("input").unwrap(),
        one(),
    )
    .unwrap();
    b.connect(
        tool.wire_output::<Legacy>("output").unwrap(),
        context.wire_input::<Legacy>("input").unwrap(),
        one(),
    )
    .unwrap();
    b.expose_input(
        "prompt",
        context.wire_input::<Legacy>("input").unwrap(),
        one(),
    )
    .unwrap();
    b.expose_output("reply", model.wire_output::<Legacy>("final").unwrap())
        .unwrap();
    b.allow_replace(&model).unwrap();
    b.require_contract("provider", &promise()).unwrap();
    let mut network = b.build().unwrap();
    for name in ["loop", "agent"] {
        let mut parent = NetworkBuilder::new();
        let child = parent.module(name, network).unwrap();
        parent
            .expose_input(
                "prompt",
                child.wire_input::<Legacy>("prompt").unwrap(),
                one(),
            )
            .unwrap();
        parent
            .expose_output("reply", child.wire_output::<Legacy>("reply").unwrap())
            .unwrap();
        parent.allow_replace(&child).unwrap();
        network = parent.build().unwrap();
    }
    network
}
fn replacement(path: &str, definition: Arc<ComponentDef>) -> Registration {
    Registration::new(
        "upgrade",
        [],
        0,
        GraphOrigin::default().child(),
        [Footprint::Component(path.into())],
        NetworkPatch::new([Edit::Replace {
            path: path.into(),
            definition,
        }]),
    )
}
fn start(compiled: &CompiledNetwork, queue: RequestQueuePolicy) -> NetworkSession {
    let s = compiled.start_with_options(SessionOptions {
        request_queue_policy: queue,
        max_requests: one(),
        capture_capacity: one(),
        ..Default::default()
    });
    assert!(s.failure().is_none());
    s
}
fn submit(s: &mut NetworkSession, text: &str) -> RequestId {
    let id = s.begin_request(Some("reply")).unwrap();
    s.inject_request(id, "prompt", Value::of(Legacy(text.into())))
        .unwrap();
    s.close_request(id).unwrap();
    id
}
fn drive(s: &mut NetworkSession, allowed: &[RequestId]) -> Vec<(String, String)> {
    let mut trace = Vec::new();
    let mut steps = 0;
    while let Some((_, job)) = s.next_job() {
        steps += 1;
        assert!(steps <= 128, "agent exceeded finite step budget");
        let metadata = job.metadata().unwrap();
        let request = metadata.request.unwrap();
        assert!(allowed.contains(&request));
        assert_eq!(
            (metadata.session, metadata.generation),
            (request.session(), request.generation())
        );
        s.complete_job(job.run()).unwrap();
        let completed = s.last_step().unwrap();
        assert_eq!(completed.request, Some(request));
        if let Some(output) = &completed.output {
            trace.push((completed.path.clone(), output.clone()));
        }
    }
    trace
}
fn take_reply(s: &mut NetworkSession, id: RequestId) -> String {
    let packet = s.take_request_output(id, "reply").unwrap().unwrap();
    s.validate_capture(id, "reply", &packet).unwrap();
    assert_eq!(packet.metadata().unwrap().request(), Some(id));
    assert_eq!(
        packet.value().identity(),
        PortDef::wire::<Legacy>("final").unwrap().value
    );
    packet.value().decoded::<Legacy>().unwrap().0.clone()
}
fn receive(s: &mut NetworkSession, id: RequestId) -> String {
    let text = take_reply(s, id);
    assert!(s.take_request_output(id, "reply").unwrap().is_none());
    assert_eq!(
        s.request_status(id),
        Ok(RequestStatus::Settled { captures: 1 })
    );
    s.release_request(id).unwrap();
    assert_eq!(s.request_status(id), Err(RequestError::Stale));
    text
}
fn turn(s: &mut NetworkSession, prompt: &str, version: &str) -> Vec<(String, String)> {
    let id = submit(s, prompt);
    let trace = drive(s, &[id]);
    assert!(s.failure().is_none(), "{:?}", s.failure());
    let expected = if prompt.starts_with("stop:") {
        format!("answer:{version}:{prompt}")
    } else {
        format!("answer:{version}:observation:lookup:{version}:{prompt}")
    };
    assert_eq!(receive(s, id), expected);
    assert!(drive(s, &[]).is_empty());
    trace
}

#[test]
fn versioned_facade_replaces_real_feedback_provider_and_restores_without_mutating_sessions() {
    assert_ne!(
        Legacy("hello".into()).encode(),
        Frame {
            kind: 0,
            text: "hello".into()
        }
        .encode()
    );
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let effects = Effects::default();
        let mut composer = Composer::new(agent(provider(false, &effects), &effects)).unwrap();
        let base = composer.current().compiled().clone();
        let mut old = start(&base, queue);
        turn(&mut old, "before", "v1");
        let adapted = facade(&effects, true, false);
        assert_eq!(adapted.interface(), &boundary());
        let registration = replacement(PROVIDER, adapted);
        let origin = registration.origin().clone();
        composer.register([registration]).unwrap();
        let upgraded = composer.current().compiled().clone();
        upgraded.check_implementation(PROVIDER, &promise()).unwrap();
        assert_eq!(
            upgraded.inspect(PROVIDER).unwrap().declared_origin,
            Some(origin)
        );
        for path in [
            "agent",
            "agent/loop",
            "agent/loop/context",
            "agent/loop/tool",
        ] {
            assert_eq!(
                upgraded.inspect(path).unwrap().declared_origin,
                base.inspect(path).unwrap().declared_origin
            );
        }
        assert!(
            upgraded
                .connections()
                .iter()
                .all(|edge| edge.capacity() == one())
        );
        assert_eq!(upgraded.connections().len(), 7);
        // Seven edge slots + one root ingress + one capture. Pending emissions
        // and payload byte sizes are separate; this is not a total memory bound.
        assert_eq!(
            upgraded.connections().len()
                + upgraded.ingress_capacities()["prompt"].get()
                + upgraded.outputs().len(),
            9
        );
        let mut current = start(&upgraded, queue);
        let trace = turn(&mut current, "question", "v2");
        let emissions: Vec<_> = trace
            .iter()
            .map(|(path, output)| (path.as_str(), output.as_str()))
            .collect();
        assert_eq!(
            emissions,
            [
                ("agent/loop/context", "output"),
                ("agent/loop/provider/input", "output"),
                ("agent/loop/provider/v2", "usage"),
                ("agent/loop/provider/v2", "invocation"),
                ("agent/loop/provider/tool", "output"),
                ("agent/loop/tool", "output"),
                ("agent/loop/context", "output"),
                ("agent/loop/provider/input", "output"),
                ("agent/loop/provider/v2", "usage"),
                ("agent/loop/provider/v2", "completion"),
                ("agent/loop/provider/final", "output"),
            ]
        );
        assert_eq!(effects.counts(), (4, 2, 2));
        turn(&mut old, "retained-v1", "v1");
        let before_stop = effects.counts();
        let stop = turn(&mut current, "stop:ready", "v2");
        assert!(!stop.iter().any(|(path, _)| path == "agent/loop/tool"));
        assert_eq!(
            effects.counts(),
            (before_stop.0 + 1, before_stop.1, before_stop.2 + 1)
        );

        // Failed updates preserve the published generation, not merely its shape.
        let bound = upgraded.capture("reply").unwrap();
        let active = composer.current().active().clone();
        let error = composer
            .register([Registration::new(
                "broken",
                ["upgrade".into()],
                0,
                GraphOrigin::default().child(),
                [Footprint::Component(PROVIDER.into())],
                NetworkPatch::new([Edit::Replace {
                    path: PROVIDER.into(),
                    definition: provider(true, &effects),
                }]),
            )])
            .unwrap_err();
        assert!(matches!(
            error,
            ComposeError::Network(NetworkError::Interface(_))
        ));
        bound.validate(composer.current().compiled()).unwrap();
        assert_eq!(composer.current().active(), &active);
        turn(&mut current, "after-failure", "v2");
        assert_eq!(
            composer.revoke("upgrade").unwrap(),
            ["upgrade".to_owned()].into()
        );
        assert!(composer.current().active().is_empty());
        assert_eq!(
            composer.current().compiled().connections(),
            base.connections()
        );
        assert_eq!(
            composer
                .current()
                .compiled()
                .inspect(PROVIDER)
                .unwrap()
                .definition_key,
            base.inspect(PROVIDER).unwrap().definition_key
        );
        let mut restored = start(composer.current().compiled(), queue);
        turn(&mut restored, "restored", "v1");
        turn(&mut old, "still-v1", "v1");
        turn(&mut current, "still-v2", "v2");
    }
}

#[test]
fn bounded_capture_backpressure_preserves_requests_and_does_not_repeat_effects() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let effects = Effects::default();
        let mut composer = Composer::new(agent(provider(false, &effects), &effects)).unwrap();
        composer
            .register([replacement(PROVIDER, facade(&effects, true, false))])
            .unwrap();
        let mut s = start(composer.current().compiled(), queue);
        // Fill one request's capture lane with two conversational turns. Separate
        // requests have separate capture capacity under BoundedLanes.
        let id = s.begin_request(None).unwrap();
        s.inject_request(id, "prompt", Value::of(Legacy("first".into())))
            .unwrap();
        drive(&mut s, &[id]);
        s.inject_request(id, "prompt", Value::of(Legacy("second".into())))
            .unwrap();
        s.close_request(id).unwrap();
        drive(&mut s, &[id]);
        assert_eq!(s.begin_request(None), Err(RequestError::Full));
        assert_eq!(s.request_work(id).unwrap().capture_backpressure, 1);
        assert!(!s.request_blocked_queues(id).unwrap().is_empty());
        assert_eq!(effects.counts(), (4, 2, 4));
        for _ in 0..3 {
            assert!(drive(&mut s, &[id]).is_empty());
            assert_eq!(effects.counts(), (4, 2, 4));
        }
        assert_eq!(
            take_reply(&mut s, id),
            "answer:v2:observation:lookup:v2:first"
        );
        drive(&mut s, &[id]);
        assert_eq!(
            take_reply(&mut s, id),
            "answer:v2:observation:lookup:v2:second"
        );
        assert!(s.take_request_output(id, "reply").unwrap().is_none());
        assert_eq!(
            s.request_status(id),
            Ok(RequestStatus::Settled { captures: 2 })
        );
        s.release_request(id).unwrap();
        assert!(drive(&mut s, &[]).is_empty());
        assert!(s.failure().is_none());
        assert_eq!(effects.counts(), (4, 2, 4));
    }
}

#[test]
fn handed_old_generation_tool_job_finishes_original_request_after_rejection_and_revoke() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let effects = Effects::default();
        let mut composer = Composer::new(agent(provider(false, &effects), &effects)).unwrap();
        composer
            .register([replacement(PROVIDER, facade(&effects, true, false))])
            .unwrap();
        let original = composer.current().compiled().capture("reply").unwrap();
        let mut s = start(composer.current().compiled(), queue);
        let id = submit(&mut s, "paused-tool");
        let mut steps = 0;
        let job = loop {
            steps += 1;
            assert!(steps <= 128);
            let (_, job) = s.next_job().expect("tool job must be dispatched");
            let metadata = job.metadata().unwrap();
            assert_eq!(metadata.request, Some(id));
            assert_eq!(
                (metadata.session, metadata.generation),
                (id.session(), id.generation())
            );
            if metadata.path == "agent/loop/tool" {
                break job;
            }
            s.complete_job(job.run()).unwrap();
            assert_eq!(s.last_step().unwrap().request, Some(id));
        };
        assert_eq!(effects.counts(), (1, 0, 1));
        let error = composer
            .register([Registration::new(
                "broken",
                ["upgrade".into()],
                0,
                GraphOrigin::default().child(),
                [Footprint::Component(PROVIDER.into())],
                NetworkPatch::new([Edit::Replace {
                    path: PROVIDER.into(),
                    definition: provider(true, &effects),
                }]),
            )])
            .unwrap_err();
        assert!(matches!(
            error,
            ComposeError::Network(NetworkError::Interface(_))
        ));
        original.validate(composer.current().compiled()).unwrap();
        assert_eq!(composer.current().active(), &["upgrade".to_owned()].into());
        composer.revoke("upgrade").unwrap();
        assert!(composer.current().active().is_empty());
        assert!(original.validate(composer.current().compiled()).is_err());
        drop(original);
        drop(composer);
        assert_eq!(effects.counts(), (1, 0, 1));

        s.complete_job(job.run()).unwrap();
        assert_eq!(s.last_step().unwrap().request, Some(id));
        drive(&mut s, &[id]);
        assert_eq!(
            receive(&mut s, id),
            "answer:v2:observation:lookup:v2:paused-tool"
        );
        assert!(s.failure().is_none());
        for _ in 0..3 {
            assert_eq!(s.run_until_idle(4).steps, 0);
            assert_eq!(effects.counts(), (2, 1, 2));
        }
    }
}

struct DropLease(Arc<AtomicUsize>);
impl Drop for DropLease {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn detached_facade_capture_and_conversion_failure_retain_leases_after_revoke() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for prompt in ["question", "reject-input", "fail-after-feedback"] {
            let effects = Effects::default();
            let mut composer = Composer::new(agent(provider(false, &effects), &effects)).unwrap();
            composer
                .register([replacement(PROVIDER, facade(&effects, true, false))])
                .unwrap();
            let code_drops = Arc::new(AtomicUsize::new(0));
            let compiled = composer
                .current()
                .compiled()
                .with_lease(Arc::new(DropLease(code_drops.clone())));
            let mut s = start(&compiled, queue);
            let id = s.begin_request(Some("reply")).unwrap();
            s.inject_request(id, "prompt", Value::of(Legacy(prompt.into())))
                .unwrap();
            s.close_request(id).unwrap();
            drive(&mut s, &[id]);
            composer.revoke("upgrade").unwrap();
            drop(compiled);
            drop(composer);

            let failure = s.failure().cloned();
            assert_eq!(failure.is_some(), prompt != "question");
            let packet = if failure.is_none() {
                let packet = s.take_request_output(id, "reply").unwrap().unwrap();
                s.validate_capture(id, "reply", &packet).unwrap();
                assert_eq!(packet.metadata().unwrap().request(), Some(id));
                s.release_request(id).unwrap();
                Some(packet)
            } else {
                None
            };
            drop(s);
            // Only the detached capture or failure owns this host resource lease;
            // the composer, compiled graph and session have all been dropped.
            assert_eq!(code_drops.load(Ordering::SeqCst), 0);
            if let Some(packet) = packet {
                assert_eq!(prompt, "question");
                assert_eq!(
                    packet.value().decoded::<Legacy>().unwrap().0,
                    "answer:v2:observation:lookup:v2:question"
                );
                assert_eq!(effects.counts(), (2, 1, 2));
                drop(packet);
            } else {
                let failure = failure.unwrap();
                let expected = if prompt == "reject-input" {
                    (0, 0, 0)
                } else {
                    (2, 1, 2)
                };
                assert_eq!(effects.counts(), expected);
                assert!(failure.summary().contains(if prompt == "reject-input" {
                    "input conversion refused"
                } else {
                    "final conversion refused"
                }));
                drop(failure);
            }
            assert_eq!(code_drops.load(Ordering::SeqCst), 1);
        }
    }
}

#[test]
fn conversion_failures_publish_no_forged_reply_or_tool_effect() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for (prompt, message, expected) in [
            ("reject-input", "input conversion refused", (0, 0, 0)),
            ("bad-tool", "tool conversion refused", (1, 0, 1)),
            ("stop:bad-final", "final conversion refused", (1, 0, 1)),
            ("fail-after-feedback", "final conversion refused", (2, 1, 2)),
        ] {
            let effects = Effects::default();
            let mut composer = Composer::new(agent(provider(false, &effects), &effects)).unwrap();
            composer
                .register([replacement(PROVIDER, facade(&effects, true, false))])
                .unwrap();
            let mut s = start(composer.current().compiled(), queue);
            let id = submit(&mut s, prompt);
            let trace = drive(&mut s, &[id]);
            if prompt == "fail-after-feedback" {
                assert!(trace.contains(&("agent/loop/tool".into(), "output".into())));
                assert_eq!(
                    trace
                        .iter()
                        .filter(|(path, _)| path == "agent/loop/provider/input")
                        .count(),
                    2
                );
                assert_eq!(
                    trace.last(),
                    Some(&("agent/loop/provider/v2".into(), "completion".into()))
                );
            }
            assert!(s.failure().unwrap().summary().contains(message));
            if prompt == "reject-input" {
                let mut source: &(dyn Error + 'static) = s.failure().unwrap();
                while let Some(cause) = source.source() {
                    source = cause;
                }
                assert_eq!(
                    source.downcast_ref::<std::io::Error>().unwrap().to_string(),
                    "unsupported legacy context"
                );
            }
            match queue {
                RequestQueuePolicy::SharedFifo => assert!(matches!(
                    s.take_request_output(id, "reply"),
                    Err(RequestError::Stale)
                )),
                RequestQueuePolicy::BoundedLanes => {
                    assert!(s.take_request_output(id, "reply").unwrap().is_none())
                }
            }
            assert_eq!(s.request_status(id), Ok(RequestStatus::Failed));
            assert!(s.take_output("reply").unwrap().is_none());
            for _ in 0..3 {
                assert!(drive(&mut s, &[id]).is_empty());
                assert_eq!(effects.counts(), expected);
            }
            // Failure is session-local. Effects before a later conversion error
            // are not rolled back; the already-consumed usage event is explicit.
            let mut healthy = start(composer.current().compiled(), queue);
            turn(&mut healthy, "healthy", "v2");
        }
    }
}

struct Idle;
impl Process for Idle {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
        Ok(Step::Finished)
    }
}
fn inert(interface: Interface) -> Arc<ComponentDef> {
    Arc::new(ComponentDef::primitive(
        Arc::new(interface),
        Arc::new(|| Idle),
    ))
}

#[test]
fn canonical_replacement_diagnostics_retain_both_directions_full_paths_phases_and_sources() {
    for output in [false, true] {
        let port = PortDef::wire::<Legacy>("value").unwrap();
        let expected = if output {
            Interface::new([], [port.clone()])
        } else {
            Interface::new([port.clone()], [])
        }
        .unwrap();
        let wrong = PortDef::wire::<Frame>("value").unwrap();
        let extra = PortDef::wire::<Legacy>("extra").unwrap();
        for (actual, reason) in [
            (Interface::default(), InterfaceReason::MissingPort),
            (
                if output {
                    Interface::new([port.clone()], [])
                } else {
                    Interface::new([], [port.clone()])
                }
                .unwrap(),
                InterfaceReason::WrongDirection,
            ),
            (
                if output {
                    Interface::new([], [wrong])
                } else {
                    Interface::new([wrong], [])
                }
                .unwrap(),
                InterfaceReason::IncompatibleType,
            ),
            (
                if output {
                    Interface::new([], [port.clone(), extra])
                } else {
                    Interface::new([port.clone(), extra], [])
                }
                .unwrap(),
                InterfaceReason::ExtraPort,
            ),
        ] {
            let canonical = expected.check_shape(&actual).unwrap_err();
            assert_eq!(*canonical.reason, reason);
            let old = inert(expected.clone());
            let new = inert(actual);
            let mut builder = NetworkBuilder::new();
            let current = builder.add("provider", old.clone()).unwrap();
            let error = builder.replace(&current, new.clone()).unwrap_err();
            assert_diagnostic(&error, &canonical, "provider");
            builder.component("provider").unwrap();
            // A failed replacement must not invalidate the original capability.
            builder.allow_replace(&current).unwrap();
            for depth in 0..=2 {
                let mut b = NetworkBuilder::new();
                b.add("provider", old.clone()).unwrap();
                let mut network = b.build().unwrap();
                let mut path = "provider".to_owned();
                for name in ["inner", "outer"].into_iter().take(depth) {
                    let mut child = NetworkBuilder::from_def(&network);
                    child
                        .allow_replace(&child.component(path.split('/').next().unwrap()).unwrap())
                        .unwrap();
                    let mut parent = NetworkBuilder::new();
                    parent.module(name, child.build().unwrap()).unwrap();
                    network = parent.build().unwrap();
                    path = format!("{name}/{path}");
                }
                let mut composer = Composer::new(network).unwrap();
                let published = composer.current().compiled().declaration(&path).unwrap();
                let error = composer
                    .register([replacement(&path, new.clone())])
                    .unwrap_err();
                let ComposeError::Network(network_error) = &error else {
                    panic!("{error:?}")
                };
                assert_diagnostic(network_error, &canonical, &path);
                assert!(
                    error
                        .source()
                        .unwrap()
                        .downcast_ref::<NetworkError>()
                        .is_some()
                );
                published.validate(composer.current().compiled()).unwrap();
                assert!(composer.current().active().is_empty());
            }
        }
    }
}
fn assert_diagnostic(error: &NetworkError, canonical: &InterfaceError, path: &str) {
    let NetworkError::Interface(actual) = error else {
        panic!("{error:?}")
    };
    let mut expected = canonical.clone();
    expected.phase = InterfacePhase::Authoring;
    expected.path = path.into();
    assert_eq!(actual.as_ref(), &expected);
    assert_eq!(
        error.source().unwrap().downcast_ref::<InterfaceError>(),
        Some(actual.as_ref())
    );
    assert!(error.to_string().contains(path));
}

#[test]
fn facade_does_not_infer_claims_accept_width_or_erase_private_obligations() {
    let effects = Effects::default();
    let mut composer = Composer::new(agent(provider(false, &effects), &effects)).unwrap();
    let published = composer.current().compiled().capture("reply").unwrap();
    for (definition, reason, endpoint) in [
        (
            provider(true, &effects),
            InterfaceReason::MissingPort,
            "input context",
        ),
        (
            facade(&effects, false, true),
            InterfaceReason::ExtraPort,
            "output extra",
        ),
    ] {
        let error = composer
            .register([replacement(PROVIDER, definition)])
            .unwrap_err();
        assert!(
            matches!(error, ComposeError::Network(NetworkError::Interface(ref e)) if e.phase == InterfacePhase::Authoring && e.path.as_ref() == PROVIDER && *e.reason == reason && e.endpoint.as_deref() == Some(endpoint))
        );
        published.validate(composer.current().compiled()).unwrap();
    }
    let error = composer
        .register([replacement(PROVIDER, facade(&effects, false, false))])
        .unwrap_err();
    assert!(
        matches!(error, ComposeError::Network(NetworkError::Interface(ref e)) if e.phase == InterfacePhase::Compiled && e.path.as_ref() == PROVIDER && e.contract.as_deref() == Some(promise().id()) && matches!(*e.reason, InterfaceReason::MissingExplicitClaim { .. }))
    );
    published.validate(composer.current().compiled()).unwrap();

    // A contradictory definition lives on a real private declaration. Public
    // boundary equality does not exempt the candidate's full contract catalog.
    let mut contradictory = NetworkBuilder::new();
    let implementation = contradictory
        .add("implementation", facade(&effects, true, false))
        .unwrap();
    contradictory
        .expose_input(
            "context",
            implementation.wire_input::<Legacy>("context").unwrap(),
            one(),
        )
        .unwrap();
    contradictory
        .expose_output(
            "tool",
            implementation.wire_output::<Legacy>("tool").unwrap(),
        )
        .unwrap();
    contradictory
        .expose_output(
            "final",
            implementation.wire_output::<Legacy>("final").unwrap(),
        )
        .unwrap();
    let false_promise = Contract::new(promise().id(), Arc::new(Interface::default())).unwrap();
    contradictory
        .add(
            "contradiction",
            Arc::new(
                inert(Interface::default())
                    .as_ref()
                    .clone()
                    .claim(&false_promise)
                    .unwrap(),
            ),
        )
        .unwrap();
    contradictory.claim(&promise()).unwrap();
    let error = composer
        .register([replacement(
            PROVIDER,
            Arc::new(ComponentDef::composite(contradictory.build().unwrap())),
        )])
        .unwrap_err();
    assert!(
        matches!(error, ComposeError::Network(NetworkError::Interface(ref e)) if matches!(*e.reason, InterfaceReason::DifferingContractDefinition { .. }))
    );
    published.validate(composer.current().compiled()).unwrap();
    assert!(composer.current().active().is_empty());

    let mut private = NetworkBuilder::new();
    let core_definition = provider(false, &effects);
    let private_promise = Contract::new("private-engine@1", boundary()).unwrap();
    let core = private
        .add(
            "core",
            Arc::new(
                core_definition
                    .as_ref()
                    .clone()
                    .claim(&private_promise)
                    .unwrap(),
            ),
        )
        .unwrap();
    private
        .expose_input(
            "context",
            core.wire_input::<Legacy>("context").unwrap(),
            one(),
        )
        .unwrap();
    private
        .expose_output("tool", core.wire_output::<Legacy>("tool").unwrap())
        .unwrap();
    private
        .expose_output("final", core.wire_output::<Legacy>("final").unwrap())
        .unwrap();
    private.require_contract("core", &private_promise).unwrap();
    private.claim(&promise()).unwrap();
    let mut frozen = Composer::new(agent(
        Arc::new(ComponentDef::composite(private.build().unwrap())),
        &effects,
    ))
    .unwrap();
    let retained = frozen.current().compiled().capture("reply").unwrap();
    let error = frozen
        .register([replacement(PROVIDER, facade(&effects, true, false))])
        .unwrap_err();
    assert!(
        matches!(error, ComposeError::Network(NetworkError::UnknownProcess(ref path)) if path == &format!("{PROVIDER}/core")),
        "{error:?}"
    );
    retained.validate(frozen.current().compiled()).unwrap();
    assert!(
        frozen
            .current()
            .compiled()
            .inspect(&format!("{PROVIDER}/core"))
            .is_ok()
    );
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        turn(
            &mut start(frozen.current().compiled(), queue),
            "still-private",
            "v1",
        );
        turn(
            &mut start(composer.current().compiled(), queue),
            "still-base",
            "v1",
        );
    }
}
