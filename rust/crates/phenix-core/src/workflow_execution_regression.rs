//! Cross-profile proof that the same workflow uses the resolved provider graph.
use crate::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentManifest, InterfaceId,
    InterfaceSchema, Kernel, KernelError, Key, LayerPolicy, LayerResult, PhenixValue,
    PluginExecution, PluginHost, PluginId, PluginInstance, PluginManifest,
    ProviderCompositionPolicy, ResolvedGeneration, ResolvedGenerationActivation,
    ServiceContribution, ServiceId, ServiceRole, Type, WorkflowDeclaration, WorkflowEdge,
    WorkflowFrame, WorkflowFrameDeclaration, WorkflowFrameSchema, WorkflowJoinAllPolicy,
    WorkflowJoinPolicy, WorkflowNode, WorkflowTopology,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const MODEL: &str = "fixture.workflow-model@1";
const TOOL: &str = "fixture.workflow-tool@1";
const TOPOLOGY: &str = "fixture.workflow-topology";
const BASIC: &str = "fixture.workflow-basic";
const ADVANCED: &str = "fixture.workflow-advanced";
const TOOL_PROVIDER: &str = "fixture.workflow-tool-provider";
const LOGGER: &str = "fixture.workflow-log-layer";

fn plugin_id(name: &str) -> PluginId {
    PluginId::parse(name).unwrap()
}

fn component_id(name: &str) -> ComponentId {
    ComponentId::parse(name).unwrap()
}

fn service(name: &str) -> ServiceId {
    ServiceId::parse(name).unwrap()
}

