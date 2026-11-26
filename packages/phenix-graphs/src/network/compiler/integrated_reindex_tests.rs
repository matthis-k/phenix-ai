//! A1 private exports and A2 adapted replacements must survive Q2 relocation.
use super::*;
use crate::ComponentDef;
use crate::compose::{Composer, Edit, NetworkPatch, Registration};
use crate::interface::Interface;
use crate::network::{ConnectionDef, NetworkBuilder, PortPath};
use crate::process::{Process, ProcessContext, Step};
use crate::runtime::{
    NetworkSession, Readiness, RequestError, RequestFailurePolicy, RequestQueuePolicy,
    RequestStatus, RequestWork, SessionOptions,
};
use crate::{Failure, Value};

const ROOTS: [&str; 3] = ["a-orphan", "middle-orphan", "z-orphan"];
const PROVIDER: &str = "left/inner/provider";

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

fn port(process: &str, name: &str) -> PortPath {
    PortPath {
        process: process.into(),
        port: name.into(),
    }
}

fn connect(from: &str, to: &str) -> Edit {
    Edit::Connect(ConnectionDef {
        from: port(from, "output"),
        to: port(to, "input"),
        capacity: one(),
    })
}

fn registration(id: &str, dependencies: &[&str], patch: NetworkPatch) -> Registration {
    Registration::new(
        id,
        dependencies.iter().map(|id| (*id).into()),
        0,
        GraphOrigin::default().child(),
        patch.footprints(),
        patch,
    )
}

fn promise() -> Contract {
    Contract::new(
        "integrated/provider@1",
        ComponentDef::adapt_wire::<i64, i64, _>(|n| Ok(n + 1))
            .unwrap()
            .interface()
            .clone(),
    )
    .unwrap()
}

struct Boot;
impl Process for Boot {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
        Ok(Step::Finished)
    }
}

#[derive(Default)]
struct Monitor(Option<i64>);
impl Process for Monitor {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, Failure> {
        // max_requests = 1 forces reuse of the same correlation lane after release.
        assert_eq!(cx.correlation_lane(), Some(0));
        if let Some(expected) = self.0.take() {
            assert_eq!(*cx.take("resume").unwrap().decoded::<i64>()?, expected);
            Ok(Step::Complete)
        } else if let Some(value) = cx.take("input") {
            self.0 = Some(*value.decoded::<i64>()?);
            Ok(Step::WaitInput("resume".into()))
        } else {
            Ok(Step::Finished)
        }
    }
}

fn preset() -> NetworkDef {
    let mut inner = NetworkBuilder::new();
    let provider = inner
        .add(
            "provider",
            Arc::new(
                ComponentDef::adapt_wire::<i64, i64, _>(|n| Ok(n + 1))
                    .unwrap()
                    .claim(&promise())
                    .unwrap(),
            ),
        )
        .unwrap();
    let boot = inner
        .add(
            "boot",
            Arc::new(ComponentDef::primitive(
                Arc::new(Interface::default()),
                Arc::new(|| Boot),
            )),
        )
        .unwrap();
    inner.start(&boot).unwrap();
    inner
        .expose_input("input", provider.wire_input::<i64>("input").unwrap(), one())
        .unwrap();
    inner
        .expose_output("output", provider.wire_output::<i64>("output").unwrap())
        .unwrap();
    inner.allow_replace(&provider).unwrap();
    inner.require_contract("provider", &promise()).unwrap();
    inner.allow_extensions().unwrap();

    let mut outer = NetworkBuilder::new();
    let child = outer.module("inner", inner.build().unwrap()).unwrap();
    outer.allow_replace(&child).unwrap();
    outer.start(&child).unwrap();
    outer
        .expose_input("input", child.wire_input::<i64>("input").unwrap(), one())
        .unwrap();
    outer
        .expose_output("output", child.wire_output::<i64>("output").unwrap())
        .unwrap();
    outer.allow_extensions().unwrap();
    let module = Arc::new(ComponentDef::composite(outer.build().unwrap()));

    let mut root = NetworkBuilder::new().with_origin(GraphOrigin::default().child());
    // Two nested groups permit an isolated root strictly between physical IDs.
    for name in ["left", "right"] {
        let child = root.add(name, module.clone()).unwrap();
        root.start(&child).unwrap();
        root.expose_input(name, child.wire_input::<i64>("input").unwrap(), one())
            .unwrap();
        root.expose_output(name, child.wire_output::<i64>("output").unwrap())
            .unwrap();
        if name == "left" {
            root.expose_output("alias", child.wire_output::<i64>("output").unwrap())
                .unwrap();
        }
    }
    root.build().unwrap()
}

