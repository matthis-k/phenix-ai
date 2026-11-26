//! Execution acceptance for canonical, key-based and generated authoring.
use phenix_graph::{
    ComponentDef, InputKey, Interface, OutputKey, PortDef, Value,
    contract::Contract,
    network::{
        CompiledNetwork, ComponentRef, InputRef, NetworkBuilder, NetworkDef, NetworkError,
        OutputRef,
    },
    process::{Process, ProcessContext, ProcessEmission, ProcessFactory, Step},
    runtime::{Readiness, SessionOptions, SessionState},
};
use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Debug)]
enum Authoring {
    Manual,
    Keys,
    #[cfg(feature = "derive")]
    Derived,
}

fn authors() -> Vec<Authoring> {
    vec![
        Authoring::Manual,
        Authoring::Keys,
        #[cfg(feature = "derive")]
        Authoring::Derived,
    ]
}

fn cap(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}

#[cfg(feature = "derive")]
#[allow(dead_code)]
mod declarations {
    use phenix_graph::Ports;

    #[derive(Ports)]
    pub struct Local {
        #[input(name = "in")]
        pub input: i64,
        #[output(name = "out")]
        pub output: i64,
    }
    #[derive(Ports)]
    pub struct Wire {
        #[input(encoded, name = "in")]
        pub input: i64,
        #[output(encoded, name = "out")]
        pub output: i64,
    }
    #[derive(Ports)]
    pub struct Empty {}
    #[derive(Ports)]
    pub struct Source {
        #[output]
        pub out: i64,
    }
    #[derive(Ports)]
    pub struct Sink {
        #[input]
        pub r#in: i64,
    }
    #[derive(Ports)]
    pub struct Leaves<T> {
        #[input(name = "owned.value")]
        pub r#type: T,
        #[input(encoded, name = "wire.value")]
        pub wire: i64,
    }
    #[derive(Ports)]
    pub struct Request<T> {
        #[input(flatten, name = "renamed")]
        pub nested: Leaves<T>,
        #[input(name = "label")]
        pub label: String,
    }
    #[derive(Ports)]
    pub struct Envelope<T> {
        #[input(flatten)]
        pub body: Request<T>,
    }
}

fn io(author: Authoring, wire: bool) -> Arc<Interface> {
    match author {
        Authoring::Manual => Arc::new(if wire {
            Interface::new(
                [PortDef::wire::<i64>("in").unwrap()],
                [PortDef::wire::<i64>("out").unwrap()],
            )
            .unwrap()
        } else {
            Interface::new(
                [PortDef::local::<i64>("in")],
                [PortDef::local::<i64>("out")],
            )
            .unwrap()
        }),
        Authoring::Keys => Arc::new(
            Interface::new(
                [input_key(wire).definition().unwrap()],
                [output_key(wire).definition().unwrap()],
            )
            .unwrap(),
        ),
        #[cfg(feature = "derive")]
        Authoring::Derived => {
            if wire {
                declarations::Wire::interface().unwrap()
            } else {
                declarations::Local::interface().unwrap()
            }
        }
    }
}

fn input_key(wire: bool) -> InputKey<i64> {
    if wire {
        InputKey::encoded("in")
    } else {
        InputKey::new("in")
    }
}
fn output_key(wire: bool) -> OutputKey<i64> {
    if wire {
        OutputKey::encoded("out")
    } else {
        OutputKey::new("out")
    }
}
fn input(author: Authoring, node: &ComponentRef, wire: bool) -> InputRef<i64> {
    match author {
        Authoring::Manual => {
            if wire {
                node.wire_input("in")
            } else {
                node.input("in")
            }
        }
        Authoring::Keys => node.input_key(input_key(wire)),
        #[cfg(feature = "derive")]
        Authoring::Derived => node.input_key(if wire {
            declarations::Wire::INPUT
        } else {
            declarations::Local::INPUT
        }),
    }
    .unwrap()
}
fn output(author: Authoring, node: &ComponentRef, wire: bool) -> OutputRef<i64> {
    match author {
        Authoring::Manual => {
            if wire {
                node.wire_output("out")
            } else {
                node.output("out")
            }
        }
        Authoring::Keys => node.output_key(output_key(wire)),
        #[cfg(feature = "derive")]
        Authoring::Derived => node.output_key(if wire {
            declarations::Wire::OUTPUT
        } else {
            declarations::Local::OUTPUT
        }),
    }
    .unwrap()
}

