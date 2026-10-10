#![forbid(unsafe_code)]

//! Standard agent-loop topology. There are no node implementations in this crate.

mod runtime_adapter;
pub use runtime_adapter::{run_agent_workflow, run_agent_workflow_pending};

use phenix_core::{
    Authority, ComponentId, ComponentImport, ComponentInterface, ComponentManifest,
    PluginExecution, PluginId, PluginManifest, WORKFLOW_PROJECTION_REVISION, WorkflowDeclaration,
    WorkflowEdge, WorkflowNode, WorkflowOutcomeProjection, WorkflowProjectionDeclaration,
    WorkflowProjectionSelector, WorkflowTopology,
};
use phenix_sdk::{AgentToolBatchInterface, AgentTurnStepInterface};
use std::collections::BTreeMap;

pub const AGENT_TOPOLOGY_PLUGIN: &str = "phenix.agent-topology";

pub fn agent_topology_manifest(maximum_authority: Authority) -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(AGENT_TOPOLOGY_PLUGIN).expect("static plugin"),
        version: 1,
        execution: PluginExecution::ResourceOnly,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority,
    }
}

pub fn agent_topology_component_manifest(authority: Authority) -> ComponentManifest {
    ComponentManifest {
        id: ComponentId::parse(AGENT_TOPOLOGY_PLUGIN).expect("static component"),
        owner: PluginId::parse(AGENT_TOPOLOGY_PLUGIN).expect("static plugin"),
        imports: vec![
            ComponentImport {
                interface: AgentTurnStepInterface::interface_id(),
                schema: AgentTurnStepInterface::schema(),
                required: true,
                authority: authority.clone(),
            },
            ComponentImport {
                interface: AgentToolBatchInterface::interface_id(),
                schema: AgentToolBatchInterface::schema(),
                required: true,
                authority: authority.clone(),
            },
        ],
        exports: Vec::new(),
        listeners: Vec::new(),
        maximum_authority: authority,
    }
}

/// The model turn either finishes the run or produces a batch of tool calls.
/// The batch provider executes all calls and commits a continuation before the
/// next model turn. Policies and implementations belong to providers.
pub fn agent_topology_declaration() -> WorkflowDeclaration {
    WorkflowDeclaration {
        owner: ComponentId::parse(AGENT_TOPOLOGY_PLUGIN).expect("static component"),
        name: "agent.turn".into(),
        topology: WorkflowTopology {
            entry: "turn".into(),
            nodes: BTreeMap::from([
                (
                    "turn".into(),
                    WorkflowNode {
                        import: AgentTurnStepInterface::interface_id(),
                        branches: BTreeMap::from([
                            (
                                "tool_calls".into(),
                                WorkflowEdge::Next {
                                    node: "tool_batch".into(),
                                },
                            ),
                            ("final".into(), WorkflowEdge::Finish),
                            ("cancelled".into(), WorkflowEdge::Finish),
                            ("failed".into(), WorkflowEdge::Finish),
                        ]),
                    },
                ),
                (
                    "tool_batch".into(),
                    WorkflowNode {
                        import: AgentToolBatchInterface::interface_id(),
                        branches: BTreeMap::from([
                            (
                                "continue".into(),
                                WorkflowEdge::Next {
                                    node: "turn".into(),
                                },
                            ),
                            ("cancelled".into(), WorkflowEdge::Finish),
                        ]),
                    },
                ),
            ]),
        },
    }
}

