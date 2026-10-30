use super::*;
use phenix_contract::Authority;

#[test]
fn deferred_arrival_survives_producer_closure_and_propagates_absence_scope() {
    let (mut declarations, policy) = super::super::tests::fixture();
    let EdgeKind::Flow { from, to } = declarations[0].edges[0].kind.clone() else {
        unreachable!()
    };
    declarations[0].edges[0].kind = EdgeKind::Deferred {
        from,
        to,
        next: DeferredWave::Next,
    };
    let graph = super::super::tests::run(&declarations, &policy).unwrap();
    let mut wave = Wave::new(&graph, "$deferred:alpha/start").unwrap();
    let resources = BTreeSet::from(["resource".into()]);
    wave.arrivals.insert(
        "alpha/start".into(),
        Resolution::Value(Some(Value::String("admitted".into())), resources.clone()),
    );
    // The old producer is inactive in this wave. It cannot close the admission.
    wave.deliver(
        &graph,
        &scope(&["resource", "other"]),
        "alpha/source",
        None,
        false,
    )
    .unwrap();
    assert!(
        matches!(wave.input("alpha/first").unwrap(), Resolution::Value(Some(_), ref r) if r == &resources)
    );
    wave.causes.insert(
        "alpha/first".into(),
        wave.input_evidence("alpha/first").unwrap(),
    );
    wave.deliver(
        &graph,
        &scope(&["resource", "other"]),
        "alpha/first",
        None,
        false,
    )
    .unwrap();
    let evidence = wave.input_evidence("beta/second").unwrap();
    assert!(evidence.causal && evidence.authorized);
    assert_eq!(evidence.resources, Some(resources));
    assert!(matches!(
        wave.input("beta/second").unwrap(),
        Resolution::Closed
    ));
    assert!(wave.emitted.is_empty());
}

#[test]
fn deferred_foreign_delivery_attenuates_and_denies_before_enqueue() {
    let (mut declarations, mut policy) = super::super::tests::fixture();
    let EdgeKind::Flow { from, to } = declarations[0].edges[1].kind.clone() else {
        unreachable!()
    };
    declarations[0].edges[1].kind = EdgeKind::Deferred {
        from,
        to,
        next: DeferredWave::Next,
    };
    let graph = super::super::tests::run(&declarations, &policy).unwrap();
    let value = Value::String("next".into());
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    wave.resources.insert(
        "alpha/first".into(),
        BTreeSet::from(["resource".into(), "other".into()]),
    );
    wave.deliver(
        &graph,
        &scope(&["resource", "other"]),
        "alpha/first",
        Some(("ok", &value)),
        false,
    )
    .unwrap();
    assert_eq!(
        wave.emitted["alpha/next"].resources,
        BTreeSet::from(["resource".into()])
    );
    for missing in [false, true] {
        let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
        wave.resources
            .insert("alpha/first".into(), BTreeSet::from(["other".into()]));
        let mut denied_graph = graph.clone();
        if missing {
            policy.delivery.clear();
            denied_graph.policy = policy.clone();
        }
        assert_eq!(
            wave.deliver(
                &denied_graph,
                &scope(&["resource", "other"]),
                "alpha/first",
                Some(("ok", &value)),
                false
            )
            .unwrap_err()
            .code,
            Code::Denied
        );
        assert!(wave.emitted.is_empty());
    }
}

#[test]
fn separate_deferred_inputs_cannot_satisfy_a_same_wave_join() {
    let (mut declarations, policy) = super::super::tests::join_fixture();
    for edge in &mut declarations[0].edges[..2] {
        let EdgeKind::Flow { from, to } = edge.kind.clone() else {
            unreachable!()
        };
        edge.kind = EdgeKind::Deferred {
            from,
            to,
            next: DeferredWave::Next,
        };
    }
    let failure = super::super::tests::run(&declarations, &policy).unwrap_err();
    assert!(failure.message.contains("disjoint input sources"));
    // Also exercise terminal reduction independently of static rejection.
    let (declarations, policy) = super::super::tests::join_fixture();
    let mut graph = super::super::tests::run(&declarations, &policy).unwrap();
    for edge in &mut graph.declarations[0].edges[..2] {
        let EdgeKind::Flow { from, to } = edge.kind.clone() else {
            unreachable!()
        };
        edge.kind = EdgeKind::Deferred {
            from,
            to,
            next: DeferredWave::Next,
        };
    }
    for admitted in ["alpha/a", "alpha/b"] {
        let mut wave = Wave::new(&graph, &format!("$deferred:{admitted}")).unwrap();
        wave.arrivals.insert(
            admitted.into(),
            Resolution::Value(
                Some(Value::String("one wave".into())),
                BTreeSet::from(["resource".into()]),
            ),
        );
        assert!(matches!(
            wave.input("alpha/merge").unwrap(),
            Resolution::Closed
        ));
    }
}

#[test]
fn deferred_delivery_is_queued_not_a_same_wave_arrival() {
    let (mut declarations, policy) = super::super::tests::fixture();
    let edge = &mut declarations[0].edges[0];
    let EdgeKind::Flow { from, to } = edge.kind.clone() else {
        unreachable!()
    };
    let producer = from.node.clone();
    let port = from.port.clone();
    let target = to.node.clone();
    let id = edge.id.clone();
    edge.kind = EdgeKind::Deferred {
        from,
        to,
        next: DeferredWave::Next,
    };
    let graph = super::super::tests::run(&declarations, &policy).unwrap();
    assert!(!graph.predecessors[&target].contains(&producer));
    assert!(graph.source_reachability[&target].contains(&format!("$deferred:{id}")));
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    let value = Value::String("next".into());
    wave.resources
        .insert(producer.clone(), BTreeSet::from(["resource".into()]));
    wave.deliver(
        &graph,
        &scope(&["resource"]),
        &producer,
        Some((&port, &value)),
        false,
    )
    .unwrap();
    assert!(matches!(wave.arrivals[&id], Resolution::Closed));
    assert_eq!(wave.emitted[&id].value, value);
    assert_eq!(
        wave.emitted[&id].resources,
        BTreeSet::from(["resource".into()])
    );
    assert_eq!(wave.values, 2);
    assert_eq!(wave.executions, 1);
    // Replaying a completion cannot admit another activation.
    wave.deliver(
        &graph,
        &scope(&["resource"]),
        &producer,
        Some((&port, &value)),
        false,
    )
    .unwrap();
    assert_eq!(wave.emitted.len(), 1);
    assert_eq!(wave.values, 2);
}

#[test]
fn deferred_closure_never_enqueues_a_wave() {
    let (mut declarations, policy) = super::super::tests::fixture();
    let edge = &mut declarations[0].edges[0];
    let EdgeKind::Flow { from, to } = edge.kind.clone() else {
        unreachable!()
    };
    let producer = from.node.clone();
    edge.kind = EdgeKind::Deferred {
        from,
        to,
        next: DeferredWave::Next,
    };
    let graph = super::super::tests::run(&declarations, &policy).unwrap();
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    wave.deliver(&graph, &scope(&["resource"]), &producer, None, false)
        .unwrap();
    assert!(wave.emitted.is_empty());
}

