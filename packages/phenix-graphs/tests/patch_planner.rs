//! End-to-end multi-contributor assembly -> compile -> execution without macros.
use phenix_graph::{
    Deployment, GraphAssembly, GraphBuilder, ModulePackage, NodeTemplate, PatchBuilder, PatchError,
    PatchPlanner, PatchSpec, PatchTarget, ReportRetention, Value,
};

fn execute(assembly: GraphAssembly, value: i64) -> i64 {
    let compiled = assembly.finish().unwrap();
    let report = Deployment::new(compiled.graph)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire(1, vec![Value::local(value)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    *report
        .publications()
        .last()
        .unwrap()
        .borrow::<i64>()
        .unwrap()
}

fn module_input(
    instance: &phenix_graph::ModuleHandle,
    port: &'static str,
) -> Result<phenix_graph::Input<i64, phenix_graph::LocalValue>, String> {
    instance
        .operation()?
        .inlets::<(), phenix_graph::LocalValue>()
        .port(phenix_graph::InletPort::<(), i64>::new(port))
        .field(phenix_graph::SlotField::<i64, i64>::new("value"))
        .target()
}

fn base(planner: &mut PatchPlanner) {
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("base").writes_all([
                PatchTarget::Node("a".into()),
                PatchTarget::Node("b".into()),
                PatchTarget::edge_nodes("a", "value", "b", "run", "value"),
                PatchTarget::PublicInput("request".into()),
                PatchTarget::PublicOutput("result".into()),
                PatchTarget::Trigger("run".into()),
            ]),
            |g| {
                let a = g.node("a", |x: i64| x + 1).map_err(|e| e.to_string())?;
                let b = g.node("b", |x: i64| x * 2).map_err(|e| e.to_string())?;
                g.connect(a.output(), b.inlet())?;
                let input = g.expose_input("request", a.inlet())?;
                g.expose_output("result", b.output())?;
                g.trigger("run", [input])?;
                Ok(())
            },
        ))
        .unwrap();
}

#[test]
fn independent_contributions_commute_and_do_not_depend_on_registration_order() {
    let mut reports = Vec::new();
    for reversed in [false, true] {
        let mut planner = PatchPlanner::new();
        let mut add = |first: bool| {
            if first {
                planner
                    .add_patch(PatchBuilder::custom(
                        PatchSpec::new("replace_a")
                            .after("base")
                            .writes(PatchTarget::Node("a".into())),
                        |g| g.replace("a", NodeTemplate::local_function(|x: i64| x * 3)),
                    ))
                    .unwrap();
            } else {
                planner
                    .add_patch(PatchBuilder::custom(
                        PatchSpec::new("replace_b")
                            .after("base")
                            .writes(PatchTarget::Node("b".into())),
                        |g| g.replace("b", NodeTemplate::local_function(|x: i64| x + 4)),
                    ))
                    .unwrap();
            }
        };
        add(reversed);
        add(!reversed);
        base(&mut planner);
        let mut assembly = GraphAssembly::new("independent");
        let plan = planner.apply(&mut assembly).unwrap();
        assert_eq!(plan.selected, ["base", "replace_a", "replace_b"]);
        assert_eq!(execute(assembly, 7), 25);
        reports.push(plan);
    }
    assert_eq!(reports[0], reports[1]);
}

#[test]
fn dependent_replacements_layer_in_declared_order_even_without_priority() {
    let mut planner = PatchPlanner::new();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("layer_two")
                .after("layer_one")
                .writes(PatchTarget::Node("a".into())),
            |g| g.replace("a", NodeTemplate::local_function(|x: i64| x * 3)),
        ))
        .unwrap();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("layer_one")
                .after("base")
                .writes(PatchTarget::Node("a".into())),
            |g| g.replace("a", NodeTemplate::local_function(|x: i64| x * 2)),
        ))
        .unwrap();
    base(&mut planner);
    let mut assembly = GraphAssembly::new("layers");
    let plan = planner.apply(&mut assembly).unwrap();
    assert_eq!(plan.selected, ["base", "layer_one", "layer_two"]);
    assert!(plan.superseded.is_empty());
    assert_eq!(execute(assembly, 4), 24);
}

