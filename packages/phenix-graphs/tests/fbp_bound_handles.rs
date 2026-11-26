//! Public, derive-free coverage of owned declaration and execution capabilities.
use phenix_graph::{
    CompiledNetwork, ComponentDef, ContractValue, Edit, Failure, InputKey, Interface,
    NetworkBuilder, NetworkDef, NetworkError, OutputKey, PortDef, Process, ProcessContext,
    ProcessEmission, Schema, SessionOptions, Step, Value,
    connection::ConnectionError,
    runtime::{
        Readiness, RequestError, RequestFailurePolicy, RequestQueuePolicy, RequestStatus,
        SessionState,
    },
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

struct Echo(bool);
impl Process for Echo {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, Failure> {
        if self.0 {
            self.0 = false;
            return Ok(Step::Complete);
        }
        let Some(value) = cx.take("in") else {
            return Ok(Step::Finished);
        };
        self.0 = true;
        Ok(Step::Emit(ProcessEmission::new("out", value)))
    }
}
fn definition<T: 'static>(key: InputKey<T>) -> NetworkDef {
    let mut input = key.definition().unwrap();
    input.name = "in".into();
    let mut output = input.clone();
    output.name = "out".into();
    let mut b = NetworkBuilder::new();
    let node = b
        .node(
            "echo",
            Arc::new(Interface::new([input], [output]).unwrap()),
            Arc::new(|| Echo(false)),
        )
        .unwrap();
    b.expose_input_dynamic("input", &node.input_endpoint("in").unwrap(), one())
        .unwrap();
    for name in ["result", "alias"] {
        b.expose_output_dynamic(name, &node.output_endpoint("out").unwrap())
            .unwrap();
    }
    b.build().unwrap()
}
fn options(queue: RequestQueuePolicy, failure: RequestFailurePolicy) -> SessionOptions {
    SessionOptions {
        request_queue_policy: queue,
        request_failure_policy: failure,
        ..Default::default()
    }
}
fn policies() -> impl Iterator<Item = (RequestQueuePolicy, RequestFailurePolicy)> {
    [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ]
    .into_iter()
    .flat_map(|q| {
        [
            RequestFailurePolicy::FailSession,
            RequestFailurePolicy::RestartAffectedProcess,
        ]
        .into_iter()
        .map(move |f| (q, f))
    })
}

struct MoveOnly(String);

#[test]
fn repeated_raw_retention_deduplicates_issuing_leases_without_encoding_or_validation() {
    let execution = Arc::new(());
    let codec = Arc::new(());
    let compiled = definition(InputKey::<Wire<0>>::encoded("input"))
        .compile()
        .unwrap()
        .with_lease(execution.clone());
    let ingress = compiled
        .bind_ingress("input", InputKey::<Wire<0>>::encoded("in"))
        .unwrap()
        .with_lease(codec.clone())
        .with_lease(codec.clone());
    let before = (Arc::strong_count(&execution), Arc::strong_count(&codec));
    let encodes = Arc::new(AtomicUsize::new(0));
    let raw = InputKey::<Wire<0>>::encoded("in").value(Wire(17, encodes.clone()));
    let identity = raw.identity();
    let bytes = raw.bytes().to_vec();
    let mut value = ingress.retain_value(raw);
    let retained = (before.0 + 1, before.1 + 1);
    for _ in 0..100 {
        value = ingress.clone().retain_value(value);
        assert_eq!(
            (Arc::strong_count(&execution), Arc::strong_count(&codec)),
            retained
        );
        assert_eq!(value.identity(), identity);
        assert_eq!(value.bytes(), bytes);
        assert_eq!(encodes.load(Ordering::SeqCst), 1);
    }
    drop(value);
    assert_eq!(
        (Arc::strong_count(&execution), Arc::strong_count(&codec)),
        before
    );

    // Retention is not admission: incompatible raw values are retained unchanged.
    let mismatch = ingress.retain_value(Value::local(MoveOnly("raw".into())));
    assert_eq!(mismatch.borrow::<MoveOnly>().unwrap().0, "raw");
    drop(mismatch);
    assert_eq!(
        (Arc::strong_count(&execution), Arc::strong_count(&codec)),
        before
    );
}

#[test]
fn owned_move_only_endpoints_and_handles_accept_clones_not_recompilation() {
    let def = definition(InputKey::<MoveOnly>::new("input"));
    let compiled = def.compile().unwrap();
    let other = def.compile().unwrap();
    let input = compiled
        .declaration("echo")
        .unwrap()
        .input_key(InputKey::<MoveOnly>::new("in"))
        .unwrap();
    let output = compiled
        .declaration("echo")
        .unwrap()
        .output_key(OutputKey::<MoveOnly>::new("out"))
        .unwrap();
    let ingress = compiled
        .bind_ingress("input", InputKey::<MoveOnly>::new("selector"))
        .unwrap();
    let capture = compiled
        .bind_capture("result", OutputKey::<MoveOnly>::new("selector"))
        .unwrap();
    assert!(ingress.matches_input(&input.clone()).unwrap());
    assert!(capture.matches_output(&output.clone()).unwrap());
    for owner in [
        &compiled,
        &compiled.clone(),
        &compiled.with_lease(Arc::new(())),
    ] {
        ingress.validate(owner).unwrap();
        capture.validate(owner).unwrap();
        input.validate(owner).unwrap();
        output.validate(owner).unwrap();
    }
    assert_eq!(ingress.validate(&other), Err(NetworkError::ForeignScope));
    assert_eq!(
        input.resolve(&other).unwrap_err(),
        NetworkError::ForeignScope
    );
    assert_eq!(
        capture.matches_output(
            &other
                .declaration("echo")
                .unwrap()
                .output::<MoveOnly>("out")
                .unwrap()
        ),
        Err(NetworkError::ForeignScope)
    );
    let clone = compiled.clone();
    drop(compiled);
    let mut session = clone.start();
    session
        .inject_bound(&ingress.clone(), MoveOnly("owned".into()))
        .unwrap();
    session.run_until_idle(32);
    let packet = session.take_bound_output(&capture).unwrap().unwrap();
    assert_eq!(packet.value().borrow::<MoveOnly>().unwrap().0, "owned");
    assert!(!session.bound_output_closed(&capture).unwrap());
    session.close_bound_input(&ingress).unwrap();
    session.run_until_idle(32);
    assert!(session.bound_output_closed(&capture).unwrap());
    assert_eq!(
        session
            .inject_bound(&ingress, MoveOnly("closed".into()))
            .unwrap_err()
            .reason,
        ConnectionError::ReaderClosed
    );
}