#[test]
fn deferred_queue_and_cumulative_value_bounds_fail_closed() {
    let (mut declarations, policy) = super::super::tests::fixture();
    let edge = &mut declarations[0].edges[0];
    let EdgeKind::Flow { from, to } = edge.kind.clone() else {
        unreachable!()
    };
    let producer = from.node.clone();
    let port = from.port.clone();
    let id = edge.id.clone();
    edge.kind = EdgeKind::Deferred {
        from,
        to,
        next: DeferredWave::Next,
    };
    let graph = super::super::tests::run(&declarations, &policy).unwrap();
    let resources = BTreeSet::from(["resource".into()]);
    let value = Value::String("next".into());
    for queue_full in [false, true] {
        let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
        wave.resources.insert(producer.clone(), resources.clone());
        if queue_full {
            for _ in 0..MAX_DEFERRED_QUEUE {
                wave.queue.push_back(DeferredActivation {
                    edge: id.clone(),
                    value: value.clone(),
                    resources: resources.clone(),
                });
            }
        } else {
            wave.values = MAX_ROOT_VALUES;
        }
        let failure = wave
            .deliver(
                &graph,
                &scope(&["resource"]),
                &producer,
                Some((&port, &value)),
                false,
            )
            .unwrap_err();
        assert_eq!(failure.code, Code::InfrastructureFailure);
        assert!(wave.emitted.is_empty());
    }
}

// Deterministic work count, not a wall-clock threshold.
#[test]
fn review_port_input_visits_only_relevant_candidates() {
    let (mut declarations, policy) = super::super::tests::join_fixture();
    for index in 0..128 {
        let mut unrelated = declarations[0].edges[0].clone();
        unrelated.id = format!("alpha/irrelevant-{index}");
        let EdgeKind::Flow { to, .. } = &mut unrelated.kind else {
            unreachable!()
        };
        let mut node = declarations[0]
            .nodes
            .iter()
            .find(|n| n.id == to.node)
            .unwrap()
            .clone();
        node.id = format!("alpha/unrelated-{index}");
        to.node = node.id.clone();
        declarations[0].nodes.push(node);
        declarations[0].edges.push(unrelated);
    }
    let graph = super::super::tests::run(&declarations, &policy).unwrap();
    let wave = Wave::new(&graph, "alpha/submit").unwrap();
    PORT_INPUT_EDGE_VISITS.with(|visits| visits.set(0));
    let _ = wave.port_input("alpha/merge", "left").unwrap();
    let visits = PORT_INPUT_EDGE_VISITS.with(|visits| visits.get());
    assert!(
        visits == 1,
        "port_input inspected {visits} edges for a port with one incoming candidate"
    );
}

// A non-selected external source closes its
// Flow candidates with the causal reason InactiveSource, not an anonymous Closed.
#[test]
fn review_closed_flow_identifies_inactive_source_reason() {
    let graph = readiness_join(InputReadiness::Optional);
    let mut wave = Wave::new(&graph, "alpha/alternative").unwrap();
    wave.deliver(&graph, &scope(&["resource"]), "alpha/source", None, false)
        .unwrap();
    assert!(matches!(wave.arrivals["alpha/a"], Resolution::Closed));
    assert_eq!(
        wave.terminal_causes["alpha/a"],
        TerminalCause::InactiveSource {
            source: "alpha/submit".into(),
        }
    );
}

// An unhandled domain error retains
// typed causal provenance on the failed downstream Flow candidate.
#[test]
fn review_failed_flow_retains_domain_error_identity() {
    let (mut declarations, policy) = super::super::tests::fixture();
    declarations[0]
        .nodes
        .iter_mut()
        .find(|node| node.id == "alpha/first")
        .unwrap()
        .execution
        .on_domain_error = DomainErrorPolicy::SkipDependents;
    let graph = super::super::tests::run(&declarations, &policy).unwrap();
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    let distinct_cause = "domain-cause-42";
    wave.complete(
        &graph,
        &scope(&["resource"]),
        "alpha/first",
        &Outcome::Domain(Value::String(distinct_cause.into())),
    )
    .unwrap();
    assert!(matches!(
        wave.input("beta/second").unwrap(),
        Resolution::Failed
    ));
    assert_eq!(
        wave.terminal_causes["alpha/next"],
        TerminalCause::Domain {
            node: "alpha/first".into(),
            value: std::rc::Rc::new(Value::String(distinct_cause.into())),
        }
    );
}

#[test]
fn terminal_provenance_propagates_and_selects_failure_by_edge_id() {
    // Two independent producers are exercised directly here; source disjointness
    // is tested by the compiler separately.
    let graph = readiness_join(InputReadiness::One);
    for reverse in [false, true] {
        let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
        let EdgeKind::Flow { from, .. } = &mut wave.edges.get_mut("alpha/b").unwrap().kind else {
            panic!()
        };
        from.node = "alpha/alternative-source".into();
        let expected = TerminalCause::Domain {
            node: "alpha/source".into(),
            value: std::rc::Rc::new(Value::String("first-by-edge-id".into())),
        };
        let mut producers = [
            ("alpha/source", "first-by-edge-id"),
            ("alpha/alternative-source", "second-by-edge-id"),
        ];
        if reverse {
            producers.reverse();
        }
        for (node, value) in producers {
            wave.nodes
                .get_mut(node)
                .unwrap()
                .1
                .execution
                .on_domain_error = DomainErrorPolicy::SkipDependents;
            wave.complete(
                &graph,
                &scope(&["resource"]),
                node,
                &Outcome::Domain(Value::String(value.into())),
            )
            .unwrap();
        }
        assert!(matches!(
            wave.input("alpha/merge").unwrap(),
            Resolution::Failed
        ));
        wave.deliver(&graph, &scope(&["resource"]), "alpha/merge", None, true)
            .unwrap();
        let outgoing = wave
            .edges
            .iter()
            .find_map(|(id, e)| {
                matches!(&e.kind, EdgeKind::Flow { from, .. } if from.node == "alpha/merge")
                    .then_some(id)
            })
            .unwrap();
        assert_eq!(wave.terminal_causes[outgoing], expected);
        assert_eq!(wave.terminal_causes["alpha/a"], expected);
        assert_eq!(
            wave.terminal_causes["alpha/b"],
            TerminalCause::Domain {
                node: "alpha/alternative-source".into(),
                value: std::rc::Rc::new(Value::String("second-by-edge-id".into())),
            }
        );
    }
    let mut wave = Wave::new(&graph, "alpha/alternative").unwrap();
    wave.deliver(&graph, &scope(&["resource"]), "alpha/source", None, false)
        .unwrap();
    wave.deliver(&graph, &scope(&["resource"]), "alpha/merge", None, false)
        .unwrap();
    let outgoing = wave
        .edges
        .iter()
        .find_map(|(id, e)| {
            matches!(&e.kind, EdgeKind::Flow { from, .. } if from.node == "alpha/merge")
                .then_some(id)
        })
        .unwrap();
    assert_eq!(
        wave.terminal_causes[outgoing],
        TerminalCause::InactiveSource {
            source: "alpha/submit".into()
        }
    );
    assert!(!wave.input_evidence("alpha/merge").unwrap().causal);
}