fn adapted() -> Arc<ComponentDef> {
    let mut builder = NetworkBuilder::new();
    let encode = builder
        .add(
            "encode",
            Arc::new(ComponentDef::adapt_wire::<i64, String, _>(|n| Ok(n.to_string())).unwrap()),
        )
        .unwrap();
    let decode = builder
        .add(
            "decode",
            Arc::new(
                ComponentDef::adapt_wire::<String, i64, _>(|s| {
                    s.parse::<i64>()
                        .map(|n| n + 10)
                        .map_err(|e| Failure::external("number conversion", e))
                })
                .unwrap(),
            ),
        )
        .unwrap();
    builder
        .connect(
            encode.wire_output::<String>("output").unwrap(),
            decode.wire_input::<String>("input").unwrap(),
            one(),
        )
        .unwrap();
    builder
        .expose_input("input", encode.wire_input::<i64>("input").unwrap(), one())
        .unwrap();
    builder
        .expose_output("output", decode.wire_output::<i64>("output").unwrap())
        .unwrap();
    builder.claim(&promise()).unwrap();
    let definition = Arc::new(ComponentDef::composite(builder.build().unwrap()));
    assert_eq!(definition.interface(), promise().required());
    definition
}

fn fixture() -> Composer {
    let mut composer = Composer::new(preset()).unwrap();
    composer
        .register([registration(
            "a2",
            &[],
            NetworkPatch::new([Edit::Replace {
                path: PROVIDER.into(),
                definition: adapted(),
            }]),
        )])
        .unwrap();
    composer
        .register([registration(
            "a1-add",
            &["a2"],
            NetworkPatch::new([
                Edit::Add {
                    path: "left/inner/side".into(),
                    definition: Arc::new(
                        ComponentDef::adapt_wire::<i64, i64, _>(|n| Ok(*n)).unwrap(),
                    ),
                },
                connect(PROVIDER, "left/inner/side"),
                Edit::Add {
                    path: "left/monitor".into(),
                    definition: Arc::new(ComponentDef::primitive(
                        Arc::new(
                            Interface::new(
                                ["input", "resume"]
                                    .map(|name| crate::PortDef::wire::<i64>(name).unwrap()),
                                [],
                            )
                            .unwrap(),
                        ),
                        Arc::new(Monitor::default),
                    )),
                },
            ]),
        )])
        .unwrap();
    let compiled = composer.current().compiled();
    let provider = compiled.declaration(PROVIDER).unwrap();
    let side = compiled.declaration("left/inner/side").unwrap();
    let monitor = compiled.declaration("left/monitor").unwrap();
    let patch = NetworkPatch::new([
        Edit::expose_nested_bound_input(
            compiled,
            "left/inner",
            "alternate",
            &[provider.wire_input::<i64>("input").unwrap()],
            one(),
        )
        .unwrap(),
        Edit::expose_nested_bound_output(
            compiled,
            "left/inner",
            "telemetry",
            &side.wire_output::<i64>("output").unwrap(),
        )
        .unwrap(),
        Edit::Connect(ConnectionDef {
            from: port("left/inner", "telemetry"),
            to: port("left/monitor", "input"),
            capacity: one(),
        }),
        Edit::expose_nested_bound_input(
            compiled,
            "left",
            "resume",
            &[monitor.wire_input::<i64>("resume").unwrap()],
            one(),
        )
        .unwrap(),
        Edit::ExposeInput {
            name: "resume".into(),
            targets: vec![port("left", "resume")],
            capacity: one(),
        },
    ]);
    composer
        .register([registration("a1-export", &["a1-add"], patch)])
        .unwrap();
    // Freeze the completed A1/A2 edits. Replaying nested copy-on-write edits
    // changes declaration keys and is deliberately ineligible for Q2 reuse.
    Composer::new(composer.current().network().clone()).unwrap()
}

