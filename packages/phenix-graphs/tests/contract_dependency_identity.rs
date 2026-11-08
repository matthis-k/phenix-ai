use phenix_graph::{
    DeclaredValue, GraphBuilder, GraphContract, GraphUnit, InterfaceShape, ModulePackage,
};

fn contract(id: &str) -> GraphContract {
    GraphContract::new(
        id,
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::local::<i64>())
            .unwrap()
            .output("value", DeclaredValue::local::<i64>())
            .unwrap(),
    )
    .unwrap()
}

fn service(name: &str) -> GraphBuilder {
    let mut graph = GraphBuilder::new(name);
    let node = graph.node("internal", |value: i64| value).unwrap();
    graph
        .expose_input_slot("run", "value", node.inlet())
        .unwrap();
    graph.expose_output("value", node.output()).unwrap();
    graph
}

fn wrapper(name: &str, dependency: &ModulePackage) -> GraphBuilder {
    let mut graph = GraphBuilder::new(name);
    graph.instance("service", dependency).unwrap();
    graph
}

#[test]
fn nested_node_and_module_claim_edits_reject_divergent_imports_in_both_orders() {
    for path in ["service/internal", "service"] {
        let dependency = service("service").module();
        let unchanged = wrapper("unchanged-wrapper", &dependency).module();
        let mut changed = wrapper("changed-wrapper", &dependency);
        let required = contract("service@1");
        changed.implements(path, &required).unwrap();
        let changed = changed.module();
        for changed_first in [false, true] {
            let (first, second) = if changed_first {
                (&changed, &unchanged)
            } else {
                (&unchanged, &changed)
            };
            let mut graph = GraphBuilder::new("root");
            graph.instance("first", first).unwrap();
            let error = graph.instance("second", second).err().unwrap();
            assert!(
                error.contains("conflicting module definition \"service\""),
                "{error}"
            );
            assert!(graph.inspect("second").is_err());
            let path = format!("first/{path}");
            assert_eq!(
                required
                    .check_implementation(&graph.inspect(&path).unwrap())
                    .is_ok(),
                changed_first
            );
            let compiled = graph.compile().unwrap();
            assert_eq!(
                required
                    .check_implementation(&compiled.find(&path).unwrap())
                    .is_ok(),
                changed_first
            );
        }
    }
}

#[test]
fn cloned_packages_with_divergent_dependency_claims_cannot_hide_behind_unique_wrappers() {
    let dependency = service("a-service").module();
    let original = wrapper("cloned-wrapper", &dependency);
    let mut changed = original.clone();
    changed
        .implements("service/internal", &contract("changed@1"))
        .unwrap();
    let original = original.module();
    let changed = changed.module();
    let plain_wrapper = wrapper("plain-outer", &original).module();
    let changed_wrapper = wrapper("changed-outer", &changed).module();
    for changed_first in [false, true] {
        let (first, second) = if changed_first {
            (&changed_wrapper, &plain_wrapper)
        } else {
            (&plain_wrapper, &changed_wrapper)
        };
        let mut graph = GraphBuilder::new("root");
        graph.instance("first", first).unwrap();
        let error = graph.instance("second", second).err().unwrap();
        assert!(
            error.contains("conflicting module definition \"a-service\""),
            "{error}"
        );
        graph.compile().unwrap();
    }
}

#[test]
fn claim_edits_refresh_every_ancestor_alias_but_leave_unrelated_dependencies_reusable() {
    let dependency = service("service").module();
    let ancestor = wrapper("ancestor", &dependency).module();
    let other_ancestor = wrapper("other-ancestor", &dependency).module();
    let unrelated = service("unrelated").module();
    let mut graph = GraphBuilder::new("root");
    graph.instance("first", &ancestor).unwrap();
    graph.instance("alias", &other_ancestor).unwrap();
    graph.instance("unrelated", &unrelated).unwrap();
    let required = contract("changed@1");
    graph
        .implements("first/service/internal", &required)
        .unwrap();
    required
        .check_implementation(&graph.inspect("alias/service/internal").unwrap())
        .unwrap();
    let error = graph.instance("stale-first", &ancestor).err().unwrap();
    assert!(
        error.contains("conflicting module definition \"ancestor\""),
        "{error}"
    );
    let error = graph
        .instance("stale-alias", &other_ancestor)
        .err()
        .unwrap();
    assert!(
        error.contains("conflicting module definition \"other-ancestor\""),
        "{error}"
    );
    graph.instance("unrelated-again", &unrelated).unwrap();
    graph.compile().unwrap();
}