#[test]
fn late_failure_provenance_dominates_closed_required_input() {
    let graph = readiness_join(InputReadiness::One);
    let root = scope(&["resource"]);
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    let EdgeKind::Flow { from, .. } = &mut wave.edges.get_mut("alpha/b").unwrap().kind else {
        panic!()
    };
    from.node = "alpha/alternative-source".into();
    wave.deliver(&graph, &root, "alpha/source", None, false)
        .unwrap();
    assert!(matches!(
        wave.input("alpha/merge").unwrap(),
        Resolution::Unresolved
    ));
    wave.nodes
        .get_mut("alpha/alternative-source")
        .unwrap()
        .1
        .execution
        .on_domain_error = DomainErrorPolicy::SkipDependents;
    wave.complete(
        &graph,
        &root,
        "alpha/alternative-source",
        &Outcome::Domain(Value::String("late failure".into())),
    )
    .unwrap();
    assert!(matches!(
        wave.input("alpha/merge").unwrap(),
        Resolution::Failed
    ));
    wave.deliver(&graph, &root, "alpha/merge", None, true)
        .unwrap();
    for (id, edge) in &wave.edges {
        if matches!(&edge.kind, EdgeKind::Flow { from, .. } if from.node == "alpha/merge") {
            assert_eq!(wave.terminal_causes[id], wave.terminal_causes["alpha/b"]);
            assert_ne!(wave.terminal_causes[id], wave.terminal_causes["alpha/a"]);
        }
    }
}

fn readiness_join(readiness: InputReadiness) -> CompiledGraph {
    use phenix_contract::ValueSchema;
    let (mut d, p) = super::super::tests::join_fixture();
    let join = d[0]
        .nodes
        .iter_mut()
        .find(|n| n.id == "alpha/merge")
        .unwrap();
    join.inputs[0].readiness = readiness;
    join.inputs[0].schema = match readiness {
        InputReadiness::Optional => ValueSchema::Option(Box::new(ValueSchema::String)),
        InputReadiness::StaticMany { .. } => ValueSchema::List(Box::new(ValueSchema::String)),
        InputReadiness::One => ValueSchema::String,
    };
    let schema = ValueSchema::Struct(
        join.inputs
            .iter()
            .map(|p| (p.name.clone(), p.schema.clone()))
            .collect(),
    );
    join.outputs[0].schema = schema.clone();
    let sink = d[0]
        .nodes
        .iter_mut()
        .find(|n| n.id == "alpha/sink")
        .unwrap();
    sink.inputs[0].schema = schema.clone();
    sink.outputs[0].schema = schema;
    super::super::tests::run(&d, &p).unwrap()
}

#[test]
fn resource_free_readiness_preserves_explicit_empty_authority() {
    for readiness in [
        InputReadiness::Optional,
        InputReadiness::StaticMany { min: 0, max: 1 },
    ] {
        let graph = readiness_join(readiness);
        for present in [false, true] {
            for selected_resource in [false, true] {
                let mut root = scope(&[]);
                if !selected_resource {
                    root.resource = None;
                }
                let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
                let payload = Value::String("raw".into());
                wave.deliver(
                    &graph,
                    &root,
                    "alpha/source",
                    present.then_some(("out", &payload)),
                    false,
                )
                .unwrap();
                assert_eq!(wave.closure["alpha/a"].resources, Some(BTreeSet::new()));
                let input = wave.port_input("alpha/merge", "left");
                if selected_resource {
                    assert_eq!(input.err().unwrap().code, Code::Denied);
                } else {
                    let Resolution::Value(Some(value), resources) = input.unwrap() else {
                        panic!()
                    };
                    assert!(resources.is_empty());
                    assert_eq!(
                        value,
                        if present {
                            Value::List(vec![payload])
                        } else if readiness == InputReadiness::Optional {
                            Value::Null
                        } else {
                            Value::List(vec![])
                        }
                    );
                }
            }
        }
        // A relevant One input anchors an unconnected absence without adding rights.
        let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
        wave.edges.remove("alpha/a");
        wave.arrivals.remove("alpha/a");
        let mut root = scope(&[]);
        root.resource = None;
        wave.deliver(
            &graph,
            &root,
            "alpha/source",
            Some(("out", &Value::String("anchor".into()))),
            false,
        )
        .unwrap();
        let Resolution::Value(Some(Value::Map(fields)), resources) =
            wave.input("alpha/merge").unwrap()
        else {
            panic!()
        };
        assert!(resources.is_empty());
        assert_eq!(
            fields["left"],
            if readiness == InputReadiness::Optional {
                Value::Null
            } else {
                Value::List(vec![])
            }
        );
    }
}

#[test]
fn disjoint_same_owner_causal_scopes_intersect_to_empty_without_restoring_root() {
    let graph = readiness_join(InputReadiness::Optional);
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    let EdgeKind::Flow { from, .. } = &mut wave.edges.get_mut("alpha/b").unwrap().kind else {
        panic!()
    };
    from.node = "alpha/alternative-source".into();
    wave.resources
        .insert("alpha/source".into(), BTreeSet::from(["left".into()]));
    wave.resources.insert(
        "alpha/alternative-source".into(),
        BTreeSet::from(["right".into()]),
    );
    let mut root = scope(&["left", "right", "root-only"]);
    root.resource = None;
    wave.deliver(&graph, &root, "alpha/source", None, false)
        .unwrap();
    wave.deliver(
        &graph,
        &root,
        "alpha/alternative-source",
        Some(("out", &Value::String("anchor".into()))),
        false,
    )
    .unwrap();
    let Resolution::Value(_, resources) = wave.input("alpha/merge").unwrap() else {
        panic!()
    };
    assert!(resources.is_empty());
    assert_eq!(
        wave.input_evidence("alpha/merge").unwrap().resources,
        Some(BTreeSet::new())
    );
}

#[test]
fn resource_free_scope_does_not_bypass_cross_boundary_flow_policy() {
    let mut graph = readiness_join(InputReadiness::Optional);
    graph.routes.insert("alpha/merge".into(), "foreign".into());
    for input in ["left", "right"] {
        graph.policy.delivery.insert(
            (
                "alpha/source".into(),
                "out".into(),
                "alpha/merge".into(),
                input.into(),
            ),
            FlowGrant {
                resources: BTreeSet::from(["resource".into()]),
                maximum_classification: Classification::Public,
            },
        );
    }
    let mut root = scope(&[]);
    root.resource = None;
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    wave.deliver(&graph, &root, "alpha/source", None, false)
        .unwrap();
    assert_eq!(wave.closure["alpha/a"].resources, Some(BTreeSet::new()));
    assert!(!wave.closure["alpha/a"].authorized);
    assert_eq!(
        wave.port_input("alpha/merge", "left").err().unwrap().code,
        Code::Denied
    );
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    assert_eq!(
        wave.deliver(
            &graph,
            &root,
            "alpha/source",
            Some(("out", &Value::String("raw".into()))),
            false
        )
        .unwrap_err()
        .code,
        Code::Denied
    );
}

