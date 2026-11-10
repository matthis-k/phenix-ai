use super::*;

fn writes(g: &GraphBuilder, edit: impl FnOnce(&mut GraphBuilder)) -> Vec<Requirement> {
    let pending = g.pending.borrow().clone();
    let mut after = g.clone();
    edit(&mut after);
    observe(g, &pending, &after).required
}
fn fixture() -> GraphBuilder {
    let mut g = GraphBuilder::new("root");
    let n = g.node("worker", |x: i64| x).unwrap();
    let input = g.expose_input("request", n.inlet()).unwrap();
    g.expose_output("result", n.output()).unwrap();
    g.trigger("run", [input]).unwrap();
    g
}
fn contract(schema: &str) -> GraphContract {
    GraphContract::new(
        "test.contract",
        InterfaceShape::new()
            .input_slot(
                "run",
                "value",
                DeclaredValue {
                    schema: schema.into(),
                    local_type: Some(TypeId::of::<i64>()),
                },
            )
            .unwrap()
            .output(
                "value",
                DeclaredValue {
                    schema: schema.into(),
                    local_type: Some(TypeId::of::<i64>()),
                },
            )
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn empty_and_repeated_clone_are_noops_with_pending_and_dependencies() {
    let mut g = fixture();
    let mut child = GraphBuilder::new("child");
    child.node("n", |x: i64| x).unwrap();
    g.instance("alias", &child.module()).unwrap();
    assert!(writes(&g, |_| {}).is_empty());
    assert!(
        writes(&g, |g| {
            *g = g.clone();
            *g = g.clone();
        })
        .is_empty()
    );
    assert!(
        writes(&g, |g| {
            let origin = GraphOrigin::default();
            g.with_origin(&origin, |g| g.with_origin(&origin.child(), |_| ()));
            let result: Result<(), ()> = g.additions(|g| {
                g.node("discarded", |x: i64| x).unwrap();
                Err(())
            });
            assert!(result.is_err());
        })
        .is_empty()
    );
}

#[test]
fn graph_fallback_needs_explicit_graph_even_with_nonempty_narrow_claims() {
    for replacement in [false, true] {
        for claims in [
            vec![],
            vec![PatchTarget::Node("unrelated".into())],
            vec![
                PatchTarget::Node("worker".into()),
                PatchTarget::PublicInput("request".into()),
                PatchTarget::PublicOutput("result".into()),
                PatchTarget::Trigger("run".into()),
            ],
            vec![PatchTarget::Graph],
        ] {
            let graph = claims.contains(&PatchTarget::Graph);
            let mut assembly = GraphAssembly::from_builder(fixture());
            let mut planner = PatchPlanner::new();
            planner
                .add_patch(PatchBuilder::custom(
                    PatchSpec::new("edit").writes_all(claims),
                    move |g| {
                        if replacement {
                            *g = GraphBuilder::new("other");
                        } else {
                            *g = g.clone().limits(CompileLimits {
                                max_depth: 7,
                                ..CompileLimits::default()
                            });
                        }
                        Ok(())
                    },
                ))
                .unwrap();
            let result = planner.apply(&mut assembly);
            if graph {
                assert!(result.is_ok(), "{result:?}");
            } else {
                assert_eq!(
                    result,
                    Err(PatchError::UndeclaredWrite {
                        patch: "edit".into(),
                        target: Box::new(PatchTarget::Graph)
                    })
                );
            }
        }
    }
}

#[test]
fn coverage_failure_discards_earlier_selected_contribution_and_preserves_store() {
    let mut assembly = GraphAssembly::from_builder(fixture());
    let store = Rc::clone(&assembly.builder.as_ref().unwrap().pending);
    let before = assembly.builder.as_ref().unwrap().clone();
    let mut planner = PatchPlanner::new();
    let mut earlier = PatchBuilder::new("earlier");
    earlier.node("extra", |x: i64| x);
    planner.add_patch(earlier).unwrap();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("later")
                .after("earlier")
                .writes(PatchTarget::Node("unrelated".into())),
            |g| {
                g.compile_limits.max_work += 1;
                Ok(())
            },
        ))
        .unwrap();
    assert!(matches!(
        planner.apply(&mut assembly),
        Err(PatchError::UndeclaredWrite { .. })
    ));
    let original = assembly.builder.as_ref().unwrap();
    assert!(Rc::ptr_eq(&store, &original.pending));
    assert!(
        observe(&before, &before.pending.borrow(), original)
            .required
            .is_empty()
    );
    assert!(assembly.revisions.is_empty());
    assert!(assembly.contributors.is_empty());
    assembly.contribute("earlier", |_| Ok(())).unwrap();
    assembly.contribute("later", |_| Ok(())).unwrap();
}