#[test]
fn highest_priority_competitor_wins_without_executing_the_loser() {
    let mut planner = PatchPlanner::new();
    base(&mut planner);
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("low")
                .after("base")
                .priority(5)
                .writes(PatchTarget::Node("a".into())),
            |_g| {
                panic!("superseded contribution must not be evaluated");
            },
        ))
        .unwrap();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("high")
                .after("base")
                .priority(20)
                .writes(PatchTarget::Node("a".into())),
            |g| g.replace("a", NodeTemplate::local_function(|x: i64| x * 10)),
        ))
        .unwrap();
    let mut assembly = GraphAssembly::new("priorities");
    let plan = planner.apply(&mut assembly).unwrap();
    assert_eq!(plan.selected, ["base", "high"]);
    assert_eq!(plan.superseded[0].loser, "low");
    assert_eq!(plan.superseded[0].winner, "high");
    assert_eq!(execute(assembly, 3), 60);
}

#[test]
fn a_superseded_intermediate_cannot_suppress_an_unrelated_lower_priority_patch() {
    let mut planner = PatchPlanner::new();
    base(&mut planner);
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("dominant")
                .after("base")
                .priority(30)
                .writes(PatchTarget::Node("a".into())),
            |g| g.replace("a", NodeTemplate::local_function(|x: i64| x * 10)),
        ))
        .unwrap();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("intermediate")
                .after("base")
                .priority(20)
                .writes_all([PatchTarget::Node("a".into()), PatchTarget::Node("b".into())]),
            |_g| {
                panic!("intermediate contribution should be superseded");
            },
        ))
        .unwrap();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("remaining")
                .after("base")
                .priority(10)
                .writes(PatchTarget::Node("b".into())),
            |g| g.replace("b", NodeTemplate::local_function(|x: i64| x + 7)),
        ))
        .unwrap();
    let mut assembly = GraphAssembly::new("intermediate_loser");
    let plan = planner.apply(&mut assembly).unwrap();
    assert_eq!(plan.selected, ["base", "dominant", "remaining"]);
    assert_eq!(execute(assembly, 2), 27);
}

#[test]
fn overlapping_writes_with_equal_priority_or_missing_priority_fail_closed() {
    for priorities in [(None, None), (Some(2), Some(2)), (Some(2), None)] {
        let mut planner = PatchPlanner::new();
        let spec = |name: &str, priority: Option<i32>| {
            let result = PatchSpec::new(name).writes(PatchTarget::Node("same".into()));
            if let Some(p) = priority {
                result.priority(p)
            } else {
                result
            }
        };
        planner
            .add_patch(PatchBuilder::custom(spec("one", priorities.0), |_g| Ok(())))
            .unwrap();
        planner
            .add_patch(PatchBuilder::custom(spec("two", priorities.1), |_g| Ok(())))
            .unwrap();
        assert!(matches!(
            planner.resolve(),
            Err(PatchError::Conflict { .. })
        ));
    }
}

#[test]
fn dependency_cycle_and_missing_dependency_are_rejected_before_any_edits() {
    let mut planner = PatchPlanner::new();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("one").after("two"),
            |_g| Ok(()),
        ))
        .unwrap();
    assert!(matches!(
        planner.resolve(),
        Err(PatchError::MissingDependency { .. })
    ));
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("two").after("one"),
            |_g| Ok(()),
        ))
        .unwrap();
    assert!(matches!(
        planner.resolve(),
        Err(PatchError::DependencyCycle(_))
    ));
}

#[test]
fn a_selected_dependent_of_a_superseded_patch_fails_closed() {
    let mut planner = PatchPlanner::new();
    base(&mut planner);
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("high")
                .after("base")
                .priority(20)
                .writes(PatchTarget::Node("a".into())),
            |_g| Ok(()),
        ))
        .unwrap();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("low")
                .after("base")
                .priority(10)
                .writes(PatchTarget::Node("a".into())),
            |_g| Ok(()),
        ))
        .unwrap();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("dependent")
                .after("low")
                .writes(PatchTarget::Node("b".into())),
            |_g| Ok(()),
        ))
        .unwrap();
    assert!(matches!(
        planner.resolve(),
        Err(PatchError::SupersededDependency { .. })
    ));
}

#[test]
fn failed_or_undeclared_edits_rollback_the_entire_plan_atomically() {
    let mut assembly = GraphAssembly::new("rollback");
    let mut planner = PatchPlanner::new();
    base(&mut planner);
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("illegal")
                .after("base")
                .writes(PatchTarget::Node("a".into())),
            |g| g.replace("b", NodeTemplate::local_function(|x: i64| x * 100)),
        ))
        .unwrap();
    assert!(matches!(
        planner.apply(&mut assembly),
        Err(PatchError::UndeclaredWrite { .. })
    ));
    assert!(assembly.revisions().is_empty());

    let mut planner = PatchPlanner::new();
    base(&mut planner);
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("failure")
                .after("base")
                .writes(PatchTarget::Node("a".into())),
            |g| {
                g.replace("a", NodeTemplate::local_function(|x: i64| x * 9))?;
                Err("deliberately rejected".into())
            },
        ))
        .unwrap();
    assert!(matches!(
        planner.apply(&mut assembly),
        Err(PatchError::Apply { .. })
    ));
    assert!(assembly.revisions().is_empty());
}

