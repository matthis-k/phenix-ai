use super::*;
use phenix_contract::Authority;

#[test]
fn intrinsic_chain_preserves_attenuation_across_foreign_delivery() {
    let (d, mut policy) = super::super::tests::switch_merge_fixture(Classification::Public);
    let value = Value::String("selected".into());
    for (required, grant, succeeds) in [
        ("a", vec!["a", "b"], true),
        ("b", vec!["a", "b"], false),
        ("a", vec!["b"], false),
    ] {
        policy
            .delivery
            .get_mut(&(
                "alpha/merge".into(),
                "out".into(),
                "beta/sink".into(),
                "in".into(),
            ))
            .unwrap()
            .resources = grant.into_iter().map(String::from).collect();
        let graph = super::super::tests::run(&d, &policy).unwrap();
        let root = ScopedRequest {
            authority: Authority {
                operations: BTreeSet::new(),
                resources: BTreeSet::from(["a".into(), "b".into()]),
            },
            resource: Some(required.into()),
        };
        let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
        // The producer has already narrowed root authority before the Switch.
        wave.resources
            .insert("alpha/source".into(), BTreeSet::from(["a".into()]));
        wave.deliver(
            &graph,
            &root,
            "alpha/source",
            Some(("out", &Value::map([("one", value.clone())]))),
            false,
        )
        .unwrap();
        for id in ["alpha/switch", "alpha/merge"] {
            let Resolution::Value(payload, resources) = wave.input(id).unwrap() else {
                panic!("expected selected value")
            };
            assert_eq!(resources, BTreeSet::from(["a".into()]));
            let result = wave.route_intrinsic(
                &graph,
                &root,
                &wave.nodes[id].1.clone(),
                &payload,
                resources,
            );
            if id == "alpha/merge" && !succeeds {
                assert_eq!(result.unwrap_err().code, Code::Denied);
            } else {
                result.unwrap();
            }
        }
        if succeeds {
            assert!(
                matches!(wave.input("beta/sink").unwrap(), Resolution::Value(payload, resources) if payload == value && resources == BTreeSet::from(["a".into()]))
            );
        } else {
            assert!(matches!(
                wave.input("beta/sink").unwrap(),
                Resolution::Unresolved
            ));
        }
    }
}

#[test]
fn confidential_branch_changes_merge_cardinality_without_public_relabeling() {
    let (d, p) = super::super::tests::switch_merge_fixture(Classification::Confidential);
    let graph = super::super::tests::run(&d, &p).unwrap();
    for case in ["one", "two"] {
        let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
        wave.route_intrinsic(
            &graph,
            &scope(&["a", "b"]),
            &wave.nodes["alpha/switch"].1.clone(),
            &Value::map([(case, Value::String("classified".into()))]),
            BTreeSet::from(["a".into()]),
        )
        .unwrap();
        let result = wave.input("alpha/merge");
        if case == "one" {
            assert!(matches!(result.unwrap(), Resolution::Value(_, _)));
        } else {
            assert!(matches!(
                result,
                Err(Failure {
                    code: Code::InfrastructureFailure,
                    ..
                })
            ));
        }
        assert!(matches!(
            wave.input("beta/sink").unwrap(),
            Resolution::Unresolved
        ));
    }
}