#[test]
fn local_contract_schema_text_is_stored_metadata_not_semantic_equality() {
    let mut g = fixture();
    let old = contract("old text");
    let new = contract("new text");
    assert_eq!(old, new);
    assert!(!interfaces::stored_contract_eq(&old, &new));
    g.module.nodes[0].claims.push(old.clone());
    assert_eq!(
        writes(&g, |g| g.module.nodes[0].claims[0] = new.clone()),
        [Requirement::Node("worker".into())]
    );
    g.module.claims.push(old.clone());
    assert!(writes(&g, |g| g.module.claims[0] = new.clone()).contains(&Requirement::Graph));
    g.module.contract_requests.push(("worker".into(), old));
    assert!(writes(&g, |g| g.module.contract_requests[0].1 = new).contains(&Requirement::Graph));
}

#[test]
fn pending_presence_hidden_raw_and_effective_payload_are_all_writes() {
    let g = fixture();
    for edit in [
        (|g: &mut GraphBuilder| {
            g.pending.borrow_mut().remove("worker");
        }) as fn(&mut GraphBuilder),
        |g| {
            g.pending
                .borrow_mut()
                .get_mut("worker")
                .unwrap()
                .output
                .schema = "diagnostic".into();
        },
        |g| {
            g.module.nodes[0].output.schema = "hidden raw".into();
        },
    ] {
        assert_eq!(writes(&g, edit), [Requirement::Node("worker".into())]);
    }
    assert!(
        writes(&g, |g| {
            let n = g.module.nodes[0].clone();
            g.pending.borrow_mut().insert("orphan".into(), n);
        })
        .contains(&Requirement::Graph)
    );
}

#[test]
fn aliases_include_before_only_and_identity_only_owners_not_unrelated() {
    let mut child = GraphBuilder::new("child");
    child.node("n", |x: i64| x).unwrap();
    let package = child.module();
    let mut g = fixture();
    g.instance("a", &package).unwrap();
    g.instance("b", &package).unwrap();
    let unrelated = GraphBuilder::new("unrelated").module();
    g.instance("other", &unrelated).unwrap();
    let expected = vec![
        Requirement::Instance("a".into()),
        Requirement::Instance("b".into()),
    ];
    assert_eq!(
        writes(&g, |g| {
            *g.identities.get_mut("child").unwrap() += 1;
        }),
        expected
    );
    assert_eq!(
        writes(&g, |g| {
            g.dependencies.get_mut("child").unwrap().definition.nodes[0]
                .output
                .schema = "changed".into();
        }),
        expected
    );
    assert_eq!(
        writes(&g, |g| {
            g.module.instances.retain(|i| i.name != "a");
            g.module.declaration_ids.remove("a");
            *g.identities.get_mut("child").unwrap() += 1;
        }),
        expected
    );
    assert!(
        writes(&g, |g| {
            g.identities.insert("orphan".into(), 1);
        })
        .contains(&Requirement::Graph)
    );
}

