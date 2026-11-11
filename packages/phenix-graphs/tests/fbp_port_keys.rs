#![cfg(feature = "derive")]
#![allow(dead_code)]
use phenix_graph::network::NetworkBuilder;
use phenix_graph::process::{Process, ProcessContext, ProcessFactory, Step};
use phenix_graph::{InputKey, InputRecord, Interface, OutputKey, PortDef, Ports, Value};
use std::num::NonZeroUsize;
use std::sync::Arc;

#[derive(Ports)]
struct Local<T>
where
    T: 'static,
{
    #[input]
    r#type: T,
    #[output]
    result: T,
}
#[derive(Ports)]
struct Encoded {
    #[input(encoded)]
    r#type: i64,
    #[output(encoded)]
    result: i64,
}
#[derive(Ports)]
struct Empty {}
#[derive(Ports)]
struct Source {
    #[output]
    value: i64,
}
#[derive(Ports)]
struct Sink {
    #[input]
    value: i64,
}

struct Noop;
impl Process for Noop {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        Ok(Step::Finished)
    }
}
fn factory() -> Arc<dyn ProcessFactory> {
    Arc::new(|| Noop)
}
fn cap() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

#[test]
fn derived_interfaces_are_exactly_the_manual_canonical_interfaces() {
    assert_eq!(
        *Local::<i64>::interface().unwrap(),
        Interface::new(
            [PortDef::local::<i64>("type")],
            [PortDef::local::<i64>("result")]
        )
        .unwrap()
    );
    assert_eq!(
        *Encoded::interface().unwrap(),
        Interface::new(
            [PortDef::wire::<i64>("type").unwrap()],
            [PortDef::wire::<i64>("result").unwrap()]
        )
        .unwrap()
    );
    assert_eq!(Local::<i64>::TYPE.name(), "type");
    assert_eq!(Encoded::RESULT.name(), "result");
    assert_eq!(*Empty::interface().unwrap(), Interface::default());
    assert!(Source::interface().unwrap().inputs().is_empty());
    assert!(Sink::interface().unwrap().outputs().is_empty());
}

#[test]
fn local_and_encoded_keys_use_equal_node_and_module_syntax() {
    for encoded in [false, true] {
        let (interface, input, output) = if encoded {
            (
                Encoded::interface().unwrap(),
                Encoded::TYPE,
                Encoded::RESULT,
            )
        } else {
            (
                Local::<i64>::interface().unwrap(),
                Local::<i64>::TYPE,
                Local::<i64>::RESULT,
            )
        };
        let mut child = NetworkBuilder::new();
        let leaf = child.node("leaf", interface.clone(), factory()).unwrap();
        child
            .expose_input("type", leaf.input_key(input).unwrap(), cap())
            .unwrap();
        child
            .expose_output("result", leaf.output_key(output).unwrap())
            .unwrap();
        let definition = child.build().unwrap();
        assert_eq!(definition.interface(), &interface);
        let mut graph = NetworkBuilder::new();
        let node = graph.node("node", interface, factory()).unwrap();
        let module = graph.module("module", definition).unwrap();
        graph
            .connect(
                node.output_key(output).unwrap(),
                module.input_key(input).unwrap(),
                cap(),
            )
            .unwrap();
        graph
            .connect(
                module.output_key(output).unwrap(),
                node.input_key(input).unwrap(),
                cap(),
            )
            .unwrap();
        assert_eq!(
            graph
                .build()
                .unwrap()
                .compile()
                .unwrap()
                .connections()
                .len(),
            2
        );
    }
}

#[test]
fn keys_are_typed_names_not_owner_or_identity_authority() {
    let mut graph = NetworkBuilder::new();
    let node = graph
        .node("node", Local::<i64>::interface().unwrap(), factory())
        .unwrap();
    assert!(node.input_key(InputKey::<i64>::new("missing")).is_err());
    assert!(node.input_key(InputKey::<String>::new("type")).is_err());
    assert!(node.input_key(InputKey::<i64>::encoded("type")).is_err());
    assert!(node.output_key(OutputKey::<i64>::new("type")).is_err());
    assert!(node.input_key(Local::<i64>::TYPE).is_ok());
    let wire = graph
        .node("wire", Encoded::interface().unwrap(), factory())
        .unwrap();
    assert!(wire.input_key(InputKey::<i64>::new("type")).is_err());
    assert!(wire.output_key(OutputKey::<i64>::new("result")).is_err());
    assert!(wire.input_key(Encoded::TYPE).is_ok());
}

