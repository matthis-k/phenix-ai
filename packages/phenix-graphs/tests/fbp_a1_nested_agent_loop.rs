//! A1's private additive seams execute an actual model/tool/feedback/stop cycle.
#[path = "support/agent_loop.rs"]
mod agent_loop;

use agent_loop::{memory, model, tool};
use phenix_graph::{
    CompiledNetwork, ComponentDef, GraphOrigin, Value,
    compose::{ComposeError, Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration},
    contract::Contract,
    interface::{Interface, PortDef},
    network::{ConnectionDef, NetworkBuilder, NetworkDef, NetworkError, PortPath},
    runtime::{
        NetworkSession, RequestError, RequestQueuePolicy, RequestStatus, SessionOptions,
        SessionState,
    },
};
use std::{
    collections::BTreeSet,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const INNER: &str = "agent/loop";
const MEMORY: &str = "agent/loop/memory";
const MODEL: &str = "agent/loop/model";
const TOOL: &str = "agent/tool";

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}
fn port(process: &str, name: &str) -> PortPath {
    PortPath {
        process: process.into(),
        port: name.into(),
    }
}
fn agent_contract() -> Contract {
    Contract::new(
        "private-agent@1",
        Arc::new(
            Interface::new(
                [PortDef::local::<String>("prompt")],
                [PortDef::local::<String>("reply")],
            )
            .unwrap(),
        ),
    )
    .unwrap()
}

fn preset(model_calls: Arc<AtomicUsize>, origin: GraphOrigin) -> NetworkDef {
    let mut inner = NetworkBuilder::new();
    let memory = inner.add("memory", memory()).unwrap();
    let model = inner.add("model", model("nested", model_calls)).unwrap();
    inner
        .connect(
            memory.output::<String>("output").unwrap(),
            model.input::<String>("input").unwrap(),
            one(),
        )
        .unwrap();
    inner
        .expose_input("prompt", memory.input::<String>("input").unwrap(), one())
        .unwrap();
    inner
        .expose_output("reply", model.output::<String>("final").unwrap())
        .unwrap();
    inner.allow_extensions().unwrap();

    let mut outer = NetworkBuilder::new();
    let child = outer
        .add(
            "loop",
            Arc::new(ComponentDef::composite(inner.build().unwrap())),
        )
        .unwrap();
    outer
        .expose_input("prompt", child.input::<String>("prompt").unwrap(), one())
        .unwrap();
    outer
        .expose_output("reply", child.output::<String>("reply").unwrap())
        .unwrap();
    outer.allow_extensions().unwrap();
    outer.claim(&agent_contract()).unwrap();

    let mut root = NetworkBuilder::new().with_origin(origin);
    let agent = root
        .add(
            "agent",
            Arc::new(ComponentDef::composite(outer.build().unwrap())),
        )
        .unwrap();
    root.expose_input("prompt", agent.input::<String>("prompt").unwrap(), one())
        .unwrap();
    root.expose_output("reply", agent.output::<String>("reply").unwrap())
        .unwrap();
    root.require_contract("agent", &agent_contract()).unwrap();
    root.build().unwrap()
}

// Independently authored typed topology, not preset + patch or a replay oracle.
// Only the canonical process implementations and semantic boundary are shared.
fn canonical(model_calls: Arc<AtomicUsize>, tool_calls: Arc<AtomicUsize>) -> CompiledNetwork {
    let mut loop_builder = NetworkBuilder::new();
    let history = loop_builder.add("memory", memory()).unwrap();
    let decision = loop_builder
        .add("model", model("nested", model_calls))
        .unwrap();
    loop_builder
        .connect(
            history.output::<String>("output").unwrap(),
            decision.input::<String>("input").unwrap(),
            one(),
        )
        .unwrap();
    loop_builder
        .expose_input("prompt", history.input::<String>("input").unwrap(), one())
        .unwrap();
    loop_builder
        .expose_output("reply", decision.output::<String>("final").unwrap())
        .unwrap();
    loop_builder
        .expose_input("feedback", history.input::<String>("input").unwrap(), one())
        .unwrap();
    loop_builder
        .expose_output("call", decision.output::<String>("tool").unwrap())
        .unwrap();
    loop_builder.allow_extensions().unwrap();

    let mut agent_builder = NetworkBuilder::new();
    let reasoning = agent_builder
        .add(
            "loop",
            Arc::new(ComponentDef::composite(loop_builder.build().unwrap())),
        )
        .unwrap();
    let lookup = agent_builder.add("tool", tool(tool_calls)).unwrap();
    agent_builder
        .connect(
            reasoning.output::<String>("call").unwrap(),
            lookup.input::<String>("input").unwrap(),
            one(),
        )
        .unwrap();
    agent_builder
        .connect(
            lookup.output::<String>("output").unwrap(),
            reasoning.input::<String>("feedback").unwrap(),
            one(),
        )
        .unwrap();
    agent_builder
        .expose_input(
            "prompt",
            reasoning.input::<String>("prompt").unwrap(),
            one(),
        )
        .unwrap();
    agent_builder
        .expose_output("reply", reasoning.output::<String>("reply").unwrap())
        .unwrap();
    agent_builder.allow_extensions().unwrap();
    agent_builder.claim(&agent_contract()).unwrap();

    let mut network = NetworkBuilder::new();
    let agent = network
        .add(
            "agent",
            Arc::new(ComponentDef::composite(agent_builder.build().unwrap())),
        )
        .unwrap();
    network
        .expose_input("prompt", agent.input::<String>("prompt").unwrap(), one())
        .unwrap();
    network
        .expose_output("reply", agent.output::<String>("reply").unwrap())
        .unwrap();
    network
        .require_contract("agent", &agent_contract())
        .unwrap();
    network.build().unwrap().compile().unwrap()
}

