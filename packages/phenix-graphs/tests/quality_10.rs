//! Quality gates for the lean graph core: ownership, structural layouts,
//! local identity, report retention, backend preparation, and model checking.
use phenix_graph::Node;
use phenix_graph::{
    BufferMode, Deployment, EdgeAdapter, GraphBuilder, InputSlot, OutputSlot, ReportRetention,
    Store, TypeIdentity, Value,
};
use std::any::{TypeId, type_name};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Debug)]
struct Counted {
    payload: i64,
    clones: Arc<AtomicUsize>,
}
impl Clone for Counted {
    fn clone(&self) -> Self {
        self.clones.fetch_add(1, Ordering::SeqCst);
        Self {
            payload: self.payload,
            clones: self.clones.clone(),
        }
    }
}

#[test]
fn single_consumer_pipeline_keeps_final_output_without_cloning_intermediates() {
    let clones = Arc::new(AtomicUsize::new(0));
    let mut graph = GraphBuilder::new("zero-clone");
    let first = graph.node("first", |x: Counted| x).unwrap();
    let second = graph.node("second", |x: Counted| x.payload * 2).unwrap();
    graph.connect(first.output(), second.inlet()).unwrap();
    let input = graph.expose_input("request", first.inlet()).unwrap();
    let trigger = graph.trigger("run", [input]).unwrap();
    let engine = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap();
    let report = engine
        .fire(
            trigger.get(),
            vec![Value::local(Counted {
                payload: 21,
                clones: clones.clone(),
            })],
        )
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publication_events().len(), 2);
    let outputs: Vec<_> = report
        .publications()
        .map(|p| p.borrow::<i64>().copied().unwrap())
        .collect();
    assert_eq!(outputs, vec![42]);
    assert_eq!(clones.load(Ordering::SeqCst), 0);
}

#[test]
fn local_atomic_layout_accepts_move_only_values() {
    struct MoveOnly(Vec<u8>);
    let mut graph = GraphBuilder::new("move-only-atomic");
    let node = graph
        .node(
            "bytes",
            Node::with_layout(
                |v: MoveOnly| v.0.len(),
                phenix_graph::Layout::<MoveOnly>::atomic(),
            ),
        )
        .unwrap();
    let input = graph
        .expose_input("request", node.local_slot::<MoveOnly>("value"))
        .unwrap();
    let trigger = graph.trigger("run", [input]).unwrap();
    let report = Deployment::new(graph.compile().unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(
            trigger.get(),
            vec![Value::local(MoveOnly(vec![1, 2, 3, 4]))],
        )
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report.publications().last().unwrap().borrow::<usize>(),
        Some(&4)
    );
}

#[test]
fn metadata_only_reporting_never_retains_payloads() {
    let mut graph = GraphBuilder::new("metadata-only");
    let node = graph.node("double", |x: i64| x * 2).unwrap();
    let input = graph.expose_input("request", node.inlet()).unwrap();
    let trigger = graph.trigger("run", [input]).unwrap();
    let report = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::MetadataOnly)
        .build()
        .unwrap()
        .fire(trigger.get(), vec![Value::local(7_i64)])
        .unwrap();
    assert!(report.success());
    assert_eq!(report.publication_events().len(), 1);
    assert_eq!(report.publications().count(), 0);
}

