use phenix_graph::compose::{
    ComposeError, Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration,
};
use phenix_graph::contract::Contract;
use phenix_graph::network::{ConnectionDef, NetworkBuilder, NetworkDef, PortPath};
use phenix_graph::process::{Process, ProcessContext, Step};
use phenix_graph::{Body, ComponentDef, GraphOrigin, Interface, PortDef};
use std::num::NonZeroUsize;
use std::sync::Arc;

struct Idle;
impl Process for Idle {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        Ok(Step::Finished)
    }
}
fn cap() -> NonZeroUsize {
    NonZeroUsize::new(7).unwrap()
}
fn port(process: &str, name: &str) -> PortPath {
    PortPath {
        process: process.into(),
        port: name.into(),
    }
}
fn key(a: &str, b: &str) -> ConnectionKey {
    ConnectionKey::new(port(a, "out"), port(b, "in"))
}
fn edge(a: &str, b: &str) -> ConnectionDef {
    ConnectionDef {
        from: port(a, "out"),
        to: port(b, "in"),
        capacity: cap(),
    }
}
fn primitive() -> Arc<ComponentDef> {
    Arc::new(ComponentDef::primitive(
        Arc::new(
            Interface::new(
                [PortDef::local::<i64>("in")],
                [PortDef::local::<i64>("out")],
            )
            .unwrap(),
        ),
        Arc::new(|| Idle),
    ))
}
fn composite() -> Arc<ComponentDef> {
    let mut builder = NetworkBuilder::new();
    let child = builder.add("leaf", primitive()).unwrap();
    builder
        .expose_input("in", child.input::<i64>("in").unwrap(), cap())
        .unwrap();
    builder
        .expose_output("out", child.output::<i64>("out").unwrap())
        .unwrap();
    Arc::new(ComponentDef::composite(builder.build().unwrap()))
}
fn add(path: &str) -> Edit {
    Edit::Add {
        path: path.into(),
        definition: primitive(),
    }
}
fn reg(id: &str, deps: &[&str], priority: i64, edits: Vec<Edit>) -> Registration {
    let patch = NetworkPatch::new(edits);
    Registration::new(
        id,
        deps.iter().map(|s| (*s).into()),
        priority,
        GraphOrigin::default(),
        patch.footprints(),
        patch,
    )
}
fn empty() -> NetworkDef {
    NetworkBuilder::new().build().unwrap()
}
fn base() -> NetworkDef {
    let mut builder = NetworkBuilder::new();
    let a = builder.add("a", primitive()).unwrap();
    let b = builder.add("b", primitive()).unwrap();
    builder
        .connect(
            a.output::<i64>("out").unwrap(),
            b.input::<i64>("in").unwrap(),
            cap(),
        )
        .unwrap();
    builder.build().unwrap()
}

fn contract() -> Contract {
    Contract::new("composition/worker", primitive().interface().clone()).unwrap()
}
fn claimed() -> Arc<ComponentDef> {
    Arc::new((*primitive()).clone().claim(&contract()).unwrap())
}

#[test]
fn introduced_add_and_replace_claims_cannot_be_stripped_by_later_registration() {
    for (path, introduce) in [
        (
            "new",
            Edit::Add {
                path: "new".into(),
                definition: claimed(),
            },
        ),
        (
            "a",
            Edit::Replace {
                path: "a".into(),
                definition: claimed(),
            },
        ),
    ] {
        let mut composer = Composer::new(base()).unwrap();
        let before = composer.current().network().instances()["a"].definition_key();
        let result = composer.register([
            reg("introduce", &[], 0, vec![introduce]),
            reg(
                "strip",
                &["introduce"],
                0,
                vec![Edit::Replace {
                    path: path.into(),
                    definition: primitive(),
                }],
            ),
        ]);
        assert!(matches!(result, Err(ComposeError::Network(_))));
        assert!(composer.current().active().is_empty());
        assert_eq!(
            composer.current().network().instances()["a"].definition_key(),
            before
        );
        assert_eq!(
            composer.current().network().connections(),
            &[edge("a", "b")]
        );
        assert!(composer.revoke("introduce").unwrap().is_empty());
    }
}

