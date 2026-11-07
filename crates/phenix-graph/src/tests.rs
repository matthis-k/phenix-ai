    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};


    const I: &'static str = <i64 as ContractValue>::SCHEMA;
    const S: &'static str = <String as ContractValue>::SCHEMA;

    fn wrap(id: u16, name: &'static str) -> NodeTemplate {
        node(id, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], move |ctx| {
                Ok(vec![emit(0, format!("{}({})", name, ctx.arg::<String>(0)?))])
            })
    }

    fn join2(id: u16, name: &'static str) -> NodeTemplate {
        node(id, vec![all(0, vec![slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], move |ctx| {
                Ok(vec![emit(0, format!("{}({},{})", name, ctx.arg::<String>(0)?, ctx.arg::<String>(1)?))])
            })
    }

    fn join3(id: u16, name: &'static str) -> NodeTemplate {
        node(id, vec![all(0, vec![
                slot(0, S, BufferMode::Deferred),
                slot(1, S, BufferMode::Deferred),
                slot(2, S, BufferMode::Deferred),
            ])],
            vec![out(0, S)], CohortMode::Inherit, vec![], move |ctx| {
                Ok(vec![emit(0, format!(
                    "{}({},{},{})",
                    name,
                    ctx.arg::<String>(0)?,
                    ctx.arg::<String>(1)?,
                    ctx.arg::<String>(2)?,
                ))])
            })
    }

    fn published_string(report: &WaveReport, from: OutputAddr) -> String {
        report.published.iter().find(|p| p.from == from)
            .unwrap_or_else(|| panic!("missing publication from {:?}", from))
            .value.get::<String>().unwrap()
    }

    fn expression_diamond() -> Engine {
        let mut g = FlatGraphBuilder::default();
        g.add_node(wrap(1, "a"));
        g.add_node(wrap(2, "b"));
        g.add_node(wrap(3, "c"));
        g.add_node(join2(4, "d"));
        g.connect(output(1, 0), input(2, 0, 0));
        g.connect(output(1, 0), input(3, 0, 0));
        g.connect(output(2, 0), input(4, 0, 0));
        g.connect(output(3, 0), input(4, 0, 1));
        g.trigger(1, vec![input(1, 0, 0)]);
        Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
    }

    fn expression_shared_branch() -> Engine {
        let mut g = FlatGraphBuilder::default();
        g.add_node(wrap(1, "a"));
        g.add_node(wrap(2, "b"));
        g.add_node(wrap(3, "c"));
        g.add_node(wrap(4, "d"));
        g.add_node(wrap(5, "e"));
        g.add_node(wrap(6, "f"));
        g.add_node(join3(7, "g"));
        g.connect(output(1, 0), input(2, 0, 0));
        g.connect(output(1, 0), input(3, 0, 0));
        g.connect(output(2, 0), input(4, 0, 0));
        g.connect(output(2, 0), input(5, 0, 0));
        g.connect(output(3, 0), input(6, 0, 0));
        g.connect(output(4, 0), input(7, 0, 0));
        g.connect(output(5, 0), input(7, 0, 1));
        g.connect(output(6, 0), input(7, 0, 2));
        g.trigger(1, vec![input(1, 0, 0)]);
        Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
    }

    fn expression_nested_joins() -> Engine {
        let mut g = FlatGraphBuilder::default();
        g.add_node(wrap(1, "a"));
        g.add_node(wrap(2, "b"));
        g.add_node(wrap(3, "c"));
        g.add_node(join2(4, "d"));
        g.add_node(wrap(5, "e"));
        g.add_node(wrap(6, "f"));
        g.add_node(join3(7, "g"));
        g.connect(output(1, 0), input(2, 0, 0));
        g.connect(output(1, 0), input(3, 0, 0));
        g.connect(output(2, 0), input(4, 0, 0));
        g.connect(output(3, 0), input(4, 0, 1));
        g.connect(output(3, 0), input(5, 0, 0));
        g.connect(output(2, 0), input(6, 0, 0));
        g.connect(output(4, 0), input(7, 0, 0));
        g.connect(output(5, 0), input(7, 0, 1));
        g.connect(output(6, 0), input(7, 0, 2));
        g.trigger(1, vec![input(1, 0, 0)]);
        Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
    }

    fn diamond(omit_right: bool) -> Engine {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0, vec![slot(0, I, BufferMode::Immediate)])],
            vec![out(0, I)], CohortMode::FreshPerInvocation, vec![],
            |ctx| Ok(vec![emit(0, ctx.arg::<i64>(0)? + 1)])));
        g.add_node(node(2, vec![all(0, vec![slot(0, I, BufferMode::Immediate)])],
            vec![out(0, I)], CohortMode::Inherit, vec![],
            |ctx| Ok(vec![emit(0, ctx.arg::<i64>(0)? * 2)])));
        g.add_node(node(3, vec![all(0, vec![slot(0, I, BufferMode::Immediate)])],
            vec![out(0, I)], CohortMode::Inherit, vec![],
            move |ctx| if omit_right { Ok(vec![]) } else { Ok(vec![emit(0, ctx.arg::<i64>(0)? + 10)]) }));
        g.add_node(node(4, vec![all(0, vec![slot(0, I, BufferMode::Deferred), slot(1, I, BufferMode::Deferred)])],
            vec![out(0, I)], CohortMode::Inherit, vec![],
            |ctx| Ok(vec![emit(0, ctx.arg::<i64>(0)? + ctx.arg::<i64>(1)?)])));
        g.connect(output(1, 0), input(2, 0, 0));
        g.connect(output(1, 0), input(3, 0, 0));
        g.connect(output(2, 0), input(4, 0, 0));
        g.connect(output(3, 0), input(4, 0, 1));
        g.trigger(1, vec![input(1, 0, 0)]);
        Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
    }

    #[test]
    fn diamond_fanout_runs_once_and_multiple_observers_share_publications() {
        let engine = diamond(false);
        let r = engine.fire(1, vec![enc(3_i64)]).unwrap();
        assert!(r.success(), "{:?}", r.errors);
        for n in 1..=4 { assert_eq!(r.count(n), 1, "node {n}"); }
        let result = r.publications().find(|p| p.source_node_id() == 4).unwrap();
        let left = r.publications().find(|p| p.source_node_id() == 2).unwrap();
        assert_eq!(result.value::<i64>().unwrap(), 22);
        assert_eq!(left.value::<i64>().unwrap(), 8);
        assert_eq!(r.count(1), 1, "observing two outputs must not execute the source twice");
    }

    #[test]
    fn repeated_invocations_join_only_their_own_cohorts() {
        let mut g = FlatGraphBuilder::default();
        // One trigger produces two values for A in ONE wave; A is allowed to
        // execute twice within the same wave, and assigns a fresh cohort to each.
        g.add_node(node(1, vec![all(0, vec![slot(0, I, BufferMode::Immediate)])],
            vec![out(0, I), out(1, I)], CohortMode::Inherit, vec![],
            |_ctx| Ok(vec![emit(0, 1_i64), emit(1, 2_i64)])));
        g.add_node(node(2, vec![all(0, vec![slot(0, I, BufferMode::Deferred)])],
            vec![out(0, I)], CohortMode::FreshPerInvocation, vec![],
            |ctx| Ok(vec![emit(0, ctx.arg::<i64>(0)?)])));
        g.add_node(node(3, vec![all(0, vec![slot(0, I, BufferMode::Deferred)])],
            vec![out(0, I)], CohortMode::Inherit, vec![], |ctx| {
                let v = ctx.arg::<i64>(0)?;
                Ok(vec![emit(0, 10 * v)])
            }));
        g.add_node(node(4, vec![all(0, vec![slot(0, I, BufferMode::Deferred)])],
            vec![out(0, I)], CohortMode::Inherit, vec![], |ctx| {
                let v = ctx.arg::<i64>(0)?;
                Ok(vec![emit(0, 100 * v)])
            }));
        g.add_node(node(5, vec![all(0, vec![slot(0, I, BufferMode::Deferred), slot(1, I, BufferMode::Deferred)])],
            vec![out(0, I)], CohortMode::Inherit, vec![], |ctx| {
                let b = ctx.arg::<i64>(0)?;
                let c = ctx.arg::<i64>(1)?;
                Ok(vec![emit(0, b + c)])
            }));
        g.connect(output(1, 0), input(2, 0, 0));
        g.connect(output(1, 1), input(2, 0, 0));
        g.connect(output(2, 0), input(3, 0, 0));
        g.connect(output(2, 0), input(4, 0, 0));
        g.connect(output(3, 0), input(5, 0, 0));
        g.connect(output(4, 0), input(5, 0, 1));
        g.trigger(1, vec![input(1, 0, 0)]);
        let e = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let r = e.fire(1, vec![enc(0_i64)]).unwrap();
        assert!(r.success(), "{:?}", r.errors);
        assert_eq!((r.count(1), r.count(2), r.count(3), r.count(4), r.count(5)), (1,2,2,2,2));
        let mut pairs: Vec<_> = r.published.iter().filter(|p| p.from == output(5, 0))
            .map(|p| p.value.get::<i64>().unwrap()).collect();
        pairs.sort();
        assert_eq!(pairs, [110, 220]);
    }

    #[test]
    fn partial_join_is_diagnosed_only_when_wave_drains() {
        let r = diamond(true).fire(1, vec![enc(3_i64)]).unwrap();
        assert_eq!(r.count(4), 0);
        assert_eq!(r.errors.len(), 1);
        assert!(matches!(&r.errors[0], WaveError::Incomplete { node: NodeId(4), port: PortId(0), group }
            if group.present == vec![SlotId(0)] && group.missing == vec![SlotId(1)]));
    }

    #[test]
    fn deferred_keeps_three_values_and_immediate_rejects_a_batch() {
        for mode in [BufferMode::Deferred, BufferMode::Immediate] {
            let mut g = FlatGraphBuilder::default();
            g.add_node(node(1, vec![all(0, vec![slot(0, I, BufferMode::Immediate)])],
                vec![out(0, I), out(1, I), out(2, I)], CohortMode::Inherit, vec![],
                |_ctx| Ok(vec![emit(0, 10_i64), emit(1, 20_i64), emit(2, 30_i64)])));
            g.add_node(node(2, vec![all(0, vec![slot(0, I, mode)])], vec![],
                CohortMode::Inherit, vec![], |_ctx| Ok(vec![])));
            for i in 0..3 { g.connect(output(1, i), input(2, 0, 0)); }
            g.trigger(1, vec![input(1, 0, 0)]);
            let r = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
                .fire(1, vec![enc(0_i64)]).unwrap();
            match mode {
                BufferMode::Deferred => { assert!(r.success()); assert_eq!(r.count(2), 3); }
                BufferMode::Immediate => {
                    assert_eq!(r.count(2), 0);
                    assert_eq!(r.errors, vec![WaveError::SlotOccupied(input(2, 0, 0))]);
                }
            }
        }
    }

    #[test]
    fn sequential_waves_and_store_reads_do_not_activate_nodes() {
        let called = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&called);
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0, vec![slot(0, I, BufferMode::Immediate)])],
            vec![out(0, I)], CohortMode::Inherit,
            vec![ResourceNeed { key: "counter", access: Access::Write }],
            move |ctx| {
                count.fetch_add(1, AtomicOrdering::SeqCst);
                let value = ctx.arg::<i64>(0)?;
                ctx.write::<i64, _>("counter", |counter| *counter += value)?;
                let total = ctx.read::<i64, _>("counter", |n| *n)?;
                assert!(ctx.read::<String, _>("counter", |s| s.len()).is_err());
                assert!(ctx.read::<i64, _>("ungranted", |n| *n).is_err());
                Ok(vec![emit(0, total)])
            }));
        g.trigger(1, vec![input(1, 0, 0)]);
        let mut store = Store::default(); store.insert("counter", 0_i64);
        let engine = Arc::new(Deployment::new(g.build().unwrap()).store(store).report_retention(ReportRetention::RetainPayloads).build().unwrap());
        // Even though the resource is present, no node has executed.
        assert_eq!(called.load(AtomicOrdering::SeqCst), 0);
        let r1 = engine.fire(1, vec![enc(2_i64)]).unwrap();
        let r2 = engine.fire(1, vec![enc(3_i64)]).unwrap();
        assert!(r1.success() && r2.success());
        assert_ne!(r1.wave, r2.wave);
        assert_eq!(called.load(AtomicOrdering::SeqCst), 2);
        assert_eq!(*engine.store.get::<i64>("counter").unwrap().read().unwrap(), 5);
    }

    #[test]
    fn any_policy_consumes_only_an_available_candidate() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![any(0, vec![slot(0, I, BufferMode::Deferred), slot(1, I, BufferMode::Deferred)])],
            vec![out(0, I)], CohortMode::Inherit, vec![], |ctx| {
                let v: i64 = if ctx.has_arg(0) { ctx.arg(0)? } else { ctx.arg(1)? };
                Ok(vec![emit(0, v)])
            }));
        g.trigger(1, vec![input(1, 0, 1)]);
        let report = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc(42_i64)]).unwrap();
        assert!(report.success());
        assert_eq!(report.count(1), 1);
        assert_eq!(report.published[0].value.get::<i64>().unwrap(), 42);
    }

    #[test]
    fn external_triggers_are_explicit_and_store_resources_never_trigger() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0, vec![slot(0, I, BufferMode::Immediate)])],
            vec![out(0, I)], CohortMode::Inherit, vec![],
            |ctx| Ok(vec![emit(0, ctx.arg::<i64>(0)?) ])));
        g.add_node(node(2, vec![all(0, vec![slot(0, I, BufferMode::Immediate)])],
            vec![out(0, I)], CohortMode::Inherit,
            vec![ResourceNeed { key: "bias", access: Access::Read }],
            |ctx| {
                let bias = ctx.read::<i64, _>("bias", |v| *v)?;
                Ok(vec![emit(0, ctx.arg::<i64>(0)? + bias)])
            }));
        g.connect(output(1, 0), input(2, 0, 0));
        g.trigger(1, vec![input(1, 0, 0)]);
        // Multiple triggers may address an input port; Store access is never an input delivery.
        g.trigger(2, vec![input(2, 0, 0)]);
        let mut store = Store::default(); store.insert("bias", 7_i64);
        let engine = Deployment::new(g.build().unwrap()).store(store).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let r = engine.fire(2, vec![enc(5_i64)]).unwrap();
        assert!(r.success());
        assert_eq!(r.count(1), 0); // no second scheduler or hidden producer call
        assert_eq!(r.count(2), 1);
        assert_eq!(r.published[0].value.get::<i64>().unwrap(), 12);
        let r2 = engine.fire(1, vec![enc(8_i64)]).unwrap();
        assert!(r2.success());
        assert_eq!(r2.count(1), 1);
        assert_eq!(r2.count(2), 1);
    }

    #[test]
    fn multiple_invocations_are_distinct_but_serial() {
        let mut g = FlatGraphBuilder::default();
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let a = Arc::clone(&active);
        let p = Arc::clone(&peak);
        g.add_node(node(1, vec![all(0, vec![slot(0, I, BufferMode::Immediate)])],
            vec![out(0, I), out(1, I)], CohortMode::Inherit, vec![],
            |_ctx| Ok(vec![emit(0, 1_i64), emit(1, 2_i64)])));
        g.add_node(node(2, vec![all(0, vec![slot(0, I, BufferMode::Deferred)])],
            vec![], CohortMode::Inherit, vec![], move |_ctx| {
                let now = a.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                p.fetch_max(now, AtomicOrdering::SeqCst);
                a.fetch_sub(1, AtomicOrdering::SeqCst);
                Ok(vec![])
            }));
        g.connect(output(1, 0), input(2, 0, 0));
        g.connect(output(1, 1), input(2, 0, 0));
        g.trigger(1, vec![input(1, 0, 0)]);
        let r = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc(0_i64)]).unwrap();
        assert!(r.success());
        assert_eq!(r.count(2), 2);
        assert_eq!(peak.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn queued_producer_must_execute_before_wave_settles() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0, vec![slot(0, I, BufferMode::Immediate)])],
            vec![out(0, I)], CohortMode::Inherit, vec![],
            |ctx| Ok(vec![emit(0, ctx.arg::<i64>(0)? + 1)])));
        g.add_node(node(2, vec![all(0, vec![slot(0, I, BufferMode::Immediate)])],
            vec![out(0, I)], CohortMode::Inherit, vec![],
            |ctx| Ok(vec![emit(0, ctx.arg::<i64>(0)? * 2)])));
        g.add_node(node(3, vec![all(0, vec![slot(0, I, BufferMode::Immediate)])],
            vec![], CohortMode::Inherit, vec![], |_ctx| Ok(vec![])));
        g.connect(output(1, 0), input(2, 0, 0));
        g.connect(output(2, 0), input(3, 0, 0));
        g.trigger(1, vec![input(1, 0, 0)]);
        let r = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc(3_i64)]).unwrap();
        assert!(r.success(), "{:?}", r.errors);
        assert_eq!(r.count(1), 1);
        assert_eq!(r.count(2), 1);
        assert_eq!(r.count(3), 1);
        assert_eq!(r.published.iter().find(|p| p.from == output(2, 0))
            .unwrap().value.get::<i64>().unwrap(), 8);
    }


    #[test]
    fn expression_diamond_produces_nested_function_string() {
        let r = expression_diamond().fire(1, vec![enc("x".to_string())]).unwrap();
        assert!(r.success(), "{:?}", r.errors);
        assert_eq!(published_string(&r, output(4, 0)), "d(b(a(x)),c(a(x)))");
        assert_eq!((r.count(1), r.count(2), r.count(3), r.count(4)), (1, 1, 1, 1));
    }

    #[test]
    fn expression_shared_branch_shape_stays_structurally_correct() {
        let r = expression_shared_branch().fire(1, vec![enc("x".to_string())]).unwrap();
        assert!(r.success(), "{:?}", r.errors);
        assert_eq!(
            published_string(&r, output(7, 0)),
            "g(d(b(a(x))),e(b(a(x))),f(c(a(x))))"
        );
        assert_eq!((r.count(1), r.count(2), r.count(3), r.count(4), r.count(5), r.count(6), r.count(7)),
            (1, 1, 1, 1, 1, 1, 1));
    }

    #[test]
    fn expression_nested_joins_show_reused_subexpressions() {
        let r = expression_nested_joins().fire(1, vec![enc("x".to_string())]).unwrap();
        assert!(r.success(), "{:?}", r.errors);
        assert_eq!(
            published_string(&r, output(7, 0)),
            "g(d(b(a(x)),c(a(x))),e(c(a(x))),f(b(a(x))))"
        );
        assert_eq!((r.count(1), r.count(2), r.count(3), r.count(4), r.count(5), r.count(6), r.count(7)),
            (1, 1, 1, 1, 1, 1, 1));
    }

    // --- Policy / colored-upstream integration tests ---

    fn any_two_graph(enabled: &[u16]) -> Engine {
        let mut g = FlatGraphBuilder::default();
        let outputs: Vec<_> = (0..3).map(|i| out(i, S)).collect();
        let subset = enabled.to_vec();
        g.add_node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            outputs, CohortMode::Inherit, vec![], move |_ctx| {
                Ok(subset.iter().map(|&i| emit(i, format!("s{}", i))).collect())
            }));
        g.add_node(node(2, vec![custom_policy(0,
            (0..3).map(|i| slot(i, S, BufferMode::Deferred)).collect(), AnyTwoOfThree).args(|values| AnyTwoOfThree::build_args(values.selected))],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                let args = ctx.prepared::<AnyTwoArgs>()?;
                Ok(vec![emit(0, format!("pair({},{})", args.first.1, args.second.1))])
            }));
        for i in 0..3 { g.connect(output(1, i), input(2, 0, i)); }
        g.trigger(1, vec![input(1, 0, 0)]);
        Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
    }

    #[test]
    fn any_two_policy_accepts_all_three_combinations_without_new_node_kinds() {
        for (slots, expected) in [
            (vec![0, 1], "pair(s0,s1)"),
            (vec![0, 2], "pair(s0,s2)"),
            (vec![1, 2], "pair(s1,s2)"),
            (vec![0, 1, 2], "pair(s0,s1)"), // deterministic preference
        ] {
            let r = any_two_graph(&slots).fire(1, vec![enc("x".to_owned())]).unwrap();
            assert!(r.success(), "slots {:?}: {:?}", slots, r.errors);
            assert_eq!(r.count(2), 1, "slots {:?}", slots);
            assert_eq!(published_string(&r, output(2, 0)), expected);
        }
    }

    #[test]
    fn any_two_policy_rejects_only_one_candidate_at_settlement() {
        for lone in 0..3 {
            let r = any_two_graph(&[lone]).fire(1, vec![enc("x".to_owned())]).unwrap();
            assert_eq!(r.count(2), 0);
            assert_eq!(r.errors.len(), 1);
            assert!(matches!(&r.errors[0], WaveError::Incomplete { node: NodeId(2), group, .. }
                if group.present == vec![SlotId(lone)] && group.requirement == "at least two of three slots"));
        }
        // No candidate means no activated cohort and therefore no phantom error.
        let r = any_two_graph(&[]).fire(1, vec![enc("x".to_owned())]).unwrap();
        assert!(r.success());
        assert_eq!(r.count(2), 0);
    }

    fn pair_then_third_graph(emit_a: bool, emit_b: bool, emit_third: bool) -> Engine {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S), out(1, S), out(2, S)], CohortMode::Inherit, vec![],
            move |_ctx| {
                let mut outputs = Vec::new();
                if emit_a { outputs.push(emit(0, "left".to_owned())); }
                if emit_b { outputs.push(emit(1, "right".to_owned())); }
                outputs.push(emit(2, "signal".to_owned()));
                Ok(outputs)
            }));
        g.add_node(node(2, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], move |_ctx| {
                if emit_third { Ok(vec![emit(0, "third".to_owned())]) } else { Ok(vec![]) }
            }));
        g.add_node(node(3, vec![custom_policy(0, vec![
                slot(0, S, BufferMode::Deferred),
                slot(1, S, BufferMode::Deferred),
                slot(2, S, BufferMode::Deferred),
            ], PairThenThird).args(decode_pair_stage)],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                match ctx.prepared::<PairStageArgs>()? {
                    PairStageArgs::Pair { left, right } => Ok(vec![emit(0, format!("pair({left},{right})"))]),
                    PairStageArgs::Third { value } => {
                        assert_eq!(value, "third");
                        Ok(vec![]) // acknowledgment, no second output
                    }
                }
            }));
        g.connect(output(1, 0), input(3, 0, 0));
        g.connect(output(1, 1), input(3, 0, 1));
        g.connect(output(1, 2), input(2, 0, 0));
        g.connect(output(2, 0), input(3, 0, 2));
        g.trigger(1, vec![input(1, 0, 0)]);
        Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
    }

    #[test]
    fn two_slots_publish_one_value_then_missing_third_is_one_wave_error() {
        let r = pair_then_third_graph(true, true, false)
            .fire(1, vec![enc("x".to_owned())]).unwrap();
        assert_eq!(r.count(3), 1); // first phase did execute
        assert_eq!(published_string(&r, output(3, 0)), "pair(left,right)");
        assert_eq!(r.published.iter().filter(|p| p.from == output(3, 0)).count(), 1);
        assert_eq!(r.errors.len(), 1, "exactly one wave error: {:?}", r.errors);
        let first_value = r.trace.timeline.iter().position(|e| *e == WaveEvent::Published(output(3, 0))).unwrap();
        let missing_third = r.trace.timeline.iter().position(|e| *e == WaveEvent::Incomplete(NodeId(3), PortId(0))).unwrap();
        assert!(first_value < missing_third, "output must precede the terminal error: {:?}", r.trace.timeline);
        assert!(matches!(&r.errors[0], WaveError::Incomplete { node: NodeId(3), port: PortId(0), group }
            if group.present == vec![SlotId(0), SlotId(1)]
                && group.missing == vec![SlotId(2)]
                && group.requirement == "third slot after pair"));
        assert!(!r.success());
    }

    #[test]
    fn pair_then_third_exhaustive_boolean_arrival_matrix() {
        for mask in 0..8 {
            let a = mask & 1 != 0;
            let b = mask & 2 != 0;
            let third = mask & 4 != 0;
            let r = pair_then_third_graph(a, b, third)
                .fire(1, vec![enc("x".to_owned())]).unwrap();
            let first_stage = a && b;
            let activated = a || b || third;
            let expected_count = if first_stage { 1 + usize::from(third) } else { 0 };
            let expected_errors = if activated && !(first_stage && third) { 1 } else { 0 };
            assert_eq!(r.count(3), expected_count, "mask {mask:03b}");
            assert_eq!(r.published.iter().filter(|p| p.from == output(3, 0)).count(),
                usize::from(first_stage), "mask {mask:03b}");
            assert_eq!(r.errors.len(), expected_errors, "mask {mask:03b}: {:?}", r.errors);
        }
    }

    #[test]
    fn third_arrival_completes_second_phase_without_another_output() {
        let r = pair_then_third_graph(true, true, true)
            .fire(1, vec![enc("x".to_owned())]).unwrap();
        assert!(r.success(), "{:?}", r.errors);
        assert_eq!(r.count(3), 2); // pair and acknowledgment are distinct invocations
        assert_eq!(r.published.iter().filter(|p| p.from == output(3, 0)).count(), 1);
        assert_eq!(published_string(&r, output(3, 0)), "pair(left,right)");
    }

    #[test]
    fn missing_first_phase_slot_does_not_emit_a_partial_result() {
        for (a, b) in [(false, true), (true, false)] {
            let r = pair_then_third_graph(a, b, true)
                .fire(1, vec![enc("x".to_owned())]).unwrap();
            assert_eq!(r.count(3), 0);
            assert!(r.published.iter().all(|p| p.from != output(3, 0)));
            assert_eq!(r.errors.len(), 1);
            assert!(matches!(&r.errors[0], WaveError::Incomplete { node: NodeId(3), group, .. }
                if group.requirement == "first pair before third"));
        }
    }

    /// A -> B -> Choose
    ///   -> E -> C -> Choose
    /// B arrives before C. E is still runnable, and its structural descendants
    /// include C. The color remains live even before C's input is delivered.
    fn colored_graph(color_wait: bool, c_emits: bool, c_fails: bool) -> Engine {
        let mut g = FlatGraphBuilder::default();
        g.add_node(wrap(1, "a"));
        g.add_node(wrap(2, "b"));
        g.add_node(wrap(3, "e"));
        g.add_node(node(4, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], move |ctx| {
                if c_fails { return Err("intentional C failure".into()); }
                if c_emits { Ok(vec![emit(0, format!("c({})", ctx.arg::<String>(0)?))]) }
                else { Ok(vec![]) }
            }));
        let chooser = if color_wait {
            custom_policy(0, vec![slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred)],
                WhenUpstreamSettles::new(PreferPairOnce)).args(decode_chosen)
        } else {
            custom_policy(0, vec![slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred)],
                PreferPairOnce).args(decode_chosen)
        };
        g.add_node(node(5, vec![chooser], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
            let args = ctx.prepared::<ChosenArgs>()?;
            Ok(vec![emit(0, format!("choose({})", args.parts.join(",")))])
        }));
        // Independent additional branch: should NOT color the chooser.
        g.add_node(wrap(6, "unrelated"));
        g.connect(output(1, 0), input(2, 0, 0));
        g.connect(output(1, 0), input(3, 0, 0));
        g.connect(output(1, 0), input(6, 0, 0));
        g.connect(output(3, 0), input(4, 0, 0));
        // The slow path occupies slot 0; the fast path occupies slot 1.
        g.connect(output(2, 0), input(5, 0, 1));
        g.connect(output(4, 0), input(5, 0, 0));
        g.trigger(1, vec![input(1, 0, 0)]);
        Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
    }

    #[test]
    fn upstream_colors_prevent_early_choice_and_preserve_both_values() {
        let r = colored_graph(true, true, false)
            .fire(1, vec![enc("x".to_owned())]).unwrap();
        assert!(r.success(), "{:?}", r.errors);
        assert_eq!(published_string(&r, output(5, 0)),
            "choose(c(e(a(x))),b(a(x)))");
        assert_eq!(r.count(5), 1);
        assert!(r.trace.waits.iter().any(|w| w.node == NodeId(5) && w.upstream.contains(&NodeId(3))),
            "E must keep the upstream color alive: {:?}", r.trace.waits);
        assert!(r.trace.waits.iter().all(|w| !w.upstream.contains(&NodeId(6))),
            "unrelated work cannot block this port");
        let exec: Vec<_> = r.trace.executed.iter().map(|id| id.0).collect();
        assert!(exec.iter().position(|&id| id == 4) < exec.iter().position(|&id| id == 5));
    }

    #[test]
    fn without_coloring_early_eager_selection_discards_late_alternative() {
        let r = colored_graph(false, true, false)
            .fire(1, vec![enc("x".to_owned())]).unwrap();
        assert!(r.success(), "{:?}", r.errors);
        assert_eq!(r.count(5), 1); // per-cohort once policy
        assert_eq!(published_string(&r, output(5, 0)), "choose(b(a(x)))");
        let exec: Vec<_> = r.trace.executed.iter().map(|id| id.0).collect();
        assert!(exec.iter().position(|&id| id == 5) < exec.iter().position(|&id| id == 4));
    }

    #[test]
    fn skipped_upstream_output_retires_color_and_unblocks_choice() {
        let r = colored_graph(true, false, false)
            .fire(1, vec![enc("x".to_owned())]).unwrap();
        assert!(r.success(), "{:?}", r.errors);
        assert_eq!(r.count(4), 1);
        assert_eq!(r.count(5), 1);
        assert_eq!(published_string(&r, output(5, 0)), "choose(b(a(x)))");
        assert!(r.trace.waits.iter().any(|w| w.node == NodeId(5)));
    }

    #[test]
    fn failed_upstream_completion_releases_color_but_retains_error() {
        let r = colored_graph(true, false, true)
            .fire(1, vec![enc("x".to_owned())]).unwrap();
        assert_eq!(r.count(5), 1);
        assert_eq!(published_string(&r, output(5, 0)), "choose(b(a(x)))");
        assert_eq!(r.errors.len(), 1);
        assert!(matches!(&r.errors[0], WaveError::NodeFailed(NodeId(4), reason)
            if reason == "intentional C failure"));
    }

    #[test]
    fn all_policy_with_coloring_still_errors_when_required_branch_skips() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(wrap(1, "a"));
        g.add_node(wrap(2, "b"));
        g.add_node(node(3, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], |_ctx| Ok(vec![])));
        g.add_node(node(4, vec![custom_policy(0, vec![
            slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred),
        ], WhenUpstreamSettles::new(AllSameCohort))],
            vec![out(0, S)], CohortMode::Inherit, vec![], |_ctx| Ok(vec![emit(0, "bad".to_owned())])));
        g.connect(output(1, 0), input(2, 0, 0));
        g.connect(output(1, 0), input(3, 0, 0));
        g.connect(output(2, 0), input(4, 0, 0));
        g.connect(output(3, 0), input(4, 0, 1));
        g.trigger(1, vec![input(1, 0, 0)]);
        let r = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("x".to_owned())]).unwrap();
        assert_eq!(r.count(4), 0);
        assert_eq!(r.errors.len(), 1);
        assert!(matches!(&r.errors[0], WaveError::Incomplete { node: NodeId(4), group, .. }
            if group.missing == vec![SlotId(1)]));
    }

    #[test]
    fn values_from_different_cohorts_cannot_satisfy_two_slot_policy() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S), out(1, S)], CohortMode::Inherit, vec![],
            |_ctx| Ok(vec![emit(0, "a".to_owned()), emit(1, "b".to_owned())])));
        g.add_node(node(2, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::FreshPerInvocation, vec![],
            |ctx| Ok(vec![emit(0, ctx.arg::<String>(0)?)])));
        g.add_node(node(3, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![],
            |ctx| Ok(vec![emit(0, ctx.arg::<String>(0)?)])));
        g.add_node(node(4, vec![custom_policy(0, vec![slot(0, S, BufferMode::Deferred),
            slot(1, S, BufferMode::Deferred)], WhenUpstreamSettles::new(AllSameCohort))],
            vec![out(0, S)], CohortMode::Inherit, vec![],
            |_ctx| Err("must not execute".into())));
        g.connect(output(1, 0), input(2, 0, 0));
        g.connect(output(1, 1), input(3, 0, 0));
        g.connect(output(2, 0), input(4, 0, 0));
        g.connect(output(3, 0), input(4, 0, 1));
        g.trigger(1, vec![input(1, 0, 0)]);
        let r = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("x".to_owned())]).unwrap();
        assert_eq!(r.count(4), 0);
        assert_eq!(r.errors.len(), 2, "each incomplete cohort is diagnosed independently");
        assert!(r.errors.iter().all(|e| matches!(e, WaveError::Incomplete { node: NodeId(4), .. })));
    }

    #[test]
    fn colored_wait_does_not_hold_for_dormant_unadmitted_branch() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(wrap(1, "a"));
        g.add_node(wrap(2, "b"));
        g.add_node(wrap(3, "unreachable"));
        g.add_node(node(4, vec![custom_policy(0, vec![slot(0, S, BufferMode::Deferred),
            slot(1, S, BufferMode::Deferred)], WhenUpstreamSettles::new(PreferPairOnce)).args(decode_chosen)],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, format!("take({})", ctx.prepared::<ChosenArgs>()?.parts.join(",")))])
            }));
        g.connect(output(1, 0), input(2, 0, 0));
        g.connect(output(2, 0), input(4, 0, 0));
        g.connect(output(3, 0), input(4, 0, 1));
        g.trigger(1, vec![input(1, 0, 0)]);
        let r = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("x".to_owned())]).unwrap();
        assert!(r.success(), "{:?}", r.errors);
        assert_eq!(r.count(3), 0);
        assert_eq!(published_string(&r, output(4, 0)), "take(b(a(x)))");
        assert!(r.trace.waits.iter().all(|w| !w.upstream.contains(&NodeId(3))));
    }

    #[test]
    fn third_slot_arriving_first_is_buffered_until_pair_stage() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![custom_policy(0, vec![
            slot(0, S, BufferMode::Deferred),
            slot(1, S, BufferMode::Deferred),
            slot(2, S, BufferMode::Deferred),
        ], PairThenThird).args(decode_pair_stage)], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
            match ctx.prepared::<PairStageArgs>()? {
                PairStageArgs::Pair { left, right } => Ok(vec![emit(0, format!("{left}+{right}"))]),
                PairStageArgs::Third { value } => { assert_eq!(value, "third"); Ok(vec![]) }
            }
        }));
        // Input slots are declared 0,1,2, but the trigger deliberately delivers 2 first.
        g.trigger(1, vec![input(1, 0, 2), input(1, 0, 1), input(1, 0, 0)]);
        let r = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("third".to_owned()), enc("b".to_owned()), enc("a".to_owned())]).unwrap();
        assert!(r.success(), "{:?}", r.errors);
        assert_eq!(r.count(1), 2);
        assert_eq!(published_string(&r, output(1, 0)), "a+b");
        assert_eq!(r.published.len(), 1);
    }

    #[test]
    fn incomplete_stage_history_does_not_leak_between_independent_waves() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S), out(1, S), out(2, S)], CohortMode::Inherit, vec![], |ctx| {
                let value = ctx.arg::<String>(0)?;
                let mut outputs = vec![emit(0, "a".to_owned()), emit(1, "b".to_owned())];
                if value == "complete" { outputs.push(emit(2, "c".to_owned())); }
                Ok(outputs)
            }));
        g.add_node(node(2, vec![custom_policy(0, vec![
            slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred),
            slot(2, S, BufferMode::Deferred),
        ], PairThenThird).args(decode_pair_stage)], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
            match ctx.prepared::<PairStageArgs>()? {
                PairStageArgs::Pair { .. } => Ok(vec![emit(0, "first".to_owned())]),
                PairStageArgs::Third { .. } => Ok(vec![]),
            }
        }));
        for slot_id in 0..3 { g.connect(output(1, slot_id), input(2, 0, slot_id)); }
        g.trigger(1, vec![input(1, 0, 0)]);
        let e = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let first = e.fire(1, vec![enc("incomplete".to_owned())]).unwrap();
        let second = e.fire(1, vec![enc("complete".to_owned())]).unwrap();
        assert_ne!(first.wave, second.wave);
        assert_eq!(first.errors.len(), 1);
        assert_eq!(first.count(2), 1);
        assert_eq!(published_string(&first, output(2, 0)), "first");
        assert!(second.success(), "{:?}", second.errors);
        assert_eq!(second.count(2), 2);
        assert_eq!(published_string(&second, output(2, 0)), "first");
    }

    #[test]
    fn consumed_pair_still_owes_third_after_output_is_consumed_downstream() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![custom_policy(0, vec![
            slot(0, S, BufferMode::Deferred),
            slot(1, S, BufferMode::Deferred),
            slot(2, S, BufferMode::Deferred),
        ], PairThenThird).args(decode_pair_stage)], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
            match ctx.prepared::<PairStageArgs>()? {
                PairStageArgs::Pair { left, right } => Ok(vec![emit(0, format!("{left}+{right}"))]),
                PairStageArgs::Third { .. } => Ok(vec![]),
            }
        }));
        g.add_node(wrap(2, "downstream"));
        g.connect(output(1, 0), input(2, 0, 0));
        g.trigger(1, vec![input(1, 0, 0), input(1, 0, 1)]);
        let r = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("a".to_owned()), enc("b".to_owned())]).unwrap();
        assert_eq!(r.count(1), 1);
        assert_eq!(r.count(2), 1);
        assert_eq!(published_string(&r, output(2, 0)), "downstream(a+b)");
        assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
        assert!(matches!(&r.errors[0], WaveError::Incomplete { node: NodeId(1), group, .. }
            if group.missing == vec![SlotId(2)] && group.requirement == "third slot after pair"));
    }

    #[test]
    fn independently_colored_cohorts_settle_missing_third_once_only() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S), out(1, S)], CohortMode::Inherit, vec![], |_ctx| {
                Ok(vec![emit(0, "complete".to_owned()), emit(1, "incomplete".to_owned())])
            }));
        g.add_node(node(2, vec![all(0, vec![slot(0, S, BufferMode::Deferred)])],
            vec![out(0, S), out(1, S), out(2, S)], CohortMode::FreshPerInvocation,
            vec![], |ctx| {
                let value = ctx.arg::<String>(0)?;
                let mut outputs = vec![emit(0, format!("left-{value}")), emit(1, format!("right-{value}"))];
                if value == "complete" { outputs.push(emit(2, "third".to_owned())); }
                Ok(outputs)
            }));
        g.add_node(node(3, vec![custom_policy(0, vec![
            slot(0, S, BufferMode::Deferred),
            slot(1, S, BufferMode::Deferred),
            slot(2, S, BufferMode::Deferred),
        ], PairThenThird).args(decode_pair_stage)], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
            match ctx.prepared::<PairStageArgs>()? {
                PairStageArgs::Pair { left, right } => Ok(vec![emit(0, format!("pair({left},{right})"))]),
                PairStageArgs::Third { .. } => Ok(vec![]),
            }
        }));
        g.connect(output(1, 0), input(2, 0, 0));
        g.connect(output(1, 1), input(2, 0, 0));
        for slot_id in 0..3 { g.connect(output(2, slot_id), input(3, 0, slot_id)); }
        g.trigger(1, vec![input(1, 0, 0)]);
        let r = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("seed".to_owned())]).unwrap();
        assert_eq!(r.count(2), 2);
        assert_eq!(r.count(3), 3); // two pairs, one third-stage acknowledgement
        assert_eq!(r.published.iter().filter(|p| p.from == output(3, 0)).count(), 2);
        assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
        assert!(matches!(&r.errors[0], WaveError::Incomplete { node: NodeId(3), group, .. }
            if group.missing == vec![SlotId(2)] && group.requirement == "third slot after pair"));
    }

    #[test]
    fn repeated_cohorts_remain_independent_under_colored_upstream_gate() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S), out(1, S)], CohortMode::Inherit, vec![],
            |_ctx| Ok(vec![emit(0, "x".to_owned()), emit(1, "y".to_owned())])));
        g.add_node(node(2, vec![all(0, vec![slot(0, S, BufferMode::Deferred)])],
            vec![out(0, S)], CohortMode::FreshPerInvocation, vec![], |ctx| {
                Ok(vec![emit(0, format!("a({})", ctx.arg::<String>(0)?))])
            }));
        g.add_node(wrap(3, "b"));
        g.add_node(wrap(4, "e"));
        g.add_node(wrap(5, "c"));
        g.add_node(node(6, vec![custom_policy(0, vec![
            slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred),
        ], WhenUpstreamSettles::new(PreferPairOnce)).args(decode_chosen)], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
            Ok(vec![emit(0, format!("d({})", ctx.prepared::<ChosenArgs>()?.parts.join(",")))])
        }));
        g.connect(output(1, 0), input(2, 0, 0));
        g.connect(output(1, 1), input(2, 0, 0));
        g.connect(output(2, 0), input(3, 0, 0));
        g.connect(output(2, 0), input(4, 0, 0));
        g.connect(output(4, 0), input(5, 0, 0));
        // Keep the slot contract aligned with colored_graph: slow is slot 0.
        g.connect(output(3, 0), input(6, 0, 1));
        g.connect(output(5, 0), input(6, 0, 0));
        g.trigger(1, vec![input(1, 0, 0)]);
        let r = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("start".to_owned())]).unwrap();
        assert!(r.success(), "{:?}", r.errors);
        assert_eq!(r.count(2), 2);
        assert_eq!(r.count(6), 2);
        let mut values: Vec<_> = r.published.iter().filter(|p| p.from == output(6, 0))
            .map(|p| p.value.get::<String>().unwrap()).collect();
        values.sort();
        assert_eq!(values, vec![
            "d(c(e(a(x))),b(a(x)))".to_owned(),
            "d(c(e(a(y))),b(a(y)))".to_owned(),
        ]);
    }

    struct RejectPreparedPolicy;
    impl PortPolicy for RejectPreparedPolicy {
        fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
            AnyOne.select(snapshot)
        }
        fn incomplete(&self, _snapshot: &PortSnapshot) -> Vec<IncompleteGroup> { vec![] }

    }

    #[test]
    fn invalid_typed_args_rejects_atomically_and_reports_once() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![],
            |ctx| Ok(vec![emit(0, ctx.arg::<String>(0)?)])));
        g.add_node(node(2, vec![custom_policy(0, vec![slot(0, S, BufferMode::Deferred)], RejectPreparedPolicy)
            .validate(|_| Err("invalid Args constructor".into()))],
            vec![out(0, S)], CohortMode::Inherit, vec![],
            |_ctx| Err("must not execute".into())));
        g.connect(output(1, 0), input(2, 0, 0));
        g.trigger(1, vec![input(1, 0, 0)]);
        let r = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("test".to_owned())]).unwrap();
        assert_eq!(r.count(1), 1);
        assert_eq!(r.count(2), 0);
        assert_eq!(r.errors, vec![WaveError::InvalidArgs(NodeId(2), PortId(0),
            CohortId(0), "invalid Args constructor".into())]);
        assert_eq!(r.published.iter().filter(|p| p.from == output(1, 0)).count(), 1);
    }

    #[test]
    fn colored_multiport_trigger_admission_reserves_upstream_before_downstream() {
        // The trigger delivers both D.slot0 and E.input atomically. D has a
        // SMALLER node ID than E: naive node-ID admission would select early.
        // Topological reservation makes E live before D checks its colors.
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![custom_policy(0, vec![
            slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred),
        ], WhenUpstreamSettles::new(PreferPairOnce)).args(decode_chosen)], vec![out(0, S)],
            CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, format!("choose({})", ctx.prepared::<ChosenArgs>()?.parts.join(",")))])
            }));
        g.add_node(wrap(2, "e"));
        g.add_node(wrap(3, "c"));
        g.connect(output(2, 0), input(3, 0, 0));
        g.connect(output(3, 0), input(1, 0, 1));
        g.trigger(1, vec![input(1, 0, 0), input(2, 0, 0)]);
        let r = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("left".to_owned()), enc("x".to_owned())]).unwrap();
        assert!(r.success(), "{:?}", r.errors);
        assert_eq!(r.count(1), 1);
        assert_eq!(published_string(&r, output(1, 0)), "choose(left,c(e(x)))");
        assert!(r.trace.waits.iter().any(|w| w.node == NodeId(1) && w.upstream.contains(&NodeId(2))));
    }

    #[test]
    fn failed_pair_invocation_cannot_admit_its_third_phase() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![custom_policy(0, vec![
            slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred),
            slot(2, S, BufferMode::Deferred),
        ], PairThenThird).args(decode_pair_stage)], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
            match ctx.prepared::<PairStageArgs>()? {
                PairStageArgs::Pair { .. } => Err("phase one failed".into()),
                PairStageArgs::Third { .. } => Err("third must never run".into()),
            }
        }));
        g.trigger(1, vec![input(1, 0, 2), input(1, 0, 0), input(1, 0, 1)]);
        let r = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("third".to_owned()), enc("a".to_owned()), enc("b".to_owned())]).unwrap();
        assert_eq!(r.count(1), 1);
        assert_eq!(r.errors.len(), 1);
        assert!(matches!(&r.errors[0], WaveError::NodeFailed(NodeId(1), reason)
            if reason == "phase one failed"));
        assert!(r.published.is_empty());
    }

    #[test]
    fn graph_rejects_edge_type_mismatch() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0, vec![slot(0, I, BufferMode::Immediate)])],
            vec![out(0, I)], CohortMode::Inherit, vec![], |_ctx| Ok(vec![])));
        g.add_node(node(2, vec![all(0, vec![slot(0, String::SCHEMA, BufferMode::Immediate)])],
            vec![], CohortMode::Inherit, vec![], |_ctx| Ok(vec![])));
        g.connect(output(1, 0), input(2, 0, 0));
        assert!(g.build().err().unwrap().contains("schema mismatch"));
    }


}

