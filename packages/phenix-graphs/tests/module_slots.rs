#![cfg(feature = "derive")]

use phenix_graph::{
    ContractValue, Deployment, FindResult, GraphBuilder, ModuleDefBuilder, ModulePackage,
    NodeTemplate, PortLayout, ReportRetention, Value,
};

#[derive(ContractValue, PortLayout, phenix_graph::InputFields)]
#[contract(schema = "test.module-slots.args@1")]
struct Args {
    left: i64,
    right: i64,
}

fn selected_input<T, M>(
    graph: &GraphBuilder,
    operation: &str,
    port: &'static str,
    slot: &'static str,
) -> Result<phenix_graph::Input<T, M>, String>
where
    T: Send + Sync + 'static,
    M: phenix_graph::ValueMode<T>,
{
    let view = graph
        .discover(operation)?
        .inlets::<(), M>()
        .port(phenix_graph::InletPort::<(), T>::new(port));
    if slot.is_empty() {
        view.target()
    } else {
        view.field(phenix_graph::SlotField::<T, T>::new(slot))
            .target()
    }
}

fn calculator(name: &str, alternative: bool) -> ModulePackage {
    let mut graph = GraphBuilder::new(name);
    graph
        .add(
            "private",
            NodeTemplate::flattened(move |args: Args| {
                if alternative {
                    -(-args.left - args.right)
                } else {
                    args.left + args.right
                }
            })
            .unwrap(),
        )
        .unwrap();
    for slot in ["left", "right"] {
        let target =
            selected_input::<i64, phenix_graph::EncodedValue>(&graph, "private", "run", slot)
                .unwrap();
        graph
            .expose_input_field("run", phenix_graph::SlotField::<(), i64>::new(slot), target)
            .unwrap();
    }
    let output = graph
        .discover("private")
        .unwrap()
        .output::<i64, phenix_graph::EncodedValue>()
        .unwrap();
    if alternative {
        graph
            .add("relay", NodeTemplate::function(|value: i64| value))
            .unwrap();
        graph
            .connect(
                output,
                graph
                    .discover("relay")
                    .unwrap()
                    .inlets::<(), phenix_graph::EncodedValue>()
                    .port(phenix_graph::InletPort::<(), i64>::new("run"))
                    .field(phenix_graph::SlotField::<i64, i64>::new("value"))
                    .target()
                    .unwrap(),
            )
            .unwrap();
        graph
            .expose_output(
                "result",
                graph
                    .discover("relay")
                    .unwrap()
                    .output::<i64, phenix_graph::EncodedValue>()
                    .unwrap(),
            )
            .unwrap();
    } else {
        graph.expose_output("result", output).unwrap();
    }
    graph.module()
}

fn nested(name: &str, child: &ModulePackage) -> ModulePackage {
    let mut graph = GraphBuilder::new(name);
    graph.instance("inner", child).unwrap();
    for slot in ["left", "right"] {
        graph
            .expose_input_field(
                "run",
                phenix_graph::SlotField::<(), i64>::new(slot),
                selected_input::<i64, phenix_graph::EncodedValue>(&graph, "inner", "run", slot)
                    .unwrap(),
            )
            .unwrap();
    }
    graph
        .expose_output(
            "result",
            graph
                .discover("inner")
                .unwrap()
                .output::<i64, phenix_graph::EncodedValue>()
                .unwrap(),
        )
        .unwrap();
    graph.module()
}