#[test]
fn merge_terminal_reduction_all_outcomes_and_arrival_orders() {
    let (d, p) = super::super::tests::merge_fixture();
    let graph = super::super::tests::run(&d, &p).unwrap();
    let scope = scope(&["resource", "other"]);
    let value = Value::String("selected".into());
    // Exhaust every pair and both completion orders, including late failures.
    for left in 0..3 {
        for right in 0..3 {
            for reverse in [false, true] {
                let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
                let candidates = if reverse {
                    [("alpha/b", right), ("alpha/a", left)]
                } else {
                    [("alpha/a", left), ("alpha/b", right)]
                };
                for (index, (edge, state)) in candidates.into_iter().enumerate() {
                    wave.arrivals.insert(
                        edge.into(),
                        match state {
                            0 => Resolution::Closed,
                            1 => Resolution::Value(
                                value.clone(),
                                BTreeSet::from(["resource".into()]),
                            ),
                            _ => Resolution::Failed,
                        },
                    );
                    if index == 0 {
                        assert!(matches!(
                            wave.input("alpha/merge").unwrap(),
                            Resolution::Unresolved
                        ));
                        assert!(matches!(wave.arrivals["alpha/c"], Resolution::Unresolved));
                    }
                }
                let result = wave.input("alpha/merge");
                if left == 2 || right == 2 {
                    assert!(matches!(result.unwrap(), Resolution::Failed));
                    wave.deliver(&graph, &scope, "alpha/merge", None, true)
                        .unwrap();
                    assert!(matches!(
                        wave.input("alpha/sink").unwrap(),
                        Resolution::Failed
                    ));
                } else if left == 1 && right == 1 {
                    assert!(matches!(
                        result,
                        Err(Failure {
                            code: Code::InfrastructureFailure,
                            ..
                        })
                    ));
                    assert!(matches!(wave.arrivals["alpha/c"], Resolution::Unresolved));
                } else if left == 0 && right == 0 {
                    assert!(matches!(result.unwrap(), Resolution::Closed));
                    wave.deliver(&graph, &scope, "alpha/merge", None, false)
                        .unwrap();
                    assert!(matches!(
                        wave.input("alpha/sink").unwrap(),
                        Resolution::Closed
                    ));
                } else {
                    let Resolution::Value(payload, resources) = result.unwrap() else {
                        panic!("expected value")
                    };
                    wave.route_intrinsic(
                        &graph,
                        &scope,
                        &wave.nodes["alpha/merge"].1.clone(),
                        &payload,
                        resources,
                    )
                    .unwrap();
                    assert!(
                        matches!(wave.input("alpha/sink").unwrap(), Resolution::Value(v, r) if v == value && r == BTreeSet::from(["resource".into()]))
                    );
                }
            }
        }
    }
}

#[test]
fn cross_source_merge_closes_inactive_candidate_per_wave() {
    let (d, p) = super::super::tests::merge_fixture();
    let graph = super::super::tests::run(&d, &p).unwrap();
    for source in ["alpha/submit", "alpha/alternative"] {
        for reverse in [false, true] {
            let mut wave = Wave::new(&graph, source).unwrap();
            let mut sources = ["alpha/source", "alpha/alternative-source"];
            if reverse {
                sources.reverse();
            }
            for id in sources {
                let NodeKind::Source { source: candidate } = &wave.nodes[id].1.kind else {
                    unreachable!()
                };
                let value = Value::String(source.into());
                let selected = (candidate == source).then_some(("out", &value));
                wave.deliver(&graph, &scope(&["resource"]), id, selected, false)
                    .unwrap();
            }
            assert!(
                matches!(wave.input("alpha/merge").unwrap(), Resolution::Value(Value::String(v), _) if v == source)
            );
        }
    }
}