#[test]
fn legacy_report_can_still_retain_payloads_for_observation() {
    let mut graph = GraphBuilder::new("legacy-report");
    let node = graph.node("double", |x: i64| x * 2).unwrap();
    let input = graph.expose_input("request", node.inlet()).unwrap();
    let trigger = graph.trigger("run", [input]).unwrap();
    let report = Deployment::new(graph.compile().unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(trigger.get(), vec![Value::local(7_i64)])
        .unwrap();
    assert!(report.success());
    assert_eq!(report.publication_events().len(), 1);
    assert_eq!(
        report.publications().next().unwrap().borrow::<i64>(),
        Some(&14)
    );
    assert_eq!(report.retention(), ReportRetention::RetainPayloads);
}

#[test]
fn local_type_id_cannot_be_forged_with_a_matching_schema_string() {
    assert_eq!(
        Value::local(3_i64).identity(),
        TypeIdentity::Local(TypeId::of::<i64>())
    );
    let wire = Value::from_wire_parts(type_name::<i64>().into(), vec![]);
    assert_eq!(
        wire.identity(),
        TypeIdentity::Wire(type_name::<i64>().into())
    );
    assert_ne!(wire.identity(), Value::local(3_i64).identity());
    assert_ne!(
        InputSlot::local::<i64>(0).identity(),
        InputSlot::new(0, type_name::<i64>().into(), BufferMode::Deferred).identity()
    );
    assert_ne!(
        OutputSlot::local::<i64>(0).identity(),
        OutputSlot::new(0, type_name::<i64>().into()).identity()
    );

    let mut graph = GraphBuilder::new("local-input");
    let node = graph.node("double", |x: i64| x * 2).unwrap();
    let input = graph.expose_input("request", node.inlet()).unwrap();
    let trigger = graph.trigger("run", [input]).unwrap();
    let engine = Deployment::new(graph.compile().unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap();
    assert!(engine.fire(trigger.get(), vec![wire]).is_err());
}

struct Reject;
impl EdgeAdapter for Reject {
    fn prepare(&self, _: &Value) -> Result<Value, String> {
        Err("preparation failed".into())
    }
}
#[test]
fn edge_adapter_is_a_preparation_step_not_a_remote_delivery() {
    let mut graph = GraphBuilder::new("adapter");
    let first = graph.node("first", |x: i64| x + 1).unwrap();
    let second = graph.node("second", |x: i64| x + 1).unwrap();
    graph.connect(first.output(), second.inlet()).unwrap();
    let input = graph.expose_input("request", first.inlet()).unwrap();
    let trigger = graph.trigger("run", [input]).unwrap();
    let compiled = graph.compile().unwrap();
    let mut deployment =
        Deployment::new(compiled).report_retention(ReportRetention::RetainPayloads);
    deployment
        .edge_adapter("first", "value", "second", "run", "value", Arc::new(Reject))
        .unwrap();
    let engine = deployment.build().unwrap();
    let report = engine
        .fire(trigger.get(), vec![Value::local(1_i64)])
        .unwrap();
    assert!(!report.success());
    assert!(
        report
            .error_messages()
            .iter()
            .any(|m| m.contains("preparation failed"))
    );
}

#[cfg(feature = "derive")]
mod layouts {
    use super::*;
    use phenix_graph::{Layout, LocalPortLayout};
    #[derive(Debug)]
    struct MoveOnly {
        id: i32,
    }
    #[derive(LocalPortLayout)]
    struct Envelope {
        #[port(atomic)]
        payload: MoveOnly,
    }
    #[derive(LocalPortLayout)]
    struct Nested {
        envelope: Envelope,
    }

    #[test]
    fn nonclone_fields_can_be_reconstructed_by_consuming_selected_values() {
        let mut graph = GraphBuilder::new("move-only-fields");
        let node = graph
            .node(
                "process",
                Node::with_layout(
                    |x: Nested| x.envelope.payload.id + 5,
                    Layout::<Nested>::deep(),
                ),
            )
            .unwrap();
        let input = graph
            .expose_input(
                "payload",
                node.field(Nested::SLOT_ENVELOPE.then(Envelope::SLOT_PAYLOAD)),
            )
            .unwrap();
        let trigger = graph.trigger("run", [input]).unwrap();
        let report = Deployment::new(graph.compile().unwrap())
            .store(Store::new())
            .report_retention(ReportRetention::RetainPayloads)
            .build()
            .unwrap()
            .fire(trigger.get(), vec![Value::local(MoveOnly { id: 7 })])
            .unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        assert_eq!(
            report.publications().last().unwrap().borrow::<i32>(),
            Some(&12)
        );
    }

    #[derive(Debug)]
    struct SharedPayload {
        id: i32,
    }
    #[derive(LocalPortLayout)]
    struct SharedEnvelope {
        #[port(atomic)]
        payload: Arc<SharedPayload>,
    }
    #[test]
    fn explicit_arc_fields_allow_shared_fanout_without_deep_clone() {
        let mut graph = GraphBuilder::new("shared-fields");
        let producer = graph
            .node("producer", |x: i32| Arc::new(SharedPayload { id: x }))
            .unwrap();
        let first = graph
            .node(
                "a",
                Node::with_layout(
                    |x: SharedEnvelope| x.payload.id * 2,
                    Layout::<SharedEnvelope>::fields(),
                ),
            )
            .unwrap();
        let second = graph
            .node(
                "b",
                Node::with_layout(
                    |x: SharedEnvelope| x.payload.id * 3,
                    Layout::<SharedEnvelope>::fields(),
                ),
            )
            .unwrap();
        graph
            .connect(producer.output(), first.field(SharedEnvelope::SLOT_PAYLOAD))
            .unwrap();
        graph
            .connect(
                producer.output(),
                second.field(SharedEnvelope::SLOT_PAYLOAD),
            )
            .unwrap();
        let ingress = graph.expose_input("request", producer.inlet()).unwrap();
        let trigger = graph.trigger("run", [ingress]).unwrap();
        let report = Deployment::new(graph.compile().unwrap())
            .store(Store::new())
            .report_retention(ReportRetention::RetainPayloads)
            .build()
            .unwrap()
            .fire(trigger.get(), vec![Value::local(5_i32)])
            .unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        let mut results: Vec<_> = report
            .publications()
            .filter_map(|p| p.borrow::<i32>().copied())
            .collect();
        results.sort_unstable();
        assert_eq!(results, vec![10, 15]);
    }
}

/// Small independent semantic evaluator for generated fan-out graphs. These
/// cases include variable topologies, branch sharing and independent cohorts.
/// Seeds are fixed and printed on failure for reproducible counterexamples.
#[test]
fn generated_acyclic_graphs_match_an_independent_reference_model() {
    for seed in 0_u64..256 {
        let mut rng = seed.wrapping_mul(0x9e3779b97f4a7c15).wrapping_add(1);
        let mut next = || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        let n = 2 + (next() % 7) as usize;
        let input_value = (next() % 17) as i64;
        let mut reference: Vec<i64> = Vec::new();
        let mut outputs: Vec<phenix_graph::OutputRef<i64>> = Vec::new();
        let mut first_input: Option<phenix_graph::Input<i64>> = None;
        let mut parents = Vec::new();
        let mut graph = GraphBuilder::new(format!("model-{seed}"));
        for i in 0..n {
            let action = (next() % 3) as u8;
            let parent = if i == 0 {
                None
            } else {
                Some((next() as usize) % i)
            };
            let step = move |x: i64| match action {
                0 => x + 1,
                1 => x.wrapping_mul(2),
                _ => x - 3,
            };
            let node = graph.node(format!("n{i}"), step).unwrap();
            let previous = parent.map(|index| reference[index]).unwrap_or(input_value);
            reference.push(match action {
                0 => previous + 1,
                1 => previous.wrapping_mul(2),
                _ => previous - 3,
            });
            if let Some(parent) = parent {
                graph
                    .connect(outputs[parent].clone(), node.inlet())
                    .unwrap();
            } else {
                first_input = Some(node.inlet());
            }
            parents.push(parent);
            outputs.push(node.output());
        }
        let ingress = graph
            .expose_input("request", first_input.expect("root input"))
            .unwrap();
        let trigger = graph.trigger("run", [ingress]).unwrap();
        let compiled = graph.compile().unwrap();
        let expected = (0..n)
            .map(|i| {
                let name = format!("n{i}");
                let id = match compiled.find(&name).unwrap() {
                    phenix_graph::FindResult::Node(node) => node.id(),
                    _ => panic!("expected node"),
                };
                (id, reference[i])
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let engine = Deployment::new(compiled)
            .store(Store::new())
            .report_retention(ReportRetention::RetainPayloads)
            .build()
            .unwrap();
        let report = engine
            .fire(trigger.get(), vec![Value::local(input_value)])
            .unwrap();
        assert!(
            report.success(),
            "seed={seed}, parents={parents:?}: {:?}",
            report.error_messages()
        );
        assert_eq!(report.publication_events().len(), n, "seed={seed}");
        let mut observed = std::collections::BTreeMap::new();
        for published in report.publications() {
            observed.insert(
                published.source_node_id(),
                *published
                    .borrow::<i64>()
                    .expect("expected local i64 output"),
            );
        }
        assert_eq!(observed, expected, "seed={seed}, parents={parents:?}");
    }
}