fn module(name: &str, factor: i64) -> ModulePackage {
    let mut g = GraphBuilder::new(name);
    let node = g.node("transform", move |x: i64| x * factor).unwrap();
    g.expose_input("input", node.inlet()).unwrap();
    g.expose_output("output", node.output()).unwrap();
    g.module()
}

#[test]
fn three_independent_plugin_contributions_create_and_execute_a_graph() {
    let model = module("model_impl", 2);
    let tool = module("tool_impl", 3);
    let mut planner = PatchPlanner::new();
    // Register deliberately in reverse order: dependencies, not installation
    // or registration order, determine assembly.
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("plugin.tool")
                .after("plugin.model")
                .writes_all([
                    PatchTarget::Instance("tool".into()),
                    PatchTarget::edge_instances("model", "output", "tool", "input"),
                    PatchTarget::PublicOutput("result".into()),
                ]),
            move |g| {
                let instance = g.instance("tool", &tool)?;
                let model_out = g
                    .discover("model")
                    .unwrap()
                    .output::<i64, phenix_graph::LocalValue>()?;
                g.connect(model_out, module_input(&instance, "input")?)?;
                g.expose_output(
                    "result",
                    instance
                        .operation()
                        .unwrap()
                        .output::<i64, phenix_graph::LocalValue>()?,
                )?;
                Ok(())
            },
        ))
        .unwrap();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("plugin.model")
                .after("plugin.base")
                .writes_all([
                    PatchTarget::Instance("model".into()),
                    PatchTarget::edge_node_to_instance("accept", "value", "model", "input"),
                ]),
            move |g| {
                let instance = g.instance("model", &model)?;
                let out = g
                    .discover("accept")
                    .unwrap()
                    .output::<i64, phenix_graph::LocalValue>()?;
                g.connect(out, module_input(&instance, "input")?)?;
                Ok(())
            },
        ))
        .unwrap();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("plugin.base").writes_all([
                PatchTarget::Node("accept".into()),
                PatchTarget::PublicInput("request".into()),
                PatchTarget::Trigger("run".into()),
            ]),
            |g| {
                let accept = g
                    .node("accept", |x: i64| x + 1)
                    .map_err(|e| e.to_string())?;
                let input = g.expose_input("request", accept.inlet())?;
                g.trigger("run", [input])?;
                Ok(())
            },
        ))
        .unwrap();
    let mut assembly = GraphAssembly::new("server");
    let plan = planner.apply(&mut assembly).unwrap();
    assert_eq!(
        plan.selected,
        ["plugin.base", "plugin.model", "plugin.tool"]
    );
    assert_eq!(assembly.revisions().len(), 3);
    assert_eq!(execute(assembly, 2), 18);
}

#[test]
fn later_plugin_rebinds_module_by_public_interface_without_perturbing_consumers() {
    let original = module("model_original", 2);
    let alternative = module("model_alternative", 5);
    let tool = module("tool_processor", 3);
    let mut planner = PatchPlanner::new();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("base").writes_all([
                PatchTarget::Instance("model".into()),
                PatchTarget::Instance("tool".into()),
                PatchTarget::edge_instances("model", "output", "tool", "input"),
                PatchTarget::PublicInput("request".into()),
                PatchTarget::PublicOutput("result".into()),
                PatchTarget::Trigger("run".into()),
            ]),
            move |g| {
                let model = g.instance("model", &original)?;
                let tool = g.instance("tool", &tool)?;
                g.connect(
                    model
                        .operation()
                        .unwrap()
                        .output::<i64, phenix_graph::LocalValue>()?,
                    module_input(&tool, "input")?,
                )?;
                let input = g.expose_input("request", module_input(&model, "input")?)?;
                g.expose_output(
                    "result",
                    tool.operation()
                        .unwrap()
                        .output::<i64, phenix_graph::LocalValue>()?,
                )?;
                g.trigger("run", [input])?;
                Ok(())
            },
        ))
        .unwrap();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("override")
                .after("base")
                .writes(PatchTarget::Instance("model".into())),
            move |g| g.replace("model", &alternative),
        ))
        .unwrap();
    let mut assembly = GraphAssembly::new("server");
    let plan = planner.apply(&mut assembly).unwrap();
    assert_eq!(plan.selected, ["base", "override"]);
    assert_eq!(execute(assembly, 4), 60);
}