#[test]
fn switch_emits_raw_payload_or_unit_and_closes_other_cases() {
    use phenix_contract::ValueSchema;
    let (mut d, p) = super::super::tests::merge_fixture();
    let schema = ValueSchema::Tagged(BTreeMap::from([
        ("empty".into(), None),
        ("payload".into(), Some(ValueSchema::String)),
    ]));
    d[0].nodes.retain(|n| n.id != "alpha/alternative-source");
    d[0].nodes[0].outputs[0].schema = schema.clone();
    d[0].sources[0].payload = schema.clone();
    let switch = d[0]
        .nodes
        .iter_mut()
        .find(|n| n.id == "alpha/merge")
        .unwrap();
    switch.kind = NodeKind::Switch {
        tagged_schema: schema.id().unwrap(),
    };
    switch.inputs[0].schema = schema;
    let label = switch.outputs[0].security;
    switch.outputs = vec![
        OutputPort {
            name: "empty".into(),
            schema: ValueSchema::Struct(BTreeMap::new()),
            security: label,
        },
        OutputPort {
            name: "payload".into(),
            schema: ValueSchema::String,
            security: label,
        },
    ];
    let mut unit_sink = d[0]
        .nodes
        .iter()
        .find(|n| n.id == "alpha/sink")
        .unwrap()
        .clone();
    unit_sink.id = "alpha/unit".into();
    unit_sink.inputs[0].schema = ValueSchema::Struct(BTreeMap::new());
    unit_sink.outputs[0].schema = unit_sink.inputs[0].schema.clone();
    d[0].nodes.push(unit_sink);
    d[0].edges.retain(|e| e.id != "alpha/b");
    let EdgeKind::Flow { from, .. } = &mut d[0].edges[1].kind else {
        unreachable!()
    };
    from.port = "payload".into();
    d[0].edges.push(EdgeDecl {
        id: "alpha/unit-edge".into(),
        kind: EdgeKind::Flow {
            from: OutPortRef {
                node: "alpha/merge".into(),
                port: "empty".into(),
            },
            to: InPortRef {
                node: "alpha/unit".into(),
                port: "in".into(),
            },
        },
    });
    let graph = super::super::tests::run(&d, &p).unwrap();
    for (case, payload, sink, expected) in [
        (
            "payload",
            Value::String("raw".into()),
            "alpha/sink",
            Value::String("raw".into()),
        ),
        (
            "empty",
            Value::Null,
            "alpha/unit",
            Value::Map(BTreeMap::new()),
        ),
    ] {
        let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
        let node = wave.nodes["alpha/merge"].1.clone();
        wave.route_intrinsic(
            &graph,
            &scope(&["resource"]),
            &node,
            &Value::map([(case, payload)]),
            BTreeSet::from(["resource".into()]),
        )
        .unwrap();
        assert!(
            matches!(wave.input(sink).unwrap(), Resolution::Value(value, _) if value == expected)
        );
        let other = if sink == "alpha/unit" {
            "alpha/sink"
        } else {
            "alpha/unit"
        };
        assert!(matches!(wave.input(other).unwrap(), Resolution::Closed));
    }
    for failed in [false, true] {
        let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
        wave.deliver(&graph, &scope(&["resource"]), "alpha/merge", None, failed)
            .unwrap();
        for sink in ["alpha/sink", "alpha/unit"] {
            assert!(matches!(
                (failed, wave.input(sink).unwrap()),
                (true, Resolution::Failed) | (false, Resolution::Closed)
            ));
        }
    }
}

#[test]
fn active_source_waits_for_ordered_pending_predecessor_but_inactive_source_does_not() {
    let (mut declarations, mut policy) = super::super::tests::fixture();
    let mut second = declarations[0].nodes[0].clone();
    second.id = "alpha/second-source".into();
    declarations[0].nodes.push(second);
    let mut inactive = declarations[0].sources[0].clone();
    inactive.id = "alpha/inactive".into();
    inactive.kind = SourceKind::ExternalAction {
        action: "inactive".into(),
    };
    policy
        .sources
        .insert(inactive.id.clone(), inactive.security);
    declarations[0].sources.push(inactive);
    let mut inactive_node = declarations[0].nodes[0].clone();
    inactive_node.id = "alpha/inactive-source".into();
    inactive_node.kind = NodeKind::Source {
        source: "alpha/inactive".into(),
    };
    declarations[0].nodes.push(inactive_node);
    for after in ["alpha/second-source", "alpha/inactive-source"] {
        declarations[0].edges.push(EdgeDecl {
            id: format!("{after}-order"),
            kind: EdgeKind::Order {
                before: "alpha/first".into(),
                after: after.into(),
            },
        });
    }
    let EdgeKind::Flow { from, .. } = &mut declarations[0].edges[1].kind else {
        unreachable!()
    };
    from.node = "alpha/second-source".into();
    from.port = "out".into();
    let grant = policy.delivery[&(
        "alpha/first".into(),
        "ok".into(),
        "beta/second".into(),
        "in".into(),
    )]
        .clone();
    policy.delivery.insert(
        (
            "alpha/second-source".into(),
            "out".into(),
            "beta/second".into(),
            "in".into(),
        ),
        grant,
    );
    declarations[0]
        .nodes
        .iter_mut()
        .find(|n| n.id == "alpha/first")
        .unwrap()
        .execution
        .on_domain_error = DomainErrorPolicy::SkipDependents;
    let graph = super::super::tests::run(&declarations, &policy).unwrap();
    let scope = scope(&["resource"]);
    let payload = Value::String("source occurrence".into());
    for outcome in [
        Outcome::Success(Value::String("producer output".into())),
        Outcome::Domain(Value::String("domain failure".into())),
    ] {
        let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
        wave.deliver(
            &graph,
            &scope,
            "alpha/source",
            Some(("out", &payload)),
            false,
        )
        .unwrap();
        assert!(wave.order_ready(&graph, "alpha/first"));
        wave.started.insert("alpha/first".into());
        // Repeated readiness passes while the retained ticket remains pending.
        for _ in 0..2 {
            assert!(!wave.order_ready(&graph, "alpha/second-source"));
            assert!(matches!(
                wave.input("beta/second").unwrap(),
                Resolution::Unresolved
            ));
        }
        assert!(wave.order_ready(&graph, "alpha/inactive-source"));
        wave.deliver(&graph, &scope, "alpha/inactive-source", None, false)
            .unwrap();
        assert!(wave.done.contains("alpha/inactive-source"));
        assert!(!wave
            .complete(&graph, &scope, "alpha/first", &outcome)
            .unwrap());
        assert!(wave.order_ready(&graph, "alpha/second-source"));
        wave.deliver(
            &graph,
            &scope,
            "alpha/second-source",
            Some(("out", &payload)),
            false,
        )
        .unwrap();
        assert!(wave.order_ready(&graph, "beta/second"));
        assert!(
            matches!(wave.input("beta/second").unwrap(), Resolution::Value(value, _) if value == payload)
        );
    }
}

