//! End-to-end declaration failures use one fallible input-layout contract.
//! These tests exercise the exact public builder and derive-expanded methods.
use phenix_graph::{
    GraphBuildError, GraphBuilder, InputLayout, InputSlot, Layout, LayoutError,
    LocalFlatten, LocalPortLayoutAt, Node, OwnedSelectedValues, push_layout_slot,
};

struct Oversized;
impl LocalPortLayoutAt<1> for Oversized {
    fn append_local_slots_at(prefix: &str, slots: &mut Vec<InputSlot>) -> Result<(), LayoutError> {
        // Exercise the real allocator, not a fake error-returning layout.
        for i in 0..=usize::from(u16::MAX) + 1 {
            let path = if prefix.is_empty() { format!("field.{i}") }
                else { format!("{prefix}.field.{i}") };
            push_layout_slot(slots, path, InputSlot::local::<i64>)?;
        }
        Ok(())
    }
    fn from_local_owned_at(
        _values: &mut OwnedSelectedValues, _prefix: &str,
    ) -> Result<Self, String> {
        Err("oversized test layout is never admitted".into())
    }
}

#[test]
fn checked_slot_ids_reject_overflow_without_wrapping_or_mutating() {
    let mut slots = vec![InputSlot::local::<i64>(0); usize::from(u16::MAX) + 1];
    let error = push_layout_slot(&mut slots, "extra".into(), InputSlot::local::<i64>)
        .unwrap_err();
    assert_eq!(error, LayoutError::TooManySlots {
        path: "extra".into(), capacity: usize::from(u16::MAX) + 1,
    });
    assert_eq!(slots.len(), usize::from(u16::MAX) + 1);
}

#[test]
fn bounded_layout_returns_a_typed_error_instead_of_panicking() {
    let err = <LocalFlatten<Oversized> as InputLayout>::slots().unwrap_err();
    assert_eq!(err, LayoutError::TooManySlots {
        path: "field.65536".into(), capacity: 65536,
    });
}

#[test]
fn failed_layout_does_not_register_half_a_node() {
    let mut graph = GraphBuilder::new("layout-fail-atomic");
    let error = graph.node("work", Node::with_layout(
        |_x: Oversized| 42_i64, Layout::<Oversized>::fields(),
    )).err().expect("oversized layout must be rejected");
    assert!(matches!(error, GraphBuildError::Layout(LayoutError::TooManySlots { .. })));
    // The same name is still available. Failed layout preparation must not
    // mutate the graph declaration or introduce a phantom input port.
    assert!(graph.node("work", |x: i64| x + 1).is_ok());
}

#[cfg(feature = "derive")]
#[test]
fn derived_local_layout_uses_the_same_checked_slot_allocator() {
    #[derive(phenix_graph::LocalPortLayout)]
    struct TwoFields { first: i64, second: i64 }
    let mut slots = vec![InputSlot::local::<i64>(0); usize::from(u16::MAX)];
    let err = <TwoFields as LocalPortLayoutAt<1>>::append_local_slots_at("", &mut slots)
        .unwrap_err();
    assert_eq!(err, LayoutError::TooManySlots {
        path: "second".into(), capacity: 65536,
    });
    // One newly appended valid slot is allowed in the temporary collector;
    // its failed result is not installed into a Port/GraphBuilder.
    assert_eq!(slots.len(), 65536);
}

#[cfg(feature = "derive")]
#[test]
fn derived_wire_layout_uses_the_same_checked_slot_allocator() {
    #[derive(phenix_graph::PortLayout)]
    struct TwoFields { first: i64, second: i64 }
    let mut slots = vec![InputSlot::deferred::<i64>(0); usize::from(u16::MAX)];
    let err = <TwoFields as phenix_graph::PortLayoutAt<1>>::append_slots_at("", &mut slots)
        .unwrap_err();
    assert_eq!(err, LayoutError::TooManySlots {
        path: "second".into(), capacity: 65536,
    });
    assert_eq!(slots.len(), 65536);
}

#[test]
fn manual_schema_errors_follow_the_same_single_layout_path() {
    use phenix_graph::{AllSameCohort, FromSelected, InputSchema, Port, SelectedValues};
    struct Failing;
    impl InputSchema for Failing {
        fn slots() -> Result<Vec<InputSlot>, LayoutError> {
            Err(LayoutError::TooManySlots { path: "manual".into(), capacity: 65536 })
        }
    }
    impl FromSelected for Failing {
        fn from_values(_: &SelectedValues<'_>) -> Result<Self, String> { Ok(Self) }
    }
    // PortArgs is a blanket implementation; no duplicated schema wrapper.
    let err = Port::for_layout::<Failing>(0, "run", AllSameCohort)
        .err().expect("schema error must propagate");
    assert_eq!(err, LayoutError::TooManySlots {
        path: "manual".into(), capacity: 65536,
    });
}

#[test]
fn local_layout_shape_is_frozen_at_declaration_not_recomputed_per_invocation() {
    use phenix_graph::{Deployment, ReportRetention, Store, Value};
    use std::sync::atomic::{AtomicUsize, Ordering};
    static EXPANSIONS: AtomicUsize = AtomicUsize::new(0);
    struct Counted(i64);
    impl LocalPortLayoutAt<1> for Counted {
        fn append_local_slots_at(prefix: &str, slots: &mut Vec<InputSlot>) -> Result<(), LayoutError> {
            EXPANSIONS.fetch_add(1, Ordering::SeqCst);
            let path = if prefix.is_empty() { "value".into() }
                else { format!("{prefix}.value") };
            push_layout_slot(slots, path, InputSlot::local::<i64>)
        }
        fn from_local_owned_at(
            values: &mut OwnedSelectedValues, prefix: &str,
        ) -> Result<Self, String> {
            let path = if prefix.is_empty() { "value".into() }
                else { format!("{prefix}.value") };
            values.take_local_path::<i64>(&path).map(Self)
        }
    }

    let mut graph = GraphBuilder::new("frozen-local-layout");
    let node = graph.node("sum", Node::with_layout(
        |Counted(x)| x + 1, Layout::<Counted>::fields(),
    )).unwrap();
    let input = graph.expose_input("value", node.local_slot::<i64>("value")).unwrap();
    graph.trigger("run", [input]).unwrap();
    assert_eq!(EXPANSIONS.load(Ordering::SeqCst), 1);
    let engine = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .store(Store::new()).build().unwrap();
    for x in [10_i64, 20_i64] {
        let report = engine.fire("run", vec![Value::local(x)]).unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        assert_eq!(report.publications().last().unwrap().value::<i64>().unwrap(), x + 1);
    }
    assert_eq!(EXPANSIONS.load(Ordering::SeqCst), 1);
}