#[test]
fn nested_calculators_route_real_public_slots_in_a_wave() {
    let first = nested("first-wrapper", &calculator("direct", false));
    let second = nested("second-wrapper", &calculator("alternative", true));
    let mut graph = GraphBuilder::new("root");
    graph.instance("first", &first).unwrap();
    graph.instance("second", &second).unwrap();
    assert!(
        selected_input::<i64, phenix_graph::EncodedValue>(&graph, "first", "run", "value").is_err()
    );
    assert!(
        selected_input::<i64, phenix_graph::EncodedValue>(&graph, "first", "run", "private")
            .is_err()
    );
    assert!(graph.discover("first/inner/private").is_err());
    graph
        .add("left", NodeTemplate::function(|x: i64| x))
        .unwrap();
    graph
        .add("right", NodeTemplate::function(|x: i64| x))
        .unwrap();
    graph
        .add(
            "sum",
            NodeTemplate::flattened(|args: Args| args.left + args.right).unwrap(),
        )
        .unwrap();
    for slot in ["left", "right"] {
        let output = graph
            .discover(slot)
            .unwrap()
            .output::<i64, phenix_graph::EncodedValue>()
            .unwrap();
        graph
            .connect(
                output,
                selected_input::<i64, phenix_graph::EncodedValue>(&graph, "first", "run", slot)
                    .unwrap(),
            )
            .unwrap();
        graph
            .connect(
                graph
                    .discover(slot)
                    .unwrap()
                    .output::<i64, phenix_graph::EncodedValue>()
                    .unwrap(),
                selected_input::<i64, phenix_graph::EncodedValue>(&graph, "second", "run", slot)
                    .unwrap(),
            )
            .unwrap();
    }
    graph
        .connect(
            graph
                .discover("first")
                .unwrap()
                .output::<i64, phenix_graph::EncodedValue>()
                .unwrap(),
            graph
                .discover("sum")
                .unwrap()
                .inlets::<(), phenix_graph::EncodedValue>()
                .port(phenix_graph::InletPort::<(), i64>::new("run"))
                .field(phenix_graph::SlotField::<i64, i64>::new("left"))
                .target()
                .unwrap(),
        )
        .unwrap();
    graph
        .connect(
            graph
                .discover("second")
                .unwrap()
                .output::<i64, phenix_graph::EncodedValue>()
                .unwrap(),
            graph
                .discover("sum")
                .unwrap()
                .inlets::<(), phenix_graph::EncodedValue>()
                .port(phenix_graph::InletPort::<(), i64>::new("run"))
                .field(phenix_graph::SlotField::<i64, i64>::new("right"))
                .target()
                .unwrap(),
        )
        .unwrap();
    let left = graph
        .expose_input(
            "left",
            graph
                .discover("left")
                .unwrap()
                .inlets::<(), phenix_graph::EncodedValue>()
                .port(phenix_graph::InletPort::<(), i64>::new("run"))
                .field(phenix_graph::SlotField::<i64, i64>::new("value"))
                .target()
                .unwrap(),
        )
        .unwrap();
    let right = graph
        .expose_input(
            "right",
            graph
                .discover("right")
                .unwrap()
                .inlets::<(), phenix_graph::EncodedValue>()
                .port(phenix_graph::InletPort::<(), i64>::new("run"))
                .field(phenix_graph::SlotField::<i64, i64>::new("value"))
                .target()
                .unwrap(),
        )
        .unwrap();
    let trigger = graph.trigger("calculate", [left, right]).unwrap();
    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.physical_node_count(), 6);
    for path in ["first", "first/inner", "second", "second/inner"] {
        let Some(FindResult::Module(module)) = compiled.find(path) else {
            panic!("missing {path}")
        };
        let ports: Vec<_> = module.input_ports().collect();
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].port_name(), "run");
        assert_eq!(ports[0].slot_name(), "left");
        assert_eq!(ports[1].slot_name(), "right");
        assert_ne!(ports[0].name(), ports[1].name());
        let left = module.resolved_input_targets(ports[0].id()).unwrap();
        let right = module.resolved_input_targets(ports[1].id()).unwrap();
        assert_eq!(left[0].node_id(), right[0].node_id());
        assert_ne!(left[0].slot_id(), right[0].slot_id());
    }
    let report = Deployment::new(compiled)
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(trigger.get(), vec![Value::of(10_i64), Value::of(3_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().last().unwrap().value::<i64>(), Ok(26));
}

#[test]
fn public_metadata_is_literal_and_grouping_adds_no_barrier() {
    let mut child = GraphBuilder::new("literal");
    let left = child.node("left", |x: i64| x + 1).unwrap();
    let right = child.node("right", |x: i64| x + 2).unwrap();
    child
        .expose_input_field(
            "run.with.dots",
            phenix_graph::SlotField::<(), i64>::new("left.with.dots"),
            left.inlet(),
        )
        .unwrap();
    child
        .expose_input_field(
            "run.with.dots",
            phenix_graph::SlotField::<(), i64>::new("right"),
            right.inlet(),
        )
        .unwrap();
    child.expose_output("result", left.output()).unwrap();
    assert!(
        child
            .expose_input_field(
                "run.with.dots",
                phenix_graph::SlotField::<(), i64>::new("right"),
                right.inlet()
            )
            .is_err()
    );
    let package = child.module();
    let mut graph = GraphBuilder::new("root");
    graph.instance("unit", &package).unwrap();
    assert!(
        selected_input::<i64, phenix_graph::LocalValue>(
            &graph,
            "unit",
            "run",
            "with.dots.left.with.dots",
        )
        .is_err()
    );
    let input = graph
        .expose_input(
            "request",
            selected_input::<i64, phenix_graph::LocalValue>(
                &graph,
                "unit",
                "run.with.dots",
                "left.with.dots",
            )
            .unwrap(),
        )
        .unwrap();
    assert!(
        graph
            .discover("unit")
            .unwrap()
            .inlets::<(), phenix_graph::LocalValue>()
            .port(phenix_graph::InletPort::<(), i64>::new("run.with.dots"))
            .field(phenix_graph::SlotField::<i64, i64>::new("right"))
            .target()
            .is_ok()
    );
    let trigger = graph.trigger("left-only", [input]).unwrap();
    let report = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(trigger.get(), vec![Value::local(7_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publication_events().len(), 1);
    assert_eq!(
        report
            .publications()
            .next()
            .unwrap()
            .borrow::<i64>()
            .copied(),
        Some(8)
    );
}

#[test]
fn replacement_checks_grouped_metadata() {
    let mut child = GraphBuilder::new("grouped");
    let node = child.node("private", |x: i64| x).unwrap();
    child
        .expose_input_field(
            "run",
            phenix_graph::SlotField::<(), i64>::new("left"),
            node.inlet(),
        )
        .unwrap();
    let grouped = child.module();
    let mut replacement = GraphBuilder::new("flat");
    let node = replacement.node("other", |x: i64| x).unwrap();
    replacement
        .expose_input(&grouped.inputs()[0].name, node.inlet())
        .unwrap();
    let replacement = replacement.module();
    let mut graph = GraphBuilder::new("root");
    graph.instance("unit", &grouped).unwrap();
    assert!(graph.replace("unit", &replacement).is_err());
}

#[test]
fn typed_exports_default_and_explicit_metadata() {
    let mut builder = GraphBuilder::new("typed-metadata");
    let node = builder.node("private", |x: i64| x).unwrap();
    builder
        .expose_input("literal.dotted", node.inlet())
        .unwrap();
    builder
        .expose_input_field(
            "run",
            phenix_graph::SlotField::<(), i64>::new("left"),
            node.inlet(),
        )
        .unwrap();
    let package = builder.module();
    assert_eq!(package.inputs()[0].port, "literal.dotted");
    assert_eq!(package.inputs()[0].slot, "value");
    assert_eq!(package.inputs()[1].port, "run");
    assert_eq!(package.inputs()[1].slot, "left");
}

#[test]
fn existing_single_input_apis_keep_value_slot_semantics() {
    let mut child = GraphBuilder::new("single");
    child
        .add("private", NodeTemplate::function(|x: i64| x + 1))
        .unwrap();
    child.expose_input("request.with.dots", "private").unwrap();
    let output = child
        .discover("private")
        .unwrap()
        .output::<i64, phenix_graph::EncodedValue>()
        .unwrap();
    child.expose_output("result", output).unwrap();
    let package = child.module();
    let mut graph = GraphBuilder::new("root");
    graph.instance("unit", &package).unwrap();
    assert!(
        selected_input::<i64, phenix_graph::EncodedValue>(
            &graph,
            "unit",
            "request.with.dots",
            "value",
        )
        .is_ok()
    );
    assert!(
        selected_input::<i64, phenix_graph::EncodedValue>(
            &graph,
            "unit",
            "request.with.dots",
            "value",
        )
        .is_ok()
    );
    assert!(
        selected_input::<i64, phenix_graph::EncodedValue>(&graph, "unit", "request.with.dots", "",)
            .is_ok()
    );
    let target = graph
        .discover("unit")
        .unwrap()
        .inlets::<(), phenix_graph::EncodedValue>()
        .port(phenix_graph::InletPort::<(), i64>::new("request.with.dots"))
        .field(phenix_graph::SlotField::<i64, i64>::new("value"))
        .target()
        .unwrap();
    let input = graph
        .expose_input_field(
            "run",
            phenix_graph::SlotField::<(), i64>::new("left"),
            target,
        )
        .unwrap();
    let trigger = graph.trigger("run", [input]).unwrap();
    let report = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(trigger.get(), vec![Value::of(41_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().next().unwrap().value::<i64>(), Ok(42));
}

#[test]
fn compiler_rejects_duplicate_public_pairs_with_distinct_routing_keys() {
    let mut builder = ModuleDefBuilder::new("duplicate");
    builder
        .register("private", NodeTemplate::function(|x: i64| x))
        .unwrap();
    for (id, name) in [(0, "one"), (1, "two")] {
        builder.input(
            id,
            name,
            <i64 as phenix_graph::ContractValue>::wire_schema(),
            vec![phenix_graph::ModuleInputTarget::NodeNamed {
                node: "private".into(),
                port: "run".into(),
                slot: "value".into(),
            }],
        );
    }
    let mut entry = builder.build();
    entry.definition.inputs[1].port = "one".into();
    let mut catalog = phenix_graph::ModuleCatalog::new();
    catalog.add(entry).unwrap();
    assert!(catalog.compile("duplicate").is_err());
}

const COLLISION_PAIRS: [(&str, &str); 6] = [
    ("run", "left"),
    ("__slot_3:runleft", "value"),
    ("__slot_3:4:runleft", "value"),
    ("__slot_", "value"),
    ("run", "value"),
    ("ordinary", "value"),
];

#[test]
fn escaped_literals_and_grouped_slots_route_independently_through_nested_modules() {
    for reverse in [false, true] {
        let mut child = GraphBuilder::new("collision-child");
        let mut order: Vec<_> = (0..COLLISION_PAIRS.len()).collect();
        if reverse {
            order.reverse();
        }
        for i in order {
            let (port, slot) = COLLISION_PAIRS[i];
            let node = format!("node-{i}");
            child
                .add(
                    &node,
                    NodeTemplate::function(move |x: i64| x + i as i64 * 100),
                )
                .unwrap();
            if slot == "value" {
                // Exercise both symbolic and typed default expose paths.
                if i % 2 == 0 {
                    child.expose_input(port, node.as_str()).unwrap();
                } else {
                    child
                        .expose_input(
                            port,
                            child
                                .discover(&node)
                                .unwrap()
                                .inlets::<(), phenix_graph::EncodedValue>()
                                .port(phenix_graph::InletPort::<(), i64>::new("run"))
                                .field(phenix_graph::SlotField::<i64, i64>::new("value"))
                                .target()
                                .unwrap(),
                        )
                        .unwrap();
                }
                assert!(child.public_input(port).is_ok());
            } else {
                child
                    .expose_input_address(port, slot, node.as_str())
                    .unwrap();
            }
        }
        child
            .expose_output(
                "result",
                child
                    .discover("node-0")
                    .unwrap()
                    .output::<i64, phenix_graph::EncodedValue>()
                    .unwrap(),
            )
            .unwrap();
        let package = child.module();
        let keys: std::collections::BTreeSet<_> =
            package.inputs().iter().map(|input| &input.name).collect();
        assert_eq!(keys.len(), COLLISION_PAIRS.len());
        assert_eq!(
            package
                .inputs()
                .iter()
                .find(|input| input.port == "ordinary")
                .unwrap()
                .name,
            "ordinary"
        );
        for input in package.inputs() {
            if input.port.starts_with("__slot_") {
                assert_ne!(input.name, input.port);
            }
        }
        let mut wrapper = GraphBuilder::new("collision-wrapper");
        wrapper.instance("inner", &package).unwrap();
        for (port, slot) in COLLISION_PAIRS {
            let target =
                selected_input::<i64, phenix_graph::EncodedValue>(&wrapper, "inner", port, slot)
                    .unwrap();
            if slot == "value" {
                assert!(
                    selected_input::<i64, phenix_graph::EncodedValue>(
                        &wrapper, "inner", port, "value",
                    )
                    .is_ok()
                );
                assert!(
                    selected_input::<i64, phenix_graph::EncodedValue>(&wrapper, "inner", port, "",)
                        .is_ok()
                );
            }
            wrapper
                .expose_input_field(port, phenix_graph::SlotField::<(), i64>::new(slot), target)
                .unwrap();
        }
        wrapper
            .expose_output(
                "result",
                wrapper
                    .discover("inner")
                    .unwrap()
                    .output::<i64, phenix_graph::EncodedValue>()
                    .unwrap(),
            )
            .unwrap();
        let wrapper = wrapper.module();
        let mut root = GraphBuilder::new("collision-root");
        root.instance("outer", &wrapper).unwrap();
        let mut inputs = Vec::new();
        for (port, slot) in COLLISION_PAIRS {
            assert!(
                selected_input::<i64, phenix_graph::EncodedValue>(&root, "outer", port, slot,)
                    .is_ok()
            );
            let target =
                selected_input::<i64, phenix_graph::EncodedValue>(&root, "outer", port, slot)
                    .unwrap();
            inputs.push(
                root.expose_input_field(
                    port,
                    phenix_graph::SlotField::<(), i64>::new(slot),
                    target,
                )
                .unwrap(),
            );
        }
        // Literal string trigger lookup must find the escaped default, not a grouped key.
        root.trigger("literal", ["__slot_3:runleft"]).unwrap();
        let trigger = root.trigger("all", inputs).unwrap();
        let report = Deployment::new(root.compile().unwrap())
            .report_retention(ReportRetention::RetainPayloads)
            .build()
            .unwrap()
            .fire(
                trigger.get(),
                (1..=COLLISION_PAIRS.len() as i64).map(Value::of).collect(),
            )
            .unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        let mut values: Vec<_> = report
            .publications()
            .map(|publication| publication.value::<i64>().unwrap())
            .collect();
        values.sort();
        assert_eq!(values, vec![1, 102, 203, 304, 405, 506]);
    }
}

#[test]
fn low_level_escaped_literals_and_grouped_slots_execute_without_key_collisions() {
    for reverse in [false, true] {
        let mut builder = ModuleDefBuilder::new("low-level-collision");
        let mut pairs = COLLISION_PAIRS.to_vec();
        if reverse {
            pairs.reverse();
        }
        for (i, (port, _slot)) in pairs.iter().copied().enumerate() {
            builder
                .register(
                    format!("node-{i}"),
                    NodeTemplate::function(move |x: i64| x + i as i64 * 100),
                )
                .unwrap();
            builder.input(
                i as u16,
                port,
                <i64 as phenix_graph::ContractValue>::wire_schema(),
                vec![phenix_graph::ModuleInputTarget::NodeNamed {
                    node: format!("node-{i}"),
                    port: "run".into(),
                    slot: "value".into(),
                }],
            );
        }
        builder.trigger(
            1,
            (0..pairs.len() as u16)
                .map(phenix_graph::ModuleTriggerTarget::PublicInput)
                .collect(),
        );
        let mut entry = builder.build();
        for (input, (port, slot)) in entry.definition.inputs.iter_mut().zip(&pairs) {
            input.slot = (*slot).into();
            input.name = if *slot == "value" && !port.starts_with("__slot_") {
                (*port).into()
            } else {
                format!("__slot_{}:{}:{port}{slot}", port.len(), slot.len())
            };
        }
        let keys: std::collections::BTreeSet<_> = entry
            .definition
            .inputs
            .iter()
            .map(|input| &input.name)
            .collect();
        assert_eq!(keys.len(), pairs.len());
        let mut catalog = phenix_graph::ModuleCatalog::new();
        catalog.add(entry).unwrap();
        let report = Deployment::new(catalog.compile("low-level-collision").unwrap())
            .report_retention(ReportRetention::RetainPayloads)
            .build()
            .unwrap()
            .fire(1, (1..=pairs.len() as i64).map(Value::of).collect())
            .unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        let mut values: Vec<_> = report
            .publications()
            .map(|publication| publication.value::<i64>().unwrap())
            .collect();
        values.sort();
        assert_eq!(values, vec![1, 102, 203, 304, 405, 506]);
    }
}