fn assert_oracle(actual: &CompiledNetwork, network: &NetworkDef) {
    // This always enters canonical recursive lowering, never Composer's reuse chain.
    let expected = network.compile().unwrap();
    assert_eq!(actual.interface(), expected.interface());
    assert_eq!(actual.connections(), expected.connections());
    assert_eq!(actual.inputs(), expected.inputs());
    assert_eq!(actual.outputs(), expected.outputs());
    assert_eq!(actual.ingress_capacities, expected.ingress_capacities);
    assert_eq!(actual.source_starts(), expected.source_starts());
    assert!(!actual.source_starts().is_empty());
    assert_eq!(actual.compile_work, expected.compile_work);
    assert_eq!(actual.compile_instances, expected.compile_instances);
    assert_eq!(actual.compilation_limits, expected.compilation_limits);
    assert_eq!(actual.contracts, expected.contracts);
    assert_eq!(
        actual.structural_reachability(),
        expected.structural_reachability()
    );
    assert_eq!(
        actual.structural_feedback_cycles(),
        expected.structural_feedback_cycles()
    );
    assert_eq!(actual.processes.len(), expected.processes.len());
    for (index, (a, b)) in actual.processes.iter().zip(&expected.processes).enumerate() {
        assert_eq!(a.path(), b.path());
        assert_eq!(a.interface(), b.interface());
        assert_eq!(a.origin(), b.origin());
        assert!(Arc::ptr_eq(a.factory(), b.factory()));
        assert_eq!(actual.physical_process_index(a.path()), Some(index));
    }
    assert_eq!(
        actual.inspections.keys().collect::<Vec<_>>(),
        expected.inspections.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        actual.boundaries.keys().collect::<Vec<_>>(),
        expected.boundaries.keys().collect::<Vec<_>>()
    );
    for (path, a) in &actual.inspections {
        let b = expected.inspect(path).unwrap();
        assert_eq!(a.path, b.path);
        assert_eq!(a.interface, b.interface);
        assert_eq!(a.claims, b.claims);
        assert_eq!(a.origin, b.origin);
        assert_eq!(a.declared_origin, b.declared_origin);
        assert_eq!(a.definition_key, b.definition_key);
        assert_eq!(a.physical_index, b.physical_index);
        // Include every nested alias, not just the root ingress/egress maps.
        assert_eq!(
            actual.boundaries[path].inputs,
            expected.boundaries[path].inputs
        );
        assert_eq!(
            actual.boundaries[path].outputs,
            expected.boundaries[path].outputs
        );
    }
    actual.check_implementation(PROVIDER, &promise()).unwrap();
    assert_eq!(actual.outputs()["left"], actual.outputs()["alias"]);
    let provider = actual.bind(PROVIDER, &promise()).unwrap();
    assert_eq!(
        provider
            .wire_input::<i64>("input")
            .unwrap()
            .resolve(actual)
            .unwrap(),
        actual
            .declaration("left/inner")
            .unwrap()
            .wire_input::<i64>("alternate")
            .unwrap()
            .resolve(actual)
            .unwrap()
    );
}