struct Echo(bool);
impl Process for Echo {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
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
fn echo() -> Arc<dyn ProcessFactory> {
    Arc::new(|| Echo(false))
}
fn body(author: Authoring, wire: bool, composite: bool) -> Arc<ComponentDef> {
    if !composite {
        return Arc::new(ComponentDef::primitive(io(author, wire), echo()));
    }
    let mut child = NetworkBuilder::new();
    let leaf = child.node("leaf", io(author, wire), echo()).unwrap();
    child
        .expose_input("in", input(author, &leaf, wire), cap(1))
        .unwrap();
    child
        .expose_output("out", output(author, &leaf, wire))
        .unwrap();
    Arc::new(ComponentDef::composite(child.build().unwrap()))
}

fn pipeline(author: Authoring, wire: bool, composite: bool) -> NetworkDef {
    let mut builder = NetworkBuilder::new();
    let before = builder.node("before", io(author, wire), echo()).unwrap();
    let target = builder
        .add("target", body(author, wire, composite))
        .unwrap();
    let after = builder.node("after", io(author, wire), echo()).unwrap();
    builder
        .connect(
            output(author, &before, wire),
            input(author, &target, wire),
            cap(2),
        )
        .unwrap();
    builder
        .connect(
            output(author, &target, wire),
            input(author, &after, wire),
            cap(4),
        )
        .unwrap();
    builder
        .expose_input("request", input(author, &before, wire), cap(3))
        .unwrap();
    builder
        .expose_output("result", output(author, &after, wire))
        .unwrap();
    builder.build().unwrap()
}

// Session and generation numbers are authenticated identities, not cross-session
// equality fields. Check consistency within each run before comparing projections.
fn execute(compiled: &CompiledNetwork, wire: bool) -> Vec<(i64, Vec<u8>, String, String, u64)> {
    let mut session = compiled.start_with_options(SessionOptions {
        capture_capacity: cap(3),
        ..Default::default()
    });
    let wrong = if wire {
        Value::local(99_i64)
    } else {
        Value::of(99_i64)
    };
    let rejected = session.inject("request", wrong).unwrap_err();
    assert_eq!(rejected.value.is_local(), wire);
    for n in [7_i64, -2, 19] {
        session
            .inject("request", if wire { Value::of(n) } else { Value::local(n) })
            .unwrap();
    }
    session.close_input("request").unwrap();
    session.run_until_idle(256);
    assert!(
        session
            .failure()
            .map(phenix_graph::Failure::summary)
            .is_none(),
        "{:?}",
        session.failure().map(phenix_graph::Failure::summary)
    );
    let mut seen = Vec::new();
    let mut owner = None;
    while let Some(packet) = session.take_output("result").unwrap() {
        let m = packet.metadata().unwrap();
        let identity = (m.session(), m.generation());
        assert_eq!(*owner.get_or_insert(identity), identity);
        assert_eq!(m.request(), None);
        assert_eq!(packet.origin, None);
        assert_eq!(m.capture(), Some("result"));
        assert_eq!(m.output(), "out");
        let path = compiled.processes()[m.process().unwrap()].path();
        assert_eq!(path, "after");
        let n = if wire {
            packet.value().get::<i64>().unwrap()
        } else {
            *packet.value().borrow::<i64>().unwrap()
        };
        assert_eq!(
            packet.value().identity(),
            if wire {
                PortDef::wire::<i64>("out").unwrap().value
            } else {
                PortDef::local::<i64>("out").value
            }
        );
        seen.push((
            n,
            packet.value().bytes().to_vec(),
            path.to_owned(),
            m.output().to_owned(),
            m.activation(),
        ));
    }
    assert_eq!(seen.iter().map(|v| v.0).collect::<Vec<_>>(), [7, -2, 19]);
    for (row, expected_bytes) in seen.iter().zip([
        [7, 0, 0, 0, 0, 0, 0, 0],
        [254, 255, 255, 255, 255, 255, 255, 255],
        [19, 0, 0, 0, 0, 0, 0, 0],
    ]) {
        assert_eq!(
            row.1,
            if wire {
                expected_bytes.to_vec()
            } else {
                vec![]
            }
        );
        assert!(row.4 > 0);
    }
    assert!(seen.windows(2).all(|pair| pair[0].4 < pair[1].4));
    assert_eq!(session.run_until_idle(256).state, SessionState::Completed);
    seen
}

#[test]
fn same_topology_authors_have_identical_canonical_compilation_and_fifo_execution() {
    for wire in [false, true] {
        for composite in [false, true] {
            let manual = pipeline(Authoring::Manual, wire, composite)
                .compile()
                .unwrap();
            let expected = execute(&manual, wire);
            for author in authors() {
                let candidate = pipeline(author, wire, composite).compile().unwrap();
                assert_eq!(candidate.interface(), manual.interface());
                assert_eq!(candidate.inputs(), manual.inputs());
                assert_eq!(candidate.outputs(), manual.outputs());
                assert_eq!(candidate.ingress_capacities(), manual.ingress_capacities());
                assert_eq!(candidate.ingress_capacities()["request"], cap(3));
                assert_eq!(candidate.connections(), manual.connections());
                let target = if composite { "target/leaf" } else { "target" };
                assert_eq!(
                    candidate
                        .connections()
                        .iter()
                        .map(|e| (
                            candidate.processes()[e.from().0].path(),
                            e.from().1,
                            candidate.processes()[e.to().0].path(),
                            e.to().1,
                            e.capacity().get(),
                        ))
                        .collect::<Vec<_>>(),
                    [
                        ("before", "out", target, "in", 2),
                        (target, "out", "after", "in", 4)
                    ]
                );
                assert_eq!(candidate.inputs()["request"], [(1, "in".to_owned())]);
                assert_eq!(candidate.outputs()["result"], (0, "out".to_owned()));
                assert_eq!(execute(&candidate, wire), expected, "{author:?}");
            }
        }
    }
}

#[test]
fn body_replacement_preserves_symbolic_incident_edges_and_public_execution_not_physical_identity() {
    for author in authors() {
        for wire in [false, true] {
            for composite_first in [false, true] {
                let old = pipeline(author, wire, composite_first);
                let old_compiled = old.compile().unwrap();
                let mut builder = NetworkBuilder::from_def(&old);
                let target = builder.component("target").unwrap();
                let stale = input(author, &target, wire);
                builder
                    .replace(&target, body(author, wire, !composite_first))
                    .unwrap();
                assert!(matches!(
                    builder.expose_input("stale", stale, cap(1)),
                    Err(NetworkError::StaleHandle)
                ));
                assert!(matches!(
                    target.input_endpoint("in"),
                    Err(NetworkError::StaleHandle)
                ));
                let new = builder.build().unwrap();
                assert_eq!(new.interface(), old.interface());
                assert_eq!(new.connections(), old.connections());
                assert_eq!(new.input_projections(), old.input_projections());
                assert_eq!(new.output_projections(), old.output_projections());
                let compiled = new.compile().unwrap();
                assert_eq!(
                    compiled.ingress_capacities(),
                    old_compiled.ingress_capacities()
                );
                assert_eq!(execute(&compiled, wire), execute(&old_compiled, wire));
            }
        }
    }
}

struct Owned(usize);

struct Assemble {
    author: Authoring,
    emitted: bool,
}
impl Process for Assemble {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        if cx.inputs().next().is_none() {
            return Ok(Step::Finished);
        }
        let result = match self.author {
            Authoring::Manual => {
                let owned = cx
                    .take("body.renamed.owned.value")
                    .ok_or_else(|| phenix_graph::Failure::application_message("missing owned"))?
                    .into_shared::<Owned>()?;
                let owned = Arc::try_unwrap(owned)
                    .map_err(|_| phenix_graph::Failure::application_message("shared owned"))?;
                let wire = cx
                    .take("body.renamed.wire.value")
                    .ok_or_else(|| phenix_graph::Failure::application_message("missing wire"))?
                    .get::<i64>()?;
                let label = cx
                    .take("body.label")
                    .ok_or_else(|| phenix_graph::Failure::application_message("missing label"))?
                    .into_owned::<String>()?;
                format!("{}:{wire}:{label}", owned.0)
            }
            Authoring::Keys => {
                let value = InputKey::<Owned>::new("body.renamed.owned.value").map_input(
                    cx,
                    "",
                    |cx, owned| {
                        InputKey::<i64>::encoded("body.renamed.wire.value").map_input(
                            cx,
                            "",
                            |cx, wire| {
                                InputKey::<String>::new("body.label").map_input(
                                    cx,
                                    "",
                                    |_, label| {
                                        Ok(Value::local(format!("{}:{wire}:{label}", owned.0)))
                                    },
                                )
                            },
                        )
                    },
                )?;
                value.borrow::<String>().unwrap().clone()
            }
            #[cfg(feature = "derive")]
            Authoring::Derived => {
                let value = cx.input_record::<declarations::Envelope<Owned>>()?;
                let record = value.borrow::<declarations::Envelope<Owned>>().unwrap();
                format!(
                    "{}:{}:{}",
                    record.body.nested.r#type.0, record.body.nested.wire, record.body.label
                )
            }
        };
        assert!(cx.inputs().next().is_none());
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new(
            "summary",
            Value::local(result),
        )))
    }
}