#[test]
fn composite_root_occurrences_and_complete_fanout_are_not_guessed() {
    let leaf = Arc::new(ComponentDef::primitive(
        Arc::new(
            Interface::new(
                [PortDef::local::<i64>("in")],
                [PortDef::local::<i64>("out")],
            )
            .unwrap(),
        ),
        Arc::new(|| Echo(false)),
    ));
    let mut inner = NetworkBuilder::new();
    let a = inner.add("a", leaf.clone()).unwrap();
    let b = inner.add("b", leaf.clone()).unwrap();
    for node in [&a, &b] {
        inner
            .expose_input("in", node.input::<i64>("in").unwrap(), one())
            .unwrap();
    }
    inner
        .expose_output("out", a.output::<i64>("out").unwrap())
        .unwrap();
    let composite = Arc::new(ComponentDef::composite(inner.build().unwrap()));
    let mut outer = NetworkBuilder::new();
    let first = outer.add("first", composite.clone()).unwrap();
    outer.add("second", composite.clone()).unwrap();
    outer
        .expose_input("all", first.input::<i64>("in").unwrap(), one())
        .unwrap();
    let compiled = outer.build().unwrap().compile().unwrap();
    assert_eq!(
        compiled.bind_definition(leaf.definition_key()).unwrap_err(),
        NetworkError::AmbiguousDefinition
    );
    assert_eq!(
        compiled
            .bind_definition(composite.definition_key())
            .unwrap_err(),
        NetworkError::AmbiguousDefinition
    );
    let declarations = compiled.declarations(composite.definition_key());
    assert_eq!(
        declarations.iter().map(|d| d.path()).collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert_eq!(compiled.declarations(leaf.definition_key()).len(), 4);
    assert_eq!(compiled.declaration("").unwrap().definition_key(), None);
    let ingress = compiled
        .bind_ingress("all", InputKey::<i64>::new("in"))
        .unwrap();
    assert!(
        ingress
            .matches_input(&declarations[0].input::<i64>("in").unwrap())
            .unwrap()
    );
    assert!(
        ingress
            .matches_input(
                &compiled
                    .declaration("")
                    .unwrap()
                    .input::<i64>("all")
                    .unwrap()
            )
            .unwrap()
    );
    assert!(
        !ingress
            .matches_input(
                &compiled
                    .declaration("first/a")
                    .unwrap()
                    .input::<i64>("in")
                    .unwrap()
            )
            .unwrap()
    );
    assert!(
        !ingress
            .matches_input(&declarations[1].input::<i64>("in").unwrap())
            .unwrap()
    );
    assert!(
        compiled
            .bind_ingress("first/a/in", InputKey::<i64>::new("in"))
            .is_err()
    );
    assert!(compiled.capture("first/a/out").is_err());
    assert!(compiled.declaration("missing").is_err());
    let unique = definition(InputKey::<i64>::new("input")).compile().unwrap();
    let key = unique.inspect("echo").unwrap().definition_key.unwrap();
    assert_eq!(unique.bind_definition(key).unwrap().path(), "echo");
    assert_eq!(
        compiled.bind_definition(key).unwrap_err(),
        NetworkError::UnknownDefinition
    );
    let replacement = definition(InputKey::<i64>::new("input")).compile().unwrap();
    assert_eq!(
        replacement.bind_definition(key).unwrap_err(),
        NetworkError::UnknownDefinition
    );
    // Detached inspection cannot mutate final membership or endpoint type.
    let mut detached = unique.inspect("echo").unwrap();
    detached.interface = Arc::new(Interface::default());
    assert!(
        unique
            .declaration("echo")
            .unwrap()
            .input::<i64>("in")
            .is_ok()
    );
}

struct Wire<const N: u8>(i64, Arc<AtomicUsize>);
impl<const N: u8> ContractValue for Wire<N> {
    fn schema() -> Schema {
        Schema {
            name: if N == 1 { "other" } else { "bound/wire" }.into(),
            revision: if N == 2 { 2 } else { 1 },
            structure: if N == 3 {
                "record{other:i64}"
            } else {
                "record{value:i64}"
            }
            .into(),
            codec: if N == 4 { "other" } else { "i64-le" }.into(),
        }
    }
    fn encode(&self) -> Vec<u8> {
        self.1.fetch_add(1, Ordering::SeqCst);
        self.0.to_le_bytes().to_vec()
    }
    fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        Ok(Self(
            i64::from_le_bytes(
                bytes
                    .try_into()
                    .map_err(|e| Failure::external("bad bytes", e))?,
            ),
            Arc::default(),
        ))
    }
}
fn wrong_schema<const N: u8>(compiled: &CompiledNetwork) {
    assert!(
        compiled
            .bind_ingress("input", InputKey::<Wire<N>>::encoded("in"))
            .is_err()
    );
    assert!(
        compiled
            .bind_capture("result", OutputKey::<Wire<N>>::encoded("out"))
            .is_err()
    );
}
#[test]
fn full_schema_mode_direction_and_raw_retry_without_reencoding() {
    let compiled = definition(InputKey::<Wire<0>>::encoded("input"))
        .compile()
        .unwrap();
    wrong_schema::<1>(&compiled);
    wrong_schema::<2>(&compiled);
    wrong_schema::<3>(&compiled);
    wrong_schema::<4>(&compiled);
    assert!(
        compiled
            .bind_ingress("input", InputKey::<Wire<0>>::new("in"))
            .is_err()
    );
    assert!(
        compiled
            .bind_capture("result", OutputKey::<Wire<0>>::new("out"))
            .is_err()
    );
    assert!(
        compiled
            .declaration("echo")
            .unwrap()
            .wire_input::<Wire<0>>("out")
            .is_err()
    );
    let ingress = compiled
        .bind_ingress("input", InputKey::<Wire<0>>::encoded("in"))
        .unwrap();
    let capture = compiled.capture("result").unwrap();
    for (q, f) in policies() {
        let mut session = compiled.start_with_options(options(q, f));
        let count = Arc::new(AtomicUsize::new(0));
        for n in [1, 2] {
            session
                .inject_bound(&ingress, Wire(n, count.clone()))
                .unwrap();
        }
        let error = session
            .inject_bound(&ingress, Wire(3, count.clone()))
            .unwrap_err();
        assert_eq!(error.reason, ConnectionError::Full);
        assert_eq!(error.value.bytes(), 3_i64.to_le_bytes());
        session.run_until_idle(32);
        session.inject_bound_value(&ingress, error.value).unwrap();
        session.run_until_idle(32);
        let mut values = vec![];
        while let Some(packet) = session.take_bound_output(&capture).unwrap() {
            values.push(Wire::<0>::decode(packet.value().bytes()).unwrap().0);
        }
        assert_eq!(values, [1, 2, 3]);
        assert_eq!(count.load(Ordering::SeqCst), 3);
        let error = session
            .inject_bound_value(&ingress, Value::local(99_i64))
            .unwrap_err();
        assert_eq!(error.reason, ConnectionError::WrongType);
        assert_eq!(error.value.borrow::<i64>(), Some(&99));
    }
}

