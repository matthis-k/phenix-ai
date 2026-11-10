//! Independent reference checks for nested module composition, cyclic agent
//! feedback, and deliberately interleaved execution across distinct waves.
use phenix_graph::{
    CompilePhase, Deployment, GraphBuilder, ModuleCatalog, ModuleDefBuilder, ReportRetention, Value,
};

fn next(rng: &mut u64) -> u64 {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    *rng
}

#[test]
fn structured_diagnostics_identify_recursive_and_missing_definitions() {
    let mut missing = ModuleDefBuilder::new("root");
    missing.try_instance("child", "missing").unwrap();
    let mut catalog = ModuleCatalog::new();
    catalog.add(missing.build()).unwrap();
    let error = match catalog.compile("root") {
        Ok(_) => panic!("missing definition compiled"),
        Err(error) => error,
    };
    assert_eq!(error.phase, CompilePhase::Declaration);
    assert_eq!(error.definition, "root");
    assert!(error.message.contains("missing"));

    let mut recursive = ModuleDefBuilder::new("root");
    recursive.try_instance("child", "root").unwrap();
    let mut catalog = ModuleCatalog::new();
    catalog.add(recursive.build()).unwrap();
    let error = match catalog.compile("root") {
        Ok(_) => panic!("recursive definition compiled"),
        Err(error) => error,
    };
    assert_eq!(error.phase, CompilePhase::Declaration);
    assert!(error.message.contains("recursive"));
}

#[test]
fn nested_compilation_matches_independent_arithmetic_reference() {
    for seed in 1..=96_u64 {
        let mut rng = seed;
        let mut base = GraphBuilder::new(format!("nest_{seed}_0"));
        let first = base.node("first", |v: i64| v + 1).unwrap();
        base.expose_input("request", first.inlet()).unwrap();
        base.expose_output("result", first.output()).unwrap();
        let mut nested = base.module();
        let x = (next(&mut rng) % 1000) as i64 - 500;
        let mut expected = x + 1;
        let depth = (next(&mut rng) % 8 + 1) as usize;
        for level in 1..=depth {
            let delta = (next(&mut rng) % 31) as i64 - 15;
            expected += delta;
            let mut parent = GraphBuilder::new(format!("nest_{seed}_{level}"));
            let child = parent.instance("child", &nested).unwrap();
            let bump = parent.node("bump", move |v: i64| v + delta).unwrap();
            parent
                .connect(
                    child
                        .operation()
                        .unwrap()
                        .output::<i64, phenix_graph::LocalValue>()
                        .unwrap(),
                    bump.inlet(),
                )
                .unwrap();
            parent
                .expose_input(
                    "request",
                    child
                        .operation()
                        .unwrap()
                        .inlets::<(), phenix_graph::LocalValue>()
                        .port(phenix_graph::InletPort::<(), i64>::new("request"))
                        .field(phenix_graph::SlotField::<i64, i64>::new("value"))
                        .target()
                        .unwrap(),
                )
                .unwrap();
            parent.expose_output("result", bump.output()).unwrap();
            nested = parent.module();
        }
        let mut graph = GraphBuilder::new(format!("root_{seed}"));
        let child = graph.instance("calculator", &nested).unwrap();
        let input = graph
            .expose_input(
                "request",
                child
                    .operation()
                    .unwrap()
                    .inlets::<(), phenix_graph::LocalValue>()
                    .port(phenix_graph::InletPort::<(), i64>::new("request"))
                    .field(phenix_graph::SlotField::<i64, i64>::new("value"))
                    .target()
                    .unwrap(),
            )
            .unwrap();
        graph
            .expose_output(
                "result",
                child
                    .operation()
                    .unwrap()
                    .output::<i64, phenix_graph::LocalValue>()
                    .unwrap(),
            )
            .unwrap();
        let trigger = graph.trigger("compute", [input]).unwrap();
        let compiled = graph.compile().unwrap();
        let engine = Deployment::new(compiled)
            .report_retention(ReportRetention::TerminalOutputs)
            .build()
            .unwrap();
        let report = engine.fire(trigger.get(), vec![Value::local(x)]).unwrap();
        assert!(
            report.success(),
            "seed={seed}: {:?}",
            report.error_messages()
        );
        assert_eq!(
            report.publications().last().unwrap().borrow::<i64>(),
            Some(&expected),
            "nested arithmetic seed={seed} depth={depth}"
        );
    }
}

#[derive(Clone)]
struct Cycle {
    remaining: usize,
    value: i64,
}