fn assert_authority(old: &CompiledNetwork, new: &CompiledNetwork) {
    assert!(!Arc::ptr_eq(&old.generation, &new.generation));
    for (parent, child, input, output) in [
        ("left/inner", PROVIDER, "input", "output"),
        ("left", "left/inner", "alternate", "telemetry"),
        ("right/inner", "right/inner/provider", "input", "output"),
    ] {
        let old_child = old.declaration(child).unwrap();
        let stale_input = old_child.wire_input::<i64>(input).unwrap();
        let stale_output = old_child.wire_output::<i64>(output).unwrap();
        assert!(stale_input.resolve(old).is_ok());
        assert!(stale_output.resolve(old).is_ok());
        assert_eq!(stale_input.resolve(new), Err(NetworkError::ForeignScope));
        assert_eq!(stale_output.resolve(new), Err(NetworkError::ForeignScope));
        assert!(matches!(
            Edit::expose_nested_bound_input(
                new,
                parent,
                "fresh-in",
                std::slice::from_ref(&stale_input),
                one()
            ),
            Err(NetworkError::ForeignScope)
        ));
        assert!(matches!(
            Edit::expose_nested_bound_output(new, parent, "fresh-out", &stale_output),
            Err(NetworkError::ForeignScope)
        ));
        assert!(
            Edit::expose_nested_bound_input(old, parent, "old-in", &[stale_input], one()).is_ok()
        );
        assert!(Edit::expose_nested_bound_output(old, parent, "old-out", &stale_output).is_ok());
        let rebound = new.declaration(child).unwrap();
        assert_eq!(
            rebound
                .wire_input::<i64>(input)
                .unwrap()
                .resolve(new)
                .unwrap(),
            new.boundaries[child].inputs[input]
        );
        assert_eq!(
            rebound
                .wire_output::<i64>(output)
                .unwrap()
                .resolve(new)
                .unwrap(),
            (
                new.boundaries[child].outputs[output].0,
                new.boundaries[child].outputs[output].1.as_str()
            )
        );
        let edit = Edit::expose_nested_bound_input(
            new,
            parent,
            "fresh-in",
            &[rebound.wire_input::<i64>(input).unwrap()],
            one(),
        )
        .unwrap();
        assert!(
            matches!(edit, Edit::ExposeNestedInput { targets, .. } if targets == vec![port(child, input)])
        );
        let edit = Edit::expose_nested_bound_output(
            new,
            parent,
            "fresh-out",
            &rebound.wire_output::<i64>(output).unwrap(),
        )
        .unwrap();
        assert!(
            matches!(edit, Edit::ExposeNestedOutput { from, .. } if from == port(child, output))
        );
    }
}

fn start(compiled: &CompiledNetwork, policy: RequestQueuePolicy) -> NetworkSession {
    let session = compiled.start_with_options(SessionOptions {
        request_queue_policy: policy,
        request_failure_policy: RequestFailurePolicy::RestartAffectedProcess,
        readiness: [("left/monitor".into(), Readiness::ANY)].into(),
        capture_capacity: one(),
        max_requests: one(),
        ..Default::default()
    });
    assert!(session.failure().is_none(), "{:?}", session.failure());
    for (index, process) in compiled.processes().iter().enumerate() {
        assert_eq!(compiled.physical_process_index(process.path()), Some(index));
        assert!(session.process_notifier(process.path()).unwrap().notify());
        assert!(session.wake_handle(process.path()).unwrap().wake());
    }
    for path in ["", "left", "left/inner", PROVIDER, "right", "missing"] {
        assert_eq!(compiled.physical_process_index(path), None);
        assert!(session.process_notifier(path).is_none());
        assert!(session.wake_handle(path).is_none());
    }
    session
}