#[test]
fn bound_requests_keep_session_ids_aliases_exact_counts_and_lane_staleness() {
    let compiled = definition(InputKey::<i64>::new("input")).compile().unwrap();
    let ingress = compiled
        .bind_ingress("input", InputKey::<i64>::new("in"))
        .unwrap();
    let result = compiled.capture("result").unwrap();
    let alias = compiled.capture("alias").unwrap();
    for (q, f) in policies() {
        let mut session = compiled.start_with_options(options(q, f));
        let mut other = compiled.start_with_options(options(q, f));
        let foreign = other.begin_bound_request(Some(&result)).unwrap();
        assert_eq!(
            session
                .inject_bound_request(foreign, &ingress, 9)
                .unwrap_err()
                .reason,
            RequestError::Foreign
        );
        let request = session.begin_bound_request(Some(&result)).unwrap();
        session.inject_bound_request(request, &ingress, 7).unwrap();
        session.close_request(request).unwrap();
        assert_eq!(
            session
                .inject_bound_request(request, &ingress, 8)
                .unwrap_err()
                .reason,
            RequestError::Sealed
        );
        session.run_until_idle(32);
        let packet = session
            .take_bound_request_output(request, &result)
            .unwrap()
            .unwrap();
        session
            .validate_bound_capture(request, &result, &packet)
            .unwrap();
        assert_eq!(
            session.validate_bound_capture(request, &alias, &packet),
            Err(RequestError::InvalidCapture)
        );
        assert_eq!(
            other.validate_bound_capture(foreign, &result, &packet),
            Err(RequestError::InvalidCapture)
        );
        // Exact requests cannot consume the other alias through the scoped path.
        assert_eq!(
            session
                .take_bound_request_output(request, &alias)
                .unwrap_err(),
            RequestError::InvalidCapture
        );
        // Global all-output draining retains canonical accounting (other alias invalidates exactness).
        if q == RequestQueuePolicy::SharedFifo {
            assert!(session.take_bound_output(&alias).unwrap().is_some());
            assert_eq!(
                session.request_status(request),
                Err(RequestError::InvalidCapture)
            );
        } else {
            // Lane queues are scoped, exactly as with canonical take_output.
            assert!(session.take_bound_output(&alias).unwrap().is_none());
            session.cancel_request(request).unwrap();
        }
        session.release_request(request).unwrap();
        let next = session.begin_bound_request(None).unwrap();
        assert_ne!(next, request);
        assert_eq!(
            session
                .take_bound_request_output(request, &result)
                .unwrap_err(),
            RequestError::Stale
        );
        assert_eq!(
            session
                .inject_bound_request_value(request, &ingress, Value::local(1_i64))
                .unwrap_err()
                .reason,
            RequestError::Stale
        );
        assert_eq!(
            session.validate_bound_capture(request, &result, &packet),
            Err(RequestError::Stale)
        );
        session.close_bound_input(&ingress).unwrap();
        assert_eq!(
            session
                .inject_bound_request(next, &ingress, 10)
                .unwrap_err()
                .reason,
            RequestError::Transport(ConnectionError::WriterClosed)
        );
    }
}

