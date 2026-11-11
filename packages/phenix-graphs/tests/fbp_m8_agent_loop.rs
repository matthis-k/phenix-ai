//! M8 architectural proof: a genuine tool-feedback FBP cycle authored by
//! ordinary plugins. The deterministic model/tool stand-ins are not network
//! providers, sandboxed tools or durable memory implementations.
use phenix_graph::{
    ComponentDef, GraphOrigin, Value,
    compose::{Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration},
    interface::{Interface, PortDef},
    network::{ConnectionDef, NetworkBuilder, PortPath},
    process::{Process, ProcessContext, ProcessEmission, Step},
    runtime::{NetworkSession, SessionOptions, SessionState},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn capacity() -> NonZeroUsize {
    NonZeroUsize::new(2).unwrap()
}
fn port(process: &str, name: &str) -> PortPath {
    PortPath {
        process: process.to_owned(),
        port: name.to_owned(),
    }
}
fn edge(from: (&str, &str), to: (&str, &str)) -> ConnectionDef {
    ConnectionDef {
        from: port(from.0, from.1),
        to: port(to.0, to.1),
        capacity: capacity(),
    }
}
fn key(from: (&str, &str), to: (&str, &str)) -> ConnectionKey {
    ConnectionKey::new(port(from.0, from.1), port(to.0, to.1))
}

struct Memory {
    history: Vec<String>,
    emitted: bool,
}
impl Process for Memory {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let Some(input) = cx.take("input") else {
            return Ok(Step::Finished);
        };
        let latest = (*input.borrow::<String>()?).clone();
        self.history.push(latest.clone());
        self.emitted = true;
        // Each process instance owns its own history. The most recent message
        // drives the decision; earlier tool feedback does not short-circuit a
        // later independent user turn.
        Ok(Step::Emit(ProcessEmission::new(
            "output",
            Value::local(format!(
                "memory#{}[{}];latest={latest}",
                self.history.len(),
                self.history.join("|")
            )),
        )))
    }
}
fn memory() -> Arc<ComponentDef> {
    Arc::new(ComponentDef::primitive(
        Arc::new(
            Interface::new(
                [PortDef::local::<String>("input")],
                [PortDef::local::<String>("output")],
            )
            .unwrap(),
        ),
        Arc::new(|| Memory {
            history: Vec::new(),
            emitted: false,
        }),
    ))
}

struct Model {
    provider: &'static str,
    activations: usize,
    emitted: bool,
    effects: Arc<AtomicUsize>,
}
impl Process for Model {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let Some(input) = cx.take("input") else {
            return Ok(Step::Finished);
        };
        let context = (*input.borrow::<String>()?).clone();
        self.activations += 1;
        self.effects.fetch_add(1, Ordering::SeqCst);
        self.emitted = true;
        if context.contains(";latest=observation:") {
            Ok(Step::Emit(ProcessEmission::new(
                "final",
                Value::local(format!(
                    "{}:answer#{}:{context}",
                    self.provider, self.activations
                )),
            )))
        } else {
            Ok(Step::Emit(ProcessEmission::new(
                "tool",
                Value::local(format!("lookup:{}", self.provider)),
            )))
        }
    }
}
fn model(provider: &'static str, effects: Arc<AtomicUsize>) -> Arc<ComponentDef> {
    Arc::new(ComponentDef::primitive(
        Arc::new(
            Interface::new(
                [PortDef::local::<String>("input")],
                [
                    PortDef::local::<String>("tool"),
                    PortDef::local::<String>("final"),
                ],
            )
            .unwrap(),
        ),
        Arc::new(move || Model {
            provider,
            activations: 0,
            emitted: false,
            effects: effects.clone(),
        }),
    ))
}

fn tool(effects: Arc<AtomicUsize>) -> Arc<ComponentDef> {
    Arc::new(ComponentDef::map::<String, String, _>(move |input| {
        effects.fetch_add(1, Ordering::SeqCst);
        format!("observation:{input}")
    }))
}
fn compaction(effects: Arc<AtomicUsize>) -> Arc<ComponentDef> {
    Arc::new(ComponentDef::map::<String, String, _>(move |input| {
        effects.fetch_add(1, Ordering::SeqCst);
        format!("compact<{input}>")
    }))
}