fn record_network(author: Authoring) -> CompiledNetwork {
    // This expected leaf list is deliberately independent of generated metadata.
    let inputs = vec![
        PortDef::local::<Owned>("body.renamed.owned.value"),
        PortDef::wire::<i64>("body.renamed.wire.value").unwrap(),
        PortDef::local::<String>("body.label"),
    ];
    let record = match author {
        #[cfg(feature = "derive")]
        Authoring::Derived => declarations::Envelope::<Owned>::interface().unwrap(),
        _ => Arc::new(Interface::new(inputs.clone(), []).unwrap()),
    };
    assert_eq!(*record, Interface::new(inputs, []).unwrap());
    assert!(record.input("body").is_none());
    let interface = Arc::new(
        Interface::new(
            record.inputs().values().cloned(),
            [PortDef::local::<String>("summary")],
        )
        .unwrap(),
    );
    let mut builder = NetworkBuilder::new();
    let node = builder
        .node(
            "assemble",
            interface,
            Arc::new(move || Assemble {
                author,
                emitted: false,
            }),
        )
        .unwrap();
    let (owned, wire, label) = match author {
        Authoring::Manual => (
            node.input::<Owned>("body.renamed.owned.value").unwrap(),
            node.wire_input::<i64>("body.renamed.wire.value").unwrap(),
            node.input::<String>("body.label").unwrap(),
        ),
        Authoring::Keys => (
            node.input_key(InputKey::<Owned>::new("body.renamed.owned.value"))
                .unwrap(),
            node.input_key(InputKey::<i64>::encoded("body.renamed.wire.value"))
                .unwrap(),
            node.input_key(InputKey::<String>::new("body.label"))
                .unwrap(),
        ),
        #[cfg(feature = "derive")]
        Authoring::Derived => {
            let body = declarations::Envelope::<Owned>::BODY;
            let nested = body.group(declarations::Request::<Owned>::NESTED);
            (
                nested
                    .input_key(&node, declarations::Leaves::<Owned>::TYPE)
                    .unwrap(),
                nested
                    .input_key(&node, declarations::Leaves::<Owned>::WIRE)
                    .unwrap(),
                body.input_key(&node, declarations::Request::<Owned>::LABEL)
                    .unwrap(),
            )
        }
    };
    builder.expose_input("owned", owned, cap(3)).unwrap();
    builder.expose_input("wire", wire, cap(4)).unwrap();
    builder.expose_input("label", label, cap(5)).unwrap();
    builder
        .expose_output("result", node.output::<String>("summary").unwrap())
        .unwrap();
    builder.build().unwrap().compile().unwrap()
}