#[test]
fn scoped_reads_skip_unrelated_heads_and_exact_single_capture_settles() {
    let mut def = definition(InputKey::<i64>::new("input"));
    // Use a single export by authoring, not by mutating compiled diagnostics.
    let mut builder = NetworkBuilder::new();
    let module = builder.module("module", def.clone()).unwrap();
    builder
        .expose_input("input", module.input::<i64>("input").unwrap(), one())
        .unwrap();
    builder
        .expose_output("result", module.output::<i64>("result").unwrap())
        .unwrap();
    def = builder.build().unwrap();
    let compiled = def.compile().unwrap();
    let input = compiled
        .bind_ingress("input", InputKey::<i64>::new("input"))
        .unwrap();
    let capture = compiled.capture("result").unwrap();
    for (q, f) in policies() {
        let mut session = compiled.start_with_options(options(q, f));
        let a = session.begin_bound_request(Some(&capture)).unwrap();
        let b = session.begin_bound_request(Some(&capture)).unwrap();
        session.inject_bound_request(a, &input, 1).unwrap();
        session.close_request(a).unwrap();
        session.run_until_idle(32);
        session.inject_bound_request(b, &input, 2).unwrap();
        session.close_request(b).unwrap();
        session.run_until_idle(32);
        let second = session
            .take_bound_request_output(b, &capture)
            .unwrap()
            .unwrap();
        assert_eq!(second.value().borrow::<i64>(), Some(&2));
        assert_eq!(
            session.request_status(b).unwrap(),
            RequestStatus::Settled { captures: 1 }
        );
        let first = if q == RequestQueuePolicy::SharedFifo {
            session.take_bound_output(&capture).unwrap().unwrap()
        } else {
            session
                .take_bound_request_output(a, &capture)
                .unwrap()
                .unwrap()
        };
        session.validate_bound_capture(a, &capture, &first).unwrap();
        assert_eq!(
            session.request_status(a).unwrap(),
            RequestStatus::Settled { captures: 1 }
        );
        session.release_request(a).unwrap();
        session.release_request(b).unwrap();
    }
}

struct Source(bool);
impl Process for Source {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
        if self.0 {
            self.0 = false;
            Ok(Step::Complete)
        } else {
            self.0 = true;
            Ok(Step::Emit(ProcessEmission::new(
                "out",
                Value::local(42_i64),
            )))
        }
    }
}
fn sources() -> NetworkDef {
    let mut b = NetworkBuilder::new();
    let interface = Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap());
    let designated = b
        .node("designated", interface.clone(), Arc::new(|| Source(false)))
        .unwrap();
    b.node("not_designated", interface, Arc::new(|| Source(false)))
        .unwrap();
    b.start(&designated).unwrap();
    b.expose_output("out", designated.output::<i64>("out").unwrap())
        .unwrap();
    let mut outer = NetworkBuilder::new();
    let nested = outer.module("nested", b.build().unwrap()).unwrap();
    outer.start(&nested).unwrap();
    outer
        .expose_output("out", nested.output::<i64>("out").unwrap())
        .unwrap();
    outer.build().unwrap()
}
#[test]
fn only_designated_physical_sources_start_and_composites_enumerate_them() {
    let def = sources();
    let compiled = def.compile().unwrap();
    let declaration = compiled.declaration("nested/designated").unwrap();
    let composite_key = compiled.inspect("nested").unwrap().definition_key.unwrap();
    assert_eq!(
        compiled.bind_definition(composite_key).unwrap().path(),
        "nested"
    );
    let source = compiled.source(&declaration).unwrap();
    for path in ["", "nested"] {
        let declaration = compiled.declaration(path).unwrap();
        assert_eq!(
            declaration
                .sources()
                .iter()
                .map(|s| s.path())
                .collect::<Vec<_>>(),
            ["nested/designated"]
        );
        assert!(compiled.source(&declaration).is_err());
    }
    assert!(
        compiled
            .source(&compiled.declaration("nested/not_designated").unwrap())
            .is_err()
    );
    assert_eq!(
        def.compile().unwrap().source(&declaration).unwrap_err(),
        NetworkError::ForeignScope
    );
    let capture = compiled.capture("out").unwrap();
    for (q, f) in policies() {
        let mut opts = options(q, f);
        opts.auto_start_sources = false;
        let mut session = compiled.start_with_options(opts);
        let id = session.begin_bound_request(Some(&capture)).unwrap();
        session
            .start_bound_request_source(id, &source.clone())
            .unwrap();
        assert_eq!(
            session.start_bound_request_source(id, &source),
            Err(RequestError::Busy)
        );
        session.close_request(id).unwrap();
        assert_eq!(
            session.start_bound_request_source(id, &source),
            Err(RequestError::Sealed)
        );
        session.run_until_idle(32);
        let packet = session
            .take_bound_request_output(id, &capture)
            .unwrap()
            .unwrap();
        assert_eq!(packet.value().borrow::<i64>(), Some(&42));
        session
            .validate_bound_capture(id, &capture, &packet)
            .unwrap();
        assert_eq!(
            session.request_status(id).unwrap(),
            RequestStatus::Settled { captures: 1 }
        );
        session.release_request(id).unwrap();
        assert_eq!(
            session.start_bound_request_source(id, &source),
            Err(RequestError::Stale)
        );
    }
}