fn register(
    id: &str,
    dependencies: &[&str],
    origin: GraphOrigin,
    edits: Vec<Edit>,
    additional_grants: impl IntoIterator<Item = Footprint>,
) -> Registration {
    // Trusted test host approves these exact paths, not a plugin-furnished
    // arbitrary namespace/wildcard. Registration carries the approved grants.
    let patch = NetworkPatch::new(edits);
    let mut grants = patch.footprints();
    grants.extend(additional_grants);
    Registration::new(
        id,
        dependencies.iter().map(|id| (*id).to_owned()),
        0,
        origin,
        grants,
        patch,
    )
}

fn preset(model_calls: Arc<AtomicUsize>, tool_calls: Arc<AtomicUsize>) -> Composer {
    let host = GraphOrigin::default();
    let mut composer = Composer::new(
        NetworkBuilder::new()
            .with_origin(host.clone())
            .build()
            .unwrap(),
    )
    .unwrap();
    assert!(composer.current().compiled().processes().is_empty());
    let edits = vec![
        Edit::Add {
            path: "memory".into(),
            definition: memory(),
        },
        Edit::Add {
            path: "model".into(),
            definition: model("builtin", model_calls),
        },
        Edit::Add {
            path: "tool".into(),
            definition: tool(tool_calls),
        },
        Edit::Connect(edge(("memory", "output"), ("model", "input"))),
        Edit::Connect(edge(("model", "tool"), ("tool", "input"))),
        // This edge is the real cycle; the model is not a monolithic agent loop.
        Edit::Connect(edge(("tool", "output"), ("memory", "input"))),
        Edit::ExposeInput {
            name: "prompt".into(),
            targets: vec![port("memory", "input")],
            capacity: capacity(),
        },
        Edit::ExposeOutput {
            name: "reply".into(),
            from: port("model", "final"),
        },
    ];
    composer
        .register([register("preset", &[], host.child(), edits, [])])
        .unwrap();
    composer
}
fn receive(session: &mut NetworkSession, prompt: &str) -> String {
    session
        .inject("prompt", Value::local(prompt.to_owned()))
        .unwrap();
    let report = session.run_until_idle(128);
    assert!(!report.budget_exhausted, "unexpected unbounded process loop");
    assert_ne!(report.state, SessionState::Failed);
    let reply = session.take_output("reply").unwrap().unwrap();
    (*reply.value().borrow::<String>().unwrap()).clone()
}
fn session(composer: &Composer) -> NetworkSession {
    composer.current().compiled().start_with_options(SessionOptions {
        capture_capacity: NonZeroUsize::new(1).unwrap(),
        ..Default::default()
    })
}

#[test]
fn ordinary_plugins_form_a_real_multi_activation_agent_cycle() {
    let model_calls = Arc::new(AtomicUsize::new(0));
    let tool_calls = Arc::new(AtomicUsize::new(0));
    let compact_calls = Arc::new(AtomicUsize::new(0));
    let mut composer = preset(model_calls.clone(), tool_calls.clone());
    let baseline = receive(&mut session(&composer), "question-A");
    assert!(baseline.starts_with("builtin:answer#2:"));
    assert!(baseline.contains("memory#2"));
    assert!(baseline.contains("observation:lookup:builtin"));
    assert!(baseline.contains("question-A"));
    assert_eq!(model_calls.load(Ordering::SeqCst), 2);
    assert_eq!(tool_calls.load(Ordering::SeqCst), 1);

    let host = GraphOrigin::default();
    let provider = register(
        "provider",
        &["preset"],
        host.child(),
        vec![Edit::Replace {
            path: "model".into(),
            definition: model("alternative", model_calls.clone()),
        }],
        [],
    );
    let patch = register(
        "compaction",
        &["preset"],
        host.child(),
        vec![
            Edit::Add {
                path: "compaction".into(),
                definition: compaction(compact_calls.clone()),
            },
            Edit::Splice {
                anchor: key(("memory", "output"), ("model", "input")),
                input: port("compaction", "input"),
                output: port("compaction", "output"),
            },
        ],
        [Footprint::Connection(key(
            ("memory", "output"),
            ("compaction", "input"),
        ))],
    );
    composer.register([provider, patch]).unwrap();
    let modified = receive(&mut session(&composer), "question-B");
    assert!(modified.starts_with("alternative:answer#2:"));
    assert!(modified.contains("compact<"));
    assert!(modified.contains("observation:lookup:alternative"));
    assert!(modified.contains("question-B"));
    assert_eq!(tool_calls.load(Ordering::SeqCst), 2);
    assert_eq!(model_calls.load(Ordering::SeqCst), 4);
    assert_eq!(compact_calls.load(Ordering::SeqCst), 2);

    composer.revoke("compaction").unwrap();
    let no_compaction = receive(&mut session(&composer), "question-C");
    assert!(no_compaction.starts_with("alternative:answer#2:"));
    assert!(!no_compaction.contains("compact<"));
    assert_eq!(compact_calls.load(Ordering::SeqCst), 2);
    composer.revoke("provider").unwrap();
    let restored = receive(&mut session(&composer), "question-D");
    assert!(restored.starts_with("builtin:answer#2:"));
    composer.revoke("preset").unwrap();
    assert!(composer.current().compiled().processes().is_empty());
}