#[test]
fn nested_renamed_flattened_records_execute_unique_move_only_local_and_wire_leaves() {
    let manual = record_network(Authoring::Manual);
    for author in authors() {
        let compiled = record_network(author);
        assert_eq!(compiled.interface(), manual.interface());
        assert_eq!(compiled.inputs(), manual.inputs());
        assert_eq!(compiled.outputs(), manual.outputs());
        assert_eq!(compiled.ingress_capacities(), manual.ingress_capacities());
        let mut session = compiled.start_with_options(SessionOptions {
            capture_capacity: cap(3),
            readiness: [("assemble".into(), Readiness::ALL)].into(),
            ..Default::default()
        });
        for (id, n, label) in [(11, 7_i64, "a"), (12, -2, "b"), (13, 19, "c")] {
            session.inject("owned", Value::local(Owned(id))).unwrap();
            session.inject("wire", Value::of(n)).unwrap();
            session
                .inject("label", Value::local(label.to_owned()))
                .unwrap();
        }
        for port in ["owned", "wire", "label"] {
            session.close_input(port).unwrap();
        }
        session.run_until_idle(128);
        assert!(
            session
                .failure()
                .map(phenix_graph::Failure::summary)
                .is_none(),
            "{author:?}: {:?}",
            session.failure().map(phenix_graph::Failure::summary)
        );
        let mut seen = Vec::new();
        while let Some(packet) = session.take_output("result").unwrap() {
            assert_eq!(packet.metadata().unwrap().capture(), Some("result"));
            assert_eq!(packet.metadata().unwrap().output(), "summary");
            seen.push(packet.value().borrow::<String>().unwrap().clone());
        }
        assert_eq!(seen, ["11:7:a", "12:-2:b", "13:19:c"]);
        assert_eq!(session.run_until_idle(128).state, SessionState::Completed);
    }
}