#[test]
fn foreign_and_stale_front_doors_never_supervise_overdue_jobs() {
    let def = definition(InputKey::<i64>::new("input"));
    let compiled = def.compile().unwrap();
    let foreign = def.compile().unwrap();
    let ingress = compiled
        .bind_ingress("input", InputKey::<i64>::new("in"))
        .unwrap();
    let wrong_input = foreign
        .bind_ingress("input", InputKey::<i64>::new("in"))
        .unwrap();
    let capture = compiled.capture("result").unwrap();
    let wrong_capture = foreign.capture("result").unwrap();
    let foreign_source = sources()
        .compile()
        .unwrap()
        .declaration("")
        .unwrap()
        .sources()
        .remove(0);
    for (q, f) in policies() {
        let mut opts = options(q, f);
        opts.deferred_timeout = Some(Duration::from_millis(20));
        let mut session = compiled.start_with_options(opts);
        let stale = session.begin_bound_request(None).unwrap();
        session.close_request(stale).unwrap();
        session.release_request(stale).unwrap();
        let cancelled = session.begin_bound_request(None).unwrap();
        session.cancel_request(cancelled).unwrap();
        let sealed = session.begin_bound_request(None).unwrap();
        session.close_request(sealed).unwrap();
        let request = session.begin_bound_request(None).unwrap();
        session.inject_bound_request(request, &ingress, 1).unwrap();
        let (_, job) = session.next_job().unwrap();
        std::thread::sleep(Duration::from_millis(30));
        let now = Instant::now();
        let before = session.overdue_jobs(now);
        let before_work = session.request_work(request).unwrap();
        assert_eq!(before.len(), 1);
        assert_eq!(
            session.inject_bound(&wrong_input, 7).unwrap_err().reason,
            ConnectionError::ForeignTopology
        );
        assert_eq!(
            session.close_bound_input(&wrong_input),
            Err(ConnectionError::ForeignTopology)
        );
        assert_eq!(
            session.begin_bound_request(Some(&wrong_capture)),
            Err(RequestError::Foreign)
        );
        assert_eq!(
            session.take_bound_output(&wrong_capture).unwrap_err(),
            ConnectionError::ForeignTopology
        );
        assert_eq!(
            session.bound_output_closed(&wrong_capture),
            Err(ConnectionError::ForeignTopology)
        );
        assert_eq!(
            session
                .inject_bound_request(request, &wrong_input, 7)
                .unwrap_err()
                .reason,
            RequestError::Foreign
        );
        assert_eq!(
            session
                .inject_bound_request(stale, &ingress, 7)
                .unwrap_err()
                .reason,
            RequestError::Stale
        );
        assert_eq!(
            session
                .inject_bound_request(cancelled, &ingress, 7)
                .unwrap_err()
                .reason,
            RequestError::Stale
        );
        assert_eq!(
            session
                .inject_bound_request(sealed, &ingress, 7)
                .unwrap_err()
                .reason,
            RequestError::Sealed
        );
        assert_eq!(
            session
                .take_bound_request_output(cancelled, &capture)
                .unwrap_err(),
            RequestError::Stale
        );
        assert_eq!(
            session
                .take_bound_request_output(request, &wrong_capture)
                .unwrap_err(),
            RequestError::Foreign
        );
        assert_eq!(
            session
                .take_bound_request_output(stale, &capture)
                .unwrap_err(),
            RequestError::Stale
        );
        assert_eq!(
            session.start_bound_request_source(request, &foreign_source),
            Err(RequestError::Foreign)
        );
        assert_eq!(session.overdue_jobs(now), before);
        assert_eq!(session.request_work(request).unwrap(), before_work);
        assert_eq!(
            session.request_status(request).unwrap(),
            RequestStatus::Open
        );
        assert_eq!(session.state(), SessionState::Waiting);
        assert!(session.failure().is_none());
        // Owner refusal did not discard or clone the runner's job.
        drop(job);
    }
}

#[test]
fn checked_configuration_and_symbolic_composition_bridge_keep_owner_proofs() {
    let def = definition(InputKey::<i64>::new("input"));
    let compiled = def.compile().unwrap();
    let other = def.compile().unwrap();
    let declaration = compiled.declaration("echo").unwrap();
    let input = declaration.input::<i64>("in").unwrap();
    let output = declaration.output::<i64>("out").unwrap();
    assert!(matches!(
        Edit::connect_bound(&compiled, &output, &input, one()).unwrap(),
        Edit::Connect(_)
    ));
    assert!(
        Edit::expose_bound_input(&compiled, "extra", std::slice::from_ref(&input), one()).is_ok()
    );
    assert!(Edit::expose_bound_output(&compiled, "extra", &output).is_ok());
    assert!(matches!(
        Edit::connect_bound(&other, &output, &input, one()),
        Err(NetworkError::ForeignScope)
    ));
    assert!(matches!(
        Edit::expose_bound_input(&other, "extra", &[input], one()),
        Err(NetworkError::ForeignScope)
    ));
    let mut opts = SessionOptions::default();
    opts.bound.readiness(&declaration, Readiness::ALL);
    opts.bound
        .resources(&declaration, phenix_graph::ValidatedResourceSet::default());
    opts.bound.discard_output(&compiled, &output).unwrap();
    let session = compiled.start_with_options(opts.clone());
    assert!(session.failure().is_none());
    assert!(session.bound_wake_handle(&declaration).unwrap().is_some());
    assert!(
        session
            .bound_process_notifier(&declaration)
            .unwrap()
            .is_some()
    );
    assert!(
        session
            .bound_wake_handle(&compiled.declaration("").unwrap())
            .is_err()
    );
    assert!(
        session
            .bound_process_notifier(&other.declaration("echo").unwrap())
            .is_err()
    );
    let foreign = other.start_with_options(opts);
    assert_eq!(foreign.state(), SessionState::Failed);
    let mut opts = SessionOptions::default();
    opts.bound
        .readiness(&compiled.declaration("").unwrap(), Readiness::ALL);
    assert_eq!(
        compiled.start_with_options(opts).state(),
        SessionState::Failed
    );
    let mut opts = SessionOptions::default();
    opts.bound.readiness(
        &declaration,
        Readiness::at_least(NonZeroUsize::new(2).unwrap()),
    );
    assert_eq!(
        compiled.start_with_options(opts).state(),
        SessionState::Failed
    );
}