fn start(compiled: &CompiledNetwork, queue: RequestQueuePolicy, feedback: bool) -> NetworkSession {
    let mut options = SessionOptions {
        capture_capacity: one(),
        max_requests: one(),
        request_queue_policy: queue,
        ..Default::default()
    };
    if !feedback {
        // The base has no tool/feedback route. Explicitly discard only this
        // unconnected output; an ordinary prompt cannot magically finish a loop.
        options
            .bound
            .discard_output(
                compiled,
                &compiled
                    .declaration(MODEL)
                    .unwrap()
                    .output::<String>("tool")
                    .unwrap(),
            )
            .unwrap();
    }
    let session = compiled.start_with_options(options);
    assert!(session.failure().is_none());
    session
}

fn turn(
    session: &mut NetworkSession,
    prompt: &str,
    feedback: bool,
) -> (Option<String>, Vec<(String, String)>) {
    let request = session.begin_request(feedback.then_some("reply")).unwrap();
    assert_eq!(session.begin_request(None), Err(RequestError::Full));
    session
        .inject_request(request, "prompt", Value::local(prompt.to_owned()))
        .unwrap();
    session.close_request(request).unwrap();
    let mut trace = Vec::new();
    let mut steps = 0;
    while let Some((_, job)) = session.next_job() {
        steps += 1;
        assert!(
            steps <= 32,
            "agent did not stop within its finite step budget"
        );
        let metadata = job.metadata().unwrap();
        assert_eq!(metadata.request, Some(request));
        assert_eq!(
            (metadata.session, metadata.generation),
            (request.session(), request.generation())
        );
        session.complete_job(job.run()).unwrap();
        let completed = session.last_step().unwrap();
        assert_eq!(completed.request, Some(request));
        if let Some(output) = &completed.output {
            trace.push((completed.path.clone(), output.clone()));
        }
    }
    assert!(session.failure().is_none());
    let packet = session.take_request_output(request, "reply").unwrap();
    assert_eq!(packet.is_some(), feedback);
    let reply = packet.map(|packet| {
        assert_eq!(packet.metadata().unwrap().request(), Some(request));
        assert_eq!(packet.metadata().unwrap().output(), "final");
        session.validate_capture(request, "reply", &packet).unwrap();
        packet.value().borrow::<String>().unwrap().clone()
    });
    assert_eq!(
        session.request_status(request),
        Ok(RequestStatus::Settled {
            captures: usize::from(feedback)
        })
    );
    session.release_request(request).unwrap();
    assert_eq!(session.request_status(request), Err(RequestError::Stale));
    // Neither final capture consumption nor scope release should restart tools.
    assert_eq!(session.run_until_idle(4).steps, 0);
    assert_eq!(session.state(), SessionState::Quiescent);
    (reply, trace)
}