#[test]
fn cross_source_closure_authority_is_root_bounded_before_aggregate_intersection() {
    for causal_origin in [false, true] {
        for root_has_grant in [false, true] {
            for selected_resource in [false, true] {
                let mut graph = readiness_join(InputReadiness::Optional);
                graph
                    .routes
                    .insert("alpha/alternative-source".into(), "foreign".into());
                graph.policy.delivery.insert(
                    (
                        "alpha/alternative-source".into(),
                        "out".into(),
                        "alpha/merge".into(),
                        "left".into(),
                    ),
                    FlowGrant {
                        resources: BTreeSet::from(["b".into()]),
                        maximum_classification: Classification::Public,
                    },
                );
                let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
                let EdgeKind::Flow { from, .. } = &mut wave.edges.get_mut("alpha/a").unwrap().kind
                else {
                    panic!()
                };
                from.node = "alpha/alternative-source".into();
                wave.resources
                    .insert("alpha/source".into(), BTreeSet::from(["a".into()]));
                if causal_origin {
                    // Even an origin bound must be re-intersected with this root.
                    wave.resources.insert(
                        "alpha/alternative-source".into(),
                        BTreeSet::from(["b".into()]),
                    );
                }
                let root = ScopedRequest {
                    authority: Authority {
                        operations: BTreeSet::new(),
                        resources: if root_has_grant {
                            BTreeSet::from(["a".into(), "b".into()])
                        } else {
                            BTreeSet::from(["a".into()])
                        },
                    },
                    resource: selected_resource.then(|| "a".into()),
                };
                wave.deliver(
                    &graph,
                    &root,
                    "alpha/source",
                    Some(("out", &Value::String("anchor".into()))),
                    false,
                )
                .unwrap();
                assert!(matches!(
                    wave.input("alpha/merge").unwrap(),
                    Resolution::Unresolved
                ));
                wave.deliver(&graph, &root, "alpha/alternative-source", None, false)
                    .unwrap();
                let evidence = &wave.closure["alpha/a"];
                assert_eq!(evidence.causal, causal_origin);
                assert_eq!(
                    evidence.resources,
                    Some(if root_has_grant {
                        BTreeSet::from(["b".into()])
                    } else {
                        BTreeSet::new()
                    })
                );
                assert_eq!(evidence.authorized, root_has_grant && !selected_resource);
                let input = wave.input("alpha/merge");
                if root_has_grant && !selected_resource {
                    let Resolution::Value(Some(Value::Map(fields)), resources) = input.unwrap()
                    else {
                        panic!()
                    };
                    assert_eq!(fields["left"], Value::Null);
                    assert!(resources.is_empty());
                } else {
                    assert_eq!(input.err().unwrap().code, Code::Denied);
                }
                // The same denied control is not an access when One closes.
                wave.nodes.get_mut("alpha/merge").unwrap().1.inputs[0].readiness =
                    InputReadiness::One;
                assert!(matches!(
                    wave.input("alpha/merge").unwrap(),
                    Resolution::Closed
                ));
            }
        }
    }
}

#[test]
fn prospective_collection_cardinality_drops_only_known_non_runnable_deliveries() {
    let payload = Value::String("never delivered outside scope".into());
    let root = scope(&["resource"]);
    for (min, max) in [(0, 0), (2, 2), (1, 1)] {
        for join in [false, true] {
            for anchor_first in [false, true] {
                for failed_anchor in [false, true] {
                    let mut graph = readiness_join(InputReadiness::StaticMany { min, max });
                    graph.routes.insert("alpha/source".into(), "foreign".into());
                    graph.policy.delivery.insert(
                        (
                            "alpha/source".into(),
                            "out".into(),
                            "alpha/merge".into(),
                            "left".into(),
                        ),
                        FlowGrant {
                            resources: BTreeSet::from(["other".into()]),
                            maximum_classification: Classification::Public,
                        },
                    );
                    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
                    if join {
                        let EdgeKind::Flow { from, .. } =
                            &mut wave.edges.get_mut("alpha/b").unwrap().kind
                        else {
                            panic!()
                        };
                        from.node = "alpha/alternative-source".into();
                        wave.resources.insert(
                            "alpha/alternative-source".into(),
                            root.authority.resources.clone(),
                        );
                        if anchor_first {
                            wave.deliver(
                                &graph,
                                &root,
                                "alpha/alternative-source",
                                (!failed_anchor).then_some(("out", &payload)),
                                failed_anchor,
                            )
                            .unwrap();
                        }
                    } else {
                        let (producer, _) = super::super::tests::fixture();
                        wave.nodes.get_mut("alpha/merge").unwrap().1.kind =
                            producer[0].nodes[1].kind.clone();
                        wave.nodes
                            .get_mut("alpha/merge")
                            .unwrap()
                            .1
                            .inputs
                            .truncate(1);
                        wave.edges.remove("alpha/b");
                        wave.arrivals.remove("alpha/b");
                    }
                    wave.values = MAX_ROOT_VALUES - 1;
                    wave.bytes = MAX_ROOT_BYTES - payload.encode().unwrap().len();
                    let result = wave.deliver(
                        &graph,
                        &root,
                        "alpha/source",
                        Some(("out", &payload)),
                        false,
                    );
                    if min == 1 {
                        // Unknown/runnable consumers retain ordinary eager delivery checks.
                        assert_eq!(result.unwrap_err().code, Code::Denied);
                        assert!(matches!(wave.arrivals["alpha/a"], Resolution::Unresolved));
                        continue;
                    }
                    result.unwrap();
                    assert!(matches!(
                        wave.arrivals["alpha/a"],
                        Resolution::Value(None, _)
                    ));
                    assert_eq!(wave.values, MAX_ROOT_VALUES);
                    assert_eq!(wave.bytes, MAX_ROOT_BYTES);
                    if join && !anchor_first {
                        assert!(matches!(
                            wave.input("alpha/merge").unwrap(),
                            Resolution::Unresolved
                        ));
                        wave.values = 1;
                        wave.bytes = payload.encode().unwrap().len();
                        wave.deliver(
                            &graph,
                            &root,
                            "alpha/alternative-source",
                            (!failed_anchor).then_some(("out", &payload)),
                            failed_anchor,
                        )
                        .unwrap();
                    }
                    let input = wave.input("alpha/merge").unwrap();
                    if join && failed_anchor {
                        assert!(matches!(input, Resolution::Failed));
                    } else {
                        assert!(matches!(input, Resolution::Closed));
                    }
                }
            }
        }
    }
}