struct Hostile {
    lease: Weak<()>,
    drops: Arc<AtomicUsize>,
}
impl Drop for Hostile {
    fn drop(&mut self) {
        assert!(
            self.lease.upgrade().is_some(),
            "payload lost its code lease"
        );
        self.drops.fetch_add(1, Ordering::SeqCst);
        panic!("hostile payload drop");
    }
}
#[test]
fn detached_and_refused_values_pin_issuance_leases_nonretroactively() {
    let def = definition(InputKey::<Hostile>::new("input"));
    let base = def.compile().unwrap();
    let early = base
        .bind_ingress("input", InputKey::<Hostile>::new("in"))
        .unwrap();
    let lease = Arc::new(());
    let weak = Arc::downgrade(&lease);
    let later_lease = Arc::new(());
    let later_weak = Arc::downgrade(&later_lease);
    let compiled = base.with_lease(lease.clone());
    let ingress = compiled
        .bind_ingress("input", InputKey::<Hostile>::new("in"))
        .unwrap();
    let enriched = compiled.with_lease(later_lease.clone());
    drop((lease, later_lease, compiled, enriched, base));
    assert!(weak.upgrade().is_some());
    assert!(later_weak.upgrade().is_none());
    let drops = Arc::new(AtomicUsize::new(0));
    let value = ingress.value(Hostile {
        lease: weak.clone(),
        drops: drops.clone(),
    });
    let mut foreign = def.compile().unwrap().start();
    let error = foreign.inject_bound_value(&ingress, value).unwrap_err();
    assert_eq!(error.reason, ConnectionError::ForeignTopology);
    drop((ingress, foreign));
    assert!(weak.upgrade().is_some());
    // Ordinary Value drop contains hostile payload unwinds, as before A5.
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(error))).is_ok());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(weak.upgrade().is_none());
    // The earlier handle held only its token, not leases added to a later clone.
    drop(early);
}

#[test]
fn every_raw_rejection_keeps_handle_and_explicit_codec_owners() {
    let def = definition(InputKey::<i64>::new("input"));
    for reason in [
        "foreign",
        "wrong_type",
        "full",
        "closed",
        "request_foreign",
        "request_stale",
        "request_sealed",
    ] {
        let lease = Arc::new(());
        let weak = Arc::downgrade(&lease);
        let codec = Arc::new(());
        let codec_weak = Arc::downgrade(&codec);
        let compiled = def.compile().unwrap().with_lease(lease.clone());
        let ingress = compiled
            .bind_ingress("input", InputKey::<i64>::new("in"))
            .unwrap()
            .with_lease(codec.clone());
        let mut session = if reason == "foreign" {
            def.compile().unwrap().start()
        } else {
            compiled.start()
        };
        let rejected = match reason {
            "foreign" => {
                session
                    .inject_bound_value(&ingress, Value::local(7_i64))
                    .unwrap_err()
                    .value
            }
            "wrong_type" => {
                session
                    .inject_bound_value(&ingress, Value::local(false))
                    .unwrap_err()
                    .value
            }
            "full" => {
                for n in [1, 2] {
                    session.inject_bound(&ingress, n).unwrap();
                }
                session
                    .inject_bound_value(&ingress, Value::local(7_i64))
                    .unwrap_err()
                    .value
            }
            "closed" => {
                session.close_bound_input(&ingress).unwrap();
                session
                    .inject_bound_value(&ingress, Value::local(7_i64))
                    .unwrap_err()
                    .value
            }
            _ => {
                let mut other = compiled.start();
                let id = if reason == "request_foreign" {
                    other.begin_bound_request(None).unwrap()
                } else {
                    session.begin_bound_request(None).unwrap()
                };
                if reason != "request_foreign" {
                    session.close_request(id).unwrap();
                }
                if reason == "request_stale" {
                    session.release_request(id).unwrap();
                }
                session
                    .inject_bound_request_value(id, &ingress, Value::local(7_i64))
                    .unwrap_err()
                    .value
            }
        };
        drop((lease, codec, compiled, ingress, session));
        assert!(weak.upgrade().is_some(), "{reason}");
        assert!(codec_weak.upgrade().is_some(), "{reason}");
        drop(rejected);
        assert!(weak.upgrade().is_none(), "{reason}");
        assert!(codec_weak.upgrade().is_none(), "{reason}");
    }
}

#[test]
fn request_full_retry_and_closure_before_later_lane_construction() {
    let compiled = definition(InputKey::<Wire<0>>::encoded("input"))
        .compile()
        .unwrap();
    let ingress = compiled
        .bind_ingress("input", InputKey::<Wire<0>>::encoded("in"))
        .unwrap();
    for (q, f) in policies() {
        let mut session = compiled.start_with_options(options(q, f));
        let request = session.begin_bound_request(None).unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        for n in [1, 2] {
            session
                .inject_bound_request(request, &ingress, Wire(n, count.clone()))
                .unwrap();
        }
        let error = session
            .inject_bound_request(request, &ingress, Wire(3, count.clone()))
            .unwrap_err();
        assert_eq!(error.reason, RequestError::Transport(ConnectionError::Full));
        session.run_until_idle(32);
        session
            .inject_bound_request_value(request, &ingress, error.value)
            .unwrap();
        session.close_request(request).unwrap();
        session.run_until_idle(32);
        let capture = compiled.capture("result").unwrap();
        let mut values = vec![];
        while let Some(packet) = session
            .take_bound_request_output(request, &capture)
            .unwrap()
        {
            values.push(Wire::<0>::decode(packet.value().bytes()).unwrap().0);
        }
        assert_eq!(values, [1, 2, 3]);
        assert_eq!(count.load(Ordering::SeqCst), 3);
        let later = session.begin_bound_request(None).unwrap();
        session.close_bound_input(&ingress).unwrap();
        let last = session.begin_bound_request(None).unwrap();
        for id in [later, last] {
            assert_eq!(
                session
                    .inject_bound_request(id, &ingress, Wire(4, count.clone()))
                    .unwrap_err()
                    .reason,
                RequestError::Transport(ConnectionError::WriterClosed)
            );
        }
    }
}

