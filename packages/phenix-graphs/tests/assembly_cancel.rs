//! Patch-ready assembly and cooperative wave cancellation acceptance tests.
//! These are deliberately external-crate tests with no graph macro or derive.
use phenix_graph::{
    Deployment, Engine, GraphAssembly, GraphBuilder, ModulePackage, NodeTemplate, PatchBuilder,
    PatchPlanner, PatchSpec, PatchTarget, ReportRetention, Store, Value,
};

// An assembly is deliberately not a second public contribution API.
// External contributors register patches with the same planner used by
// ordinary plugin patches; the broad claim is local to each single-patch
// atomicity fixture, which tests staging rather than conflict resolution.
fn contribute<F>(assembly: &mut GraphAssembly, id: &str, edit: F) -> Result<(), String>
where
    F: FnOnce(&mut GraphBuilder) -> Result<(), String> + 'static,
{
    let mut planner = PatchPlanner::new();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new(id).writes(PatchTarget::Graph),
            edit,
        ))
        .map_err(|err| err.to_string())?;
    planner.apply(assembly).map_err(|err| err.to_string())?;
    Ok(())
}

fn chain() -> (Engine, u16) {
    let mut builder = GraphBuilder::new("chain");
    let first = builder.node("first", |x: i64| x + 1).unwrap();
    let second = builder.node("second", |x: i64| x * 2).unwrap();
    builder.connect(first.output(), second.inlet()).unwrap();
    let input = builder.expose_input("request", first.inlet()).unwrap();
    builder.expose_output("result", second.output()).unwrap();
    let trigger = builder.trigger("start", [input]).unwrap();
    let engine = Deployment::new(builder.compile().unwrap())
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap();
    (engine, trigger.get())
}

#[test]
fn cancellation_discards_ready_work_without_creating_incomplete_errors() {
    let (engine, trigger) = chain();
    let mut wave = engine.begin(trigger, vec![Value::local(7_i64)]).unwrap();
    assert!(!wave.can_finish());
    assert_eq!(wave.cancel().len(), 0);
    assert_eq!(wave.cancel().len(), 0); // idempotent
    assert!(wave.next_ready().is_none());
    assert!(wave.can_finish());
    let report = wave.finish().unwrap();
    assert!(report.cancelled());
    assert!(!report.success());
    assert!(report.errors().is_empty());
    assert!(report.publication_events().is_empty());
    // The cancelled wave did not change the compiled graph or later turns.
    let next = engine.fire(trigger, vec![Value::local(7_i64)]).unwrap();
    assert!(next.success());
    assert_eq!(
        next.publications().last().unwrap().borrow::<i64>(),
        Some(&16)
    );
}

#[test]
fn cancellation_invalidates_outstanding_tickets_and_is_wave_local() {
    let (engine, trigger) = chain();
    let mut a = engine.begin(trigger, vec![Value::local(3_i64)]).unwrap();
    let mut b = engine.begin(trigger, vec![Value::local(5_i64)]).unwrap();
    assert_ne!(a.wave_id(), b.wave_id());
    let (a_ticket, a_ctx) = a.next_ready().unwrap().into_parts();
    let (b_ticket, b_ctx) = b.next_ready().unwrap().into_parts();
    let a_result = a.action(a_ticket).unwrap().execute(a_ctx);
    let b_result = b.action(b_ticket).unwrap().execute(b_ctx);
    assert_eq!(a.in_flight_count(), 1);
    assert_eq!(a.cancel(), &[a_ticket]);
    assert_eq!(a.in_flight_count(), 0);
    assert_eq!(a.cancel(), &[a_ticket]);
    assert!(a.action(a_ticket).is_none());
    assert!(
        a.complete(a_ticket, a_result)
            .unwrap_err()
            .contains("cancelled")
    );
    let cancelled = a.finish().unwrap();
    assert!(cancelled.cancelled());
    assert_eq!(cancelled.publication_events().len(), 0);

    b.complete(b_ticket, b_result).unwrap();
    let (next_ticket, next_ctx) = b.next_ready().unwrap().into_parts();
    let next_result = b.action(next_ticket).unwrap().execute(next_ctx);
    b.complete(next_ticket, next_result).unwrap();
    assert!(b.next_ready().is_none());
    let completed = b.finish().unwrap();
    assert!(completed.success(), "{:?}", completed.error_messages());
    assert_eq!(
        completed.publications().last().unwrap().borrow::<i64>(),
        Some(&12)
    );
}