#[test]
fn order_only_constant_is_dormant_and_err_routes_actual_domain_value() {
    let (mut d, mut p) = super::super::tests::fixture();
    let mut constant = d[0].nodes[1].clone();
    constant.id = "alpha/constant".into();
    constant.kind = NodeKind::Constant {
        value: Value::String("literal".into()),
    };
    constant.inputs[0].name = "start".into();
    constant.inputs[0].schema = phenix_contract::ValueSchema::Struct(BTreeMap::new());
    constant.outputs = vec![OutputPort {
        name: "out".into(),
        schema: phenix_contract::ValueSchema::String,
        security: constant.outputs[0].security,
    }];
    d[0].nodes.push(constant);
    d[0].edges.push(EdgeDecl {
        id: "alpha/order-constant".into(),
        kind: EdgeKind::Order {
            before: "alpha/first".into(),
            after: "alpha/constant".into(),
        },
    });
    let graph = super::super::tests::run(&d, &p).unwrap();
    let wave = Wave::new(&graph, "alpha/submit").unwrap();
    assert!(matches!(
        wave.input("alpha/constant").unwrap(),
        Resolution::Closed
    ));
    assert!(!graph.source_reachability["alpha/constant"].contains("alpha/submit"));
    let EdgeKind::Flow { from, .. } = &mut d[0].edges[1].kind else {
        unreachable!()
    };
    from.port = "err".into();
    let grant = p.delivery[&(
        "alpha/first".into(),
        "ok".into(),
        "beta/second".into(),
        "in".into(),
    )]
        .clone();
    p.delivery.insert(
        (
            "alpha/first".into(),
            "err".into(),
            "beta/second".into(),
            "in".into(),
        ),
        grant,
    );
    let graph = super::super::tests::run(&d, &p).unwrap();
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    wave.complete(
        &graph,
        &scope(&["resource"]),
        "alpha/first",
        &Outcome::Domain(Value::String("actual error".into())),
    )
    .unwrap();
    assert!(
        matches!(wave.input("beta/second").unwrap(), Resolution::Value(Value::String(v), _) if v == "actual error")
    );
}

#[test]
fn duplicate_external_action_ids_reject_offline() {
    let (mut d, mut p) = super::super::tests::fixture();
    let mut source = d[0].sources[0].clone();
    source.id = "alpha/duplicate".into();
    p.sources.insert(source.id.clone(), source.security);
    d[0].sources.push(source);
    assert!(super::super::tests::run(&d, &p)
        .unwrap_err()
        .message
        .contains("duplicate graph-v2 action"));
}