#[test]
fn prospective_collection_counts_all_selected_edges_atomically_but_waits_for_other_producers() {
    for same_producer in [false, true] {
        let mut graph = readiness_join(InputReadiness::StaticMany { min: 0, max: 1 });
        graph.routes.insert("alpha/source".into(), "foreign".into());
        graph.policy.delivery.insert(
            (
                "alpha/source".into(),
                "out".into(),
                "alpha/merge".into(),
                "left".into(),
            ),
            FlowGrant {
                resources: BTreeSet::from(["other".into()]),
                maximum_classification: Classification::Public,
            },
        );
        let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
        // Keep the unrelated required anchor pending, independently of collection terminality.
        let EdgeKind::Flow { from, .. } = &mut wave.edges.get_mut("alpha/b").unwrap().kind else {
            panic!()
        };
        from.node = "alpha/alternative-source".into();
        let mut extra = wave.edges["alpha/a"].clone();
        extra.id = "alpha/extra".into();
        if !same_producer {
            let EdgeKind::Flow { from, .. } = &mut extra.kind else {
                panic!()
            };
            from.node = "alpha/alternative-source".into();
        }
        wave.arrivals
            .insert(extra.id.clone(), Resolution::Unresolved);
        wave.input_edges
            .get_mut(&("alpha/merge".into(), "left".into()))
            .unwrap()
            .push(extra.id.clone());
        wave.edges.insert(extra.id.clone(), extra);
        let result = wave.deliver(
            &graph,
            &scope(&["resource"]),
            "alpha/source",
            Some(("out", &Value::String("raw".into()))),
            false,
        );
        if same_producer {
            result.unwrap();
            assert!(matches!(
                wave.arrivals["alpha/a"],
                Resolution::Value(None, _)
            ));
            assert!(matches!(
                wave.arrivals["alpha/extra"],
                Resolution::Value(None, _)
            ));
            assert!(matches!(
                wave.input("alpha/merge").unwrap(),
                Resolution::Unresolved
            ));
            assert_eq!(wave.values, 1);
        } else {
            assert_eq!(result.unwrap_err().code, Code::Denied);
            assert!(matches!(wave.arrivals["alpha/a"], Resolution::Unresolved));
            assert!(matches!(
                wave.arrivals["alpha/extra"],
                Resolution::Unresolved
            ));
        }
    }
}

#[test]
fn collection_pending_candidates_retain_values_and_failure_dominates_in_both_orders() {
    for join in [false, true] {
        for failure_first in [false, true] {
            let graph = readiness_join(InputReadiness::StaticMany { min: 2, max: 2 });
            let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
            let mut extra = wave.edges["alpha/a"].clone();
            extra.id = "alpha/extra".into();
            let EdgeKind::Flow { from, .. } = &mut extra.kind else {
                panic!()
            };
            from.node = "alpha/alternative-source".into();
            wave.arrivals
                .insert(extra.id.clone(), Resolution::Unresolved);
            wave.input_edges
                .get_mut(&("alpha/merge".into(), "left".into()))
                .unwrap()
                .push(extra.id.clone());
            wave.edges.insert(extra.id.clone(), extra);
            if !join {
                let (producer, _) = super::super::tests::fixture();
                wave.nodes.get_mut("alpha/merge").unwrap().1.kind =
                    producer[0].nodes[1].kind.clone();
                wave.nodes
                    .get_mut("alpha/merge")
                    .unwrap()
                    .1
                    .inputs
                    .truncate(1);
                wave.edges.remove("alpha/b");
                wave.arrivals.remove("alpha/b");
            }
            let root = scope(&["resource"]);
            if failure_first {
                wave.deliver(&graph, &root, "alpha/alternative-source", None, true)
                    .unwrap();
                assert!(matches!(
                    wave.input("alpha/merge").unwrap(),
                    Resolution::Unresolved
                ));
            }
            wave.deliver(
                &graph,
                &root,
                "alpha/source",
                Some(("out", &Value::String("raw".into()))),
                false,
            )
            .unwrap();
            if !failure_first {
                assert!(matches!(
                    wave.arrivals["alpha/a"],
                    Resolution::Value(Some(_), _)
                ));
                assert!(matches!(
                    wave.input("alpha/merge").unwrap(),
                    Resolution::Unresolved
                ));
                wave.deliver(&graph, &root, "alpha/alternative-source", None, true)
                    .unwrap();
            }
            assert!(matches!(
                wave.input("alpha/merge").unwrap(),
                Resolution::Failed
            ));
            assert!(!wave.started.contains("alpha/merge"));
        }
    }
}

#[test]
fn optional_and_many_wait_for_terminality_and_preserve_complete_values() {
    for readiness in [
        InputReadiness::Optional,
        InputReadiness::StaticMany { min: 0, max: 2 },
    ] {
        let graph = readiness_join(readiness);
        for present in [false, true] {
            let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
            wave.arrivals.insert(
                "alpha/a".into(),
                if present {
                    Resolution::Value(
                        Some(Value::String("raw".into())),
                        BTreeSet::from(["resource".into()]),
                    )
                } else {
                    Resolution::Closed
                },
            );
            assert!(matches!(
                wave.input("alpha/merge").unwrap(),
                Resolution::Unresolved
            ));
            wave.arrivals.insert(
                "alpha/b".into(),
                Resolution::Value(
                    Some(Value::String("anchor".into())),
                    BTreeSet::from(["resource".into()]),
                ),
            );
            let Resolution::Value(Some(Value::Map(fields)), resources) =
                wave.input("alpha/merge").unwrap()
            else {
                panic!()
            };
            assert_eq!(resources, BTreeSet::from(["resource".into()]));
            assert_eq!(
                fields["left"],
                if present {
                    Value::List(vec![Value::String("raw".into())])
                } else if matches!(readiness, InputReadiness::Optional) {
                    Value::Null
                } else {
                    Value::List(vec![])
                }
            );
        }
    }
}

#[test]
fn absence_intersects_causal_control_and_denies_only_observable_synthesis() {
    let graph = readiness_join(InputReadiness::Optional);
    for authorized in [false, true] {
        for overlap in [false, true] {
            let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
            wave.arrivals.insert("alpha/a".into(), Resolution::Closed);
            wave.closure.insert(
                "alpha/a".into(),
                ClosureEvidence {
                    resources: Some(BTreeSet::from([
                        if overlap { "resource" } else { "other" }.into()
                    ])),
                    authorized,
                    causal: true,
                },
            );
            wave.arrivals.insert(
                "alpha/b".into(),
                Resolution::Value(
                    Some(Value::String("anchor".into())),
                    BTreeSet::from(["resource".into()]),
                ),
            );
            let input = wave.input("alpha/merge");
            assert_eq!(input.is_ok(), authorized);
            if authorized {
                let Resolution::Value(_, resources) = input.unwrap() else {
                    panic!()
                };
                assert_eq!(
                    resources,
                    if overlap {
                        BTreeSet::from(["resource".into()])
                    } else {
                        BTreeSet::new()
                    }
                );
            }
            // A required closure makes optional control unobservable.
            wave.arrivals.insert("alpha/b".into(), Resolution::Closed);
            assert!(matches!(
                wave.input("alpha/merge").unwrap(),
                Resolution::Closed
            ));
            wave.arrivals.insert("alpha/b".into(), Resolution::Failed);
            assert!(matches!(
                wave.input("alpha/merge").unwrap(),
                Resolution::Failed
            ));
        }
    }
}

