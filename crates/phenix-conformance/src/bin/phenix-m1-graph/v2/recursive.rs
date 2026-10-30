//! Package-backed recursive preparation through the existing native dispatcher.
use super::*;
use phenix_contract::graph::v2::composition::*;
use phenix_kernel::graph::v2::recursive_effective_id;

pub(super) fn run(
    packages: &[String],
    composition: &Composition,
    policy: &TrustedCompositionPolicy,
    output: &Path,
) {
    let witness = output.join("v2-recursive-constructor");
    let consumer = output.join("v2-recursive-consumer");
    std::env::set_var("PHENIX_GRAPH_CONSTRUCTOR_WITNESS", &witness);
    std::env::set_var("PHENIX_GRAPH_B_CONSUME_WITNESS", &consumer);
    assert!(!witness.exists());
    assert!(!consumer.exists());
    let mut bodies = declarations(false, false);
    // Foreign edges belong to trusted boundary wiring, not the attested root body.
    bodies[0]
        .edges
        .retain(|edge| matches!(&edge.kind, EdgeKind::Flow { to, .. } if to.node == FIRST));
    assert!(bodies[1].sources.is_empty());
    let mut composition = composition.clone();
    composition.artifacts = packages
        .iter()
        .zip(&bodies)
        .enumerate()
        .map(|(i, (package, body))| {
            repack(package, &output.join(format!("v2-recursive-{i}")), body)
        })
        .collect();
    let verified: Vec<_> = composition
        .artifacts
        .iter()
        .map(|path| {
            phenix_artifact::VerifiedPackage::verify_trusted_read_only(Path::new(path)).unwrap()
        })
        .collect();
    let child = GraphDefinition {
        id: GraphDefinitionId::new("example.graph-b/child").unwrap(),
        body: verified[1].graph_v2().unwrap().clone(),
        inputs: vec![BoundaryInput {
            port: bodies[1].nodes[0].inputs[0].clone(),
            target: InPortRef {
                node: SECOND.into(),
                port: "in".into(),
            },
        }],
        outputs: vec![],
        children: vec![],
        wiring: vec![],
    };
    let root = GraphDefinition {
        id: GraphDefinitionId::new("example.graph-a/root").unwrap(),
        body: verified[0].graph_v2().unwrap().clone(),
        inputs: vec![],
        outputs: vec![],
        children: ["left", "right"]
            .map(|name| GraphChild {
                name: name.into(),
                definition: child.id.clone(),
            })
            .into(),
        wiring: ["left", "right"]
            .map(|name| BoundaryFlow {
                name: name.into(),
                from: BoundaryFrom::Local(OutPortRef {
                    node: FIRST.into(),
                    port: "ok".into(),
                }),
                to: BoundaryTo::Child {
                    child: name.into(),
                    port: "in".into(),
                },
            })
            .into(),
    };
    let instance = GraphInstanceId::new(root.id.clone());
    let effective = |owner, kind, instance: GraphInstanceId, authored: &str| {
        recursive_effective_id(
            owner,
            kind,
            &LogicalId {
                instance,
                authored: authored.into(),
            },
        )
        .unwrap()
    };
    let source = effective(A, "source", instance.clone(), SOURCE);
    let first = effective(A, "node", instance.clone(), FIRST);
    let children =
        ["left", "right"].map(|name| effective(B, "node", instance.child(name).unwrap(), SECOND));
    assert_ne!(children[0], children[1]);
    let mut policy = policy.clone();
    policy.sources.clear();
    policy.sources.insert(source.clone(), LABEL);
    policy.topology.clear();
    policy.delivery.clear();
    for second in &children {
        policy.topology.insert((A.into(), second.clone()));
        policy.delivery.insert(
            (first.clone(), "ok".into(), second.clone(), "in".into()),
            FlowGrant {
                resources: BTreeSet::from(["example.graph.delivery".into()]),
                maximum_classification: Classification::Confidential,
            },
        );
    }
    let definitions = vec![root.clone(), child];
    let budget = ExpansionBudget {
        depth: 2,
        occurrences: 3,
        items: 64,
    };
    let mut cases = Vec::new();
    for negative in [
        "altered-body",
        "authored-source",
        "authored-topology",
        "delivery",
        "source-ceiling",
        "grant",
    ] {
        let mut definitions = definitions.clone();
        let mut c = composition.clone();
        let mut p = policy.clone();
        match negative {
            "altered-body" => {
                definitions[0].body.sources[0].kind = SourceKind::ExternalAction {
                    action: "example.graph.altered".into(),
                }
            }
            "authored-source" => {
                p.sources.clear();
                p.sources.insert(SOURCE.into(), LABEL);
            }
            "authored-topology" => {
                p.topology.clear();
                p.topology.insert((A.into(), SECOND.into()));
            }
            "delivery" => p.delivery.clear(),
            "source-ceiling" => {
                p.source_owners.get_mut(A).unwrap().owner_ceiling = Classification::Public
            }
            "grant" => c.grants.retain(|g| g.caller != B),
            _ => unreachable!(),
        }
        assert_eq!(
            Prepared::prepare_recursive_graph_v2(c, &definitions, &root.id, budget, &p)
                .err()
                .unwrap()
                .code,
            Code::Denied,
            "{negative}"
        );
        assert!(!witness.exists(), "{negative} ran constructor");
        assert!(!consumer.exists(), "{negative} ran consumer");
        cases.push(negative);
    }
    let prepared = Prepared::prepare_recursive_graph_v2(
        composition.clone(),
        &definitions,
        &root.id,
        budget,
        &policy,
    )
    .unwrap();
    assert!(!witness.exists(), "preparation ran constructor");
    let kernel = prepared.load().unwrap();
    assert!(witness.exists());
    let payload = Value::map([("text", Value::String("recursive-native".into()))]);
    let mut execution = kernel
        .begin_action_scoped(
            ACTION,
            &payload,
            ScopedRequest {
                authority: composition.authority,
                resource: None,
            },
        )
        .unwrap();
    let mut terminals = 0;
    assert_eq!(
        execution
            .pump(|update| {
                assert!(
                    !matches!(update.event, GraphEvent::NodeFinished(_)),
                    "protected payload exposed"
                );
                if matches!(update.event, GraphEvent::Finished(_)) {
                    terminals += 1;
                }
            })
            .unwrap(),
        GraphStatus::Finished
    );
    assert_eq!(execution.completion(), Some(GraphCompletion::Success));
    assert_eq!(execution.steps().len(), 3);
    assert_eq!(execution.steps()[0].node, first);
    assert_eq!(
        execution.steps()[0].outcome,
        Outcome::Success(Value::String("recursive-native".into()))
    );
    let actual: BTreeSet<_> = execution.steps()[1..]
        .iter()
        .map(|step| {
            assert_eq!(
                step.outcome,
                Outcome::Success(Value::String("consumed:recursive-native".into()))
            );
            step.node.clone()
        })
        .collect();
    assert_eq!(actual, BTreeSet::from(children));
    assert_eq!(fs::read_to_string(&consumer).unwrap(), "called\ncalled\n");
    assert_eq!(kernel.inspect()["calls"], 3);
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
    assert_eq!(
        execution
            .pump(|_| panic!("duplicate recursive event"))
            .unwrap(),
        GraphStatus::Finished
    );
    assert_eq!(terminals, 1);
    drop(execution);
    kernel.shutdown().unwrap();
    cases.push("repeated-source-free-child-native-execution");
    fs::write(output.join("graph-v2-recursive.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "format": "phenix.graph-v2-recursive.v1", "full_V2": false, "p1_parity": false,
        "cases": cases,
        "limits": ["finite root with two source-free child occurrences", "no repeated action sources, recursive cycles or P1 parity"]
    })).unwrap()).unwrap();
    std::env::set_var(
        "PHENIX_GRAPH_CONSTRUCTOR_WITNESS",
        output.join("v2-constructor-witness"),
    );
    std::env::set_var(
        "PHENIX_GRAPH_B_CONSUME_WITNESS",
        output.join("v2-consumer-witness"),
    );
}
