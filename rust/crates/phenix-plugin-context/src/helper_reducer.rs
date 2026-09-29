use phenix_core::{
    Authority, Bytes, CallableId, ComponentExport, ComponentId, ComponentImport,
    ComponentInterface, ComponentManifest, PhenixValue, PluginContext, PluginExecution, PluginHost,
    PluginId, PluginInstance, PluginManifest, RoutingProfileId, SdkClient, ServiceContribution,
    ServiceId, ServiceRole,
};
use phenix_sdk::{
    context_reducer_service, ContextReducerCommand, ContextReducerInterface,
    ContextReducerProposal, ContextReducerRequest, ContextReducerResponse, DerivedReductionSummary,
    HelperInvocationCommand, HelperInvocationInterface, HelperInvocationKind,
    HelperInvocationRequest, HelperInvocationResponse,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

pub const HELPER_REDUCER_PLUGIN: &str = "phenix.context-reducer.helper";
pub const HELPER_REDUCER_COMPONENT: &str = "phenix.context-reducer.helper";
const STEP_RUNNER_PLUGIN: &str = "phenix.step-runner";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HelperReducerConfig {
    pub profile_id: RoutingProfileId,
    pub callable_id: CallableId,
}

#[must_use]
pub fn helper_reducer_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(HELPER_REDUCER_PLUGIN)
            .expect("static helper reducer plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: vec![
            PluginId::parse(STEP_RUNNER_PLUGIN).expect("static step runner plugin id is valid")
        ],
        services: vec![ServiceContribution {
            role: ServiceRole::Terminal,
            service: context_reducer_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

#[must_use]
pub fn helper_reducer_component_id() -> ComponentId {
    ComponentId::parse(HELPER_REDUCER_COMPONENT)
        .expect("static helper reducer component id is valid")
}

#[must_use]
pub fn helper_reducer_component_manifest() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: helper_reducer_component_id(),
        owner: PluginId::parse(HELPER_REDUCER_PLUGIN)
            .expect("static helper reducer plugin id is valid"),
        imports: vec![ComponentImport {
            interface: HelperInvocationInterface::interface_id(),
            schema: HelperInvocationInterface::schema(),
            required: true,
            authority: Authority::default(),
        }],
        exports: vec![ComponentExport {
            interface: ContextReducerInterface::interface_id(),
            schema: ContextReducerInterface::schema(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        maximum_authority: Authority::default(),
    }
}

#[must_use]
pub fn helper_reducer_factory(config: HelperReducerConfig) -> Box<dyn PluginInstance> {
    Box::new(HelperReducer { config })
}

struct HelperReducer {
    config: HelperReducerConfig,
}

struct HelperReducerSdk<'host, 'runtime> {
    helper: SdkClient<'host, 'runtime, HelperInvocationInterface>,
}

type HelperReducerContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, HelperReducerSdk<'host, 'runtime>>;

fn context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> HelperReducerContext<'host, 'runtime> {
    PluginContext::new(
        host,
        HelperReducerSdk {
            helper: SdkClient::new(host, helper_reducer_component_id()),
        },
        (),
        (),
    )
}

impl PluginInstance for HelperReducer {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &context_reducer_service() {
            return Err(format!("unsupported helper reducer service: {service}"));
        }
        let context = context(host);
        let command = context
            .kernel
            .decode_projected::<ContextReducerCommand>(
                &ContextReducerInterface::interface_id(),
                input,
            )
            .map_err(|error| error.to_string())?;
        let ContextReducerCommand::Reduce { request } = command;
        let response = reduce(&context, &self.config, request)?;
        context
            .kernel
            .encode_value(&ContextReducerResponse::Proposal { proposal: response })
            .map_err(|error| error.to_string())
    }
}

fn reduce(
    context: &HelperReducerContext<'_, '_>,
    config: &HelperReducerConfig,
    request: ContextReducerRequest,
) -> Result<ContextReducerProposal, String> {
    let input = helper_input(&request)?;
    let response: HelperInvocationResponse = context
        .sdk
        .helper
        .invoke_projected(&HelperInvocationCommand::Invoke {
            request: HelperInvocationRequest {
                execution_id: request.execution_id.clone(),
                parent_attempt_id: request.parent_attempt_id.clone(),
                profile_id: config.profile_id.clone(),
                kind: HelperInvocationKind::Helper,
                callable_id: config.callable_id.clone(),
                input,
                tools: Vec::new(),
            },
        })
        .map_err(|error| format!("context reducer helper invocation failed: {error}"))?;
    if !response.tool_calls.is_empty() {
        return Err("context reducer helper must not request tools".into());
    }
    let output = std::str::from_utf8(response.output.as_ref())
        .map_err(|_| "context reducer helper output is not UTF-8".to_owned())?;
    let decisions: HelperReducerOutput = serde_json::from_str(output.trim()).map_err(|error| {
        format!("context reducer helper output is not valid decision JSON: {error}")
    })?;
    build_proposal(&request, &response.attempt_id, decisions)
}

fn helper_input(request: &ContextReducerRequest) -> Result<Bytes, String> {
    let items = request
        .eligible
        .iter()
        .map(|item| {
            serde_json::json!({
                "item_id": item.item_id,
                "content": String::from_utf8_lossy(item.content.as_ref()),
                "recoverable": item.recovery.is_some(),
            })
        })
        .collect::<Vec<_>>();
    let payload = serde_json::json!({
        "instruction": "Return JSON with one decision per item. Each decision is retain, omit, or summarize. A summarize decision must include a concise summary. Do not invent item IDs.",
        "query": request.query,
        "stage": request.stage,
        "items": items,
    });
    serde_json::to_vec(&payload)
        .map(Bytes::from)
        .map_err(|error| format!("context reducer helper input encoding failed: {error}"))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HelperReducerOutput {
    decisions: Vec<HelperReducerDecision>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum HelperReducerDecision {
    Retain { item_id: String },
    Omit { item_id: String },
    Summarize { item_id: String, summary: String },
}

fn build_proposal(
    request: &ContextReducerRequest,
    helper_attempt_id: &str,
    output: HelperReducerOutput,
) -> Result<ContextReducerProposal, String> {
    let mut eligible = BTreeMap::new();
    for item in &request.eligible {
        if eligible.insert(item.item_id.as_str(), item).is_some() {
            return Err(format!(
                "context reducer request contains duplicate item {}",
                item.item_id
            ));
        }
    }

    let mut seen = BTreeSet::new();
    let mut retained_item_ids = Vec::new();
    let mut omitted_item_ids = Vec::new();
    let mut summaries = Vec::new();
    for decision in output.decisions {
        let (item_id, action) = match decision {
            HelperReducerDecision::Retain { item_id } => (item_id, DecisionAction::Retain),
            HelperReducerDecision::Omit { item_id } => (item_id, DecisionAction::Omit),
            HelperReducerDecision::Summarize { item_id, summary } => {
                (item_id, DecisionAction::Summarize(summary))
            }
        };
        if !seen.insert(item_id.clone()) {
            return Err(format!("context reducer helper repeated item {item_id}"));
        }
        let item = eligible
            .get(item_id.as_str())
            .ok_or_else(|| format!("context reducer helper returned unknown item {item_id}"))?;
        match action {
            DecisionAction::Retain => retained_item_ids.push(item_id),
            DecisionAction::Omit => {
                if item.recovery.is_none() {
                    return Err(format!(
                        "context reducer helper cannot omit unrecoverable item {item_id}"
                    ));
                }
                omitted_item_ids.push(item_id);
            }
            DecisionAction::Summarize(summary) => {
                if summary.trim().is_empty() {
                    return Err(format!(
                        "context reducer helper returned empty summary for {item_id}"
                    ));
                }
                let recovery = item.recovery.clone().ok_or_else(|| {
                    format!("context reducer helper cannot summarize unrecoverable item {item_id}")
                })?;
                retained_item_ids.push(item_id.clone());
                summaries.push(DerivedReductionSummary {
                    item_id,
                    content: Bytes::from(summary.into_bytes()),
                    exact_sources: vec![recovery],
                });
            }
        }
    }
    for item_id in eligible.keys() {
        if !seen.contains(*item_id) {
            return Err(format!(
                "context reducer helper omitted decision for item {item_id}"
            ));
        }
    }

    let mut proposal = ContextReducerProposal {
        execution_id: request.execution_id.clone(),
        expected_projection: request.expected_projection.clone(),
        configuration_revision: request.configuration_revision.clone(),
        authority_revision: request.authority_revision.clone(),
        capability_generation: request.capability_generation.clone(),
        stage: request.stage,
        helper_attempt_id: helper_attempt_id.to_owned(),
        reduction_id: ContextReducerProposal::reduction_identity(helper_attempt_id),
        retained_item_ids,
        omitted_item_ids,
        summaries,
        encoded_output_bytes: 0,
    };
    let encoded = serde_json::to_vec(&PhenixValue::from(&proposal))
        .map_err(|error| format!("context reducer proposal encoding failed: {error}"))?;
    proposal.encoded_output_bytes = u64::try_from(encoded.len()).unwrap_or(u64::MAX);
    Ok(proposal)
}

enum DecisionAction {
    Retain,
    Omit,
    Summarize(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{ContextResourceId, ContextRevisionId};
    use phenix_sdk::{
        ContextReducerStage, ExactContextReference, ProjectionRevision, ReducerEligibleItem,
    };

    fn request() -> ContextReducerRequest {
        ContextReducerRequest {
            execution_id: "execution-1".into(),
            expected_projection: ProjectionRevision {
                revision: 4,
                cache_epoch: 2,
            },
            configuration_revision: "config-1".into(),
            authority_revision: "authority-1".into(),
            capability_generation: "capability-1".into(),
            stage: ContextReducerStage::HistorySummary,
            query: "fix the failing test".into(),
            parent_attempt_id: "attempt-parent".into(),
            max_output_bytes: 4096,
            eligible: vec![
                ReducerEligibleItem {
                    item_id: "item-1".into(),
                    content: Bytes::from(b"verbose history".to_vec()),
                    recovery: Some(ExactContextReference {
                        resource_id: ContextResourceId::parse("history-1").unwrap(),
                        revision: ContextRevisionId::parse("revision-1").unwrap(),
                    }),
                },
                ReducerEligibleItem {
                    item_id: "item-2".into(),
                    content: Bytes::from(b"keep me".to_vec()),
                    recovery: None,
                },
            ],
        }
    }

    #[test]
    fn helper_reducer_has_no_authority_and_only_imports_helper_invocation() {
        let manifest = helper_reducer_manifest();
        assert_eq!(manifest.maximum_authority, Authority::default());
        assert_eq!(
            manifest.dependencies,
            vec![PluginId::parse(STEP_RUNNER_PLUGIN).unwrap()]
        );
        let component = helper_reducer_component_manifest();
        assert_eq!(component.maximum_authority, Authority::default());
        assert_eq!(component.imports.len(), 1);
        assert_eq!(
            component.imports[0].interface,
            HelperInvocationInterface::interface_id()
        );
        assert_eq!(component.exports.len(), 1);
        assert_eq!(
            component.exports[0].interface,
            ContextReducerInterface::interface_id()
        );
    }

    #[test]
    fn helper_output_cannot_forge_recovery_references() {
        let request = request();
        let proposal = build_proposal(
            &request,
            "attempt-helper",
            HelperReducerOutput {
                decisions: vec![
                    HelperReducerDecision::Summarize {
                        item_id: "item-1".into(),
                        summary: "short history".into(),
                    },
                    HelperReducerDecision::Retain {
                        item_id: "item-2".into(),
                    },
                ],
            },
        )
        .unwrap();

        assert_eq!(proposal.helper_attempt_id, "attempt-helper");
        assert_eq!(
            proposal.reduction_id,
            ContextReducerProposal::reduction_identity("attempt-helper")
        );
        assert_eq!(proposal.summaries.len(), 1);
        assert_eq!(
            proposal.summaries[0].exact_sources,
            vec![request.eligible[0].recovery.clone().unwrap()]
        );
        proposal
            .validate_against(&request, &request.expected_projection)
            .unwrap();
    }

    #[test]
    fn helper_output_fails_closed_for_unrecoverable_omission() {
        let request = request();
        let error = build_proposal(
            &request,
            "attempt-helper",
            HelperReducerOutput {
                decisions: vec![
                    HelperReducerDecision::Retain {
                        item_id: "item-1".into(),
                    },
                    HelperReducerDecision::Omit {
                        item_id: "item-2".into(),
                    },
                ],
            },
        )
        .unwrap_err();
        assert!(error.contains("cannot omit unrecoverable item item-2"));
    }
}