#[test]
fn random_finite_cycles_match_small_step_reference() {
    let mut g = GraphBuilder::new("cyclic-reference");
    let tool = g
        .node("tool", |mut state: Cycle| {
            state.remaining -= 1;
            state.value += 2;
            state
        })
        .unwrap();
    let finish = g.node("finish", |state: Cycle| state.value).unwrap();
    let tool_target = tool.inlet();
    let final_target = finish.inlet();
    let model = g
        .node("model", move |state: Cycle| {
            phenix_graph::Output::to(
                if state.remaining != 0 {
                    &tool_target
                } else {
                    &final_target
                },
                state,
            )
        })
        .unwrap();
    g.connect(model.output(), tool.inlet()).unwrap();
    g.connect(tool.output(), model.inlet()).unwrap();
    g.connect(model.output(), finish.inlet()).unwrap();
    let input = g.expose_input("request", model.inlet()).unwrap();
    g.expose_output("result", finish.output()).unwrap();
    let trigger = g.trigger("run", [input]).unwrap();
    let engine = Deployment::new(g.compile().unwrap())
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap();
    let mut rng = 0x7651fe_u64;
    for _ in 0..96 {
        let steps = (next(&mut rng) % 25) as usize;
        let x = (next(&mut rng) % 111) as i64;
        let result = engine
            .fire(
                trigger.get(),
                vec![Value::local(Cycle {
                    remaining: steps,
                    value: x,
                })],
            )
            .unwrap();
        assert!(
            result.success(),
            "steps={steps}: {:?}",
            result.error_messages()
        );
        assert_eq!(
            result.publications().last().unwrap().borrow::<i64>(),
            Some(&(x + 2 * steps as i64))
        );
        // Each iteration emits model-left and tool; last iteration emits model-right + finish.
        assert_eq!(result.publication_events().len(), 2 * steps + 2);
    }
}

#[test]
fn randomized_wave_interleavings_and_cancellations_are_isolated() {
    let mut g = GraphBuilder::new("interleaved");
    let first = g.node("first", |v: i64| v + 3).unwrap();
    let second = g.node("second", |v: i64| v * 2).unwrap();
    g.connect(first.output(), second.inlet()).unwrap();
    let input = g.expose_input("request", first.inlet()).unwrap();
    g.expose_output("result", second.output()).unwrap();
    let trigger = g.trigger("run", [input]).unwrap();
    let engine = Deployment::new(g.compile().unwrap())
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap();

    let mut rng = 0x90c024_u64;
    for case in 0..64_i64 {
        let inputs: Vec<i64> = (0..5).map(|i| 10 * case + i).collect();
        let mut waves: Vec<_> = inputs
            .iter()
            .map(|&x| engine.begin(trigger.get(), vec![Value::local(x)]).unwrap())
            .collect();
        let mut cancelled = [false; 5];
        for step in 0..80 {
            let index = (next(&mut rng) % waves.len() as u64) as usize;
            if step % 13 == 0 && !waves[index].can_finish() {
                waves[index].cancel();
                cancelled[index] = true;
            } else if let Some(ready) = waves[index].next_ready() {
                let (ticket, ctx) = ready.into_parts();
                let result = waves[index].action(ticket).unwrap().execute(ctx);
                waves[index].complete(ticket, result).unwrap();
            }
        }
        for (i, mut wave) in waves.into_iter().enumerate() {
            while let Some(ready) = wave.next_ready() {
                let (ticket, ctx) = ready.into_parts();
                let result = wave.action(ticket).unwrap().execute(ctx);
                wave.complete(ticket, result).unwrap();
            }
            let report = wave.finish().unwrap();
            if cancelled[i] {
                assert!(report.cancelled());
                assert!(!report.success());
                assert!(report.publications().next().is_none());
            } else {
                assert!(
                    report.success(),
                    "case={case} wave={i}: {:?}",
                    report.error_messages()
                );
                assert_eq!(
                    report.publications().last().unwrap().borrow::<i64>(),
                    Some(&((inputs[i] + 3) * 2))
                );
            }
        }
    }
}

#[test]
fn expansion_diagnostic_reports_invalid_named_connection() {
    use phenix_graph::{ModuleInputTarget, ModuleOutputSource, NodeTemplate};
    let mut root = ModuleDefBuilder::new("invalid_wiring");
    let target = NodeTemplate::local_function(|value: i64| value + 1);
    root.add_named("target", target).unwrap();
    root.connect(
        ModuleOutputSource::NodeNamed {
            node: "absent".into(),
            output: "value".into(),
        },
        ModuleInputTarget::NodeNamed {
            node: "target".into(),
            port: "run".into(),
            slot: "value".into(),
        },
    );
    let mut catalog = ModuleCatalog::new();
    catalog.add(root.build()).unwrap();
    let diagnostic = match catalog.compile("invalid_wiring") {
        Ok(_) => panic!("invalid wiring compiled"),
        Err(err) => err,
    };
    assert_eq!(diagnostic.phase, CompilePhase::Expansion);
    assert!(diagnostic.message.contains("absent"));
}

#[test]
fn trigger_binding_diagnostic_is_separate_from_expansion() {
    use phenix_graph::ModuleTriggerTarget;
    let mut builder = ModuleDefBuilder::new("missing_trigger_port");
    builder.trigger(7, vec![ModuleTriggerTarget::PublicInput(99)]);
    let mut catalog = ModuleCatalog::new();
    catalog.add(builder.build()).unwrap();
    let diagnostic = match catalog.compile("missing_trigger_port") {
        Ok(_) => panic!("invalid trigger compiled"),
        Err(diagnostic) => diagnostic,
    };
    assert_eq!(diagnostic.phase, CompilePhase::Triggers);
    assert!(diagnostic.message.contains("99"));
}