#[test]
fn authorized_private_agent_cycle_matches_manual_graph_and_revokes_atomically() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let model_calls = Arc::new(AtomicUsize::new(0));
        let tool_calls = Arc::new(AtomicUsize::new(0));
        let provider = GraphOrigin::default().child();
        let mut composer = Composer::new(preset(model_calls.clone(), provider.clone())).unwrap();
        let base = composer.current().compiled().clone();
        let memory = base.declaration(MEMORY).unwrap();
        let model = base.declaration(MODEL).unwrap();
        let mut old = start(&base, queue, false);
        assert!(turn(&mut old, "before-extension", false).0.is_none());
        assert_eq!(model_calls.load(Ordering::SeqCst), 1);
        assert_eq!(tool_calls.load(Ordering::SeqCst), 0);

        let tool_origin = GraphOrigin::default().child();
        let tools = Registration::new(
            "tools",
            [],
            0,
            tool_origin.clone(),
            [Footprint::Component(TOOL.into())],
            NetworkPatch::new([Edit::Add {
                path: TOOL.into(),
                definition: tool(tool_calls.clone()),
            }]),
        );
        let call = ConnectionKey::new(port(INNER, "call"), port(TOOL, "input"));
        let feedback = ConnectionKey::new(port(TOOL, "output"), port(INNER, "feedback"));
        let cycle = NetworkPatch::new([
            Edit::expose_nested_bound_input(
                &base,
                INNER,
                "feedback",
                &[memory.input::<String>("input").unwrap()],
                one(),
            )
            .unwrap(),
            Edit::expose_nested_bound_output(
                &base,
                INNER,
                "call",
                &model.output::<String>("tool").unwrap(),
            )
            .unwrap(),
            Edit::Connect(ConnectionDef {
                from: call.from.clone(),
                to: call.to.clone(),
                capacity: one(),
            }),
            Edit::Connect(ConnectionDef {
                from: feedback.from.clone(),
                to: feedback.to.clone(),
                capacity: one(),
            }),
        ]);
        let grants: BTreeSet<_> = [
            Footprint::NestedInputExport {
                parent: INNER.into(),
                name: "feedback".into(),
            },
            Footprint::NestedOutputExport {
                parent: INNER.into(),
                name: "call".into(),
            },
            Footprint::Connection(call),
            Footprint::Connection(feedback.clone()),
        ]
        .into();
        assert_eq!(cycle.footprints(), grants);
        let mut denied = grants.clone();
        denied.remove(&Footprint::Connection(feedback.clone()));
        assert!(matches!(composer.register([
            tools.clone(),
            Registration::new("cycle", ["tools".into()], 0, provider.child(), denied, cycle.clone()),
        ]), Err(ComposeError::Unauthorized { footprint: Footprint::Connection(edge), .. }) if edge == feedback));
        memory.validate(composer.current().compiled()).unwrap();
        assert!(composer.current().active().is_empty());
        assert!(composer.current().compiled().inspect(TOOL).is_err());

        composer
            .register([
                tools,
                Registration::new(
                    "cycle",
                    ["tools".into()],
                    0,
                    provider.child(),
                    grants,
                    cycle,
                ),
            ])
            .unwrap();
        let compiled = composer.current().compiled().clone();
        assert_eq!(compiled.processes().len(), 3);
        assert_eq!(compiled.connections().len(), 3);
        assert!(
            compiled
                .connections()
                .iter()
                .all(|edge| edge.capacity() == one())
        );
        // With one admitted request and capture capacity one, this is the
        // scoped packet-slot envelope, not a bound on payload bytes or history.
        let packet_slots = compiled
            .connections()
            .iter()
            .map(|edge| edge.capacity().get())
            .sum::<usize>()
            + compiled
                .inputs()
                .iter()
                .map(|(name, targets)| compiled.ingress_capacities()[name].get() * targets.len())
                .sum::<usize>()
            + compiled.outputs().len();
        assert_eq!(packet_slots, 5);
        compiled
            .check_implementation("agent", &agent_contract())
            .unwrap();
        for path in [MEMORY, MODEL] {
            let original = base.inspect(path).unwrap();
            let current = compiled.inspect(path).unwrap();
            assert_eq!(current.definition_key, original.definition_key);
            assert_eq!(current.origin, original.origin);
        }
        assert_eq!(
            compiled.inspect(TOOL).unwrap().declared_origin,
            Some(tool_origin)
        );
        assert_eq!(
            compiled.inspect("agent").unwrap().interface,
            base.inspect("agent").unwrap().interface
        );

        let manual_models = Arc::new(AtomicUsize::new(0));
        let manual_tools = Arc::new(AtomicUsize::new(0));
        let manual = canonical(manual_models.clone(), manual_tools.clone());
        assert_eq!(compiled.connections(), manual.connections());
        assert_eq!(compiled.inputs(), manual.inputs());
        assert_eq!(compiled.ingress_capacities(), manual.ingress_capacities());
        assert_eq!(compiled.outputs(), manual.outputs());
        for path in ["", "agent", INNER, MEMORY, MODEL, TOOL] {
            assert_eq!(
                compiled.inspect(path).unwrap().interface,
                manual.inspect(path).unwrap().interface
            );
            assert_eq!(
                compiled.inspect(path).unwrap().claims,
                manual.inspect(path).unwrap().claims
            );
        }
        let mut actual = start(&compiled, queue, true);
        let mut expected = start(&manual, queue, true);
        let cycle_trace: Vec<_> = [
            (MEMORY, "output"),
            (MODEL, "tool"),
            (TOOL, "output"),
            (MEMORY, "output"),
            (MODEL, "final"),
        ]
        .into_iter()
        .map(|(path, output)| (path.to_owned(), output.to_owned()))
        .collect();
        for prompt in ["question-A", "question-B"] {
            let result = turn(&mut actual, prompt, true);
            assert_eq!(result, turn(&mut expected, prompt, true));
            assert_eq!(result.1, cycle_trace);
            let reply = result.0.unwrap();
            assert!(reply.contains(prompt));
            assert!(reply.contains("observation:lookup:nested"));
        }
        assert_eq!(manual_models.load(Ordering::SeqCst), 4);
        assert_eq!(manual_tools.load(Ordering::SeqCst), 2);
        assert!(turn(&mut old, "old-generation", false).0.is_none());
        assert_eq!(tool_calls.load(Ordering::SeqCst), 2);

        // Exact grants and a dependency do not authorize a nonexistent port.
        let broken = NetworkPatch::new([Edit::Connect(ConnectionDef {
            from: port(TOOL, "output"),
            to: port(INNER, "missing"),
            capacity: one(),
        })]);
        let active = composer.current().active().clone();
        let error = composer
            .register([Registration::new(
                "broken",
                ["cycle".into()],
                0,
                provider.child(),
                broken.footprints(),
                broken,
            )])
            .unwrap_err();
        // Compilation reports the port relative to the enclosing agent network.
        assert!(
            matches!(&error, ComposeError::Network(NetworkError::UnknownInput(path)) if path == &port("loop", "missing")),
            "{error:?}"
        );
        compiled
            .capture("reply")
            .unwrap()
            .validate(composer.current().compiled())
            .unwrap();
        assert_eq!(composer.current().active(), &active);
        assert_eq!(
            composer.current().compiled().connections(),
            compiled.connections()
        );
        let result = turn(&mut actual, "after-invalid-update", true);
        assert_eq!(result, turn(&mut expected, "after-invalid-update", true));
        assert_eq!(result.1, cycle_trace);

        assert_eq!(
            composer.revoke("tools").unwrap(),
            ["cycle".to_owned(), "tools".to_owned()].into()
        );
        assert!(composer.current().active().is_empty());
        assert!(composer.current().compiled().inspect(TOOL).is_err());
        assert_eq!(
            composer.current().compiled().connections(),
            base.connections()
        );
        assert_eq!(
            composer
                .current()
                .compiled()
                .inspect(INNER)
                .unwrap()
                .interface,
            base.inspect(INNER).unwrap().interface
        );
        composer
            .current()
            .compiled()
            .check_implementation("agent", &agent_contract())
            .unwrap();
        let mut restored = start(composer.current().compiled(), queue, false);
        assert!(turn(&mut restored, "after-revoke", false).0.is_none());
        let result = turn(&mut actual, "retained-generation", true);
        assert_eq!(result, turn(&mut expected, "retained-generation", true));
        assert_eq!(result.1, cycle_trace);

        // The model's stop branch can also finish without issuing any tool call.
        let result = turn(&mut actual, "observation:already-done", true);
        assert_eq!(
            result,
            turn(&mut expected, "observation:already-done", true)
        );
        assert!(result.0.as_ref().unwrap().contains("already-done"));
        assert_eq!(
            result.1,
            [
                (MEMORY.to_owned(), "output".to_owned()),
                (MODEL.to_owned(), "final".to_owned())
            ]
        );
        assert_eq!(manual_tools.load(Ordering::SeqCst), 4);
        assert_eq!(tool_calls.load(Ordering::SeqCst), 4);
        assert_eq!(manual_models.load(Ordering::SeqCst), 9);
        assert_eq!(model_calls.load(Ordering::SeqCst), 12); // Three incomplete base turns.
    }
}
