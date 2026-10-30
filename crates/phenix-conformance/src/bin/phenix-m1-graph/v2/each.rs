//! Executor acceptance through loaded native providers, not synthetic Wave outcomes.
use super::*;

const SLOW: &str = "example.graph.slow@1/run";
const COLLECT: &str = "example.graph.each-collect@1/run";

pub(super) fn profile_schemas(package: &str) -> (ValueSchema, phenix_contract::OperationSchema) {
    let verified =
        phenix_artifact::VerifiedPackage::verify_trusted_read_only(Path::new(package)).unwrap();
    let collect = verified
        .metadata()
        .definitions
        .iter()
        .find(|d| d.contract == "example.graph.each-collect@1")
        .unwrap()
        .operations
        .iter()
        .find(|s| s.name == "run")
        .unwrap()
        .clone();
    let request = ValueSchema::Struct(BTreeMap::from([("text".into(), ValueSchema::String)]));
    let batch = ValueSchema::List(Box::new(ValueSchema::Struct(BTreeMap::from([
        ("route".into(), ValueSchema::U64),
        ("input".into(), request),
    ]))));
    (batch, collect)
}

pub(super) fn run(packages: &[String], output: &Path) {
    let controls = [output.join("each-a.control"), output.join("each-b.control")];
    for (key, path) in ["PHENIX_GRAPH_A_EACH_CONTROL", "PHENIX_GRAPH_B_EACH_CONTROL"]
        .into_iter()
        .zip(&controls)
    {
        std::env::set_var(key, path);
    }
    let (batch, collect) = profile_schemas(&packages[1]);
    let execution = ExecutionPolicy {
        on_domain_error: DomainErrorPolicy::RouteErr,
    };
    let port = |name: &str, schema| OutputPort {
        name: name.into(),
        schema,
        security: LABEL,
    };
    let input = |schema| InputPort {
        name: "in".into(),
        schema,
        readiness: InputReadiness::One,
        security: LABEL,
    };
    let mut d = declarations(false, false);
    d[0].sources[0].payload = batch.clone();
    d[0].nodes = vec![
        NodeDecl {
            id: SOURCE.into(),
            kind: NodeKind::Source {
                source: SOURCE.into(),
            },
            inputs: vec![],
            outputs: vec![port("out", batch.clone())],
            execution,
        },
        NodeDecl {
            id: FIRST.into(),
            kind: NodeKind::InvokeEach {
                collection: "Workers".into(),
                operation: SLOW.into(),
                include_self: true,
                max: 3,
            },
            inputs: vec![input(batch)],
            outputs: vec![port("out", collect.request.clone())],
            execution,
        },
    ];
    if let EdgeKind::Flow { from, .. } = &mut d[0].edges[1].kind {
        from.port = "out".into();
    }
    d[1].nodes = vec![NodeDecl {
        id: SECOND.into(),
        kind: NodeKind::Invoke {
            selector: String::new(),
            operation: COLLECT.into(),
        },
        inputs: vec![input(collect.request.clone())],
        outputs: vec![
            port("err", collect.error.clone()),
            port("ok", collect.response.clone()),
        ],
        execution,
    }];
    let resources = BTreeSet::from([
        "example.graph-a.property.result".into(),
        "example.graph.delivery".into(),
    ]);
    let set = format!(
        "{}/set",
        phenix_contract::property_contract(A, "result").unwrap()
    );
    let get = format!(
        "{}/get",
        phenix_contract::property_contract(A, "result").unwrap()
    );
    let grants: Vec<_> = [
        ("root", KEY),
        (A, SLOW),
        (B, SLOW),
        (B, COLLECT),
        (A, set.as_str()),
        (A, get.as_str()),
    ]
    .into_iter()
    .map(|(caller, key)| {
        let (interface, operation) = key.rsplit_once('/').unwrap();
        ScopedGrant {
            caller: caller.into(),
            interface: interface.into(),
            operation: operation.into(),
            resources: resources.clone(),
        }
    })
    .collect();
    let composition = Composition {
        format: "phenix.composition.v3".into(),
        artifacts: d
            .iter()
            .enumerate()
            .map(|(i, g)| repack(&packages[i], &output.join(format!("v2-each-{i}")), g))
            .collect(),
        definitions: vec![],
        entry: Selection {
            interface: TASK.into(),
            operation: "run".into(),
            provider: A.into(),
        },
        slots: vec![SlotBinding {
            caller: A.into(),
            slot: "Workers".into(),
            index: 0,
            provider: B.into(),
            export: "slow".into(),
        }],
        authority: Authority {
            operations: grants
                .iter()
                .map(|g| format!("{}/{}", g.interface, g.operation))
                .collect(),
            resources: resources.clone(),
        },
        grants,
        excluded: BTreeSet::new(),
        substitutions: vec![],
        resource: None,
    };
    let mut policy = TrustedCompositionPolicy::default();
    policy.source_owners.insert(
        A.into(),
        SourceOwnerPolicy {
            owner_ceiling: Classification::Confidential,
            composition_ceiling: Classification::Confidential,
        },
    );
    policy.sources.insert(SOURCE.into(), LABEL);
    policy.topology.insert((A.into(), SECOND.into()));
    for (owner, operation) in [(A, SLOW), (B, SLOW), (B, COLLECT)] {
        policy.operations.insert(
            (owner.into(), operation.into()),
            OperationSecurity {
                input: LABEL,
                ok: LABEL,
                err: LABEL,
            },
        );
    }
    // InvokeEach requires exact delivery grants on both boundaries, even when
    // the source and the authored batch node have the same contributor.
    for (from, to) in [(SOURCE, FIRST), (FIRST, SECOND)] {
        policy.delivery.insert(
            (from.into(), "out".into(), to.into(), "in".into()),
            FlowGrant {
                resources: resources.clone(),
                maximum_classification: Classification::Confidential,
            },
        );
    }
    for key in policy.delivery.keys() {
        let mut missing = policy.clone();
        missing.delivery.remove(key);
        let failure = Prepared::prepare_graph_v2(composition.clone(), &missing)
            .err()
            .unwrap();
        assert_eq!(failure.code, Code::Denied);
        assert_eq!(
            failure.message, "cross-plugin flow lacks exact delivery grant",
            "missing exact fixture grant: {key:?}"
        );
    }
    let kernel = Prepared::prepare_graph_v2(composition.clone(), &policy)
        .unwrap()
        .load()
        .unwrap();
    let calls = |index: usize| {
        fs::read_to_string(controls[index].with_extension("calls")).unwrap_or_default()
    };
    let mut cases = Vec::new();
    for profile in [
        "reverse",
        "empty",
        "serialize",
        "denied",
        "cancel-held",
        "failure-held",
    ] {
        for path in &controls {
            fs::write(path, "hold").unwrap();
        }
        let before = [calls(0), calls(1)];
        let fds = fs::read_dir("/proc/self/fd").unwrap().count();
        // Workers#0 is B; the appended self route is A.
        let routes: &[u64] = match profile {
            "empty" => &[],
            "serialize" => &[0, 0],
            _ => &[1, 0],
        };
        let payload = Value::List(
            routes
                .iter()
                .enumerate()
                .map(|(i, route)| {
                    Value::map([
                        ("route", Value::U64(*route)),
                        (
                            "input",
                            Value::map([(
                                "text",
                                Value::String(if i == 1 {
                                    "domain".into()
                                } else {
                                    "first".into()
                                }),
                            )]),
                        ),
                    ])
                })
                .collect(),
        );
        let mut authority = composition.authority.clone();
        if profile == "denied" {
            authority.operations.remove(SLOW);
        }
        let mut run = kernel
            .begin_action_scoped(
                ACTION,
                &payload,
                ScopedRequest {
                    authority,
                    resource: None,
                },
            )
            .unwrap();
        let terminals = std::cell::Cell::new(0);
        let mut observe = |u: &phenix_kernel::graph::GraphUpdate<'_>| {
            assert!(!matches!(u.event, GraphEvent::NodeFinished(_)));
            if matches!(u.event, GraphEvent::Finished(_)) {
                assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
                terminals.set(terminals.get() + 1);
            }
        };
        let first = run.pump(&mut observe);
        if profile == "denied" {
            assert_eq!(first.unwrap_err().code, Code::Denied);
            assert_eq!([calls(0), calls(1)], before, "denial ran native effects");
            assert!(run.steps().is_empty());
        } else if profile == "failure-held" {
            assert_eq!(first.unwrap(), GraphStatus::Pending);
            assert_eq!(kernel.inspect()["pending"]["physical_leases"], 2);
            assert_eq!(calls(0), format!("{}first\n", before[0]));
            assert_eq!(calls(1), format!("{}domain\n", before[1]));
            fs::write(&controls[1], "fail").unwrap();
            for _ in 0..4 {
                assert_eq!(run.pump(&mut observe).unwrap(), GraphStatus::Draining);
                assert!(run.completion().is_none());
                assert_eq!(terminals.get(), 0);
                assert!(!run.steps().iter().any(|s| s.node == SECOND));
                assert_eq!(kernel.inspect()["pending"]["physical_leases"], 2);
                assert!(kernel.inspect()["pending"]["tickets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|ticket| ticket["provider"] == B
                        && ticket["logical"] == "failed"
                        && ticket["native_quiescent"] == false
                        && ticket["quiescent"] == false));
                assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds + 2);
                assert!(kernel.shutdown().is_err());
            }
            for path in &controls {
                fs::write(path, "drain").unwrap();
            }
            assert_eq!(run.pump(&mut observe).unwrap_err().code, Code::PluginFailed);
            assert!(
                matches!(run.completion(), Some(GraphCompletion::Failure(f)) if f.message == "each held failure")
            );
            assert!(!run.steps().iter().any(|s| s.node == SECOND));
            assert!(
                run.steps().is_empty(),
                "late cleanup output escaped failure"
            );
            for _ in 0..3 {
                assert_eq!(
                    run.pump(|_| panic!("duplicate failure event"))
                        .unwrap_err()
                        .code,
                    Code::PluginFailed
                );
            }
        } else {
            if profile != "empty" {
                assert_eq!(first.unwrap(), GraphStatus::Pending);
                let count = if profile == "serialize" { 1 } else { 2 };
                assert_eq!(kernel.inspect()["pending"]["physical_leases"], count);
                assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds + count);
                assert_eq!(
                    calls(1),
                    format!(
                        "{}{}\n",
                        before[1],
                        if profile == "serialize" {
                            "first"
                        } else {
                            "domain"
                        }
                    )
                );
                if profile != "serialize" {
                    assert_eq!(calls(0), format!("{}first\n", before[0]));
                }
                if profile == "cancel-held" {
                    run.cancel().unwrap();
                }
                // Release B first while A's physical work remains held.
                fs::write(
                    &controls[1],
                    if profile == "cancel-held" {
                        "drain"
                    } else {
                        "release"
                    },
                )
                .unwrap();
                for _ in 0..4 {
                    let status = run.pump(&mut observe).unwrap();
                    if profile != "serialize" {
                        assert_eq!(
                            status,
                            if profile == "cancel-held" {
                                GraphStatus::Draining
                            } else {
                                GraphStatus::Pending
                            }
                        );
                        assert!(run.completion().is_none());
                        assert!(!run.steps().iter().any(|s| s.node == SECOND));
                        assert_eq!(terminals.get(), 0);
                        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
                        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds + 1);
                        if profile == "reverse" {
                            assert_eq!(run.steps().len(), 1);
                            assert_eq!(
                                run.steps()[0].outcome,
                                Outcome::Domain(Value::String("domain fixture b".into()))
                            );
                        }
                        assert!(kernel.shutdown().is_err());
                    }
                }
                fs::write(
                    &controls[0],
                    if profile == "cancel-held" {
                        "drain"
                    } else {
                        "release"
                    },
                )
                .unwrap();
                for _ in 0..16 {
                    if run.pump(&mut observe).unwrap() == GraphStatus::Finished {
                        break;
                    }
                }
            } else {
                let mut status = first.unwrap();
                for _ in 0..16 {
                    assert_eq!([calls(0), calls(1)], before, "empty batch invoked a branch");
                    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
                    if status == GraphStatus::Finished {
                        break;
                    }
                    assert_eq!(status, GraphStatus::Pending);
                    assert!(run.completion().is_none());
                    status = run.pump(&mut observe).unwrap();
                }
                assert_eq!(status, GraphStatus::Finished, "empty batch pump limit");
                assert_eq!([calls(0), calls(1)], before, "empty batch invoked a branch");
            }
            assert_eq!(
                run.completion(),
                Some(if profile == "cancel-held" {
                    GraphCompletion::Cancelled
                } else if matches!(profile, "reverse" | "serialize") {
                    GraphCompletion::Domain
                } else {
                    GraphCompletion::Success
                })
            );
            if profile == "cancel-held" {
                assert!(
                    run.steps().is_empty(),
                    "late cleanup output escaped cancellation"
                );
            } else {
                let expected = Value::List(
                    routes
                        .iter()
                        .enumerate()
                        .map(|(i, _)| {
                            Value::map([(
                                if i == 1 { "Err" } else { "Ok" },
                                Value::String(
                                    if i == 1 { "domain fixture b" } else { "first" }.into(),
                                ),
                            )])
                        })
                        .collect(),
                );
                assert_eq!(
                    run.steps()
                        .iter()
                        .find(|s| s.node == SECOND)
                        .unwrap()
                        .outcome,
                    Outcome::Success(expected)
                );
            }
            let retained = format!("{:?}", run.steps());
            let native_calls = [calls(0), calls(1)];
            for _ in 0..3 {
                assert_eq!(
                    run.pump(|_| panic!("duplicate event")).unwrap(),
                    GraphStatus::Finished
                );
            }
            assert_eq!(format!("{:?}", run.steps()), retained);
            assert_eq!([calls(0), calls(1)], native_calls);
            if profile == "serialize" {
                assert_eq!(calls(1), format!("{}first\ndomain\n", before[1]));
                assert_eq!(calls(0), before[0]);
            }
        }
        assert_eq!(terminals.get(), 1);
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds);
        cases.push(profile);
    }
    kernel.shutdown().unwrap();
    for key in ["PHENIX_GRAPH_A_EACH_CONTROL", "PHENIX_GRAPH_B_EACH_CONTROL"] {
        std::env::remove_var(key);
    }
    fs::write(output.join("graph-v2-each.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "format": "phenix.graph-v2-each.v1", "cases": cases, "full_V2": false, "p1_parity": false,
        "acceptance": "focused native executor InvokeEach", "ci": "pending"
    })).unwrap()).unwrap();
}