#[test]
fn precise_edge_claims_allow_independent_rewires() {
    let mut planner = PatchPlanner::new();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("one")
                .writes(PatchTarget::edge_nodes("a", "value", "b", "run", "value")),
            |_g| Ok(()),
        ))
        .unwrap();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("two")
                .writes(PatchTarget::edge_nodes("c", "value", "d", "run", "value")),
            |_g| Ok(()),
        ))
        .unwrap();
    assert_eq!(planner.resolve().unwrap().selected.len(), 2);
}

#[test]
fn broad_connection_claim_conflicts_with_all_precise_connection_edits() {
    let mut planner = PatchPlanner::new();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("one").writes(PatchTarget::Connections),
            |_g| Ok(()),
        ))
        .unwrap();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("two")
                .writes(PatchTarget::edge_nodes("a", "value", "b", "run", "value")),
            |_g| Ok(()),
        ))
        .unwrap();
    assert!(matches!(
        planner.resolve(),
        Err(PatchError::Conflict { .. })
    ));
}

#[test]
fn randomized_multi_contributor_module_chains_match_independent_reference() {
    // Many deterministic seeds, variable module depth and factors, reverse
    // registration order; all claims and output interfaces are checked through
    // the real resolver, builder, compiler and executor.
    for seed in 1..=64_u64 {
        let stages = (seed % 6 + 1) as usize;
        let mut planner = PatchPlanner::new();
        for i in (0..stages).rev() {
            let name = format!("stage{i}");
            let patch_id = format!("contributor.stage{i}");
            let predecessor = if i == 0 {
                "contributor.base".to_string()
            } else {
                format!("contributor.stage{}", i - 1)
            };
            let factor = ((seed + i as u64) % 4 + 1) as i64;
            let module = module(&format!("module_{seed}_{i}"), factor);
            let edge = if i == 0 {
                PatchTarget::edge_node_to_instance("accept", "value", &name, "input")
            } else {
                PatchTarget::edge_instances(&format!("stage{}", i - 1), "output", &name, "input")
            };
            let mut claims = vec![PatchTarget::Instance(name.clone()), edge];
            if i == stages - 1 {
                claims.push(PatchTarget::PublicOutput("result".into()));
            }
            planner
                .add_patch(PatchBuilder::custom(
                    PatchSpec::new(patch_id)
                        .after(predecessor)
                        .writes_all(claims),
                    move |g| {
                        let instance = g.instance(&name, &module)?;
                        let from = if i == 0 {
                            g.discover("accept")
                                .unwrap()
                                .output::<i64, phenix_graph::LocalValue>()?
                        } else {
                            g.discover(&format!("stage{}", i - 1))?
                                .output::<i64, phenix_graph::LocalValue>()?
                        };
                        g.connect(from, module_input(&instance, "input")?)?;
                        if i == stages - 1 {
                            g.expose_output(
                                "result",
                                instance
                                    .operation()
                                    .unwrap()
                                    .output::<i64, phenix_graph::LocalValue>()?,
                            )?;
                        }
                        Ok(())
                    },
                ))
                .unwrap();
        }
        planner
            .add_patch(PatchBuilder::custom(
                PatchSpec::new("contributor.base").writes_all([
                    PatchTarget::Node("accept".into()),
                    PatchTarget::PublicInput("request".into()),
                    PatchTarget::Trigger("run".into()),
                ]),
                |g| {
                    let accept = g
                        .node("accept", |x: i64| x + 1)
                        .map_err(|e| e.to_string())?;
                    let input = g.expose_input("request", accept.inlet())?;
                    g.trigger("run", [input])?;
                    Ok(())
                },
            ))
            .unwrap();
        let mut assembly = GraphAssembly::new(format!("generated_{seed}"));
        let plan = planner.apply(&mut assembly).unwrap();
        assert_eq!(plan.selected.len(), stages + 1);
        assert!(plan.superseded.is_empty());
        let start = (seed % 7) as i64;
        let expected = (0..stages).fold(start + 1, |value, i| {
            value * ((seed + i as u64) % 4 + 1) as i64
        });
        assert_eq!(execute(assembly, start), expected, "seed {seed}");
    }
}

