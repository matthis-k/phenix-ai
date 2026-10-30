use super::*;
use phenix_contract::ValueSchema;

fn fixture() -> (Vec<GraphContributionV2>, TrustedCompositionPolicy) {
    let (mut declarations, mut policy) = super::super::tests::fixture();
    declarations.truncate(1);
    let graph = &mut declarations[0];
    graph.edges.truncate(1);
    let input = ValueSchema::List(Box::new(ValueSchema::Struct(BTreeMap::from([
        ("route".into(), ValueSchema::U64),
        ("input".into(), ValueSchema::String),
    ]))));
    graph.sources[0].payload = input.clone();
    graph.nodes[0].outputs[0].schema = input.clone();
    let node = &mut graph.nodes[1];
    node.kind = NodeKind::InvokeEach {
        collection: "Workers".into(),
        operation: "example.api@1/run".into(),
        include_self: true,
        max: 3,
    };
    node.inputs[0].schema = input;
    node.outputs.truncate(1);
    node.outputs[0].name = "out".into();
    node.outputs[0].schema = ValueSchema::List(Box::new(ValueSchema::Tagged(BTreeMap::from([
        ("Ok".into(), Some(ValueSchema::String)),
        ("Err".into(), Some(ValueSchema::String)),
    ]))));
    let labels = policy.operations.values().next().unwrap().clone();
    for provider in ["worker-a", "worker-b", "alpha"] {
        policy.operations.insert(
            (provider.into(), "example.api@1/run".into()),
            labels.clone(),
        );
    }
    (declarations, policy)
}

fn compile(
    declarations: &[GraphContributionV2],
    policy: &TrustedCompositionPolicy,
) -> Result<CompiledGraph, Failure> {
    super::super::compile_each(
        declarations,
        policy,
        |_| true,
        |owner, selector, _| {
            Ok((
                match selector {
                    "Workers#0" => "worker-a",
                    "Workers#1" => "worker-b",
                    "" => owner,
                    _ => return Err(invalid("unexpected selector")),
                }
                .into(),
                OperationSchema {
                    name: "run".into(),
                    request: ValueSchema::String,
                    response: ValueSchema::String,
                    error: ValueSchema::String,
                },
                BTreeSet::from(["resource".into()]),
            ))
        },
        |_, slot| {
            if slot == "Workers" {
                Ok(2)
            } else {
                Err(invalid("unknown collection"))
            }
        },
    )
}

fn items(routes: &[u64]) -> Value {
    Value::List(
        routes
            .iter()
            .enumerate()
            .map(|(index, route)| {
                Value::map([
                    ("route", Value::U64(*route)),
                    ("input", Value::String(format!("call-{index}"))),
                ])
            })
            .collect(),
    )
}

fn scope() -> ScopedRequest {
    ScopedRequest {
        authority: phenix_contract::Authority {
            operations: BTreeSet::new(),
            resources: BTreeSet::from(["resource".into()]),
        },
        resource: Some("resource".into()),
    }
}

#[test]
fn preparation_freezes_only_homogeneous_attested_selected_routes() {
    let (declarations, mut policy) = fixture();
    let graph = compile(&declarations, &policy).unwrap();
    assert_eq!(
        graph.each_routes["alpha/first"]
            .iter()
            .map(|(provider, _)| provider.as_str())
            .collect::<Vec<_>>(),
        ["worker-a", "worker-b", "alpha"]
    );
    let bytes = declarations[0].encode().unwrap();
    assert_eq!(
        GraphContributionV2::decode(&bytes)
            .unwrap()
            .encode()
            .unwrap(),
        bytes
    );
    policy
        .operations
        .remove(&("worker-b".into(), "example.api@1/run".into()));
    assert_eq!(
        compile(&declarations, &policy).unwrap_err().code,
        Code::Denied
    );
    let (_, mut policy) = fixture();
    policy.delivery.clear();
    assert_eq!(
        compile(&declarations, &policy).unwrap_err().code,
        Code::Denied
    );
}

#[test]
fn invalid_membership_is_atomic_before_any_branch_exists() {
    let (declarations, policy) = fixture();
    let graph = compile(&declarations, &policy).unwrap();
    let node = &declarations[0].nodes[1];
    for request in [
        items(&[0, 1, 2, 0]),
        items(&[0, 99]),
        Value::List(vec![Value::Null]),
    ] {
        let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
        assert!(wave
            .admit_each(
                &graph,
                &scope(),
                node,
                &request,
                BTreeSet::from(["resource".into()])
            )
            .is_err());
        assert!(wave.each.is_empty());
        assert!(wave.branch_keys.is_empty());
        assert!(wave.started.is_empty());
        assert_eq!(wave.executions, 0);
        assert_eq!(wave.bytes, 0);
    }
    let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
    wave.executions = MAX_ROOT_NODES;
    assert!(wave
        .admit_each(
            &graph,
            &scope(),
            node,
            &items(&[0]),
            BTreeSet::from(["resource".into()])
        )
        .is_err());
    assert!(wave.each.is_empty());
}

#[test]
fn original_order_and_typed_domain_outcomes_survive_reverse_and_duplicate_completion() {
    let (declarations, policy) = fixture();
    let graph = compile(&declarations, &policy).unwrap();
    let scope = scope();
    let node = &declarations[0].nodes[1];
    for routes in [&[][..], &[0][..], &[0, 1, 0][..]] {
        let mut wave = Wave::new(&graph, "alpha/submit").unwrap();
        wave.admit_each(
            &graph,
            &scope,
            node,
            &items(routes),
            scope.authority.resources.clone(),
        )
        .unwrap();
        assert_eq!(wave.each[&node.id].branches.len(), routes.len());
        for index in (0..routes.len()).rev() {
            let key = wave.each[&node.id].branches[index].key.clone();
            assert_eq!(
                wave.branch_identity(&key),
                Some((node.id.as_str(), index as u32))
            );
            let outcome = if index == 1 {
                Outcome::Domain(Value::String("failed".into()))
            } else {
                Outcome::Success(Value::String(format!("result-{index}")))
            };
            wave.complete(&graph, &scope, &key, &outcome).unwrap();
            let retained = wave.each[&node.id].branches[index].outcome.clone();
            let bytes = wave.bytes;
            wave.complete(
                &graph,
                &scope,
                &key,
                &Outcome::Success(Value::String("duplicate".into())),
            )
            .unwrap();
            assert_eq!(wave.each[&node.id].branches[index].outcome, retained);
            assert_eq!(wave.bytes, bytes);
            assert!(wave.completed(&key));
        }
        assert!(!wave.done.contains(&node.id));
        for (index, branch) in wave.each[&node.id].branches.iter().enumerate() {
            assert_eq!(
                branch.outcome,
                Some(if index == 1 {
                    Value::map([("Err", Value::String("failed".into()))])
                } else {
                    Value::map([("Ok", Value::String(format!("result-{index}")))])
                })
            );
        }
    }
}