#[test]
fn empty_source_and_sink_are_native_without_unit_ports() {
    let mut graph = NetworkBuilder::new();
    let empty = graph
        .node("empty", Empty::interface().unwrap(), factory())
        .unwrap();
    let source = graph
        .node("source", Source::interface().unwrap(), factory())
        .unwrap();
    let sink = graph
        .node("sink", Sink::interface().unwrap(), factory())
        .unwrap();
    graph.start(&empty).unwrap();
    graph.start(&source).unwrap();
    graph
        .connect(
            source.output_key(Source::VALUE).unwrap(),
            sink.input_key(Sink::VALUE).unwrap(),
            cap(),
        )
        .unwrap();
    let compiled = graph.build().unwrap().compile().unwrap();
    assert_eq!(compiled.processes().len(), 3);
    assert_eq!(compiled.connections().len(), 1);
}

#[derive(phenix_graph::ContractValue, Debug, PartialEq)]
#[contract(schema = "ports/codec-golden")]
struct Record {
    r#type: i64,
    value: Option<()>,
}

#[test]
fn contract_value_record_codec_keeps_its_length_framed_golden_bytes() {
    use phenix_graph::ContractValue;
    for tag in [0, 1] {
        let value = Record {
            r#type: 7,
            value: (tag == 1).then_some(()),
        };
        let bytes = [
            8, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, tag,
        ];
        assert_eq!(value.encode(), bytes);
        assert_eq!(Record::decode(&bytes).unwrap(), value);
        for length in 0..bytes.len() {
            assert!(Record::decode(&bytes[..length]).is_err());
        }
        let mut trailing = bytes.to_vec();
        trailing.push(0);
        assert!(Record::decode(&trailing).is_err());
    }
}

#[derive(Ports)]
struct Inner<T> {
    #[input(name = "named.value")]
    r#type: T,
    #[input(encoded, name = "wire.value")]
    wire: i64,
}

#[derive(Ports)]
struct Request<T> {
    #[input(flatten, name = "request")]
    nested: Inner<T>,
    #[input(name = "independent")]
    other: String,
}

#[derive(Ports)]
struct Envelope<T> {
    #[input(flatten)]
    body: Request<T>,
}

fn context(values: impl IntoIterator<Item = (&'static str, Value)>) -> ProcessContext {
    ProcessContext::new(
        values
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect(),
        vec![],
    )
}

#[test]
fn renamed_nested_raw_generic_records_reconstruct_only_real_leaf_ports() {
    let interface = Envelope::<String>::interface().unwrap();
    assert_eq!(
        interface
            .inputs()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "body.independent",
            "body.request.named.value",
            "body.request.wire.value"
        ]
    );
    assert!(interface.input("body.request").is_none());
    let mut builder = NetworkBuilder::new();
    let node = builder.node("node", interface, factory()).unwrap();
    let group = Envelope::<String>::BODY.group(Request::<String>::NESTED);
    assert!(group.input_key(&node, Inner::<String>::TYPE).is_ok());
    assert!(group.input_key(&node, Inner::<String>::WIRE).is_ok());
    assert!(
        group
            .input_key(&node, InputKey::<i64>::new("wire.value"))
            .is_err()
    );
    let mut cx = context([
        (
            "body.request.named.value",
            Value::local(String::from("local")),
        ),
        ("body.request.wire.value", Value::of(42_i64)),
        ("body.independent", Value::local(String::from("other"))),
        ("untouched", Value::local(9_i64)),
    ]);
    let value = cx.input_record::<Envelope<String>>().unwrap();
    let record = value.borrow::<Envelope<String>>().unwrap();
    assert_eq!(record.body.nested.r#type, "local");
    assert_eq!(record.body.nested.wire, 42);
    assert_eq!(record.body.other, "other");
    assert_eq!(
        cx.inputs().map(|(name, _)| name).collect::<Vec<_>>(),
        ["untouched"]
    );
}

#[derive(Ports)]
struct DuplicateFlatten {
    #[input(flatten, name = "request")]
    nested: Inner<i64>,
    #[input(name = "request.named.value")]
    collision: i64,
}

#[test]
fn flattened_path_collisions_fail_before_consumption() {
    let error = DuplicateFlatten::interface().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("duplicate port name \"request.named.value\"")
    );
    let mut cx = context([("request.named.value", Value::local(7_i64))]);
    assert!(cx.input_record::<DuplicateFlatten>().is_err());
    assert!(cx.input("request.named.value").is_some());
}