#[test]
fn all_record_authors_report_later_leaf_decode_failure_without_publishing_a_capture() {
    for author in authors() {
        let compiled = record_network(author);
        let mut session = compiled.start_with_options(SessionOptions {
            readiness: [("assemble".into(), Readiness::ALL)].into(),
            ..Default::default()
        });
        session.inject("owned", Value::local(Owned(31))).unwrap();
        // Correct transport identity, deliberately invalid application bytes.
        session
            .inject(
                "wire",
                Value::from_wire_parts(
                    <i64 as phenix_graph::ContractValue>::wire_schema(),
                    vec![1, 2],
                ),
            )
            .unwrap();
        session
            .inject("label", Value::local("unused".to_owned()))
            .unwrap();
        assert_eq!(session.run_until_idle(64).state, SessionState::Failed);
        assert!(
            session
                .failure()
                .map(phenix_graph::Failure::summary)
                .unwrap()
                .contains("bad i64 length"),
            "{author:?}: {:?}",
            session.failure().map(phenix_graph::Failure::summary)
        );
        assert!(session.take_output("result").unwrap().is_none());
    }
}

struct Count(Arc<Mutex<Vec<&'static str>>>, &'static str);
impl Process for Count {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        self.0.lock().unwrap().push(self.1);
        Ok(Step::Finished)
    }
}
struct Source(bool);
impl Process for Source {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        if self.0 {
            return Ok(Step::Finished);
        }
        self.0 = true;
        Ok(Step::Emit(ProcessEmission::new(
            "out",
            Value::local(23_i64),
        )))
    }
}
struct Sink(Arc<Mutex<Vec<i64>>>);
impl Process for Sink {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        if let Some(value) = cx.take("in") {
            self.0.lock().unwrap().push(*value.borrow::<i64>().unwrap());
            Ok(Step::Complete)
        } else {
            Ok(Step::Finished)
        }
    }
}