// Conformance tests are deliberately separated from scenario tests above.
// A failure in the graph is an expected result when asserted through WaveReport;
// only a violated test assertion is a failing test.
#[cfg(test)]
mod conformance {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    const S: &'static str = <String as ContractValue>::SCHEMA;
    const I: &'static str = <i64 as ContractValue>::SCHEMA;

    fn passthrough(id: u16) -> NodeTemplate {
        node(id, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![],
            |ctx| Ok(vec![emit(0, ctx.arg::<String>(0)?) ]))
    }
    fn scalar(id: u16) -> NodeTemplate {
        node(id, vec![all(0, vec![slot(0, I, BufferMode::Immediate)])],
            vec![out(0, I)], CohortMode::Inherit, vec![],
            |ctx| Ok(vec![emit(0, ctx.arg::<i64>(0)?)]))
    }
    fn identity_graph() -> Engine {
        let mut g = FlatGraphBuilder::default();
        g.add_node(passthrough(1));
        g.trigger(1, vec![input(1, 0, 0)]);
        Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
    }
    fn val(report: &WaveReport, node_id: u16) -> String {
        report.published.iter().find(|p| p.from == output(node_id, 0))
            .unwrap_or_else(|| panic!("node {node_id} did not publish"))
            .value.get::<String>().unwrap()
    }
    fn single_with_policy(policy: impl PortPolicy + 'static, mode: BufferMode) -> Arc<Graph> {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![custom_policy(0, vec![slot(0, S, mode), slot(1, S, mode)], policy)],
            vec![], CohortMode::Inherit, vec![], |_ctx| Ok(vec![])));
        g.build().unwrap()
    }
    fn optional_pair_node(id: u16) -> NodeTemplate {
        node(id, vec![custom_policy(0, vec![
                slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred),
            ], WhenUpstreamSettles::new(OptionalPair)).args(decode_optional_pair)],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                let args = ctx.prepared::<OptionalPairArgs>()?;
                Ok(vec![emit(0, format!("{}|{}",
                    args.first.as_deref().unwrap_or("_"),
                    args.second.as_deref().unwrap_or("_")))])
            })
    }
    fn passthrough_module(id: &str, node_id: u16) -> ModuleEntry {
        let mut module = ModuleDefBuilder::new(id);
        module.node(passthrough(node_id));
        module.input(0, "input", S, vec![ModuleInputTarget::Node(input(node_id, 0, 0))]);
        module.output(0, "output", S, ModuleOutputSource::Node(output(node_id, 0)));
        module.build()
    }
    fn named_transform(id: u16, label: &'static str) -> NodeTemplate {
        node(id, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], move |ctx| {
                Ok(vec![emit(0, format!("{label}({})", ctx.arg::<String>(0)?))])
            })
    }
    fn two_step_module(id: &str) -> ModuleEntry {
        let mut module = ModuleDefBuilder::new(id);
        module.node(named_transform(1, "first"));
        module.node(named_transform(2, "second"));
        module.connect(ModuleOutputSource::Node(output(1, 0)),
            ModuleInputTarget::Node(input(2, 0, 0)));
        module.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        module.output(0, "output", S, ModuleOutputSource::Node(output(2, 0)));
        module.build()
    }
    fn two_step_root(id: &str, module_definition: Option<&str>) -> ModuleEntry {
        let mut root = ModuleDefBuilder::new(id);
        if let Some(definition) = module_definition {
            root.try_instance("pipeline", definition).unwrap();
            root.input(0, "input", S, vec![ModuleInputTarget::Instance {
                name: "pipeline".into(), port: 0,
            }]);
            root.output(0, "output", S, ModuleOutputSource::Instance {
                name: "pipeline".into(), port: 0,
            });
        } else {
            root.node(named_transform(1, "first"));
            root.node(named_transform(2, "second"));
            root.connect(ModuleOutputSource::Node(output(1, 0)),
                ModuleInputTarget::Node(input(2, 0, 0)));
            root.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
            root.output(0, "output", S, ModuleOutputSource::Node(output(2, 0)));
        }
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        root.build()
    }
    fn fork_join_body(id: &str, omit_right_for: Option<&'static str>, fail_right_for: Option<&'static str>) -> ModuleEntry {
        let mut module = ModuleDefBuilder::new(id);
        module.node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S), out(1, S)], CohortMode::Inherit, vec![], |ctx| {
                let value = ctx.arg::<String>(0)?;
                Ok(vec![emit(0, format!("left({value})")), emit(1, value)])
            }));
        module.node(node(2, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], move |ctx| {
                let value = ctx.arg::<String>(0)?;
                if omit_right_for == Some(value.as_str()) { return Ok(vec![]); }
                if fail_right_for == Some(value.as_str()) { return Err("right branch failed".into()); }
                Ok(vec![emit(0, format!("right({value})"))])
            }));
        module.node(node(3, vec![all(0, vec![slot(0, S, BufferMode::Deferred),
            slot(1, S, BufferMode::Deferred)])], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, format!("join({},{})",
                    ctx.arg::<String>(0)?, ctx.arg::<String>(1)?))])
            }));
        module.connect(ModuleOutputSource::Node(output(1, 0)),
            ModuleInputTarget::Node(input(3, 0, 0)));
        module.connect(ModuleOutputSource::Node(output(1, 1)),
            ModuleInputTarget::Node(input(2, 0, 0)));
        module.connect(ModuleOutputSource::Node(output(2, 0)),
            ModuleInputTarget::Node(input(3, 0, 1)));
        module.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        module.output(0, "output", S, ModuleOutputSource::Node(output(3, 0)));
        module.build()
    }
    fn root_for_module(id: &str, definition: &str) -> ModuleEntry {
        let mut root = ModuleDefBuilder::new(id);
        root.try_instance("work", definition).unwrap();
        root.input(0, "input", S, vec![ModuleInputTarget::Instance {
            name: "work".into(), port: 0,
        }]);
        root.output(0, "output", S, ModuleOutputSource::Instance {
            name: "work".into(), port: 0,
        });
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        root.build()
    }
    fn flat_fork_join_root(id: &str, omit: Option<&'static str>, fail: Option<&'static str>) -> ModuleEntry {
        let mut root = fork_join_body(id, omit, fail);
        root.triggers.push(ModuleTrigger { id: TriggerId(1), name: "1".into(),
            targets: vec![ModuleTriggerTarget::PublicInput(0)] });
        root
    }
    fn connected_module_root(id: &str, definition: &str) -> ModuleEntry {
        let mut root = ModuleDefBuilder::new(id);
        root.node(named_transform(1, "before"));
        root.node(named_transform(4, "after"));
        root.try_instance("pipeline", definition).unwrap();
        root.connect(ModuleOutputSource::Node(output(1, 0)),
            ModuleInputTarget::Instance { name: "pipeline".into(), port: 0 });
        root.connect(ModuleOutputSource::Instance { name: "pipeline".into(), port: 0 },
            ModuleInputTarget::Node(input(4, 0, 0)));
        root.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        root.output(0, "output", S, ModuleOutputSource::Node(output(4, 0)));
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        root.build()
    }
    fn module_process_root(id: &str, definition: &str, nested: bool) -> ModuleEntry {
        let mut root = ModuleDefBuilder::new(id);
        if nested {
            root.try_instance("inner", definition).unwrap();
            root.input(0, "input", S, vec![ModuleInputTarget::Instance {
                name: "inner".into(), port: 0,
            }]);
            root.output(0, "output", S, ModuleOutputSource::Instance {
                name: "inner".into(), port: 0,
            });
        } else {
            root.node(passthrough(1));
            root.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
            root.output(0, "output", S, ModuleOutputSource::Node(output(1, 0)));
        }
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        root.build()
    }
    fn compile_module(definitions: Vec<ModuleEntry>, root: &str) -> Result<Arc<Graph>, String> {
        let mut catalog = ModuleCatalog::default();
        for definition in definitions { catalog.add(definition)?; }
        catalog.compile(root)
    }
    fn named_lookup_graph() -> Arc<Graph> {
        let mut deepest = ModuleDefBuilder::new("deepest-definition");
        deepest.add_named("execute", passthrough(1)).unwrap();
        deepest.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        deepest.output(0, "output", S, ModuleOutputSource::Node(output(1, 0)));

        let mut inner = ModuleDefBuilder::new("inner-definition");
        inner.try_instance("deep", "deepest-definition").unwrap();
        inner.input(0, "input", S, vec![ModuleInputTarget::Instance { name: "deep".into(), port: 0 }]);
        inner.output(0, "output", S, ModuleOutputSource::Instance { name: "deep".into(), port: 0 });

        let mut process = ModuleDefBuilder::new("process-definition");
        process.add_named("validate", passthrough(1)).unwrap();
        process.try_instance("inner", "inner-definition").unwrap();
        process.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        process.output(0, "output", S, ModuleOutputSource::Node(output(1, 0)));

        let mut root = ModuleDefBuilder::new("lookup-root");
        let mut source = passthrough(1);
        source.name = Some("source".into());
        root.add(NodeDef::Node(source)).unwrap();
        root.node(passthrough(90));
        root.add(NodeDef::Module(ModuleInstance { name: "process".into(),
            definition: "process-definition".into() })).unwrap();
        root.add(NodeDef::Module(ModuleInstance { name: "backup".into(),
            definition: "process-definition".into() })).unwrap();
        root.input(0, "incoming", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        root.output(0, "outgoing", S, ModuleOutputSource::Node(output(1, 0)));
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        compile_module(vec![deepest.build(), inner.build(), process.build(), root.build()], "lookup-root").unwrap()
    }
    fn module_outputs(report: &WaveReport, graph: &Graph) -> Vec<(u16, String, u64)> {
        let mut outputs: Vec<_> = graph.metadata.outputs.iter().flat_map(|(port, source)| {
            report.published.iter().filter(move |published| published.from == *source)
                .map(move |published| (*port, published.value.get::<String>().unwrap(), published.cohort.0))
        }).collect();
        outputs.sort();
        outputs
    }
    fn synthetic(cohort: u64, slots: &[(u16, u64)]) -> PortSnapshot {
        PortSnapshot {
            slots: [0_u16,1,2].into_iter().map(|id| SlotSnapshot {
                slot: SlotId(id), name: None,
                candidates: slots.iter().filter(|(slot,_)| *slot == id)
                    .map(|(slot,delivery)| Candidate { slot: SlotId(*slot), delivery: DeliveryId(*delivery), cohort: CohortId(cohort) }).collect(),
        }).collect(),
            reservations: vec![], colors: BTreeMap::new(),
        }
    }

    struct RejectBadCohort;
    impl PortPolicy for RejectBadCohort {
        fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
            AllSameCohort.select(snapshot)
        }
        fn incomplete(&self, snapshot: &PortSnapshot) -> Vec<IncompleteGroup> {
            AllSameCohort.incomplete(snapshot)
        }

    }

    // 1. Values, types and authoring contracts.
    #[test]
    fn value_codec_roundtrip_and_schema_identity() {
        for n in [i64::MIN, -1, 0, 1, 42, i64::MAX] { assert_eq!(enc(n).get::<i64>().unwrap(), n); }
        for s in ["", "x", "hello", "日本語", "😀"] {
            assert_eq!(enc(s.to_string()).get::<String>().unwrap(), s);
        }
        assert!(enc(1_i64).get::<String>().is_err());
        assert!(enc("hi".to_string()).get::<i64>().is_err());
    }
    #[test]
    fn malformed_value_payload_is_rejected_without_panic() {
        assert!(Value { schema: I, bytes: vec![1,2], local: None }.get::<i64>().is_err());
        assert!(Value { schema: S, bytes: vec![0xff,0xfe], local: None }.get::<String>().is_err());
    }
    #[test]
    fn unknown_trigger_does_not_admit_a_wave() {
        let e = identity_graph();
        assert!(e.fire(300, vec![enc("x".to_string())]).is_err());
        let ok = e.fire(1, vec![enc("x".to_string())]).unwrap();
        assert_eq!(ok.wave, WaveId(1));
        assert!(ok.success());
    }
    #[test]
    fn wrong_trigger_arity_is_rejected_before_admission() {
        let e = identity_graph();
        assert!(e.fire(1, vec![]).is_err());
        assert!(e.fire(1, vec![enc("x".to_string()),enc("y".to_string())]).is_err());
        let ok = e.fire(1, vec![enc("z".to_string())]).unwrap();
        assert!(ok.success());
        assert_eq!(ok.wave, WaveId(1));
    }
    #[test]
    fn trigger_wrong_schema_rejects_atomically_and_next_wave_works() {
        let e = identity_graph();
        assert!(e.fire(1, vec![enc(99_i64)]).is_err());
        let ok = e.fire(1, vec![enc("valid".to_string())]).unwrap();
        assert!(ok.success());
        assert_eq!(ok.wave, WaveId(1));
        assert_eq!(val(&ok, 1), "valid");
    }
    #[test]
    fn duplicated_immediate_trigger_address_rejected_atomically() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(passthrough(1));
        g.trigger(1, vec![input(1,0,0), input(1,0,0)]);
        g.trigger(2, vec![input(1,0,0)]);
        let e = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        assert!(e.fire(1, vec![enc("a".to_string()),enc("b".to_string())]).is_err());
        let r = e.fire(2, vec![enc("good".to_string())]).unwrap();
        assert_eq!(r.wave, WaveId(1));
        assert_eq!(val(&r, 1), "good");
    }

    // 2. Compiler and structural boundaries (all are rejection tests).
    #[test]
    fn compiler_rejects_duplicate_port_ids() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0,vec![slot(0,S,BufferMode::Immediate)]),
                                all(0,vec![slot(1,S,BufferMode::Immediate)])],
            vec![], CohortMode::Inherit, vec![], |_| Ok(vec![])));
        assert!(g.build().err().unwrap().contains("duplicate/empty port"));
    }
    #[test]
    fn compiler_rejects_empty_ports() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0,vec![])], vec![], CohortMode::Inherit, vec![], |_| Ok(vec![])));
        assert!(g.build().err().unwrap().contains("duplicate/empty port"));
    }
    #[test]
    fn compiler_rejects_duplicate_input_slots_within_port() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0,vec![slot(0,S,BufferMode::Immediate),slot(0,S,BufferMode::Deferred)])],
            vec![], CohortMode::Inherit, vec![], |_| Ok(vec![])));
        assert!(g.build().err().unwrap().contains("duplicate input slot"));
    }
    #[test]
    fn compiler_rejects_duplicate_output_slots() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0,vec![slot(0,S,BufferMode::Immediate)])],
            vec![out(0,S),out(0,S)], CohortMode::Inherit, vec![], |_| Ok(vec![])));
        assert!(g.build().err().unwrap().contains("duplicate output slot"));
    }
    #[test]
    fn compiler_rejects_duplicate_connections() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(passthrough(1));g.add_node(passthrough(2));
        g.connect(output(1,0),input(2,0,0));
        g.connect(output(1,0),input(2,0,0));
        assert!(g.build().err().unwrap().contains("duplicate connection"));
    }
    #[test]
    fn generated_edge_ids_are_deterministic_and_fanout_is_ordinary_edges() {
        let mut g = FlatGraphBuilder::default();
        for id in 1..=3 { g.add_node(passthrough(id)); }
        g.fanout(output(1, 0), [input(2, 0, 0), input(3, 0, 0)]);
        let graph = g.build().unwrap();
        let edges = &graph.by_output[&output(1, 0)];
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].id, EdgeId(1));
        assert_eq!(edges[1].id, EdgeId(2));
        assert_eq!(edges[0].to, input(2, 0, 0));
        assert_eq!(edges[1].to, input(3, 0, 0));
    }
    #[test]
    fn module_connections_accept_node_addresses_without_wrapper_enums() {
        let mut m = ModuleDefBuilder::new("fanout-module");
        for id in 1..=3 { m.node(passthrough(id)); }
        m.fanout(output(1, 0), [input(2, 0, 0), input(3, 0, 0)]);
        let entry = m.build();
        assert_eq!(entry.definition.edges.len(), 2);
        assert_eq!(entry.definition.edges[0].from, ModuleOutputSource::Node(output(1, 0)));
        assert_eq!(entry.definition.edges[1].to, ModuleInputTarget::Node(input(3, 0, 0)));
        let mut catalog = ModuleCatalog::new();
        catalog.add(entry).unwrap();
        let graph = catalog.compile("fanout-module").unwrap();
        assert_eq!(graph.metadata.edges.len(), 2);
        let destinations = &graph.by_output[&output(1, 0)];
        assert_eq!(destinations.len(), 2);
        assert_eq!(destinations[0].id, EdgeId(1));
        assert_eq!(destinations[1].id, EdgeId(2));
    }
    #[test]
    fn module_duplicate_connections_are_rejected_after_flattening() {
        let mut m = ModuleDefBuilder::new("duplicate-connect");
        m.node(passthrough(1)); m.node(passthrough(2));
        m.connect(output(1, 0), input(2, 0, 0));
        m.connect(output(1, 0), input(2, 0, 0));
        let mut catalog = ModuleCatalog::new();
        catalog.add(m.build()).unwrap();
        assert!(catalog.compile("duplicate-connect").err().unwrap().message.contains("duplicate connection"));
    }

    #[test]
    fn compiler_rejects_unknown_input_and_output_endpoints() {
        let mut a = FlatGraphBuilder::default();
        a.add_node(passthrough(1));
        a.connect(output(1,0),input(2,0,0));
        assert!(a.build().err().unwrap().contains("unknown input"));
        let mut b = FlatGraphBuilder::default();
        b.add_node(passthrough(2));
        b.connect(output(1,0),input(2,0,0));
        assert!(b.build().err().unwrap().contains("unknown output"));
    }
    #[test]
    fn compiler_rejects_mismatched_edge_schemas() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(passthrough(1));g.add_node(scalar(2));
        g.connect(output(1,0),input(2,0,0));
        assert!(g.build().err().unwrap().contains("schema mismatch"));
    }
    #[test]
    fn compiler_rejects_unknown_trigger_target() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(passthrough(1));
        g.trigger(1,vec![input(2,0,0)]);
        assert!(g.build().err().unwrap().contains("unknown input"));
    }



    #[test]
    fn module_expansion_matches_manual_flat_graph_without_wrapper_invocations() {
        let modular = compile_module(vec![two_step_module("process"),
            two_step_root("modular-root", Some("process"))], "modular-root").unwrap();
        let flat = compile_module(vec![two_step_root("flat-root", None)], "flat-root").unwrap();
        let modular_report = Deployment::new(modular.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("x".to_string())]).unwrap();
        let flat_report = Deployment::new(flat.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("x".to_string())]).unwrap();

        assert!(modular_report.success(), "{:?}", modular_report.errors);
        assert!(flat_report.success(), "{:?}", flat_report.errors);
        assert_eq!(module_outputs(&modular_report, &modular), module_outputs(&flat_report, &flat));
        assert_eq!(modular_report.trace.invocations.len(), flat_report.trace.invocations.len());
        assert_eq!(modular.nodes.len(), 2, "modules do not become runtime nodes");
        assert_eq!(modular_report.trace.executed.len(), 2);
        assert_eq!(modular.module_output(0), Some(output(2, 0)));
    }

    #[test]
    fn module_public_input_fans_out_atomically_to_all_mapped_slots() {
        let mut root = ModuleDefBuilder::new("fanout-root");
        root.node(passthrough(1));
        root.node(passthrough(2));
        root.input(0, "shared", S, vec![
            ModuleInputTarget::Node(input(1, 0, 0)),
            ModuleInputTarget::Node(input(2, 0, 0)),
        ]);
        root.output(0, "first", S, ModuleOutputSource::Node(output(1, 0)));
        root.output(1, "second", S, ModuleOutputSource::Node(output(2, 0)));
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let graph = compile_module(vec![root.build()], "fanout-root").unwrap();
        let report = Deployment::new(graph.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("shared".to_string())]).unwrap();
        assert!(report.success(), "{:?}", report.errors);
        assert_eq!(report.trace.invocations.len(), 2);
        assert_eq!(module_outputs(&report, &graph), vec![
            (0, "shared".to_string(), 0), (1, "shared".to_string(), 0),
        ]);
    }

    #[test]
    fn repeated_module_instances_have_independent_flat_nodes_and_paths() {
        let mut root = ModuleDefBuilder::new("twice-root");
        root.try_instance("left", "identity").unwrap();
        root.try_instance("right", "identity").unwrap();
        root.input(0, "input", S, vec![
            ModuleInputTarget::Instance { name: "left".into(), port: 0 },
            ModuleInputTarget::Instance { name: "right".into(), port: 0 },
        ]);
        root.output(0, "left", S, ModuleOutputSource::Instance { name: "left".into(), port: 0 });
        root.output(1, "right", S, ModuleOutputSource::Instance { name: "right".into(), port: 0 });
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let graph = compile_module(vec![passthrough_module("identity", 1), root.build()], "twice-root").unwrap();
        assert_eq!(graph.nodes.len(), 2);
        let origins: BTreeSet<_> = graph.metadata.nodes.values()
            .map(|origin| origin.instance_path.clone()).collect();
        assert_eq!(origins, [vec!["left".to_string()], vec!["right".to_string()]].into());
        assert_eq!(graph.metadata.instances.len(), 2);
        assert_ne!(graph.module_output(0), graph.module_output(1));

        let report = Deployment::new(graph.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("x".to_string())]).unwrap();
        assert!(report.success(), "{:?}", report.errors);
        assert_eq!(report.trace.invocations.len(), 2);
        assert_eq!(module_outputs(&report, &graph), vec![
            (0, "x".to_string(), 0), (1, "x".to_string(), 0),
        ]);
    }

    #[test]
    fn module_node_factories_create_instance_local_mutable_state() {
        let mut definition = ModuleDefBuilder::new("stateful");
        let factories = Arc::new(AtomicUsize::new(0));
        let create = Arc::clone(&factories);
        definition.node(node_with_factory(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], move || {
                create.fetch_add(1, AtomicOrdering::SeqCst);
                let calls = Arc::new(AtomicUsize::new(0));
                move |ctx| {
                    let call = calls.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                    Ok(vec![emit(0, format!("{call}:{}", ctx.arg::<String>(0)?))])
                }
            }));
        definition.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        definition.output(0, "output", S, ModuleOutputSource::Node(output(1, 0)));
        let mut root = ModuleDefBuilder::new("stateful-root");
        root.try_instance("left", "stateful").unwrap();
        root.try_instance("right", "stateful").unwrap();
        root.input(0, "input", S, vec![
            ModuleInputTarget::Instance { name: "left".into(), port: 0 },
            ModuleInputTarget::Instance { name: "right".into(), port: 0 },
        ]);
        root.output(0, "left", S, ModuleOutputSource::Instance { name: "left".into(), port: 0 });
        root.output(1, "right", S, ModuleOutputSource::Instance { name: "right".into(), port: 0 });
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let graph = compile_module(vec![definition.build(), root.build()], "stateful-root").unwrap();
        assert_eq!(factories.load(AtomicOrdering::SeqCst), 0,
            "compilation must not instantiate mutable node logic");
        let engine = Deployment::new(graph.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        assert_eq!(factories.load(AtomicOrdering::SeqCst), 2,
            "each engine owns one instance per physical node");
        let report = engine.fire(1, vec![enc("x".to_string())]).unwrap();
        assert!(report.success(), "{:?}", report.errors);
        assert_eq!(module_outputs(&report, &graph), vec![
            (0, "1:x".to_string(), 0), (1, "1:x".to_string(), 0),
        ]);
    }

    #[test]
    fn publication_observers_do_not_create_scheduler_invocations() {
        let graph = identity_graph();
        let report = graph.fire(1, vec![Value::of("one".to_owned())]).unwrap();
        let mut observer_a = Vec::new();
        let mut observer_b = Vec::new();
        for publication in report.publications() { observer_a.push(publication.value::<String>().unwrap()); }
        for publication in report.publications() { observer_b.push(publication.value::<String>().unwrap()); }
        assert_eq!(observer_a, vec!["one"]);
        assert_eq!(observer_b, observer_a);
        assert_eq!(report.invocation_count(1), 1);
    }

    #[test]
    fn factory_state_is_isolated_between_engines_sharing_a_compiled_graph() {
        let mut root = ModuleDefBuilder::new("counter-root");
        root.add(NodeDef::Node(NodeTemplate::with_factory(
            1,
            vec![Port::all(0, vec![InputSlot::new(0, S, BufferMode::Immediate)])],
            vec![OutputSlot::new(0, S)],
            || {
                let count = Arc::new(AtomicUsize::new(0));
                move |_ctx| {
                    let next = count.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                    Ok(vec![emit(0, next.to_string())])
                }
            },
        ))).unwrap();
        root.input(0, "in", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        root.output(0, "out", S, ModuleOutputSource::Node(output(1, 0)));
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let mut catalog = ModuleCatalog::new();
        catalog.add(root.build()).unwrap();
        let shared_graph = catalog.compile("counter-root").unwrap();
        let separate_graph = catalog.compile("counter-root").unwrap();
        let first_engine = Deployment::new(shared_graph.clone()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let second_engine = Deployment::new(shared_graph).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let separate_engine = Deployment::new(separate_graph).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap();

        let fire = |engine: &Engine| engine.fire(1, vec![Value::of("tick".to_owned())])
            .unwrap().publications().next().unwrap().value::<String>().unwrap();
        assert_eq!(fire(&first_engine), "1");
        assert_eq!(fire(&second_engine), "1", "engines sharing topology own separate node state");
        assert_eq!(fire(&first_engine), "2", "an engine preserves its own state across waves");
        assert_eq!(fire(&separate_engine), "1", "separately compiled graphs also isolate state");
    }

    #[test]
    fn nested_module_expansion_is_recursive_and_keeps_hierarchical_provenance() {
        let mut outer = ModuleDefBuilder::new("outer");
        outer.try_instance("step", "identity").unwrap();
        outer.input(0, "input", S, vec![ModuleInputTarget::Instance {
            name: "step".into(), port: 0,
        }]);
        outer.output(0, "output", S, ModuleOutputSource::Instance {
            name: "step".into(), port: 0,
        });
        let graph = compile_module(vec![passthrough_module("identity", 8), outer.build(),
            module_process_root("nested-root", "outer", true)], "nested-root").unwrap();
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.metadata.nodes[&NodeId(1)].instance_path,
            vec!["inner".to_string(), "step".to_string()]);
        assert_eq!(graph.metadata.nodes[&NodeId(1)].definition, "identity");
        assert_eq!(graph.metadata.instances.len(), 2);

        let report = Deployment::new(graph.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("nested".to_string())]).unwrap();
        assert!(report.success(), "{:?}", report.errors);
        assert_eq!(module_outputs(&report, &graph), vec![(0, "nested".to_string(), 0)]);
    }

    #[test]
    fn module_declaration_order_does_not_change_flat_identity() {
        let forward = two_step_module("process");
        let mut reverse = ModuleDefBuilder::new("reverse-process");
        reverse.node(named_transform(2, "second"));
        reverse.node(named_transform(1, "first"));
        reverse.connect(ModuleOutputSource::Node(output(1, 0)),
            ModuleInputTarget::Node(input(2, 0, 0)));
        reverse.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        reverse.output(0, "output", S, ModuleOutputSource::Node(output(2, 0)));
        let a = compile_module(vec![forward, two_step_root("root-a", Some("process"))], "root-a").unwrap();
        let b = compile_module(vec![reverse.build(), two_step_root("root-b", Some("reverse-process"))], "root-b").unwrap();
        assert_eq!(a.nodes.keys().collect::<Vec<_>>(), b.nodes.keys().collect::<Vec<_>>());
        assert_eq!(a.by_output.keys().collect::<Vec<_>>(), b.by_output.keys().collect::<Vec<_>>());
        for id in [NodeId(1), NodeId(2)] {
            assert_eq!(a.nodes[&id].outputs[0].schema, b.nodes[&id].outputs[0].schema);
            assert_eq!(a.metadata.nodes[&id].local_node, b.metadata.nodes[&id].local_node);
        }

        let mut forward_instances = ModuleDefBuilder::new("forward-instances");
        forward_instances.try_instance("left", "identity").unwrap();
        forward_instances.try_instance("right", "identity").unwrap();
        let mut reverse_instances = ModuleDefBuilder::new("reverse-instances");
        reverse_instances.try_instance("right", "identity").unwrap();
        reverse_instances.try_instance("left", "identity").unwrap();
        let forward = compile_module(vec![passthrough_module("identity", 1), forward_instances.build()],
            "forward-instances").unwrap();
        let reverse = compile_module(vec![passthrough_module("identity", 1), reverse_instances.build()],
            "reverse-instances").unwrap();
        assert_eq!(forward.metadata.nodes, reverse.metadata.nodes);
    }

    #[test]
    fn connection_provenance_survives_declaration_reordering() {
        let mut original = ModuleDefBuilder::new("stable-edges");
        original.node(passthrough(1));
        original.node(passthrough(2));
        original.node(passthrough(3));
        original.connect(output(1, 0), input(2, 0, 0));
        original.connect(output(2, 0), input(3, 0, 0));
        let mut entry = original.build();
        let before = entry.definition.edges.iter().map(|edge| edge.declaration_id).collect::<Vec<_>>();
        assert_eq!(before, vec![1, 2]);
        entry.definition.edges.reverse();
        let mut catalog = ModuleCatalog::new();
        catalog.add(entry).unwrap();
        let graph = catalog.compile("stable-edges").unwrap();
        let ids: BTreeSet<_> = graph.metadata.edges.values().map(|origin| origin.local_edge).collect();
        assert_eq!(ids, BTreeSet::from([1, 2]));
        let first = graph.by_output[&output(1, 0)][0].id;
        assert_eq!(graph.edge_provenance(first).unwrap().local_edge, 1);
    }

    #[test]
    fn declaration_edge_provenance_keeps_full_u32_identity() {
        let mut module = ModuleDefBuilder::new("long-lived-edge");
        module.node(passthrough(1));
        module.node(passthrough(2));
        module.connect(output(1, 0), input(2, 0, 0));
        let mut entry = module.build();
        entry.definition.edges[0].declaration_id = u32::MAX;
        let mut catalog = ModuleCatalog::new();
        catalog.add(entry).unwrap();

        let graph = catalog.compile("long-lived-edge").unwrap();
        let runtime_edge = graph.by_output[&output(1, 0)][0].id;
        assert_eq!(graph.edge_provenance(runtime_edge).unwrap().local_edge, u32::MAX);
    }

    #[test]
    fn module_metadata_retains_definitions_instance_edges_and_port_mappings() {
        let graph = compile_module(vec![two_step_module("process"),
            two_step_root("metadata-root", Some("process"))], "metadata-root").unwrap();
        assert!(graph.metadata.definitions.contains_key("process"));
        assert_eq!(graph.metadata.definitions["process"].inputs[0].name, "input");
        assert_eq!(graph.metadata.definitions["process"].outputs[0].name, "output");
        assert_eq!(graph.metadata.nodes[&NodeId(1)], NodeProvenance {
            definition: "process".into(), instance_path: vec!["pipeline".into()],
            local_node: NodeId(1),
        });
        let edge_id = *graph.metadata.edges.keys().next().unwrap();
        assert_eq!(graph.edge_provenance(edge_id).unwrap().definition, "process");
        assert_eq!(graph.edge_provenance(edge_id).unwrap().local_edge, 1);
        let expanded_edge = graph.by_output.values().flatten().find(|edge| edge.id == edge_id).unwrap();
        assert_eq!(expanded_edge.from, output(1, 0));
        assert_eq!(expanded_edge.to, input(2, 0, 0));
        assert_eq!(graph.module_output(0), Some(output(2, 0)));
    }

    #[test]
    fn module_instances_can_select_distinct_compatible_definitions() {
        let mut plain = ModuleDefBuilder::new("plain-definition");
        plain.node(passthrough(1));
        plain.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        plain.output(0, "output", S, ModuleOutputSource::Node(output(1, 0)));

        let mut decorated = ModuleDefBuilder::new("decorated-definition");
        decorated.node(passthrough(8));
        decorated.add_named("decorated-node", node(1,
            vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, format!("decorated({})", ctx.arg::<String>(0)?))])
            })).unwrap();
        decorated.try_instance("observer", "observer").unwrap();
        decorated.connect(ModuleOutputSource::Node(output(1, 0)),
            ModuleInputTarget::Instance { name: "observer".into(), port: 0 });
        decorated.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        decorated.output(0, "output", S, ModuleOutputSource::Node(output(1, 0)));

        let mut root = ModuleDefBuilder::new("composition-root");
        root.try_instance("decorated", "decorated-definition").unwrap();
        root.try_instance("plain", "plain-definition").unwrap();
        root.input(0, "input", S, vec![
            ModuleInputTarget::Instance { name: "decorated".into(), port: 0 },
            ModuleInputTarget::Instance { name: "plain".into(), port: 0 },
        ]);
        root.output(0, "decorated", S, ModuleOutputSource::Instance {
            name: "decorated".into(), port: 0,
        });
        root.output(1, "plain", S, ModuleOutputSource::Instance {
            name: "plain".into(), port: 0,
        });
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let graph = compile_module(vec![plain.build(), decorated.build(),
            passthrough_module("observer", 7), root.build()], "composition-root").unwrap();
        let decorated_instance = graph.metadata.instances.iter()
            .find(|instance| instance.instance_path == ["decorated".to_string()]).unwrap();
        assert_eq!(decorated_instance.definition, "decorated-definition");
        assert_eq!(graph.nodes.len(), 4);
        assert!(matches!(graph.find("decorated/decorated-node"), Some(FindResult::Node(_))));
        assert!(matches!(graph.find("decorated/observer"), Some(FindResult::Module(_))));
        assert!(graph.find("decorated/8").is_none());
        let report = Deployment::new(graph.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("x".to_string())]).unwrap();
        assert!(report.success(), "{:?}", report.errors);
        assert_eq!(module_outputs(&report, &graph), vec![
            (0, "decorated(x)".to_string(), 0), (1, "x".to_string(), 0),
        ]);
        assert_eq!(report.trace.executed.len(), 3);
    }

    #[test]
    fn named_lookup_supports_absolute_relative_and_standard_pattern_matching() {
        use FindResult::{Module, Node};

        let graph = named_lookup_graph();
        let before = Deployment::new(graph.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("before".to_string())]).unwrap();
        if let Some(Node(source)) = graph.find("source") {
            assert_eq!(source.name(), Some("source"));
            assert!(source.instance_path().is_empty());
            assert_eq!(source.id(), 1);
            assert_eq!(source.input_port_count(), 1);
            assert_eq!(source.output_count(), 1);
            assert!(!source.creates_fresh_cohort());
        } else {
            panic!("named root node must be found as Node");
        }

        let Some(Module(process)) = graph.find("process") else {
            panic!("module lookup must return a Module variant");
        };
        assert_eq!(process.name(), "process");
        assert_eq!(process.qualified_name(), "process");
        assert_eq!(process.definition_id(), "process-definition");
        assert_eq!(process.expanded_node_count(), 2);
        let inputs: Vec<_> = process.input_ports().map(|port| (port.id(), port.name().to_owned(), port.schema())).collect();
        let outputs: Vec<_> = process.output_ports().map(|port| (port.id(), port.name().to_owned(), port.schema())).collect();
        assert_eq!(inputs, vec![(0, "input".to_string(), S)]);
        assert_eq!(outputs, vec![(0, "output".to_string(), S)]);

        let Some(Node(absolute_validate)) = graph.find("process/validate") else {
            panic!("qualified node must resolve");
        };
        let Some(Node(relative_validate)) = process.find("validate") else {
            panic!("module-relative node must resolve");
        };
        assert_eq!(absolute_validate.id(), relative_validate.id());
        assert_eq!(absolute_validate.definition_id(), Some("process-definition"));
        assert_eq!(absolute_validate.instance_path(), &["process".to_string()]);
        assert_eq!(process.resolved_input_targets(0), Some(vec![ResolvedInputEndpoint {
            node: absolute_validate.id(), port: 0, slot: 0,
        }]));
        assert_eq!(process.resolved_output_source(0), Some(ResolvedOutputEndpoint {
            node: absolute_validate.id(), slot: 0,
        }));

        let Some(Module(inner)) = process.find("inner") else {
            panic!("nested module lookup must return Module directly");
        };
        let Some(Node(execute)) = inner.find("deep/execute") else {
            panic!("relative lookup must accept arbitrarily nested suffixes");
        };
        assert_eq!(execute.name(), Some("execute"));
        assert_eq!(execute.input_port_count(), 1);
        assert_eq!(execute.output_count(), 1);
        assert_eq!(execute.instance_path(), &["process".to_string(), "inner".to_string(), "deep".to_string()]);
        assert_eq!(inner.resolved_input_targets(0), Some(vec![ResolvedInputEndpoint {
            node: execute.id(), port: 0, slot: 0,
        }]));
        assert_eq!(inner.resolved_output_source(0), Some(ResolvedOutputEndpoint {
            node: execute.id(), slot: 0,
        }));
        assert!(matches!(graph.find("process/inner/deep/execute"), Some(Node(_))));

        assert!(matches!(graph.find("process"), Some(Module(_))));
        assert!(!matches!(graph.find("process"), Some(Node(_))));
        match graph.find("process") {
            Some(Node(_)) => panic!("process is a module, not an executable wrapper node"),
            Some(Module(module)) => assert_eq!(module.name(), "process"),
            None => panic!("known module must be found"),
        }
        assert!(graph.find("missing").is_none());
        assert!(graph.find("process/missing").is_none());
        assert!(graph.find("process//validate").is_none());
        assert!(graph.find("process/../source").is_none());
        let after = Deployment::new(graph.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("after".to_string())]).unwrap();
        assert!(before.success() && after.success());
        assert_eq!(before.trace.invocations, after.trace.invocations);
        assert_eq!(before.published[0].from, after.published[0].from);
        assert_eq!(before.published[0].value.get::<String>().unwrap(), "before");
        assert_eq!(after.published[0].value.get::<String>().unwrap(), "after");
    }

    #[test]
    fn named_lookup_reuses_definition_names_but_keeps_instance_paths_distinct() {
        let graph = named_lookup_graph();
        let Some(FindResult::Node(process_validate)) = graph.find("process/validate") else { panic!() };
        let Some(FindResult::Node(backup_validate)) = graph.find("backup/validate") else { panic!() };
        assert_ne!(process_validate.id(), backup_validate.id());
        assert_eq!(process_validate.name(), backup_validate.name());
        assert_ne!(process_validate.instance_path(), backup_validate.instance_path());
        assert!(graph.find("90").is_none(), "anonymous nodes do not get lookup entries");
        assert!(graph.find("process/90").is_none());
    }

    #[test]
    fn duplicate_and_invalid_name_registration_is_structured_and_atomic() {
        let mut root = ModuleDefBuilder::new("name-errors");
        root.add_named("same", passthrough(1)).unwrap();
        assert_eq!(root.add_named("same", passthrough(2)),
            Err(ModuleNameError::DuplicateName("same".to_string())));
        root.try_instance("child", "leaf").unwrap();
        assert_eq!(root.try_instance("child", "leaf"),
            Err(ModuleNameError::DuplicateName("child".to_string())));
        assert_eq!(root.try_instance("same", "leaf"),
            Err(ModuleNameError::DuplicateName("same".to_string())));
        assert_eq!(root.add_named("bad/path", passthrough(3)),
            Err(ModuleNameError::InvalidSegment("bad/path".to_string())));
        assert_eq!(root.try_instance("..", "leaf"),
            Err(ModuleNameError::InvalidSegment("..".to_string())));
        assert_eq!(root.nodes.len(), 1);
        assert_eq!(root.instances.len(), 1, "failed names do not register extra instances");

        let graph = compile_module(vec![passthrough_module("leaf", 1), root.build()], "name-errors").unwrap();
        assert!(matches!(graph.find("same"), Some(FindResult::Node(_))));
        assert!(matches!(graph.find("child"), Some(FindResult::Module(_))));
        assert!(graph.find("bad/path").is_none());

        let mut flat = FlatGraphBuilder::default();
        flat.add_named("flat-source", passthrough(1)).unwrap();
        assert_eq!(flat.add_named("flat-source", passthrough(2)),
            Err(ModuleNameError::DuplicateName("flat-source".to_string())));
        flat.add_named("fresh", node(2, vec![], vec![], CohortMode::FreshPerInvocation, vec![],
            |_ctx| Ok(vec![]))).unwrap();
        let flat = flat.build().unwrap();
        assert!(matches!(flat.find("flat-source"), Some(FindResult::Node(_))));
        let Some(FindResult::Node(fresh)) = flat.find("fresh") else { panic!() };
        assert!(fresh.creates_fresh_cohort());
    }

    #[test]
    fn parent_nodes_connect_through_module_ports_like_ordinary_edges() {
        let modular = compile_module(vec![two_step_module("process"),
            connected_module_root("connected-root", "process")], "connected-root").unwrap();
        let mut flat = ModuleDefBuilder::new("connected-flat");
        flat.node(named_transform(1, "before"));
        flat.node(named_transform(2, "first"));
        flat.node(named_transform(3, "second"));
        flat.node(named_transform(4, "after"));
        flat.connect(ModuleOutputSource::Node(output(1, 0)), ModuleInputTarget::Node(input(2, 0, 0)));
        flat.connect(ModuleOutputSource::Node(output(2, 0)), ModuleInputTarget::Node(input(3, 0, 0)));
        flat.connect(ModuleOutputSource::Node(output(3, 0)), ModuleInputTarget::Node(input(4, 0, 0)));
        flat.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        flat.output(0, "output", S, ModuleOutputSource::Node(output(4, 0)));
        flat.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let flat = compile_module(vec![flat.build()], "connected-flat").unwrap();

        let modular_report = Deployment::new(modular.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("x".to_string())]).unwrap();
        let flat_report = Deployment::new(flat.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("x".to_string())]).unwrap();
        assert!(modular_report.success(), "{:?}", modular_report.errors);
        assert!(flat_report.success(), "{:?}", flat_report.errors);
        assert_eq!(module_outputs(&modular_report, &modular), module_outputs(&flat_report, &flat));
        assert_eq!(modular_report.trace.executed.len(), 4);
        assert_eq!(flat_report.trace.executed.len(), 4);
    }

    #[test]
    fn unified_node_def_insertion_of_node_and_module_keeps_flat_execution() {
        let mut modular = ModuleDefBuilder::new("unified-root");
        let mut source = named_transform(1, "source");
        source.name = Some("source".into());
        modular.add(NodeDef::Node(source)).unwrap();
        modular.add(NodeDef::Module(ModuleInstance {
            name: "process".into(), definition: "identity".into(),
        })).unwrap();
        modular.add(NodeDef::Node(named_transform(3, "sink"))).unwrap();
        modular.connect(ModuleOutputSource::Node(output(1, 0)),
            ModuleInputTarget::Instance { name: "process".into(), port: 0 });
        modular.connect(ModuleOutputSource::Instance { name: "process".into(), port: 0 },
            ModuleInputTarget::Node(input(3, 0, 0)));
        modular.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        modular.output(0, "output", S, ModuleOutputSource::Node(output(3, 0)));
        modular.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);

        let mut flat = ModuleDefBuilder::new("unified-flat");
        flat.node(named_transform(1, "source"));
        flat.node(passthrough(2));
        flat.node(named_transform(3, "sink"));
        flat.connect(ModuleOutputSource::Node(output(1, 0)), ModuleInputTarget::Node(input(2, 0, 0)));
        flat.connect(ModuleOutputSource::Node(output(2, 0)), ModuleInputTarget::Node(input(3, 0, 0)));
        flat.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        flat.output(0, "output", S, ModuleOutputSource::Node(output(3, 0)));
        flat.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);

        let modular = compile_module(vec![passthrough_module("identity", 7), modular.build()], "unified-root").unwrap();
        let flat = compile_module(vec![flat.build()], "unified-flat").unwrap();
        assert!(matches!(modular.find("process"), Some(FindResult::Module(_))));
        let a = Deployment::new(modular.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("x".to_string())]).unwrap();
        let b = Deployment::new(flat.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("x".to_string())]).unwrap();
        assert!(a.success() && b.success());
        assert_eq!(module_outputs(&a, &modular), module_outputs(&b, &flat));
        assert_eq!(a.trace.executed.len(), 3, "module insertion adds no wrapper invocation");
    }

    #[test]
    fn module_policy_reservations_and_cohorts_match_flat_execution_across_waves() {
        let modular = compile_module(vec![fork_join_body("fork", Some("omit"), Some("fail")),
            root_for_module("fork-root", "fork")], "fork-root").unwrap();
        let flat = compile_module(vec![flat_fork_join_root("fork-flat", Some("omit"), Some("fail"))],
            "fork-flat").unwrap();
        let modular_engine = Deployment::new(modular.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let flat_engine = Deployment::new(flat.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();

        for value in ["success", "omit", "fail", "success"] {
            let a = modular_engine.fire(1, vec![enc(value.to_string())]).unwrap();
            let b = flat_engine.fire(1, vec![enc(value.to_string())]).unwrap();
            assert_eq!(a.success(), b.success(), "value={value}");
            assert_eq!(a.errors, b.errors, "value={value}");
            assert_eq!(a.trace.executed, b.trace.executed, "value={value}");
            assert_eq!(a.wave, b.wave);
            assert_eq!(module_outputs(&a, &modular), module_outputs(&b, &flat), "value={value}");
        }

        let success = modular_engine.fire(1, vec![enc("last".to_string())]).unwrap();
        assert!(success.success());
        assert_eq!(success.wave, WaveId(5));
        assert_eq!(module_outputs(&success, &modular), vec![
            (0, "join(left(last),right(last))".to_string(), 0),
        ]);
    }

    #[test]
    fn internal_failure_keeps_independent_module_output_path_live() {
        let mut module = ModuleDefBuilder::new("independent-branches");
        module.node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S), out(1, S)], CohortMode::Inherit, vec![], |ctx| {
                let value = ctx.arg::<String>(0)?;
                Ok(vec![emit(0, format!("good({value})")), emit(1, value)])
            }));
        module.node(named_transform(2, "published"));
        module.node(node(3, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], |_ctx| Err("independent failure".into())));
        module.connect(ModuleOutputSource::Node(output(1, 0)), ModuleInputTarget::Node(input(2, 0, 0)));
        module.connect(ModuleOutputSource::Node(output(1, 1)), ModuleInputTarget::Node(input(3, 0, 0)));
        module.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        module.output(0, "good", S, ModuleOutputSource::Node(output(2, 0)));
        let mut root = ModuleDefBuilder::new("independent-root");
        root.try_instance("branches", "independent-branches").unwrap();
        root.input(0, "input", S, vec![ModuleInputTarget::Instance { name: "branches".into(), port: 0 }]);
        root.output(0, "good", S, ModuleOutputSource::Instance { name: "branches".into(), port: 0 });
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let graph = compile_module(vec![module.build(), root.build()], "independent-root").unwrap();

        let report = Deployment::new(graph.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("x".to_string())]).unwrap();
        assert_eq!(report.errors.len(), 1);
        assert!(matches!(&report.errors[0], WaveError::NodeFailed(node, reason)
            if graph.node_provenance(*node).unwrap().instance_path == ["branches".to_string()]
                && reason == "independent failure"));
        assert_eq!(module_outputs(&report, &graph), vec![(0, "published(good(x))".to_string(), 0)]);
    }

    #[test]
    fn module_instance_failures_are_isolated_and_resolve_to_their_paths() {
        let mut definition = ModuleDefBuilder::new("conditional");
        definition.node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                let value = ctx.arg::<String>(0)?;
                if value == "fail" { Err("selected instance failed".into()) }
                else { Ok(vec![emit(0, value)]) }
            }));
        definition.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        definition.output(0, "output", S, ModuleOutputSource::Node(output(1, 0)));
        let mut wrapper = ModuleDefBuilder::new("wrapper");
        wrapper.try_instance("inner", "conditional").unwrap();
        wrapper.input(0, "input", S, vec![ModuleInputTarget::Instance { name: "inner".into(), port: 0 }]);
        wrapper.output(0, "output", S, ModuleOutputSource::Instance { name: "inner".into(), port: 0 });
        let mut root = ModuleDefBuilder::new("isolated-instances");
        root.try_instance("failing", "wrapper").unwrap();
        root.try_instance("passing", "wrapper").unwrap();
        root.input(0, "bad", S, vec![ModuleInputTarget::Instance { name: "failing".into(), port: 0 }]);
        root.input(1, "good", S, vec![ModuleInputTarget::Instance { name: "passing".into(), port: 0 }]);
        root.output(0, "bad-output", S, ModuleOutputSource::Instance { name: "failing".into(), port: 0 });
        root.output(1, "good-output", S, ModuleOutputSource::Instance { name: "passing".into(), port: 0 });
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0), ModuleTriggerTarget::PublicInput(1)]);
        let graph = compile_module(vec![definition.build(), wrapper.build(), root.build()], "isolated-instances").unwrap();

        let report = Deployment::new(graph.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("fail".to_string()), enc("ok".to_string())]).unwrap();
        assert_eq!(report.errors.len(), 1);
        assert!(matches!(&report.errors[0], WaveError::NodeFailed(node, reason)
            if graph.node_provenance(*node).unwrap().instance_path == ["failing".to_string(), "inner".to_string()]
                && reason == "selected instance failed"));
        assert_eq!(module_outputs(&report, &graph), vec![(1, "ok".to_string(), 0)]);
        assert_eq!(report.trace.executed.len(), 2);
    }

    #[test]
    fn module_edge_fanout_and_internal_output_validation_remain_atomic() {
        for invalid_schema in [false, true] {
            let mut module = ModuleDefBuilder::new(if invalid_schema { "bad-output" } else { "fanout" });
            module.node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
                vec![out(0, S), out(1, S)], CohortMode::Inherit, vec![], move |_ctx| {
                    if invalid_schema { Ok(vec![emit(0, "valid".to_string()), emit(1, 42_i64)]) }
                    else { Ok(vec![emit(0, "first".to_string()), emit(1, "second".to_string())]) }
                }));
            module.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
            module.output(0, "first", S, ModuleOutputSource::Node(output(1, 0)));
            module.output(1, "second", S, ModuleOutputSource::Node(output(1, 1)));
            let definition_id = if invalid_schema { "bad-output" } else { "fanout" };
            let mut root = ModuleDefBuilder::new("atomic-root");
            root.node(passthrough(2));
            root.try_instance("work", definition_id).unwrap();
            root.connect(ModuleOutputSource::Instance { name: "work".into(), port: 0 },
                ModuleInputTarget::Node(input(2, 0, 0)));
            root.connect(ModuleOutputSource::Instance { name: "work".into(), port: 1 },
                ModuleInputTarget::Node(input(2, 0, 0)));
            root.input(0, "input", S, vec![ModuleInputTarget::Instance { name: "work".into(), port: 0 }]);
            root.output(0, "output", S, ModuleOutputSource::Node(output(2, 0)));
            root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
            let graph = compile_module(vec![module.build(), root.build()], "atomic-root").unwrap();
            let report = Deployment::new(graph).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
                .fire(1, vec![enc("input".to_string())]).unwrap();
            assert_eq!(report.count(1), 0);
            assert!(report.published.is_empty());
            if invalid_schema {
                assert!(matches!(report.errors.as_slice(), [WaveError::InvalidOutput(_, _)]));
            } else {
                assert_eq!(report.errors, vec![WaveError::SlotOccupied(input(1, 0, 0))]);
            }
        }
    }

    #[test]
    fn module_public_output_reaches_parent_consumer_before_other_internal_branch_finishes() {
        let mut module = ModuleDefBuilder::new("early-output");
        module.node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S), out(1, S)], CohortMode::Inherit, vec![], |_ctx| {
                Ok(vec![emit(0, "public".to_string()), emit(1, "slow".to_string())])
            }));
        module.node(named_transform(2, "source"));
        module.node(named_transform(3, "slow-first"));
        module.node(named_transform(4, "slow-last"));
        module.connect(ModuleOutputSource::Node(output(1, 0)), ModuleInputTarget::Node(input(2, 0, 0)));
        module.connect(ModuleOutputSource::Node(output(1, 1)), ModuleInputTarget::Node(input(3, 0, 0)));
        module.connect(ModuleOutputSource::Node(output(3, 0)), ModuleInputTarget::Node(input(4, 0, 0)));
        module.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        module.output(0, "output", S, ModuleOutputSource::Node(output(2, 0)));
        let mut root = ModuleDefBuilder::new("early-root");
        root.node(named_transform(9, "consumer"));
        root.node(named_transform(10, "consumer-two"));
        root.try_instance("work", "early-output").unwrap();
        root.connect(ModuleOutputSource::Instance { name: "work".into(), port: 0 },
            ModuleInputTarget::Node(input(9, 0, 0)));
        root.connect(ModuleOutputSource::Instance { name: "work".into(), port: 0 },
            ModuleInputTarget::Node(input(10, 0, 0)));
        root.input(0, "input", S, vec![ModuleInputTarget::Instance { name: "work".into(), port: 0 }]);
        root.output(0, "output", S, ModuleOutputSource::Node(output(9, 0)));
        root.output(1, "output-two", S, ModuleOutputSource::Node(output(10, 0)));
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let graph = compile_module(vec![module.build(), root.build()], "early-root").unwrap();
        let report = Deployment::new(graph.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("start".to_string())]).unwrap();
        assert!(report.success(), "{:?}", report.errors);
        assert_eq!(module_outputs(&report, &graph), vec![
            (0, "consumer(source(public))".to_string(), 0),
            (1, "consumer-two(source(public))".to_string(), 0),
        ]);
        let consumer = report.trace.executed.iter().position(|id|
            graph.node_provenance(*id).unwrap().local_node == NodeId(9)).unwrap();
        let slow_last = report.trace.executed.iter().position(|id|
            graph.node_provenance(*id).unwrap().local_node == NodeId(4)).unwrap();
        assert!(consumer < slow_last, "module output consumers do not wait for unrelated internal work");
    }

    #[test]
    fn module_keeps_custom_colored_optional_pair_policy() {
        let mut module = ModuleDefBuilder::new("optional-pair");
        module.node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S), out(1, S)], CohortMode::Inherit, vec![], |ctx| {
                let value = ctx.arg::<String>(0)?;
                Ok(vec![emit(0, format!("first({value})")), emit(1, value)])
            }));
        module.node(optional_pair_node(2));
        module.node(node(3, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                let value = ctx.arg::<String>(0)?;
                if value == "skip" { Ok(vec![]) }
                else { Ok(vec![emit(0, format!("second({value})") )]) }
            }));
        module.connect(ModuleOutputSource::Node(output(1, 0)),
            ModuleInputTarget::Node(input(2, 0, 0)));
        module.connect(ModuleOutputSource::Node(output(1, 1)),
            ModuleInputTarget::Node(input(3, 0, 0)));
        module.connect(ModuleOutputSource::Node(output(3, 0)),
            ModuleInputTarget::Node(input(2, 0, 1)));
        module.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        module.output(0, "output", S, ModuleOutputSource::Node(output(2, 0)));
        let graph = compile_module(vec![module.build(), root_for_module("optional-root", "optional-pair")],
            "optional-root").unwrap();
        let engine = Deployment::new(graph.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let both = engine.fire(1, vec![enc("both".to_string())]).unwrap();
        assert!(both.success(), "{:?}", both.errors);
        assert_eq!(module_outputs(&both, &graph), vec![(0, "first(both)|second(both)".to_string(), 0)]);
        assert!(both.trace.waits.iter().any(|wait| wait.node == NodeId(2) && wait.upstream.contains(&NodeId(3))));

        let one = engine.fire(1, vec![enc("skip".to_string())]).unwrap();
        assert!(one.success(), "{:?}", one.errors);
        assert_eq!(module_outputs(&one, &graph), vec![(0, "first(skip)|_".to_string(), 0)]);
        assert!(one.trace.waits.iter().any(|wait| wait.node == NodeId(2) && wait.upstream.contains(&NodeId(3))));
    }

    #[test]
    fn any_policy_selection_is_unchanged_inside_a_module() {
        let mut module = ModuleDefBuilder::new("any-input");
        module.node(node(1, vec![any(0, vec![slot(0, S, BufferMode::Deferred),
            slot(1, S, BufferMode::Deferred)])], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                let value = if ctx.has_arg(0) { ctx.arg::<String>(0)? } else { ctx.arg::<String>(1)? };
                Ok(vec![emit(0, format!("selected({value})"))])
            }));
        module.input(0, "first", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        module.input(1, "second", S, vec![ModuleInputTarget::Node(input(1, 0, 1))]);
        module.output(0, "output", S, ModuleOutputSource::Node(output(1, 0)));
        let mut root = ModuleDefBuilder::new("any-root");
        root.try_instance("work", "any-input").unwrap();
        root.input(0, "first", S, vec![ModuleInputTarget::Instance { name: "work".into(), port: 0 }]);
        root.input(1, "second", S, vec![ModuleInputTarget::Instance { name: "work".into(), port: 1 }]);
        root.output(0, "output", S, ModuleOutputSource::Instance { name: "work".into(), port: 0 });
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        root.trigger(2, vec![ModuleTriggerTarget::PublicInput(1)]);
        root.trigger(3, vec![ModuleTriggerTarget::PublicInput(0), ModuleTriggerTarget::PublicInput(1)]);
        let graph = compile_module(vec![module.build(), root.build()], "any-root").unwrap();
        let engine = Deployment::new(graph.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        for (trigger, values, expected) in [
            (1, vec!["left"], vec!["selected(left)"]),
            (2, vec!["right"], vec!["selected(right)"]),
            (3, vec!["left", "right"], vec!["selected(left)", "selected(right)"]),
        ] {
            let report = engine.fire(trigger,
                values.into_iter().map(|value| enc(value.to_string())).collect()).unwrap();
            assert!(report.success(), "{:?}", report.errors);
            assert_eq!(report.count(1), expected.len());
            let expected: Vec<_> = expected.into_iter().map(|value| (0, value.to_string(), 0)).collect();
            assert_eq!(module_outputs(&report, &graph), expected);
        }
    }

    #[test]
    fn staged_policy_keeps_phase_reservations_inside_expanded_module() {
        let mut module = ModuleDefBuilder::new("staged");
        module.node(node(1, vec![custom_policy(0, vec![
                slot(0, S, BufferMode::Deferred),
                slot(1, S, BufferMode::Deferred),
                slot(2, S, BufferMode::Deferred),
            ], PairThenThird).args(decode_pair_stage)], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                match ctx.prepared::<PairStageArgs>()? {
                    PairStageArgs::Pair { left, right } => Ok(vec![emit(0, format!("pair({left},{right})"))]),
                    PairStageArgs::Third { .. } => Ok(vec![]),
                }
            }));
        for slot in 0..3 {
            module.input(slot, ["left", "right", "third"][slot as usize], S,
                vec![ModuleInputTarget::Node(input(1, 0, slot))]);
        }
        module.output(0, "output", S, ModuleOutputSource::Node(output(1, 0)));
        let mut root = ModuleDefBuilder::new("staged-root");
        root.try_instance("work", "staged").unwrap();
        for port in 0..3 {
            root.input(port, ["left", "right", "third"][port as usize], S,
                vec![ModuleInputTarget::Instance { name: "work".into(), port }]);
        }
        root.output(0, "output", S, ModuleOutputSource::Instance { name: "work".into(), port: 0 });
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0), ModuleTriggerTarget::PublicInput(1)]);
        root.trigger(2, vec![ModuleTriggerTarget::PublicInput(0), ModuleTriggerTarget::PublicInput(1),
            ModuleTriggerTarget::PublicInput(2)]);
        let graph = compile_module(vec![module.build(), root.build()], "staged-root").unwrap();
        let engine = Deployment::new(graph.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();

        let incomplete = engine.fire(1, vec![enc("a".to_string()), enc("b".to_string())]).unwrap();
        assert_eq!(incomplete.count(1), 1);
        assert_eq!(incomplete.errors.len(), 1);
        assert_eq!(module_outputs(&incomplete, &graph), vec![(0, "pair(a,b)".to_string(), 0)]);

        let complete = engine.fire(2, vec![
            enc("a".to_string()), enc("b".to_string()), enc("third".to_string()),
        ]).unwrap();
        assert!(complete.success(), "{:?}", complete.errors);
        assert_eq!(complete.count(1), 2);
        assert_eq!(module_outputs(&complete, &graph), vec![(0, "pair(a,b)".to_string(), 0)]);
    }

    #[test]
    fn fresh_cohorts_and_multiple_invocations_survive_module_expansion() {
        let mut module = ModuleDefBuilder::new("fresh-pair");
        module.node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Deferred)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, ctx.arg::<String>(0)?)])
            }));
        module.node(node(2, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S), out(1, S)], CohortMode::FreshPerInvocation, vec![], |ctx| {
                let value = ctx.arg::<String>(0)?;
                Ok(vec![emit(0, format!("left({value})")), emit(1, format!("right({value})"))])
            }));
        module.node(named_transform(3, "branch-a"));
        module.node(named_transform(4, "branch-b"));
        module.node(node(5, vec![all(0, vec![slot(0, S, BufferMode::Deferred),
            slot(1, S, BufferMode::Deferred)])], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, format!("join({},{})",
                    ctx.arg::<String>(0)?, ctx.arg::<String>(1)?))])
            }));
        module.connect(ModuleOutputSource::Node(output(1, 0)), ModuleInputTarget::Node(input(2, 0, 0)));
        module.connect(ModuleOutputSource::Node(output(2, 0)), ModuleInputTarget::Node(input(3, 0, 0)));
        module.connect(ModuleOutputSource::Node(output(2, 1)), ModuleInputTarget::Node(input(4, 0, 0)));
        module.connect(ModuleOutputSource::Node(output(3, 0)), ModuleInputTarget::Node(input(5, 0, 0)));
        module.connect(ModuleOutputSource::Node(output(4, 0)), ModuleInputTarget::Node(input(5, 0, 1)));
        module.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        module.output(0, "output", S, ModuleOutputSource::Node(output(5, 0)));
        let mut root = ModuleDefBuilder::new("fresh-root");
        root.try_instance("work", "fresh-pair").unwrap();
        root.input(0, "input", S, vec![ModuleInputTarget::Instance { name: "work".into(), port: 0 }]);
        root.output(0, "output", S, ModuleOutputSource::Instance { name: "work".into(), port: 0 });
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0), ModuleTriggerTarget::PublicInput(0)]);
        let graph = compile_module(vec![module.build(), root.build()], "fresh-root").unwrap();
        let report = Deployment::new(graph.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap().fire(1, vec![
            enc("a".to_string()), enc("b".to_string()),
        ]).unwrap();
        assert!(report.success(), "{:?}", report.errors);
        assert_eq!(report.count(1), 2);
        assert_eq!(report.count(2), 2);
        assert_eq!(report.count(5), 2);
        let outputs = module_outputs(&report, &graph);
        assert_eq!(outputs.len(), 2);
        assert!(outputs.iter().any(|(_, value, cohort)| value == "join(branch-a(left(a)),branch-b(right(a)))" && *cohort > 0));
        assert!(outputs.iter().any(|(_, value, cohort)| value == "join(branch-a(left(b)),branch-b(right(b)))" && *cohort > 0));
        assert_ne!(outputs[0].2, outputs[1].2);
    }

    #[test]
    fn module_data_cycles_keep_existing_feedback_semantics() {
        let mut module = ModuleDefBuilder::new("feedback-module");
        module.node(node(1, vec![all(0, vec![slot(0, I, BufferMode::Immediate)])],
            vec![out(0, I)], CohortMode::Inherit, vec![], |ctx| {
                let value = ctx.arg::<i64>(0)?;
                Ok(if value < 3 { vec![emit(0, value + 1)] } else { vec![] })
            }));
        module.connect(ModuleOutputSource::Node(output(1, 0)),
            ModuleInputTarget::Node(input(1, 0, 0)));
        module.input(0, "input", I, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        module.output(0, "output", I, ModuleOutputSource::Node(output(1, 0)));
        module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let graph = compile_module(vec![module.build()], "feedback-module").unwrap();
        let report = Deployment::new(graph.clone()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc(0_i64)]).unwrap();
        assert!(report.success(), "{:?}", report.errors);
        assert_eq!(report.count(1), 4);
        let values: Vec<_> = report.published.iter().map(|published|
            published.value.get::<i64>().unwrap()).collect();
        assert_eq!(values, vec![1, 2, 3]);
        assert_eq!(graph.metadata.nodes[&NodeId(1)].instance_path, Vec::<String>::new());
    }

    #[test]
    fn module_compiler_rejects_recursive_definitions_but_not_data_cycles() {
        let mut a = ModuleDefBuilder::new("A");
        a.try_instance("b", "B").unwrap();
        let mut b = ModuleDefBuilder::new("B");
        b.try_instance("a", "A").unwrap();
        assert!(compile_module(vec![a.build(), b.build()], "A").err().unwrap().contains("recursive module"));

        let mut cycle = ModuleDefBuilder::new("data-cycle");
        cycle.node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Deferred),
            slot(1, S, BufferMode::Deferred)])], vec![out(0, S)], CohortMode::Inherit,
            vec![], |ctx| Ok(vec![emit(0, ctx.arg::<String>(0)?)])));
        cycle.connect(ModuleOutputSource::Node(output(1, 0)),
            ModuleInputTarget::Node(input(1, 0, 1)));
        cycle.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        cycle.output(0, "output", S, ModuleOutputSource::Node(output(1, 0)));
        cycle.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        assert!(compile_module(vec![cycle.build()], "data-cycle").is_ok());
    }

    #[test]
    fn compiler_budgets_deep_wide_and_high_work_module_expansion() {
        let mut depth_catalog = ModuleCatalog::new();
        for depth in 0..=MAX_MODULE_EXPANSION_DEPTH {
            let mut definition = ModuleDefBuilder::new(format!("depth-{depth}"));
            if depth < MAX_MODULE_EXPANSION_DEPTH {
                definition.try_instance("next", format!("depth-{}", depth + 1)).unwrap();
            }
            depth_catalog.add(definition.build()).unwrap();
        }
        assert!(depth_catalog.compile("depth-0").err().unwrap().message.contains("depth exceeds"));

        let mut wide_catalog = ModuleCatalog::new();
        wide_catalog.add(ModuleDefBuilder::new("leaf").build()).unwrap();
        let mut wide = ModuleDefBuilder::new("wide");
        for index in 0..=MAX_MODULE_INSTANCE_COUNT {
            wide.instances.push(ModuleInstance::new(format!("m{index}"), "leaf").unwrap());
        }
        wide_catalog.add(wide.build()).unwrap();
        assert!(wide_catalog.compile("wide").err().unwrap().message.contains("instance"));

        let mut large = ModuleDefBuilder::new("large");
        large.node(passthrough(1));
        large.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0));
            (MAX_MODULE_EXPANSION_WORK + 1) as usize]);
        let mut work_catalog = ModuleCatalog::new();
        work_catalog.add(large.build()).unwrap();
        assert!(work_catalog.compile("large").err().unwrap().message.contains("work exceeds"));
    }

    #[test]
    fn module_compiler_rejects_duplicate_instances_and_boundary_bypasses() {
        let mut duplicate = ModuleDefBuilder::new("duplicate-root");
        duplicate.try_instance("same", "identity").unwrap();
        assert_eq!(duplicate.try_instance("same", "identity"),
            Err(ModuleNameError::DuplicateName("same".to_string())));
        assert_eq!(duplicate.instances.len(), 1, "failed registration leaves no stale instance");
        let duplicate = compile_module(vec![passthrough_module("identity", 1), duplicate.build()],
            "duplicate-root").unwrap();
        assert!(matches!(duplicate.find("same"), Some(FindResult::Module(_))));

        let mut bypass = ModuleDefBuilder::new("bypass-root");
        bypass.try_instance("private", "identity").unwrap();
        bypass.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        bypass.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        assert!(compile_module(vec![passthrough_module("identity", 1), bypass.build()],
            "bypass-root").err().unwrap().contains("unknown local node"));

        let mut output_bypass = ModuleDefBuilder::new("output-bypass-root");
        output_bypass.try_instance("private", "identity").unwrap();
        output_bypass.output(0, "hidden", S, ModuleOutputSource::Node(output(1, 0)));
        assert!(compile_module(vec![passthrough_module("identity", 1), output_bypass.build()],
            "output-bypass-root").err().unwrap().contains("unknown local node"));
    }

    #[test]
    fn entry_triggers_do_not_modify_nested_module_definitions() {
        let mut child = ModuleDefBuilder::new("child-with-entry");
        child.node(passthrough(1));
        child.input(0, "incoming", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        child.output(0, "outgoing", S, ModuleOutputSource::Node(output(1, 0)));
        child.trigger(42, vec![ModuleTriggerTarget::PublicInput(0)]);
        let mut root = ModuleDefBuilder::new("parent-entry");
        root.try_instance("child", "child-with-entry").unwrap();
        root.input(0, "incoming", S, vec![ModuleInputTarget::Instance {name:"child".into(), port:0}]);
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let graph = compile_module(vec![child.build(),root.build()], "parent-entry").unwrap();
        assert!(graph.triggers.contains_key(&TriggerId(1)));
        assert!(!graph.triggers.contains_key(&TriggerId(42)));
    }

    #[test]
    fn module_compiler_rejects_hidden_triggers_invalid_exports_and_schema_mismatches() {
        let mut trigger = ModuleDefBuilder::new("trigger-root");
        trigger.node(passthrough(1));
        trigger.trigger(1, vec![ModuleTriggerTarget::Internal(input(1, 0, 0))]);
        assert!(compile_module(vec![trigger.build()], "trigger-root").err().unwrap()
            .contains("bypasses its public input boundary"));

        let mut nested_trigger = ModuleDefBuilder::new("triggering-child");
        nested_trigger.node(passthrough(1));
        nested_trigger.trigger(2, vec![ModuleTriggerTarget::Internal(input(1, 0, 0))]);
        let mut root = ModuleDefBuilder::new("nested-trigger-root");
        root.try_instance("child", "triggering-child").unwrap();
        let nested = compile_module(vec![nested_trigger.build(), root.build()], "nested-trigger-root").unwrap();
        assert!(nested.triggers.is_empty(), "nested entry bindings must not activate in the parent");

        let mut wrong_schema = ModuleDefBuilder::new("wrong-schema-root");
        wrong_schema.node(passthrough(1));
        wrong_schema.input(0, "input", I, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        assert!(compile_module(vec![wrong_schema.build()], "wrong-schema-root")
            .err().unwrap().contains("schema mismatch on module input"));

        let mut bad_export = ModuleDefBuilder::new("bad-export-root");
        bad_export.node(passthrough(1));
        bad_export.output(0, "output", S, ModuleOutputSource::Node(output(1, 9)));
        assert!(compile_module(vec![bad_export.build()], "bad-export-root")
            .err().unwrap().contains("unknown local output"));
    }

    // The typed constructor couples input decoding and output shape at
    // construction time; only the finished action is erased by the executor.
    struct TextArgs { value: String }
    impl InputSchema for TextArgs {
        fn slots() -> Result<Vec<InputSlot>, LayoutError> {
            Ok(vec![InputSlot::deferred::<String>(0).named("value")])
        }
    }
    impl FromSelected for TextArgs {
        fn from_values(values: &SelectedValues<'_>) -> Result<Self, String> {
            Ok(Self { value: values.get("value")? })
        }
    }
    struct TextResult(String);
    impl OutputSchema for TextResult {
        fn output_slot() -> OutputSlot {
            OutputSlot::typed::<String>(0).named("value")
        }
    }
    impl IntoEmission for TextResult {
        fn into_emission(self) -> Emission { emit(0, self.0) }
    }

    #[test]
    fn inferred_typed_node_has_one_bound_decoder_and_output_contract() {
        let mut module = ModuleDefBuilder::new("typed-example");
        // The node ID is assigned by the graph, not the author.
        module.add_named("typed", NodeTemplate::typed("run", AllSameCohort,
            |args: TextArgs, _ctx| Ok(TextResult(format!("typed:{}", args.value)))).unwrap()).unwrap();
        module.input(0, "value", S, vec![ModuleInputTarget::NodeNamed {
            node: "typed".into(), port: "run".into(), slot: "value".into(),
        }]);
        module.output(0, "value", S, ModuleOutputSource::NodeNamed {
            node: "typed".into(), output: "value".into(),
        });
        module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let mut catalog = ModuleCatalog::new();
        catalog.add(module.build()).unwrap();
        let graph = catalog.compile("typed-example").unwrap();
        let report = Deployment::new(graph).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![Value::of("hello".to_owned())]).unwrap();
        assert!(report.success(), "{:?}", report.errors());
        assert_eq!(report.publications().next().unwrap().value::<String>().unwrap(), "typed:hello");
    }

    #[test]
    fn two_ports_share_one_node_private_state_per_engine() {
        struct Counter { value: std::sync::atomic::AtomicUsize }
        impl NodeAction for Counter {
            fn execute(&self, ctx: InvocationCtx) -> Result<Vec<Emission>, String> {
                let n = self.value.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                Ok(vec![emit(0, format!("{n}:{}:{}", ctx.source_port_id(), ctx.arg::<String>(0)?))])
            }
        }
        let mut builder = FlatGraphBuilder::default();
        builder.add_node(NodeTemplate::with_action_factory(1, vec![
            Port::all(0, vec![InputSlot::deferred::<String>(0)]),
            Port::all(1, vec![InputSlot::deferred::<String>(0)]),
        ], vec![OutputSlot::typed::<String>(0)],
            || Counter { value: std::sync::atomic::AtomicUsize::new(0) }));
        builder.trigger(1, vec![input(1, 0, 0)]);
        builder.trigger(2, vec![input(1, 1, 0)]);
        let compiled = builder.build().unwrap();
        let first = Deployment::new(Arc::clone(&compiled)).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let second = Deployment::new(compiled).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let fire = |engine: &Engine, trigger| engine.fire(trigger, vec![Value::of("x".to_owned())])
            .unwrap().publications().next().unwrap().value::<String>().unwrap();
        assert_eq!(fire(&first, 1), "1:0:x");
        assert_eq!(fire(&first, 2), "2:1:x");
        assert_eq!(fire(&second, 2), "1:1:x");
    }

    #[test]
    fn typed_action_factory_owns_one_private_state_per_engine() {
        let mut module = ModuleDefBuilder::new("typed-factory");
        module.add_named("typed", NodeTemplate::typed_factory("run", AllSameCohort,
            || {
                let counter = std::sync::atomic::AtomicUsize::new(0);
                move |args: TextArgs, _ctx: &InvocationCtx| {
                    let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    Ok(TextResult(format!("{n}:{}", args.value)))
                }
            }).unwrap()).unwrap();
        module.input(0, "value", S, vec![ModuleInputTarget::NodeNamed {
            node: "typed".into(), port: "run".into(), slot: "value".into(),
        }]);
        module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let mut catalog = ModuleCatalog::new();
        catalog.add(module.build()).unwrap();
        let graph = catalog.compile("typed-factory").unwrap();
        let a = Deployment::new(Arc::clone(&graph)).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let b = Deployment::new(graph).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let fire = |engine: &Engine| engine.fire(1, vec![Value::of("x".to_owned())])
            .unwrap().publications().next().unwrap().value::<String>().unwrap();
        assert_eq!(fire(&a), "1:x");
        assert_eq!(fire(&a), "2:x");
        assert_eq!(fire(&b), "1:x");
    }

    #[test]
    fn indirect_emission_color_is_not_reported_as_direct_delivery() {
        let mut builder = FlatGraphBuilder::default();
        builder.add_node(named_transform(1, "source"));
        builder.add_node(named_transform(2, "intermediate"));
        builder.add_node(node(3, vec![all(0, vec![
            slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred)
        ])], vec![], CohortMode::Inherit, vec![], |_| Ok(vec![])));
        builder.connect(output(1, 0), input(2, 0, 0));
        builder.connect(output(2, 0), input(3, 0, 1));
        let graph = builder.build().unwrap();
        let mut state = WaveState::new(&graph, WaveId(1));
        assert!(state.deliver_batch(vec![Incoming {
            to: input(3, 0, 0), cohort: CohortId(7), value: enc("x".to_owned()),
        }]));
        state.record_settled(InvocationId(9), NodeId(1), CohortId(7),
            SettledOutcome::Emitted(BTreeSet::from([input(2, 0, 0)])));
        let port = &graph.nodes[&NodeId(3)].ports[0];
        let snapshot = state.snapshots(NodeId(3), port);
        let facts = snapshot.colors.get(&CohortId(7)).unwrap();
        assert!(facts.iter().any(|fact| fact.invocation == InvocationId(9)
            && fact.state == ColorState::Terminal(ColorOutcome::EmittedTowardReachablePort)));
        assert!(!facts.iter().any(|fact| fact.state == ColorState::Terminal(ColorOutcome::Delivered)));
    }

    #[test]
    fn skipped_output_edge_is_closed_not_a_delivered_color() {
        let mut builder = FlatGraphBuilder::default();
        builder.add_node(named_transform(1, "source"));
        builder.add_node(named_transform(2, "taken_if_broadcast"));
        builder.add_node(node(3, vec![all(0, vec![
            slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred)
        ])], vec![], CohortMode::Inherit, vec![], |_| Ok(vec![])));
        builder.add_node(named_transform(4, "selected"));
        builder.connect(output(1, 0), input(2, 0, 0));
        builder.connect(output(1, 0), input(4, 0, 0));
        builder.connect(output(2, 0), input(3, 0, 1));
        let graph = builder.build().unwrap();
        let mut state = WaveState::new(&graph, WaveId(1));
        assert!(state.deliver_batch(vec![Incoming {
            to: input(3, 0, 0), cohort: CohortId(7), value: enc("x".to_owned()),
        }]));
        // Source physically connects to the eventual consumer through node 2,
        // but its selector chose only the edge ending at node 4.
        state.record_settled(InvocationId(9), NodeId(1), CohortId(7),
            SettledOutcome::Emitted(BTreeSet::from([input(4, 0, 0)])));
        let port = &graph.nodes[&NodeId(3)].ports[0];
        let snapshot = state.snapshots(NodeId(3), port);
        let facts = snapshot.colors.get(&CohortId(7)).unwrap();
        assert!(facts.iter().any(|fact| fact.invocation == InvocationId(9)
            && fact.state == ColorState::Terminal(ColorOutcome::Closed)));
        assert!(!facts.iter().any(|fact| fact.invocation == InvocationId(9)
            && matches!(&fact.state, ColorState::Terminal(ColorOutcome::Delivered | ColorOutcome::EmittedTowardReachablePort))));
    }

    #[test]
    fn typed_symbolic_handles_reject_wrong_scope_and_incorrect_schema() {
        let mut left = ModuleDefBuilder::new("left-declaration");
        let mut right = ModuleDefBuilder::new("right-declaration");
        let a = left.register("a", NodeTemplate::typed("run", AllSameCohort,
            |args: TextArgs, _| Ok(TextResult(args.value))).unwrap()).unwrap();
        let b = left.register("b", NodeTemplate::typed("run", AllSameCohort,
            |args: TextArgs, _| Ok(TextResult(args.value))).unwrap()).unwrap();
        let alien = right.register("c", NodeTemplate::typed("run", AllSameCohort,
            |args: TextArgs, _| Ok(TextResult(args.value))).unwrap()).unwrap();
        left.connect_decl(a.output::<String>("value"), b.input("run", "value")).unwrap();
        assert!(left.connect_decl(a.output::<String>("value"), alien.input("run", "value"))
            .err().unwrap().contains("another declaration"));
        assert!(left.connect_decl(a.output::<i64>("value"), b.input("run", "value"))
            .err().unwrap().contains("schema mismatch"));
        left.export_input("value", vec![a.input::<String>("run", "value")]).unwrap();
        left.export_output("value", b.output::<String>("value")).unwrap();
        assert_eq!(left.edges.len(), 1, "invalid attempts must not insert connections");
        let mut catalog = ModuleCatalog::new();
        catalog.add(left.build()).unwrap();
        assert!(catalog.compile("left-declaration").is_ok());
    }

    #[test]
    fn symbolic_connection_rejects_removed_port_at_compilation() {
        let mut module = ModuleDefBuilder::new("renamed-output");
        let source = module.register("source", NodeTemplate::typed("run", AllSameCohort,
            |args: TextArgs, _| Ok(TextResult(args.value))).unwrap()).unwrap();
        let sink = module.register("sink", NodeTemplate::typed("run", AllSameCohort,
            |args: TextArgs, _| Ok(TextResult(args.value))).unwrap()).unwrap();
        module.connect_decl(source.output::<String>("value"), sink.input("run", "value"))
            .unwrap();
        // Model an editable declaration change before future region replacement.
        let old = module.nodes.iter_mut().find(|n| n.name.as_deref() == Some("source")).unwrap();
        old.outputs[0].name = Some("renamed".to_owned());
        let mut catalog = ModuleCatalog::new();
        catalog.add(module.build()).unwrap();
        assert!(catalog.compile("renamed-output").err().unwrap().message.contains("unknown output"));
    }

    #[test]
    fn symbolic_connections_resolve_after_declaration_changes() {
        let mut original = ModuleDefBuilder::new("symbolic");
        let mut a = named_transform(1, "producer").named("producer").unwrap();
        let mut b = named_transform(2, "consumer").named("consumer").unwrap();
        for node in [&mut a, &mut b] {
            node.ports[0].name = Some("run".into());
            node.ports[0].slots[0].name = Some("value".into());
            node.outputs[0].name = Some("result".into());
        }
        original.add(a).unwrap();
        original.add(b).unwrap();
        original.connect_named("producer", "result", "consumer", "run", "value").unwrap();
        let edges = original.edges.clone();
        assert!(matches!(edges[0].from, ModuleOutputSource::NodeNamed { .. }));
        assert!(matches!(edges[0].to, ModuleInputTarget::NodeNamed { .. }));
        // Symbolic wiring is resolved by the compiler, never frozen as numeric endpoints.
        let original_def = original.build().definition;
        let mut catalog = ModuleCatalog::new();
        catalog.add(ModuleEntry { definition: original_def, triggers: vec![] }).unwrap();
        assert!(catalog.compile("symbolic").is_ok());
    }

    #[test]
    fn module_compiler_rejects_duplicate_public_names_unknown_defs_and_ports() {
        let mut duplicate_name = ModuleDefBuilder::new("duplicate-port-root");
        duplicate_name.node(passthrough(1));
        duplicate_name.input(0, "same", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        duplicate_name.output(0, "same", S, ModuleOutputSource::Node(output(1, 0)));
        // Input and output live in different name namespaces.  This is an
        // identity interface rather than a duplicate declaration.
        assert!(compile_module(vec![duplicate_name.build()], "duplicate-port-root").is_ok());

        let mut duplicate_inputs = ModuleDefBuilder::new("duplicate-input-root");
        duplicate_inputs.node(passthrough(1));
        duplicate_inputs.input(0, "same", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        duplicate_inputs.input(1, "same", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        assert!(compile_module(vec![duplicate_inputs.build()], "duplicate-input-root")
            .err().unwrap().contains("duplicate module public port"));

        let mut unknown_definition = ModuleDefBuilder::new("unknown-def-root");
        unknown_definition.try_instance("missing", "not-defined").unwrap();
        assert!(compile_module(vec![unknown_definition.build()], "unknown-def-root")
            .err().unwrap().contains("unknown definition"));

        let mut unknown_port = ModuleDefBuilder::new("unknown-port-root");
        unknown_port.try_instance("child", "identity").unwrap();
        unknown_port.input(0, "input", S, vec![ModuleInputTarget::Instance {
            name: "child".into(), port: 88,
        }]);
        assert!(compile_module(vec![passthrough_module("identity", 1), unknown_port.build()],
            "unknown-port-root").err().unwrap().contains("unknown input port"));

        let mut catalog = ModuleCatalog::default();
        assert!(catalog.add(passthrough_module("duplicate", 1)).is_ok());
        assert!(catalog.add(passthrough_module("duplicate", 2)).is_err());
    }

    #[test]
    fn module_builder_rejects_duplicate_local_nodes_and_expansion_rejects_id_overflow() {
        let mut duplicate_nodes = ModuleDefBuilder::new("duplicate-local-nodes");
        duplicate_nodes.add(NodeDef::Node(passthrough(1))).unwrap();
        assert_eq!(duplicate_nodes.add(NodeDef::Node(passthrough(1))),
            Err(ModuleNameError::DuplicateNodeId(1)));
        assert_eq!(duplicate_nodes.nodes.len(), 1);

        let mut catalog = ModuleCatalog::default();
        catalog.add(passthrough_module("overflow", 1)).unwrap();
        let mut expander = ModuleExpander {
            catalog: &catalog,
            builder: FlatGraphBuilder::default(),
            metadata: CompiledMetadata::default(),
            next_node: u16::MAX as u32 + 1,
            next_edge: u16::MAX as u32 + 1,
            expanded_instances: Cell::new(0),
            expansion_work: Cell::new(0),
            limits: CompileLimits::default(),
        };
        let error = expander.expand_scope("overflow", &mut Vec::new(), &mut Vec::new())
            .err().unwrap();
        assert!(error.contains("node ID capacity"));

        let mut edges = ModuleDefBuilder::new("edge-overflow");
        edges.node(passthrough(1));
        edges.node(passthrough(2));
        edges.connect(ModuleOutputSource::Node(output(1, 0)),
            ModuleInputTarget::Node(input(2, 0, 0)));
        let mut catalog = ModuleCatalog::default();
        catalog.add(edges.build()).unwrap();
        let mut expander = ModuleExpander {
            catalog: &catalog,
            builder: FlatGraphBuilder::default(),
            metadata: CompiledMetadata::default(),
            next_node: 1,
            next_edge: u16::MAX as u32 + 1,
            expanded_instances: Cell::new(0),
            expansion_work: Cell::new(0),
            limits: CompileLimits::default(),
        };
        let error = expander.expand_scope("edge-overflow", &mut Vec::new(), &mut Vec::new())
            .err().unwrap();
        assert!(error.contains("edge ID capacity"));
    }

    #[test]
    fn module_compiler_rejects_duplicate_resolved_public_input_targets() {
        let mut direct = ModuleDefBuilder::new("duplicate-targets");
        direct.node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Deferred)])],
            vec![], CohortMode::Inherit, vec![], |_ctx| Ok(vec![])));
        direct.input(0, "input", S, vec![
            ModuleInputTarget::Node(input(1, 0, 0)),
            ModuleInputTarget::Node(input(1, 0, 0)),
        ]);
        assert!(compile_module(vec![direct.build()], "duplicate-targets")
            .err().unwrap().contains("duplicate target on module input"));

        let mut child = ModuleDefBuilder::new("target-child");
        child.node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Deferred)])],
            vec![], CohortMode::Inherit, vec![], |_ctx| Ok(vec![])));
        child.input(0, "input", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        let mut parent = ModuleDefBuilder::new("nested-duplicate-targets");
        parent.try_instance("child", "target-child").unwrap();
        parent.input(0, "input", S, vec![
            ModuleInputTarget::Instance { name: "child".into(), port: 0 },
            ModuleInputTarget::Instance { name: "child".into(), port: 0 },
        ]);
        assert!(compile_module(vec![child.build(), parent.build()], "nested-duplicate-targets")
            .err().unwrap().contains("duplicate target on module input"));
    }

    // 3. Slot and policy unit tests: direct internal state observation.
    #[test]
    fn all_same_cohort_joins_only_matching_deliveries() {
        let mut snap = synthetic(4,&[(0,11),(1,22)]);
        assert!(AllSameCohort.select(&snap).is_none()); // third slot not present
        snap.slots.pop(); // make it an actual two-slot port
        let selected = AllSameCohort.select(&snap).unwrap();
        assert_eq!(selected.picks.iter().map(|p|p.slot).collect::<Vec<_>>(),vec![SlotId(0),SlotId(1)]);
        snap.slots[1].candidates[0].cohort = CohortId(5);
        assert!(AllSameCohort.select(&snap).is_none());
        assert_eq!(AllSameCohort.incomplete(&snap).len(),2);
    }
    #[test]
    fn any_one_selects_available_slot_without_consuming_other_slot() {
        let mut snap = synthetic(3,&[(1,13)]);
        snap.slots.truncate(2);
        let selected = AnyOne.select(&snap).unwrap();
        assert_eq!(selected.picks.len(),1);
        assert_eq!(selected.picks[0].slot,SlotId(1));
        assert_eq!(snap.slots[1].candidates.len(),1);
        assert!(AnyOne.incomplete(&snap).is_empty());
    }
    #[test]
    fn arbitrary_any_two_args_are_typed_and_ordered() {
        let args = AnyTwoOfThree::build_args(&BTreeMap::from([
            (SlotId(2),enc("right".to_string())),(SlotId(0),enc("left".to_string()))
        ])).unwrap();
        assert_eq!(args.first,(SlotId(0),"left".to_string()));
        assert_eq!(args.second,(SlotId(2),"right".to_string()));
        assert!(AnyTwoOfThree::build_args(&BTreeMap::from([(SlotId(0),enc(2_i64))])).is_err());
    }
    #[test]
    fn immediate_slot_collision_rejects_whole_batch_not_first_value_only() {
        let graph=single_with_policy(AllSameCohort,BufferMode::Immediate);
        let mut state=WaveState::new(&graph,WaveId(1));
        assert!(!state.deliver_batch(vec![
            Incoming{to:input(1,0,0),cohort:CohortId(1),value:enc("a".to_string())},
            Incoming{to:input(1,0,0),cohort:CohortId(1),value:enc("b".to_string())},
            Incoming{to:input(1,0,1),cohort:CohortId(1),value:enc("c".to_string())},
        ]));
        assert!(state.buffers.is_empty());
        assert!(state.ready.is_empty());
        assert_eq!(state.report.errors,vec![WaveError::SlotOccupied(input(1,0,0))]);
    }
    #[test]
    fn wrong_schema_delivery_rejects_whole_batch_without_stealing_values() {
        let graph=single_with_policy(AllSameCohort,BufferMode::Deferred);
        let mut state=WaveState::new(&graph,WaveId(1));
        assert!(!state.deliver_batch(vec![
            Incoming{to:input(1,0,0),cohort:CohortId(2),value:enc("okay".to_string())},
            Incoming{to:input(1,0,1),cohort:CohortId(2),value:enc(3_i64)},
        ]));
        assert!(state.buffers.is_empty());assert!(state.ready.is_empty());
        assert_eq!(state.report.errors,vec![WaveError::WrongSchema(input(1,0,1))]);
    }
    #[test]
    fn deferred_buffer_picks_fifo_and_never_reuses_delivery_id() {
        let graph=single_with_policy(AnyOne,BufferMode::Deferred);
        let mut state=WaveState::new(&graph,WaveId(1));
        assert!(state.deliver_batch((0..4).map(|i| Incoming{
            to:input(1,0,0),cohort:CohortId(7),value:enc(format!("v{i}"))
        }).collect()));
        assert_eq!(state.ready.len(),4);
        let mut ids=BTreeSet::new();
        for (i, invocation) in state.ready.iter().enumerate() {
            assert!(ids.insert(invocation.id));
            assert_eq!(invocation.args.raw()[&SlotId(0)].get::<String>().unwrap(),format!("v{i}"));
        }
        assert!(state.buffers.values().all(|q|q.is_empty()));
    }

    #[derive(Clone, Copy)]
    enum Malice { Duplicate, UnknownSlot, UnknownDelivery, WrongCohort, Empty }
    struct MalformedPolicy(Malice);
    impl PortPolicy for MalformedPolicy {
        fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
            let candidate=snapshot.slots[0].candidates.first()?;
            let base=Pick{slot:candidate.slot,delivery:candidate.delivery};
            let picks=match self.0 {
                Malice::Duplicate => vec![base.clone(),base],
                Malice::UnknownSlot => vec![Pick{slot:SlotId(88),delivery:candidate.delivery}],
                Malice::UnknownDelivery => vec![Pick{slot:candidate.slot,delivery:DeliveryId(9900)}],
                Malice::WrongCohort => vec![base],
                Malice::Empty => vec![],
            };
            Some(Selection{cohort:if matches!(self.0,Malice::WrongCohort){CohortId(777)}else{candidate.cohort},picks})
        }
        fn incomplete(&self,_:&PortSnapshot)->Vec<IncompleteGroup>{vec![]}
    }
    #[test]
    fn malformed_policies_are_rejected_atomically_with_single_error_each() {
        for malice in [Malice::Duplicate,Malice::UnknownSlot,Malice::UnknownDelivery,
                      Malice::WrongCohort,Malice::Empty] {
            let graph=single_with_policy(MalformedPolicy(malice),BufferMode::Deferred);
            let mut st=WaveState::new(&graph,WaveId(1));
            assert!(st.deliver_batch(vec![Incoming{to:input(1,0,0),cohort:CohortId(1),
                value:enc("x".to_string())}]));
            assert_eq!(st.report.errors,vec![WaveError::InvalidSelection(NodeId(1),PortId(0))]);
            assert!(st.ready.is_empty());
            assert_eq!(st.buffers[&input(1,0,0)].len(),1);
            assert!(st.reservations.is_empty());
            st.reevaluate_waiting();
            st.settle();
            assert_eq!(st.report.errors.len(),1,"policy must not cause repeated errors");
        }
    }
    struct MalformedOneCohort;
    impl PortPolicy for MalformedOneCohort {
        fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
            let candidate = snapshot.slots[0].candidates.first()?;
            if candidate.cohort == CohortId(10) {
                let pick = Pick { slot: candidate.slot, delivery: candidate.delivery };
                Some(Selection { cohort: candidate.cohort, picks: vec![pick.clone(), pick] })
            } else {
                AnyOne.select(snapshot)
            }
        }
        fn incomplete(&self, _snapshot: &PortSnapshot) -> Vec<IncompleteGroup> { vec![] }
    }
    #[test]
    fn malformed_selection_rejects_only_its_identifiable_cohort() {
        let graph = single_with_policy(MalformedOneCohort, BufferMode::Deferred);
        let mut state = WaveState::new(&graph, WaveId(1));
        assert!(state.deliver_batch(vec![
            Incoming { to: input(1, 0, 0), cohort: CohortId(10), value: enc("bad".to_owned()) },
            Incoming { to: input(1, 0, 0), cohort: CohortId(20), value: enc("good".to_owned()) },
        ]));

        assert_eq!(state.report.errors, vec![WaveError::InvalidSelection(NodeId(1), PortId(0))]);
        assert_eq!(state.ready.len(), 1, "the unrelated cohort remains selectable");
        assert_eq!(state.ready[0].cohort, CohortId(20));
        assert!(!state.failed_ports.contains(&(NodeId(1), PortId(0))));
        assert_eq!(state.buffers[&input(1, 0, 0)].len(), 1);
        assert_eq!(state.buffers[&input(1, 0, 0)][0].cohort, CohortId(10));
    }
    struct MalformedRetirement;
    impl PortPolicy for MalformedRetirement {
        fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
            AnyOne.select(snapshot)
        }
        fn incomplete(&self, _snapshot: &PortSnapshot) -> Vec<IncompleteGroup> { vec![] }
        fn retire(&self, _snapshot: &PortSnapshot, selection: &Selection) -> Vec<Pick> {
            selection.picks.clone()
        }
    }
    #[test]
    fn malformed_policy_retirement_is_rejected_without_consuming_deliveries() {
        let graph = single_with_policy(MalformedRetirement, BufferMode::Deferred);
        let mut state = WaveState::new(&graph, WaveId(1));
        assert!(state.deliver_batch(vec![Incoming {
            to: input(1, 0, 0), cohort: CohortId(4), value: enc("kept".to_owned()),
        }]));
        assert_eq!(state.report.errors,
            vec![WaveError::InvalidSelection(NodeId(1), PortId(0))]);
        assert!(state.ready.is_empty());
        assert!(state.reservations.is_empty());
        assert_eq!(state.buffers[&input(1, 0, 0)].len(), 1);
    }
    struct RejectPrepare;
    impl PortPolicy for RejectPrepare {
        fn select(&self,snapshot:&PortSnapshot)->Option<Selection>{AnyOne.select(snapshot)}
        fn incomplete(&self,_:&PortSnapshot)->Vec<IncompleteGroup>{vec![]}

    }
    #[test]
    fn typed_args_construction_failure_does_not_consume_or_enqueue() {
        let mut graph=single_with_policy(RejectPrepare,BufferMode::Deferred);
        let owned_graph = Arc::get_mut(&mut graph).expect("uniquely owned graph");
        let owned_node = Arc::get_mut(owned_graph.nodes.get_mut(&NodeId(1)).unwrap())
            .expect("uniquely owned node");
        owned_node.ports[0].args = Some(Arc::new(ValidatorArgsBuilder(
            |_: &SelectedValues<'_>| Err::<(), String>("bad arguments".into()))));
        let mut st=WaveState::new(&graph,WaveId(1));
        assert!(st.deliver_batch(vec![Incoming{to:input(1,0,1),cohort:CohortId(1),
            value:enc("x".to_string())}]));
        assert_eq!(st.report.errors,vec![WaveError::InvalidArgs(NodeId(1), PortId(0),
            CohortId(1), "bad arguments".into())]);
        assert!(st.ready.is_empty());assert!(st.reservations.is_empty());
        assert_eq!(st.buffers[&input(1,0,1)].len(),1);
    }
    #[test]
    fn prepare_failure_rejects_only_its_cohort_and_preserves_other_work() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![custom_policy(0, vec![
                slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred),
            ], RejectBadCohort).validate(validate_cohort)], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, format!("pair({},{})",
                    ctx.arg::<String>(0)?, ctx.arg::<String>(1)?))])
            }));
        let engine = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let incoming = |slot: u16, cohort: u64, value: &str| Incoming {
            to: input(1, 0, slot), cohort: CohortId(cohort),
            value: enc(value.to_string()),
        };
        let report = engine.run(vec![
            incoming(0, 10, "bad"), incoming(1, 10, "bad-right"),
            incoming(0, 20, "good-left"), incoming(1, 20, "good-right"),
        ]).unwrap();

        assert_eq!(report.count(1), 1, "the valid cohort must still reserve and execute");
        assert_eq!(report.errors, vec![WaveError::InvalidArgs(NodeId(1), PortId(0),
            CohortId(10), "invalid arguments for bad cohort".into())]);
        assert_eq!(report.published.len(), 1);
        assert_eq!(report.published[0].value.get::<String>().unwrap(), "pair(good-left,good-right)");
    }
    #[test]
    fn different_cohorts_cannot_reserve_partial_pair() {
        let graph=single_with_policy(AllSameCohort,BufferMode::Deferred);
        let mut st=WaveState::new(&graph,WaveId(1));
        assert!(st.deliver_batch(vec![
            Incoming{to:input(1,0,0),cohort:CohortId(5),value:enc("a".to_string())},
            Incoming{to:input(1,0,1),cohort:CohortId(6),value:enc("b".to_string())},
        ]));
        assert!(st.ready.is_empty());
        st.settle();
        let incomplete=st.report.errors.iter().filter(|e|matches!(e,WaveError::Incomplete{..})).count();
        assert_eq!(incomplete,2);
        assert_eq!(st.buffers.values().map(|b|b.len()).sum::<usize>(),2);
    }
    #[test]
    fn independent_ports_can_both_activate_same_node_once_per_port() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,S,BufferMode::Immediate)]),
            all(1,vec![slot(0,S,BufferMode::Immediate)])],vec![out(0,S)],CohortMode::Inherit,
            vec![],|ctx|Ok(vec![emit(0,format!("p{}:{}",ctx.from_port.0,ctx.arg::<String>(0)?))])));
        g.trigger(1,vec![input(1,0,0),input(1,1,0)]);
        let r=Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap().fire(1,
            vec![enc("x".to_string()),enc("y".to_string())]).unwrap();
        assert!(r.success(),"{:?}",r.errors);
        assert_eq!(r.count(1),2);
        assert_eq!(r.trace.executed,vec![NodeId(1),NodeId(1)]);
        assert_eq!(r.published.iter().map(|p|p.value.get::<String>().unwrap()).collect::<Vec<_>>(),
            vec!["p0:x".to_string(),"p1:y".to_string()]);
    }
    #[test]
    fn multiple_invocations_have_distinct_identity_and_retain_cohort() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![any(0,vec![slot(0,S,BufferMode::Deferred)])],
            vec![out(0,S)],CohortMode::Inherit,vec![],|ctx| {
                Ok(vec![emit(0,format!("{}:{}",ctx.invocation.0,ctx.cohort.0))])
            }));
        g.trigger(1,vec![input(1,0,0),input(1,0,0),input(1,0,0)]);
        let r=Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap().fire(1,
            vec![enc("x".to_string()),enc("y".to_string()),enc("z".to_string())]).unwrap();
        assert!(r.success());
        assert_eq!(r.count(1),3);
        assert_eq!(r.published.iter().map(|p|p.value.get::<String>().unwrap()).collect::<Vec<_>>(),
            vec!["1:0".to_string(),"2:0".to_string(),"3:0".to_string()]);
    }
    #[test]
    fn empty_selected_outputs_are_legal_and_do_not_fabricate_messages() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,S,BufferMode::Immediate)])],
            vec![out(0,S)],CohortMode::Inherit,vec![],|_|Ok(vec![])));
        g.add_node(passthrough(2));
        g.connect(output(1,0),input(2,0,0));g.trigger(1,vec![input(1,0,0)]);
        let r=Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1,vec![enc("x".to_string())]).unwrap();
        assert!(r.success());assert_eq!(r.count(1),1);assert_eq!(r.count(2),0);
        assert!(r.published.is_empty());
    }

    // 4. Output validation and atomicity.
    fn invalid_output_engine(f: impl Fn(InvocationCtx)->Result<Vec<Emission>,String>+Send+Sync+'static) -> Engine {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,S,BufferMode::Immediate)])],
            vec![out(0,S),out(1,S)],CohortMode::Inherit,vec![],f));
        g.add_node(passthrough(2));g.add_node(passthrough(3));
        g.connect(output(1,0),input(2,0,0));
        g.connect(output(1,1),input(3,0,0));
        g.trigger(1,vec![input(1,0,0)]);
        Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
    }
    #[test]
    fn duplicate_output_slot_rejects_all_sibling_outputs() {
        let e=invalid_output_engine(|_|Ok(vec![emit(0,"a".to_string()),emit(1,"b".to_string()),emit(0,"c".to_string())]));
        let r=e.fire(1,vec![enc("x".to_string())]).unwrap();
        assert_eq!(r.published.len(),0);assert_eq!(r.count(2),0);assert_eq!(r.count(3),0);
        assert!(matches!(r.errors.as_slice(),[WaveError::InvalidOutput(NodeId(1),_)]));
    }
    #[test]
    fn wrong_output_schema_rejects_all_sibling_outputs() {
        let e=invalid_output_engine(|_|Ok(vec![emit(0,"good".to_string()),emit(1,123_i64)]));
        let r=e.fire(1,vec![enc("x".to_string())]).unwrap();
        assert_eq!(r.published.len(),0);assert_eq!(r.count(2),0);assert_eq!(r.count(3),0);
        assert!(matches!(r.errors.as_slice(),[WaveError::InvalidOutput(NodeId(1),_)]));
    }
    #[test]
    fn nonexistent_output_slot_rejects_all_sibling_outputs() {
        let e=invalid_output_engine(|_|Ok(vec![emit(0,"good".to_string()),emit(9,"bad".to_string())]));
        let r=e.fire(1,vec![enc("x".to_string())]).unwrap();
        assert_eq!(r.published.len(),0);assert_eq!(r.count(2),0);assert_eq!(r.count(3),0);
        assert!(matches!(r.errors.as_slice(),[WaveError::InvalidOutput(NodeId(1),_)]));
    }
    #[test]
    fn destination_occupancy_rejects_entire_fanout_publication() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,S,BufferMode::Immediate)])],
            vec![out(0,S),out(1,S),out(2,S)],CohortMode::Inherit,vec![],|_|
                Ok(vec![emit(0,"good".to_string()),emit(1,"bad1".to_string()),emit(2,"bad2".to_string())])));
        g.add_node(passthrough(2));g.add_node(passthrough(3));
        g.connect(output(1,0),input(2,0,0));
        g.connect(output(1,1),input(3,0,0));g.connect(output(1,2),input(3,0,0));
        g.trigger(1,vec![input(1,0,0)]);
        let r=Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap().fire(1,vec![enc("x".to_string())]).unwrap();
        assert!(matches!(r.errors.as_slice(),[WaveError::SlotOccupied(a)] if *a==input(3,0,0)));
        assert_eq!(r.count(2),0);assert_eq!(r.count(3),0);assert!(r.published.is_empty());
    }
    #[test]
    fn output_fanout_to_two_valid_consumers_is_shared_not_recomputed() {
        let called=Arc::new(AtomicUsize::new(0));let cnt=Arc::clone(&called);
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,S,BufferMode::Immediate)])],
            vec![out(0,S)],CohortMode::Inherit,vec![],move |ctx|{
                cnt.fetch_add(1,AtomicOrdering::SeqCst);
                Ok(vec![emit(0,format!("a({})",ctx.arg::<String>(0)?))])
            }));
        g.add_node(passthrough(2));g.add_node(passthrough(3));
        g.connect(output(1,0),input(2,0,0));g.connect(output(1,0),input(3,0,0));
        g.trigger(1,vec![input(1,0,0)]);
        let r=Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap().fire(1,vec![enc("x".to_string())]).unwrap();
        assert!(r.success());assert_eq!(called.load(AtomicOrdering::SeqCst),1);
        assert_eq!(r.count(1),1);assert_eq!(r.count(2),1);assert_eq!(r.count(3),1);
        assert_eq!(val(&r,2),"a(x)");assert_eq!(val(&r,3),"a(x)");
    }

    // 5. Store access is orthogonal to scheduling and keeps intended state.
    #[test]
    fn unauthorized_read_and_write_fail_without_implicit_activation() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,I,BufferMode::Immediate)])],
            vec![],CohortMode::Inherit,vec![],|ctx| {
                assert!(ctx.read::<i64,_>("counter",|x|*x).is_err());
                assert!(ctx.write::<i64,_>("counter",|x|*x=40).is_err());
                Ok(vec![])
            }));
        g.trigger(1,vec![input(1,0,0)]);
        let mut store=Store::default();store.insert("counter",7_i64);
        let e=Deployment::new(g.build().unwrap()).store(store).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        assert_eq!(*e.store.get::<i64>("counter").unwrap().read().unwrap(),7);
        let r=e.fire(1,vec![enc(1_i64)]).unwrap();
        assert!(r.success());assert_eq!(r.count(1),1);
        assert_eq!(*e.store.get::<i64>("counter").unwrap().read().unwrap(),7);
    }
    #[test]
    fn read_grant_does_not_imply_write_grant() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,I,BufferMode::Immediate)])],
            vec![out(0,I)],CohortMode::Inherit,
            vec![ResourceNeed{key:"counter",access:Access::Read}],|ctx| {
                assert!(ctx.write::<i64,_>("counter",|x|*x=99).is_err());
                Ok(vec![emit(0,ctx.read::<i64,_>("counter",|x|*x)?)])
            }));
        g.trigger(1,vec![input(1,0,0)]);
        let mut store=Store::default();store.insert("counter",17_i64);
        let e=Deployment::new(g.build().unwrap()).store(store).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let r=e.fire(1,vec![enc(1_i64)]).unwrap();
        assert!(r.success());assert_eq!(r.published[0].value.get::<i64>().unwrap(),17);
    }
    #[test]
    fn absent_resource_and_wrong_type_are_rejected() {
        let mut store=Store::default();store.insert("name","abc".to_string());
        assert!(store.get::<i64>("name").is_err());
        assert!(store.get::<i64>("missing").is_err());
        assert_eq!(store.get::<String>("name").unwrap().read().unwrap().as_str(),"abc");
    }
    #[test]
    fn store_writes_persist_across_waves_but_wave_internal_buffers_do_not() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,I,BufferMode::Immediate)])],
            vec![out(0,I)],CohortMode::Inherit,
            vec![ResourceNeed{key:"counter",access:Access::Write}],|ctx| {
                let amount=ctx.arg::<i64>(0)?;
                let result=ctx.write::<i64,_>("counter",|x|{*x+=amount;*x})?;
                Ok(vec![emit(0,result)])
            }));
        g.trigger(1,vec![input(1,0,0)]);
        let mut store=Store::default();store.insert("counter",0_i64);
        let e=Deployment::new(g.build().unwrap()).store(store).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        for (amount,expected) in [(2_i64,2_i64),(3,5),(-4,1)] {
            let r=e.fire(1,vec![enc(amount)]).unwrap();
            assert!(r.success());assert_eq!(r.count(1),1);
            assert_eq!(r.published[0].value.get::<i64>().unwrap(),expected);
        }
    }

    // 6. Coloring and progress are *execution facts*, not permission grants.
    #[test]
    fn coloring_decorator_blocks_only_colored_cohorts() {
        let mut snap=synthetic(4,&[(0,10),(1,11)]);
        snap.slots.truncate(2);
        let select=WhenUpstreamSettles::new(PreferPairOnce);
        assert_eq!(select.select(&snap).unwrap().picks.len(),2);
        snap.colors.insert(CohortId(4), vec![ColorFact {
            invocation: InvocationId(9), node: NodeId(99), state: ColorState::Open,
        }]);
        assert!(select.select(&snap).is_none());
        snap.colors.get_mut(&CohortId(4)).unwrap()[0].state =
            ColorState::Terminal(ColorOutcome::Closed);
        assert_eq!(select.select(&snap).unwrap().picks.len(),2);
    }
    #[test]
    fn coloring_does_not_block_unrelated_cohort() {
        let mut snap=synthetic(5,&[(0,11)]);
        snap.slots.truncate(2);
        snap.colors.insert(CohortId(6), vec![ColorFact {
            invocation: InvocationId(10), node: NodeId(88), state: ColorState::Open,
        }]);
        assert_eq!(WhenUpstreamSettles::new(AnyOne).select(&snap).unwrap().cohort,CohortId(5));
    }

    struct ObserveColors { seen: Arc<std::sync::Mutex<Vec<Vec<ColorFact>>>> }
    impl PortPolicy for ObserveColors {
        fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
            self.seen.lock().unwrap().push(snapshot.colors.values().flatten().cloned().collect());
            AnyOne.select(snapshot)
        }
        fn incomplete(&self, snapshot: &PortSnapshot) -> Vec<IncompleteGroup> {
            AnyOne.incomplete(snapshot)
        }
    }

    #[test]
    fn policies_observe_delivered_closed_and_failed_reachable_colors() {
        for (fails, expected) in [(false, ColorOutcome::Closed), (true, ColorOutcome::Failed("upstream failed".into()))] {
            let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
            let mut graph = FlatGraphBuilder::default();
            graph.add_node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
                vec![out(0, S), out(1, S)], CohortMode::Inherit, vec![], |_ctx| {
                    Ok(vec![emit(0, "left".to_owned()), emit(1, "right".to_owned())])
                }));
            graph.add_node(passthrough(2));
            graph.add_node(node(3, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
                vec![out(0, S)], CohortMode::Inherit, vec![], move |_ctx| {
                    if fails { Err("upstream failed".into()) } else { Ok(vec![]) }
                }));
            graph.add_node(node(4, vec![custom_policy(0, vec![
                    slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred),
                ], WhenUpstreamSettles::new(ObserveColors { seen: Arc::clone(&seen) }))],
                vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                    let value = if ctx.has_arg(0) { ctx.arg::<String>(0)? } else { ctx.arg::<String>(1)? };
                    Ok(vec![emit(0, value)])
                }));
            graph.connect(output(1, 0), input(2, 0, 0));
            graph.connect(output(1, 1), input(3, 0, 0));
            graph.connect(output(2, 0), input(4, 0, 0));
            graph.connect(output(3, 0), input(4, 0, 1));
            graph.trigger(1, vec![input(1, 0, 0)]);

            let report = Deployment::new(graph.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
                .fire(1, vec![enc("start".to_owned())]).unwrap();
            assert_eq!(report.count(4), 1);
            assert_eq!(report.success(), !fails);
            if fails {
                assert!(matches!(report.errors.as_slice(), [WaveError::NodeFailed(NodeId(3), message)]
                    if message == "upstream failed"));
            } else {
                assert_eq!(val(&report, 4), "left");
            }
            let colors = seen.lock().unwrap();
            assert!(colors.iter().flatten().any(|fact| fact.node == NodeId(2)
                && fact.invocation == InvocationId(2)
                && fact.state == ColorState::Terminal(ColorOutcome::Delivered)), "{colors:?}");
            assert!(colors.iter().flatten().any(|fact| fact.node == NodeId(3)
                && fact.invocation == InvocationId(3)
                && fact.state == ColorState::Terminal(expected.clone())), "{colors:?}");
            assert!(colors.iter().any(|snapshot| snapshot.iter().any(|fact|
                fact.node == NodeId(3) && fact.state == ColorState::Terminal(expected.clone()))
                && snapshot.iter().all(|fact| fact.state != ColorState::Open)),
                "terminal policy snapshot must have no open obligations: {colors:?}");
        }
    }

    #[test]
    fn repeated_invocations_on_one_edge_resolve_independent_colors() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut graph = FlatGraphBuilder::default();
        graph.add_node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Deferred)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                let value = ctx.arg::<String>(0)?;
                if value == "deliver" { Ok(vec![emit(0, value)]) } else { Ok(vec![]) }
            }));
        graph.add_node(node(2, vec![custom_policy(0, vec![slot(0, S, BufferMode::Deferred)],
                WhenUpstreamSettles::new(ObserveColors { seen: Arc::clone(&seen) }))],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, ctx.arg::<String>(0)?)])
            }));
        graph.connect(output(1, 0), input(2, 0, 0));
        let engine = Deployment::new(graph.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let report = engine.run(vec![
            Incoming { to: input(1, 0, 0), cohort: CohortId(7), value: enc("close".to_owned()) },
            Incoming { to: input(1, 0, 0), cohort: CohortId(7), value: enc("deliver".to_owned()) },
        ]).unwrap();

        assert!(report.success(), "{:?}", report.errors);
        assert_eq!((report.count(1), report.count(2)), (2, 1));
        assert_eq!(val(&report, 2), "deliver");
        let snapshots = seen.lock().unwrap();
        assert!(snapshots.iter().any(|snapshot| {
            let terminal = |invocation: InvocationId, outcome: ColorOutcome| snapshot.iter().any(|fact|
                fact.invocation == invocation && fact.node == NodeId(1)
                    && fact.state == ColorState::Terminal(outcome.clone()));
            terminal(InvocationId(1), ColorOutcome::Closed)
                && terminal(InvocationId(2), ColorOutcome::Delivered)
                && snapshot.iter().all(|fact| fact.state != ColorState::Open)
        }), "both invocation obligations must settle independently: {snapshots:?}");
    }

    struct ObserveAnyTwo { seen: Arc<std::sync::Mutex<Vec<Vec<ColorFact>>>> }
    impl PortPolicy for ObserveAnyTwo {
        fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
            self.seen.lock().unwrap().push(snapshot.colors.values().flatten().cloned().collect());
            AnyTwoOfThree.select(snapshot)
        }
        fn incomplete(&self, snapshot: &PortSnapshot) -> Vec<IncompleteGroup> {
            AnyTwoOfThree.incomplete(snapshot)
        }

    }

    #[test]
    fn n_of_m_policy_observes_open_closed_and_failed_candidates_at_settlement() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut graph = FlatGraphBuilder::default();
        graph.add_node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S), out(1, S), out(2, S)], CohortMode::Inherit, vec![], |_| {
                Ok(vec![emit(0, "delivered".to_owned()), emit(1, "close".to_owned()),
                    emit(2, "fail".to_owned())])
            }));
        graph.add_node(node(2, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], |_| Ok(vec![])));
        graph.add_node(node(3, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], |_| Err("candidate failed".into())));
        graph.add_node(node(4, vec![custom_policy(0, (0..3)
                .map(|id| slot(id, S, BufferMode::Deferred)).collect(),
                WhenUpstreamSettles::new(ObserveAnyTwo { seen: Arc::clone(&seen) }))
                .args(|values| AnyTwoOfThree::build_args(values.selected))],
            vec![], CohortMode::Inherit, vec![], |_| Ok(vec![])));
        graph.connect(output(1, 0), input(4, 0, 0));
        graph.connect(output(1, 1), input(2, 0, 0));
        graph.connect(output(1, 2), input(3, 0, 0));
        graph.connect(output(2, 0), input(4, 0, 1));
        graph.connect(output(3, 0), input(4, 0, 2));
        graph.trigger(1, vec![input(1, 0, 0)]);

        let report = Deployment::new(graph.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("start".to_owned())]).unwrap();
        assert_eq!(report.count(4), 0);
        assert!(matches!(report.errors.as_slice(), [
            WaveError::NodeFailed(NodeId(3), message),
            WaveError::Incomplete { node: NodeId(4), group, .. }
        ] if message == "candidate failed"
            && group.present == vec![SlotId(0)]
            && group.missing == vec![SlotId(1), SlotId(2)]));

        let snapshots = seen.lock().unwrap();
        assert!(snapshots.iter().any(|snapshot| {
            let state = |node: NodeId, expected: ColorOutcome| snapshot.iter().any(|fact| fact.node == node
                && fact.state == ColorState::Terminal(expected.clone()));
            state(NodeId(1), ColorOutcome::Delivered)
                && state(NodeId(2), ColorOutcome::Closed)
                && state(NodeId(3), ColorOutcome::Failed("candidate failed".into()))
                && snapshot.iter().all(|fact| fact.state != ColorState::Open)
        }), "expected settled causal outcomes, got {snapshots:?}");
    }

    #[test]
    fn required_failure_settles_obligations_and_next_wave_executes() {
        let mut graph = FlatGraphBuilder::default();
        graph.add_node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S), out(1, S)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, "required".to_owned()), emit(1, ctx.arg::<String>(0)?)])
            }));
        graph.add_node(node(2, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                let value = ctx.arg::<String>(0)?;
                if value == "Wave A" { Err("Wave A branch failure".into()) }
                else { Ok(vec![emit(0, value)]) }
            }));
        graph.add_node(node(3, vec![all(0, vec![
                slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred),
            ])], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, format!("{}+{}", ctx.arg::<String>(0)?, ctx.arg::<String>(1)?))])
            }));
        graph.connect(output(1, 0), input(3, 0, 0));
        graph.connect(output(1, 1), input(2, 0, 0));
        graph.connect(output(2, 0), input(3, 0, 1));
        graph.trigger(1, vec![input(1, 0, 0)]);
        let engine = Deployment::new(graph.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();

        let failed = engine.fire(1, vec![enc("Wave A".to_owned())]).unwrap();
        assert_eq!(failed.count(3), 0);
        assert!(matches!(failed.errors.as_slice(), [
            WaveError::NodeFailed(NodeId(2), message),
            WaveError::Incomplete { node: NodeId(3), group, .. }
        ] if message == "Wave A branch failure"
            && group.present == vec![SlotId(0)]
            && group.missing == vec![SlotId(1)]));

        let recovered = engine.fire(1, vec![enc("Wave B".to_owned())]).unwrap();
        assert!(recovered.success(), "{:?}", recovered.errors);
        assert_eq!(recovered.count(3), 1);
        assert_eq!(val(&recovered, 3), "required+Wave B");
    }
    #[test]
    fn optional_pair_policy_populates_both_or_the_reachable_single_option() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(optional_pair_node(1));
        g.trigger(1, vec![input(1, 0, 0), input(1, 0, 1)]);
        g.trigger(2, vec![input(1, 0, 0)]);
        g.trigger(3, vec![input(1, 0, 1)]);
        let engine = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();

        let both = engine.fire(1, vec![
            enc("left".to_string()), enc("right".to_string()),
        ]).unwrap();
        assert!(both.success(), "{:?}", both.errors);
        assert_eq!(val(&both, 1), "left|right");

        let first_only = engine.fire(2, vec![enc("left".to_string())]).unwrap();
        assert!(first_only.success(), "{:?}", first_only.errors);
        assert_eq!(val(&first_only, 1), "left|_");
        assert!(first_only.trace.waits.is_empty(), "unreachable slot must not cause a wait");

        let second_only = engine.fire(3, vec![enc("right".to_string())]).unwrap();
        assert!(second_only.success(), "{:?}", second_only.errors);
        assert_eq!(val(&second_only, 1), "_|right");
        assert!(second_only.trace.waits.is_empty(), "unreachable slot must not cause a wait");
    }
    #[test]
    fn optional_pair_waits_for_reachable_upstream_before_using_single_option() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S), out(1, S)], CohortMode::Inherit, vec![], |_ctx| {
                Ok(vec![emit(0, "left".to_string()), emit(1, "signal".to_string())])
            }));
        g.add_node(node(2, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], |_ctx| Ok(vec![])));
        g.add_node(optional_pair_node(3));
        g.connect(output(1, 0), input(3, 0, 0));
        g.connect(output(1, 1), input(2, 0, 0));
        g.connect(output(2, 0), input(3, 0, 1));
        g.trigger(1, vec![input(1, 0, 0)]);

        let report = Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("start".to_string())]).unwrap();
        assert!(report.success(), "{:?}", report.errors);
        assert_eq!(report.count(3), 1);
        assert_eq!(val(&report, 3), "left|_");
        assert!(report.trace.waits.iter().any(|wait|
            wait.node == NodeId(3) && wait.upstream.contains(&NodeId(2))));
    }
    #[test]
    fn structural_color_reachability_distinguishes_relevant_and_unrelated() {
        let mut g=FlatGraphBuilder::default();
        for n in 1..=4 {g.add_node(passthrough(n));}
        g.connect(output(1,0),input(2,0,0));
        g.connect(output(2,0),input(4,0,0));
        let graph=g.build().unwrap();
        assert_eq!(graph.can_reach_port(NodeId(1),(NodeId(4),PortId(0))),(true,false));
        assert_eq!(graph.can_reach_port(NodeId(3),(NodeId(4),PortId(0))),(false,false));
        assert_eq!(graph.can_reach_port(NodeId(1),(NodeId(4),PortId(0))),(true,false));
        assert_eq!(graph.can_reach_port(NodeId(1),(NodeId(4),PortId(0))),(true,false),
            "repeated reachability queries remain stable");
    }
    #[test]
    fn fresh_cohort_propagation_is_explicit_and_changes_join_correlation() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,S,BufferMode::Immediate)])],
            vec![out(0,S)],CohortMode::FreshPerInvocation,vec![],|ctx|
                Ok(vec![emit(0,ctx.arg::<String>(0)?) ])));
        g.add_node(passthrough(2));
        g.connect(output(1,0),input(2,0,0));
        g.trigger(1,vec![input(1,0,0)]);
        let graph=g.build().unwrap();
        assert_eq!(graph.can_reach_port(NodeId(1),(NodeId(2),PortId(0))),(false,true));
        let r=Deployment::new(graph).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap().fire(1,vec![enc("x".to_string())]).unwrap();
        assert!(r.success());assert_eq!(r.published[0].cohort,r.published[1].cohort);
        assert_ne!(r.published[0].cohort,CohortId(0));
    }

    // 7. Wave-level expected failures must not poison the next wave.
    #[test]
    fn expected_node_failure_in_wave_a_is_followed_by_success_in_wave_b() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,S,BufferMode::Immediate)])],
            vec![out(0,S)],CohortMode::Inherit,vec![],|ctx| {
                let s=ctx.arg::<String>(0)?;
                if s=="fail" {return Err("intentional application error".into());}
                Ok(vec![emit(0,format!("ok({s})"))])
            }));
        g.trigger(1,vec![input(1,0,0)]);
        let e=Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let a=e.fire(1,vec![enc("fail".to_string())]).unwrap();
        assert!(!a.success());assert_eq!(a.count(1),1);assert!(a.published.is_empty());
        assert!(matches!(a.errors.as_slice(),[WaveError::NodeFailed(NodeId(1),why)] if why=="intentional application error"));
        let b=e.fire(1,vec![enc("pass".to_string())]).unwrap();
        assert!(b.success());assert_eq!(val(&b,1),"ok(pass)");
        assert_eq!(b.wave.0,a.wave.0+1);assert_eq!(b.count(1),1);
    }
    #[test]
    fn expected_panic_in_wave_a_is_contained_and_wave_b_still_runs() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,S,BufferMode::Immediate)])],
            vec![out(0,S)],CohortMode::Inherit,vec![],|ctx| {
                let s=ctx.arg::<String>(0)?;
                if s=="panic" {panic!("expected test panic");}
                Ok(vec![emit(0,format!("fine({s})"))])
            }));
        g.trigger(1,vec![input(1,0,0)]);
        let e=Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let a=e.fire(1,vec![enc("panic".to_string())]).unwrap();
        assert!(matches!(a.errors.as_slice(),[WaveError::NodeFailed(NodeId(1),why)] if why=="node panicked"));
        let b=e.fire(1,vec![enc("x".to_string())]).unwrap();
        assert!(b.success());assert_eq!(val(&b,1),"fine(x)");
    }
    #[test]
    fn failed_branch_does_not_suppress_unrelated_success_within_same_wave() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,S,BufferMode::Immediate)])],
            vec![out(0,S)],CohortMode::Inherit,vec![],|ctx|Ok(vec![emit(0,ctx.arg::<String>(0)?) ])));
        g.add_node(node(2,vec![all(0,vec![slot(0,S,BufferMode::Immediate)])],
            vec![out(0,S)],CohortMode::Inherit,vec![],|ctx|{
                let s=ctx.arg::<String>(0)?;
                if s=="bad" {Err("branch failed".into())} else {Ok(vec![emit(0,format!("left({s})"))])}
            }));
        g.add_node(node(3,vec![all(0,vec![slot(0,S,BufferMode::Immediate)])],
            vec![out(0,S)],CohortMode::Inherit,vec![],|ctx|
                Ok(vec![emit(0,format!("right({})",ctx.arg::<String>(0)?))])));
        g.connect(output(1,0),input(2,0,0));g.connect(output(1,0),input(3,0,0));
        g.trigger(1,vec![input(1,0,0)]);
        let e=Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let a=e.fire(1,vec![enc("bad".to_string())]).unwrap();
        assert!(!a.success());assert_eq!(a.count(2),1);assert_eq!(a.count(3),1);
        assert_eq!(val(&a,3),"right(bad)");
        assert!(a.published.iter().all(|p|p.from!=output(2,0)));
        let b=e.fire(1,vec![enc("good".to_string())]).unwrap();
        assert!(b.success(),"{:?}",b.errors);
        assert_eq!(val(&b,2),"left(good)");assert_eq!(val(&b,3),"right(good)");
    }
    #[test]
    fn output_validation_failure_wave_a_does_not_affect_wave_b() {
        let e=invalid_output_engine(|ctx|{
            let v=ctx.arg::<String>(0)?;
            if v=="bad" {Ok(vec![emit(0, "okay".to_string()),emit(1,0_i64)])}
            else {Ok(vec![emit(0,format!("left({v})")),emit(1,format!("right({v})"))])}
        });
        let a=e.fire(1,vec![enc("bad".to_string())]).unwrap();
        assert!(matches!(a.errors.as_slice(),[WaveError::InvalidOutput(NodeId(1),_)]));
        assert!(a.published.is_empty());
        let b=e.fire(1,vec![enc("good".to_string())]).unwrap();
        assert!(b.success());assert_eq!(val(&b,2),"left(good)");assert_eq!(val(&b,3),"right(good)");
    }
    #[test]
    fn fanout_rejection_wave_a_does_not_poison_wave_b() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,S,BufferMode::Immediate)])],
            vec![out(0,S),out(1,S),out(2,S)],CohortMode::Inherit,vec![],|ctx|{
                let s=ctx.arg::<String>(0)?;
                if s=="bad" {Ok(vec![emit(0, "valid".to_string()),emit(1, "one".to_string()),emit(2, "two".to_string())])}
                else {Ok(vec![emit(0,format!("valid({s})"))])}
            }));
        g.add_node(passthrough(2));g.add_node(passthrough(3));
        g.connect(output(1,0),input(2,0,0));
        g.connect(output(1,1),input(3,0,0));g.connect(output(1,2),input(3,0,0));
        g.trigger(1,vec![input(1,0,0)]);
        let e=Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let a=e.fire(1,vec![enc("bad".to_string())]).unwrap();
        assert!(matches!(a.errors.as_slice(),[WaveError::SlotOccupied(_)]));assert!(a.published.is_empty());
        let b=e.fire(1,vec![enc("good".to_string())]).unwrap();
        assert!(b.success());assert_eq!(val(&b,2),"valid(good)");assert_eq!(b.count(3),0);
    }

    // 8. Causal cohorts, publication observation, and cycles.
    #[test]
    fn two_cohorts_in_same_wave_form_two_independent_diamonds() {
        let mut g=FlatGraphBuilder::default();
        for (id,label) in [(1,"a"),(2,"b"),(3,"c")] {
            g.add_node(node(id,vec![all(0,vec![slot(0,S,BufferMode::Deferred)])],
                vec![out(0,S)],CohortMode::Inherit,vec![],move |ctx|
                    Ok(vec![emit(0,format!("{label}({})",ctx.arg::<String>(0)?))])));
        }
        g.add_node(node(4,vec![all(0,vec![slot(0,S,BufferMode::Deferred),slot(1,S,BufferMode::Deferred)])],
            vec![out(0,S)],CohortMode::Inherit,vec![],|ctx|
                Ok(vec![emit(0,format!("d({},{})",ctx.arg::<String>(0)?,ctx.arg::<String>(1)?))])));
        g.connect(output(1,0),input(2,0,0));g.connect(output(1,0),input(3,0,0));
        g.connect(output(2,0),input(4,0,0));g.connect(output(3,0),input(4,0,1));
        let e=Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let r=e.run(vec![
            Incoming{to:input(1,0,0),cohort:CohortId(10),value:enc("x".to_string())},
            Incoming{to:input(1,0,0),cohort:CohortId(20),value:enc("y".to_string())},
        ]).unwrap();
        assert!(r.success(),"{:?}",r.errors);
        for n in 1..=4 {assert_eq!(r.count(n),2);}
        let outputs:BTreeMap<_,_>=r.published.iter().filter(|p|p.from==output(4,0))
            .map(|p|(p.cohort,p.value.get::<String>().unwrap())).collect();
        assert_eq!(outputs[&CohortId(10)],"d(b(a(x)),c(a(x)))");
        assert_eq!(outputs[&CohortId(20)],"d(b(a(y)),c(a(y)))");
    }
    #[test]
    fn separate_waves_are_isolated_even_when_first_has_stranded_join() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,S,BufferMode::Immediate)])],
            vec![out(0,S),out(1,S)],CohortMode::Inherit,vec![],|ctx|{
                let s=ctx.arg::<String>(0)?;
                if s=="A" {Ok(vec![emit(0, "one".to_string())])}
                else {Ok(vec![emit(0, "one".to_string()),emit(1, "two".to_string())])}
            }));
        g.add_node(node(2,vec![all(0,vec![slot(0,S,BufferMode::Deferred),slot(1,S,BufferMode::Deferred)])],
            vec![out(0,S)],CohortMode::Inherit,vec![],|ctx|
                Ok(vec![emit(0,format!("join({},{})",ctx.arg::<String>(0)?,ctx.arg::<String>(1)?))])));
        g.connect(output(1,0),input(2,0,0));g.connect(output(1,1),input(2,0,1));
        g.trigger(1,vec![input(1,0,0)]);
        let e=Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let a=e.fire(1,vec![enc("A".to_string())]).unwrap();
        assert!(!a.success());assert_eq!(a.count(2),0);
        assert!(matches!(a.errors.as_slice(),[WaveError::Incomplete{..}]));
        let b=e.fire(1,vec![enc("B".to_string())]).unwrap();
        assert!(b.success(),"{:?}",b.errors);assert_eq!(b.count(2),1);
        assert_eq!(val(&b,2),"join(one,two)");
    }


    #[test]
    fn dormant_disconnected_cycle_does_not_block_an_unrelated_graph() {
        let mut g=FlatGraphBuilder::default();
        for id in 1..=3 {g.add_node(passthrough(id));}
        g.connect(output(1,0),input(2,0,0));g.connect(output(2,0),input(1,0,0));
        g.trigger(1,vec![input(3,0,0)]);
        let r=Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1,vec![enc("x".to_string())]).unwrap();
        assert!(r.success());assert_eq!(r.count(1),0);assert_eq!(r.count(2),0);
        assert_eq!(val(&r,3),"x");
    }
    #[test]
    fn finite_feedback_cycle_runs_until_node_stops_emitting() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,I,BufferMode::Immediate)])],
            vec![out(0,I)],CohortMode::Inherit,vec![],|ctx| {
                let n=ctx.arg::<i64>(0)?;
                Ok(if n<4 {vec![emit(0,n+1)]}else{vec![]})
            }));
        g.connect(output(1,0),input(1,0,0));
        g.trigger(1,vec![input(1,0,0)]);
        let r=Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1,vec![enc(0_i64)]).unwrap();
        assert!(r.success(),"{:?}",r.errors);assert_eq!(r.count(1),5);
        assert_eq!(r.published.iter().map(|p|p.value.get::<i64>().unwrap()).collect::<Vec<_>>(),vec![1,2,3,4]);
    }
    #[test]
    fn feedback_join_without_second_input_is_expected_deadlock_diagnostic() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,S,BufferMode::Deferred),slot(1,S,BufferMode::Deferred)])],
            vec![out(0,S)],CohortMode::Inherit,vec![],|ctx|
                Ok(vec![emit(0,ctx.arg::<String>(0)?) ])));
        g.connect(output(1,0),input(1,0,1));
        g.trigger(1,vec![input(1,0,0)]);
        let e=Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let r=e.fire(1,vec![enc("x".to_string())]).unwrap();
        assert!(!r.success());assert_eq!(r.count(1),0);
        assert!(matches!(r.errors.as_slice(),[WaveError::Incomplete{node:NodeId(1),group,..}]
            if group.missing==vec![SlotId(1)]));
    }

    // 9. Deterministic DAG generation: evaluate expression independently.
    fn generated_dag(seed: u32, reverse_insert: bool) -> (Engine, String, usize) {
        let count=10_usize;
        // Every node is reachable from node 1: each node i>1 includes i-1 as
        // predecessor. Others are selected by a simple, reproducible generator.
        let mut predecessors:Vec<Vec<usize>>=vec![vec![];count+1];
        let mut rng=seed.wrapping_add(0x9e3779b9);
        for i in 2..=count {
            let mut parents=vec![i-1];
            for _ in 0..2 {
                rng=rng.wrapping_mul(1664525).wrapping_add(1013904223);
                let other=(rng as usize % (i-1))+1;
                if !parents.contains(&other) {parents.push(other);}
            }
            parents.sort_unstable();
            predecessors[i]=parents;
        }
        let mut g=FlatGraphBuilder::default();
        let mut ids: Vec<usize>=(1..=count).collect();
        if reverse_insert {ids.reverse();}
        for i in ids {
            let parents=predecessors[i].len();
            let slots=if i==1 {vec![slot(0,S,BufferMode::Deferred)]}
                else {(0..parents).map(|j|slot(j as u16,S,BufferMode::Deferred)).collect()};
            g.add_node(node(i as u16,vec![all(0,slots)],vec![out(0,S)],CohortMode::Inherit,vec![],move |ctx|{
                let parts=(0..parents).map(|j|ctx.arg::<String>(j as u16)).collect::<Result<Vec<_>,_>>()?;
                let text=if i==1 {format!("n1({})",ctx.arg::<String>(0)?)}
                    else {format!("n{i}({})",parts.join(","))};
                Ok(vec![emit(0,text)])
            }));
        }
        let mut connections=vec![];
        for i in 2..=count {
            for (s,&parent) in predecessors[i].iter().enumerate() {
                connections.push((output(parent as u16,0),input(i as u16,0,s as u16)));
            }
        }
        if reverse_insert {connections.reverse();}
        for (from,to) in connections {g.connect(from,to);}
        g.trigger(1,vec![input(1,0,0)]);
        let mut expected=vec![String::new();count+1];
        expected[1]="n1(x)".into();
        for i in 2..=count {
            let parts:Vec<_>=predecessors[i].iter().map(|p|expected[*p].as_str()).collect();
            expected[i]=format!("n{i}({})",parts.join(","));
        }
        (Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap(),expected[count].clone(),count)
    }
    #[test]
    fn generated_dags_match_independent_expression_evaluator_across_seeds() {
        for seed in 0..32 {
            for reverse in [false,true] {
                let (e,expected,n)=generated_dag(seed,reverse);
                let r=e.fire(1,vec![enc("x".to_string())]).unwrap();
                assert!(r.success(),"seed={seed} reversed={reverse}: {:?}",r.errors);
                assert_eq!(r.published.len(),n);
                for id in 1..=(n as u16) {assert_eq!(r.count(id),1,"seed={seed} node={id}");}
                assert_eq!(val(&r,n as u16),expected,"seed={seed} reversed={reverse}");
            }
        }
    }
    #[test]
    fn generated_dag_is_independent_of_node_and_edge_insertion_order() {
        for seed in 0..20 {
            let (e1,expected,_)=generated_dag(seed,false);
            let (e2,expected_reversed,_)=generated_dag(seed,true);
            let a=e1.fire(1,vec![enc("x".to_string())]).unwrap();
            let b=e2.fire(1,vec![enc("x".to_string())]).unwrap();
            assert!(a.success()&&b.success());
            assert_eq!(expected,expected_reversed);
            assert_eq!(val(&a,10),val(&b,10));
        }
    }

    // 10. Explicit lifecycle limits and negative cases at the mechanism boundary.
    #[test]
    fn premature_settlement_while_invocation_is_queued_is_rejected() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(passthrough(1));
        let graph=g.build().unwrap();
        let mut st=WaveState::new(&graph,WaveId(1));
        assert!(st.deliver_batch(vec![Incoming{to:input(1,0,0),cohort:CohortId(0),
            value:enc("x".to_string())}]));
        assert_eq!(st.ready.len(),1);
        // `settle` is private to the scheduler; this is an intentional panic
        // test proving the current guard rejects an invalid early call.
        let premature=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||st.settle()));
        assert!(premature.is_err());
        assert!(st.report.errors.is_empty());
    }
    #[test]
    fn direct_graph_named_registration_uses_one_checked_index() {
        let mut builder = FlatGraphBuilder::default();
        builder.add_node(passthrough(1).named("worker").unwrap());
        assert_eq!(builder.add_named("worker", passthrough(2)),
            Err(ModuleNameError::DuplicateName("worker".into())));
        assert_eq!(builder.try_add_node(passthrough(1).named("replacement").unwrap()),
            Err(ModuleNameError::DuplicateNodeId(1)));
        assert_eq!(builder.nodes.len(), 1);
        assert_eq!(builder.metadata.name_index.len(), 1);
        let compiled = builder.build().unwrap();
        let Some(FindResult::Node(worker)) = compiled.find("worker") else {
            panic!("pre-named direct graph node was not indexed");
        };
        assert_eq!(worker.id(), 1);
        assert_eq!(worker.name(), Some("worker"));
        assert!(compiled.find("replacement").is_none());
    }

    #[test]
    fn fresh_cohort_ids_cannot_alias_ingress_cohorts() {
        let mut graph = FlatGraphBuilder::default();
        graph.add_node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Deferred)])],
            vec![out(0, S)], CohortMode::FreshPerInvocation, vec![], |ctx| {
                Ok(vec![emit(0, ctx.arg::<String>(0)?)])
            }));
        graph.add_node(node(2, vec![all(0, vec![slot(0, S, BufferMode::Deferred),
            slot(1, S, BufferMode::Deferred)])], vec![], CohortMode::Inherit,
            vec![], |_ctx| Ok(vec![])));
        graph.connect(output(1, 0), input(2, 0, 1));
        let engine = Deployment::new(graph.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let result = engine.run(vec![
            Incoming { to: input(1, 0, 0), cohort: CohortId(0),
                value: enc("fresh".to_string()) },
            Incoming { to: input(2, 0, 0), cohort: CohortId(1),
                value: enc("preexisting".to_string()) },
        ]).unwrap();
        assert_eq!(result.count(1), 1);
        assert_eq!(result.count(2), 0, "distinct causal cohorts must not be joined");
        assert_eq!(result.published[0].cohort, CohortId(2));
        assert!(result.errors.iter().any(|error| matches!(error,
            WaveError::Incomplete { node: NodeId(2), .. })));
    }

    #[test]
    fn builder_duplicate_ids_are_rejected_instead_of_overwritten() {
        let mut node_graph = FlatGraphBuilder::default();
        node_graph.add_node(named_transform(1, "original"));
        let duplicate_node = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
            node_graph.add_node(named_transform(1, "replacement"))));
        assert!(duplicate_node.is_err());
        node_graph.trigger(1, vec![input(1, 0, 0)]);
        let graph = node_graph.build().unwrap();
        let node_report = Deployment::new(graph).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![enc("x".to_string())]).unwrap();
        assert_eq!(val(&node_report, 1), "original(x)");

        let mut trigger_graph = FlatGraphBuilder::default();
        trigger_graph.add_node(passthrough(1));
        trigger_graph.add_node(passthrough(2));
        trigger_graph.trigger(1, vec![input(1, 0, 0)]);
        let duplicate_trigger = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
            trigger_graph.trigger(1, vec![input(2, 0, 0)])));
        assert!(duplicate_trigger.is_err());
    }
    #[test]
    fn failed_node_write_is_not_implicitly_rolled_back_by_wave_engine() {
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,I,BufferMode::Immediate)])],
            vec![out(0,I)],CohortMode::Inherit,
            vec![ResourceNeed{key:"counter",access:Access::Write}],|ctx| {
                let n=ctx.arg::<i64>(0)?;
                let new=ctx.write::<i64,_>("counter",|stored|{*stored+=n;*stored})?;
                if n<0 {return Err("failure after committed store mutation".into());}
                Ok(vec![emit(0,new)])
            }));
        g.trigger(1,vec![input(1,0,0)]);
        let mut store=Store::default();store.insert("counter",10_i64);
        let e=Deployment::new(g.build().unwrap()).store(store).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let a=e.fire(1,vec![enc(-2_i64)]).unwrap();
        assert!(matches!(a.errors.as_slice(),[WaveError::NodeFailed(NodeId(1),_)]));
        assert_eq!(*e.store.get::<i64>("counter").unwrap().read().unwrap(),8);
        let b=e.fire(1,vec![enc(3_i64)]).unwrap();
        assert!(b.success());assert_eq!(b.published[0].value.get::<i64>().unwrap(),11);
        // This test documents existing semantics, NOT transaction guarantees.
    }

    #[test]
    fn panic_while_holding_store_write_lock_poisoning_is_reported_in_later_waves() {
        let mut g = FlatGraphBuilder::default();
        g.add_node(node(1, vec![all(0, vec![slot(0, I, BufferMode::Immediate)])],
            vec![], CohortMode::Inherit,
            vec![ResourceNeed { key: "counter", access: Access::Write }], |ctx| {
                let value = ctx.arg::<i64>(0)?;
                if value == 0 {
                    ctx.write::<i64, _>("counter", |_counter| panic!("panic with write lock held"))?;
                } else {
                    ctx.write::<i64, _>("counter", |counter| *counter += value)?;
                }
                Ok(vec![])
            }));
        g.trigger(1, vec![input(1, 0, 0)]);
        let mut store = Store::default();
        store.insert("counter", 0_i64);
        let engine = Deployment::new(g.build().unwrap()).store(store).report_retention(ReportRetention::RetainPayloads).build().unwrap();

        let panicked = engine.fire(1, vec![enc(0_i64)]).unwrap();
        assert!(matches!(panicked.errors.as_slice(), [WaveError::NodeFailed(NodeId(1), message)]
            if message == "node panicked"));
        let poisoned = engine.fire(1, vec![enc(1_i64)]).unwrap();
        assert!(matches!(poisoned.errors.as_slice(), [WaveError::NodeFailed(NodeId(1), message)]
            if message.contains("poisoned")));
    }

    #[test]
    fn independent_trigger_waves_have_monotonic_ids_and_no_output_carryover() {
        let e=identity_graph();
        for i in 1..=8_u64 {
            let label=format!("value-{i}");
            let r=e.fire(1,vec![enc(label.clone())]).unwrap();
            assert!(r.success());assert_eq!(r.wave,WaveId(i));
            assert_eq!(r.count(1),1);assert_eq!(r.published.len(),1);
            assert_eq!(val(&r,1),label);
        }
    }

    #[test]
    fn immediate_slot_rejects_second_later_arrival_without_losing_first() {
        let graph=single_with_policy(AllSameCohort,BufferMode::Immediate);
        let mut st=WaveState::new(&graph,WaveId(1));
        assert!(st.deliver_batch(vec![Incoming{
            to:input(1,0,0),cohort:CohortId(9),value:enc("first".to_string())
        }]));
        assert_eq!(st.buffers[&input(1,0,0)].len(),1);
        assert!(!st.deliver_batch(vec![Incoming{
            to:input(1,0,0),cohort:CohortId(9),value:enc("second".to_string())
        }]));
        assert_eq!(st.buffers[&input(1,0,0)].len(),1);
        assert_eq!(st.buffers[&input(1,0,0)][0].value.get::<String>().unwrap(),"first");
        assert_eq!(st.report.errors,vec![WaveError::SlotOccupied(input(1,0,0))]);
    }
    #[test]
    fn deferred_slot_keeps_interleaved_cohorts_and_fifo_order() {
        let graph=single_with_policy(AllSameCohort,BufferMode::Deferred);
        let mut st=WaveState::new(&graph,WaveId(1));
        let incoming=|s:u16,c:u64,text:&str|Incoming{
            to:input(1,0,s),cohort:CohortId(c),value:enc(text.to_string())
        };
        assert!(st.deliver_batch(vec![incoming(0,1,"a1"),incoming(0,2,"a2"),incoming(0,1,"a3")]));
        assert_eq!(st.ready.len(),0);
        assert!(st.deliver_batch(vec![incoming(1,2,"b2"),incoming(1,1,"b1"),incoming(1,1,"b3")]));
        assert_eq!(st.ready.len(),3);
        let pairs=st.ready.iter().map(|r|(
            r.cohort.0,
            r.args.raw()[&SlotId(0)].get::<String>().unwrap(),
            r.args.raw()[&SlotId(1)].get::<String>().unwrap(),
        )).collect::<Vec<_>>();
        assert_eq!(pairs,vec![
            (1,"a1".into(),"b1".into()),
            (2,"a2".into(),"b2".into()),
            (1,"a3".into(),"b3".into()),
        ]);
        assert!(st.buffers.values().all(|b|b.is_empty()));
    }


    #[test]
    fn integration_coloring_staged_policy_fanout_cohorts_and_recovery() {
        // A.out0/out1 -> D.slot0/slot1
        // A.out2 -> C -> D.slot2
        // D.out -> E (shared downstream consumer).
        // One mixed wave intentionally fails only the "skip" cohort; a later
        // independent wave succeeds despite reusing every physical node.
        let mut g=FlatGraphBuilder::default();
        g.add_node(node(1,vec![all(0,vec![slot(0,S,BufferMode::Deferred)])],
            vec![out(0,S),out(1,S),out(2,S)],CohortMode::Inherit,vec![],|ctx|{
                let label=ctx.arg::<String>(0)?;
                Ok(vec![
                    emit(0,format!("left:{label}")),
                    emit(1,format!("right:{label}")),
                    emit(2,label),
                ])
            }));
        g.add_node(node(2,vec![all(0,vec![slot(0,S,BufferMode::Deferred)])],
            vec![out(0,S)],CohortMode::Inherit,vec![],|ctx|{
                let label=ctx.arg::<String>(0)?;
                Ok(if label=="skip" {vec![]}else{vec![emit(0,"third".to_string())]})
            }));
        g.add_node(node(3,vec![custom_policy(0,vec![
                slot(0,S,BufferMode::Deferred),slot(1,S,BufferMode::Deferred),
                slot(2,S,BufferMode::Deferred),
            ],WhenUpstreamSettles::new(PairThenThird)).args(decode_pair_stage)],
            vec![out(0,S)],CohortMode::Inherit,vec![],|ctx|{
                match ctx.prepared::<PairStageArgs>()? {
                    PairStageArgs::Pair{left,right}=>Ok(vec![emit(0,format!("pair({left},{right})"))]),
                    PairStageArgs::Third{value}=>{
                        assert_eq!(value,"third");
                        Ok(vec![])
                    }
                }
            }));
        g.add_node(node(4,vec![all(0,vec![slot(0,S,BufferMode::Deferred)])],
            vec![out(0,S)],CohortMode::Inherit,vec![],|ctx|
                Ok(vec![emit(0,format!("down({})",ctx.arg::<String>(0)?))])));
        g.connect(output(1,0),input(3,0,0));
        g.connect(output(1,1),input(3,0,1));
        g.connect(output(1,2),input(2,0,0));
        g.connect(output(2,0),input(3,0,2));
        g.connect(output(3,0),input(4,0,0));
        g.trigger(1,vec![input(1,0,0)]);
        let e=Deployment::new(g.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let mixed=e.run(vec![
            Incoming{to:input(1,0,0),cohort:CohortId(10),
                value:enc("skip".to_string())},
            Incoming{to:input(1,0,0),cohort:CohortId(20),
                value:enc("good".to_string())},
        ]).unwrap();
        assert!(!mixed.success());
        assert_eq!((mixed.count(1),mixed.count(2),mixed.count(3),mixed.count(4)),(2,2,3,2));
        assert_eq!(mixed.errors.len(),1,"{:?}",mixed.errors);
        assert!(matches!(&mixed.errors[0],WaveError::Incomplete{node:NodeId(3),group,..}
            if group.cohort==CohortId(10)&&group.missing==vec![SlotId(2)]));
        let outputs:BTreeMap<_,_>=mixed.published.iter().filter(|p|p.from==output(4,0))
            .map(|p|(p.cohort,p.value.get::<String>().unwrap())).collect();
        assert_eq!(outputs[&CohortId(10)],"down(pair(left:skip,right:skip))");
        assert_eq!(outputs[&CohortId(20)],"down(pair(left:good,right:good))");
        assert!(!mixed.trace.waits.is_empty(),"upstream coloring should have been exercised");
        let next=e.fire(1,vec![enc("next".to_string())]).unwrap();
        assert!(next.success(),"{:?}",next.errors);
        assert_eq!((next.count(1),next.count(2),next.count(3),next.count(4)),(1,1,2,1));
        assert_eq!(val(&next,4),"down(pair(left:next,right:next))");
        assert_ne!(mixed.wave,next.wave);
    }

}
#[cfg(test)]
mod partitioned_active_coloring_conformance {
    use super::*;

