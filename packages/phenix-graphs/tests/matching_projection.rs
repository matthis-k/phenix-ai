#![cfg(feature = "derive")]

use phenix_graph::*;

#[derive(Clone, LocalPortLayout, InputFields)]
struct Pair {
    left: i64,
    right: i64,
}

#[test]
fn matching_projection_preserves_structured_execution_without_a_module_action() {
    let mut module = Module::new(
        "sum",
        InputInterface::single(Layout::<Pair>::fields()).unwrap(),
    )
    .unwrap();
    let child = module
        .node("child", |p: Pair| p.left + p.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();

    // An explicit one-call projection replaces one expose per physical leaf.
    module.expose_matching(child.operation().inlets()).unwrap();
    let sum = module.finish(child.output()).unwrap();

    let mut graph = GraphBuilder::new("root");
    let instance = graph.instantiate("sum", &sum).unwrap();
    let left = graph
        .expose_input(
            "left",
            instance.inlet().field(Pair::SLOT_LEFT).target().unwrap(),
        )
        .unwrap();
    let right = graph
        .expose_input(
            "right",
            instance.inlet().field(Pair::SLOT_RIGHT).target().unwrap(),
        )
        .unwrap();
    graph.trigger("run", [left, right]).unwrap();
    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.physical_node_count(), 1);

    let report = Deployment::new(compiled)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire("run", vec![Value::local(4_i64), Value::local(6_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report.publications().next().unwrap().borrow::<i64>(),
        Some(&10)
    );
}

#[test]
fn matching_projection_supports_fanout_to_independent_children() {
    let mut module = Module::new(
        "fanout",
        InputInterface::single(Layout::<i64>::atomic()).unwrap(),
    )
    .unwrap();
    let first = module.node("first", |x: i64| x + 1).unwrap();
    let second = module.node("second", |x: i64| x + 2).unwrap();

    module.expose_matching(first.operation().inlets()).unwrap();
    module.expose_matching(second.operation().inlets()).unwrap();
    let package = module.finish(first.output()).unwrap();

    let mut graph = GraphBuilder::new("root");
    graph.instantiate("child", &package).unwrap();
    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.physical_node_count(), 2);
    let Some(FindResult::Module(child)) = compiled.find("child") else {
        panic!("missing module instance")
    };
    let inlet = child.input_ports().next().unwrap();
    assert_eq!(child.resolved_input_targets(inlet.id()).unwrap().len(), 2);
}

#[test]
fn duplicate_projection_fails_without_exposing_other_fields() {
    let mut module = Module::new(
        "duplicate",
        InputInterface::single(Layout::<Pair>::fields()).unwrap(),
    )
    .unwrap();
    let child = module
        .node("child", |p: Pair| p.left + p.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();

    module
        .expose(
            module.inlet().field(Pair::SLOT_LEFT),
            child.inlet().field(Pair::SLOT_LEFT).target().unwrap(),
        )
        .unwrap();
    assert!(module.expose_matching(child.operation().inlets()).is_err());

    // No partial commit: the right-hand leaf was not silently projected.
    assert!(module.finish(child.output()).is_err());
}

#[test]
fn matching_projection_rejects_foreign_and_stale_interfaces() {
    let mut module = Module::new(
        "scope",
        InputInterface::single(Layout::<Pair>::fields()).unwrap(),
    )
    .unwrap();
    let child = module
        .node("child", |p: Pair| p.left + p.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();

    let mut foreign = GraphBuilder::new("foreign");
    let elsewhere = foreign
        .node("child", |p: Pair| p.left + p.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    assert!(
        module
            .expose_matching(elsewhere.operation().inlets())
            .is_err()
    );

    // Discovery is type-selectable: a caller may request the wrong owner type.
    // The graph must still compare the entire actual leaf frontier.
    module.node("impostor", |x: i64| x).unwrap();
    let mismatched = module
        .discover("impostor")
        .unwrap()
        .inlets::<SinglePort<Pair, LocalFlatten<Pair>>, LocalValue>();
    assert!(module.expose_matching(mismatched).is_err());

    let stale = child.operation().inlets();
    let child = child.input(Layout::fields()).unwrap();
    assert!(module.expose_matching(stale).is_err());

    // An invalid attempt cannot poison subsequent valid construction.
    module.expose_matching(child.operation().inlets()).unwrap();
    module.finish(child.output()).unwrap();
}

#[test]
fn manual_and_bulk_reject_incompatible_existing_local_exports_without_mutation() {
    for encoded in [false, true] {
        let mut module = Module::new(
            "local",
            InputInterface::single(Layout::<i64>::atomic()).unwrap(),
        )
        .unwrap();
        let child = module.node("child", |x: i64| x).unwrap();
        let existing = if encoded {
            let wrong = module.node("wrong", Node::wire(|x: i64| x)).unwrap();
            module.expose_input("run", wrong.inlet()).unwrap()
        } else {
            let wrong = module.node("wrong", |x: String| x).unwrap();
            module.expose_input("run", wrong.inlet()).unwrap()
        };
        let before = format!("{:?}", module.clone().module().inputs());
        assert!(
            module
                .expose(module.inlet(), child.inlet())
                .err()
                .unwrap()
                .contains("incompatible existing public input")
        );
        assert!(
            module
                .expose_matching(child.operation().inlets())
                .unwrap_err()
                .contains("incompatible existing public input")
        );
        assert_eq!(before, format!("{:?}", module.clone().module().inputs()));
        // The original export token is still usable after both failures.
        module.trigger("original", [existing]).unwrap();
    }
}

#[test]
fn manual_and_bulk_reject_incompatible_existing_encoded_exports_without_mutation() {
    for local in [false, true] {
        let mut module = Module::new(
            "wire",
            InputInterface::<SinglePort<i64, Atomic<i64>>, EncodedValue>::single(
                Layout::<i64>::wire_atomic(),
            )
            .unwrap(),
        )
        .unwrap();
        let child = module.node("child", Node::wire(|x: i64| x)).unwrap();
        let existing = if local {
            let wrong = module.node("wrong", |x: i64| x).unwrap();
            module.expose_input("run", wrong.inlet()).unwrap()
        } else {
            let wrong = module.node("wrong", Node::wire(|x: String| x)).unwrap();
            module.expose_input("run", wrong.inlet()).unwrap()
        };
        let before = format!("{:?}", module.clone().module().inputs());
        assert!(
            module
                .expose(module.inlet(), child.inlet())
                .err()
                .unwrap()
                .contains("incompatible existing public input")
        );
        assert!(
            module
                .expose_matching(child.operation().inlets())
                .unwrap_err()
                .contains("incompatible existing public input")
        );
        assert_eq!(before, format!("{:?}", module.clone().module().inputs()));
        module.trigger("original", [existing]).unwrap();
    }
}

#[test]
fn encoded_manual_and_matching_projection_share_export_identity_and_fanout() {
    let mut module = Module::new(
        "wire_fanout",
        InputInterface::<SinglePort<i64, Atomic<i64>>, EncodedValue>::single(
            Layout::<i64>::wire_atomic(),
        )
        .unwrap(),
    )
    .unwrap();
    let first = module.node("first", Node::wire(|x: i64| x)).unwrap();
    let second = module.node("second", Node::wire(|x: i64| x)).unwrap();
    let original = module.expose(module.inlet(), first.inlet()).unwrap();
    module.expose_matching(second.operation().inlets()).unwrap();
    module.trigger("original", [original.clone()]).unwrap();
    let package = module.finish(first.output()).unwrap();
    let inputs = package.untyped().inputs();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].id, original.id());
    assert_eq!(inputs[0].targets.len(), 2);
    assert_eq!(package.compile().unwrap().physical_node_count(), 2);
}

#[derive(Clone, LocalPortLayout, InputFields)]
struct Triple {
    first: i64,
    second: i64,
    last: i64,
}

#[test]
fn manual_projection_rejects_foreign_selectors_and_foreign_or_stale_destinations() {
    let interface = || InputInterface::single(Layout::<i64>::atomic()).unwrap();
    let mut module = Module::new("local", interface()).unwrap();
    let mut foreign = Module::new("foreign", interface()).unwrap();
    let elsewhere = foreign.node("elsewhere", |x: i64| x).unwrap();
    let child = module.node("child", |x: i64| x).unwrap();
    let stale = child.inlet();
    let child = child.input(Layout::atomic()).unwrap();
    assert!(module.expose(foreign.inlet(), child.inlet()).is_err());
    assert!(module.expose(module.inlet(), elsewhere.inlet()).is_err());
    assert!(module.expose(module.inlet(), stale).is_err());
    assert!(module.clone().module().inputs().is_empty());
    let first = module.expose(module.inlet(), child.inlet()).unwrap();
    assert_eq!(first.id(), 0);
    assert!(module.expose(module.inlet(), child.inlet()).is_err());
    module.trigger("still_valid", [first]).unwrap();
    module.finish(child.output()).unwrap();
}

#[test]
fn bulk_projection_rejects_stale_public_interface_without_appending_fanout() {
    for replace_builder in [false, true] {
        let mut module = Module::new(
            "original",
            InputInterface::single(Layout::<i64>::atomic()).unwrap(),
        )
        .unwrap();
        if replace_builder {
            *module = GraphBuilder::new("replacement");
        } else {
            *module = GraphBuilder::clone(&module);
        }
        let first = module.node("first", |x: i64| x).unwrap();
        let second = module.node("second", |x: i64| x).unwrap();
        let exported = module.expose_input("run", first.inlet()).unwrap();
        let before = format!("{:?}", module.clone().module().inputs());
        assert!(
            module
                .expose_matching(second.operation().inlets())
                .unwrap_err()
                .contains("public selector belongs to another module")
        );
        assert_eq!(before, format!("{:?}", module.clone().module().inputs()));
        assert!(module.expose(module.inlet(), second.inlet()).is_err());
        module.trigger("original_token", [exported]).unwrap();
        let next = module.expose_input("next", second.inlet()).unwrap();
        assert_eq!(next.id(), 1);
    }
}

#[test]
fn late_leaf_failure_commits_neither_earlier_addition_nor_fanout() {
    for incompatible in [false, true] {
        let mut module = Module::new(
            "late",
            InputInterface::single(Layout::<Triple>::fields()).unwrap(),
        )
        .unwrap();
        let child = module
            .node("child", |x: Triple| x.first + x.second + x.last)
            .unwrap()
            .input(Layout::fields())
            .unwrap();
        let other = module.node("other", |x: i64| x).unwrap();
        let second = module
            .expose(module.inlet().field(Triple::SLOT_SECOND), other.inlet())
            .unwrap();
        let last = if incompatible {
            let wrong = module.node("wrong", |x: String| x).unwrap();
            module
                .expose_input_field(
                    "run",
                    SlotField::<Triple, String>::new("last"),
                    wrong.inlet(),
                )
                .unwrap()
        } else {
            module
                .expose(
                    module.inlet().field(Triple::SLOT_LAST),
                    child.inlet().field(Triple::SLOT_LAST).target().unwrap(),
                )
                .unwrap()
        };
        let before = format!("{:?}", module.clone().module().inputs());
        assert!(module.expose_matching(child.operation().inlets()).is_err());
        assert_eq!(before, format!("{:?}", module.clone().module().inputs()));
        module.trigger("unchanged", [second.clone(), last]).unwrap();
        // The planned first leaf did not consume an ID; valid fanout retains its ID.
        let first = module
            .expose(
                module.inlet().field(Triple::SLOT_FIRST),
                child.inlet().field(Triple::SLOT_FIRST).target().unwrap(),
            )
            .unwrap();
        assert_eq!(first.id(), 2);
        let fanout = module
            .expose(
                module.inlet().field(Triple::SLOT_SECOND),
                child.inlet().field(Triple::SLOT_SECOND).target().unwrap(),
            )
            .unwrap();
        assert_eq!(fanout.id(), second.id());
    }
}