#[test]
fn empty_source_and_sink_execute_without_synthetic_unit_ports() {
    for author in authors() {
        let empty = Arc::new(Interface::default());
        let source = Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap());
        let sink = Arc::new(Interface::new([PortDef::local::<i64>("in")], []).unwrap());
        let (empty, source, sink) = match author {
            #[cfg(feature = "derive")]
            Authoring::Derived => {
                assert_eq!(declarations::Empty::interface().unwrap(), empty);
                assert_eq!(declarations::Source::interface().unwrap(), source);
                assert_eq!(declarations::Sink::interface().unwrap(), sink);
                (
                    declarations::Empty::interface().unwrap(),
                    declarations::Source::interface().unwrap(),
                    declarations::Sink::interface().unwrap(),
                )
            }
            _ => (empty, source, sink),
        };
        let calls = Arc::new(Mutex::new(vec![]));
        let values = Arc::new(Mutex::new(vec![]));
        let mut builder = NetworkBuilder::new();
        let c = calls.clone();
        let empty = builder
            .node("empty", empty, Arc::new(move || Count(c.clone(), "empty")))
            .unwrap();
        let source = builder
            .node("source", source, Arc::new(|| Source(false)))
            .unwrap();
        let v = values.clone();
        let sink = builder
            .node("sink", sink, Arc::new(move || Sink(v.clone())))
            .unwrap();
        let (from, to) = match author {
            #[cfg(feature = "derive")]
            Authoring::Derived => (
                source.output_key(declarations::Source::OUT).unwrap(),
                sink.input_key(declarations::Sink::IN).unwrap(),
            ),
            _ => (output(author, &source, false), input(author, &sink, false)),
        };
        builder.connect(from, to, cap(2)).unwrap();
        builder.start(&empty).unwrap();
        builder.start(&source).unwrap();
        assert!(matches!(
            builder.start(&sink),
            Err(NetworkError::InvalidSource(_))
        ));
        let compiled = builder.build().unwrap().compile().unwrap();
        assert_eq!(*compiled.interface().as_ref(), Interface::default());
        assert!(compiled.inputs().is_empty());
        assert!(compiled.outputs().is_empty());
        assert!(compiled.ingress_capacities().is_empty());
        assert_eq!(compiled.source_starts(), [0, 2]);
        assert_eq!(compiled.connections().len(), 1);
        assert_eq!(compiled.connections()[0].from(), (2, "out"));
        assert_eq!(compiled.connections()[0].to(), (1, "in"));
        assert_eq!(compiled.connections()[0].capacity(), cap(2));
        let mut session = compiled.start();
        assert_eq!(session.run_until_idle(64).state, SessionState::Completed);
        assert_eq!(*calls.lock().unwrap(), ["empty"]);
        assert_eq!(*values.lock().unwrap(), [23]);
    }
}