#[test]
fn cancellation_handle_is_thread_safe_and_observed_before_dispatch() {
    let (engine, trigger) = chain();
    let mut wave = engine.begin(trigger, vec![Value::local(3_i64)]).unwrap();
    let cancel = wave.cancellation();
    assert_eq!(cancel.wave_id(), wave.wave_id());
    std::thread::spawn(move || cancel.request()).join().unwrap();
    assert!(wave.next_ready().is_none());
    assert!(wave.finish().unwrap().cancelled());
}

#[test]
fn requested_cancellation_rejects_late_completion() {
    let (engine, trigger) = chain();
    let mut wave = engine.begin(trigger, vec![Value::local(5_i64)]).unwrap();
    let (ticket, ctx) = wave.next_ready().unwrap().into_parts();
    let result = wave.action(ticket).unwrap().execute(ctx);
    let cancel = wave.cancellation();
    cancel.request();
    assert!(
        wave.complete(ticket, result)
            .unwrap_err()
            .contains("cancelled")
    );
    assert!(wave.finish().unwrap().cancelled());
}

#[test]
fn cancelled_wave_preserves_prior_publication_but_prevents_followup() {
    let (engine, trigger) = chain();
    let mut wave = engine.begin(trigger, vec![Value::local(2_i64)]).unwrap();
    let (ticket, ctx) = wave.next_ready().unwrap().into_parts();
    let result = wave.action(ticket).unwrap().execute(ctx);
    wave.complete(ticket, result).unwrap();
    assert_eq!(wave.cancel().len(), 0);
    let report = wave.finish().unwrap();
    assert!(report.cancelled());
    assert_eq!(report.publication_events().len(), 1);
    assert!(report.publications().next().is_none()); // terminal-only retention
}

#[test]
fn assembly_contributions_are_transactional_and_replace_nodes_by_interface() {
    let mut assembly = GraphAssembly::new("contributed");
    contribute(&mut assembly, "base", |g| {
        let first = g.node("first", |x: i64| x + 1).unwrap();
        let second = g.node("second", |x: i64| x * 2).unwrap();
        g.connect(first.output(), second.inlet())?;
        let start = g.expose_input("request", first.inlet())?;
        g.expose_output("result", second.output())?;
        g.trigger("submit", [start])?;
        Ok(())
    })
    .unwrap();
    assert!(
        contribute(&mut assembly, "bad", |g| {
            g.replace("first", NodeTemplate::local_function(|x: i64| x * 999))?;
            Err("reject whole contribution".to_string())
        })
        .is_err()
    );
    assert_eq!(assembly.revisions().len(), 1);
    assert!(contribute(&mut assembly, "base", |_| Ok(())).is_err());
    contribute(&mut assembly, "patch", |g| {
        // An interface-changing patch fails without corrupting the declaration.
        assert!(
            g.replace("first", NodeTemplate::local_function(|x: String| x))
                .is_err()
        );
        g.replace("first", NodeTemplate::local_function(|x: i64| x * 3))?;
        Ok(())
    })
    .unwrap();
    let artifact = assembly.finish().unwrap();
    assert_eq!(artifact.revisions.len(), 2);
    assert_eq!(artifact.revisions[0].contributor, "base");
    assert_eq!(artifact.revisions[1].contributor, "patch");
    assert!(artifact.revisions[1].changes.iter().any(
        |change| matches!(change, phenix_graph::AssemblyChange::NodeReplaced(name)
            if name == "first")
    ));
    let report = Deployment::new(artifact.graph)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire(1, vec![Value::local(4_i64)])
        .unwrap();
    assert!(report.success());
    assert_eq!(
        report.publications().last().unwrap().borrow::<i64>(),
        Some(&24)
    );
}

fn module(name: &str, factor: i64) -> ModulePackage {
    let mut builder = GraphBuilder::new(name);
    let transform = builder.node("transform", move |x: i64| x * factor).unwrap();
    builder.expose_input("input", transform.inlet()).unwrap();
    builder.expose_output("output", transform.output()).unwrap();
    builder.module()
}