fn turn(session: &mut NetworkSession, input: i64) -> Vec<(String, Option<String>)> {
    let request = session.begin_request(None).unwrap();
    session
        .inject_request(request, "left", Value::of(input))
        .unwrap();
    session
        .inject_request(request, "right", Value::of(input))
        .unwrap();
    let mut trace = Vec::new();
    for resume in [false, true] {
        if resume {
            assert_eq!(session.request_work(request).unwrap().waiting_input, 1);
            assert_eq!(
                session.release_request(request),
                Err(RequestError::NotSettled)
            );
            session
                .inject_request(request, "resume", Value::of(input + 10))
                .unwrap();
            session.close_request(request).unwrap();
        }
        while let Some((_, job)) = session.next_job() {
            assert!(trace.len() < 64, "nested request failed to settle");
            session.complete_job(job.run()).unwrap();
            let step = session.last_step().unwrap();
            if step.request == Some(request) {
                trace.push((step.path.clone(), step.output.clone()));
            }
        }
    }
    assert!(session.failure().is_none(), "{:?}", session.failure());
    for (name, expected) in [
        ("left", input + 10),
        ("alias", input + 10),
        ("right", input + 1),
    ] {
        let packet = session.take_request_output(request, name).unwrap().unwrap();
        assert_eq!(*packet.value().decoded::<i64>().unwrap(), expected);
        assert_eq!(packet.metadata().unwrap().request(), Some(request));
        assert_eq!(packet.metadata().unwrap().capture(), Some(name));
        session.validate_capture(request, name, &packet).unwrap();
        if name == "left" {
            assert_eq!(
                session.validate_capture(request, "alias", &packet),
                Err(RequestError::InvalidCapture)
            );
        }
        assert!(
            session
                .take_request_output(request, name)
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        session.request_status(request),
        Ok(RequestStatus::Settled { captures: 3 })
    );
    session.release_request(request).unwrap();
    assert_eq!(session.request_status(request), Err(RequestError::Stale));
    assert_eq!(session.run_until_idle(4).steps, 0);
    for path in [
        "left/inner/provider/encode",
        "left/inner/provider/decode",
        "left/inner/side",
        "left/monitor",
        "right/inner/provider",
    ] {
        assert!(
            trace.iter().any(|(actual, _)| actual == path),
            "missing execution: {path}"
        );
    }
    trace
}

fn cancel_staged(session: &mut NetworkSession) {
    let request = session.begin_request(None).unwrap();
    assert_eq!(session.begin_request(None), Err(RequestError::Full));
    session
        .inject_request(request, "left", Value::of(101_i64))
        .unwrap();
    let report = session.run_until_idle(64);
    assert!(!report.budget_exhausted);
    assert!(session.failure().is_none(), "{:?}", session.failure());
    assert_eq!(session.request_work(request).unwrap().waiting_input, 1);
    assert_eq!(
        session.release_request(request),
        Err(RequestError::NotSettled)
    );
    let notifier = session.process_notifier("left/monitor").unwrap();
    let wake = session.wake_handle("left/monitor").unwrap();
    assert!(session.cancel_request(request).unwrap().is_empty());
    assert!(!notifier.notify());
    assert!(!wake.wake());
    assert_eq!(
        session.request_status(request),
        Ok(RequestStatus::Cancelled)
    );
    assert_eq!(
        session.request_work(request).unwrap(),
        RequestWork::default()
    );
    let disposal = session.request_disposal(request).unwrap();
    assert_eq!(disposal.activations, 1);
    assert_eq!(disposal.queued_packets, 2);
    assert_eq!(disposal.handed_jobs, 0);
    session.release_request(request).unwrap();
    assert_eq!(session.request_status(request), Err(RequestError::Stale));
    assert_eq!(session.run_until_idle(4).steps, 0);
}

#[test]
fn nested_a1_a2_survivors_match_full_lowering_through_single_root_reindex() {
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let mut composer = fixture();
        let initial = composer.current().compiled().clone();
        assert_oracle(&initial, composer.current().network());
        let mut old_session = start(&initial, policy);
        let expected_trace = turn(&mut old_session, 7);
        assert_eq!(
            turn(
                &mut start(&composer.current().network().compile().unwrap(), policy),
                7
            ),
            expected_trace
        );
        for name in ROOTS {
            let before = composer.current().network().clone();
            let old = composer.current().compiled().clone();
            composer
                .register([registration(
                    name,
                    &[],
                    NetworkPatch::new([Edit::Add {
                        path: name.into(),
                        definition: Arc::new(ComponentDef::map(|n: i64| n)),
                    }]),
                )])
                .unwrap();
            let after = composer.current().network();
            let incremental = old
                .reuse_after_isolated_root_addition(&before, after)
                .expect("nested survivors must actually qualify for Q2 insertion");
            let insertion = incremental.physical_process_index(name).unwrap();
            let left = old
                .physical_process_index("left/inner/provider/encode")
                .unwrap();
            let right = old.physical_process_index("right/inner/provider").unwrap();
            match name {
                "a-orphan" => assert!(insertion <= left),
                "middle-orphan" => assert!(left < insertion && insertion <= right),
                "z-orphan" => assert!(right < insertion),
                _ => unreachable!(),
            }
            for path in ["left/inner/provider/encode", "right/inner/provider"] {
                let index = old.physical_process_index(path).unwrap();
                assert_eq!(
                    incremental.physical_process_index(path),
                    Some(index + usize::from(index >= insertion))
                );
            }
            assert_eq!(
                turn(&mut start(&after.compile().unwrap(), policy), 7),
                expected_trace
            );
            for compiled in [&incremental, composer.current().compiled()] {
                assert_oracle(compiled, after);
                assert_authority(&old, compiled);
                let mut session = start(compiled, policy);
                assert_eq!(turn(&mut session, 7), expected_trace);
                cancel_staged(&mut session);
                assert_eq!(turn(&mut session, 19), expected_trace);
            }
        }
        for name in ROOTS {
            let before = composer.current().network().clone();
            let old = composer.current().compiled().clone();
            composer.revoke(name).unwrap();
            let after = composer.current().network();
            let incremental = old
                .reuse_after_isolated_root_removal(&before, after)
                .expect("nested survivors must actually qualify for Q2 removal");
            let removal = old.physical_process_index(name).unwrap();
            for process in incremental.processes() {
                let index = old.physical_process_index(process.path()).unwrap();
                assert_eq!(
                    incremental.physical_process_index(process.path()),
                    Some(index - usize::from(index > removal))
                );
            }
            assert_eq!(
                turn(&mut start(&after.compile().unwrap(), policy), 7),
                expected_trace
            );
            for compiled in [&incremental, composer.current().compiled()] {
                assert_oracle(compiled, after);
                assert_authority(&old, compiled);
                let mut session = start(compiled, policy);
                assert_eq!(turn(&mut session, 7), expected_trace);
                cancel_staged(&mut session);
                assert_eq!(turn(&mut session, 19), expected_trace);
            }
        }
        // A previously started compiled session retains its own topology and authority.
        assert_eq!(turn(&mut old_session, 19), expected_trace);
        cancel_staged(&mut old_session);
        assert_eq!(turn(&mut old_session, 31), expected_trace);
        assert_eq!(
            initial.processes().len(),
            composer.current().compiled().processes().len()
        );
    }
}