#[test]
fn same_name_routes_tokens_and_bookkeeping_are_observed() {
    let g = fixture();
    assert_eq!(
        writes(&g, |g| g.module.inputs[0].schema = "changed".into()),
        [Requirement::PublicInput("request".into())]
    );
    assert_eq!(
        writes(&g, |g| g.module.output.as_mut().unwrap().schema =
            "changed".into()),
        [Requirement::PublicOutput("result".into())]
    );
    assert_eq!(
        writes(&g, |g| g.module.triggers[0]
            .targets
            .push(ModuleTriggerTarget::Source("worker".into()))),
        [Requirement::Trigger("run".into())]
    );
    assert_eq!(
        writes(&g, |g| {
            *g.public_input_ids.get_mut("request").unwrap() += 1;
        }),
        [Requirement::PublicInput("request".into())]
    );
    for edit in [
        (|g: &mut GraphBuilder| {
            g.module.next_node_id += 1;
        }) as fn(&mut GraphBuilder),
        |g| {
            g.module.next_connection += 1;
        },
        |g| {
            g.module.node_names.clear();
        },
        |g| {
            g.module.node_ids.clear();
        },
        |g| {
            g.module.scope += 1;
        },
        |g| {
            g.module.id.push('x');
        },
        |g| {
            g.compile_limits.max_depth += 1;
        },
        |g| {
            g.compile_limits.max_instances += 1;
        },
        |g| {
            g.compile_limits.max_work += 1;
        },
    ] {
        assert!(writes(&g, edit).contains(&Requirement::Graph));
    }
}

#[test]
fn node_payload_matrix_preserves_opaque_factory_but_observes_every_leaf() {
    let mut g = fixture();
    g.module.nodes[0].grants.push(ResourceNeed {
        key: "resource",
        access: Access::Read,
    });
    let edits: &[fn(&mut NodeTemplate)] = &[
        |n| n.composition_origin = Some(GraphOrigin::default()),
        |n| n.claims.push(contract("diagnostic")),
        |n| n.id.0 += 1,
        |n| n.name = Some("renamed".into()),
        |n| n.declaring_scope = Some(0),
        |n| n.declaration_id += 1,
        |n| n.ports[0].id.0 += 1,
        |n| n.ports[0].name = Some("other".into()),
        |n| n.ports[0].slots[0].id.0 += 1,
        |n| n.ports[0].slots[0].name = Some("other".into()),
        |n| n.ports[0].slots[0].schema = "changed local text".into(),
        |n| n.ports[0].slots[0].local_type = Some(TypeId::of::<bool>()),
        |n| {
            n.ports[0].slots[0].buffer = match n.ports[0].slots[0].buffer {
                BufferMode::Immediate => BufferMode::Deferred,
                BufferMode::Deferred => BufferMode::Immediate,
            }
        },
        |n| n.ports[0].args = NodeTemplate::function(|x: i64| x).ports[0].args.clone(),
        |n| {
            n.ports[0].policy = NodeTemplate::local_function(|x: i64| x).ports[0]
                .policy
                .clone()
        },
        |n| n.output.id.0 += 1,
        |n| n.output.name = Some("other".into()),
        |n| n.output.schema = "changed local text".into(),
        |n| n.output.local_type = Some(TypeId::of::<bool>()),
        |n| n.grants[0].key = "other",
        |n| n.grants[0].access = Access::Write,
        |n| n.cohort_mode = CohortMode::FreshPerInvocation,
        |n| n.logic_factory = NodeTemplate::local_function(|x: i64| x).logic_factory,
    ];
    for (index, edit) in edits.iter().enumerate() {
        let before = &g.module.nodes[0];
        let mut after = before.clone();
        edit(&mut after);
        assert!(!node(before, &after), "unobserved field case {index}");
        // Pending payload does not alter the raw registration indexes.
        let required = writes(&g, |g| {
            let mut pending = g.pending.borrow_mut();
            let n = pending.get_mut("worker").unwrap();
            n.grants = before.grants.clone();
            edit(n);
        });
        assert!(
            required.contains(&Requirement::Node("worker".into())),
            "case {index}"
        );
    }
}