struct MoveOnly {
    id: usize,
    events: Arc<std::sync::Mutex<Vec<&'static str>>>,
}
impl Drop for MoveOnly {
    fn drop(&mut self) {
        self.events.lock().unwrap().push("payload");
    }
}
struct Lease(Arc<std::sync::Mutex<Vec<&'static str>>>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.lock().unwrap().push("lease");
    }
}

#[derive(Ports)]
struct OwnedLeaf<T> {
    #[input(name = "request.value")]
    value: T,
}

#[derive(Ports)]
struct SharedLeaf {
    #[input(name = "request.value")]
    value: Arc<MoveOnly>,
}

#[derive(Ports)]
struct WireLeaf {
    #[input(encoded, name = "request.value")]
    value: i64,
}

#[test]
fn unique_move_only_leaf_moves_and_shared_move_only_leaf_errors_without_clone() {
    let events = Arc::new(std::sync::Mutex::new(vec![]));
    let mut cx = context([(
        "request.value",
        Value::local(MoveOnly {
            id: 17,
            events: events.clone(),
        }),
    )]);
    let record = cx.input_record::<OwnedLeaf<MoveOnly>>().unwrap();
    assert_eq!(record.borrow::<OwnedLeaf<MoveOnly>>().unwrap().value.id, 17);
    assert!(events.lock().unwrap().is_empty());
    drop(record);
    assert_eq!(*events.lock().unwrap(), ["payload"]);
    events.lock().unwrap().clear();
    let source = Value::local(MoveOnly {
        id: 19,
        events: events.clone(),
    });
    let mut cx = context([("request.value", source.clone())]);
    assert!(
        cx.input_record::<OwnedLeaf<MoveOnly>>()
            .unwrap_err()
            .contains("is shared")
    );
    assert!(events.lock().unwrap().is_empty());
    drop(source);
    assert_eq!(*events.lock().unwrap(), ["payload"]);
}

#[test]
fn leased_move_only_input_retains_code_through_reconstruction_and_escaping_clones() {
    let events = Arc::new(std::sync::Mutex::new(vec![]));
    let source = Value::local(MoveOnly {
        id: 31,
        events: events.clone(),
    })
    .with_lease(Arc::new(Lease(events.clone())));
    let mut cx = context([("request.value", source)]);
    let record = cx.input_record::<OwnedLeaf<MoveOnly>>().unwrap();
    assert_eq!(record.borrow::<OwnedLeaf<MoveOnly>>().unwrap().value.id, 31);
    assert!(record.shared::<OwnedLeaf<MoveOnly>>().is_err());
    let retained = record.leased_shared::<OwnedLeaf<MoveOnly>>().unwrap();
    drop(record);
    drop(cx);
    assert!(events.lock().unwrap().is_empty());
    drop(retained);
    assert_eq!(*events.lock().unwrap(), ["payload", "lease"]);
}

#[test]
fn leased_local_reconstruction_pins_code_on_error_and_unwind() {
    for panic in [false, true] {
        let events = Arc::new(std::sync::Mutex::new(vec![]));
        let source = Value::local(MoveOnly {
            id: 41,
            events: events.clone(),
        })
        .with_lease(Arc::new(Lease(events.clone())));
        let mut cx = context([("request.value", source)]);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            OwnedLeaf::<MoveOnly>::VALUE.map_input(&mut cx, "", |_, payload| {
                assert_eq!(payload.id, 41);
                assert!(events.lock().unwrap().is_empty());
                if panic {
                    panic!("reconstruction failed");
                }
                Err("reconstruction failed".to_owned())
            })
        }));
        if panic {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap().unwrap_err(), "reconstruction failed");
        }
        assert_eq!(*events.lock().unwrap(), ["payload", "lease"]);
    }
}