#[test]
fn keys_do_not_grant_scope_revision_claim_or_compiled_generation_authority() {
    for author in authors() {
        let contract = Contract::new("acceptance/echo", io(Authoring::Manual, false)).unwrap();
        let definition = body(author, false, false);
        assert!(definition.claims().is_empty());
        assert!(contract.check_implementation(&definition).is_err());
        let claimed = Arc::new(definition.as_ref().clone().claim(&contract).unwrap());
        let mut builder = NetworkBuilder::new();
        let node = builder.add("same", claimed.clone()).unwrap();
        let old_input = input(author, &node, false);
        let erased_input = node.input_endpoint("in").unwrap();
        let erased_output = node.output_endpoint("out").unwrap();
        let bound = builder.bind(&node, &contract).unwrap();
        let mut other = NetworkBuilder::new();
        other.add("same", claimed.clone()).unwrap();
        assert!(matches!(
            other.expose_input("foreign", old_input.clone(), cap(1)),
            Err(NetworkError::ForeignScope)
        ));
        assert!(matches!(
            other.expose_output("foreign", bound.output::<i64>("out").unwrap()),
            Err(NetworkError::ForeignScope)
        ));
        assert!(matches!(
            other.connect_dynamic(&erased_output, &erased_input, cap(1)),
            Err(NetworkError::ForeignScope)
        ));
        assert!(matches!(
            builder.replace(
                &node,
                Arc::new(ComponentDef::primitive(
                    Arc::new(Interface::default()),
                    echo()
                ))
            ),
            Err(NetworkError::Interface(_))
        ));
        assert!(builder.replace(&node, definition).is_err());
        // Failure must not revoke the original scope, endpoint, or bound view.
        builder
            .expose_input("request", old_input.clone(), cap(3))
            .unwrap();
        builder
            .expose_output("result", bound.output::<i64>("out").unwrap())
            .unwrap();
        assert!(node.input_key(InputKey::<String>::new("in")).is_err());
        assert!(node.input_key(InputKey::<i64>::encoded("in")).is_err());
        assert!(node.output_key(OutputKey::<i64>::new("missing")).is_err());
        builder.replace(&node, claimed).unwrap();
        assert!(matches!(
            builder.connect_dynamic(&erased_output, &erased_input, cap(1)),
            Err(NetworkError::StaleHandle)
        ));
        assert!(matches!(
            bound.output::<i64>("out"),
            Err(NetworkError::StaleHandle)
        ));
        assert!(matches!(
            builder.expose_input("stale", old_input, cap(1)),
            Err(NetworkError::StaleHandle)
        ));
        let network = builder.build().unwrap();
        let a = network.compile().unwrap();
        let b = network.compile().unwrap();
        let binding = a.bind("same", &contract).unwrap();
        let input = binding.input::<i64>("in").unwrap();
        let output = binding.output::<i64>("out").unwrap();
        assert_eq!(input.resolve(&a).unwrap(), [(0, "in".to_owned())]);
        assert_eq!(output.resolve(&a).unwrap(), (0, "out"));
        assert!(matches!(input.resolve(&b), Err(NetworkError::ForeignScope)));
        assert!(matches!(
            output.resolve(&b),
            Err(NetworkError::ForeignScope)
        ));
    }
}

#[test]
fn equal_public_shape_and_claim_do_not_erase_private_obligations() {
    for author in authors() {
        let contract = Contract::new("acceptance/private", io(Authoring::Manual, false)).unwrap();
        let claimed = Arc::new(
            body(author, false, false)
                .as_ref()
                .clone()
                .claim(&contract)
                .unwrap(),
        );
        let mut child = NetworkBuilder::new();
        let leaf = child.add("private", claimed.clone()).unwrap();
        child
            .expose_input("in", input(author, &leaf, false), cap(2))
            .unwrap();
        child
            .expose_output("out", output(author, &leaf, false))
            .unwrap();
        child.claim(&contract).unwrap();
        child.require_contract("private", &contract).unwrap();
        let mut builder = NetworkBuilder::new();
        let module = builder.module("module", child.build().unwrap()).unwrap();
        let endpoint = input(author, &module, false);
        assert!(builder.replace(&module, claimed).is_err());
        builder.expose_input("request", endpoint, cap(3)).unwrap();
        builder
            .expose_output("result", output(author, &module, false))
            .unwrap();
        let compiled = builder.build().unwrap().compile().unwrap();
        compiled
            .check_implementation("module/private", &contract)
            .unwrap();
        let mut session = compiled.start();
        session.inject("request", Value::local(17_i64)).unwrap();
        session.run_until_idle(64);
        assert!(
            session
                .failure()
                .map(phenix_graph::Failure::summary)
                .is_none()
        );
        assert_eq!(
            *session
                .take_output("result")
                .unwrap()
                .unwrap()
                .value()
                .borrow::<i64>()
                .unwrap(),
            17
        );
    }
}