#[test]
fn owned_import_and_provenance_remain_precise_but_historical_import_is_global() {
    let mut child = GraphBuilder::new("child");
    child.node("n", |x: i64| x).unwrap();
    let package = child.module();
    let g = fixture();
    assert_eq!(
        writes(&g, |g| {
            g.with_origin(&GraphOrigin::default(), |g| g.instance("alias", &package))
                .unwrap();
        }),
        [Requirement::Instance("alias".into())]
    );
    assert!(
        writes(&g, |g| {
            g.instance("alias", &package).unwrap();
            g.module.instances.clear();
            g.module.declaration_ids.clear();
        })
        .contains(&Requirement::Graph)
    );
    let mut aliased = g.clone();
    aliased.instance("a", &package).unwrap();
    aliased.instance("b", &package).unwrap();
    assert_eq!(
        writes(&aliased, |g| {
            g.module.instances.clear();
            g.module.declaration_ids.clear();
            g.dependencies.remove("child");
            g.identities.remove("child");
        }),
        [
            Requirement::Instance("a".into()),
            Requirement::Instance("b".into())
        ]
    );
}

#[test]
fn edges_preserve_record_identity_cursor_and_append_order() {
    let mut g = fixture();
    g.node("sink", |x: i64| x).unwrap();
    g.connect("worker", "sink").unwrap();
    let key = patch_connection_key(&g.module.edges[0].from, &g.module.edges[0].to);
    assert_eq!(
        writes(&g, |g| {
            g.module.edges.clear();
        }),
        [Requirement::Connection(key.clone())]
    );
    assert_eq!(
        writes(&g, |g| {
            let e = g.module.edges.pop().unwrap();
            g.module.connect(e.from, e.to);
        }),
        [Requirement::Connection(key)]
    );
    assert!(
        writes(&g, |g| {
            let e = g.module.edges[0].clone();
            g.module.connect(e.from, e.to);
            g.module.edges.reverse();
        })
        .contains(&Requirement::Graph)
    );
    assert!(
        writes(&g, |g| {
            let e = g.module.edges[0].clone();
            g.module.connect(e.from, e.to);
            g.module.edges.pop();
        })
        .contains(&Requirement::Graph)
    );
    assert!(
        writes(&g, |g| g.module.edges.push(g.module.edges[0].clone()))
            .contains(&Requirement::Graph)
    );
}

#[test]
fn failed_callback_and_panic_discard_candidate_pending_and_earlier_edits() {
    for panic in [false, true] {
        let mut original = GraphBuilder::new("root");
        let handle = original.node("worker", |x: i64| x).unwrap();
        let endpoint = handle.inlet();
        let mut assembly = GraphAssembly::from_builder(original);
        let before = assembly.builder.as_ref().unwrap().clone();
        let store = Rc::clone(&assembly.builder.as_ref().unwrap().pending);
        let mut planner = PatchPlanner::new();
        planner
            .add_patch(PatchBuilder::custom(
                PatchSpec::new("earlier").writes(PatchTarget::Graph),
                |g| {
                    let n = g.node("extra", |x: i64| x).unwrap();
                    let input = g.expose_input("request", n.inlet()).unwrap();
                    g.trigger("run", [input])?;
                    Ok(())
                },
            ))
            .unwrap();
        planner
            .add_patch(PatchBuilder::custom(
                PatchSpec::new("later")
                    .after("earlier")
                    .writes(PatchTarget::Graph),
                move |g| {
                    g.node("candidate", |x: i64| x)
                        .unwrap()
                        .input(Layout::atomic())
                        .unwrap();
                    if panic {
                        panic!("candidate failure");
                    }
                    Err("candidate failure".into())
                },
            ))
            .unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            planner.apply(&mut assembly)
        }));
        if panic {
            assert!(result.is_err());
        } else {
            assert!(matches!(result, Ok(Err(PatchError::Apply { .. }))));
        }
        let original = assembly.builder.as_mut().unwrap();
        assert!(Rc::ptr_eq(&store, &original.pending));
        assert!(
            observe(&before, &before.pending.borrow(), original)
                .required
                .is_empty()
        );
        original.expose_input("still-valid", endpoint).unwrap();
        handle.input(Layout::atomic()).unwrap();
        assert!(assembly.revisions.is_empty());
        assert!(assembly.contributors.is_empty());
        assembly.contribute("earlier", |_| Ok(())).unwrap();
        assembly.contribute("later", |_| Ok(())).unwrap();
    }
}

