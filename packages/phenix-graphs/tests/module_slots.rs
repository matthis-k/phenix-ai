#![cfg(feature = "derive")]

use phenix_graph::{
    ContractValue, Deployment, FindResult, GraphBuilder, ModuleDefBuilder, ModulePackage,
    NodeTemplate, PortLayout, ReportRetention, Value,
};

#[derive(ContractValue, PortLayout)]
#[contract(schema = "test.module-slots.args@1")]
struct Args {
    left: i64,
    right: i64,
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
        let target = graph.wire_input::<i64>("private", "run", slot).unwrap();
        graph.expose_input_slot("run", slot, target).unwrap();
    }
    let output = graph.wire_output::<i64>("private", "value").unwrap();
    if alternative {
        graph
            .add("relay", NodeTemplate::function(|value: i64| value))
            .unwrap();
        graph
            .connect(
                output,
                graph.wire_input::<i64>("relay", "run", "value").unwrap(),
            )
            .unwrap();
        graph
            .expose_output(
                "result",
                graph.wire_output::<i64>("relay", "value").unwrap(),
            )
            .unwrap();
    } else {
        graph.expose_output("result", output).unwrap();
    }
    graph.module()
}

fn nested(name: &str, child: &ModulePackage) -> ModulePackage {
    let mut graph = GraphBuilder::new(name);
    let instance = graph.instance("inner", child).unwrap();
    for slot in ["left", "right"] {
        graph
            .expose_input_slot(
                "run",
                slot,
                instance.input_slot::<i64>("run", slot).unwrap(),
            )
            .unwrap();
    }
    graph
        .expose_output("result", instance.output::<i64>("result").unwrap())
        .unwrap();
    graph.module()
}