#[cfg(feature = "derive")]
#[allow(dead_code)]
mod reconstruction_failures {
    use super::*;
    use phenix_graph::{ContractValue, Ports, Schema};
    use std::sync::{
        Weak,
        atomic::{AtomicBool, Ordering},
    };

    struct Lease(Arc<Mutex<Vec<&'static str>>>);
    impl Drop for Lease {
        fn drop(&mut self) {
            self.0.lock().unwrap().push("lease");
        }
    }
    struct LeasedOwned {
        lease: Weak<Lease>,
        dropped_with_live_lease: Arc<AtomicBool>,
        events: Arc<Mutex<Vec<&'static str>>>,
    }
    impl Drop for LeasedOwned {
        fn drop(&mut self) {
            self.dropped_with_live_lease
                .store(self.lease.upgrade().is_some(), Ordering::SeqCst);
            self.events.lock().unwrap().push("payload");
        }
    }
    struct Later(bool);
    impl ContractValue for Later {
        fn schema() -> Schema {
            Schema {
                name: "acceptance/later".into(),
                revision: 1,
                structure: "bool".into(),
                codec: "failure-v1".into(),
            }
        }
        fn encode(&self) -> Vec<u8> {
            vec![u8::from(self.0)]
        }
        fn decode(bytes: &[u8]) -> Result<Self, phenix_graph::Failure> {
            if bytes == [1] {
                panic!("later decoder unwound")
            }
            Err(phenix_graph::Failure::application_message(
                "later decoder failed",
            ))
        }
    }
    #[derive(Ports)]
    struct Earlier {
        #[input(name = "owned")]
        value: LeasedOwned,
    }
    #[derive(Ports)]
    struct LaterRecord {
        #[input(encoded, name = "wire")]
        value: Later,
    }
    #[derive(Ports)]
    struct Nested {
        #[input(flatten, name = "first")]
        earlier: Earlier,
        #[input(flatten, name = "later")]
        later: LaterRecord,
    }
    #[derive(Ports)]
    struct Envelope {
        #[input(flatten, name = "request")]
        nested: Nested,
    }

    #[test]
    fn generated_nested_later_leaf_error_and_unwind_drop_acquired_payload_before_lease() {
        for panic in [false, true] {
            let events = Arc::new(Mutex::new(vec![]));
            let live = Arc::new(AtomicBool::new(false));
            let lease = Arc::new(Lease(events.clone()));
            let payload = LeasedOwned {
                lease: Arc::downgrade(&lease),
                dropped_with_live_lease: live.clone(),
                events: events.clone(),
            };
            let mut cx = ProcessContext::new(
                [
                    (
                        "request.first.owned".into(),
                        Value::local(payload).with_lease(lease),
                    ),
                    ("request.later.wire".into(), Value::of(Later(panic))),
                    ("unselected".into(), Value::local(97_i64)),
                ]
                .into(),
                vec![],
            );
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                cx.input_record::<Envelope>()
            }));
            let failure = result.unwrap().unwrap_err();
            if panic {
                assert_eq!(failure.kind(), phenix_graph::FailureKind::Panic);
            } else {
                assert_eq!(failure.summary(), "later decoder failed");
            }
            assert!(
                live.load(Ordering::SeqCst),
                "payload destructor must observe a live code lease"
            );
            assert_eq!(*events.lock().unwrap(), ["payload"]);
            drop(failure);
            assert_eq!(*events.lock().unwrap(), ["payload", "lease"]);
            // Reconstruction is not transactional: earlier leaves were acquired.
            assert!(cx.input("request.first.owned").is_none());
            assert!(cx.input("request.later.wire").is_none());
            assert_eq!(
                *cx.input("unselected").unwrap().borrow::<i64>().unwrap(),
                97
            );
        }
    }
}