/// Portable normal-result projection for the selected standard agent nodes.
/// State transitions and usage validation remain with the agent adapter.
pub fn agent_topology_projections() -> Vec<WorkflowProjectionDeclaration> {
    let owner = ComponentId::parse(AGENT_TOPOLOGY_PLUGIN).expect("static component");
    [
        (
            "turn",
            &[
                ("ToolCalls", "tool_calls"),
                ("Final", "final"),
                ("Cancelled", "cancelled"),
                ("Failed", "failed"),
            ][..],
        ),
        (
            "tool_batch",
            &[("Continue", "continue"), ("Cancelled", "cancelled")][..],
        ),
    ]
    .into_iter()
    .map(|(node, variants)| WorkflowProjectionDeclaration {
        owner: owner.clone(),
        workflow: "agent.turn".into(),
        node: node.into(),
        projection: WorkflowOutcomeProjection {
            revision: WORKFLOW_PROJECTION_REVISION,
            selector: WorkflowProjectionSelector::VariantTag,
            cases: variants
                .iter()
                .map(|(tag, outcome)| ((*tag).into(), (*outcome).into()))
                .collect(),
        },
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{ComponentExport, GenerationResolutionError, ResolvedGeneration};

    fn provider(
        name: &str,
        interface: phenix_core::InterfaceId,
        schema: phenix_core::InterfaceSchema,
    ) -> ComponentManifest {
        ComponentManifest {
            id: ComponentId::parse(name).unwrap(),
            owner: PluginId::parse("fixture.providers").unwrap(),
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface,
                schema,
                priority: 100,
                required_authority: Authority::default(),
            }],
            listeners: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    #[test]
    fn pending_agent_tool_batch_executes_exactly_one_effect_and_preserves_history() {
        use phenix_core::{
            Bytes, CallableId, Kernel, ModelToolCall, ModelToolDescriptor, ModelToolResult,
            ModelToolTurn, PhenixSchema, PhenixValue, PluginHost, PluginInstance, Project,
            ResolvedGenerationActivation, ServiceId,
        };
        use phenix_sdk::{
            AgentLoopCommand, AgentLoopResponse, AgentToolBatchRequest, AgentToolBatchResponse,
            AgentTurnStepRequest, AgentTurnStepResponse,
        };
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        struct ToolFlow {
            effects: Arc<AtomicUsize>,
        }

        impl PluginInstance for ToolFlow {
            fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
                Ok(())
            }

            fn invoke_component(
                &mut self,
                component: &ComponentId,
                _service: &ServiceId,
                input: &[u8],
                _host: &PluginHost<'_>,
            ) -> Result<Vec<u8>, String> {
                let value: PhenixValue =
                    serde_json::from_slice(input).map_err(|error| error.to_string())?;
                let response = match component.as_str() {
                    "fixture.turn-provider" => {
                        let request = AgentTurnStepRequest::try_from(Project(&value))
                            .map_err(|error| format!("{error:?}"))?;
                        let mut state = request.state;
                        state.usage.model_calls += 1;
                        let turn = if state.continuation.is_empty() {
                            let call = ModelToolCall {
                                call_id: "effect-1".into(),
                                callable_id: CallableId::parse("fixture.echo").unwrap(),
                                input: PhenixValue::String("request".into()),
                            };
                            state.seen_tool_call_ids.push(call.call_id.clone());
                            AgentTurnStepResponse::ToolCalls {
                                state,
                                assistant_output: Bytes::from(b"use tool".to_vec()),
                                tool_calls: vec![call],
                            }
                        } else {
                            assert_eq!(state.continuation.len(), 1);
                            assert_eq!(state.continuation[0].tool_results.len(), 1);
                            AgentTurnStepResponse::Final {
                                state,
                                output: Bytes::from(b"final after tool".to_vec()),
                            }
                        };
                        PhenixValue::from(&turn)
                    }
                    "fixture.batch-provider" => {
                        let request = AgentToolBatchRequest::try_from(Project(&value))
                            .map_err(|error| format!("{error:?}"))?;
                        let mut state = request.state;
                        assert_eq!(request.tool_calls.len(), 1);
                        self.effects.fetch_add(1, Ordering::AcqRel);
                        state.usage.tool_calls += 1;
                        let call = &request.tool_calls[0];
                        state.continuation.push(ModelToolTurn {
                            assistant_output: request.assistant_output,
                            tool_calls: request.tool_calls.clone(),
                            tool_results: vec![ModelToolResult {
                                call_id: call.call_id.clone(),
                                callable_id: call.callable_id.clone(),
                                output: PhenixValue::String("effect complete".into()),
                                is_error: false,
                            }],
                        });
                        PhenixValue::from(&AgentToolBatchResponse::Continue { state })
                    }
                    other => return Err(format!("wrong provider: {other}")),
                };
                serde_json::to_vec(&response).map_err(|error| error.to_string())
            }
        }

        let compiled = ResolvedGeneration::resolve(
            [
                agent_topology_manifest(Authority::default()),
                PluginManifest {
                    id: PluginId::parse("fixture.providers").unwrap(),
                    version: 1,
                    execution: PluginExecution::Embedded,
                    dependencies: Vec::new(),
                    services: Vec::new(),
                    resource_namespaces: Vec::new(),
                    maximum_authority: Authority::default(),
                },
            ],
            [
                agent_topology_component_manifest(Authority::default()),
                provider(
                    "fixture.turn-provider",
                    AgentTurnStepInterface::interface_id(),
                    AgentTurnStepInterface::schema(),
                ),
                provider(
                    "fixture.batch-provider",
                    AgentToolBatchInterface::interface_id(),
                    AgentToolBatchInterface::schema(),
                ),
            ],
            [],
            &Authority::default(),
        )
        .unwrap()
        .with_workflows([agent_topology_declaration()])
        .unwrap()
        .with_workflow_projections(agent_topology_projections())
        .unwrap();

        let mut outcomes = Vec::new();
        for native_pending in [false, true] {
            let effects = Arc::new(AtomicUsize::new(0));
            let mut kernel = Kernel::new(compiled.kernel_config().clone());
            kernel.activate_resolved_generation(&compiled).unwrap();
            let effect_counter = Arc::clone(&effects);
            kernel.register_embedded_factory(
                PluginId::parse("fixture.providers").unwrap(),
                move || Box::new(ToolFlow { effects: Arc::clone(&effect_counter) }),
            ).unwrap();
            kernel.activate_all().unwrap();
            let root = kernel.root_execution_handle(&Authority::default());
            let command = AgentLoopCommand::Run {
                execution_id: "tool-flow".into(),
                session_id: None,
                parent_attempt_id: None,
                callable_id: None,
                input: Bytes::from(b"run effect".to_vec()),
                tools: vec![ModelToolDescriptor {
                    id: CallableId::parse("fixture.echo").unwrap(),
                    description: "Echo tool".into(),
                    input_schema: PhenixSchema::Any,
                    output_schema: PhenixSchema::Any,
                }],
            };
            let outcome = if native_pending {
                run_agent_workflow_pending(&root, command, || false, None)
            } else {
                run_agent_workflow(&root, command, || false, None)
            }.unwrap();
            assert!(matches!(
                &outcome,
                AgentLoopResponse::Completed { output, usage }
                    if output.as_ref() == b"final after tool"
                        && usage.model_calls == 2
                        && usage.tool_calls == 1
            ));
            assert_eq!(effects.load(Ordering::Acquire), 1, "no tool replay allowed");
            outcomes.push(outcome);
        }
        assert_eq!(outcomes[0], outcomes[1]);
    }

    #[test]
    fn pending_and_cooperative_agent_runs_preserve_typed_output_and_usage() {
        use phenix_core::{
            Bytes, Kernel, PhenixValue, PluginHost, PluginInstance, Project,
            ResolvedGenerationActivation, ServiceId,
        };
        use phenix_sdk::{
            AgentLoopCommand, AgentLoopResponse, AgentTurnStepRequest, AgentTurnStepResponse,
        };

        struct TerminalTurn;

        impl PluginInstance for TerminalTurn {
            fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
                Ok(())
            }

            fn invoke_component(
                &mut self,
                component: &ComponentId,
                _service: &ServiceId,
                input: &[u8],
                _host: &PluginHost<'_>,
            ) -> Result<Vec<u8>, String> {
                if component.as_str() != "fixture.turn-provider" {
                    return Err(format!("unexpected tool batch invocation: {component}"));
                }
                let value: PhenixValue =
                    serde_json::from_slice(input).map_err(|error| error.to_string())?;
                let request = AgentTurnStepRequest::try_from(Project(&value))
                    .map_err(|error| format!("{error:?}"))?;
                let mut state = request.state;
                state.usage.model_calls += 1;
                let response = AgentTurnStepResponse::Final {
                    state,
                    output: Bytes::from(b"completed".to_vec()),
                };
                serde_json::to_vec(&PhenixValue::from(&response)).map_err(|error| error.to_string())
            }
        }

        let provider_manifest = PluginManifest {
            id: PluginId::parse("fixture.providers").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let compiled = ResolvedGeneration::resolve(
            [
                agent_topology_manifest(Authority::default()),
                provider_manifest.clone(),
            ],
            [
                agent_topology_component_manifest(Authority::default()),
                provider(
                    "fixture.turn-provider",
                    AgentTurnStepInterface::interface_id(),
                    AgentTurnStepInterface::schema(),
                ),
                provider(
                    "fixture.batch-provider",
                    AgentToolBatchInterface::interface_id(),
                    AgentToolBatchInterface::schema(),
                ),
            ],
            [],
            &Authority::default(),
        )
        .unwrap()
        .with_workflows([agent_topology_declaration()])
        .unwrap()
        .with_workflow_projections(agent_topology_projections())
        .unwrap();

        let mut results = Vec::new();
        for native_pending in [false, true] {
            let mut kernel = Kernel::new(compiled.kernel_config().clone());
            kernel.activate_resolved_generation(&compiled).unwrap();
            kernel
                .register_embedded_factory(provider_manifest.id.clone(), || Box::new(TerminalTurn))
                .unwrap();
            kernel.activate_all().unwrap();
            let root = kernel.root_execution_handle(&Authority::default());
            let command = AgentLoopCommand::Run {
                execution_id: "parity".into(),
                session_id: None,
                parent_attempt_id: None,
                callable_id: None,
                input: Bytes::from(b"prompt".to_vec()),
                tools: Vec::new(),
            };
            let result = if native_pending {
                run_agent_workflow_pending(&root, command, || false, None)
            } else {
                run_agent_workflow(&root, command, || false, None)
            }
            .unwrap();
            assert!(matches!(
                &result,
                AgentLoopResponse::Completed { output, usage }
                    if output.as_ref() == b"completed"
                        && usage.model_calls == 1
                        && usage.tool_calls == 0
            ));
            results.push(result);
        }
        assert_eq!(results[0], results[1]);
    }

    #[test]
    fn standard_branches_cover_every_typed_node_outcome() {
        use phenix_core::Bytes;
        use phenix_sdk::{
            AgentLoopFailure, AgentLoopUsage, AgentToolBatchResponse, AgentTurnState,
            AgentTurnStepResponse,
        };
        let state = AgentTurnState {
            execution_id: "run".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(Vec::<u8>::new()),
            tools: Vec::new(),
            continuation: Vec::new(),
            seen_tool_call_ids: Vec::new(),
            observations: BTreeMap::new(),
            usage: AgentLoopUsage {
                model_calls: 0,
                tool_calls: 0,
            },
        };
        let turn_outcomes = [
            AgentTurnStepResponse::ToolCalls {
                state: state.clone(),
                assistant_output: Bytes::from(Vec::<u8>::new()),
                tool_calls: Vec::new(),
            },
            AgentTurnStepResponse::Final {
                state: state.clone(),
                output: Bytes::from(Vec::<u8>::new()),
            },
            AgentTurnStepResponse::Cancelled {
                state: state.clone(),
            },
            AgentTurnStepResponse::Failed {
                state: state.clone(),
                failure: AgentLoopFailure::ModelTurnLimitExceeded { limit: 1 },
            },
        ];
        let batch_outcomes = [
            AgentToolBatchResponse::Continue {
                state: state.clone(),
            },
            AgentToolBatchResponse::Cancelled { state },
        ];
        let topology = agent_topology_declaration().topology;
        let projections = agent_topology_projections();
        for outcome in turn_outcomes {
            assert!(
                topology.nodes["turn"]
                    .branches
                    .contains_key(outcome.workflow_outcome()),
            );
            assert_eq!(
                projections
                    .iter()
                    .find(|p| p.node == "turn")
                    .unwrap()
                    .projection
                    .project_checked(
                        AgentTurnStepInterface::schema().response(),
                        &phenix_core::PhenixValue::from(&outcome),
                    ),
                Ok(outcome.workflow_outcome().to_owned()),
            );
        }
        for outcome in batch_outcomes {
            assert!(
                topology.nodes["tool_batch"]
                    .branches
                    .contains_key(outcome.workflow_outcome()),
            );
            assert_eq!(
                projections
                    .iter()
                    .find(|p| p.node == "tool_batch")
                    .unwrap()
                    .projection
                    .project_checked(
                        AgentToolBatchInterface::schema().response(),
                        &phenix_core::PhenixValue::from(&outcome),
                    ),
                Ok(outcome.workflow_outcome().to_owned()),
            );
        }
    }

    #[test]
    fn startup_checks_both_imports_and_keeps_topology_owner_nonexecutable() {
        let authority = Authority::default();
        let providers = PluginManifest {
            id: PluginId::parse("fixture.providers").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: authority.clone(),
        };
        let components = vec![
            agent_topology_component_manifest(authority.clone()),
            provider(
                "fixture.turn",
                AgentTurnStepInterface::interface_id(),
                AgentTurnStepInterface::schema(),
            ),
            provider(
                "fixture.batch",
                AgentToolBatchInterface::interface_id(),
                AgentToolBatchInterface::schema(),
            ),
        ];
        let resolved = ResolvedGeneration::resolve(
            [
                agent_topology_manifest(authority.clone()),
                providers.clone(),
            ],
            components.clone(),
            [],
            &authority,
        )
        .unwrap()
        .with_workflows([agent_topology_declaration()])
        .unwrap();
        let resolved = resolved
            .with_workflow_projections(agent_topology_projections())
            .unwrap();
        assert_eq!(resolved.workflow_projections().len(), 2);
        let workflow = resolved
            .generation_topology()
            .workflow(
                &ComponentId::parse(AGENT_TOPOLOGY_PLUGIN).unwrap(),
                "agent.turn",
            )
            .unwrap();
        assert!(workflow.outcome_projection("turn").is_some());
        assert!(workflow.outcome_projection("tool_batch").is_some());
        let mut duplicated = agent_topology_projections();
        duplicated.push(duplicated[0].clone());
        assert!(matches!(
            resolved.clone().with_workflow_projections(duplicated),
            Err(GenerationResolutionError::DuplicateProjection { .. })
        ));
        let mut incompatible = agent_topology_projections();
        incompatible[0]
            .projection
            .cases
            .insert("final".into(), "undeclared".into());
        let base = ResolvedGeneration::resolve(
            [
                agent_topology_manifest(authority.clone()),
                providers.clone(),
            ],
            components.clone(),
            [],
            &authority,
        )
        .unwrap()
        .with_workflows([agent_topology_declaration()])
        .unwrap();
        assert_ne!(base.generation(), resolved.generation());
        assert!(matches!(
            base.with_workflow_projections(incompatible),
            Err(GenerationResolutionError::InvalidProjection { .. })
        ));
        assert!(
            resolved
                .generation_topology()
                .workflow(
                    &ComponentId::parse(AGENT_TOPOLOGY_PLUGIN).unwrap(),
                    "agent.turn"
                )
                .is_some()
        );

        let mut missing = components;
        missing.pop();
        assert!(matches!(
            ResolvedGeneration::resolve(
                [agent_topology_manifest(authority.clone()), providers],
                missing,
                [],
                &authority
            ),
            Err(GenerationResolutionError::ComponentGraph(_))
        ));
        let plugin = agent_topology_manifest(authority);
        assert!(plugin.services.is_empty());
        assert!(plugin.dependencies.is_empty());
        assert!(matches!(plugin.execution, PluginExecution::ResourceOnly));
    }
}