#[test]
fn lift_does_not_mask_unrelated_graph_difference() {
    let c = contract("i64");
    let mut child = GraphBuilder::new("child");
    let n = child.node("n", |x: i64| x).unwrap();
    child.expose_input("run", n.inlet()).unwrap();
    child.expose_output("value", n.output()).unwrap();
    child.implements_public(&c).unwrap();
    child.require_contract("", &c).unwrap();
    let mut g = GraphBuilder::new("root");
    g.instance("alias", &child.module()).unwrap();
    let mut replacement = NodeTemplate::local_function(|x: i64| x);
    replacement.claims.push(c);
    let mut assembly = GraphAssembly::from_builder(g);
    let mut planner = PatchPlanner::new();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("replace")
                .writes(PatchTarget::Node("alias".into()))
                .writes(PatchTarget::IncidentConnections("alias".into())),
            move |g| {
                g.replace("alias", replacement)?;
                g.compile_limits.max_work += 1;
                Ok(())
            },
        ))
        .unwrap();
    assert_eq!(
        planner.apply(&mut assembly),
        Err(PatchError::UndeclaredWrite {
            patch: "replace".into(),
            target: Box::new(PatchTarget::Graph)
        })
    );
}

#[test]
fn underdeclared_global_edit_cannot_bypass_priority_resolution() {
    for honest in [false, true] {
        let mut assembly = GraphAssembly::from_builder(fixture());
        let mut planner = PatchPlanner::new();
        let mut earlier = PatchBuilder::new("earlier");
        earlier.priority(10);
        earlier.node("extra", |x: i64| x);
        planner.add_patch(earlier).unwrap();
        let ran = Rc::new(RefCell::new(false));
        let flag = Rc::clone(&ran);
        planner
            .add_patch(PatchBuilder::custom(
                PatchSpec::new("later").priority(1).writes(if honest {
                    PatchTarget::Graph
                } else {
                    PatchTarget::Node("unrelated".into())
                }),
                move |g| {
                    *flag.borrow_mut() = true;
                    g.compile_limits.max_work += 1;
                    Ok(())
                },
            ))
            .unwrap();
        let resolution = planner.resolve().unwrap();
        if honest {
            assert_eq!(resolution.selected, ["earlier"]);
            assert_eq!(resolution.superseded.len(), 1);
            planner.apply(&mut assembly).unwrap();
            assert!(!*ran.borrow());
        } else {
            assert_eq!(resolution.selected, ["earlier", "later"]);
            assert_eq!(
                planner.apply(&mut assembly),
                Err(PatchError::UndeclaredWrite {
                    patch: "later".into(),
                    target: Box::new(PatchTarget::Graph)
                })
            );
            assert!(*ran.borrow());
            assert!(assembly.revisions.is_empty());
            assert!(
                !assembly
                    .builder
                    .as_ref()
                    .unwrap()
                    .module
                    .node_names
                    .contains_key("extra")
            );
        }
    }
}