#[test]
fn one_shot_build_compiles_all_contributions_and_preserves_resolution() {
    let mut planner = PatchPlanner::new();
    base(&mut planner);
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("replace")
                .after("base")
                .writes(PatchTarget::Node("b".into())),
            |g| g.replace("b", NodeTemplate::local_function(|x: i64| x - 5)),
        ))
        .unwrap();
    let (artifact, plan) = planner.build("one_shot").unwrap();
    assert_eq!(plan.selected, ["base", "replace"]);
    assert_eq!(artifact.revisions.len(), 2);
    let engine = Deployment::new(artifact.graph)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap();
    let report = engine.fire(1, vec![Value::local(10_i64)]).unwrap();
    assert!(report.success());
    assert_eq!(
        report.publications().last().unwrap().borrow::<i64>(),
        Some(&6)
    );
}

#[test]
fn independently_contributed_edge_splices_compose_by_exact_position() {
    use phenix_graph::{PatchAnchor, SplicePosition};
    for reverse in [false, true] {
        let mut planner = PatchPlanner::new();
        base(&mut planner);
        let anchor = PatchAnchor::nodes("a", "value", "b", "run", "value");
        let add_first = |planner: &mut PatchPlanner| {
            planner
                .add_patch(PatchBuilder::splice(
                    PatchSpec::new("insert.add").after("base"),
                    anchor.clone(),
                    SplicePosition::new(1, 3).unwrap(),
                    None,
                    "increment",
                    |x: i64| x + 10,
                ))
                .unwrap();
        };
        let add_second = |planner: &mut PatchPlanner| {
            planner
                .add_patch(PatchBuilder::splice(
                    PatchSpec::new("insert.multiply").after("base"),
                    anchor.clone(),
                    SplicePosition::new(2, 3).unwrap(),
                    None,
                    "multiply",
                    |x: i64| x * 3,
                ))
                .unwrap();
        };
        if reverse {
            add_second(&mut planner);
            add_first(&mut planner);
        } else {
            add_first(&mut planner);
            add_second(&mut planner);
        }
        let mut assembly = GraphAssembly::new("spliced");
        let plan = planner.apply(&mut assembly).unwrap();
        assert_eq!(plan.splices.len(), 2);
        assert_eq!(plan.splices[0].node, "increment");
        assert_eq!(plan.splices[1].node, "multiply");
        assert_eq!(execute(assembly, 2), 78); // ((2 + 1 + 10) * 3) * 2
    }
}

#[test]
fn equivalent_splice_positions_require_a_distinct_explicit_order() {
    use phenix_graph::{PatchAnchor, SplicePosition};
    assert!(SplicePosition::new(0, 4).is_err());
    assert!(SplicePosition::new(1, 0).is_err());
    assert!(SplicePosition::new(4, 4).is_err());
    let anchor = PatchAnchor::nodes("a", "value", "b", "run", "value");
    let mut planner = PatchPlanner::new();
    base(&mut planner);
    planner
        .add_patch(PatchBuilder::splice(
            PatchSpec::new("first").after("base"),
            anchor.clone(),
            SplicePosition::new(1, 2).unwrap(),
            None,
            "add",
            |x: i64| x + 5,
        ))
        .unwrap();
    planner
        .add_patch(PatchBuilder::splice(
            PatchSpec::new("second").after("base"),
            anchor.clone(),
            SplicePosition::new(2, 4).unwrap(),
            None,
            "multiply",
            |x: i64| x * 2,
        ))
        .unwrap();
    assert!(matches!(
        planner.resolve(),
        Err(PatchError::SpliceCollision { .. })
    ));

    let mut planner = PatchPlanner::new();
    base(&mut planner);
    planner
        .add_patch(PatchBuilder::splice(
            PatchSpec::new("first").after("base"),
            anchor.clone(),
            SplicePosition::new(1, 2).unwrap(),
            Some(10),
            "add",
            |x: i64| x + 5,
        ))
        .unwrap();
    planner
        .add_patch(PatchBuilder::splice(
            PatchSpec::new("second").after("base"),
            anchor,
            SplicePosition::new(2, 4).unwrap(),
            Some(20),
            "multiply",
            |x: i64| x * 2,
        ))
        .unwrap();
    let mut assembly = GraphAssembly::new("ordered_splices");
    planner.apply(&mut assembly).unwrap();
    assert_eq!(execute(assembly, 2), 32); // ((2+1+5)*2)*2
}