#[test]
fn per_session_memory_isolated_and_cancelled_feedback_cannot_resume() {
    let model_calls = Arc::new(AtomicUsize::new(0));
    let tool_calls = Arc::new(AtomicUsize::new(0));
    let composer = preset(model_calls.clone(), tool_calls.clone());
    let mut a = session(&composer);
    let mut b = session(&composer);

    let first = receive(&mut a, "A-first");
    assert!(first.contains("memory#2"));
    assert!(!first.contains("B-first"));
    let second = receive(&mut b, "B-first");
    assert!(second.contains("memory#2"));
    assert!(!second.contains("A-first"));
    let followup = receive(&mut a, "A-second");
    assert!(followup.contains("memory#4"));
    assert!(followup.contains("A-first"));
    assert!(!followup.contains("B-first"));
    assert_eq!(tool_calls.load(Ordering::SeqCst), 3);
    assert_eq!(model_calls.load(Ordering::SeqCst), 6);

    let mut c = session(&composer);
    c.inject("prompt", Value::local(String::from("cancel-me")))
        .unwrap();
    // Only the first memory step runs; there is no model or tool effect yet.
    let report = c.run_until_idle(1);
    assert_eq!(report.steps, 1);
    c.cancel();
    assert_eq!(c.state(), SessionState::Cancelled);
    assert!(c.take_output("reply").unwrap().is_none());
    assert_eq!(c.run_until_idle(128).steps, 0);
    assert_eq!(tool_calls.load(Ordering::SeqCst), 3);
}

#[test]
fn blocked_output_does_not_repeat_tool_effects_on_retry() {
    let model_calls = Arc::new(AtomicUsize::new(0));
    let tool_calls = Arc::new(AtomicUsize::new(0));
    let composer = preset(model_calls, tool_calls.clone());
    let mut s = session(&composer);

    s.inject("prompt", Value::local(String::from("first")))
        .unwrap();
    let first = s.run_until_idle(128);
    assert!(!first.budget_exhausted);
    assert_eq!(tool_calls.load(Ordering::SeqCst), 1);

    s.inject("prompt", Value::local(String::from("second")))
        .unwrap();
    let report = s.run_until_idle(128);
    assert!(!report.budget_exhausted);
    // The first capture is bounded to 1. The second final emission must wait
    // for the host to drain the capture, without reissuing the tool effect.
    assert_eq!(tool_calls.load(Ordering::SeqCst), 2);
    for _ in 0..3 {
        s.run_until_idle(128);
        assert_eq!(tool_calls.load(Ordering::SeqCst), 2);
    }
    let one = s.take_output("reply").unwrap().unwrap();
    assert!(one.value().borrow::<String>().unwrap().contains("first"));
    s.run_until_idle(128);
    let two = s.take_output("reply").unwrap().unwrap();
    assert!(two.value().borrow::<String>().unwrap().contains("second"));
    assert_eq!(tool_calls.load(Ordering::SeqCst), 2);
    assert!(s.failure().is_none());
}