#[test]
fn each_edit_checkpoints_claims_even_within_one_patch() {
    let mut composer = Composer::new(empty()).unwrap();
    assert!(matches!(
        composer.register([reg(
            "bad",
            &[],
            0,
            vec![
                Edit::Add {
                    path: "worker".into(),
                    definition: primitive()
                },
                Edit::Replace {
                    path: "worker".into(),
                    definition: claimed()
                },
                Edit::Replace {
                    path: "worker".into(),
                    definition: primitive()
                },
            ]
        )]),
        Err(ComposeError::Network(_))
    ));
    assert!(composer.current().network().instances().is_empty());
    assert!(composer.current().network().requirements().is_empty());
}

#[test]
fn failed_claim_stripping_preserves_published_native_bindings_and_registry() {
    let mut composer = Composer::new(empty()).unwrap();
    composer
        .register([reg(
            "introduce",
            &[],
            0,
            vec![Edit::Add {
                path: "worker".into(),
                definition: claimed(),
            }],
        )])
        .unwrap();
    let published = composer.current().clone();
    let output = published
        .compiled()
        .bind("worker", &contract())
        .unwrap()
        .output::<i64>("out")
        .unwrap();
    assert!(matches!(
        composer.register([reg(
            "strip",
            &["introduce"],
            0,
            vec![Edit::Replace {
                path: "worker".into(),
                definition: primitive()
            }]
        )]),
        Err(ComposeError::Network(_))
    ));
    assert_eq!(
        output.resolve(composer.current().compiled()).unwrap(),
        (0, "out")
    );
    assert_eq!(composer.current().active(), &["introduce".into()].into());
    assert_eq!(
        composer
            .current()
            .compiled()
            .inspect("worker")
            .unwrap()
            .declared_origin,
        published
            .compiled()
            .inspect("worker")
            .unwrap()
            .declared_origin
    );
    assert!(composer.revoke("strip").unwrap().is_empty());
    composer.revoke("introduce").unwrap();
    assert!(composer.current().network().requirements().is_empty());
}

#[test]
fn replacing_an_introduced_claim_with_a_different_same_id_definition_fails() {
    let wrong = Contract::new(contract().id(), Arc::new(Interface::default())).unwrap();
    let definition = Arc::new(
        ComponentDef::primitive(wrong.required().clone(), Arc::new(|| Idle))
            .claim(&wrong)
            .unwrap(),
    );
    let mut composer = Composer::new(empty()).unwrap();
    assert!(matches!(
        composer.register([
            reg(
                "introduce",
                &[],
                0,
                vec![Edit::Add {
                    path: "worker".into(),
                    definition: claimed()
                }]
            ),
            reg(
                "change",
                &["introduce"],
                0,
                vec![Edit::Replace {
                    path: "worker".into(),
                    definition
                }]
            ),
        ]),
        Err(ComposeError::Network(_))
    ));
    assert!(composer.current().active().is_empty());
}

#[test]
fn introduced_private_requirements_survive_replacement_in_the_same_replay() {
    let mut child = NetworkBuilder::new();
    let leaf = child.add("leaf", claimed()).unwrap();
    child
        .expose_input("in", leaf.input::<i64>("in").unwrap(), cap())
        .unwrap();
    child
        .expose_output("out", leaf.output::<i64>("out").unwrap())
        .unwrap();
    child.require_contract("leaf", &contract()).unwrap();
    let definition = Arc::new(ComponentDef::composite(child.build().unwrap()));
    for introduce in [
        Edit::Add {
            path: "new".into(),
            definition: definition.clone(),
        },
        Edit::Replace {
            path: "a".into(),
            definition,
        },
    ] {
        let path = match &introduce {
            Edit::Add { path, .. } | Edit::Replace { path, .. } => path.clone(),
            _ => unreachable!(),
        };
        let mut composer = Composer::new(base()).unwrap();
        assert!(matches!(
            composer.register([
                reg("introduce", &[], 0, vec![introduce]),
                reg(
                    "remove-private",
                    &["introduce"],
                    0,
                    vec![Edit::Replace {
                        path,
                        definition: primitive()
                    }]
                ),
            ]),
            Err(ComposeError::Network(_))
        ));
        assert_eq!(
            composer.current().network().connections(),
            &[edge("a", "b")]
        );
        assert!(composer.current().active().is_empty());
    }
}