#[test]
fn noncommuting_direct_rewrite_and_splice_on_same_edge_conflict() {
    use phenix_graph::{PatchAnchor, SplicePosition};
    let mut planner = PatchPlanner::new();
    base(&mut planner);
    let anchor = PatchAnchor::nodes("a", "value", "b", "run", "value");
    planner
        .add_patch(PatchBuilder::splice(
            PatchSpec::new("splice").after("base"),
            anchor,
            SplicePosition::new(1, 2).unwrap(),
            None,
            "insert",
            |x: i64| x + 3,
        ))
        .unwrap();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("rewire")
                .after("base")
                .writes(PatchTarget::edge_nodes("a", "value", "b", "run", "value")),
            |_g| Ok(()),
        ))
        .unwrap();
    assert!(matches!(
        planner.resolve(),
        Err(PatchError::Conflict { .. })
    ));
}

#[test]
fn missing_original_splice_anchor_fails_and_preserves_previous_assembly() {
    use phenix_graph::{PatchAnchor, SplicePosition};
    let mut assembly = GraphAssembly::new("failed_splice");
    let mut planner = PatchPlanner::new();
    base(&mut planner);
    planner
        .add_patch(PatchBuilder::splice(
            PatchSpec::new("splice").after("base"),
            PatchAnchor::nodes("a", "value", "b", "run", "nonexistent"),
            SplicePosition::new(1, 2).unwrap(),
            None,
            "added",
            |x: i64| x + 3,
        ))
        .unwrap();
    assert!(matches!(
        planner.apply(&mut assembly),
        Err(PatchError::Apply { .. })
    ));
    assert!(assembly.revisions().is_empty());
}

#[test]
fn splicing_into_public_module_input_preserves_module_interface() {
    use phenix_graph::{PatchAnchor, SplicePosition};
    let child = module("child", 2);
    let mut planner = PatchPlanner::new();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("base").writes_all([
                PatchTarget::Node("accept".into()),
                PatchTarget::Instance("child".into()),
                PatchTarget::edge_node_to_instance("accept", "value", "child", "input"),
                PatchTarget::PublicInput("request".into()),
                PatchTarget::PublicOutput("result".into()),
                PatchTarget::Trigger("run".into()),
            ]),
            move |g| {
                let a = g
                    .node("accept", |x: i64| x + 1)
                    .map_err(|e| e.to_string())?;
                let instance = g.instance("child", &child)?;
                g.connect(a.output(), module_input(&instance, "input")?)?;
                let input = g.expose_input("request", a.inlet())?;
                g.expose_output(
                    "result",
                    instance
                        .operation()
                        .unwrap()
                        .output::<i64, phenix_graph::LocalValue>()?,
                )?;
                g.trigger("run", [input])?;
                Ok(())
            },
        ))
        .unwrap();
    planner
        .add_patch(PatchBuilder::splice(
            PatchSpec::new("plugin.insert").after("base"),
            PatchAnchor::node_to_instance("accept", "value", "child", "input"),
            SplicePosition::new(1, 2).unwrap(),
            None,
            "added",
            |x: i64| x + 5,
        ))
        .unwrap();
    let (artifact, resolution) = planner.build("modular_splice").unwrap();
    assert_eq!(resolution.splices.len(), 1);
    let report = Deployment::new(artifact.graph)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire(1, vec![Value::local(2_i64)])
        .unwrap();
    assert!(report.success());
    assert_eq!(
        report.publications().last().unwrap().borrow::<i64>(),
        Some(&16)
    );
}

#[test]
fn identifier_and_node_instance_namespace_conflicts_are_explicit() {
    let mut planner = PatchPlanner::new();
    assert!(matches!(
        planner.add_patch(PatchBuilder::custom(PatchSpec::new("invalid/name"), |_g| {
            Ok(())
        })),
        Err(PatchError::InvalidId(_))
    ));
    assert!(matches!(
        planner.add_patch(PatchBuilder::custom(
            PatchSpec::new("graph.splice.lowering"),
            |_g| Ok(())
        )),
        Err(PatchError::InvalidId(_))
    ));
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("node").writes(PatchTarget::Node("duplicate".into())),
            |_g| Ok(()),
        ))
        .unwrap();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("instance").writes(PatchTarget::Instance("duplicate".into())),
            |_g| Ok(()),
        ))
        .unwrap();
    assert!(matches!(
        planner.resolve(),
        Err(PatchError::Conflict { .. })
    ));
}