fn manifest(name: &str, execution: PluginExecution) -> PluginManifest {
    PluginManifest {
        id: plugin_id(name),
        version: 1,
        execution,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn provider(name: &str, contract: &str, priority: i32) -> ComponentManifest {
    ComponentManifest {
        id: component_id(name),
        owner: plugin_id(name),
        imports: Vec::new(),
        exports: vec![ComponentExport {
            interface: InterfaceId::parse(contract).unwrap(),
            schema: InterfaceSchema::default(),
            priority,
            required_authority: Authority::default(),
        }],
        listeners: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn topology() -> WorkflowDeclaration {
    WorkflowDeclaration {
        owner: component_id(TOPOLOGY),
        name: "turn".into(),
        topology: WorkflowTopology {
            entry: "model".into(),
            nodes: BTreeMap::from([
                (
                    "model".into(),
                    WorkflowNode {
                        import: InterfaceId::parse(MODEL).unwrap(),
                        branches: BTreeMap::from([
                            (
                                "tools".into(),
                                WorkflowEdge::Next {
                                    node: "tool".into(),
                                },
                            ),
                            ("final".into(), WorkflowEdge::Finish),
                        ]),
                    },
                ),
                (
                    "tool".into(),
                    WorkflowNode {
                        import: InterfaceId::parse(TOOL).unwrap(),
                        branches: BTreeMap::from([(
                            "done".into(),
                            WorkflowEdge::Next {
                                node: "model".into(),
                            },
                        )]),
                    },
                ),
            ]),
        },
    }
}

fn fixed_fork_workflow(policy: WorkflowJoinPolicy) -> WorkflowDeclaration {
    WorkflowDeclaration {
        owner: component_id(TOPOLOGY),
        name: "turn".into(),
        topology: WorkflowTopology {
            entry: "model".into(),
            nodes: BTreeMap::from([
                (
                    "model".into(),
                    WorkflowNode {
                        import: InterfaceId::parse(MODEL).unwrap(),
                        branches: BTreeMap::from([
                            ("final".into(), WorkflowEdge::Finish),
                            (
                                "tools".into(),
                                WorkflowEdge::Fork {
                                    branches: BTreeMap::from([
                                        ("alpha".into(), "alpha-tool".into()),
                                        ("beta".into(), "beta-tool".into()),
                                    ]),
                                    policy,
                                    outputs: BTreeMap::from([
                                        ("alpha".into(), vec![Key::parse("alpha").unwrap()]),
                                        ("beta".into(), vec![Key::parse("beta").unwrap()]),
                                    ]),
                                    on_success: Box::new(WorkflowEdge::Next {
                                        node: "model".into(),
                                    }),
                                    on_failure: Box::new(WorkflowEdge::Finish),
                                },
                            ),
                        ]),
                    },
                ),
                (
                    "alpha-tool".into(),
                    WorkflowNode {
                        import: InterfaceId::parse(TOOL).unwrap(),
                        branches: BTreeMap::from([
                            ("done".into(), WorkflowEdge::Finish),
                            ("failed".into(), WorkflowEdge::Fail),
                        ]),
                    },
                ),
                (
                    "beta-tool".into(),
                    WorkflowNode {
                        import: InterfaceId::parse(TOOL).unwrap(),
                        branches: BTreeMap::from([("done".into(), WorkflowEdge::Finish)]),
                    },
                ),
            ]),
        },
    }
}

fn selected_fork_generation(policy: WorkflowJoinPolicy) -> ResolvedGeneration {
    resolve_with_workflow(BASIC, false, fixed_fork_workflow(policy))
        .with_workflow_frame_schemas([WorkflowFrameDeclaration {
            owner: component_id(TOPOLOGY),
            name: "turn".into(),
            schema: WorkflowFrameSchema {
                revision: 1,
                slots: BTreeMap::from([
                    (Key::parse("alpha").unwrap(), Type::U64),
                    (Key::parse("beta").unwrap(), Type::U64),
                ]),
            },
        }])
        .unwrap()
}

fn selected_nested_fork_generation() -> ResolvedGeneration {
    let mut workflow =
        fixed_fork_workflow(WorkflowJoinPolicy::All(WorkflowJoinAllPolicy::CollectAll));
    workflow
        .topology
        .nodes
        .get_mut("alpha-tool")
        .unwrap()
        .branches
        .insert(
            "done".into(),
            WorkflowEdge::Fork {
                branches: BTreeMap::from([
                    ("first".into(), "inner-one".into()),
                    ("second".into(), "inner-two".into()),
                ]),
                policy: WorkflowJoinPolicy::All(WorkflowJoinAllPolicy::CollectAll),
                outputs: BTreeMap::new(),
                on_success: Box::new(WorkflowEdge::Finish),
                on_failure: Box::new(WorkflowEdge::Finish),
            },
        );
    for name in ["inner-one", "inner-two"] {
        workflow.topology.nodes.insert(
            name.into(),
            WorkflowNode {
                import: InterfaceId::parse(TOOL).unwrap(),
                branches: BTreeMap::from([("done".into(), WorkflowEdge::Finish)]),
            },
        );
    }
    resolve_with_workflow(BASIC, false, workflow)
        .with_workflow_frame_schemas([WorkflowFrameDeclaration {
            owner: component_id(TOPOLOGY),
            name: "turn".into(),
            schema: WorkflowFrameSchema {
                revision: 1,
                slots: BTreeMap::from([
                    (Key::parse("alpha").unwrap(), Type::U64),
                    (Key::parse("beta").unwrap(), Type::U64),
                ]),
            },
        }])
        .unwrap()
}

#[test]
fn nested_fork_yields_to_outer_siblings_without_new_root_or_binding() {
    let resolved = selected_nested_fork_generation();
    let kernel = started_kernel(&resolved, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let schema = resolved
        .generation_topology()
        .workflow(&component_id(TOPOLOGY), "turn")
        .unwrap()
        .frame_schema()
        .unwrap()
        .clone();
    let alpha = Key::parse("alpha").unwrap();
    let beta = Key::parse("beta").unwrap();
    let mut frame = WorkflowFrame::new(
        schema,
        BTreeMap::from([
            (alpha.clone(), PhenixValue::U64(0)),
            (beta.clone(), PhenixValue::U64(0)),
        ]),
    )
    .unwrap();
    let mut seen = Vec::new();
    let report = root
        .execute_workflow_with_frame(
            (&component_id(TOPOLOGY), "turn"),
            (&mut seen, &mut frame),
            |node, _, frame, seen| {
                if node.starts_with("inner-") || node == "beta-tool" {
                    assert_eq!(frame.get(&alpha), Some(&PhenixValue::U64(0)));
                }
                seen.push(node.to_owned());
                Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap())
            },
            |node, _, output, frame, _| {
                if node == "inner-one" {
                    frame.set(&alpha, PhenixValue::U64(40)).unwrap();
                } else if node == "beta-tool" {
                    frame.set(&beta, PhenixValue::U64(8)).unwrap();
                }
                match serde_json::from_slice::<PhenixValue>(output).unwrap() {
                    PhenixValue::String(outcome) => Ok::<String, String>(outcome),
                    _ => Err("unexpected mock outcome".into()),
                }
            },
            || false,
            None,
        )
        .unwrap();
    assert_eq!(report.final_outcome, "final");
    assert_eq!(report.executed_nodes, 6);
    assert_eq!(
        seen,
        [
            "model",
            "alpha-tool",
            "beta-tool",
            "inner-one",
            "inner-two",
            "model"
        ]
    );
    // Inner child-only modifications never escape an unselected Join slot.
    assert_eq!(frame.get(&alpha), Some(&PhenixValue::U64(0)));
    assert_eq!(frame.get(&beta), Some(&PhenixValue::U64(8)));
}

#[test]
fn non_agent_fork_join_executes_two_pinned_providers_with_isolated_frames() {
    let resolved =
        selected_fork_generation(WorkflowJoinPolicy::All(WorkflowJoinAllPolicy::CollectAll));
    let kernel = started_kernel(&resolved, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let schema = resolved
        .generation_topology()
        .workflow(&component_id(TOPOLOGY), "turn")
        .unwrap()
        .frame_schema()
        .unwrap()
        .clone();
    let alpha = Key::parse("alpha").unwrap();
    let beta = Key::parse("beta").unwrap();
    let mut frame = WorkflowFrame::new(
        schema,
        BTreeMap::from([
            (alpha.clone(), PhenixValue::U64(0)),
            (beta.clone(), PhenixValue::U64(0)),
        ]),
    )
    .unwrap();
    let mut seen = Vec::new();
    let report = root
        .execute_workflow_with_frame(
            (&component_id(TOPOLOGY), "turn"),
            (&mut seen, &mut frame),
            |node, _, frame, seen| {
                // Both children see only the state at their fork. The joined
                // outputs become visible only in the parent continuation.
                if node == "alpha-tool" || node == "beta-tool" {
                    assert_eq!(frame.get(&alpha), Some(&PhenixValue::U64(0)));
                    assert_eq!(frame.get(&beta), Some(&PhenixValue::U64(0)));
                }
                seen.push(format!("enter:{node}"));
                Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap())
            },
            |node, _, output, frame, seen| {
                seen.push(format!("exit:{node}"));
                if node == "alpha-tool" {
                    frame.set(&alpha, PhenixValue::U64(7)).unwrap();
                } else if node == "beta-tool" {
                    frame.set(&beta, PhenixValue::U64(11)).unwrap();
                }
                match serde_json::from_slice::<PhenixValue>(output).unwrap() {
                    PhenixValue::String(outcome) => Ok::<_, String>(outcome),
                    value => Err(format!("unrecognized mock outcome: {value:?}")),
                }
            },
            || false,
            None,
        )
        .unwrap();
    assert_eq!(report.executed_nodes, 4);
    assert_eq!(report.final_outcome, "final");
    assert_eq!(frame.get(&alpha), Some(&PhenixValue::U64(7)));
    assert_eq!(frame.get(&beta), Some(&PhenixValue::U64(11)));
    assert_eq!(
        seen,
        [
            "enter:model",
            "exit:model",
            "enter:alpha-tool",
            "exit:alpha-tool",
            "enter:beta-tool",
            "exit:beta-tool",
            "enter:model",
            "exit:model"
        ]
    );
}

#[test]
fn fork_fail_fast_does_not_invoke_sibling_after_failed_first_child() {
    let resolved =
        selected_fork_generation(WorkflowJoinPolicy::All(WorkflowJoinAllPolicy::FailFast));
    let kernel = started_kernel(&resolved, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let schema = resolved
        .generation_topology()
        .workflow(&component_id(TOPOLOGY), "turn")
        .unwrap()
        .frame_schema()
        .unwrap()
        .clone();
    let mut frame = WorkflowFrame::new(
        schema,
        BTreeMap::from([
            (Key::parse("alpha").unwrap(), PhenixValue::U64(0)),
            (Key::parse("beta").unwrap(), PhenixValue::U64(0)),
        ]),
    )
    .unwrap();
    let mut seen = Vec::new();
    let result = root
        .execute_workflow_with_frame(
            (&component_id(TOPOLOGY), "turn"),
            (&mut seen, &mut frame),
            |node, _, _, seen| {
                seen.push(node.to_owned());
                Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap())
            },
            |node, _, output, _, _| {
                if node == "alpha-tool" {
                    return Ok::<String, String>("failed".into());
                }
                match serde_json::from_slice::<PhenixValue>(output).unwrap() {
                    PhenixValue::String(outcome) => Ok(outcome),
                    _ => Err("unrecognized outcome".into()),
                }
            },
            || false,
            None,
        )
        .unwrap();
    assert_eq!(result.final_outcome, "tools/failure");
    assert_eq!(seen, ["model", "alpha-tool"]);
}

#[test]
fn quorum_one_settles_before_dispatching_unneeded_second_child() {
    let resolved = selected_fork_generation(WorkflowJoinPolicy::Quorum(
        std::num::NonZeroUsize::new(1).unwrap(),
    ));
    let kernel = started_kernel(&resolved, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let schema = resolved
        .generation_topology()
        .workflow(&component_id(TOPOLOGY), "turn")
        .unwrap()
        .frame_schema()
        .unwrap()
        .clone();
    let alpha = Key::parse("alpha").unwrap();
    let beta = Key::parse("beta").unwrap();
    let mut frame = WorkflowFrame::new(
        schema,
        BTreeMap::from([
            (alpha.clone(), PhenixValue::U64(0)),
            (beta.clone(), PhenixValue::U64(0)),
        ]),
    )
    .unwrap();
    let mut seen = Vec::new();
    let report = root
        .execute_workflow_with_frame(
            (&component_id(TOPOLOGY), "turn"),
            (&mut seen, &mut frame),
            |node, _, _, seen| {
                seen.push(node.to_owned());
                Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap())
            },
            |node, _, output, frame, _| {
                if node == "alpha-tool" {
                    frame.set(&alpha, PhenixValue::U64(23)).unwrap();
                }
                match serde_json::from_slice::<PhenixValue>(output).unwrap() {
                    PhenixValue::String(outcome) => Ok::<String, String>(outcome),
                    _ => Err("invalid provider result".into()),
                }
            },
            || false,
            None,
        )
        .unwrap();
    assert_eq!(report.executed_nodes, 3);
    assert_eq!(seen, ["model", "alpha-tool", "model"]);
    assert_eq!(frame.get(&alpha), Some(&PhenixValue::U64(23)));
    assert_eq!(frame.get(&beta), Some(&PhenixValue::U64(0)));
}

#[test]
fn first_completed_stops_before_the_second_child_invocation() {
    let resolved = selected_fork_generation(WorkflowJoinPolicy::FirstCompleted);
    let kernel = started_kernel(&resolved, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let schema = resolved
        .generation_topology()
        .workflow(&component_id(TOPOLOGY), "turn")
        .unwrap()
        .frame_schema()
        .unwrap()
        .clone();
    let alpha = Key::parse("alpha").unwrap();
    let beta = Key::parse("beta").unwrap();
    let mut frame = WorkflowFrame::new(
        schema,
        BTreeMap::from([
            (alpha.clone(), PhenixValue::U64(0)),
            (beta.clone(), PhenixValue::U64(0)),
        ]),
    )
    .unwrap();
    let mut seen = Vec::new();
    let report = root
        .execute_workflow_with_frame(
            (&component_id(TOPOLOGY), "turn"),
            (&mut seen, &mut frame),
            |node, _, _, seen| {
                seen.push(node.to_owned());
                Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap())
            },
            |node, _, output, frame, _| {
                if node == "alpha-tool" {
                    frame.set(&alpha, PhenixValue::U64(31)).unwrap();
                }
                match serde_json::from_slice::<PhenixValue>(output).unwrap() {
                    PhenixValue::String(outcome) => Ok::<_, String>(outcome),
                    _ => Err("invalid result".into()),
                }
            },
            || false,
            None,
        )
        .unwrap();
    assert_eq!(report.executed_nodes, 3);
    assert_eq!(report.final_outcome, "final");
    assert_eq!(seen, ["model", "alpha-tool", "model"]);
    assert_eq!(frame.get(&alpha), Some(&PhenixValue::U64(31)));
    assert_eq!(frame.get(&beta), Some(&PhenixValue::U64(0)));
}

#[test]
fn first_success_ignores_declared_failure_without_swallowing_an_error() {
    let resolved = selected_fork_generation(WorkflowJoinPolicy::FirstSuccess);
    let kernel = started_kernel(&resolved, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let schema = resolved
        .generation_topology()
        .workflow(&component_id(TOPOLOGY), "turn")
        .unwrap()
        .frame_schema()
        .unwrap()
        .clone();
    let alpha = Key::parse("alpha").unwrap();
    let beta = Key::parse("beta").unwrap();
    let mut frame = WorkflowFrame::new(
        schema,
        BTreeMap::from([
            (alpha.clone(), PhenixValue::U64(0)),
            (beta.clone(), PhenixValue::U64(0)),
        ]),
    )
    .unwrap();
    let mut seen = Vec::new();
    let report = root
        .execute_workflow_with_frame(
            (&component_id(TOPOLOGY), "turn"),
            (&mut seen, &mut frame),
            |node, _, _, seen| {
                seen.push(node.to_owned());
                Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap())
            },
            |node, _, output, frame, _| {
                if node == "alpha-tool" {
                    return Ok::<String, String>("failed".into());
                }
                if node == "beta-tool" {
                    frame.set(&beta, PhenixValue::U64(17)).unwrap();
                }
                match serde_json::from_slice::<PhenixValue>(output).unwrap() {
                    PhenixValue::String(outcome) => Ok::<_, String>(outcome),
                    _ => Err("invalid result".into()),
                }
            },
            || false,
            None,
        )
        .unwrap();
    assert_eq!(report.final_outcome, "final");
    assert_eq!(seen, ["model", "alpha-tool", "beta-tool", "model"]);
    assert_eq!(frame.get(&alpha), Some(&PhenixValue::U64(0)));
    assert_eq!(frame.get(&beta), Some(&PhenixValue::U64(17)));
}

#[test]
fn fork_adapter_error_is_not_treated_as_declared_failure() {
    let resolved =
        selected_fork_generation(WorkflowJoinPolicy::All(WorkflowJoinAllPolicy::FailFast));
    let kernel = started_kernel(&resolved, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let schema = resolved
        .generation_topology()
        .workflow(&component_id(TOPOLOGY), "turn")
        .unwrap()
        .frame_schema()
        .unwrap()
        .clone();
    let mut frame = WorkflowFrame::new(
        schema,
        BTreeMap::from([
            (Key::parse("alpha").unwrap(), PhenixValue::U64(0)),
            (Key::parse("beta").unwrap(), PhenixValue::U64(0)),
        ]),
    )
    .unwrap();
    let mut seen = Vec::new();
    let result = root.execute_workflow_with_frame(
        (&component_id(TOPOLOGY), "turn"),
        (&mut seen, &mut frame),
        |node, _, _, seen| {
            seen.push(node.to_owned());
            if node == "alpha-tool" {
                return Err::<Vec<u8>, String>("invalid request preparation".into());
            }
            Ok(serde_json::to_vec(&PhenixValue::Unit).unwrap())
        },
        |_, _, output, _, _| match serde_json::from_slice::<PhenixValue>(output).unwrap() {
            PhenixValue::String(outcome) => Ok::<String, String>(outcome),
            _ => Err("unrecognized provider result".into()),
        },
        || false,
        None,
    );
    assert!(matches!(
        result,
        Err(crate::WorkflowRunError::NodeFailed { node, .. }) if node == "alpha-tool"
    ));
    assert_eq!(seen, ["model", "alpha-tool"]);
}

#[test]
fn unframed_fork_fails_before_its_first_provider_call() {
    let resolved = selected_fork_generation(WorkflowJoinPolicy::FirstSuccess);
    let kernel = started_kernel(&resolved, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let result = root.execute_workflow(
        (&component_id(TOPOLOGY), "turn"),
        &mut (),
        |_, _, _| -> Result<Vec<u8>, String> { panic!("no import may be invoked") },
        |_, _, _, _| -> Result<String, String> { panic!("no result may be projected") },
        || false,
        None,
    );
    assert!(matches!(
        result,
        Err(crate::WorkflowRunError::StructuredFrameRequired { .. })
    ));
}

fn map_fork_workflow() -> WorkflowDeclaration {
    WorkflowDeclaration {
        owner: component_id(TOPOLOGY),
        name: "turn".into(),
        topology: WorkflowTopology {
            entry: "model".into(),
            nodes: BTreeMap::from([
                (
                    "model".into(),
                    WorkflowNode {
                        import: InterfaceId::parse(MODEL).unwrap(),
                        branches: BTreeMap::from([
                            ("final".into(), WorkflowEdge::Finish),
                            (
                                "tools".into(),
                                WorkflowEdge::MapFork {
                                    collection: Key::parse("items").unwrap(),
                                    item_slot: Key::parse("item").unwrap(),
                                    child_output_slot: Key::parse("result").unwrap(),
                                    output_slot: Key::parse("results").unwrap(),
                                    max_children: 4,
                                    branch_entry: "map-tool".into(),
                                    policy: WorkflowJoinPolicy::All(
                                        WorkflowJoinAllPolicy::CollectAll,
                                    ),
                                    on_success: Box::new(WorkflowEdge::Next {
                                        node: "model".into(),
                                    }),
                                    on_failure: Box::new(WorkflowEdge::Finish),
                                },
                            ),
                        ]),
                    },
                ),
                (
                    "map-tool".into(),
                    WorkflowNode {
                        import: InterfaceId::parse(TOOL).unwrap(),
                        branches: BTreeMap::from([("done".into(), WorkflowEdge::Finish)]),
                    },
                ),
            ]),
        },
    }
}

fn selected_map_generation() -> ResolvedGeneration {
    resolve_with_workflow(BASIC, false, map_fork_workflow())
        .with_workflow_frame_schemas([WorkflowFrameDeclaration {
            owner: component_id(TOPOLOGY),
            name: "turn".into(),
            schema: WorkflowFrameSchema {
                revision: 1,
                slots: BTreeMap::from([
                    (
                        Key::parse("items").unwrap(),
                        Type::List(Box::new(Type::U64)),
                    ),
                    (Key::parse("item").unwrap(), Type::U64),
                    (Key::parse("result").unwrap(), Type::U64),
                    (
                        Key::parse("results").unwrap(),
                        Type::List(Box::new(Type::U64)),
                    ),
                ]),
            },
        }])
        .unwrap()
}

#[test]
fn empty_map_all_is_vacuously_successful_without_child_invocation() {
    let resolved = selected_map_generation();
    let kernel = started_kernel(&resolved, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let schema = resolved
        .generation_topology()
        .workflow(&component_id(TOPOLOGY), "turn")
        .unwrap()
        .frame_schema()
        .unwrap()
        .clone();
    let output = Key::parse("results").unwrap();
    let mut frame = WorkflowFrame::new(
        schema,
        BTreeMap::from([
            (Key::parse("items").unwrap(), PhenixValue::List(Vec::new())),
            (Key::parse("item").unwrap(), PhenixValue::U64(0)),
            (Key::parse("result").unwrap(), PhenixValue::U64(0)),
            (
                output.clone(),
                PhenixValue::List(vec![PhenixValue::U64(42)]),
            ),
        ]),
    )
    .unwrap();
    let mut executed = Vec::new();
    let report = root
        .execute_workflow_with_frame(
            (&component_id(TOPOLOGY), "turn"),
            (&mut executed, &mut frame),
            |node, _, _, executed| {
                executed.push(node.to_owned());
                Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap())
            },
            |_, _, output, _, _| match serde_json::from_slice::<PhenixValue>(output).unwrap() {
                PhenixValue::String(outcome) => Ok::<String, String>(outcome),
                _ => Err("invalid outcome".into()),
            },
            || false,
            None,
        )
        .unwrap();
    assert_eq!(report.final_outcome, "final");
    assert_eq!(report.executed_nodes, 2);
    assert_eq!(executed, ["model", "model"]);
    assert_eq!(frame.get(&output), Some(&PhenixValue::List(Vec::new())));
}

#[test]
fn non_agent_bounded_map_fanout_collects_typed_results_without_leaking_child_frames() {
    let resolved = selected_map_generation();
    let kernel = started_kernel(&resolved, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let schema = resolved
        .generation_topology()
        .workflow(&component_id(TOPOLOGY), "turn")
        .unwrap()
        .frame_schema()
        .unwrap()
        .clone();
    let item = Key::parse("item").unwrap();
    let result_slot = Key::parse("result").unwrap();
    let results = Key::parse("results").unwrap();
    let mut frame = WorkflowFrame::new(
        schema,
        BTreeMap::from([
            (
                Key::parse("items").unwrap(),
                PhenixValue::List([2, 4, 6].into_iter().map(PhenixValue::U64).collect()),
            ),
            (item.clone(), PhenixValue::U64(0)),
            (result_slot.clone(), PhenixValue::U64(0)),
            (results.clone(), PhenixValue::List(Vec::new())),
        ]),
    )
    .unwrap();
    let mut seen = Vec::new();
    let report = root
        .execute_workflow_with_frame(
            (&component_id(TOPOLOGY), "turn"),
            (&mut seen, &mut frame),
            |node, _, frame, seen| {
                if node == "map-tool" {
                    seen.push(match frame.get(&item) {
                        Some(PhenixValue::U64(value)) => *value,
                        _ => panic!("map child must have its typed item"),
                    });
                }
                Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap())
            },
            |node, _, output, frame, _| {
                if node == "map-tool" {
                    let value = match frame.get(&item) {
                        Some(PhenixValue::U64(value)) => *value,
                        _ => return Err("map item absent".to_owned()),
                    };
                    frame
                        .set(&result_slot, PhenixValue::U64(value * 3))
                        .unwrap();
                }
                match serde_json::from_slice::<PhenixValue>(output).unwrap() {
                    PhenixValue::String(outcome) => Ok::<_, String>(outcome),
                    _ => Err("unexpected mock provider response".into()),
                }
            },
            || false,
            None,
        )
        .unwrap();
    assert_eq!(report.executed_nodes, 5);
    assert_eq!(report.final_outcome, "final");
    assert_eq!(seen, [2, 4, 6]);
    assert_eq!(
        frame.get(&results),
        Some(&PhenixValue::List(vec![
            PhenixValue::U64(6),
            PhenixValue::U64(12),
            PhenixValue::U64(18),
        ]))
    );
    assert_eq!(frame.get(&item), Some(&PhenixValue::U64(0)));
    assert_eq!(frame.get(&result_slot), Some(&PhenixValue::U64(0)));
}

#[test]
fn child_scope_cancellation_stops_before_provider_dispatch() {
    use std::cell::Cell;

    let resolved = selected_fork_generation(WorkflowJoinPolicy::FirstSuccess);
    let kernel = started_kernel(&resolved, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let schema = resolved
        .generation_topology()
        .workflow(&component_id(TOPOLOGY), "turn")
        .unwrap()
        .frame_schema()
        .unwrap()
        .clone();
    let mut frame = WorkflowFrame::new(
        schema,
        BTreeMap::from([
            (Key::parse("alpha").unwrap(), PhenixValue::U64(0)),
            (Key::parse("beta").unwrap(), PhenixValue::U64(0)),
        ]),
    )
    .unwrap();
    let cancellation = Cell::new(false);
    let mut prepared = Vec::new();
    let result = root.execute_workflow_with_frame(
        (&component_id(TOPOLOGY), "turn"),
        (&mut prepared, &mut frame),
        |node, _, _, prepared| {
            prepared.push(node.to_owned());
            if node == "alpha-tool" {
                cancellation.set(true);
            }
            Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap())
        },
        |_, _, output, _, _| match serde_json::from_slice::<PhenixValue>(output).unwrap() {
            PhenixValue::String(outcome) => Ok::<_, String>(outcome),
            _ => Err("invalid provider response".into()),
        },
        || cancellation.get(),
        None,
    );
    assert!(matches!(
        result,
        Err(crate::WorkflowRunError::Cancelled {
            executed_nodes: 1,
            ..
        })
    ));
    assert_eq!(prepared, ["model", "alpha-tool"]);
}

#[test]
fn map_max_child_limit_rejects_before_dispatching_a_mapped_provider() {
    let resolved = selected_map_generation();
    let kernel = started_kernel(&resolved, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let schema = resolved
        .generation_topology()
        .workflow(&component_id(TOPOLOGY), "turn")
        .unwrap()
        .frame_schema()
        .unwrap()
        .clone();
    let mut frame = WorkflowFrame::new(
        schema,
        BTreeMap::from([
            (
                Key::parse("items").unwrap(),
                PhenixValue::List((1..=5).map(PhenixValue::U64).collect()),
            ),
            (Key::parse("item").unwrap(), PhenixValue::U64(0)),
            (Key::parse("result").unwrap(), PhenixValue::U64(0)),
            (
                Key::parse("results").unwrap(),
                PhenixValue::List(Vec::new()),
            ),
        ]),
    )
    .unwrap();
    let mut seen = Vec::new();
    let outcome = root.execute_workflow_with_frame(
        (&component_id(TOPOLOGY), "turn"),
        (&mut seen, &mut frame),
        |node, _, _, seen| {
            seen.push(node.to_owned());
            Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap())
        },
        |_, _, output, _, _| match serde_json::from_slice::<PhenixValue>(output).unwrap() {
            PhenixValue::String(outcome) => Ok::<_, String>(outcome),
            _ => Err("invalid provider response".into()),
        },
        || false,
        None,
    );
    assert!(matches!(
        outcome,
        Err(crate::WorkflowRunError::InvalidMapInput { .. })
    ));
    assert_eq!(seen, ["model"]);
}

fn typed_include_generation_unbound() -> ResolvedGeneration {
    let owner = component_id(TOPOLOGY);
    let parent = WorkflowDeclaration {
        owner: owner.clone(),
        name: "turn".into(),
        topology: WorkflowTopology {
            entry: "model".into(),
            nodes: BTreeMap::from([(
                "model".into(),
                WorkflowNode {
                    import: InterfaceId::parse(MODEL).unwrap(),
                    branches: BTreeMap::from([
                        ("final".into(), WorkflowEdge::Finish),
                        (
                            "tools".into(),
                            WorkflowEdge::IncludeMapped {
                                workflow: "reusable".into(),
                                site: "answer".into(),
                                inputs: BTreeMap::from([(
                                    Key::parse("parent_input").unwrap(),
                                    Key::parse("child_input").unwrap(),
                                )]),
                                outputs: BTreeMap::from([(
                                    Key::parse("child_output").unwrap(),
                                    Key::parse("parent_output").unwrap(),
                                )]),
                                on_exit: BTreeMap::from([(
                                    "done".into(),
                                    WorkflowEdge::Next {
                                        node: "model".into(),
                                    },
                                )]),
                            },
                        ),
                    ]),
                },
            )]),
        },
    };
    let child = WorkflowDeclaration {
        owner: owner.clone(),
        name: "reusable".into(),
        topology: WorkflowTopology {
            entry: "work".into(),
            nodes: BTreeMap::from([(
                "work".into(),
                WorkflowNode {
                    import: InterfaceId::parse(TOOL).unwrap(),
                    branches: BTreeMap::from([("done".into(), WorkflowEdge::Finish)]),
                },
            )]),
        },
    };
    let _ = owner;
    resolve_with_workflows(BASIC, false, vec![parent, child])
}

fn typed_include_generation() -> ResolvedGeneration {
    typed_include_generation_unbound()
        .with_workflow_frame_schemas([WorkflowFrameDeclaration {
            owner: component_id(TOPOLOGY),
            name: "turn".into(),
            schema: WorkflowFrameSchema {
                revision: 1,
                slots: BTreeMap::from([
                    (Key::parse("parent_input").unwrap(), Type::U64),
                    (Key::parse("child_input").unwrap(), Type::U64),
                    (Key::parse("child_output").unwrap(), Type::U64),
                    (Key::parse("parent_output").unwrap(), Type::U64),
                ]),
            },
        }])
        .unwrap()
}

#[test]
fn mapped_inlined_subplan_passes_typed_input_output_through_pinned_provider() {
    let generation = typed_include_generation();
    let kernel = started_kernel(&generation, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let schema = generation
        .generation_topology()
        .workflow(&component_id(TOPOLOGY), "turn")
        .unwrap()
        .frame_schema()
        .unwrap()
        .clone();
    let parent_input = Key::parse("parent_input").unwrap();
    let child_input = Key::parse("child_input").unwrap();
    let child_output = Key::parse("child_output").unwrap();
    let parent_output = Key::parse("parent_output").unwrap();
    let mut frame = WorkflowFrame::new(
        schema,
        BTreeMap::from([
            (parent_input.clone(), PhenixValue::U64(6)),
            (child_input.clone(), PhenixValue::U64(0)),
            (child_output.clone(), PhenixValue::U64(0)),
            (parent_output.clone(), PhenixValue::U64(0)),
        ]),
    )
    .unwrap();
    let mut seen = Vec::new();
    let report = root
        .execute_workflow_with_frame(
            (&component_id(TOPOLOGY), "turn"),
            (&mut seen, &mut frame),
            |node, _, frame, seen| {
                if node == "__include__/answer/work" {
                    assert_eq!(frame.get(&child_input), Some(&PhenixValue::U64(6)));
                    assert_eq!(frame.get(&parent_input), Some(&PhenixValue::U64(6)));
                }
                seen.push(node.to_owned());
                Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap())
            },
            |node, _, output, frame, _| {
                if node == "__include__/answer/work" {
                    frame.set(&child_output, PhenixValue::U64(18)).unwrap();
                }
                match serde_json::from_slice::<PhenixValue>(output).unwrap() {
                    PhenixValue::String(outcome) => Ok::<String, String>(outcome),
                    _ => Err("unexpected mock output".into()),
                }
            },
            || false,
            None,
        )
        .unwrap();
    assert_eq!(report.executed_nodes, 3);
    assert_eq!(report.final_outcome, "final");
    assert_eq!(seen, ["model", "__include__/answer/work", "model",]);
    assert_eq!(frame.get(&child_input), Some(&PhenixValue::U64(6)));
    assert_eq!(frame.get(&child_output), Some(&PhenixValue::U64(18)));
    assert_eq!(frame.get(&parent_output), Some(&PhenixValue::U64(18)));
}

#[test]
fn mapped_subplan_incompatible_type_rejects_before_provider_dispatch() {
    let declaration = typed_include_generation_unbound();
    let altered = WorkflowFrameDeclaration {
        owner: component_id(TOPOLOGY),
        name: "turn".into(),
        schema: WorkflowFrameSchema {
            revision: 1,
            slots: BTreeMap::from([
                (Key::parse("parent_input").unwrap(), Type::String),
                (Key::parse("child_input").unwrap(), Type::U64),
                (Key::parse("child_output").unwrap(), Type::U64),
                (Key::parse("parent_output").unwrap(), Type::U64),
            ]),
        },
    };
    let outcome = declaration.with_workflow_frame_schemas([altered]);
    assert!(matches!(
        outcome,
        Err(crate::GenerationResolutionError::InvalidWorkflow {
            error: crate::workflow::WorkflowCompileError::InvalidFrameTransfer { .. },
            ..
        })
    ));
}

fn resolve(selected: &str, logging: bool) -> ResolvedGeneration {
    resolve_with_workflow(selected, logging, topology())
}

fn resolve_with_workflow(
    selected: &str,
    logging: bool,
    workflow: WorkflowDeclaration,
) -> ResolvedGeneration {
    resolve_with_workflows(selected, logging, vec![workflow])
}

fn resolve_with_workflows(
    selected: &str,
    logging: bool,
    workflows: Vec<WorkflowDeclaration>,
) -> ResolvedGeneration {
    let policy = ProviderCompositionPolicy::new()
        .with_explicit_binding(InterfaceId::parse(MODEL).unwrap(), component_id(selected));
    let mut plugins = vec![
        manifest(TOPOLOGY, PluginExecution::ResourceOnly),
        manifest(BASIC, PluginExecution::Embedded),
        manifest(ADVANCED, PluginExecution::Embedded),
        manifest(TOOL_PROVIDER, PluginExecution::Embedded),
    ];
    let mut layers = BTreeMap::new();
    if logging {
        let mut logger = manifest(LOGGER, PluginExecution::Embedded);
        logger.services.push(ServiceContribution {
            service: service(MODEL),
            role: ServiceRole::Layer,
            priority: 100,
            required_authority: Authority::default(),
        });
        plugins.push(logger);
        layers.insert(
            service(MODEL),
            vec![LayerPolicy {
                plugin: plugin_id(LOGGER),
                priority: 100,
                required: true,
                enabled: true,
            }],
        );
    }
    ResolvedGeneration::resolve_with_composition_policies(
        plugins,
        [
            ComponentManifest {
                id: component_id(TOPOLOGY),
                owner: plugin_id(TOPOLOGY),
                imports: [MODEL, TOOL]
                    .into_iter()
                    .map(|name| ComponentImport {
                        interface: InterfaceId::parse(name).unwrap(),
                        schema: InterfaceSchema::default(),
                        required: true,
                        authority: Authority::default(),
                    })
                    .collect(),
                exports: Vec::new(),
                listeners: Vec::new(),
                maximum_authority: Authority::default(),
            },
            provider(BASIC, MODEL, 100),
            provider(ADVANCED, MODEL, 10),
            provider(TOOL_PROVIDER, TOOL, 100),
        ],
        [],
        [],
        [],
        [],
        layers,
        policy,
        &Authority::default(),
    )
    .unwrap()
    .with_workflows(workflows)
    .unwrap()
}

struct MockNode {
    kind: &'static str,
    model_calls: u32,
}

impl PluginInstance for MockNode {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke_component(
        &mut self,
        _component: &ComponentId,
        _service: &ServiceId,
        _input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let outcome = if self.kind == "tool" {
            "done"
        } else {
            self.model_calls += 1;
            if self.model_calls == 1 {
                "tools"
            } else {
                "final"
            }
        };
        serde_json::to_vec(&PhenixValue::String(outcome.to_owned()))
            .map_err(|error| error.to_string())
    }
}

struct LoggingLayer {
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl PluginInstance for LoggingLayer {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke_layer(
        &mut self,
        _service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<LayerResult, String> {
        self.log.lock().unwrap().push("before");
        let result = host
            .continue_service(input, host.authority())
            .map_err(|error| error.to_string())?;
        self.log.lock().unwrap().push("after");
        Ok(LayerResult::Handled(result))
    }
}

fn started_kernel(resolved: &ResolvedGeneration, log: &Arc<Mutex<Vec<&'static str>>>) -> Kernel {
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    kernel.activate_resolved_generation(resolved).unwrap();
    for (name, kind) in [
        (BASIC, "basic"),
        (ADVANCED, "advanced"),
        (TOOL_PROVIDER, "tool"),
    ] {
        let id = plugin_id(name);
        kernel
            .register_embedded_factory(id, move || {
                Box::new(MockNode {
                    kind,
                    model_calls: 0,
                })
            })
            .unwrap();
    }
    if resolved
        .plugins()
        .iter()
        .any(|plugin| plugin.id == plugin_id(LOGGER))
    {
        let log = Arc::clone(log);
        kernel
            .register_embedded_factory(plugin_id(LOGGER), move || {
                Box::new(LoggingLayer {
                    log: Arc::clone(&log),
                })
            })
            .unwrap();
    }
    kernel.activate_all().unwrap();
    kernel
}

#[test]
fn basic_and_advanced_execute_identical_topology_with_different_providers() {
    let basic = resolve(BASIC, false);
    let advanced = resolve(ADVANCED, true);
    assert_eq!(
        basic.workflows()[0].topology,
        advanced.workflows()[0].topology
    );
    let mut histories = Vec::new();
    for (resolved, expected_provider, logging) in
        [(basic, BASIC, false), (advanced, ADVANCED, true)]
    {
        let log = Arc::new(Mutex::new(Vec::new()));
        let kernel = started_kernel(&resolved, &log);
        let root = kernel.root_execution_handle(&Authority::default());
        let chosen = resolved
            .generation_topology()
            .workflow(&component_id(TOPOLOGY), "turn")
            .unwrap()
            .bound_import(&InterfaceId::parse(MODEL).unwrap())
            .unwrap();
        assert_eq!(chosen.exporter(), &component_id(expected_provider));

        let mut seen = Vec::new();
        let report = root
            .execute_workflow(
                (&component_id(TOPOLOGY), "turn"),
                &mut seen,
                |_, _, _| Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap()),
                |node, _, output, history| {
                    history.push(node.to_owned());
                    match serde_json::from_slice::<PhenixValue>(output).unwrap() {
                        PhenixValue::String(value) => Ok::<_, String>(value),
                        other => Err(format!("unexpected outcome: {other:?}")),
                    }
                },
                || false,
                None,
            )
            .unwrap();
        assert_eq!(report.executed_nodes, 3);
        assert_eq!(
            log.lock().unwrap().as_slice(),
            if logging {
                &["before", "after", "before", "after"][..]
            } else {
                &[][..]
            }
        );
        histories.push(seen);
    }
    assert_eq!(histories[0], histories[1]);
    assert_eq!(histories[0], ["model", "tool", "model"]);
}

#[test]
fn frame_contracts_change_generation_and_reject_invalid_candidate_inputs() {
    let ordinary = resolve(BASIC, false);
    let owner = component_id(TOPOLOGY);
    let counter = Key::parse("counter").unwrap();
    let declaration = WorkflowFrameDeclaration {
        owner: owner.clone(),
        name: "turn".into(),
        schema: WorkflowFrameSchema {
            revision: 1,
            slots: BTreeMap::from([(counter.clone(), Type::U64)]),
        },
    };
    assert_eq!(
        ordinary
            .clone()
            .with_workflow_frame_schemas(std::iter::empty())
            .unwrap()
            .generation(),
        ordinary.generation(),
        "an absent contribution cannot change generation identity"
    );
    let typed = ordinary
        .clone()
        .with_workflow_frame_schemas([declaration.clone()])
        .unwrap();
    assert_ne!(typed.generation(), ordinary.generation());
    assert_eq!(
        typed
            .generation_topology()
            .workflow(&owner, "turn")
            .unwrap()
            .frame_schema(),
        Some(&declaration.schema)
    );
    assert_eq!(
        typed
            .clone()
            .with_workflow_frame_schemas([declaration.clone()])
            .unwrap()
            .generation(),
        typed.generation()
    );
    assert!(matches!(
        ordinary
            .clone()
            .with_workflow_frame_schemas([declaration.clone(), declaration.clone()]),
        Err(crate::GenerationResolutionError::DuplicateFrameSchema { .. })
    ));
    let mut different = declaration.clone();
    different.schema.revision = 2;
    assert_ne!(
        ordinary
            .clone()
            .with_workflow_frame_schemas([different.clone()])
            .unwrap()
            .generation(),
        typed.generation()
    );
    assert!(matches!(
        typed.clone().with_workflow_frame_schemas([different]),
        Err(crate::GenerationResolutionError::FrameSchemasAlreadyBound)
    ));
    assert!(matches!(
        typed
            .clone()
            .with_workflow_frame_schemas(std::iter::empty()),
        Err(crate::GenerationResolutionError::FrameSchemasAlreadyBound)
    ));
    let mut missing = declaration.clone();
    missing.name = "unselected".into();
    assert!(matches!(
        ordinary.clone().with_workflow_frame_schemas([missing]),
        Err(crate::GenerationResolutionError::MissingFrameWorkflow { .. })
    ));
    let mut forbidden = declaration;
    forbidden.schema.slots.insert(
        counter,
        Type::Object {
            contract: InterfaceId::parse("fixture.forbidden@1").unwrap(),
        },
    );
    assert!(matches!(
        ordinary.with_workflow_frame_schemas([forbidden]),
        Err(crate::GenerationResolutionError::InvalidFrameSchema { .. })
    ));
}

#[test]
fn frame_aware_entry_requires_a_selected_schema_before_invocation() {
    let resolved = resolve(BASIC, false);
    let kernel = started_kernel(&resolved, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let counter = Key::parse("counter").unwrap();
    let schema = WorkflowFrameSchema {
        revision: 1,
        slots: BTreeMap::from([(counter.clone(), Type::U64)]),
    };
    let mut frame = WorkflowFrame::new(
        schema,
        BTreeMap::from([(counter.clone(), PhenixValue::U64(0))]),
    )
    .unwrap();
    let result = root.execute_workflow_with_frame(
        (&component_id(TOPOLOGY), "turn"),
        (&mut (), &mut frame),
        |_, _, _, _| -> Result<Vec<u8>, String> {
            panic!("an unselected frame schema must reject before preparing a service request")
        },
        |_, _, _, _, _| -> Result<String, String> {
            panic!("a rejected entry must never project provider output")
        },
        || false,
        None,
    );
    assert!(matches!(
        result,
        Err(crate::WorkflowRunError::MissingFrameSchema { .. })
    ));
}

#[test]
fn frame_entry_rejects_mismatched_schema_before_service_invocation() {
    let counter = Key::parse("counter").unwrap();
    let selected = WorkflowFrameSchema {
        revision: 1,
        slots: BTreeMap::from([(counter.clone(), Type::U64)]),
    };
    let resolved = resolve(BASIC, false)
        .with_workflow_frame_schemas([WorkflowFrameDeclaration {
            owner: component_id(TOPOLOGY),
            name: "turn".into(),
            schema: selected.clone(),
        }])
        .unwrap();
    let kernel = started_kernel(&resolved, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let mut outdated = selected;
    outdated.revision = 2;
    let mut frame = WorkflowFrame::new(
        outdated,
        BTreeMap::from([(counter.clone(), PhenixValue::U64(0))]),
    )
    .unwrap();
    let result = root.execute_workflow_with_frame(
        (&component_id(TOPOLOGY), "turn"),
        (&mut (), &mut frame),
        |_, _, _, _| -> Result<Vec<u8>, String> {
            panic!("mismatched schema must fail before request preparation")
        },
        |_, _, _, _, _| -> Result<String, String> {
            panic!("mismatched schema must fail before provider output")
        },
        || false,
        None,
    );
    assert!(matches!(
        result,
        Err(crate::WorkflowRunError::FrameSchemaMismatch { .. })
    ));
    assert_eq!(frame.get(&counter), Some(&PhenixValue::U64(0)));
}

#[test]
fn typed_frame_execution_uses_pinned_imports_and_commits_each_node_output() {
    let counter = Key::parse("counter").unwrap();
    let frame_schema = WorkflowFrameSchema {
        revision: 1,
        slots: BTreeMap::from([(counter.clone(), Type::U64)]),
    };
    let resolved = resolve(BASIC, false)
        .with_workflow_frame_schemas([WorkflowFrameDeclaration {
            owner: component_id(TOPOLOGY),
            name: "turn".into(),
            schema: frame_schema.clone(),
        }])
        .unwrap();
    let kernel = started_kernel(&resolved, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let mut frame = WorkflowFrame::new(
        frame_schema,
        BTreeMap::from([(counter.clone(), PhenixValue::U64(0))]),
    )
    .unwrap();
    let mut seen = Vec::new();
    let report = root
        .execute_workflow_with_frame(
            (&component_id(TOPOLOGY), "turn"),
            (&mut seen, &mut frame),
            |_, _, frame, _| {
                assert!(matches!(
                    frame.get(&counter),
                    Some(PhenixValue::U64(count)) if *count <= 2
                ));
                Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap())
            },
            |node, _, output, frame, seen| {
                seen.push(node.to_owned());
                let count = match frame.get(&counter) {
                    Some(PhenixValue::U64(count)) => *count,
                    _ => return Err("missing frame counter".into()),
                };
                frame
                    .set(&counter, PhenixValue::U64(count + 1))
                    .map_err(|error| error.to_string())?;
                match serde_json::from_slice::<PhenixValue>(output).unwrap() {
                    PhenixValue::String(outcome) => Ok::<_, String>(outcome),
                    _ => Err("invalid outcome".into()),
                }
            },
            || false,
            None,
        )
        .unwrap();
    assert_eq!(report.executed_nodes, 3);
    assert_eq!(frame.get(&counter), Some(&PhenixValue::U64(3)));
    assert_eq!(seen, ["model", "tool", "model"]);
}

#[test]
fn typed_frame_projection_failure_rolls_back_data_without_replaying_side_effects() {
    let counter = Key::parse("counter").unwrap();
    let frame_schema = WorkflowFrameSchema {
        revision: 1,
        slots: BTreeMap::from([(counter.clone(), Type::U64)]),
    };
    let resolved = resolve(BASIC, false)
        .with_workflow_frame_schemas([WorkflowFrameDeclaration {
            owner: component_id(TOPOLOGY),
            name: "turn".into(),
            schema: frame_schema.clone(),
        }])
        .unwrap();
    let kernel = started_kernel(&resolved, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let mut frame = WorkflowFrame::new(
        frame_schema,
        BTreeMap::from([(counter.clone(), PhenixValue::U64(0))]),
    )
    .unwrap();
    let result = root.execute_workflow_with_frame(
        (&component_id(TOPOLOGY), "turn"),
        (&mut (), &mut frame),
        |_, _, _, _| Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap()),
        |_, _, _, frame, _| {
            frame.set(&counter, PhenixValue::U64(17)).unwrap();
            Err::<String, _>("intentional invalid projection".into())
        },
        || false,
        None,
    );
    assert!(matches!(
        result,
        Err(crate::WorkflowRunError::NodeFailed { .. })
    ));
    assert_eq!(frame.get(&counter), Some(&PhenixValue::U64(0)));
}

#[test]
fn cancellation_during_request_preparation_never_dispatches_that_node() {
    use std::cell::Cell;

    for (cancel_at, expected_completed, expected_log) in
        [("model", 0_u64, 0_usize), ("tool", 1_u64, 2_usize)]
    {
        let resolved = resolve(BASIC, true);
        let log = Arc::new(Mutex::new(Vec::new()));
        let kernel = started_kernel(&resolved, &log);
        let root = kernel.root_execution_handle(&Authority::default());
        let cancelled = Cell::new(false);

        let result = root.execute_workflow(
            (&component_id(TOPOLOGY), "turn"),
            &mut (),
            |node, _, _| {
                if node == cancel_at {
                    cancelled.set(true);
                }
                Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap())
            },
            |_, _, response, _| match serde_json::from_slice::<PhenixValue>(response).unwrap() {
                PhenixValue::String(outcome) => Ok::<_, String>(outcome),
                other => Err(format!("unexpected outcome: {other:?}")),
            },
            || cancelled.get(),
            None,
        );
        assert!(matches!(
            result,
            Err(crate::WorkflowRunError::Cancelled {
                next_node,
                executed_nodes,
            }) if next_node == cancel_at && executed_nodes == expected_completed
        ));
        assert_eq!(
            log.lock().unwrap().len(),
            expected_log,
            "a cancelled node cannot enter even the first service layer"
        );
    }
}

#[test]
fn a_foreign_generation_import_cannot_be_replayed() {
    let basic = resolve(BASIC, false);
    let advanced = resolve(ADVANCED, true);
    let kernel = started_kernel(&basic, &Arc::new(Mutex::new(Vec::new())));
    let root = kernel.root_execution_handle(&Authority::default());
    let foreign = advanced
        .generation_topology()
        .workflow(&component_id(TOPOLOGY), "turn")
        .unwrap()
        .bound_import(&InterfaceId::parse(MODEL).unwrap())
        .unwrap();
    let result = root.invoke_import(foreign, &[]);
    assert!(matches!(
        result,
        Err(KernelError::PinnedBindingChanged { .. })
    ));
}

struct CounterProvider {
    calls: Arc<Mutex<u64>>,
}

impl PluginInstance for CounterProvider {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke_component(
        &mut self,
        _component: &ComponentId,
        _service: &ServiceId,
        _input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        let outcome = if *calls < 3 { "again" } else { "recorded" };
        serde_json::to_vec(&PhenixValue::String(outcome.into())).map_err(|error| error.to_string())
    }
}

#[test]
fn non_agent_workflow_cycles_without_an_agent_plugin_or_extra_executor() {
    const OWNER: &str = "fixture.counter-workflow";
    const PROVIDER: &str = "fixture.counter-provider";
    const INCREMENT: &str = "fixture.counter.increment@1";

    let resolved = ResolvedGeneration::resolve(
        [
            manifest(OWNER, PluginExecution::ResourceOnly),
            manifest(PROVIDER, PluginExecution::Embedded),
        ],
        [
            ComponentManifest {
                id: component_id(OWNER),
                owner: plugin_id(OWNER),
                imports: vec![ComponentImport {
                    interface: InterfaceId::parse(INCREMENT).unwrap(),
                    schema: InterfaceSchema::default(),
                    required: true,
                    authority: Authority::default(),
                }],
                exports: Vec::new(),
                listeners: Vec::new(),
                maximum_authority: Authority::default(),
            },
            provider(PROVIDER, INCREMENT, 100),
        ],
        [],
        &Authority::default(),
    )
    .unwrap()
    .with_workflows([
        WorkflowDeclaration {
            owner: component_id(OWNER),
            name: "record".into(),
            topology: WorkflowTopology {
                entry: "record".into(),
                nodes: BTreeMap::from([(
                    "record".into(),
                    WorkflowNode {
                        import: InterfaceId::parse(INCREMENT).unwrap(),
                        branches: BTreeMap::from([
                            (
                                "again".into(),
                                WorkflowEdge::Next {
                                    node: "record".into(),
                                },
                            ),
                            ("recorded".into(), WorkflowEdge::Finish),
                        ]),
                    },
                )]),
            },
        },
        WorkflowDeclaration {
            owner: component_id(OWNER),
            name: "poll".into(),
            topology: WorkflowTopology {
                entry: "poll".into(),
                nodes: BTreeMap::from([(
                    "poll".into(),
                    WorkflowNode {
                        import: InterfaceId::parse(INCREMENT).unwrap(),
                        branches: BTreeMap::from([(
                            "again".into(),
                            WorkflowEdge::Next {
                                node: "poll".into(),
                            },
                        )]),
                    },
                )]),
            },
        },
    ])
    .unwrap();

    let calls = Arc::new(Mutex::new(0_u64));
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    kernel.activate_resolved_generation(&resolved).unwrap();
    let shared = Arc::clone(&calls);
    kernel
        .register_embedded_factory(plugin_id(PROVIDER), move || {
            Box::new(CounterProvider {
                calls: Arc::clone(&shared),
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();

    let root = kernel.root_execution_handle(&Authority::default());
    assert_eq!(root.generation(), Some(resolved.generation()));

    // Exercise a plan with no Exit using the real Core root, selected import,
    // embedded provider and opt-in step budget. No agent plugin is present.
    let cancelled = root.execute_workflow(
        (&component_id(OWNER), "poll"),
        &mut (),
        |_, _, _| Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap()),
        |_, _, output, _| match serde_json::from_slice::<PhenixValue>(output).unwrap() {
            PhenixValue::String(outcome) => Ok::<_, String>(outcome),
            other => Err(format!("unexpected counter outcome: {other:?}")),
        },
        || true,
        None,
    );
    assert!(matches!(
        cancelled,
        Err(crate::WorkflowRunError::Cancelled {
            executed_nodes: 0,
            ..
        })
    ));
    assert_eq!(*calls.lock().unwrap(), 0);

    let bounded = root.execute_workflow(
        (&component_id(OWNER), "poll"),
        &mut (),
        |_, _, _| Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap()),
        |_, _, output, _| match serde_json::from_slice::<PhenixValue>(output).unwrap() {
            PhenixValue::String(outcome) => Ok::<_, String>(outcome),
            other => Err(format!("unexpected counter outcome: {other:?}")),
        },
        || false,
        std::num::NonZeroU64::new(2),
    );
    assert!(matches!(
        bounded,
        Err(crate::WorkflowRunError::StepLimitReached {
            executed_nodes: 2,
            ..
        })
    ));
    assert_eq!(*calls.lock().unwrap(), 2);
    *calls.lock().unwrap() = 0;

    let report = root
        .execute_workflow(
            (&component_id(OWNER), "record"),
            &mut (),
            |_, _, _| Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap()),
            |_, _, output, _| match serde_json::from_slice::<PhenixValue>(output).unwrap() {
                PhenixValue::String(outcome) => Ok::<_, String>(outcome),
                other => Err(format!("unexpected counter outcome: {other:?}")),
            },
            || false,
            None,
        )
        .unwrap();
    assert_eq!(report.executed_nodes, 3);
    assert_eq!(report.final_outcome, "recorded");
    assert_eq!(*calls.lock().unwrap(), 3);
}