#[test]
fn exact_request_lifts_do_not_absorb_prefix_metadata_or_extra_requests() {
    let c = contract("local text");
    let mut child = GraphBuilder::new("child");
    let n = child.node("n", |x: i64| x).unwrap();
    child.expose_input("run", n.inlet()).unwrap();
    child.expose_output("value", n.output()).unwrap();
    child.implements_public(&c).unwrap();
    child.require_contract("", &c).unwrap();
    child.require_contract("", &c).unwrap();
    let mut g = GraphBuilder::new("root");
    g.instance("a", &child.module()).unwrap();
    // Reuse one package identity across aliases.
    let package = g.dependencies.get("child").unwrap().clone();
    let mut second = g.clone();
    second.module.try_instance("b", "child").unwrap();
    assert!(entry(&package, &second.dependencies["child"]));
    g = second;
    g.require_contract("unrelated", &c).unwrap();
    let mut after = g.clone();
    for name in ["b", "a"] {
        let mut replacement = NodeTemplate::local_function(|x: i64| x);
        replacement.claims.push(c.clone());
        after.replace(name, replacement).unwrap();
    }
    let pending = g.pending.borrow().clone();
    assert_eq!(
        observe(&g, &pending, &after).required,
        [
            Requirement::Node("a".into()),
            Requirement::Node("b".into()),
            Requirement::Instance("a".into()),
            Requirement::Instance("b".into()),
        ]
    );
    let edits: &[fn(&mut GraphBuilder)] = &[
        |g| {
            g.module.contract_requests.pop();
        },
        |g| {
            let extra = g.module.contract_requests.last().unwrap().clone();
            g.module.contract_requests.push(extra);
        },
        |g| {
            g.module.contract_requests[0].0 = "other".into();
        },
        |g| {
            g.module.contract_requests[1].1 = contract("different stored local text");
        },
        |g| {
            g.module.claims.push(contract("local text"));
        },
        |g| {
            g.compile_limits.max_work += 1;
        },
    ];
    for (index, edit) in edits.iter().enumerate() {
        let mut changed = after.clone();
        edit(&mut changed);
        assert!(
            observe(&g, &pending, &changed)
                .required
                .contains(&Requirement::Graph),
            "case {index}"
        );
    }
}

#[test]
fn captured_original_store_mutation_is_explicitly_outside_candidate_rollback() {
    let mut original = GraphBuilder::new("root");
    let handle = original.node("worker", |x: i64| x).unwrap();
    let before = original.pending.borrow()["worker"].declaration_id;
    let mut assembly = GraphAssembly::from_builder(original);
    let mut planner = PatchPlanner::new();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("external"),
            move |_| {
                handle.input(Layout::atomic()).unwrap();
                Err("external effects cannot be rolled back".into())
            },
        ))
        .unwrap();
    assert!(matches!(
        planner.apply(&mut assembly),
        Err(PatchError::Apply { .. })
    ));
    assert_ne!(
        assembly.builder.as_ref().unwrap().pending.borrow()["worker"].declaration_id,
        before
    );
    assert!(assembly.revisions.is_empty());
}

