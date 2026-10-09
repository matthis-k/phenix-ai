#![forbid(unsafe_code)]

//! Standard agent-loop topology. There are no node implementations in this crate.

mod runtime_adapter;
pub use runtime_adapter::run_agent_workflow;

use phenix_core::{
    Authority, ComponentId, ComponentImport, ComponentInterface, ComponentManifest,
    PluginExecution, PluginId, PluginManifest, WorkflowDeclaration, WorkflowEdge, WorkflowNode,
    WorkflowTopology, WorkflowOutcomeProjection, WorkflowProjectionDeclaration,
    WorkflowProjectionSelector, WORKFLOW_PROJECTION_REVISION,
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
        ("turn", &["tool_calls", "final", "cancelled", "failed"][..]),
        ("tool_batch", &["continue", "cancelled"][..]),
    ]
    .into_iter()
    .map(|(node, variants)| WorkflowProjectionDeclaration {
        owner: owner.clone(),
        workflow: "agent.turn".into(),
        node: node.into(),
        projection: WorkflowOutcomeProjection {
            revision: WORKFLOW_PROJECTION_REVISION,
            selector: WorkflowProjectionSelector::VariantTag,
            cases: variants.iter().map(|name| ((*name).into(), (*name).into())).collect(),
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
        for outcome in turn_outcomes {
            assert!(
                topology.nodes["turn"]
                    .branches
                    .contains_key(outcome.workflow_outcome()),
            );
        }
        for outcome in batch_outcomes {
            assert!(
                topology.nodes["tool_batch"]
                    .branches
                    .contains_key(outcome.workflow_outcome()),
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