#[test]
fn foreign_configuration_is_rejected_before_factories_and_pure_validation_has_no_codec_callbacks() {
    let spawns = Arc::new(AtomicUsize::new(0));
    let mut builder = NetworkBuilder::new();
    let counter = spawns.clone();
    builder
        .node(
            "worker",
            Arc::new(Interface::default()),
            Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                Echo(false)
            }),
        )
        .unwrap();
    let def = builder.build().unwrap();
    let compiled = def.compile().unwrap();
    let other = def.compile().unwrap();
    let mut opts = SessionOptions::default();
    opts.bound
        .readiness(&compiled.declaration("worker").unwrap(), Readiness::ALL);
    let refused = other.start_with_options(opts);
    assert_eq!(refused.state(), SessionState::Failed);
    assert_eq!(spawns.load(Ordering::SeqCst), 0);

    let encodes = Arc::new(AtomicUsize::new(0));
    let compiled = definition(InputKey::<Wire<0>>::encoded("input"))
        .compile()
        .unwrap();
    let ingress = compiled
        .bind_ingress("input", InputKey::<Wire<0>>::encoded("in"))
        .unwrap();
    let output = compiled
        .declaration("echo")
        .unwrap()
        .wire_output::<Wire<0>>("out")
        .unwrap();
    let capture = compiled.capture("result").unwrap();
    for _ in 0..10 {
        ingress.validate(&compiled).unwrap();
        capture.validate(&compiled).unwrap();
        output.validate(&compiled).unwrap();
        assert!(capture.matches_output(&output).unwrap());
    }
    assert_eq!(encodes.load(Ordering::SeqCst), 0);
    let _ = ingress.value(Wire(1, encodes.clone()));
    assert_eq!(encodes.load(Ordering::SeqCst), 1);
}