#[test]
fn module_rebinding_preserves_public_interface_and_external_connections() {
    let first = module("math_original", 2);
    let replacement = module("math_replacement", 3);
    let incompatible = {
        let mut b = GraphBuilder::new("wrong_signature");
        let n = b.node("change", |s: String| s).unwrap();
        b.expose_input("input", n.inlet()).unwrap();
        b.expose_output("output", n.output()).unwrap();
        b.module()
    };
    let mut assembly = GraphAssembly::new("server");
    contribute(&mut assembly, "base", move |g| {
        let instance = g.instance("math", &first)?;
        let start = g.expose_input(
            "request",
            instance
                .operation()?
                .inlets::<(), phenix_graph::LocalValue>()
                .port(phenix_graph::InletPort::<(), i64>::new("input"))
                .field(phenix_graph::SlotField::<i64, i64>::new("value"))
                .target()?,
        )?;
        g.expose_output(
            "result",
            instance
                .operation()
                .unwrap()
                .output::<i64, phenix_graph::LocalValue>()?,
        )?;
        g.trigger("submit", [start])?;
        Ok(())
    })
    .unwrap();
    contribute(&mut assembly, "rebinding", move |g| {
        assert!(g.replace("math", &incompatible).is_err());
        g.replace("math", &replacement)
    })
    .unwrap();
    let artifact = assembly.finish().unwrap();
    let report = Deployment::new(artifact.graph)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire(1, vec![Value::local(7_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report.publications().last().unwrap().borrow::<i64>(),
        Some(&21)
    );
}

#[test]
fn disconnect_and_rewire_reject_missing_edges_without_dangling_references() {
    let mut g = GraphBuilder::new("rewire");
    let a = g.node("a", |x: i64| x + 1).unwrap();
    let b = g.node("b", |x: i64| x * 2).unwrap();
    let c = g.node("c", |x: i64| x * 3).unwrap();
    g.connect(a.output(), b.inlet()).unwrap();
    g.disconnect(a.output(), b.inlet()).unwrap();
    assert!(g.disconnect(a.output(), b.inlet()).is_err());
    g.connect(a.output(), c.inlet()).unwrap();
    g.connect(c.output(), b.inlet()).unwrap();
    let input = g.expose_input("request", a.inlet()).unwrap();
    g.expose_output("result", b.output()).unwrap();
    g.trigger("submit", [input]).unwrap();
    let report = Deployment::new(g.compile().unwrap())
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire(1, vec![Value::local(2_i64)])
        .unwrap();
    assert!(report.success());
    assert_eq!(
        report.publications().last().unwrap().borrow::<i64>(),
        Some(&18)
    );
}

#[derive(Clone)]
struct AgentStep {
    prompt: String,
    ran_tool: bool,
}

#[test]
fn macro_free_agent_loop_uses_real_typed_conditional_outputs() {
    use phenix_graph::Output;
    let mut g = GraphBuilder::new("echo-agent");
    let accept = g
        .node("accept", |text: String| AgentStep {
            prompt: text,
            ran_tool: false,
        })
        .unwrap();
    let tool = g
        .node("tool", |call: String| format!("tool:{call}"))
        .unwrap();
    let finish = g.node("finish", |answer: String| answer).unwrap();
    let tool_target = tool.inlet();
    let final_target = finish.inlet();
    let model = g
        .node("model", move |step: AgentStep| {
            let needs_tool = step.prompt.starts_with("/tool ") && !step.ran_tool;
            Output::to(
                if needs_tool {
                    &tool_target
                } else {
                    &final_target
                },
                step.prompt,
            )
        })
        .unwrap();
    let resume = g
        .node("resume", |result: String| AgentStep {
            prompt: result.strip_prefix("tool:").unwrap().to_owned(),
            ran_tool: true,
        })
        .unwrap();
    g.connect(accept.output(), model.inlet()).unwrap();
    g.connect(model.output(), tool.inlet()).unwrap();
    g.connect(tool.output(), resume.inlet()).unwrap();
    g.connect(resume.output(), model.inlet()).unwrap();
    g.connect(model.output(), finish.inlet()).unwrap();
    let input = g.expose_input("prompt", accept.inlet()).unwrap();
    g.expose_output("answer", finish.output()).unwrap();
    let trigger = g.trigger("submit", [input]).unwrap();
    let engine = Deployment::new(g.compile().unwrap())
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap();
    for prompt in ["hello", "/tool hello", "again", "/tool second"] {
        let report = engine
            .fire(trigger.get(), vec![Value::local(prompt.to_owned())])
            .unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        assert_eq!(
            report
                .publications()
                .last()
                .unwrap()
                .borrow::<String>()
                .map(String::as_str),
            Some(prompt)
        );
        assert_eq!(
            report.publication_events().len(),
            if prompt.starts_with("/tool ") { 6 } else { 3 }
        );
    }
}

#[test]
fn branch_cancellation_never_emits_missing_path_or_incomplete_error() {
    use phenix_graph::Output;
    let mut g = GraphBuilder::new("conditional");
    let yes = g.node("yes", |n: i64| n + 10).unwrap();
    let no = g.node("no", |n: i64| n + 20).unwrap();
    let yes_target = yes.inlet();
    let no_target = no.inlet();
    let model = g
        .node("model", move |n: i64| {
            Output::to(if n > 0 { &yes_target } else { &no_target }, n.abs())
        })
        .unwrap();
    g.connect(model.output(), yes.inlet()).unwrap();
    g.connect(model.output(), no.inlet()).unwrap();
    let input = g.expose_input("n", model.inlet()).unwrap();
    let trigger = g.trigger("run", [input]).unwrap();
    let engine = Deployment::new(g.compile().unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap();
    let mut wave = engine
        .begin(trigger.get(), vec![Value::local(1_i64)])
        .unwrap();
    let (ticket, ctx) = wave.next_ready().unwrap().into_parts();
    let result = wave.action(ticket).unwrap().execute(ctx);
    wave.complete(ticket, result).unwrap();
    wave.cancel(); // queued branch should be discarded without incompletes
    let report = wave.finish().unwrap();
    assert!(report.cancelled());
    assert!(report.errors().is_empty());
    assert_eq!(report.publication_events().len(), 1);
}

#[test]
fn later_contributor_can_patch_by_named_ports_without_original_handles() {
    let mut assembly = GraphAssembly::new("late_patch");
    contribute(&mut assembly, "base", |g| {
        let a = g.node("a", |x: i64| x + 1).unwrap();
        let b = g.node("b", |x: i64| x * 2).unwrap();
        g.connect(a.output(), b.inlet())?;
        let input = g.expose_input("request", a.inlet())?;
        g.expose_output("result", b.output())?;
        g.trigger("run", [input])?;
        Ok(())
    })
    .unwrap();
    contribute(&mut assembly, "routing.patch", |g| {
        let old_source = g
            .discover("a")
            .unwrap()
            .output::<i64, phenix_graph::LocalValue>()?;
        let old_target = g
            .discover("b")
            .unwrap()
            .inlets::<(), phenix_graph::LocalValue>()
            .port(phenix_graph::InletPort::<(), i64>::new("run"))
            .field(phenix_graph::SlotField::<i64, i64>::new("value"))
            .target()?;
        g.disconnect(old_source, old_target)?;
        let extra = g.node("extra", |x: i64| x + 10).unwrap();
        let from_a = g
            .discover("a")
            .unwrap()
            .output::<i64, phenix_graph::LocalValue>()?;
        let into_b = g
            .discover("b")
            .unwrap()
            .inlets::<(), phenix_graph::LocalValue>()
            .port(phenix_graph::InletPort::<(), i64>::new("run"))
            .field(phenix_graph::SlotField::<i64, i64>::new("value"))
            .target()?;
        g.connect(from_a, extra.inlet())?;
        g.connect(extra.output(), into_b)?;
        Ok(())
    })
    .unwrap();
    let artifact = assembly.finish().unwrap();
    assert!(
        artifact.revisions[1]
            .changes
            .iter()
            .any(|change| matches!(change, phenix_graph::AssemblyChange::ConnectionRemoved(_)))
    );
    let report = Deployment::new(artifact.graph)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire(1, vec![Value::local(2_i64)])
        .unwrap();
    assert!(report.success());
    assert_eq!(
        report.publications().last().unwrap().borrow::<i64>(),
        Some(&26)
    );
}