#[test]
fn nested_a1_a2_survivors_match_full_lowering_for_every_batch_root_subset() {
    let mut composer = fixture();
    let patch = NetworkPatch::new(ROOTS.map(|name| Edit::Add {
        path: name.into(),
        definition: Arc::new(ComponentDef::map(|n: i64| n)),
    }));
    composer
        .register([registration("pack", &[], patch)])
        .unwrap();
    let before = composer.current().network().clone();
    let old = composer.current().compiled().clone();
    assert_oracle(&old, &before);
    for mask in 1_u32..(1 << ROOTS.len()) {
        if mask.count_ones() < 2 {
            continue;
        }
        let mut after = before.clone();
        for (bit, name) in ROOTS.iter().enumerate() {
            if mask & (1 << bit) != 0 {
                after.instances.remove(*name);
            }
        }
        let incremental = old
            .reuse_after_batch_isolated_root_removal(&before, &after)
            .expect("all isolated-root subsets around nested survivors must qualify");
        assert_oracle(&incremental, &after);
        assert_authority(&old, &incremental);
        for policy in [
            RequestQueuePolicy::SharedFifo,
            RequestQueuePolicy::BoundedLanes,
        ] {
            let canonical = after.compile().unwrap();
            let mut old_session = start(&old, policy);
            let mut session = start(&incremental, policy);
            let trace = turn(&mut session, 7);
            assert_eq!(trace, turn(&mut start(&canonical, policy), 7));
            cancel_staged(&mut session);
            assert_eq!(trace, turn(&mut session, 31));
            assert_eq!(trace, turn(&mut old_session, 19));
        }
    }
    composer.revoke("pack").unwrap();
    let published = composer.current().compiled();
    assert_oracle(published, composer.current().network());
    assert_authority(&old, published);
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        turn(&mut start(published, policy), 31);
    }
}