    const S: &str = <String as ContractValue>::SCHEMA;
    const I: &str = <i64 as ContractValue>::SCHEMA;

    fn component(mut template: NodeTemplate, name: &str) -> NodeTemplate {
        template.id = NodeId(1);
        template.name = Some(name.to_owned());
        template
    }

    fn single_node_module(
        id: &str,
        template: NodeTemplate,
        inputs: Vec<(u16, &'static str, &'static str, InputAddr)>,
        outputs: Vec<(u16, &'static str, &'static str, OutputAddr)>,
    ) -> ModuleEntry {
        let mut module = ModuleDefBuilder::new(id);
        module.add(template).unwrap();
        for (port, name, schema, target) in inputs {
            module.input(port, name, schema, vec![ModuleInputTarget::Node(target)]);
        }
        for (port, name, schema, source) in outputs {
            module.output(port, name, schema, ModuleOutputSource::Node(source));
        }
        module.build()
    }

    fn compile(definitions: Vec<ModuleEntry>, root: &str) -> Arc<Graph> {
        let mut catalog = ModuleCatalog::new();
        for definition in definitions { catalog.add(definition).unwrap(); }
        catalog.compile(root).unwrap()
    }

    fn delayed_nodes(fresh_gate: bool) -> Vec<NodeTemplate> {
        let mut gate = node(5, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                let value = ctx.arg::<String>(0)?;
                match value.as_str() {
                    "delay(skip)" => Ok(vec![]),
                    "delay(fail)" => Err("slow branch failed".into()),
                    _ => Ok(vec![emit(0, format!("slow({value})"))]),
                }
            });
        if fresh_gate { gate = gate.with_fresh_cohort(); }
        vec![
            node(1, vec![all(0, vec![slot(0, S, BufferMode::Deferred)])],
                vec![out(0, S), out(1, S), out(2, S)], CohortMode::Inherit, vec![], |ctx| {
                    let value = ctx.arg::<String>(0)?;
                    Ok(vec![emit(0, value.clone()), emit(1, value.clone()), emit(2, value)])
                }),
            node(2, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
                vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                    Ok(vec![emit(0, format!("fast({})", ctx.arg::<String>(0)?))])
                }),
            node(3, vec![all(0, vec![
                slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred),
            ])], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, format!("pair({},{})",
                    ctx.arg::<String>(0)?, ctx.arg::<String>(1)?))])
            }),
            node(4, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
                vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                    Ok(vec![emit(0, format!("delay({})", ctx.arg::<String>(0)?))])
                }),
            gate,
            node(6, vec![custom_policy(0, vec![
                slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred),
            ], WhenUpstreamSettles::new(PreferPairOnce)).args(decode_chosen)],
                vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                    Ok(vec![emit(0, format!("choose({})",
                        ctx.prepared::<ChosenArgs>()?.parts.join(",")))])
                }),
        ]
    }

    fn delayed_graph(partitioned: bool, fresh_gate: bool) -> Arc<Graph> {
        let nodes = delayed_nodes(fresh_gate);
        if !partitioned {
            let mut root = ModuleDefBuilder::new("delayed-root");
            for (node, name) in nodes.into_iter().zip(["source", "fast", "join", "delay", "gate", "choose"]) {
                root.add_named(name, node).unwrap();
            }
            for (from, to) in [
                (output(1, 0), input(2, 0, 0)),
                (output(1, 1), input(3, 0, 0)),
                (output(1, 2), input(4, 0, 0)),
                (output(4, 0), input(5, 0, 0)),
                (output(5, 0), input(3, 0, 1)),
                (output(3, 0), input(6, 0, 0)),
                (output(2, 0), input(6, 0, 1)),
            ] { root.connect(ModuleOutputSource::Node(from), ModuleInputTarget::Node(to)); }
            root.input(0, "request", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
            root.output(0, "result", S, ModuleOutputSource::Node(output(6, 0)));
            root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
            return compile(vec![root.build()], "delayed-root");
        }

        let nodes: Vec<_> = nodes.into_iter().zip(["source", "fast", "join", "delay", "gate", "choose"])
            .map(|(node, name)| component(node, name)).collect();
        let definitions = vec![
            single_node_module("source-def", nodes[0].clone(),
                vec![(0, "request", S, input(1, 0, 0))],
                vec![(0, "fast", S, output(1, 0)), (1, "left", S, output(1, 1)),
                    (2, "slow", S, output(1, 2))]),
            single_node_module("fast-def", nodes[1].clone(),
                vec![(0, "request", S, input(1, 0, 0))], vec![(0, "value", S, output(1, 0))]),
            single_node_module("join-def", nodes[2].clone(),
                vec![(0, "left", S, input(1, 0, 0)), (1, "right", S, input(1, 0, 1))],
                vec![(0, "value", S, output(1, 0))]),
            single_node_module("delay-def", nodes[3].clone(),
                vec![(0, "request", S, input(1, 0, 0))], vec![(0, "value", S, output(1, 0))]),
            single_node_module("gate-def", nodes[4].clone(),
                vec![(0, "request", S, input(1, 0, 0))], vec![(0, "value", S, output(1, 0))]),
            single_node_module("choose-def", nodes[5].clone(),
                vec![(0, "slow", S, input(1, 0, 0)), (1, "fast", S, input(1, 0, 1))],
                vec![(0, "result", S, output(1, 0))]),
        ];
        let mut pipeline = ModuleDefBuilder::new("delayed-pipeline");
        for (name, definition) in [
            ("fast", "fast-def"), ("join", "join-def"), ("delay", "delay-def"),
            ("gate", "gate-def"), ("choose", "choose-def"),
        ] { pipeline.try_instance(name, definition).unwrap(); }
        for (port, name, target) in [
            (0, "fast", ("fast", 0)), (1, "join-left", ("join", 0)),
            (2, "slow", ("delay", 0)),
        ] {
            pipeline.input(port, name, S, vec![ModuleInputTarget::Instance {
                name: target.0.into(), port: target.1,
            }]);
        }
        pipeline.output(0, "result", S, ModuleOutputSource::Instance {
            name: "choose".into(), port: 0,
        });
        for (from_name, from_port, to_name, to_port) in [
            ("delay", 0, "gate", 0),
            ("gate", 0, "join", 1), ("join", 0, "choose", 0),
            ("fast", 0, "choose", 1),
        ] {
            pipeline.connect(ModuleOutputSource::Instance { name: from_name.into(), port: from_port },
                ModuleInputTarget::Instance { name: to_name.into(), port: to_port });
        }
        let mut root = ModuleDefBuilder::new("delayed-root");
        root.try_instance("source", "source-def").unwrap();
        root.try_instance("pipeline", "delayed-pipeline").unwrap();
        root.input(0, "request", S, vec![ModuleInputTarget::Instance {
            name: "source".into(), port: 0,
        }]);
        root.output(0, "result", S, ModuleOutputSource::Instance {
            name: "pipeline".into(), port: 0,
        });
        for (source_port, pipeline_port) in [(0, 0), (1, 1), (2, 2)] {
            root.connect(ModuleOutputSource::Instance { name: "source".into(), port: source_port },
                ModuleInputTarget::Instance { name: "pipeline".into(), port: pipeline_port });
        }
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let mut modules = definitions;
        modules.push(pipeline.build());
        modules.push(root.build());
        compile(modules, "delayed-root")
    }

    struct LatestOnce;
    impl PortPolicy for LatestOnce {
        fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
            for first in snapshot.slots.first()?.candidates() {
                if snapshot.reservations().iter().any(|reservation|
                    reservation.cohort_id() == first.cohort_id()) { continue; }
                let latest = snapshot.slots.get(1)?.candidates().iter()
                    .filter(|candidate| candidate.cohort_id() == first.cohort_id())
                    .max_by_key(|candidate| candidate.delivery_id())?;
                return Some(Selection::new(first.cohort_id(), vec![
                    Pick::new(first.slot_id(), first.delivery_id()),
                    Pick::new(latest.slot_id(), latest.delivery_id()),
                ]));
            }
            None
        }
        fn incomplete(&self, _snapshot: &PortSnapshot) -> Vec<IncompleteGroup> { vec![] }
        fn retire(&self, snapshot: &PortSnapshot, selection: &Selection) -> Vec<Pick> {
            snapshot.slots.iter().flat_map(|slot| slot.candidates.iter())
                .filter(|candidate| candidate.cohort == selection.cohort
                    && !selection.picks.iter().any(|pick|
                        pick.slot == candidate.slot && pick.delivery == candidate.delivery))
                .map(|candidate| Pick { slot: candidate.slot, delivery: candidate.delivery })
                .collect()
        }
    }

    fn feedback_nodes() -> Vec<NodeTemplate> {
        vec![
            node(1, vec![all(0, vec![slot(0, S, BufferMode::Deferred)])],
                vec![out(0, S), out(1, I)], CohortMode::Inherit, vec![], |ctx| {
                    Ok(vec![emit(0, ctx.arg::<String>(0)?), emit(1, 0_i64)])
                }),
            node(2, vec![all(0, vec![slot(0, I, BufferMode::Deferred)])],
                vec![out(0, I), out(1, I)], CohortMode::Inherit, vec![], |ctx| {
                    let current = ctx.arg::<i64>(0)?;
                    if current == 3 { return Ok(vec![]); }
                    let next = current + 1;
                    Ok(vec![emit(0, next), emit(1, next)])
                }),
            node(3, vec![Port::new(0, vec![
                slot(0, S, BufferMode::Deferred), slot(1, I, BufferMode::Deferred),
            ], WhenUpstreamSettles::new(LatestOnce))], vec![out(0, S)],
                CohortMode::Inherit, vec![], |ctx| {
                    Ok(vec![emit(0, format!("choose({}, {})",
                        ctx.arg::<String>(0)?, ctx.arg::<i64>(1)?))])
                }),
        ]
    }

    fn feedback_graph(partitioned: bool) -> Arc<Graph> {
        let nodes = feedback_nodes();
        if !partitioned {
            let mut root = ModuleDefBuilder::new("feedback-root");
            for (template, name) in nodes.into_iter().zip(["source", "loop", "choose"]) {
                root.add_named(name, template).unwrap();
            }
            for (from, to) in [
                (output(1, 0), input(3, 0, 0)), (output(1, 1), input(2, 0, 0)),
                (output(2, 0), input(2, 0, 0)), (output(2, 1), input(3, 0, 1)),
            ] { root.connect(ModuleOutputSource::Node(from), ModuleInputTarget::Node(to)); }
            root.input(0, "request", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
            root.output(0, "result", S, ModuleOutputSource::Node(output(3, 0)));
            root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
            return compile(vec![root.build()], "feedback-root");
        }

        let nodes: Vec<_> = nodes.into_iter().zip(["source", "loop", "choose"])
            .map(|(node, name)| component(node, name)).collect();
        let source = single_node_module("feedback-source", nodes[0].clone(),
            vec![(0, "request", S, input(1, 0, 0))],
            vec![(0, "source", S, output(1, 0)), (1, "seed", I, output(1, 1))]);
        let mut loop_module = ModuleDefBuilder::new("feedback-loop");
        loop_module.add(nodes[1].clone()).unwrap();
        loop_module.input(0, "seed", I, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        loop_module.output(0, "feedback", I, ModuleOutputSource::Node(output(1, 0)));
        loop_module.output(1, "latest", I, ModuleOutputSource::Node(output(1, 1)));
        loop_module.connect(ModuleOutputSource::Node(output(1, 0)), ModuleInputTarget::Node(input(1, 0, 0)));
        let choose = single_node_module("feedback-choose", nodes[2].clone(),
            vec![(0, "source", S, input(1, 0, 0)), (1, "latest", I, input(1, 0, 1))],
            vec![(0, "result", S, output(1, 0))]);
        let mut work = ModuleDefBuilder::new("feedback-work");
        work.try_instance("loop", "feedback-loop").unwrap();
        work.try_instance("choose", "feedback-choose").unwrap();
        work.input(0, "source", S, vec![ModuleInputTarget::Instance {
            name: "choose".into(), port: 0,
        }]);
        work.input(1, "seed", I, vec![ModuleInputTarget::Instance {
            name: "loop".into(), port: 0,
        }]);
        work.output(0, "result", S, ModuleOutputSource::Instance {
            name: "choose".into(), port: 0,
        });
        work.connect(ModuleOutputSource::Instance { name: "loop".into(), port: 1 },
            ModuleInputTarget::Instance { name: "choose".into(), port: 1 });
        let mut root = ModuleDefBuilder::new("feedback-root");
        root.try_instance("source", "feedback-source").unwrap();
        root.try_instance("work", "feedback-work").unwrap();
        root.input(0, "request", S, vec![ModuleInputTarget::Instance {
            name: "source".into(), port: 0,
        }]);
        root.output(0, "result", S, ModuleOutputSource::Instance {
            name: "work".into(), port: 0,
        });
        root.connect(ModuleOutputSource::Instance { name: "source".into(), port: 0 },
            ModuleInputTarget::Instance { name: "work".into(), port: 0 });
        root.connect(ModuleOutputSource::Instance { name: "source".into(), port: 1 },
            ModuleInputTarget::Instance { name: "work".into(), port: 1 });
        root.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        compile(vec![source, loop_module.build(), choose, work.build(), root.build()], "feedback-root")
    }

    fn logical_id(graph: &Graph, name: &str) -> NodeId {
        graph.nodes.iter().find_map(|(&id, node)| (node.name.as_deref() == Some(name)).then_some(id))
            .unwrap_or_else(|| panic!("missing logical node {name}"))
    }

    fn logical_name(graph: &Graph, node: NodeId) -> String {
        graph.nodes[&node].name.clone().unwrap_or_else(|| format!("node{}", node.0))
    }

    fn normalized_errors(graph: &Graph, report: &WaveReport) -> Vec<String> {
        let mut errors: Vec<_> = report.errors.iter().map(|error| match error {
            WaveError::NodeFailed(node, message) => format!("failed:{}:{message}", logical_name(graph, *node)),
            WaveError::Incomplete { node, port, group } => format!(
                "incomplete:{}:{}:{}:{:?}:{:?}:{}", logical_name(graph, *node), port.0,
                group.cohort_id(), group.present().collect::<Vec<_>>(),
                group.missing().collect::<Vec<_>>(), group.requirement()),
            other => format!("{other:?}"),
        }).collect();
        errors.sort();
        errors
    }

    fn normalized_publications(graph: &Graph, report: &WaveReport) -> Vec<(String, u16, u64, String)> {
        let mut publications: Vec<_> = report.published.iter().map(|publication| {
            let value = if publication.value.schema == S {
                publication.value.get::<String>().unwrap()
            } else if publication.value.schema == I {
                publication.value.get::<i64>().unwrap().to_string()
            } else {
                format!("schema:{}", publication.value.schema)
            };
            (logical_name(graph, publication.from.node), publication.from.slot.0,
                publication.cohort.0, value)
        }).collect();
        publications.sort();
        publications
    }

    fn normalized_invocations(graph: &Graph, report: &WaveReport) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for &node in &report.trace.invocations {
            *counts.entry(logical_name(graph, node)).or_insert(0) += 1;
        }
        counts
    }

    fn run_inputs(graph: &Arc<Graph>, values: &[(u64, &str)]) -> WaveReport {
        let source = logical_id(graph, "source");
        Deployment::new(Arc::clone(graph)).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap().run(values.iter().map(|(cohort, value)| Incoming {
            to: InputAddr { node: source, port: PortId(0), slot: SlotId(0) },
            cohort: CohortId(*cohort), value: enc((*value).to_owned()),
        }).collect()).unwrap()
    }

    struct CapturedWave {
        report: WaveReport,
        active_nodes: BTreeSet<NodeId>,
        active_partitions: BTreeMap<PartitionId, usize>,
        pending_inputs: BTreeMap<InputAddr, Vec<Value>>,
    }

    fn run_capturing_activity(engine: &Engine, incoming: Vec<Incoming>) -> CapturedWave {
        let wave = WaveId(engine.next_wave.fetch_add(1, Ordering::Relaxed));
        let next_cohort = incoming.iter().map(|delivery| delivery.cohort.0)
            .max().unwrap_or(0).checked_add(1).unwrap();
        let mut state = WaveState::new(&engine.graph, wave);
        state.next_cohort = next_cohort;
        assert!(state.deliver_batch(incoming));
        while let Some(invocation) = state.ready.pop_front() {
            state.report.trace.executed.push(invocation.node);
            state.report.trace.timeline.push(WaveEvent::Started(invocation.node));
            let node = &engine.graph.nodes[&invocation.node];
            let context = InvocationCtx {
                wave: invocation.wave, invocation: invocation.id, cohort: invocation.cohort,
                from_port: invocation.port, args: invocation.args,
                store: Arc::clone(&engine.store), grants: node.grants.clone(),
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
                engine.instances[&invocation.node].execute(context)))
                .unwrap_or_else(|_| Err("node panicked".to_owned()));
            state.publish(Completion { id: invocation.id, node: node.id,
                port: invocation.port, cohort: invocation.cohort, result });
            state.activity.finished(invocation.id, invocation.node,
                &engine.graph.structural().partitions);
            state.reevaluate_after_completion(invocation.node);
        }
        state.settle();
        let pending_inputs = state.buffers.iter().filter_map(|(address, queue)| {
            (!queue.is_empty()).then(|| (*address, queue.iter().map(|delivery| delivery.value.clone()).collect()))
        }).collect();
        CapturedWave {
            report: state.report,
            active_nodes: state.activity.nodes,
            active_partitions: state.activity.active_partitions,
            pending_inputs,
        }
    }

    fn assert_equivalent(flat: &Graph, flat_report: &WaveReport, partitioned: &Graph, partitioned_report: &WaveReport) {
        assert_eq!(normalized_publications(flat, flat_report), normalized_publications(partitioned, partitioned_report));
        assert_eq!(normalized_errors(flat, flat_report), normalized_errors(partitioned, partitioned_report));
        assert_eq!(normalized_invocations(flat, flat_report), normalized_invocations(partitioned, partitioned_report));
    }

    #[test]
    fn module_partitions_preserve_delayed_join_skip_failure_and_cohorts() {
        let flat = delayed_graph(false, false);
        let partitioned = delayed_graph(true, false);
        assert!(partitioned.structural().partitions.node_partition.values().copied()
            .collect::<BTreeSet<_>>().len() >= 6);

        for (cohort, value, expected) in [
            (10, "good", "choose(pair(good,slow(delay(good))),fast(good))"),
            (11, "skip", "choose(fast(skip))"),
            (12, "fail", "choose(fast(fail))"),
        ] {
            let flat_report = run_inputs(&flat, &[(cohort, value)]);
            let partitioned_report = run_inputs(&partitioned, &[(cohort, value)]);
            assert_equivalent(&flat, &flat_report, &partitioned, &partitioned_report);
            assert!(partitioned_report.published.iter().any(|publication|
                partitioned.nodes[&publication.from.node].name.as_deref() == Some("choose")
                    && publication.value.get::<String>().as_deref() == Ok(expected)
                    && publication.cohort == CohortId(cohort)));
            assert_eq!(partitioned_report.count(logical_id(&partitioned, "choose").0), 1);
            if value == "good" {
                assert!(partitioned_report.success(), "{:?}", partitioned_report.errors);
            } else if value == "skip" {
                assert!(matches!(partitioned_report.errors.as_slice(), [WaveError::Incomplete {
                    group, ..
                }] if group.cohort_id() == cohort && group.missing().collect::<Vec<_>>() == vec![1]));
            } else {
                assert_eq!(partitioned_report.errors.iter().filter(|error|
                    matches!(error, WaveError::NodeFailed(node, reason)
                        if *node == logical_id(&partitioned, "gate") && reason == "slow branch failed")).count(), 1);
                assert_eq!(partitioned_report.errors.iter().filter(|error|
                    matches!(error, WaveError::Incomplete { node, .. }
                        if *node == logical_id(&partitioned, "join"))).count(), 1);
            }
        }

        let mixed_flat = run_inputs(&flat, &[(20, "skip"), (30, "good")]);
        let mixed_partitioned = run_inputs(&partitioned, &[(20, "skip"), (30, "good")]);
        assert_equivalent(&flat, &mixed_flat, &partitioned, &mixed_partitioned);
        let chosen: BTreeMap<_, _> = mixed_partitioned.published.iter()
            .filter(|publication| partitioned.nodes[&publication.from.node].name.as_deref() == Some("choose"))
            .map(|publication| (publication.cohort, publication.value.get::<String>().unwrap()))
            .collect();
        assert_eq!(chosen[&CohortId(20)], "choose(fast(skip))");
        assert_eq!(chosen[&CohortId(30)], "choose(pair(good,slow(delay(good))),fast(good))");
        assert_eq!(mixed_partitioned.errors.len(), 1);

        let success_engine = Deployment::new(Arc::clone(&partitioned)).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let success_state = run_capturing_activity(&success_engine, vec![Incoming {
            to: input(logical_id(&partitioned, "source").0, 0, 0),
            cohort: CohortId(35), value: enc("good".to_owned()),
        }]);
        assert!(success_state.report.success());
        assert!(success_state.active_nodes.is_empty());
        assert!(success_state.active_partitions.is_empty());

        let skip_state = run_capturing_activity(&success_engine, vec![Incoming {
            to: input(logical_id(&partitioned, "source").0, 0, 0),
            cohort: CohortId(36), value: enc("skip".to_owned()),
        }]);
        assert_eq!(skip_state.active_nodes, BTreeSet::from([logical_id(&partitioned, "join")]));
        assert_eq!(skip_state.active_partitions.len(), 1);
        assert!(skip_state.pending_inputs.contains_key(&input(logical_id(&partitioned, "join").0, 0, 0)));
    }

    #[test]
    fn nested_module_gate_reset_does_not_cross_join_cohorts() {
        let flat = delayed_graph(false, true);
        let partitioned = delayed_graph(true, true);
        let flat_report = run_inputs(&flat, &[(40, "good")]);
        let partitioned_report = run_inputs(&partitioned, &[(40, "good")]);
        assert_equivalent(&flat, &flat_report, &partitioned, &partitioned_report);
        assert!(partitioned_report.published.iter().any(|publication|
            partitioned.nodes[&publication.from.node].name.as_deref() == Some("choose")
                && publication.value.get::<String>().as_deref() == Ok("choose(fast(good))")
                && publication.cohort == CohortId(40)));
        let incomplete: Vec<_> = partitioned_report.errors.iter().filter_map(|error| match error {
            WaveError::Incomplete { node, group, .. } if *node == logical_id(&partitioned, "join") =>
                Some((group.cohort_id(), group.present().collect::<Vec<_>>(), group.missing().collect::<Vec<_>>())),
            _ => None,
        }).collect();
        assert_eq!(incomplete, vec![(40, vec![0], vec![1]), (41, vec![1], vec![0])]);
    }

    #[test]
    fn nested_feedback_partition_matches_flat_latest_selection() {
        let flat = feedback_graph(false);
        let partitioned = feedback_graph(true);
        assert!(partitioned.structural().partitions.node_partition.values().copied()
            .collect::<BTreeSet<_>>().len() >= 3);
        let flat_report = run_inputs(&flat, &[(50, "source")]);
        let partitioned_engine = Deployment::new(Arc::clone(&partitioned)).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let incoming = || vec![Incoming {
            to: input(logical_id(&partitioned, "source").0, 0, 0),
            cohort: CohortId(50), value: enc("source".to_owned()),
        }];
        let first_wave = run_capturing_activity(&partitioned_engine, incoming());
        let partitioned_report = &first_wave.report;
        assert_equivalent(&flat, &flat_report, &partitioned, &partitioned_report);
        assert!(partitioned_report.success(), "{:?}", partitioned_report.errors);
        assert_eq!(partitioned_report.count(logical_id(&partitioned, "loop").0), 4);
        assert_eq!(partitioned_report.count(logical_id(&partitioned, "choose").0), 1);
        assert!(partitioned_report.published.iter().any(|publication|
            partitioned.nodes[&publication.from.node].name.as_deref() == Some("choose")
                && publication.value.get::<String>().as_deref() == Ok("choose(source, 3)")
                && publication.cohort == CohortId(50)));
        assert!(partitioned_report.trace.waits.iter().any(|wait|
            wait.node == logical_id(&partitioned, "choose")
                && wait.upstream.contains(&logical_id(&partitioned, "loop"))));
        let chooser_input = input(logical_id(&partitioned, "choose").0, 0, 1);
        assert!(!first_wave.pending_inputs.contains_key(&chooser_input),
            "the one-shot policy explicitly retires unselected alternatives");
        assert!(first_wave.active_nodes.is_empty());
        assert!(first_wave.active_partitions.is_empty());

        // A new wave owns fresh buffers and color facts after explicit retirement.
        let second_wave = run_capturing_activity(&partitioned_engine, incoming());
        assert_ne!(first_wave.report.wave_id(), second_wave.report.wave_id());
        assert!(second_wave.report.success());
        assert!(second_wave.pending_inputs.is_empty());
        assert!(second_wave.active_nodes.is_empty());
        assert!(second_wave.report.published.iter().any(|publication|
            partitioned.nodes[&publication.from.node].name.as_deref() == Some("choose")
                && publication.value.get::<String>().as_deref() == Ok("choose(source, 3)")
                && publication.cohort == CohortId(50)));
    }
}