#[derive(Debug)]
struct HostileCause(Weak<()>, Arc<AtomicUsize>);
impl std::fmt::Display for HostileCause {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        panic!("external formatting must stay passive")
    }
}
impl std::error::Error for HostileCause {}
impl Drop for HostileCause {
    fn drop(&mut self) {
        assert!(self.0.upgrade().is_some());
        self.1.fetch_add(1, Ordering::SeqCst);
        panic!("hostile cause drop");
    }
}
struct Failing;
impl Process for Failing {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, Failure> {
        Err(Failure::from_value(cx.input("in").unwrap()).unwrap())
    }
}
#[test]
fn bound_admission_failure_and_independently_escaped_cleanup_keep_known_leases() {
    for (q, f) in policies() {
        let lease = Arc::new(());
        let weak = Arc::downgrade(&lease);
        let drops = Arc::new(AtomicUsize::new(0));
        let mut builder = NetworkBuilder::new();
        let node = builder
            .node(
                "fail",
                Arc::new(Interface::new([PortDef::local::<Failure>("in")], []).unwrap()),
                Arc::new(|| Failing),
            )
            .unwrap();
        builder
            .expose_input("input", node.input::<Failure>("in").unwrap(), one())
            .unwrap();
        let compiled = builder.build().unwrap().compile().unwrap();
        let ingress = compiled
            .bind_ingress("input", InputKey::<Failure>::new("in"))
            .unwrap()
            .with_lease(lease.clone());
        let failure =
            Failure::external("primary", HostileCause(weak.clone(), drops.clone())).with_cleanup(
                Failure::external("secondary", HostileCause(weak.clone(), drops.clone())),
            );
        let mut session = compiled.start_with_options(options(q, f));
        let id = session.begin_bound_request(None).unwrap();
        session.inject_bound_request(id, &ingress, failure).unwrap();
        session.close_request(id).unwrap();
        session.run_until_idle(32);
        let failure = session.request_failure(id).unwrap().unwrap().clone();
        assert_eq!(failure.summary(), "primary");
        let cleanup = failure.cleanup()[0].clone();
        drop((lease, compiled, ingress, session, failure));
        assert!(weak.upgrade().is_some());
        assert!(cleanup.dispose_catching());
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn source_handoff_keeps_issuance_and_extra_leases_after_handle_and_session_drop() {
    let base = sources().compile().unwrap();
    let lease = Arc::new(());
    let weak = Arc::downgrade(&lease);
    let extra = Arc::new(());
    let extra_weak = Arc::downgrade(&extra);
    let compiled = base.with_lease(lease.clone());
    let source = compiled
        .declaration("")
        .unwrap()
        .sources()
        .remove(0)
        .with_lease(extra.clone());
    // Owner identity is preserved by with_lease; an older clone is not enriched.
    let mut session = base.start_with_options(SessionOptions {
        auto_start_sources: false,
        ..Default::default()
    });
    drop((base, compiled, lease, extra));
    let id = session.begin_bound_request(None).unwrap();
    session.start_bound_request_source(id, &source).unwrap();
    let (_, job) = session.next_job().unwrap();
    drop((source, session));
    assert!(weak.upgrade().is_some());
    assert!(extra_weak.upgrade().is_some());
    let completion = job.run();
    assert!(weak.upgrade().is_some());
    assert!(extra_weak.upgrade().is_some());
    drop(completion);
    assert!(weak.upgrade().is_none());
    assert!(extra_weak.upgrade().is_none());
}

#[test]
fn pure_authentication_uses_cached_identity_not_schema_providers() {
    static SCHEMAS: AtomicUsize = AtomicUsize::new(0);
    struct Counted;
    impl ContractValue for Counted {
        fn schema() -> Schema {
            SCHEMAS.fetch_add(1, Ordering::SeqCst);
            Wire::<0>::schema()
        }
        fn encode(&self) -> Vec<u8> {
            vec![]
        }
        fn decode(_: &[u8]) -> Result<Self, Failure> {
            Ok(Self)
        }
    }
    let compiled = definition(InputKey::<Counted>::encoded("input"))
        .compile()
        .unwrap();
    let declaration = compiled.declaration("echo").unwrap();
    let input = declaration.wire_input::<Counted>("in").unwrap();
    let output = declaration.wire_output::<Counted>("out").unwrap();
    let ingress = compiled
        .bind_ingress("input", InputKey::<Counted>::encoded("in"))
        .unwrap();
    let capture = compiled
        .bind_capture("result", OutputKey::<Counted>::encoded("out"))
        .unwrap();
    let before = SCHEMAS.load(Ordering::SeqCst);
    for _ in 0..10 {
        declaration.validate(&compiled).unwrap();
        input.validate(&compiled).unwrap();
        output.validate(&compiled).unwrap();
        ingress.validate(&compiled).unwrap();
        capture.validate(&compiled).unwrap();
        assert!(ingress.matches_input(&input).unwrap());
        assert!(capture.matches_output(&output).unwrap());
    }
    assert_eq!(SCHEMAS.load(Ordering::SeqCst), before);
}

#[test]
fn captured_values_keep_the_capture_handles_explicit_owner() {
    let compiled = definition(InputKey::<i64>::new("input")).compile().unwrap();
    let lease = Arc::new(());
    let weak = Arc::downgrade(&lease);
    let capture = compiled
        .capture("result")
        .unwrap()
        .with_lease(lease.clone());
    let ingress = compiled
        .bind_ingress("input", InputKey::<i64>::new("in"))
        .unwrap();
    let mut session = compiled.start();
    session.inject_bound(&ingress, 7).unwrap();
    session.run_until_idle(32);
    let packet = session.take_bound_output(&capture).unwrap().unwrap();
    let value = packet.value().clone();
    drop((lease, compiled, ingress, capture, session, packet));
    assert!(weak.upgrade().is_some());
    assert_eq!(value.borrow::<i64>(), Some(&7));
    drop(value);
    assert!(weak.upgrade().is_none());
}

#[test]
fn bound_fanout_full_is_atomic_for_slow_branch_and_raw_retry() {
    let mut builder = NetworkBuilder::new();
    let fast = builder
        .node(
            "fast",
            Arc::new(
                Interface::new(
                    [PortDef::local::<i64>("in")],
                    [PortDef::local::<i64>("out")],
                )
                .unwrap(),
            ),
            Arc::new(|| Echo(false)),
        )
        .unwrap();
    let slow = builder
        .node(
            "slow",
            Arc::new(
                Interface::new(
                    [PortDef::local::<i64>("in"), PortDef::local::<i64>("gate")],
                    [PortDef::local::<i64>("out")],
                )
                .unwrap(),
            ),
            Arc::new(|| Echo(false)),
        )
        .unwrap();
    for node in [&fast, &slow] {
        builder
            .expose_input("fanout", node.input::<i64>("in").unwrap(), one())
            .unwrap();
    }
    builder
        .expose_input("gate", slow.input::<i64>("gate").unwrap(), one())
        .unwrap();
    builder
        .expose_output("fast", fast.output::<i64>("out").unwrap())
        .unwrap();
    builder
        .expose_output("slow", slow.output::<i64>("out").unwrap())
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    let fanout = compiled
        .bind_ingress("fanout", InputKey::<i64>::new("in"))
        .unwrap();
    let gate = compiled
        .bind_ingress("gate", InputKey::<i64>::new("gate"))
        .unwrap();
    let fast = compiled.capture("fast").unwrap();
    let slow = compiled.capture("slow").unwrap();
    for (q, f) in policies() {
        let mut opts = options(q, f);
        opts.bound
            .readiness(&compiled.declaration("slow").unwrap(), Readiness::ALL);
        let mut session = compiled.start_with_options(opts);
        let id = session.begin_bound_request(None).unwrap();
        session.inject_bound_request(id, &fanout, 1).unwrap();
        session.run_until_idle(32);
        assert_eq!(
            session
                .take_bound_request_output(id, &fast)
                .unwrap()
                .unwrap()
                .value()
                .borrow::<i64>(),
            Some(&1)
        );
        let rejected = session.inject_bound_request(id, &fanout, 2).unwrap_err();
        assert_eq!(
            rejected.reason,
            RequestError::Transport(ConnectionError::Full)
        );
        session.run_until_idle(32);
        assert!(
            session
                .take_bound_request_output(id, &fast)
                .unwrap()
                .is_none()
        );
        session.inject_bound_request(id, &gate, 0).unwrap();
        session.run_until_idle(32);
        assert_eq!(
            session
                .take_bound_request_output(id, &slow)
                .unwrap()
                .unwrap()
                .value()
                .borrow::<i64>(),
            Some(&1)
        );
        session
            .inject_bound_request_value(id, &fanout, rejected.value)
            .unwrap();
        session.inject_bound_request(id, &gate, 0).unwrap();
        session.close_request(id).unwrap();
        session.run_until_idle(32);
        for capture in [&fast, &slow] {
            assert_eq!(
                session
                    .take_bound_request_output(id, capture)
                    .unwrap()
                    .unwrap()
                    .value()
                    .borrow::<i64>(),
                Some(&2)
            );
            assert!(
                session
                    .take_bound_request_output(id, capture)
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(
            session.request_status(id).unwrap(),
            RequestStatus::Settled { captures: 4 }
        );
        session.release_request(id).unwrap();
    }
}