#[test]
fn closed_delivery_projects_exact_grants_without_eager_access_or_root_restore() {
    for resources in [vec!["resource"], vec!["other"]] {
        let mut graph = readiness_join(InputReadiness::Optional);
        // Isolate runtime projection at a selected-provider boundary.
        graph.routes.insert("alpha/merge".into(), "foreign".into());
        for input in ["left", "right"] {
            graph.policy.delivery.insert(
                (
                    "alpha/source".into(),
                    "out".into(),
                    "alpha/merge".into(),
                    input.into(),
                ),
                FlowGrant {
                    resources: resources.iter().map(|r| (*r).into()).collect(),
                    maximum_classification: Classification::Public,
                },
            );
        }
        let root = scope(&["resource", "other", "root-only"]);
        let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
        wave.deliver(&graph, &root, "alpha/source", None, false)
            .unwrap();
        assert_eq!(
            wave.closure["alpha/a"].resources,
            Some(resources.iter().map(|r| (*r).into()).collect())
        );
        assert!(matches!(
            wave.input("alpha/merge").unwrap(),
            Resolution::Closed
        ));
        // A real retained input now makes the optional absence observable.
        wave.arrivals.insert(
            "alpha/b".into(),
            Resolution::Value(
                Some(Value::String("anchor".into())),
                BTreeSet::from(["resource".into()]),
            ),
        );
        wave.closure.remove("alpha/b");
        assert_eq!(wave.input("alpha/merge").is_ok(), resources == ["resource"]);
        let fresh = Wave::new(&graph, "alpha/alternative").unwrap();
        assert!(fresh.closure.is_empty());
        assert!(fresh.causes.is_empty());
    }
}

#[test]
fn inactive_evidence_is_not_an_activation_bound_and_nested_options_wrap_once() {
    let graph = readiness_join(InputReadiness::Optional);
    let mut wave = Wave::new(&graph, "alpha/alternative").unwrap();
    wave.deliver(&graph, &scope(&["resource"]), "alpha/source", None, false)
        .unwrap();
    assert!(!wave.closure["alpha/a"].causal);
    assert!(wave.closure["alpha/a"].resources.is_none());
    assert!(!wave.input_evidence("alpha/merge").unwrap().causal);
    assert_eq!(
        wave.port_input("alpha/merge", "left").err().unwrap().code,
        Code::Denied
    );
    // The reducer preserves a raw inner None as Some(None), not outer None.
    wave.nodes.get_mut("alpha/merge").unwrap().1.inputs[0].schema =
        phenix_contract::ValueSchema::Option(Box::new(phenix_contract::ValueSchema::Option(
            Box::new(phenix_contract::ValueSchema::String),
        )));
    wave.arrivals.insert(
        "alpha/a".into(),
        Resolution::Value(Some(Value::Null), BTreeSet::from(["resource".into()])),
    );
    wave.closure.remove("alpha/a");
    assert!(
        matches!(wave.port_input("alpha/merge", "left").unwrap(), Resolution::Value(Some(Value::List(values)), _) if values == vec![Value::Null])
    );
}

#[test]
fn static_many_stable_edges_both_cardinality_bounds_and_failure_precedence() {
    let graph = readiness_join(InputReadiness::StaticMany { min: 1, max: 1 });
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    let mut edge = wave.edges["alpha/a"].clone();
    edge.id = "alpha/0-first".into();
    wave.input_edges
        .get_mut(&("alpha/merge".into(), "left".into()))
        .unwrap()
        .insert(0, edge.id.clone());
    wave.edges.insert(edge.id.clone(), edge.clone());
    wave.arrivals
        .insert(edge.id.clone(), Resolution::Unresolved);
    wave.arrivals.insert(
        "alpha/a".into(),
        Resolution::Value(
            Some(Value::String("last".into())),
            BTreeSet::from(["resource".into()]),
        ),
    );
    assert!(matches!(
        wave.port_input("alpha/merge", "left").unwrap(),
        Resolution::Unresolved
    ));
    wave.arrivals.insert(
        edge.id.clone(),
        Resolution::Value(
            Some(Value::String("first".into())),
            BTreeSet::from(["resource".into()]),
        ),
    );
    assert!(matches!(
        wave.port_input("alpha/merge", "left").unwrap(),
        Resolution::Closed
    ));
    wave.nodes.get_mut("alpha/merge").unwrap().1.inputs[0].readiness =
        InputReadiness::StaticMany { min: 0, max: 2 };
    let Resolution::Value(Some(Value::List(values)), _) =
        wave.port_input("alpha/merge", "left").unwrap()
    else {
        panic!()
    };
    assert_eq!(
        values,
        vec![Value::String("first".into()), Value::String("last".into())]
    );
    wave.nodes.get_mut("alpha/merge").unwrap().1.inputs[0].readiness =
        InputReadiness::StaticMany { min: 1, max: 1 };
    wave.arrivals.insert("alpha/a".into(), Resolution::Closed);
    wave.arrivals.insert(edge.id.clone(), Resolution::Closed);
    assert!(matches!(
        wave.port_input("alpha/merge", "left").unwrap(),
        Resolution::Closed
    ));
    wave.arrivals.insert(edge.id, Resolution::Failed);
    assert!(matches!(
        wave.port_input("alpha/merge", "left").unwrap(),
        Resolution::Failed
    ));
}