// Consumer-oriented contract tests. The scenario deliberately uses only PUBLIC
// constructors, trait methods, and observation APIs (except private test setup).
// In particular, a consumer never names the runtime's erased node representation.
#[cfg(test)]
mod declarative_consumer_api_tests {
    use super::{
        BufferMode, Candidate, ContractValue, Emission, Engine, IncompleteGroup,
        InputSlot, InvocationCtx, ModuleCatalog, ModuleDefBuilder, ModuleInputTarget,
        ModuleOutputSource, ModuleTriggerTarget, NodeAction, NodeTemplate, OutputSlot, Pick,
        Port, PortPolicy, PortSnapshot, Selection, Store, Value,
        WaveError, WhenUpstreamSettles, emit, input, output,
    };
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

    const TEXT: &str = <String as ContractValue>::SCHEMA;

    // A normal user-defined behavior is an ordinary struct literal, not an
    // enum case registered with the core graph executor.
    struct FormatAction { prefix: String }
    struct PreparedText { value: String }

    impl NodeAction for FormatAction {
        fn execute(&self, ctx: InvocationCtx) -> Result<Vec<Emission>, String> {
            let value = &ctx.prepared::<PreparedText>()?.value;
            let count = ctx.write::<usize, _>("custom.counter", |count| {
                *count += 1;
                *count
            })?;
            Ok(vec![emit(0, format!("{}{}#{count}", self.prefix, value))])
        }
    }

