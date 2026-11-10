use phenix_graph::*;

fn chain(name: &str) -> TypedModulePackage<SinglePort<i64, LocalAtomic<i64>>, i64> {
    let mut module = Module::new(
        name,
        InputInterface::single(Layout::<i64>::atomic()).unwrap(),
    )
    .unwrap();
    let a = module.node("a", |x: i64| x + 1).unwrap();
    let b = module.node("b", |x: i64| x * 2).unwrap();
    module.expose(module.inlet(), a.inlet()).unwrap();
    module.connect(a.output(), b.inlet()).unwrap();
    module.finish(b.output()).unwrap()
}

fn run(graph: GraphBuilder, count: usize, expected: i64) {
    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.physical_node_count(), count);
    let report = Deployment::new(compiled)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire("run", vec![Value::local(3_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report.publications().next().unwrap().borrow::<i64>(),
        Some(&expected)
    );
}

#[test]
fn direct_chain_node_module_node_retargets_edges_and_public_exports() {
    let package = chain("implementation");
    let mut graph = GraphBuilder::new("root");
    let source = graph.node("source", |x: i64| x).unwrap();
    let target = graph.node("target", |x: i64| (x + 1) * 2).unwrap();
    let original_input = target.inlet();
    let original_output = target.output();
    let sink = graph.node("sink", |x: i64| x + 10).unwrap();
    graph.connect(source.output(), target.inlet()).unwrap();
    graph.connect(target.output(), sink.inlet()).unwrap();
    let input = graph.expose_input("input", source.inlet()).unwrap();
    graph.expose_output("output", sink.output()).unwrap();
    graph.trigger("run", [input]).unwrap();
    graph.replace("target", &package).unwrap();
    assert!(graph.expose_input("stale", target.inlet()).is_err());
    assert!(target.input(Layout::atomic()).is_err());
    run(graph.clone(), 4, 18);
    let old = graph
        .discover("target")
        .unwrap()
        .output::<i64, LocalValue>()
        .unwrap();
    graph
        .replace("target", NodeTemplate::local_function(|x: i64| (x + 1) * 2))
        .unwrap();
    assert!(graph.expose_output("stale", old).is_err());
    assert!(
        graph
            .expose_input("stale_original", original_input)
            .is_err()
    );
    assert!(
        graph
            .expose_output("stale_original", original_output)
            .is_err()
    );
    // An unrelated retained handle remains attached to the original pending map.
    source.input(Layout::atomic()).unwrap();
    run(graph, 3, 18);
}

#[test]
fn boundary_input_output_and_repeated_nested_instances_survive_substitution() {
    let leaf = chain("leaf");
    let mut outer = Module::new(
        "outer",
        InputInterface::single(Layout::<i64>::atomic()).unwrap(),
    )
    .unwrap();
    let original = outer.node("operation", |x: i64| (x + 1) * 2).unwrap();
    outer.expose(outer.inlet(), original.inlet()).unwrap();
    // Finish only after substitution, using fresh discovery for the output.
    outer.replace("operation", &leaf).unwrap();
    let output = outer
        .discover("operation")
        .unwrap()
        .output::<i64, LocalValue>()
        .unwrap();
    let outer = outer.finish(output).unwrap();
    let mut graph = GraphBuilder::new("root");
    let a = graph.instantiate("a", &outer).unwrap();
    let b = graph.instantiate("b", &outer).unwrap();
    graph.connect(a.output(), b.inlet()).unwrap();
    let input = graph.expose_input("input", a.inlet()).unwrap();
    graph.expose_output("output", b.output()).unwrap();
    graph.trigger("run", [input]).unwrap();
    run(graph.clone(), 4, 18);
    // Replacing only one alias does not alter the shared definition or the other alias.
    graph
        .replace("a", NodeTemplate::local_function(|x: i64| (x + 1) * 2))
        .unwrap();
    run(graph, 3, 18);
}

#[test]
fn invalid_cross_kind_replacement_is_atomic_including_pending_handles() {
    let mut invalid = GraphBuilder::new("invalid");
    let node = invalid.node("valid", |x: i64| x).unwrap();
    invalid.expose_input("run", node.inlet()).unwrap();
    invalid.expose_output("value", node.output()).unwrap();
    invalid
        .add(
            "bad",
            NodeTemplate::with_action(
                0,
                vec![Port::all(0, vec![]).named("empty")],
                OutputSlot::local::<i64>(0).named("value"),
                |_: InvocationCtx| Ok(vec![emit_local(0, 1_i64)]),
            ),
        )
        .unwrap();
    let invalid = invalid.module();
    let mut graph = GraphBuilder::new("root");
    let target = graph.node("target", |x: i64| x).unwrap();
    let input = graph.expose_input("input", target.inlet()).unwrap();
    graph.expose_output("output", target.output()).unwrap();
    graph.trigger("run", [input]).unwrap();
    assert!(graph.replace("target", invalid).is_err());
    // Failed compilation neither removes the node nor revokes its layout handle.
    target.input(Layout::atomic()).unwrap();
    run(graph, 1, 3);
}

#[test]
fn names_representation_and_complete_layout_are_not_inferred_or_coerced() {
    let mut graph = GraphBuilder::new("root");
    let target = graph.node("target", |x: i64| x).unwrap();
    let mut wrong = GraphBuilder::new("wrong");
    let node = wrong.node("node", |x: i64| x).unwrap();
    wrong.expose_input("renamed", node.inlet()).unwrap();
    wrong.expose_output("value", node.output()).unwrap();
    assert!(graph.replace("target", wrong.module()).is_err());
    let mut wrong = Module::new(
        "encoded",
        InputInterface::<SinglePort<i64, Atomic<i64>>, EncodedValue>::single(
            Layout::<i64>::wire_atomic(),
        )
        .unwrap(),
    )
    .unwrap();
    let node = wrong.node("node", Node::wire(|x: i64| x)).unwrap();
    wrong.expose(wrong.inlet(), node.inlet()).unwrap();
    assert!(
        graph
            .replace("target", wrong.finish(node.output()).unwrap())
            .is_err()
    );
    assert!(graph.expose_input("still_valid", target.inlet()).is_ok());
}

#[test]
fn encoded_substitution_uses_existing_decoder_and_flattening() {
    let mut module = Module::new(
        "wire",
        InputInterface::<SinglePort<i64, Atomic<i64>>, EncodedValue>::single(
            Layout::<i64>::wire_atomic(),
        )
        .unwrap(),
    )
    .unwrap();
    let a = module.node("a", Node::wire(|x: i64| x + 1)).unwrap();
    let b = module.node("b", Node::wire(|x: i64| x * 2)).unwrap();
    module.expose(module.inlet(), a.inlet()).unwrap();
    module.connect(a.output(), b.inlet()).unwrap();
    let package = module.finish(b.output()).unwrap();
    let mut graph = GraphBuilder::new("root");
    let node = graph
        .node("target", Node::wire(|x: i64| (x + 1) * 2))
        .unwrap();
    let input = graph.expose_input("input", node.inlet()).unwrap();
    graph.expose_output("output", node.output()).unwrap();
    graph.trigger("run", [input]).unwrap();
    graph.replace("target", &package).unwrap();
    for graph in [graph.clone(), {
        graph
            .replace("target", NodeTemplate::function(|x: i64| (x + 1) * 2))
            .unwrap();
        graph
    }] {
        let report = Deployment::new(graph.compile().unwrap())
            .report_retention(ReportRetention::TerminalOutputs)
            .build()
            .unwrap()
            .fire("run", vec![Value::of(3_i64)])
            .unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        assert_eq!(
            report
                .publications()
                .next()
                .unwrap()
                .value::<i64>()
                .unwrap(),
            8
        );
    }
}

fn contract() -> GraphContract {
    GraphContract::new(
        "service",
        InterfaceShape::node(NodeSignature {
            input: DeclaredValue::local::<i64>(),
            output: DeclaredValue::local::<i64>(),
        })
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn claims_are_explicit_requirements_survive_and_provider_origin_changes() {
    let contract = contract();
    let original = GraphOrigin::default();
    let provider = GraphOrigin::default();
    let mut module = GraphBuilder::new("claimed");
    let node = module.node("implementation", |x: i64| x).unwrap();
    module.expose_input("run", node.inlet()).unwrap();
    module.expose_output("value", node.output()).unwrap();
    module.implements_public(&contract).unwrap();
    module.require_contract("", &contract).unwrap();
    let package = module.module();
    let mut graph = GraphBuilder::new("root");
    graph.with_origin(&original, |g| {
        g.node("target", |x: i64| x).unwrap();
    });
    graph.implements("target", &contract).unwrap();
    graph.require_contract("target", &contract).unwrap();
    assert!(graph.replace("target", chain("unclaimed")).is_err());
    graph
        .with_origin(&provider, |g| g.replace("target", &package))
        .unwrap();
    let compiled = graph.clone().compile().unwrap();
    assert_eq!(compiled.find("target").unwrap().origin(), Some(&provider));
    assert!(graph.bind_contract("target", &contract).is_ok());
    assert!(
        graph
            .replace("target", NodeTemplate::local_function(|x: i64| x))
            .is_err()
    );
    let mut replacement = NodeTemplate::local_function(|x: i64| x);
    replacement.claims.push(contract.clone());
    graph
        .with_origin(&original, |g| g.replace("target", replacement))
        .unwrap();
    assert!(graph.bind_contract("target", &contract).is_ok());
    let compiled = graph.clone().compile().unwrap();
    assert_eq!(compiled.find("target").unwrap().origin(), Some(&original));
    // The package's root request was lifted, not dropped during replacement.
    assert!(graph.replace("target", chain("still_unclaimed")).is_err());
}

#[test]
fn requirements_on_removed_private_units_fail_atomically() {
    let contract = contract();
    let mut module = GraphBuilder::new("private_request");
    let node = module.node("private", |x: i64| x).unwrap();
    module.implements("private", &contract).unwrap();
    module.require_contract("private", &contract).unwrap();
    module.expose_input("run", node.inlet()).unwrap();
    module.expose_output("value", node.output()).unwrap();
    let mut graph = GraphBuilder::new("root");
    graph.instance("target", &module.module()).unwrap();
    let output = graph
        .discover("target")
        .unwrap()
        .output::<i64, LocalValue>()
        .unwrap();
    assert!(
        graph
            .replace("target", NodeTemplate::local_function(|x: i64| x))
            .is_err()
    );
    assert!(graph.expose_output("value", output).is_ok());
    assert_eq!(graph.compile().unwrap().physical_node_count(), 1);
}

#[test]
fn patch_substitution_records_retargeted_connections_and_revisions() {
    let mut base = PatchBuilder::new("base");
    base.node("target", |x: i64| x).node("sink", |x: i64| x);
    base.connect("target", "sink").unwrap();
    base.expose_input("input", "target");
    base.expose_output("output", "sink");
    base.trigger("run", &["input"]);
    let mut substitute = PatchBuilder::new("substitute");
    substitute
        .after("base")
        .replace("target", chain("patch_module"));
    let mut back = PatchBuilder::new("back");
    back.after("substitute")
        .replace("target", NodeTemplate::local_function(|x: i64| (x + 1) * 2));
    let mut planner = PatchPlanner::new();
    planner.add_patch(base).unwrap();
    planner.add_patch(substitute).unwrap();
    planner.add_patch(back).unwrap();
    let mut assembly = GraphAssembly::new("root");
    planner.apply(&mut assembly).unwrap();
    let revisions = assembly.revisions();
    assert!(
        revisions[1]
            .changes
            .contains(&AssemblyChange::InstanceAdded("target".into()))
    );
    assert!(
        revisions[2]
            .changes
            .contains(&AssemblyChange::NodeAdded("target".into()))
    );
    assert!(
        revisions[1]
            .changes
            .iter()
            .any(|c| matches!(c, AssemblyChange::ConnectionRemoved(_)))
    );
    let compiled = assembly.finish().unwrap();
    assert_eq!(compiled.graph.physical_node_count(), 2);
}

#[cfg(feature = "derive")]
#[test]
fn structured_multichild_substitution_keeps_internal_edge_and_frontier() {
    #[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
    struct Pair {
        left: i64,
        right: i64,
    }
    let mut module = Module::new(
        "structured",
        InputInterface::single(Layout::<Pair>::fields()).unwrap(),
    )
    .unwrap();
    let triple = module.node("triple", |x: i64| x * 3).unwrap();
    let sum = module
        .node("sum", |p: Pair| p.left + p.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    module
        .expose(
            module.inlet().field(Pair::SLOT_LEFT),
            sum.inlet().field(Pair::SLOT_LEFT).target().unwrap(),
        )
        .unwrap();
    module
        .expose(module.inlet().field(Pair::SLOT_RIGHT), triple.inlet())
        .unwrap();
    module
        .connect(
            triple.output(),
            sum.inlet().field(Pair::SLOT_RIGHT).target().unwrap(),
        )
        .unwrap();
    let package = module.finish(sum.output()).unwrap();
    let mut graph = GraphBuilder::new("root");
    let node = graph
        .node("target", |p: Pair| p.left + 3 * p.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    let left = graph
        .expose_input(
            "left",
            node.inlet().field(Pair::SLOT_LEFT).target().unwrap(),
        )
        .unwrap();
    let right = graph
        .expose_input(
            "right",
            node.inlet().field(Pair::SLOT_RIGHT).target().unwrap(),
        )
        .unwrap();
    graph.expose_output("value", node.output()).unwrap();
    graph.trigger("run", [left, right]).unwrap();
    graph.replace("target", &package).unwrap();
    assert!(
        graph
            .discover("target")
            .unwrap()
            .inlets::<Pair, LocalValue>()
            .port(InletPort::<Pair, Pair>::new("run"))
            .target()
            .is_err()
    );
    for graph in [graph.clone(), {
        let template = std::sync::Arc::new(Node::with_layout(
            |p: Pair| p.left + 3 * p.right,
            Layout::<Pair>::fields(),
        ))
        .template()
        .unwrap();
        graph.replace("target", template).unwrap();
        graph
    }] {
        let report = Deployment::new(graph.compile().unwrap())
            .report_retention(ReportRetention::TerminalOutputs)
            .build()
            .unwrap()
            .fire("run", vec![Value::local(2_i64), Value::local(4_i64)])
            .unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        assert_eq!(
            report.publications().next().unwrap().borrow::<i64>(),
            Some(&14)
        );
    }
}

#[test]
fn unordered_action_replacements_commute_even_on_opposite_ends_of_one_edge() {
    for (reverse, cross_kind) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut base = PatchBuilder::new("base");
        base.node("a", |x: i64| x).node("b", |x: i64| x);
        base.connect("a", "b").unwrap();
        base.expose_input("input", "a")
            .expose_output("output", "b")
            .trigger("run", &["input"]);
        let mut a = PatchBuilder::new(if reverse { "z" } else { "a" });
        a.after("base");
        if cross_kind {
            a.replace("a", chain("a_module"));
        } else {
            a.replace("a", NodeTemplate::local_function(|x: i64| (x + 1) * 2));
        }
        let mut b = PatchBuilder::new(if reverse { "a" } else { "z" });
        b.after("base");
        if cross_kind {
            b.replace("b", chain("b_module"));
        } else {
            b.replace("b", NodeTemplate::local_function(|x: i64| (x + 1) * 2));
        }
        let mut planner = PatchPlanner::new();
        planner.add_patch(base).unwrap();
        planner.add_patch(a).unwrap();
        planner.add_patch(b).unwrap();
        let (built, resolution) = planner.build("root").unwrap();
        assert_eq!(resolution.selected.len(), 3);
        assert!(resolution.superseded.is_empty());
        let report = Deployment::new(built.graph)
            .report_retention(ReportRetention::TerminalOutputs)
            .build()
            .unwrap()
            .fire("run", vec![Value::local(3_i64)])
            .unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        assert_eq!(
            report.publications().next().unwrap().borrow::<i64>(),
            Some(&18)
        );
    }
}

#[test]
fn replacement_commutes_with_disjoint_connection_edit() {
    for cross_kind in [false, true] {
        let mut base = PatchBuilder::new("base");
        for name in ["target", "sink", "other", "old_sink", "new_sink"] {
            base.node(name, |x: i64| x);
        }
        base.connect("target", "sink").unwrap();
        base.connect("other", "old_sink").unwrap();
        base.expose_input("input", "target")
            .expose_output("output", "sink")
            .trigger("run", &["input"]);
        let mut replace = PatchBuilder::new("replace");
        replace.after("base");
        if cross_kind {
            replace.replace("target", chain("disjoint_module"));
        } else {
            replace.replace("target", NodeTemplate::local_function(|x: i64| (x + 1) * 2));
        }
        let mut rewire = PatchBuilder::new("rewire");
        rewire
            .after("base")
            .disconnect("other", "old_sink")
            .unwrap();
        rewire.connect("other", "new_sink").unwrap();
        let mut planner = PatchPlanner::new();
        planner.add_patch(base).unwrap();
        planner.add_patch(replace).unwrap();
        planner.add_patch(rewire).unwrap();
        let (built, _) = planner.build("root").unwrap();
        assert_eq!(
            built.graph.physical_node_count(),
            if cross_kind { 6 } else { 5 }
        );
        let report = Deployment::new(built.graph)
            .report_retention(ReportRetention::TerminalOutputs)
            .build()
            .unwrap()
            .fire("run", vec![Value::local(3_i64)])
            .unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        assert_eq!(
            report.publications().next().unwrap().borrow::<i64>(),
            Some(&8)
        );
    }
}

#[test]
fn incident_topology_claims_conflict_in_both_directions_and_representations() {
    for target in [
        PatchTarget::connection_pattern("target", "sink").unwrap(),
        PatchTarget::connection_pattern("source", "target").unwrap(),
        PatchTarget::edge_nodes("target", "value", "sink", "run", "value"),
        PatchTarget::edge_node_to_instance("source", "value", "target", "run"),
        PatchTarget::edge_instance_to_node("target", "value", "sink", "run", "value"),
        PatchTarget::Splice(PatchAnchor::nodes(
            "source", "value", "target", "run", "value",
        )),
        PatchTarget::Connections,
    ] {
        // Exercise overlap symmetrically: lexical selection order must not matter.
        for reverse in [false, true] {
            let mut replacement = PatchBuilder::new(if reverse { "a" } else { "z" });
            replacement.replace("target", chain("incident_module"));
            let edit = PatchBuilder::custom(
                PatchSpec::new(if reverse { "z" } else { "a" }).writes(target.clone()),
                |_| Ok(()),
            );
            let mut planner = PatchPlanner::new();
            planner.add_patch(replacement).unwrap();
            planner.add_patch(edit).unwrap();
            assert!(matches!(
                planner.resolve(),
                Err(PatchError::Conflict { .. })
            ));
        }
    }
    // Operation names are exact, not prefixes or punctuation-delimited paths.
    let mut replacement = PatchBuilder::new("replace");
    replacement.replace("target", chain("exact_name"));
    let edit = PatchBuilder::custom(
        PatchSpec::new("edge")
            .writes(PatchTarget::connection_pattern("target_suffix", "other").unwrap()),
        |_| Ok(()),
    );
    let mut planner = PatchPlanner::new();
    planner.add_patch(replacement).unwrap();
    planner.add_patch(edit).unwrap();
    assert_eq!(planner.resolve().unwrap().selected.len(), 2);
}

#[test]
fn unordered_module_to_node_replacements_lift_only_their_own_requests() {
    let contract = contract();
    let mut package = GraphBuilder::new("requested");
    let node = package.node("implementation", |x: i64| x).unwrap();
    package.expose_input("run", node.inlet()).unwrap();
    package.expose_output("value", node.output()).unwrap();
    package.implements_public(&contract).unwrap();
    package.require_contract("", &contract).unwrap();
    let package = package.module();
    let base = PatchBuilder::custom(
        PatchSpec::new("base").writes_all([
            PatchTarget::Instance("a".into()),
            PatchTarget::Instance("b".into()),
        ]),
        move |g| {
            g.instance("a", &package)?;
            g.instance("b", &package)?;
            Ok(())
        },
    );
    let mut a = PatchBuilder::new("replace_a");
    let mut template = NodeTemplate::local_function(|x: i64| x);
    template.claims.push(contract.clone());
    a.after("base").replace("a", template.clone());
    let mut b = PatchBuilder::new("replace_b");
    b.after("base").replace("b", template);
    let mut planner = PatchPlanner::new();
    planner.add_patch(base).unwrap();
    planner.add_patch(a).unwrap();
    planner.add_patch(b).unwrap();
    let mut assembly = GraphAssembly::new("root");
    planner.apply(&mut assembly).unwrap();
    assert!(
        assembly.revisions()[1]
            .changes
            .contains(&AssemblyChange::InstanceRebound(String::new()))
    );
    assert!(
        assembly.revisions()[2]
            .changes
            .contains(&AssemblyChange::InstanceRebound(String::new()))
    );
    // Lifted requirements remain final obligations on the replacement nodes.
    assert_eq!(assembly.finish().unwrap().graph.physical_node_count(), 2);
}

#[test]
fn operation_write_does_not_authorize_unrelated_root_requests() {
    for replace in [false, true] {
        let contract = contract();
        let mut package = GraphBuilder::new("requested");
        let node = package.node("implementation", |x: i64| x).unwrap();
        package.expose_input("run", node.inlet()).unwrap();
        package.expose_output("value", node.output()).unwrap();
        package.implements_public(&contract).unwrap();
        package.require_contract("", &contract).unwrap();
        let mut base = PatchBuilder::new("base");
        base.instance("target", package.module());
        let mut planner = PatchPlanner::new();
        planner.add_patch(base).unwrap();
        let mut assembly = GraphAssembly::new("root");
        planner.apply(&mut assembly).unwrap();
        let revisions = assembly.revisions().to_vec();
        let edit = PatchBuilder::custom(
            PatchSpec::new("invalid").writes(PatchTarget::Node("target".into())),
            move |g| {
                if replace {
                    let mut template = NodeTemplate::local_function(|x: i64| x);
                    template.claims.push(contract.clone());
                    g.replace("target", template)?;
                    g.require_contract("unrelated", &contract)
                        .map_err(|e| e.to_string())?;
                } else {
                    // Even the expected request is not a lift if the instance remains.
                    g.require_contract("target", &contract)
                        .map_err(|e| e.to_string())?;
                }
                Ok(())
            },
        );
        let mut planner = PatchPlanner::new();
        planner.add_patch(edit).unwrap();
        assert_eq!(
            planner.apply(&mut assembly),
            Err(PatchError::UndeclaredWrite {
                patch: "invalid".into(),
                target: Box::new(PatchTarget::Graph),
            })
        );
        assert_eq!(assembly.revisions(), revisions);
        assert_eq!(assembly.finish().unwrap().graph.physical_node_count(), 1);
    }
}

#[test]
fn cross_kind_replacement_defers_but_retains_unrelated_final_requests() {
    for to_node in [false, true] {
        for later in ["later", "target_suffix"] {
            let contract = contract();
            let mut graph = GraphBuilder::new("root");
            graph.require_contract(later, &contract).unwrap();
            if to_node {
                graph
                    .instance("target", chain("original").untyped())
                    .unwrap();
                graph
                    .replace("target", NodeTemplate::local_function(|x: i64| x))
                    .unwrap();
            } else {
                graph.node("target", |x: i64| x).unwrap();
                graph.replace("target", chain("replacement")).unwrap();
            }
            // Validation must not consume or discard the deferred request.
            let error = graph
                .clone()
                .compile()
                .err()
                .expect("missing final requirement");
            assert!(
                error.to_string().contains("requested unit is missing"),
                "{error}"
            );
            graph.node(later, |x: i64| x).unwrap();
            assert!(
                graph.clone().compile().is_err(),
                "shape alone must not satisfy the request"
            );
            graph.implements(later, &contract).unwrap();
            assert!(graph.compile().is_ok());
        }
    }
}

#[test]
fn requests_on_the_replacement_or_its_descendants_are_not_deferred() {
    for to_node in [false, true] {
        for path in ["target", "target/missing"] {
            let mut graph = GraphBuilder::new("root");
            if to_node {
                graph
                    .instance("target", chain("original").untyped())
                    .unwrap();
            } else {
                graph.node("target", |x: i64| x).unwrap();
            }
            let output = graph
                .discover("target")
                .unwrap()
                .output::<i64, LocalValue>()
                .unwrap();
            graph.require_contract(path, &contract()).unwrap();
            let result = if to_node {
                graph.replace("target", NodeTemplate::local_function(|x: i64| x))
            } else {
                graph.replace("target", chain("replacement"))
            };
            assert!(result.is_err(), "affected requirement {path} was deferred");
            // Rejected validation must not revoke the original declaration.
            graph.expose_output("value", output).unwrap();
        }
    }
}

#[test]
fn unrelated_import_requests_are_deferred_but_replacement_dependency_requests_are_checked() {
    let contract = contract();
    let mut imported = GraphBuilder::new("imported");
    let node = imported.node("private", |x: i64| x).unwrap();
    imported.expose_input("run", node.inlet()).unwrap();
    imported.expose_output("value", node.output()).unwrap();
    imported.require_contract("private", &contract).unwrap();
    let package = imported.module();

    let mut graph = GraphBuilder::new("root");
    graph.instance("other", &package).unwrap();
    graph.node("target", |x: i64| x).unwrap();
    graph.replace("target", chain("replacement")).unwrap();
    assert!(graph.clone().compile().is_err());
    graph.implements("other/private", &contract).unwrap();
    assert!(graph.compile().is_ok());

    // The same request is relevant when imported under the new implementation,
    // including through another module rather than on its immediate boundary.
    let mut outer = GraphBuilder::new("outer");
    outer.instance("child", &package).unwrap();
    let child = outer.discover("child").unwrap();
    let input = child
        .inlets::<(), LocalValue>()
        .port(InletPort::<(), i64>::new("run"))
        .target()
        .unwrap();
    outer.expose_input("run", input).unwrap();
    outer
        .expose_output("value", child.output::<i64, LocalValue>().unwrap())
        .unwrap();
    let mut graph = GraphBuilder::new("root");
    let node = graph.node("target", |x: i64| x).unwrap();
    assert!(graph.replace("target", outer.module()).is_err());
    node.input(Layout::atomic()).unwrap();
    assert!(graph.compile().is_ok());
}

#[test]
fn unrelated_import_claims_are_still_validated_during_substitution() {
    let mut imported = GraphBuilder::new("invalid_claim");
    let mut template = NodeTemplate::local_function(|x: String| x);
    template.claims.push(contract());
    imported.add("private", template).unwrap();
    let output = imported
        .discover("private")
        .unwrap()
        .output::<String, LocalValue>()
        .unwrap();
    imported.expose_output("value", output).unwrap();
    let mut graph = GraphBuilder::new("root");
    graph.instance("other", &imported.module()).unwrap();
    let node = graph.node("target", |x: i64| x).unwrap();
    assert!(graph.replace("target", chain("replacement")).is_err());
    node.input(Layout::atomic()).unwrap();
}