#[test]
fn preserved_claims_and_unrelated_rewire_commute_and_revoke_rebuilds_obligations() {
    let mut composer = Composer::new(base()).unwrap();
    composer
        .register([
            reg(
                "introduce",
                &[],
                0,
                vec![Edit::Replace {
                    path: "a".into(),
                    definition: claimed(),
                }],
            ),
            reg(
                "replace",
                &["introduce"],
                0,
                vec![Edit::Replace {
                    path: "a".into(),
                    definition: claimed(),
                }],
            ),
            reg(
                "rewire",
                &[],
                0,
                vec![
                    add("c"),
                    Edit::Disconnect(key("a", "b")),
                    Edit::Connect(edge("a", "c")),
                ],
            ),
        ])
        .unwrap();
    assert_eq!(composer.current().active().len(), 3);
    composer
        .current()
        .compiled()
        .check_implementation("a", &contract())
        .unwrap();
    assert!(
        composer
            .current()
            .network()
            .requirements()
            .contains(&("a".into(), contract()))
    );
    assert_eq!(
        composer.current().network().connections(),
        &[edge("a", "c")]
    );
    assert_eq!(
        composer.revoke("introduce").unwrap(),
        ["introduce".into(), "replace".into()].into()
    );
    assert!(composer.current().network().requirements().is_empty());
    assert_eq!(
        composer.current().network().connections(),
        &[edge("a", "c")]
    );
    composer
        .register([reg(
            "unclaimed",
            &[],
            0,
            vec![Edit::Replace {
                path: "a".into(),
                definition: primitive(),
            }],
        )])
        .unwrap();
}

#[test]
fn plugin_a_add_b_extend_c_replace_opposite_body_and_revoke() {
    let a = reg("A", &[], 0, vec![add("a")]);
    let b = reg(
        "B",
        &["A"],
        0,
        vec![
            add("b"),
            Edit::Connect(edge("a", "b")),
            Edit::ExposeInput {
                name: "request".into(),
                targets: vec![port("a", "in")],
                capacity: cap(),
            },
            Edit::ExposeOutput {
                name: "reply".into(),
                from: port("b", "out"),
            },
        ],
    );
    let c_origin = GraphOrigin::default();
    let patch = NetworkPatch::new([Edit::Replace {
        path: "a".into(),
        definition: composite(),
    }]);
    let c = Registration::new(
        "C",
        ["B".into()],
        0,
        c_origin.clone(),
        patch.footprints(),
        patch,
    );
    for registrations in [vec![a.clone(), b.clone(), c.clone()], vec![c, a, b]] {
        let mut composer = Composer::new(empty()).unwrap();
        composer.register(registrations).unwrap();
        let result = composer.current();
        assert_eq!(result.active().len(), 3);
        assert!(matches!(
            result.network().instances()["a"].body(),
            Body::Composite(_)
        ));
        assert_eq!(result.network().instances()["a"].origin(), Some(&c_origin));
        assert_eq!(result.network().connections(), &[edge("a", "b")]);
        assert_eq!(result.compiled().processes()[0].path(), "a/leaf");
        assert_eq!(result.compiled().processes()[0].origin(), Some(&c_origin));
        assert_eq!(
            composer.revoke("A").unwrap(),
            ["A".into(), "B".into(), "C".into()].into()
        );
        assert!(composer.current().network().instances().is_empty());
    }
}