    struct PrepareText;
    impl PortPolicy for PrepareText {
        fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
            let candidate = snapshot.slots().first()?.candidates().first()?;
            Some(Selection::new(candidate.cohort_id(), vec![
                Pick::new(candidate.slot_id(), candidate.delivery_id()),
            ]))
        }
        fn incomplete(&self, _snapshot: &PortSnapshot) -> Vec<IncompleteGroup> { vec![] }

    }

    fn compile_named(action: NodeTemplate) -> Arc<super::CompiledGraph> {
        let mut module = ModuleDefBuilder::new("custom-action");
        module.add_named("formatter", action).unwrap();
        module.input(0, "input", TEXT, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        module.output(0, "output", TEXT, ModuleOutputSource::Node(output(1, 0)));
        module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let mut catalog = ModuleCatalog::new();
        catalog.add(module.build()).unwrap();
        catalog.compile("custom-action").unwrap()
    }

    #[test]
    fn public_struct_literal_action_and_erased_policy_with_typed_args_and_store() {
        let policy: Arc<dyn PortPolicy> = Arc::new(PrepareText);
        let node = NodeTemplate::with_action(
            1,
            vec![Port::new(0,
                vec![InputSlot::deferred::<String>(0)],
                WhenUpstreamSettles { policy },
            ).args(|selected| {
                let value = selected.selected.values().next().ok_or("missing candidate")?.get::<String>()?;
                Ok(PreparedText { value: format!("{value}!") })
            })],
            vec![OutputSlot::typed::<String>(0)],
            FormatAction { prefix: "result:".to_owned() },
        ).writes("custom.counter");
        let graph = compile_named(node);
        let mut store = Store::new();
        store.insert("custom.counter", 0_usize);
        let engine = Deployment::new(graph).store(store).report_retention(ReportRetention::RetainPayloads).build().unwrap();

        let first = engine.fire(1, vec![Value::of("a".to_owned())]).unwrap();
        assert!(first.success(), "{:?}", first.error_messages());
        assert_eq!(first.publications().next().unwrap().value::<String>().unwrap(), "result:a!#1");
        let second = engine.fire(1, vec![Value::of("b".to_owned())]).unwrap();
        assert_eq!(second.publications().next().unwrap().value::<String>().unwrap(), "result:b!#2");
        assert_eq!(second.error_count(), 0);
    }

    struct StatefulAction { sequence: AtomicUsize }
    impl NodeAction for StatefulAction {
        fn execute(&self, ctx: InvocationCtx) -> Result<Vec<Emission>, String> {
            let value = ctx.arg::<String>(0)?;
            let n = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(vec![emit(0, format!("{n}:{value}"))])
        }
    }

    #[test]
    fn per_engine_factory_creates_separate_struct_instances() {
        let graph = compile_named(NodeTemplate::with_action_factory(
            1, vec![Port::all(0, vec![InputSlot::new(0, TEXT, BufferMode::Deferred)])],
            vec![OutputSlot::new(0, TEXT)],
            || StatefulAction { sequence: AtomicUsize::new(0) },
        ));
        let first = Deployment::new(Arc::clone(&graph)).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let second = Deployment::new(graph).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let call = |e: &Engine| {
            e.fire(1, vec![Value::of("x".to_owned())]).unwrap()
                .publications().next().unwrap().value::<String>().unwrap()
        };
        assert_eq!(call(&first), "1:x");
        assert_eq!(call(&first), "2:x");
        assert_eq!(call(&second), "1:x");
    }

    #[test]
    fn action_without_resource_grant_does_not_gain_access() {
        let node = NodeTemplate::with_action(
            1,
            vec![Port::new(0, vec![InputSlot::new(0, TEXT, BufferMode::Deferred)],
                PrepareText).args(|selected| {
                    let value = selected.selected.values().next().ok_or("missing candidate")?.get::<String>()?;
                    Ok(PreparedText { value })
                })],
            vec![OutputSlot::new(0, TEXT)],
            FormatAction { prefix: "denied:".to_owned() },
        );
        let mut store = Store::new();
        store.insert("custom.counter", 0_usize);
        let report = Deployment::new(compile_named(node)).store(store).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![Value::of("x".to_owned())]).unwrap();
        assert_eq!(report.publications().count(), 0);
        assert!(matches!(report.errors(), [WaveError::NodeFailed(_, message)]
            if message.contains("not granted")));
    }

    #[test]
    fn dynamically_erased_action_is_accepted_directly() {
        let action: Arc<dyn NodeAction> = Arc::new(|_ctx: InvocationCtx| Ok(vec![emit(0, ())]));
        let node = NodeTemplate::with_dyn_action(
            1,
            vec![Port::any(0, vec![InputSlot::deferred::<String>(0)])],
            vec![OutputSlot::typed::<()>(0)], action,
        );
        let mut module = ModuleDefBuilder::new("dyn-action");
        module.add_named("skip", node).unwrap();
        module.input(0, "start", TEXT, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let mut catalog = ModuleCatalog::new();
        catalog.add(module.build()).unwrap();
        let result = Deployment::new(catalog.compile("dyn-action").unwrap()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![Value::of("x".to_owned())]).unwrap();
        assert!(result.success());
        assert_eq!(result.invocation_count(1), 1);
    }

    #[test]
    fn unit_action_is_ordinary_node_action_without_a_preset() {
        let node = NodeTemplate::with_action(
            1, vec![Port::any(0, vec![InputSlot::new(0, TEXT, BufferMode::Deferred)])],
            vec![OutputSlot::typed::<()>(0)], |_ctx| Ok(vec![emit(0, ())]),
        );
        let mut module = ModuleDefBuilder::new("skip");
        module.add(node.named("skip").unwrap()).unwrap();
        module.input(0, "start", TEXT, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let mut catalog = ModuleCatalog::new();
        catalog.add(module.build()).unwrap();
        let engine = Deployment::new(catalog.compile("skip").unwrap()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
        let result = engine.fire(1, vec![Value::of("anything".to_owned())]).unwrap();
        assert!(result.success());
        assert_eq!(result.invocation_count(1), 1);
        assert_eq!(result.publications().count(), 1);
        assert_eq!(result.publications().next().unwrap().value::<()>(), Ok(()));
    }

    struct InvalidPolicy;
    impl PortPolicy for InvalidPolicy {
        fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
            let candidate: &Candidate = snapshot.slots().first()?.candidates().first()?;
            Some(Selection::new(candidate.cohort_id(), vec![Pick::new(99, candidate.delivery_id())]))
        }
        fn incomplete(&self, _snapshot: &PortSnapshot) -> Vec<IncompleteGroup> {
            vec![IncompleteGroup::new(0, vec![0], vec![1], "bad policy should not settle again")]
        }
    }
    #[test]
    fn invalid_policy_has_only_one_terminal_error_at_settlement() {
        let node = NodeTemplate::with_action(
            1, vec![Port::new(0, vec![InputSlot::new(0, TEXT, BufferMode::Deferred)], InvalidPolicy)],
            vec![OutputSlot::typed::<()>(0)], |_ctx| Ok(vec![emit(0, ())]),
        );
        let mut module = ModuleDefBuilder::new("invalid");
        module.add_named("fail", node).unwrap();
        module.input(0, "start", TEXT, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let mut catalog = ModuleCatalog::new();
        catalog.add(module.build()).unwrap();
        let result = Deployment::new(catalog.compile("invalid").unwrap()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(1, vec![Value::of("x".to_owned())]).unwrap();
        assert!(matches!(result.errors(), [WaveError::InvalidSelection(_, _)]));
    }
}

// Graph-only regression fixture: exercises nested modules, cycles and Store.
// Not a public product interface or part of the graph API surface.
#[cfg(test)]
mod echo_agent_poc {
    use super::*;

    const TEXT: &str = <String as ContractValue>::SCHEMA;
    const STATE: &str = "echo-agent.transcript";

    #[derive(Default, Debug)]
    struct Transcript {
        messages: Vec<(&'static str, String)>,
    }

    fn text_port() -> Vec<Port> {
        vec![Port::all(0, vec![InputSlot::new(0, TEXT, BufferMode::Deferred)])]
    }

    // Replace this module with a real provider without rewriting AgentLoop.
    fn echo_model() -> ModuleEntry {
        let mut m = ModuleDefBuilder::new("echo-model");
        let model = node(
            1, text_port(), vec![out(0, TEXT), out(1, TEXT)],
            CohortMode::Inherit,
            vec![ResourceNeed { key: STATE, access: Access::Read }],
            |ctx| {
                let message = ctx.arg::<String>(0)?;
                // A model can emit either a final response or a tool request.
                // After the tool result has been recorded, the same model emits Final.
                let should_call_tool = message.starts_with("/tool ")
                    && ctx.read::<Transcript, _>(STATE, |state| {
                        state.messages.last().is_some_and(|(role, _)| *role == "user")
                    })?;
                Ok(vec![emit(if should_call_tool { 1 } else { 0 }, message)])
            },
        );
        m.add_named("model", model).unwrap();
        m.input(0, "prompt", TEXT, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        m.output(0, "final", TEXT, ModuleOutputSource::Node(output(1, 0)));
        m.output(1, "tool_call", TEXT, ModuleOutputSource::Node(output(1, 1)));
        m.build()
    }

    fn echo_tool() -> ModuleEntry {
        let mut m = ModuleDefBuilder::new("echo-tool");
        m.add_named("echo", NodeTemplate::function(|request: String| format!("tool-ok:{request}"))).unwrap();
        m.input(0, "call", TEXT, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        m.output(0, "result", TEXT, ModuleOutputSource::Node(output(1, 0)));
        m.build()
    }

    fn agent_loop() -> ModuleEntry {
        let mut m = ModuleDefBuilder::new("agent-loop");

        // Node 1: persist the user turn, then send the message to the model.
        m.add_named("accept", node(
            1, text_port(), vec![out(0, TEXT)], CohortMode::Inherit,
            vec![ResourceNeed { key: STATE, access: Access::Write }],
            |ctx| {
                let message = ctx.arg::<String>(0)?;
                ctx.write::<Transcript, _>(STATE, |state| {
                    state.messages.push(("user", message.clone()));
                })?;
                Ok(vec![emit(0, message)])
            },
        )).unwrap();

        // Node 2: convert the tool response into a model continuation.
        m.add_named("record_tool", node(
            2, text_port(), vec![out(0, TEXT)], CohortMode::Inherit,
            vec![ResourceNeed { key: STATE, access: Access::Write }],
            |ctx| {
                let result = ctx.arg::<String>(0)?;
                let original = result.strip_prefix("tool-ok:")
                    .ok_or("malformed echo-tool response")?.to_owned();
                ctx.write::<Transcript, _>(STATE, |state| {
                    state.messages.push(("tool", result));
                })?;
                Ok(vec![emit(0, original)])
            },
        )).unwrap();

        // Node 3: persist only the final assistant answer, then publish it.
        m.add_named("finish", node(
            3, text_port(), vec![out(0, TEXT)], CohortMode::Inherit,
            vec![ResourceNeed { key: STATE, access: Access::Write }],
            |ctx| {
                let reply = ctx.arg::<String>(0)?;
                ctx.write::<Transcript, _>(STATE, |state| {
                    state.messages.push(("assistant", reply.clone()));
                })?;
                Ok(vec![emit(0, reply)])
            },
        )).unwrap();

        m.try_instance("model", "echo-model").unwrap();
        m.try_instance("tool", "echo-tool").unwrap();

        use ModuleInputTarget::{Instance as ToInstance, Node as ToNode};
        use ModuleOutputSource::{Instance as FromInstance, Node as FromNode};

        // The entire agent loop is ordinary typed connections; no Loop node kind.
        // Local Node addresses coerce automatically; module boundaries are explicit.
        m.connect(output(1, 0), ToInstance { name: "model".into(), port: 0 });
        m.connect(FromInstance { name: "model".into(), port: 1 },
                  ToInstance { name: "tool".into(), port: 0 });
        m.connect(FromInstance { name: "tool".into(), port: 0 }, input(2, 0, 0));
        m.connect(output(2, 0), ToInstance { name: "model".into(), port: 0 }); // back edge
        m.connect(FromInstance { name: "model".into(), port: 0 }, input(3, 0, 0));

        m.input(0, "message", TEXT, vec![ToNode(input(1, 0, 0))]);
        m.output(0, "reply", TEXT, FromNode(output(3, 0)));
        m.build()
    }

    fn echo_server() -> ModuleEntry {
        let mut server = ModuleDefBuilder::new("echo-server");
        server.try_instance("agent", "agent-loop").unwrap();
        server.input(0, "message", TEXT, vec![ModuleInputTarget::Instance {
            name: "agent".into(), port: 0,
        }]);
        server.output(0, "reply", TEXT, ModuleOutputSource::Instance {
            name: "agent".into(), port: 0,
        });
        server.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        server.build()
    }

    fn build_graph() -> Arc<CompiledGraph> {
        let mut catalog = ModuleCatalog::new();
        catalog.add(echo_model()).unwrap();
        catalog.add(echo_tool()).unwrap();
        catalog.add(agent_loop()).unwrap();
        catalog.add(echo_server()).unwrap();
        catalog.compile("echo-server").expect("valid echo-agent graph")
    }

    fn new_engine() -> Engine {
        let mut store = Store::new();
        store.insert(STATE, Transcript::default());
        Deployment::new(build_graph()).store(store).report_retention(ReportRetention::RetainPayloads).build().unwrap()
    }

    fn reply(engine: &Engine, input_message: &str) -> Result<String, String> {
        let final_addr = engine.graph.module_output(0).ok_or("no root output")?;
        let report = engine.fire(1, vec![Value::of(input_message.to_owned())])?;
        if !report.success() { return Err(report.error_messages().join("; ")); }
        report.publications()
            .find(|publication| publication.source_node_id() == final_addr.node.0
                && publication.source_slot_id() == final_addr.slot.0)
            .ok_or("agent failed to produce a final answer")?
            .value::<String>()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn normal_echo_turns_publish_identical_messages() {
            let engine = new_engine();
            assert_eq!(engine.graph.physical_node_count(), 5);
            assert!(matches!(engine.graph.find("agent/model"), Some(FindResult::Module(_))));
            for message in ["hello", "", "second message", "!", "/tool-less"] {
                assert_eq!(reply(&engine, message).unwrap(), message);
            }
            let roles = engine.store.get::<Transcript>(STATE).unwrap()
                .read().unwrap().messages.iter().map(|(role, _)| *role).collect::<Vec<_>>();
            assert_eq!(roles, ["user", "assistant", "user", "assistant",
                               "user", "assistant", "user", "assistant", "user", "assistant"]);
        }

        #[test]
        fn tool_request_loops_once_and_preserves_echo_contract() {
            let engine = new_engine();
            assert_eq!(reply(&engine, "/tool hello").unwrap(), "/tool hello");
            let transcript = engine.store.get::<Transcript>(STATE).unwrap().read().unwrap();
            assert_eq!(transcript.messages, [
                ("user", "/tool hello".to_string()),
                ("tool", "tool-ok:/tool hello".to_string()),
                ("assistant", "/tool hello".to_string()),
            ]);
            drop(transcript);
            // No tool-state leakage into subsequent turns.
            assert_eq!(reply(&engine, "ordinary").unwrap(), "ordinary");
            assert_eq!(reply(&engine, "/tool again").unwrap(), "/tool again");
            let transcript = engine.store.get::<Transcript>(STATE).unwrap().read().unwrap();
            assert_eq!(transcript.messages.iter().filter(|(role, _)| *role == "tool").count(), 2);
        }

        #[test]
        fn absent_resource_fails_without_publishing_a_final_reply() {
            let engine = Deployment::new(build_graph()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
            let report = engine.fire(1, vec![Value::of("hello".to_owned())]).unwrap();
            assert!(!report.success());
            assert!(report.publications().all(|publication|
                engine.graph.module_output(0).is_some_and(|addr|
                    publication.source_node_id() != addr.node.0 || publication.source_slot_id() != addr.slot.0)));
        }
    }
}

/// Optional wiring shorthand; expands only into the public builder API.
/// `wire!(module, source.value => sink.run.left)`
#[macro_export]
macro_rules! wire {
    ($graph:expr, $from:ident.$out:ident => $to:ident.$port:ident.$slot:ident) => {
        $graph.connect_named(stringify!($from), stringify!($out),
            stringify!($to), stringify!($port), stringify!($slot))
    };
    ($graph:expr, $from:ident.$out:ident => $to:ident.$public_input:ident) => {
        $graph.connect_public_named(stringify!($from), stringify!($out),
            stringify!($to), stringify!($public_input))
    };
}


#[cfg(test)]
mod sparse_activity_partition_tests {
    use super::*;

    const S: &str = <String as ContractValue>::SCHEMA;
    const I: &str = <i64 as ContractValue>::SCHEMA;

    fn published_string(report: &WaveReport, from: OutputAddr) -> String {
        report.published.iter().find(|publication| publication.from == from)
            .unwrap_or_else(|| panic!("no publication from {from:?}"))
            .value.get::<String>().unwrap()
    }

    fn pass(id: u16) -> NodeTemplate {
        node(id, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, ctx.arg::<String>(0)?)])
            })
    }
    fn pair(id: u16) -> NodeTemplate {
        node(id, vec![all(0, vec![
            slot(0, S, BufferMode::Deferred),
            slot(1, S, BufferMode::Deferred),
        ])], vec![], CohortMode::Inherit, vec![], |_| Ok(vec![]))
    }
    fn incoming(node: u16, slot: u16, value: &str) -> Incoming {
        Incoming {
            to: input(node, 0, slot), cohort: CohortId(4),
            value: enc(value.to_owned()),
        }
    }
    fn finish_no_outputs(state: &mut WaveState<'_>) {
        let job = state.ready.pop_front().expect("scheduled invocation");
        state.publish(Completion { id: job.id, node: job.node, port: job.port,
            cohort: job.cohort, result: Ok(vec![]) });
        state.activity.finished(job.id, job.node,
            &state.graph.structural().partitions);
        state.reevaluate_after_completion(job.node);
    }

    #[test]
    fn pending_then_executing_then_dormant_tracks_slots_not_history() {
        let mut builder = FlatGraphBuilder::default();
        builder.add_node(pair(1));
        let graph = builder.build().unwrap();
        let mut wave = WaveState::new(&graph, WaveId(1));
        let root = graph.structural().partitions.region(NodeId(1));
        assert_eq!(wave.activity.state(NodeId(1)), NodeState::Dormant);
        assert!(wave.activity.active_partitions.is_empty());

        assert!(wave.deliver_batch(vec![incoming(1, 0, "left")]));
        assert_eq!(wave.activity.state(NodeId(1)), NodeState::InputPending);
        assert_eq!(wave.activity.pending_ports[&(NodeId(1), PortId(0))], 1);
        assert_eq!(wave.activity.active_partitions[&root], 1);
        assert!(wave.ready.is_empty());

        assert!(wave.deliver_batch(vec![incoming(1, 1, "right")]));
        assert_eq!(wave.activity.state(NodeId(1)), NodeState::Executing);
        assert_eq!(wave.ready.len(), 1);
        assert!(wave.activity.pending_ports.is_empty());
        finish_no_outputs(&mut wave);
        assert_eq!(wave.activity.state(NodeId(1)), NodeState::Dormant);
        assert!(wave.activity.nodes.is_empty());
        assert!(wave.activity.active_partitions.is_empty());
        assert!(!wave.reservations.is_empty(), "history is not activity");
    }

    #[test]
    fn completion_with_more_partial_inputs_becomes_pending_then_reexecutes() {
        let mut builder = FlatGraphBuilder::default();
        builder.add_node(pair(1));
        let graph = builder.build().unwrap();
        let mut wave = WaveState::new(&graph, WaveId(1));
        assert!(wave.deliver_batch(vec![incoming(1, 0, "a"), incoming(1, 1, "b")]));
        assert_eq!(wave.ready.len(), 1);
        assert!(wave.deliver_batch(vec![incoming(1, 0, "next-a")]));
        assert_eq!(wave.activity.state(NodeId(1)), NodeState::Executing,
            "pending input and an admitted invocation coexist");
        finish_no_outputs(&mut wave);
        assert_eq!(wave.activity.state(NodeId(1)), NodeState::InputPending);
        assert_eq!(wave.activity.pending_ports[&(NodeId(1), PortId(0))], 1);
        assert!(wave.deliver_batch(vec![incoming(1, 1, "next-b")]));
        assert_eq!(wave.activity.state(NodeId(1)), NodeState::Executing);
        finish_no_outputs(&mut wave);
        assert_eq!(wave.activity.state(NodeId(1)), NodeState::Dormant);
        assert_eq!(wave.report.trace.invocations.len(), 2);
    }

    #[test]
    fn partial_input_is_not_an_open_producer_for_an_optional_choice() {
        let mut builder = FlatGraphBuilder::default();
        builder.add_node(pass(1));
        builder.add_node(node(2, vec![all(0, vec![
            slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred),
        ])], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
            Ok(vec![emit(0, format!("join({}, {})",
                ctx.arg::<String>(0)?, ctx.arg::<String>(1)?))])
        }));
        builder.add_node(node(3, vec![custom_policy(0, vec![
            slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred),
        ], WhenUpstreamSettles::new(PreferPairOnce)).args(decode_chosen)],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, format!("choose({})",
                    ctx.prepared::<ChosenArgs>()?.parts.join(",")))])
            }));
        builder.connect(output(1, 0), input(3, 0, 0));
        builder.connect(output(2, 0), input(3, 0, 1));
        let graph = builder.build().unwrap();

        let mut activity = WaveState::new(&graph, WaveId(1));
        assert_eq!(activity.activity.state(NodeId(2)), NodeState::Dormant);
        assert!(activity.deliver_batch(vec![
            Incoming { to: input(1, 0, 0), cohort: CohortId(7), value: enc("ready".to_owned()) },
            Incoming { to: input(2, 0, 0), cohort: CohortId(7), value: enc("partial".to_owned()) },
        ]));
        assert_eq!(activity.activity.state(NodeId(2)), NodeState::InputPending);
        assert_eq!(activity.activity.state(NodeId(1)), NodeState::Executing);
        assert!(!activity.activity.producers.values().any(|region|
            region.values().any(|producer| producer.node == NodeId(2))));

        let report = Deployment::new(graph).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap().run(vec![
            Incoming { to: input(1, 0, 0), cohort: CohortId(7), value: enc("ready".to_owned()) },
            Incoming { to: input(2, 0, 0), cohort: CohortId(7), value: enc("partial".to_owned()) },
        ]).unwrap();
        assert_eq!(report.count(2), 0);
        assert_eq!(report.count(3), 1);
        assert_eq!(published_string(&report, output(3, 0)), "choose(ready)");
        assert!(report.published.iter().any(|publication|
            publication.from == output(3, 0) && publication.cohort == CohortId(7)));
        assert!(matches!(report.errors.as_slice(), [WaveError::Incomplete {
            node: NodeId(2), group, ..
        }] if group.missing == vec![SlotId(1)]));
    }

    #[test]
    fn coloring_propagates_through_a_partially_ready_join() {
        let mut builder = FlatGraphBuilder::default();
        builder.add_node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S), out(1, S), out(2, S)], CohortMode::Inherit, vec![], |ctx| {
                let value = ctx.arg::<String>(0)?;
                Ok(vec![emit(0, value.clone()), emit(1, value.clone()), emit(2, value)])
            }));
        builder.add_node(node(2, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, format!("fast({})", ctx.arg::<String>(0)?))])
            }));
        builder.add_node(node(3, vec![all(0, vec![
            slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred),
        ])], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
            Ok(vec![emit(0, format!("join({}, {})",
                ctx.arg::<String>(0)?, ctx.arg::<String>(1)?))])
        }));
        builder.add_node(node(4, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, format!("delay1({})", ctx.arg::<String>(0)?))])
            }));
        builder.add_node(node(5, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, format!("delay2({})", ctx.arg::<String>(0)?))])
            }));
        builder.add_node(node(6, vec![custom_policy(0, vec![
            slot(0, S, BufferMode::Deferred), slot(1, S, BufferMode::Deferred),
        ], WhenUpstreamSettles::new(PreferPairOnce)).args(decode_chosen)],
            vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, format!("choose({})",
                    ctx.prepared::<ChosenArgs>()?.parts.join(",")))])
            }));
        builder.connect(output(1, 0), input(2, 0, 0));
        builder.connect(output(1, 1), input(3, 0, 0));
        builder.connect(output(1, 2), input(4, 0, 0));
        builder.connect(output(4, 0), input(5, 0, 0));
        builder.connect(output(5, 0), input(3, 0, 1));
        // Slow is slot 0; fast is slot 1.
        builder.connect(output(3, 0), input(6, 0, 0));
        builder.connect(output(2, 0), input(6, 0, 1));
        let graph = builder.build().unwrap();
        let report = Deployment::new(graph).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap().run(vec![Incoming {
            to: input(1, 0, 0), cohort: CohortId(9), value: enc("x".to_owned()),
        }]).unwrap();

        assert!(report.success(), "{:?}", report.errors);
        assert_eq!(report.count(3), 1);
        assert_eq!(report.count(6), 1);
        assert_eq!(published_string(&report, output(6, 0)),
            "choose(join(x, delay2(delay1(x))),fast(x))");
        assert!(report.published.iter().any(|publication|
            publication.from == output(6, 0) && publication.cohort == CohortId(9)));
        assert!(report.trace.waits.iter().any(|wait| wait.node == NodeId(6)
            && (wait.upstream.contains(&NodeId(4)) || wait.upstream.contains(&NodeId(5)))),
            "the admitted delay chain must keep the chooser open: {:?}", report.trace.waits);
        assert!(report.trace.executed.iter().position(|node| *node == NodeId(3))
            < report.trace.executed.iter().position(|node| *node == NodeId(6)));
    }

    struct LatestLoopValue;
    impl PortPolicy for LatestLoopValue {
        fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
            for first in snapshot.slots.first()?.candidates() {
                if snapshot.reservations().iter().any(|record|
                    record.cohort_id() == first.cohort_id()) { continue; }
                let latest = snapshot.slots.get(1)?.candidates().iter()
                    .filter(|candidate| candidate.cohort_id() == first.cohort_id())
                    .max_by_key(|candidate| candidate.delivery_id())?;
                return Some(Selection::new(first.cohort_id(), vec![
                    Pick::new(first.slot_id(), first.delivery_id()),
                    Pick::new(latest.slot_id(), latest.delivery_id()),
                ]));
            }
            None
        }
        fn incomplete(&self, _snapshot: &PortSnapshot) -> Vec<IncompleteGroup> { vec![] }
        fn retire(&self, snapshot: &PortSnapshot, selection: &Selection) -> Vec<Pick> {
            snapshot.slots.iter().flat_map(|slot| slot.candidates.iter())
                .filter(|candidate| candidate.cohort == selection.cohort
                    && !selection.picks.iter().any(|pick|
                        pick.slot == candidate.slot && pick.delivery == candidate.delivery))
                .map(|candidate| Pick { slot: candidate.slot, delivery: candidate.delivery })
                .collect()
        }
    }

    fn late_feedback_graph(coloring: bool) -> Engine {
        let mut builder = FlatGraphBuilder::default();
        builder.add_node(node(1, vec![all(0, vec![slot(0, S, BufferMode::Immediate)])],
            vec![out(0, S), out(1, I)], CohortMode::Inherit, vec![], |ctx| {
                Ok(vec![emit(0, ctx.arg::<String>(0)?), emit(1, 0_i64)])
            }));
        builder.add_node(node(2, vec![all(0, vec![slot(0, I, BufferMode::Deferred)])],
            vec![out(0, I), out(1, I)], CohortMode::Inherit, vec![], |ctx| {
                let current = ctx.arg::<i64>(0)?;
                if current == 3 { return Ok(vec![]); }
                let next = current + 1;
                Ok(vec![emit(0, next), emit(1, next)])
            }));
        let policy: Arc<dyn PortPolicy> = if coloring {
            Arc::new(WhenUpstreamSettles::new(LatestLoopValue))
        } else {
            Arc::new(LatestLoopValue)
        };
        builder.add_node(node(3, vec![Port::with_policy(0, vec![
            slot(0, S, BufferMode::Deferred), slot(1, I, BufferMode::Deferred),
        ], policy)], vec![out(0, S)], CohortMode::Inherit, vec![], |ctx| {
            Ok(vec![emit(0, format!("choose({}, {})",
                ctx.arg::<String>(0)?, ctx.arg::<i64>(1)?))])
        }));
        builder.connect(output(1, 0), input(3, 0, 0));
        builder.connect(output(1, 1), input(2, 0, 0));
        builder.connect(output(2, 0), input(2, 0, 0));
        builder.connect(output(2, 1), input(3, 0, 1));
        Deployment::new(builder.build().unwrap()).store(Store::default()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
    }

    #[test]
    fn finite_feedback_colors_wait_for_latest_loop_delivery() {
        let incoming = || vec![Incoming {
            to: input(1, 0, 0), cohort: CohortId(12), value: enc("source".to_owned()),
        }];
        let colored = late_feedback_graph(true).run(incoming()).unwrap();
        assert!(colored.success(), "{:?}", colored.errors);
        assert_eq!(colored.count(2), 4);
        assert_eq!(colored.count(3), 1);
        assert_eq!(published_string(&colored, output(3, 0)), "choose(source, 3)");
        assert!(colored.published.iter().any(|publication|
            publication.from == output(3, 0) && publication.cohort == CohortId(12)));
        assert!(colored.trace.waits.iter().any(|wait| wait.node == NodeId(3)
            && wait.upstream.contains(&NodeId(2))));

        // The no-coloring mutant selects the first loop output and then cannot
        // revise the one-shot policy reservation.
        let uncolored = late_feedback_graph(false).run(incoming()).unwrap();
        assert_eq!(uncolored.count(3), 1);
        assert_eq!(published_string(&uncolored, output(3, 0)), "choose(source, 1)");
        assert_ne!(published_string(&uncolored, output(3, 0)),
            published_string(&colored, output(3, 0)));
    }

    #[test]
    fn emitting_into_dormant_node_activates_its_pending_input() {
        let mut builder = FlatGraphBuilder::default();
        builder.add_node(pass(1));
        builder.add_node(pair(2));
        builder.connect(output(1, 0), input(2, 0, 0));
        let graph = builder.build().unwrap();
        let mut wave = WaveState::new(&graph, WaveId(1));
        assert!(wave.deliver_batch(vec![incoming(1, 0, "message")]));
        assert_eq!(wave.activity.state(NodeId(2)), NodeState::Dormant);
        let source = wave.ready.pop_front().unwrap();
        wave.publish(Completion { id: source.id, node: source.node, port: source.port,
            cohort: source.cohort, result: Ok(vec![emit(0, "message".to_owned())]) });
        assert_eq!(wave.activity.state(NodeId(2)), NodeState::InputPending);
        wave.activity.finished(source.id, source.node,
            &graph.structural().partitions);
        assert_eq!(wave.activity.state(NodeId(1)), NodeState::Dormant);
        assert_eq!(wave.activity.state(NodeId(2)), NodeState::InputPending);
    }

    #[test]
    fn completion_only_reevaluates_reachable_pending_ports() {
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
        struct CountChecks(Arc<AtomicUsize>);
        impl PortPolicy for CountChecks {
            fn select(&self, _snapshot: &PortSnapshot) -> Option<Selection> {
                self.0.fetch_add(1, AtomicOrdering::SeqCst);
                None
            }
            fn incomplete(&self, _snapshot: &PortSnapshot) -> Vec<IncompleteGroup> { vec![] }
        }
        let unrelated_checks = Arc::new(AtomicUsize::new(0));
        let mut builder = FlatGraphBuilder::default();
        builder.add_node(pass(1));
        builder.add_node(node(2, vec![Port::any_after_upstream(0,
            vec![slot(0, S, BufferMode::Deferred)])], vec![],
            CohortMode::Inherit, vec![], |_| Ok(vec![])));
        builder.add_node(node(3, vec![custom_policy(0,
            vec![slot(0, S, BufferMode::Deferred)],
            CountChecks(Arc::clone(&unrelated_checks)))], vec![],
            CohortMode::Inherit, vec![], |_| Ok(vec![])));
        builder.connect(output(1, 0), input(2, 0, 0));
        let graph = builder.build().unwrap();
        let mut wave = WaveState::new(&graph, WaveId(1));
        assert!(wave.deliver_batch(vec![incoming(1, 0, "source")]));
        assert!(wave.deliver_batch(vec![incoming(2, 0, "candidate"),
            incoming(3, 0, "unrelated")]));
        assert_eq!(wave.activity.state(NodeId(2)), NodeState::InputPending);
        assert_eq!(wave.activity.state(NodeId(3)), NodeState::InputPending);
        let checks_before = unrelated_checks.load(AtomicOrdering::SeqCst);
        assert!(checks_before > 0);
        let source = wave.ready.pop_front().unwrap();
        wave.publish(Completion { id: source.id, node: source.node,
            port: source.port, cohort: source.cohort, result: Ok(vec![]) });
        wave.activity.finished(source.id, source.node, &graph.structural().partitions);
        wave.reevaluate_after_completion(source.node);
        assert_eq!(wave.activity.state(NodeId(2)), NodeState::Executing,
            "target must wake when its source closes without emitting");
        assert_eq!(unrelated_checks.load(AtomicOrdering::SeqCst), checks_before,
            "unrelated pending port must not be reconsidered");
        assert_eq!(wave.activity.state(NodeId(3)), NodeState::InputPending);
    }

    #[test]
    fn module_partition_index_is_conservative_but_exact_reachability_is_not() {
        let mut inner = ModuleDefBuilder::new("inner");
        inner.node(pass(1));
        inner.input(0, "in", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        inner.output(0, "out", S, ModuleOutputSource::Node(output(1, 0)));
        let mut outer = ModuleDefBuilder::new("outer");
        outer.node(pass(1));
        outer.node(pass(2));
        outer.try_instance("child", "inner").unwrap();
        outer.connect(ModuleOutputSource::Node(output(1, 0)), ModuleInputTarget::Instance {
            name: "child".into(), port: 0,
        });
        outer.connect(ModuleOutputSource::Instance { name: "child".into(), port: 0 },
            ModuleInputTarget::Node(input(2, 0, 0)));
        outer.input(0, "in", S, vec![ModuleInputTarget::Node(input(1, 0, 0))]);
        outer.output(0, "out", S, ModuleOutputSource::Node(output(2, 0)));
        outer.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
        let mut catalog = ModuleCatalog::new();
        catalog.add(inner.build()).unwrap();
        catalog.add(outer.build()).unwrap();
        let graph = catalog.compile("outer").unwrap();
        let index = &graph.structural().partitions;
        assert_eq!(index.node_partition.values().copied().collect::<BTreeSet<_>>().len(), 2);
        let nodes: BTreeMap<_, _> = graph.metadata.nodes.iter()
            .map(|(id, p)| ((p.instance_path.clone(), p.local_node), *id)).collect();
        let start = nodes[&(vec![], NodeId(1))];
        let middle = nodes[&(vec!["child".to_owned()], NodeId(1))];
        let end = nodes[&(vec![], NodeId(2))];
        assert_ne!(index.region(start), index.region(middle));
        assert_eq!(index.region(start), index.region(end));
        assert!(index.possibly_reaches(end, start),
            "region quotient can be cyclic despite an acyclic physical graph");
        assert_eq!(graph.can_reach_port(end, (start, PortId(0))), (false, false),
            "exact reachability must reject the quotient false positive");
        assert_eq!(graph.can_reach_port(start, (end, PortId(0))), (true, false));

        let mut wave = WaveState::new(&graph, WaveId(1));
        assert!(wave.deliver_batch(vec![Incoming { to: InputAddr {node:start,port:PortId(0),slot:SlotId(0)},
            cohort:CohortId(0),value:enc("x".to_owned()) }]));
        assert_eq!(wave.activity.active_partitions.len(), 1);
        let producer = wave.ready.pop_front().unwrap();
        wave.publish(Completion { id: producer.id, node: start, port: producer.port,
            cohort: producer.cohort, result: Ok(vec![emit(0, "x".to_owned())]) });
        assert_eq!(wave.activity.state(middle), NodeState::Executing);
        assert_eq!(wave.activity.active_partitions.len(), 2,
            "newly activated child region joins the active index");
        wave.activity.finished(producer.id, start, index);
        assert_eq!(wave.activity.active_partitions.len(), 1);
        assert!(wave.activity.active_partitions.contains_key(&index.region(middle)));
    }

    #[test]
    fn settled_fresh_cohort_delivery_is_not_misattributed_to_other_cohorts() {
        let mut builder = FlatGraphBuilder::default();
        builder.add_node(pass(1).with_fresh_cohort());
        builder.add_node(pair(2));
        builder.connect(output(1, 0), input(2, 0, 1));
        let graph = builder.build().unwrap();
        let mut wave = WaveState::new(&graph, WaveId(1));
        assert!(wave.deliver_batch(vec![
            incoming(2, 0, "waiting"),
            Incoming { to: input(2, 0, 1), cohort: CohortId(9), value: enc("delivered".to_owned()) },
        ]));
        wave.record_settled(InvocationId(33), NodeId(1), CohortId(9),
            SettledOutcome::Emitted(BTreeSet::from([input(2, 0, 1)])));
        let port = &graph.nodes[&NodeId(2)].ports[0];
        let snapshot = wave.snapshots(NodeId(2), port);
        assert!(snapshot.colors.get(&CohortId(4)).into_iter().flatten()
            .all(|fact| fact.state != ColorState::Terminal(ColorOutcome::Delivered)));
        assert!(snapshot.colors.get(&CohortId(9)).into_iter().flatten()
            .any(|fact| fact.state == ColorState::Terminal(ColorOutcome::Delivered)));
    }

    fn reference_reach(graph: &Graph, from: NodeId, target: PortKey) -> (bool, bool) {
        let mut queue = VecDeque::from([(from,
            graph.nodes[&from].cohort_mode == CohortMode::FreshPerInvocation)]);
        let mut visited = BTreeSet::new();
        let mut preserve = false;
        let mut reset = false;
        while let Some((node, refreshed)) = queue.pop_front() {
            if !visited.insert((node, refreshed)) { continue; }
            for edge in graph.by_node.get(&node).into_iter().flatten() {
                if (edge.to.node, edge.to.port) == target {
                    if refreshed { reset = true; } else { preserve = true; }
                } else {
                    queue.push_back((edge.to.node, refreshed ||
                        graph.nodes[&edge.to.node].cohort_mode == CohortMode::FreshPerInvocation));
                }
            }
        }
        (preserve, reset)
    }

    #[test]
    fn reverse_cache_matches_forward_reference_for_cyclic_multiport_graphs() {
        let mut random = 0xD1B54A32D192ED03_u64;
        let mut next = || {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            random
        };
        for _case in 0..24 {
            let mut g = FlatGraphBuilder::default();
            for n in 1..=9_u16 {
                g.add_node(node(n, vec![
                    all(0, vec![slot(0, S, BufferMode::Deferred)]),
                    all(1, vec![slot(0, S, BufferMode::Deferred)]),
                ], vec![out(0, S)],
                    if next() % 4 == 0 { CohortMode::FreshPerInvocation }
                    else { CohortMode::Inherit }, vec![],
                    |ctx| Ok(vec![emit(0, ctx.arg::<String>(0)?)]) ));
            }
            for src in 1..=9_u16 {
                for dst in 1..=9_u16 {
                    for port in 0..=1_u16 {
                        if next() % 6 == 0 {
                            g.connect(output(src, 0), input(dst, port, 0));
                        }
                    }
                }
            }
            let graph = g.build().unwrap();
            for src in 1..=9_u16 {
                for dst in 1..=9_u16 {
                    for port in 0..=1_u16 {
                        let target = (NodeId(dst), PortId(port));
                        assert_eq!(graph.can_reach_port(NodeId(src), target),
                            reference_reach(&graph, NodeId(src), target),
                            "source {src}, target {dst}.{port}");
                    }
                }
            }
        }
    }
