//! Cross-profile proof that the same workflow uses the resolved provider graph.
use crate::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentManifest, InterfaceId,
    InterfaceSchema, Kernel, KernelError, LayerPolicy, LayerResult, PhenixValue, PluginExecution,
    PluginHost, PluginId, PluginInstance, PluginManifest, ProviderCompositionPolicy,
    ResolvedGeneration, ResolvedGenerationActivation, ServiceContribution, ServiceId, ServiceRole,
    WorkflowDeclaration, WorkflowEdge, WorkflowFrame, WorkflowFrameDeclaration, WorkflowFrameSchema,
    WorkflowNode, WorkflowTopology, Key, Type,
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

fn resolve(selected: &str, logging: bool) -> ResolvedGeneration {
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
    .with_workflows([topology()])
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
    let report = root.execute_workflow_with_frame(
        (&component_id(TOPOLOGY), "turn"),
        &mut seen,
        &mut frame,
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
    ).unwrap();
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
        &mut (),
        &mut frame,
        |_, _, _, _| Ok::<_, String>(serde_json::to_vec(&PhenixValue::Unit).unwrap()),
        |_, _, _, frame, _| {
            frame.set(&counter, PhenixValue::U64(17)).unwrap();
            Err::<String, _>("intentional invalid projection".into())
        },
        || false,
        None,
    );
    assert!(matches!(result, Err(crate::WorkflowRunError::NodeFailed { .. })));
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
