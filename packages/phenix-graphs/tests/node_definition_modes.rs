use phenix_graph::{
    DeclaredValue, Deployment, EncodedValue, GraphBuilder, Input, InputSlot, InvocationCtx,
    LayoutError, LocalValue, Node, NodeDefinition, NodeSignature, NodeTemplate, OutputRef,
    OutputSlot, Port, ReportRetention, Value, emit, emit_local,
};
use std::sync::Arc;

struct Encode;
impl NodeDefinition<i64, i64> for Encode {
    type Layout = phenix_graph::LocalAtomic<i64>;
    type InputMode = LocalValue;
    type OutputMode = EncodedValue;

    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        Ok(NodeTemplate::with_action(
            0,
            vec![Port::all(0, vec![InputSlot::local::<i64>(0).named("value")]).named("run")],
            OutputSlot::typed::<i64>(0).named("value"),
            |ctx: InvocationCtx| Ok(vec![emit(0, *ctx.shared::<i64>(0)? + 1)]),
        ))
    }

    fn signature() -> NodeSignature {
        NodeSignature {
            input: DeclaredValue::local::<i64>(),
            output: DeclaredValue::wire::<i64>(),
        }
    }
}

struct Decode;
impl NodeDefinition<i64, i64> for Decode {
    type Layout = phenix_graph::Atomic<i64>;
    type InputMode = EncodedValue;
    type OutputMode = LocalValue;

    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        Ok(NodeTemplate::with_action(
            0,
            vec![Port::all(0, vec![InputSlot::deferred::<i64>(0).named("value")]).named("run")],
            OutputSlot::local::<i64>(0).named("value"),
            |ctx: InvocationCtx| Ok(vec![emit_local(0, ctx.arg::<i64>(0)? + 1)]),
        ))
    }

    fn signature() -> NodeSignature {
        NodeSignature {
            input: DeclaredValue::wire::<i64>(),
            output: DeclaredValue::local::<i64>(),
        }
    }
}

struct Encoded;
impl NodeDefinition<i64, i64> for Encoded {
    type Layout = phenix_graph::Atomic<i64>;
    type InputMode = EncodedValue;
    type OutputMode = EncodedValue;

    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        Ok(NodeTemplate::function(|value: i64| value + 1))
    }

    fn signature() -> NodeSignature {
        NodeSignature {
            input: DeclaredValue::wire::<i64>(),
            output: DeclaredValue::wire::<i64>(),
        }
    }
}

#[test]
fn custom_definitions_choose_input_and_output_modes_independently() {
    let mut graph = GraphBuilder::new("custom-modes");
    let source = graph.node("source", |value: i64| value + 1).unwrap();
    let encode = graph.node("encode", Encode).unwrap();
    let encoded = graph.node("encoded", Encoded).unwrap();
    let wire = graph
        .node("wire", Node::wire(|value: i64| value + 1))
        .unwrap();
    let decode = graph.node("decode", Decode).unwrap();
    let sink = graph.node("sink", |value: i64| value + 1).unwrap();

    let _: Input<i64, LocalValue> = encode.inlet();
    let _: OutputRef<i64, EncodedValue> = encode.output();
    let _: Input<i64, EncodedValue> = encoded.inlet();
    let _: OutputRef<i64, EncodedValue> = encoded.output();
    let _: Input<i64, EncodedValue> = decode.inlet();
    let _: OutputRef<i64, LocalValue> = decode.output();

    graph.connect(source.output(), encode.inlet()).unwrap();
    graph.connect(encode.output(), encoded.inlet()).unwrap();
    graph.connect(encoded.output(), wire.inlet()).unwrap();
    graph.connect(wire.output(), decode.inlet()).unwrap();
    graph.connect(decode.output(), sink.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("run", [request]).unwrap();

    let report = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire("run", vec![Value::local(36_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    let publications: Vec<_> = report.publications().collect();
    assert_eq!(publications.len(), 6);
    assert_eq!(publications[0].borrow::<i64>(), Some(&37));
    assert_eq!(publications[1].value::<i64>().unwrap(), 38);
    assert_eq!(publications[2].value::<i64>().unwrap(), 39);
    assert_eq!(publications[3].value::<i64>().unwrap(), 40);
    assert_eq!(publications[4].borrow::<i64>(), Some(&41));
    assert_eq!(publications[5].borrow::<i64>(), Some(&42));
}

struct BadAtomic(Vec<Port>);
impl NodeDefinition<i64, i64> for BadAtomic {
    type Layout = phenix_graph::LocalAtomic<i64>;
    type InputMode = LocalValue;
    type OutputMode = LocalValue;
    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        Ok(NodeTemplate::with_action(
            0,
            self.0.clone(),
            OutputSlot::local::<i64>(0).named("value"),
            |_| Ok(vec![]),
        ))
    }
    fn signature() -> NodeSignature {
        NodeSignature {
            input: DeclaredValue::local::<i64>(),
            output: DeclaredValue::local::<i64>(),
        }
    }
}

#[test]
fn atomic_admission_rejects_wrong_shape_identity_and_unnamed_endpoints() {
    let valid = || Port::all(0, vec![InputSlot::local::<i64>(0).named("value")]).named("run");
    let mut graph = GraphBuilder::new("bad-atomic");
    for ports in [
        vec![],
        vec![
            valid(),
            Port::all(1, vec![InputSlot::local::<i64>(0).named("value")]).named("other"),
        ],
        vec![Port::all(0, vec![]).named("run")],
        vec![Port::all(0, vec![InputSlot::local::<String>(0).named("value")]).named("run")],
        vec![Port::all(0, vec![InputSlot::deferred::<i64>(0).named("value")]).named("run")],
        vec![Port::all(0, vec![InputSlot::local::<i64>(0)])],
        vec![
            Port::all(
                0,
                vec![
                    InputSlot::local::<i64>(0).named("value"),
                    InputSlot::local::<i64>(0).named("other"),
                ],
            )
            .named("run"),
        ],
    ] {
        assert!(graph.node("bad", BadAtomic(ports)).is_err());
        assert!(graph.discover("bad").is_err());
    }
    assert!(graph.node("bad", BadAtomic(vec![valid()])).is_ok());
}

struct LyingEncoded;
impl NodeDefinition<i64, i64> for LyingEncoded {
    type Layout = phenix_graph::Atomic<i64>;
    type InputMode = EncodedValue;
    type OutputMode = LocalValue;
    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        Ok(NodeTemplate::with_action(
            0,
            vec![Port::all(0, vec![InputSlot::deferred::<String>(0).named("value")]).named("run")],
            OutputSlot::local::<i64>(0).named("value"),
            |_| Ok(vec![]),
        ))
    }
    fn signature() -> NodeSignature {
        NodeSignature {
            input: DeclaredValue::wire::<String>(),
            output: DeclaredValue::local::<i64>(),
        }
    }
}

#[test]
fn encoded_atomic_schema_is_checked_against_the_domain_not_just_signature() {
    assert!(
        GraphBuilder::new("lying")
            .node("bad", LyingEncoded)
            .is_err()
    );
    let mut graph = GraphBuilder::new("wire-atomic-layout");
    let node = graph
        .node(
            "valid",
            Node::wire_layout(
                |x: i64| x,
                phenix_graph::Layout::custom::<phenix_graph::Atomic<i64>>(),
            ),
        )
        .unwrap();
    let _: Input<i64, EncodedValue> = node.inlet();
}