#[test]
fn addressed_notifiers_and_wakes_isolate_nested_peers_after_q2_reindex() {
    use crate::process::ProcessEmission;
    use crate::runtime::{ReadinessPolicy, ReadinessSnapshot};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Default)]
    struct Gate {
        allowed: AtomicBool,
        calls: AtomicUsize,
    }
    impl ReadinessPolicy for Gate {
        fn notification_driven(&self) -> bool {
            true
        }

        fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.allowed.load(Ordering::SeqCst) {
                return vec![];
            }
            snapshot
                .heads
                .iter()
                .filter(|head| head.available)
                .map(|head| head.port.clone())
                .collect()
        }
    }

    #[derive(Default)]
    struct WakeEcho {
        stage: u8,
        value: Option<Value>,
    }
    impl Process for WakeEcho {
        fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, Failure> {
            match self.stage {
                0 => {
                    self.value = Some(cx.take("input").unwrap());
                    self.stage = 1;
                    Ok(Step::WaitWake)
                }
                1 => {
                    self.stage = 2;
                    Ok(Step::Emit(ProcessEmission::new(
                        "output",
                        self.value.take().unwrap(),
                    )))
                }
                2 => {
                    self.stage = 0;
                    Ok(Step::Complete)
                }
                _ => unreachable!(),
            }
        }
    }

    let names = ["left", "right"];
    let paths = ["nested/left", "nested/right"];
    let interface = Arc::new(
        Interface::new(
            [crate::PortDef::wire::<i64>("input").unwrap()],
            [crate::PortDef::wire::<i64>("output").unwrap()],
        )
        .unwrap(),
    );
    let mut nested = NetworkBuilder::new();
    for name in names {
        let child = nested
            .add(
                name,
                Arc::new(ComponentDef::primitive(
                    interface.clone(),
                    Arc::new(WakeEcho::default),
                )),
            )
            .unwrap();
        nested
            .expose_input(name, child.wire_input::<i64>("input").unwrap(), one())
            .unwrap();
        nested
            .expose_output(name, child.wire_output::<i64>("output").unwrap())
            .unwrap();
    }
    let mut root = NetworkBuilder::new();
    let child = root.module("nested", nested.build().unwrap()).unwrap();
    for name in names {
        root.expose_input(name, child.wire_input::<i64>(name).unwrap(), one())
            .unwrap();
        root.expose_output(name, child.wire_output::<i64>(name).unwrap())
            .unwrap();
    }
    let before = root.build().unwrap();
    let baseline = before.compile().unwrap();
    let mut after = before.clone();
    after
        .instances
        .insert("a-orphan".into(), Arc::new(ComponentDef::map(|n: i64| n)));
    let inserted = baseline
        .reuse_after_isolated_root_addition(&before, &after)
        .expect("the addressed nested fixture must qualify for Q2 insertion");
    let removed = inserted
        .reuse_after_isolated_root_removal(&after, &before)
        .expect("the addressed nested fixture must qualify for Q2 removal");

    for (compiled, offset) in [(&inserted, 1), (&removed, 0)] {
        let indices = paths.map(|path| compiled.physical_process_index(path).unwrap());
        assert_eq!(indices, [offset, offset + 1]);
        for policy in [
            RequestQueuePolicy::SharedFifo,
            RequestQueuePolicy::BoundedLanes,
        ] {
            let gates = [Arc::new(Gate::default()), Arc::new(Gate::default())];
            let mut session = compiled.start_with_options(SessionOptions {
                request_queue_policy: policy,
                max_requests: NonZeroUsize::new(2).unwrap(),
                readiness: paths
                    .iter()
                    .zip(&gates)
                    .map(|(path, gate)| ((*path).into(), Readiness::new(gate.clone())))
                    .collect(),
                ..Default::default()
            });
            assert!(!session.run_until_idle(16).budget_exhausted);
            let requests = names.map(|_| session.begin_request(None).unwrap());
            let values = [11_i64, 22];
            for target in 0..2 {
                session
                    .inject_request(requests[target], names[target], Value::of(values[target]))
                    .unwrap();
            }
            assert_eq!(session.run_until_idle(16).steps, 0);
            let calls = gates
                .each_ref()
                .map(|gate| gate.calls.load(Ordering::SeqCst));
            assert!(calls.iter().all(|count| *count > 0));
            for gate in &gates {
                gate.allowed.store(true, Ordering::SeqCst);
            }
            // No sealed-scope watchers or generic notifications may hide a bad address.
            assert!(session.next_job().is_none());
            assert_eq!(
                gates
                    .each_ref()
                    .map(|gate| gate.calls.load(Ordering::SeqCst)),
                calls
            );
            for target in 0..2 {
                let calls = gates
                    .each_ref()
                    .map(|gate| gate.calls.load(Ordering::SeqCst));
                assert!(session.process_notifier(paths[target]).unwrap().notify());
                let (index, job) = session.next_job().expect("only the notified gate admits");
                assert_eq!(index, indices[target]);
                let mut expected = calls;
                expected[target] += 1;
                assert_eq!(
                    gates
                        .each_ref()
                        .map(|gate| gate.calls.load(Ordering::SeqCst)),
                    expected
                );
                session.complete_job(job.run()).unwrap();
                assert!(session.next_job().is_none());
                assert_eq!(
                    session.request_work(requests[target]).unwrap().waiting_wake,
                    1
                );
            }
            // Readiness invalidation must not wake either suspended continuation.
            assert!(session.process_notifier(paths[0]).unwrap().notify());
            assert!(session.next_job().is_none());
            for request in requests {
                assert_eq!(session.request_status(request), Ok(RequestStatus::Open));
                assert_eq!(session.request_work(request).unwrap().waiting_wake, 1);
            }
            for target in [1, 0] {
                assert!(session.wake_handle(paths[target]).unwrap().wake());
                let (index, job) = session.next_job().expect("the addressed waiter resumes");
                assert_eq!(index, indices[target]);
                session.complete_job(job.run()).unwrap();
                let (index, job) = session.next_job().expect("the same activation completes");
                assert_eq!(index, indices[target]);
                session.complete_job(job.run()).unwrap();
                assert!(session.next_job().is_none());
                let packet = session
                    .take_request_output(requests[target], names[target])
                    .unwrap()
                    .unwrap();
                assert_eq!(*packet.value().decoded::<i64>().unwrap(), values[target]);
                let metadata = packet.metadata().unwrap();
                assert_eq!(metadata.process(), Some(indices[target]));
                assert_eq!(metadata.request(), Some(requests[target]));
                assert_eq!(metadata.capture(), Some(names[target]));
                assert_eq!(metadata.output(), "output");
                assert_eq!(metadata.session(), requests[target].session());
                assert_eq!(metadata.generation(), requests[target].generation());
                session
                    .validate_capture(requests[target], names[target], &packet)
                    .unwrap();
                assert_eq!(
                    session.validate_capture(requests[1 - target], names[target], &packet),
                    Err(RequestError::InvalidCapture)
                );
                if target == 1 {
                    assert_eq!(session.request_work(requests[0]).unwrap().waiting_wake, 1);
                    assert!(
                        session
                            .take_request_output(requests[0], names[0])
                            .unwrap()
                            .is_none()
                    );
                }
            }
            for request in requests {
                session.close_request(request).unwrap();
                assert_eq!(
                    session.request_status(request),
                    Ok(RequestStatus::Settled { captures: 1 })
                );
                session.release_request(request).unwrap();
            }
            assert!(session.failure().is_none(), "{:?}", session.failure());
        }
    }
}