#[test]
fn explicit_arc_fanout_and_encoded_records_retain_leases_until_payload_drop() {
    let events = Arc::new(std::sync::Mutex::new(vec![]));
    let source = Value::local(Arc::new(MoveOnly {
        id: 23,
        events: events.clone(),
    }))
    .with_lease(Arc::new(Lease(events.clone())));
    let mut first = context([("request.value", source.clone())]);
    let mut second = context([("request.value", source)]);
    let a = first.input_record::<SharedLeaf>().unwrap();
    let b = second.input_record::<SharedLeaf>().unwrap();
    assert_eq!(a.borrow::<SharedLeaf>().unwrap().value.id, 23);
    assert!(Arc::ptr_eq(
        &a.borrow::<SharedLeaf>().unwrap().value,
        &b.borrow::<SharedLeaf>().unwrap().value
    ));
    drop(a);
    assert!(events.lock().unwrap().is_empty());
    drop(b);
    assert_eq!(*events.lock().unwrap(), ["payload", "lease"]);
    events.lock().unwrap().clear();
    let mut cx = context([(
        "request.value",
        Value::of(29_i64).with_lease(Arc::new(Lease(events.clone()))),
    )]);
    let record = WireLeaf::from_inputs(&mut cx).unwrap();
    assert_eq!(record.borrow::<WireLeaf>().unwrap().value, 29);
    assert!(events.lock().unwrap().is_empty());
    assert!(record.shared::<WireLeaf>().is_err());
    drop(record);
    assert_eq!(*events.lock().unwrap(), ["lease"]);
}

#[test]
fn reconstruction_checks_local_encoded_identity_and_missing_leaves() {
    let mut cx = context([("request.value", Value::local(3_i64))]);
    assert!(
        cx.input_record::<WireLeaf>()
            .unwrap_err()
            .contains("incompatible input")
    );
    assert!(cx.input("request.value").is_some());
    let mut cx = context([]);
    assert!(
        cx.input_record::<WireLeaf>()
            .unwrap_err()
            .contains("missing input")
    );
}

#[derive(Ports)]
struct SelectedWire {
    #[input(encoded, name = "request.wire.value")]
    value: i64,
}

struct SelectLeaf(Arc<std::sync::Mutex<Vec<i64>>>);
impl Process for SelectLeaf {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if cx.inputs().next().is_none() {
            return Ok(Step::Complete);
        }
        let record = cx.input_record::<SelectedWire>()?;
        self.0
            .lock()
            .unwrap()
            .push(record.borrow::<SelectedWire>().unwrap().value);
        Ok(Step::Complete)
    }
}

#[test]
fn nested_leaf_is_an_independent_selectable_fifo_not_a_hidden_group_slot() {
    let seen = Arc::new(std::sync::Mutex::new(vec![]));
    let captured = seen.clone();
    let mut builder = NetworkBuilder::new();
    let node = builder
        .node(
            "select",
            Request::<i64>::interface().unwrap(),
            Arc::new(move || SelectLeaf(captured.clone())),
        )
        .unwrap();
    builder
        .expose_input(
            "wire",
            Request::<i64>::NESTED
                .input_key(&node, Inner::<i64>::WIRE)
                .unwrap(),
            NonZeroUsize::new(3).unwrap(),
        )
        .unwrap();
    builder
        .expose_input(
            "local",
            Request::<i64>::NESTED
                .input_key(&node, Inner::<i64>::TYPE)
                .unwrap(),
            cap(),
        )
        .unwrap();
    let mut session = builder.build().unwrap().compile().unwrap().start();
    for value in [4_i64, 5, 6] {
        session.inject("wire", Value::of(value)).unwrap();
    }
    session.run_until_idle(100);
    assert!(session.failure().is_none(), "{:?}", session.failure());
    assert_eq!(*seen.lock().unwrap(), [4, 5, 6]);
}