#[test]
fn repeated_existing_claim_does_not_diverge_dependency_identity() {
    let mut dependency = service("service");
    let required = contract("existing@1");
    dependency.implements("internal", &required).unwrap();
    let dependency = dependency.module();
    let mut first = wrapper("first-wrapper", &dependency);
    first.implements("service/internal", &required).unwrap();
    let first = first.module();
    let second = wrapper("second-wrapper", &dependency).module();
    let mut graph = GraphBuilder::new("root");
    graph.instance("first", &first).unwrap();
    graph.instance("second", &second).unwrap();
    graph.compile().unwrap();
}

fn requested_service(name: &str, requests: &[GraphContract]) -> ModulePackage {
    let mut graph = service(name);
    for required in requests {
        graph.implements("internal", required).unwrap();
        graph.require_contract("internal", required).unwrap();
    }
    graph.module()
}

#[test]
fn module_replacement_retains_every_existing_contract_request_atomically() {
    let first = contract("first@1");
    let second = contract("second@1");
    let original = requested_service("original", &[first.clone(), second.clone()]);
    let mut graph = GraphBuilder::new("root");
    let handle = graph.instance("service", &original).unwrap();
    let before = graph.inspect("service").unwrap().inspect().unwrap();
    for requests in [vec![], vec![first.clone()], vec![second.clone()]] {
        let replacement = requested_service("missing-request", &requests);
        let error = graph.replace("service", &replacement).unwrap_err();
        assert!(error.contains("removes contract request"), "{error}");
        assert_eq!(graph.inspect("service").unwrap().inspect().unwrap(), before);
    }
    // A rejected replacement neither revokes existing handles nor imports its package.
    graph
        .expose_output("still-valid", handle.local_output::<i64>("value").unwrap())
        .unwrap();
    let replacement = requested_service("missing-request", &[second.clone(), first.clone()]);
    graph.replace("service", &replacement).unwrap();
    graph.compile().unwrap();
}

#[test]
fn module_replacement_rejects_changed_request_definition_or_path() {
    let required = contract("service@1");
    let original = requested_service("original", std::slice::from_ref(&required));
    for changed_path in [false, true] {
        let mut replacement = service("replacement");
        replacement.implements("internal", &required).unwrap();
        if changed_path {
            replacement.require_contract("missing", &required).unwrap();
        } else {
            let changed = GraphContract::new(
                required.id(),
                InterfaceShape::new()
                    .input_slot("run", "value", DeclaredValue::local::<i64>())
                    .unwrap(),
            )
            .unwrap();
            replacement.require_contract("internal", &changed).unwrap();
        }
        let mut graph = GraphBuilder::new("root");
        graph.instance("service", &original).unwrap();
        assert!(graph.replace("service", replacement.module()).is_err());
        graph.compile().unwrap();
    }
}

#[test]
fn preserved_requests_remain_final_compile_obligations_after_replacement() {
    let required = contract("service@1");
    let original = requested_service("original", std::slice::from_ref(&required));
    let mut replacement = service("replacement");
    replacement.require_contract("internal", &required).unwrap();
    let mut graph = GraphBuilder::new("root");
    graph.instance("service", &original).unwrap();
    graph.replace("service", replacement.module()).unwrap();
    let error = graph.compile().err().unwrap().to_string();
    assert!(
        error.contains("service@1") && error.contains("missing explicit contract claim"),
        "{error}"
    );
}