#[test]
fn independent_registrations_commute_even_when_admitted_separately() {
    let a = reg("a", &[], 0, vec![add("a")]);
    let b = reg("b", &[], 0, vec![add("b")]);
    let mut first = Composer::new(empty()).unwrap();
    first.register([a.clone()]).unwrap();
    first.register([b.clone()]).unwrap();
    let mut second = Composer::new(empty()).unwrap();
    second.register([b]).unwrap();
    second.register([a]).unwrap();
    assert_eq!(first.current().active(), second.current().active());
    assert_eq!(
        first
            .current()
            .network()
            .instances()
            .keys()
            .collect::<Vec<_>>(),
        second
            .current()
            .network()
            .instances()
            .keys()
            .collect::<Vec<_>>()
    );
}

#[test]
fn explicit_priority_defeats_prerequisite_and_all_transitive_dependents() {
    let low = reg("low", &[], -4, vec![add("a")]);
    let high = reg("high", &[], 5, vec![add("a")]);
    let b = reg("b", &["low"], 100, vec![add("b")]);
    let c = reg("c", &["b"], 200, vec![add("c")]);
    for registrations in [
        vec![low.clone(), high.clone(), b.clone(), c.clone()],
        vec![c, b, high, low],
    ] {
        let mut composer = Composer::new(empty()).unwrap();
        composer.register(registrations).unwrap();
        assert_eq!(composer.current().active(), &["high".into()].into());
        composer.revoke("high").unwrap();
        assert_eq!(
            composer.current().active(),
            &["low".into(), "b".into(), "c".into()].into()
        );
    }
}

#[test]
fn defeated_intermediate_cannot_suppress_an_independent_lower_priority_patch() {
    let mut composer = Composer::new(empty()).unwrap();
    composer
        .register([
            reg("high", &[], 3, vec![add("x")]),
            reg("middle", &[], 2, vec![add("x"), add("y")]),
            reg("low", &[], 1, vec![add("y")]),
        ])
        .unwrap();
    assert_eq!(
        composer.current().active(),
        &["high".into(), "low".into()].into()
    );
}

#[test]
fn deactivated_dependent_cannot_suppress_an_eligible_patch() {
    let mut composer = Composer::new(empty()).unwrap();
    composer
        .register([
            reg("prerequisite", &[], 1, vec![add("x")]),
            reg("winner", &[], 2, vec![add("x")]),
            reg("dependent", &["prerequisite"], 10, vec![add("y")]),
            reg("eligible", &[], 0, vec![add("y")]),
        ])
        .unwrap();
    assert_eq!(
        composer.current().active(),
        &["winner".into(), "eligible".into()].into()
    );
}

#[test]
fn equal_priorities_reject_without_publishing_or_registering() {
    let mut composer = Composer::new(empty()).unwrap();
    composer
        .register([reg("a", &[], 3, vec![add("shared")])])
        .unwrap();
    assert!(matches!(
        composer.register([reg("b", &[], 3, vec![add("shared")])]),
        Err(ComposeError::Conflict { .. })
    ));
    assert_eq!(composer.current().active(), &["a".into()].into());
    assert!(composer.revoke("b").unwrap().is_empty());
}

#[test]
fn missing_and_cyclic_dependencies_are_atomic() {
    let mut composer = Composer::new(empty()).unwrap();
    assert!(matches!(
        composer.register([reg("a", &["absent"], 0, vec![])]),
        Err(ComposeError::MissingDependency { .. })
    ));
    assert!(matches!(
        composer.register([reg("a", &["b"], 0, vec![]), reg("b", &["a"], 0, vec![])]),
        Err(ComposeError::DependencyCycle)
    ));
    assert!(composer.current().active().is_empty());
}

#[test]
fn final_compile_failure_discards_entire_candidate_and_registry() {
    let mut composer = Composer::new(base()).unwrap();
    let bad = reg(
        "bad",
        &[],
        0,
        vec![add("extra"), Edit::Connect(edge("extra", "missing"))],
    );
    assert!(matches!(
        composer.register([bad]),
        Err(ComposeError::Network(_))
    ));
    assert_eq!(composer.current().network().instances().len(), 2);
    assert_eq!(
        composer.current().network().connections(),
        &[edge("a", "b")]
    );
    assert!(composer.revoke("bad").unwrap().is_empty());
}