#[test]
fn byte_and_node_limits_fail_closed() {
    let (d, p) = super::super::tests::fixture();
    let mut graph = super::super::tests::run(&d, &p).unwrap();
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    wave.bytes = MAX_ROOT_BYTES;
    assert_eq!(
        wave.complete(
            &graph,
            &scope(&["resource"]),
            "alpha/first",
            &Outcome::Success(Value::String("over limit".into()))
        )
        .unwrap_err()
        .code,
        Code::InfrastructureFailure
    );
    let node = graph.declarations[0].nodes[0].clone();
    for index in 0..MAX_ROOT_NODES {
        let mut node = node.clone();
        node.id = format!("alpha/extra-{index}");
        graph.declarations[0].nodes.push(node);
    }
    assert_eq!(
        Wave::new(&graph, "alpha/submit").err().unwrap().code,
        Code::InfrastructureFailure
    );
}

fn scope(resources: &[&str]) -> ScopedRequest {
    ScopedRequest {
        authority: Authority {
            operations: BTreeSet::new(),
            resources: resources.iter().map(|r| (*r).into()).collect(),
        },
        resource: Some("resource".into()),
    }
}

#[test]
fn producer_output_not_source_and_repeated_completion_is_inert() {
    let (d, p) = super::super::tests::fixture();
    let graph = super::super::tests::run(&d, &p).unwrap();
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    let scope = scope(&["resource"]);
    wave.deliver(
        &graph,
        &scope,
        "alpha/source",
        Some(("out", &Value::String("source".into()))),
        false,
    )
    .unwrap();
    let output = Outcome::Success(Value::String("actual producer".into()));
    wave.complete(&graph, &scope, "alpha/first", &output)
        .unwrap();
    assert!(
        matches!(&wave.arrivals["alpha/next"], Resolution::Value(Value::String(v), _) if v == "actual producer")
    );
    let values = wave.values;
    wave.complete(&graph, &scope, "alpha/first", &output)
        .unwrap();
    assert_eq!(values, wave.values);
}

#[test]
fn inactive_source_closes_and_domain_policies_are_distinct() {
    let (d, p) = super::super::tests::fixture();
    let graph = super::super::tests::run(&d, &p).unwrap();
    let scope = scope(&["resource"]);
    let mut wave = Wave::new(&graph, "other/source").unwrap();
    wave.deliver(&graph, &scope, "alpha/source", None, false)
        .unwrap();
    assert!(matches!(wave.arrivals["alpha/start"], Resolution::Closed));
    for policy in [
        DomainErrorPolicy::RouteErr,
        DomainErrorPolicy::SkipDependents,
        DomainErrorPolicy::FailRoot,
    ] {
        let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
        wave.nodes
            .get_mut("alpha/first")
            .unwrap()
            .1
            .execution
            .on_domain_error = policy;
        assert_eq!(
            wave.complete(
                &graph,
                &scope,
                "alpha/first",
                &Outcome::Domain(Value::String("domain".into()))
            )
            .unwrap(),
            policy == DomainErrorPolicy::FailRoot
        );
        assert!(match policy {
            DomainErrorPolicy::RouteErr =>
                matches!(wave.arrivals["alpha/next"], Resolution::Closed),
            _ => matches!(wave.arrivals["alpha/next"], Resolution::Failed),
        });
    }
}

#[test]
fn delivery_denies_narrowed_and_inherited_resources_and_bounds_values() {
    let (d, p) = super::super::tests::fixture();
    let graph = super::super::tests::run(&d, &p).unwrap();
    for resources in [vec![], vec!["other"]] {
        let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
        assert_eq!(
            wave.deliver(
                &graph,
                &scope(&resources),
                "alpha/source",
                Some(("out", &Value::String("protected".into()))),
                false
            )
            .unwrap_err()
            .code,
            Code::Denied
        );
    }
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    wave.resources.insert("alpha/first".into(), BTreeSet::new());
    assert_eq!(
        wave.complete(
            &graph,
            &scope(&["resource"]),
            "alpha/first",
            &Outcome::Success(Value::String("protected".into()))
        )
        .unwrap_err()
        .code,
        Code::Denied
    );
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    wave.values = MAX_ROOT_VALUES;
    assert_eq!(
        wave.deliver(
            &graph,
            &scope(&["resource"]),
            "alpha/source",
            Some(("out", &Value::String("bounded".into()))),
            false
        )
        .unwrap_err()
        .code,
        Code::InfrastructureFailure
    );
}