#[test]
fn discarded_join_delivery_skips_destination_checks_but_not_runnable_delivery() {
    let value = Value::String("authorized producer result".into());
    let root = scope(&["resource"]);
    for sibling_closure in [false, true] {
        for success_first in [false, true] {
            for runnable_target in [false, true] {
                let (mut d, mut p) = super::super::tests::join_fixture();
                let (producer, _) = super::super::tests::fixture();
                d[0].nodes.push(producer[0].nodes[1].clone());
                d[0].edges.push(producer[0].edges[0].clone());
                for (index, edge) in d[0].edges[..2].iter_mut().enumerate() {
                    let EdgeKind::Flow { from, .. } = &mut edge.kind else {
                        panic!()
                    };
                    from.node = if index == 1 || sibling_closure {
                        "alpha/first"
                    } else {
                        "alpha/source"
                    }
                    .into();
                    from.port = if index == 1 {
                        "ok"
                    } else if sibling_closure {
                        "err"
                    } else {
                        "out"
                    }
                    .into();
                    edge.id = if (index == 1) == success_first {
                        "alpha/a"
                    } else {
                        "alpha/b"
                    }
                    .into();
                }
                for (port, input) in [("ok", "right"), ("err", "left")] {
                    p.delivery.insert(
                        (
                            "alpha/first".into(),
                            port.into(),
                            "alpha/merge".into(),
                            input.into(),
                        ),
                        FlowGrant {
                            resources: BTreeSet::from(["other".into()]),
                            maximum_classification: Classification::Public,
                        },
                    );
                }
                if runnable_target {
                    let (merge, _) = super::super::tests::merge_fixture();
                    let mut sink = merge[0]
                        .nodes
                        .iter()
                        .find(|n| n.id == "alpha/sink")
                        .unwrap()
                        .clone();
                    sink.id = "alpha/runnable".into();
                    d[0].nodes.push(sink);
                    d[0].edges.push(EdgeDecl {
                        id: "alpha/z-runnable".into(),
                        kind: EdgeKind::Flow {
                            from: OutPortRef {
                                node: "alpha/first".into(),
                                port: "ok".into(),
                            },
                            to: InPortRef {
                                node: "alpha/runnable".into(),
                                port: "in".into(),
                            },
                        },
                    });
                    p.delivery.insert(
                        (
                            "alpha/first".into(),
                            "ok".into(),
                            "alpha/runnable".into(),
                            "in".into(),
                        ),
                        FlowGrant {
                            resources: BTreeSet::from(["other".into()]),
                            maximum_classification: Classification::Public,
                        },
                    );
                }
                let graph = super::super::tests::run(&d, &p).unwrap();
                let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
                wave.started.insert("alpha/first".into());
                wave.resources
                    .insert("alpha/first".into(), root.authority.resources.clone());
                if !sibling_closure {
                    wave.deliver(&graph, &root, "alpha/source", None, false)
                        .unwrap();
                    assert!(wave.join_non_runnable("alpha/merge").unwrap());
                    assert!(!wave.done.contains("alpha/first"));
                }
                // Only the produced value fits: a discarded delivery must cost
                // neither a second value nor a second copy of the payload bytes.
                wave.values = MAX_ROOT_VALUES - 1;
                wave.bytes = MAX_ROOT_BYTES - value.encode().unwrap().len();
                let result = wave.complete(
                    &graph,
                    &root,
                    "alpha/first",
                    &Outcome::Success(value.clone()),
                );
                let success_edge = if success_first { "alpha/a" } else { "alpha/b" };
                assert!(matches!(
                    wave.arrivals[success_edge],
                    Resolution::Value(None, _)
                ));
                assert_eq!(wave.values, MAX_ROOT_VALUES);
                assert_eq!(wave.bytes, MAX_ROOT_BYTES);
                if runnable_target {
                    assert_eq!(result.unwrap_err().code, Code::Denied);
                    assert!(!wave.done.contains("alpha/first"));
                    assert!(matches!(
                        wave.input("alpha/runnable").unwrap(),
                        Resolution::Unresolved
                    ));
                } else {
                    assert!(!result.unwrap());
                    assert!(wave.done.contains("alpha/first"));
                    assert!(matches!(
                        wave.input("alpha/merge").unwrap(),
                        Resolution::Closed
                    ));
                    assert!(matches!(wave.arrivals["alpha/c"], Resolution::Unresolved));
                    // Producer-level accounting remains enforced even if every
                    // destination discards the completion.
                    let mut over_budget = Wave::new(&graph, "alpha/submit").unwrap();
                    over_budget.values = MAX_ROOT_VALUES;
                    assert_eq!(
                        over_budget
                            .complete(
                                &graph,
                                &root,
                                "alpha/first",
                                &Outcome::Success(value.clone())
                            )
                            .unwrap_err()
                            .code,
                        Code::InfrastructureFailure
                    );
                }
            }
        }
    }
}

#[test]
fn join_delivery_releases_success_payloads_in_both_orders_and_when_dormant() {
    let (mut d, p) = super::super::tests::join_fixture();
    d[0].nodes
        .iter_mut()
        .find(|n| n.id == "alpha/alternative-source")
        .unwrap()
        .kind = NodeKind::Source {
        source: "alpha/submit".into(),
    };
    let EdgeKind::Flow { from, .. } = &mut d[0].edges[1].kind else {
        panic!()
    };
    from.node = "alpha/alternative-source".into();
    let graph = super::super::tests::run(&d, &p).unwrap();
    let root = scope(&["resource", "other"]);
    let value = Value::String("retained only while runnable".repeat(100));
    for reverse in [false, true] {
        for failed in [false, true] {
            let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
            wave.started.insert("alpha/alternative-source".into());
            let mut deliveries = [
                ("alpha/source", None, false),
                (
                    "alpha/alternative-source",
                    (!failed).then_some(("out", &value)),
                    failed,
                ),
            ];
            if reverse {
                deliveries.reverse();
            }
            for (index, (node, selected, failed)) in deliveries.into_iter().enumerate() {
                wave.deliver(&graph, &root, node, selected, failed).unwrap();
                if index == 0 {
                    assert!(matches!(
                        wave.input("alpha/merge").unwrap(),
                        Resolution::Unresolved
                    ));
                    assert!(matches!(wave.arrivals["alpha/c"], Resolution::Unresolved));
                    if !reverse {
                        assert!(wave.started.contains("alpha/alternative-source"));
                        assert!(!wave.done.contains("alpha/alternative-source"));
                    } else if !failed {
                        assert!(matches!(
                            wave.arrivals["alpha/b"],
                            Resolution::Value(Some(_), _)
                        ));
                    }
                }
            }
            assert!(wave.join_non_runnable("alpha/merge").unwrap());
            if failed {
                assert!(matches!(
                    wave.input("alpha/merge").unwrap(),
                    Resolution::Failed
                ));
                assert!(matches!(wave.arrivals["alpha/b"], Resolution::Failed));
            } else {
                assert!(matches!(
                    wave.input("alpha/merge").unwrap(),
                    Resolution::Closed
                ));
                assert!(
                    matches!(&wave.arrivals["alpha/b"], Resolution::Value(None, resources) if resources == &root.authority.resources)
                );
            }
            assert!(matches!(wave.arrivals["alpha/a"], Resolution::Closed));
            assert!(!wave.started.contains("alpha/merge"));
        }
    }
    // A missing candidate is genuinely Closed from the beginning of the wave.
    d[0].edges.remove(0);
    let graph = super::super::tests::run(&d, &p).unwrap();
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    assert!(wave.join_non_runnable("alpha/merge").unwrap());
    wave.deliver(
        &graph,
        &root,
        "alpha/alternative-source",
        Some(("out", &value)),
        false,
    )
    .unwrap();
    assert!(matches!(
        wave.arrivals["alpha/b"],
        Resolution::Value(None, _)
    ));
    assert!(matches!(
        wave.input("alpha/merge").unwrap(),
        Resolution::Closed
    ));
}

#[test]
fn released_payload_on_runnable_consumer_is_an_invariant_failure() {
    let (d, p) = super::super::tests::join_fixture();
    let graph = super::super::tests::run(&d, &p).unwrap();
    for (edge, node) in [("alpha/a", "alpha/merge"), ("alpha/c", "alpha/sink")] {
        let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
        wave.arrivals
            .insert(edge.into(), Resolution::Value(None, BTreeSet::new()));
        assert_eq!(
            wave.input(node).err().unwrap().code,
            Code::InfrastructureFailure
        );
    }
}