#[test]
fn cross_kind_export_residuals_keep_public_owners() {
    let mut child = GraphBuilder::new("child");
    let n = child.node("n", |x: i64| x).unwrap();
    child.expose_input("run", n.inlet()).unwrap();
    child.expose_output("value", n.output()).unwrap();
    let package = Rc::new(child.module());
    type ResidualEdit = (fn(&mut GraphBuilder), Option<PatchTarget>);
    let edits: &[ResidualEdit] = &[
        (|_| {}, None),
        (
            |g| {
                g.module.inputs[0]
                    .targets
                    .push(ModuleInputTarget::NodeNamed {
                        node: "extra".into(),
                        port: "run".into(),
                        slot: "value".into(),
                    });
            },
            Some(PatchTarget::PublicInput("request".into())),
        ),
        (
            |g| g.module.inputs[0].targets.reverse(),
            Some(PatchTarget::PublicInput("request".into())),
        ),
        (
            |g| g.module.inputs[0].schema = "changed input metadata".into(),
            Some(PatchTarget::PublicInput("request".into())),
        ),
        (
            |g| *g.public_input_ids.get_mut("request").unwrap() += 1,
            Some(PatchTarget::PublicInput("request".into())),
        ),
        (
            |g| g.module.output.as_mut().unwrap().schema = "changed output metadata".into(),
            Some(PatchTarget::PublicOutput("result".into())),
        ),
    ];
    for to_module in [true, false] {
        let mut g = fixture();
        g.node("sink", |x: i64| x).unwrap();
        g.node("extra", |x: i64| x).unwrap();
        g.module.inputs[0]
            .targets
            .push(ModuleInputTarget::NodeNamed {
                node: "sink".into(),
                port: "run".into(),
                slot: "value".into(),
            });
        if !to_module {
            g.replace("worker", package.as_ref()).unwrap();
        }
        g.clone().compile().unwrap();
        for (index, &(edit, ref residual)) in edits.iter().enumerate() {
            let package = Rc::clone(&package);
            let apply = move |g: &mut GraphBuilder| -> Result<(), String> {
                if to_module {
                    g.replace("worker", package.as_ref())?;
                } else {
                    g.replace("worker", NodeTemplate::local_function(|x: i64| x))?;
                }
                edit(g);
                Ok(())
            };
            let mut after = g.clone();
            apply(&mut after).unwrap();
            let mut expected = vec![
                Requirement::Node("worker".into()),
                Requirement::Instance("worker".into()),
            ];
            match residual {
                Some(PatchTarget::PublicInput(name)) => {
                    expected.push(Requirement::PublicInput(name.clone()));
                }
                Some(PatchTarget::PublicOutput(name)) => {
                    expected.push(Requirement::PublicOutput(name.clone()));
                }
                None => {
                    assert_eq!(after.module.inputs[0].targets.len(), 2);
                    after.clone().compile().unwrap();
                }
                _ => unreachable!(),
            }
            assert_eq!(
                observe(&g, &g.pending.borrow(), &after).required,
                expected,
                "direction {to_module}, case {index}"
            );
            let mut assembly = GraphAssembly::from_builder(g.clone());
            let mut planner = PatchPlanner::new();
            planner
                .add_patch(PatchBuilder::custom(
                    PatchSpec::new("replace")
                        .writes(PatchTarget::Node("worker".into()))
                        .writes(PatchTarget::IncidentConnections("worker".into())),
                    apply,
                ))
                .unwrap();
            let result = planner.apply(&mut assembly);
            if let Some(target) = residual {
                assert_eq!(
                    result,
                    Err(PatchError::UndeclaredWrite {
                        patch: "replace".into(),
                        target: Box::new(target.clone()),
                    }),
                    "direction {to_module}, case {index}"
                );
                assert!(assembly.revisions.is_empty());
            } else {
                assert!(result.is_ok(), "{result:?}");
            }
        }
    }
}

#[test]
fn precise_custom_candidate_layout_is_not_a_graph_write_or_a_noop() {
    for declared in [false, true] {
        let mut assembly = GraphAssembly::from_builder(fixture());
        let mut spec = PatchSpec::new("layout");
        if declared {
            spec = spec.writes(PatchTarget::Node("new".into()));
        }
        let mut planner = PatchPlanner::new();
        planner
            .add_patch(PatchBuilder::custom(spec, |g| {
                g.node("new", |x: i64| x)
                    .unwrap()
                    .input(Layout::atomic())
                    .unwrap();
                Ok(())
            }))
            .unwrap();
        let result = planner.apply(&mut assembly);
        if declared {
            assert!(result.is_ok(), "{result:?}");
        } else {
            assert_eq!(
                result,
                Err(PatchError::UndeclaredWrite {
                    patch: "layout".into(),
                    target: Box::new(PatchTarget::Node("new".into()))
                })
            );
        }
    }
}