#[test]
fn replacement_and_unrelated_rewire_do_not_conflict_or_touch_incidents() {
    let replacement = reg(
        "replace",
        &[],
        0,
        vec![Edit::Replace {
            path: "a".into(),
            definition: composite(),
        }],
    );
    let rewire = reg(
        "rewire",
        &[],
        0,
        vec![
            add("c"),
            Edit::Disconnect(key("a", "b")),
            Edit::Connect(edge("a", "c")),
        ],
    );
    let mut composer = Composer::new(base()).unwrap();
    composer.register([rewire, replacement]).unwrap();
    assert_eq!(composer.current().active().len(), 2);
    assert_eq!(
        composer.current().network().connections(),
        &[edge("a", "c")]
    );
    composer.revoke("replace").unwrap();
    assert!(matches!(
        composer.current().network().instances()["a"].body(),
        Body::Primitive(_)
    ));
    assert_eq!(
        composer.current().network().connections(),
        &[edge("a", "c")]
    );
    composer.revoke("rewire").unwrap();
    assert_eq!(
        composer.current().network().connections(),
        &[edge("a", "b")]
    );
}

fn splice(id: &str, dependencies: &[&str], predecessor: &str) -> Registration {
    let patch = NetworkPatch::new([
        add(id),
        Edit::Splice {
            anchor: key("a", "b"),
            input: port(id, "in"),
            output: port(id, "out"),
        },
    ]);
    let mut grants = patch.footprints();
    grants.extend([
        Footprint::Connection(key(predecessor, id)),
        Footprint::Connection(key(predecessor, "b")),
    ]);
    Registration::new(
        id,
        dependencies.iter().map(|s| (*s).into()),
        0,
        GraphOrigin::default(),
        grants,
        patch,
    )
}

#[test]
fn splice_chains_keep_original_anchor_capacity_and_dependency_order() {
    let mut composer = Composer::new(base()).unwrap();
    composer
        .register([splice("z", &["x"], "x"), splice("x", &[], "a")])
        .unwrap();
    assert_eq!(
        composer.current().network().connections(),
        &[edge("a", "x"), edge("x", "z"), edge("z", "b")]
    );
    assert!(composer.current().footprints()["z"].contains(&Footprint::Connection(key("x", "b"))));
    composer.revoke("x").unwrap();
    assert_eq!(
        composer.current().network().connections(),
        &[edge("a", "b")]
    );
}

#[test]
fn independent_splices_are_ambiguous_even_with_priorities() {
    let mut composer = Composer::new(base()).unwrap();
    let y = splice("y", &[], "a");
    let high = Registration::new(
        "y",
        [],
        99,
        GraphOrigin::default(),
        y.grants().clone(),
        y.patch().clone(),
    );
    assert!(matches!(
        composer.register([splice("x", &[], "a"), high]),
        Err(ComposeError::Conflict { .. })
    ));
}

#[test]
fn generated_splice_edges_conflict_with_explicit_writes() {
    let mut composer = Composer::new(base()).unwrap();
    assert!(matches!(
        composer.register([
            splice("x", &[], "a"),
            reg("other", &[], 0, vec![Edit::Connect(edge("a", "x"))])
        ]),
        Err(ComposeError::Conflict { .. })
    ));
}

#[test]
fn source_start_compiles_without_factory_execution() {
    let interface = Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap());
    let definition = Arc::new(ComponentDef::primitive(
        interface,
        Arc::new(|| -> Idle { panic!("compile must not instantiate") }),
    ));
    let mut composer = Composer::new(empty()).unwrap();
    composer
        .register([reg(
            "source",
            &[],
            0,
            vec![
                Edit::Add {
                    path: "source".into(),
                    definition,
                },
                Edit::AddSource("source".into()),
            ],
        )])
        .unwrap();
    assert_eq!(composer.current().compiled().source_starts(), &[0]);
}