#[test]
fn nested_calculators_route_real_public_slots_in_a_wave() {
    let first = nested("first-wrapper", &calculator("direct", false));
    let second = nested("second-wrapper", &calculator("alternative", true));
    let mut graph = GraphBuilder::new("root");
    let a = graph.instance("first", &first).unwrap();
    let b = graph.instance("second", &second).unwrap();
    assert!(a.input::<i64>("run").is_err());
    assert!(a.input_slot::<i64>("run", "private").is_err());
    assert!(
        graph
            .wire_input::<i64>("first/inner/private", "run", "left")
            .is_err()
    );
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
        let output = graph.wire_output::<i64>(slot, "value").unwrap();
        graph
            .connect(
                output,
                graph.wire_input::<i64>("first", "run", slot).unwrap(),
            )
            .unwrap();
        graph
            .connect(
                graph.wire_output::<i64>(slot, "value").unwrap(),
                b.input_slot::<i64>("run", slot).unwrap(),
            )
            .unwrap();
    }
    graph
        .connect(
            a.output::<i64>("result").unwrap(),
            graph.wire_input::<i64>("sum", "run", "left").unwrap(),
        )
        .unwrap();
    graph
        .connect(
            b.output::<i64>("result").unwrap(),
            graph.wire_input::<i64>("sum", "run", "right").unwrap(),
        )
        .unwrap();
    let left = graph
        .expose_input(
            "left",
            graph.wire_input::<i64>("left", "run", "value").unwrap(),
        )
        .unwrap();
    let right = graph
        .expose_input(
            "right",
            graph.wire_input::<i64>("right", "run", "value").unwrap(),
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
        .expose_input_slot("run.with.dots", "left.with.dots", left.inlet())
        .unwrap();
    child
        .expose_input_slot("run.with.dots", "right", right.inlet())
        .unwrap();
    assert!(
        child
            .expose_input_slot("run.with.dots", "right", right.inlet())
            .is_err()
    );
    let package = child.module();
    let mut graph = GraphBuilder::new("root");
    let instance = graph.instance("unit", &package).unwrap();
    assert!(
        instance
            .local_input_slot::<i64>("run", "with.dots.left.with.dots")
            .is_err()
    );
    let input = graph
        .expose_input(
            "request",
            instance
                .local_input_slot::<i64>("run.with.dots", "left.with.dots")
                .unwrap(),
        )
        .unwrap();
    assert!(
        graph
            .local_input::<i64>("unit", "run.with.dots", "right")
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
        .expose_input_slot("run", "left", node.inlet())
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
fn low_level_exports_default_and_explicit_metadata() {
    let mut builder = ModuleDefBuilder::new("low-level");
    let node = builder.register_fn("private", |x: i64| x).unwrap();
    builder
        .export_input("literal.dotted", vec![node.input::<i64>("run", "value")])
        .unwrap();
    builder
        .export_input_slot("run", "left", vec![node.input::<i64>("run", "value")])
        .unwrap();
    let entry = builder.build();
    assert_eq!(entry.definition.inputs[0].port, "literal.dotted");
    assert_eq!(entry.definition.inputs[0].slot, "value");
    assert_eq!(entry.definition.inputs[1].port, "run");
    assert_eq!(entry.definition.inputs[1].slot, "left");
}

#[test]
fn existing_single_input_apis_keep_value_slot_semantics() {
    let mut child = GraphBuilder::new("single");
    child
        .add("private", NodeTemplate::function(|x: i64| x + 1))
        .unwrap();
    child.expose_input("request.with.dots", "private").unwrap();
    let package = child.module();
    let mut graph = GraphBuilder::new("root");
    let instance = graph.instance("unit", &package).unwrap();
    assert!(instance.input::<i64>("request.with.dots").is_ok());
    assert!(
        instance
            .input_slot::<i64>("request.with.dots", "value")
            .is_ok()
    );
    assert!(
        graph
            .wire_input::<i64>("unit", "request.with.dots", "")
            .is_ok()
    );
    let target = graph
        .wire_input::<i64>("unit", "request.with.dots", "value")
        .unwrap();
    let input = graph.expose_input_slot("run", "left", target).unwrap();
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
    let node = builder.register_fn("private", |x: i64| x).unwrap();
    builder
        .export_input("one", vec![node.input::<i64>("run", "value")])
        .unwrap();
    builder
        .export_input("two", vec![node.input::<i64>("run", "value")])
        .unwrap();
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
                            child.wire_input::<i64>(&node, "run", "value").unwrap(),
                        )
                        .unwrap();
                }
                assert!(child.public_input(port).is_ok());
            } else {
                child.expose_input_slot(port, slot, node.as_str()).unwrap();
            }
        }
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
        let inner = wrapper.instance("inner", &package).unwrap();
        for (port, slot) in COLLISION_PAIRS {
            let target = inner.input_slot::<i64>(port, slot).unwrap();
            if slot == "value" {
                assert!(inner.input::<i64>(port).is_ok());
                assert!(wrapper.wire_input::<i64>("inner", port, "").is_ok());
            }
            wrapper.expose_input_slot(port, slot, target).unwrap();
        }
        let wrapper = wrapper.module();
        let mut root = GraphBuilder::new("collision-root");
        let outer = root.instance("outer", &wrapper).unwrap();
        let mut inputs = Vec::new();
        for (port, slot) in COLLISION_PAIRS {
            assert!(outer.input_slot::<i64>(port, slot).is_ok());
            let target = root.wire_input::<i64>("outer", port, slot).unwrap();
            inputs.push(root.expose_input_slot(port, slot, target).unwrap());
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
        for (i, (port, slot)) in pairs.iter().copied().enumerate() {
            let node = builder
                .register_fn(format!("node-{i}"), move |x: i64| x + i as i64 * 100)
                .unwrap();
            let targets = vec![node.input::<i64>("run", "value")];
            if slot == "value" {
                builder.export_input(port, targets).unwrap();
            } else {
                builder.export_input_slot(port, slot, targets).unwrap();
            }
        }
        builder.trigger(
            1,
            (0..pairs.len() as u16)
                .map(phenix_graph::ModuleTriggerTarget::PublicInput)
                .collect(),
        );
        let entry = builder.build();
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