#[test]
fn join_terminal_reduction_all_outcomes_orders_and_resource_intersection() {
    let (d, p) = super::super::tests::join_fixture();
    let graph = super::super::tests::run(&d, &p).unwrap();
    let root = scope(&["resource", "left-only", "right-only"]);
    for left in 0..3 {
        for right in 0..3 {
            for reverse in [false, true] {
                for overlap in [false, true] {
                    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
                    let mut candidates = [
                        ("alpha/a", left, "left-only"),
                        ("alpha/b", right, "right-only"),
                    ];
                    if reverse {
                        candidates.reverse();
                    }
                    for (index, (edge, state, resource)) in candidates.into_iter().enumerate() {
                        let mut resources = BTreeSet::from([resource.into()]);
                        if overlap {
                            resources.insert("resource".into());
                        }
                        wave.arrivals.insert(
                            edge.into(),
                            match state {
                                0 => Resolution::Closed,
                                1 => Resolution::Value(Some(Value::String(edge.into())), resources),
                                _ => Resolution::Failed,
                            },
                        );
                        if index == 0 {
                            for _ in 0..3 {
                                assert_eq!(
                                    wave.join_non_runnable("alpha/merge").unwrap(),
                                    state == 0
                                );
                                assert!(matches!(
                                    wave.input("alpha/merge").unwrap(),
                                    Resolution::Unresolved
                                ));
                                assert!(matches!(wave.arrivals["alpha/c"], Resolution::Unresolved));
                                assert!(!wave.started.contains("alpha/merge"));
                                assert!(!wave.done.contains("alpha/merge"));
                            }
                        }
                    }
                    assert_eq!(
                        wave.join_non_runnable("alpha/merge").unwrap(),
                        left == 0 || right == 0
                    );
                    match wave.input("alpha/merge").unwrap() {
                        Resolution::Failed => {
                            assert!(left == 2 || right == 2);
                            wave.deliver(&graph, &root, "alpha/merge", None, true)
                                .unwrap();
                            assert!(matches!(
                                wave.input("alpha/sink").unwrap(),
                                Resolution::Failed
                            ));
                        }
                        Resolution::Closed => {
                            assert!(left == 0 || right == 0);
                            assert!(left != 2 && right != 2);
                            wave.deliver(&graph, &root, "alpha/merge", None, false)
                                .unwrap();
                            assert!(matches!(
                                wave.input("alpha/sink").unwrap(),
                                Resolution::Closed
                            ));
                        }
                        Resolution::Value(Some(value), resources) => {
                            assert_eq!((left, right), (1, 1));
                            assert_eq!(
                                value,
                                Value::map([
                                    ("left", Value::String("alpha/a".into())),
                                    ("right", Value::String("alpha/b".into())),
                                ])
                            );
                            let expected = if overlap {
                                BTreeSet::from(["resource".into()])
                            } else {
                                BTreeSet::new()
                            };
                            assert_eq!(resources, expected);
                            wave.route_intrinsic(
                                &graph,
                                &root,
                                &wave.nodes["alpha/merge"].1.clone(),
                                &value,
                                resources,
                            )
                            .unwrap();
                            assert!(
                                matches!(wave.input("alpha/sink").unwrap(), Resolution::Value(Some(v), r) if v == value && r == expected)
                            );
                        }
                        Resolution::Unresolved => panic!("all candidates terminal"),
                        Resolution::Value(None, _) => panic!("runnable payload released"),
                    }
                    assert!(!wave.started.contains("alpha/merge"));
                }
            }
        }
    }
}

#[test]
fn join_closed_input_waits_for_active_domain_failure_without_cancelling_producer() {
    let (mut d, mut p) = super::super::tests::join_fixture();
    let (producer, producer_policy) = super::super::tests::fixture();
    let mut invoke = producer[0].nodes[1].clone();
    invoke.execution.on_domain_error = DomainErrorPolicy::SkipDependents;
    d[0].nodes.push(invoke);
    let EdgeKind::Flow { from, .. } = &mut d[0].edges[1].kind else {
        panic!()
    };
    from.node = "alpha/first".into();
    from.port = "ok".into();
    d[0].edges.push(producer[0].edges[0].clone());
    p.operations = producer_policy.operations;
    p.delivery.extend(producer_policy.delivery);
    p.delivery.insert(
        (
            "alpha/first".into(),
            "ok".into(),
            "alpha/merge".into(),
            "right".into(),
        ),
        FlowGrant {
            resources: BTreeSet::from(["resource".into()]),
            maximum_classification: Classification::Public,
        },
    );
    let graph = super::super::tests::run(&d, &p).unwrap();
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    wave.started.insert("alpha/first".into());
    wave.arrivals.insert("alpha/a".into(), Resolution::Closed);
    assert!(wave.join_non_runnable("alpha/merge").unwrap());
    assert!(matches!(
        wave.input("alpha/merge").unwrap(),
        Resolution::Unresolved
    ));
    assert!(wave.started.contains("alpha/first"));
    assert!(!wave.done.contains("alpha/first"));
    assert!(!wave
        .complete(
            &graph,
            &scope(&["resource"]),
            "alpha/first",
            &Outcome::Domain(Value::String("domain".into()))
        )
        .unwrap());
    assert!(wave.join_non_runnable("alpha/merge").unwrap());
    assert!(matches!(
        wave.input("alpha/merge").unwrap(),
        Resolution::Failed
    ));
}

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
            let Resolution::Value(Some(payload), resources) = wave.input(id).unwrap() else {
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
                matches!(wave.input("beta/sink").unwrap(), Resolution::Value(Some(payload), resources) if payload == value && resources == BTreeSet::from(["a".into()]))
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
                                Some(value.clone()),
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
                    let Resolution::Value(Some(payload), resources) = result.unwrap() else {
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
                        matches!(wave.input("alpha/sink").unwrap(), Resolution::Value(Some(v), r) if v == value && r == BTreeSet::from(["resource".into()]))
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
                matches!(wave.input("alpha/merge").unwrap(), Resolution::Value(Some(Value::String(v)), _) if v == source)
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
            matches!(wave.input(sink).unwrap(), Resolution::Value(Some(value), _) if value == expected)
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
            matches!(wave.input("beta/second").unwrap(), Resolution::Value(Some(value), _) if value == payload)
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
    // Model the scope retained when pump_v2 selects this invocation.
    wave.resources
        .insert("alpha/first".into(), BTreeSet::from(["resource".into()]));
    wave.complete(
        &graph,
        &scope(&["resource"]),
        "alpha/first",
        &Outcome::Domain(Value::String("actual error".into())),
    )
    .unwrap();
    assert!(
        matches!(wave.input("beta/second").unwrap(), Resolution::Value(Some(Value::String(v)), _) if v == "actual error")
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
    let Resolution::Value(_, resources) = wave.input("alpha/first").unwrap() else {
        panic!()
    };
    wave.resources.insert("alpha/first".into(), resources);
    wave.complete(&graph, &scope, "alpha/first", &output)
        .unwrap();
    assert!(
        matches!(&wave.arrivals["alpha/next"], Resolution::Value(Some(Value::String(v)), _) if v == "actual producer")
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