#[test]
fn dependency_payload_matrix_charges_all_aliases() {
    let package = fixture().module();
    let mut g = GraphBuilder::new("parent");
    g.instance("a", &package).unwrap();
    g.instance("b", &package).unwrap();
    let edits: &[fn(&mut ModuleEntry)] = &[
        |e| {
            e.definition.declaration_ids.insert("nested".into(), 42);
        },
        |e| e.definition.claims.push(contract("text")),
        |e| {
            e.definition
                .contract_requests
                .push(("worker".into(), contract("text")))
        },
        |e| e.definition.nodes[0].output.schema = "text".into(),
        |e| {
            e.definition.instances.push(ModuleInstance {
                name: "nested".into(),
                definition: "root".into(),
            })
        },
        |e| {
            e.definition.edges.push(ModuleEdge {
                declaration_id: 1,
                from: ModuleOutputSource::NodeNamed {
                    node: "worker".into(),
                    output: "value".into(),
                },
                to: ModuleInputTarget::NodeNamed {
                    node: "worker".into(),
                    port: "run".into(),
                    slot: "value".into(),
                },
            })
        },
        |e| e.definition.inputs[0].id += 1,
        |e| e.definition.inputs[0].name.push('x'),
        |e| e.definition.inputs[0].port.push('x'),
        |e| e.definition.inputs[0].slot.push('x'),
        |e| e.definition.inputs[0].schema = "text".into(),
        |e| e.definition.inputs[0].local_type = Some(TypeId::of::<bool>()),
        |e| e.definition.inputs[0].targets.clear(),
        |e| e.definition.output.as_mut().unwrap().id += 1,
        |e| e.definition.output.as_mut().unwrap().name.push('x'),
        |e| e.definition.output.as_mut().unwrap().schema = "text".into(),
        |e| e.definition.output.as_mut().unwrap().local_type = Some(TypeId::of::<bool>()),
        |e| {
            e.definition.output.as_mut().unwrap().source = ModuleOutputSource::Node(OutputAddr {
                node: NodeId(9),
                slot: SlotId(3),
            })
        },
        |e| e.triggers[0].id.0 += 1,
        |e| e.triggers[0].name.push('x'),
        |e| e.triggers[0].targets.clear(),
    ];
    for (index, edit) in edits.iter().enumerate() {
        assert_eq!(
            writes(&g, |g| edit(g.dependencies.get_mut("root").unwrap())),
            [
                Requirement::Instance("a".into()),
                Requirement::Instance("b".into())
            ],
            "case {index}"
        );
    }
    assert!(
        writes(&g, |g| g
            .dependencies
            .get_mut("root")
            .unwrap()
            .definition
            .id
            .push('x'))
        .contains(&Requirement::Graph)
    );
}

#[test]
fn destructive_and_same_name_export_changes_keep_precise_owners() {
    let g = fixture();
    let input_edits: &[fn(&mut ModuleInput)] = &[
        |i| i.id += 1,
        |i| i.port.push('x'),
        |i| i.slot.push('x'),
        |i| i.schema = "text".into(),
        |i| i.local_type = Some(TypeId::of::<bool>()),
        |i| i.targets.clear(),
    ];
    for edit in input_edits {
        assert_eq!(
            writes(&g, |g| edit(&mut g.module.inputs[0])),
            [Requirement::PublicInput("request".into())]
        );
    }
    assert_eq!(
        writes(&g, |g| {
            g.module.inputs.clear();
            g.public_input_ids.clear();
        }),
        [Requirement::PublicInput("request".into())]
    );
    assert_eq!(
        writes(&g, |g| {
            g.module.output = None;
        }),
        [Requirement::PublicOutput("result".into())]
    );
    assert_eq!(
        writes(&g, |g| {
            g.module.triggers.clear();
            g.trigger_names.clear();
        }),
        [Requirement::Trigger("run".into())]
    );
    assert_eq!(
        writes(&g, |g| {
            g.module.nodes.clear();
            g.module.node_names.clear();
            g.module.node_ids.clear();
            g.pending.borrow_mut().clear();
        }),
        [Requirement::Node("worker".into())]
    );
    assert!(
        writes(&g, |g| {
            g.trigger_names.insert("run".into(), 99);
        })
        .contains(&Requirement::Graph)
    );
    assert!(
        writes(&g, |g| {
            g.public_input_ids.remove("request");
        })
        .contains(&Requirement::Graph)
    );
}