#[test]
fn final_compile_checks_all_fanout_targets_and_source_contracts() {
    for edit in [
        Edit::ExposeInput {
            name: "in".into(),
            targets: vec![port("a", "in"), port("missing", "in")],
            capacity: cap(),
        },
        Edit::AddSource("a".into()),
    ] {
        let mut composer = Composer::new(base()).unwrap();
        assert!(matches!(
            composer.register([reg("bad", &[], 0, vec![edit])]),
            Err(ComposeError::Network(_))
        ));
        assert!(composer.current().network().interface().inputs().is_empty());
        assert!(composer.current().network().source_starts().is_empty());
    }
}

#[test]
fn splice_generated_edges_require_separate_explicit_grants() {
    let patch = NetworkPatch::new([
        add("x"),
        Edit::Splice {
            anchor: key("a", "b"),
            input: port("x", "in"),
            output: port("x", "out"),
        },
    ]);
    let registration = Registration::new(
        "x",
        [],
        0,
        GraphOrigin::default(),
        patch.footprints(),
        patch,
    );
    let mut composer = Composer::new(base()).unwrap();
    assert!(
        matches!(composer.register([registration]), Err(ComposeError::Unauthorized { footprint: Footprint::Connection(edge), .. }) if edge == key("a", "x"))
    );
    assert_eq!(
        composer.current().network().connections(),
        &[edge("a", "b")]
    );
}

#[test]
fn interface_changing_replacement_is_atomic() {
    let definition = Arc::new(ComponentDef::composite(empty()));
    let mut composer = Composer::new(base()).unwrap();
    assert!(matches!(
        composer.register([reg(
            "bad",
            &[],
            0,
            vec![Edit::Replace {
                path: "a".into(),
                definition
            }]
        )]),
        Err(ComposeError::Network(_))
    ));
    assert_eq!(
        composer.current().network().connections(),
        &[edge("a", "b")]
    );
}

#[test]
fn composite_to_primitive_replacement_and_base_origin_restore() {
    let base_origin = GraphOrigin::default();
    let mut builder = NetworkBuilder::new();
    builder
        .add(
            "module",
            Arc::new((*composite()).clone().with_origin(base_origin.clone())),
        )
        .unwrap();
    let base = builder.build().unwrap();
    let mut composer = Composer::new(base).unwrap();
    composer
        .register([reg(
            "replace",
            &[],
            0,
            vec![Edit::Replace {
                path: "module".into(),
                definition: primitive(),
            }],
        )])
        .unwrap();
    assert_eq!(
        composer.current().compiled().processes()[0].path(),
        "module"
    );
    composer.revoke("replace").unwrap();
    assert_eq!(
        composer.current().network().instances()["module"].origin(),
        Some(&base_origin)
    );
    assert_eq!(
        composer.current().compiled().processes()[0].path(),
        "module/leaf"
    );
    assert_eq!(
        composer.current().compiled().processes()[0].origin(),
        Some(&base_origin)
    );
}

#[test]
fn dependency_ordered_writes_need_no_priority_or_fractional_positions() {
    let mut composer = Composer::new(empty()).unwrap();
    composer
        .register([
            reg(
                "last",
                &["middle"],
                -100,
                vec![Edit::Replace {
                    path: "module".into(),
                    definition: primitive(),
                }],
            ),
            reg(
                "first",
                &[],
                100,
                vec![Edit::Add {
                    path: "module".into(),
                    definition: primitive(),
                }],
            ),
            reg(
                "middle",
                &["first"],
                0,
                vec![Edit::Replace {
                    path: "module".into(),
                    definition: composite(),
                }],
            ),
        ])
        .unwrap();
    assert_eq!(composer.current().active().len(), 3);
    assert!(matches!(
        composer.current().network().instances()["module"].body(),
        Body::Primitive(_)
    ));
}
