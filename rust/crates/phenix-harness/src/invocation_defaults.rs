use phenix_core::{
    Authority, CallableId, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, InterfaceSchema, ModelToolDescriptor, PluginContext, PluginExecution,
    PluginHost, PluginId, PluginInstance, PluginManifest, RoutingProfileId, SdkClient,
    ServiceContribution, ServiceId, ServiceRole,
};
use phenix_plugin_catalog::{
    OptionCommand, OptionContext, OptionKey, OptionResponse, OptionSubjectId, OptionValue,
    OptionsInterface,
};
use phenix_sdk::{
    context_recovery_service, invocation_clock_service, invocation_defaults_service,
    recovery_cold_gate, validate_recovery_decision, ContextNeed, ContextRecoveryCommand,
    ContextRecoveryDecision, ContextRecoveryInterface, ContextRecoveryRequest,
    ContextRecoveryResponse, DelegationResourcePolicy, HelperInvocationRequest,
    InvocationClockCommand, InvocationClockInterface, InvocationClockResponse,
    InvocationDefaultsCommand, InvocationDefaultsInterface, InvocationDefaultsResponse,
    InvocationIntent, InvocationParams, InvocationRequest, RecoveryClassifierPolicy,
    RecoveryColdGate, RouteSelectionPolicy, RoutingEstimateMode, UsagePolicy,
};
use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

pub const INVOCATION_DEFAULTS_PLUGIN: &str = "phenix.harness.invocation-defaults";
pub const INVOCATION_DEFAULTS_COMPONENT: &str = "phenix.harness.invocation-defaults";
const ROUTING_PROFILE_OPTION: &str = "model.default";
const DEFAULT_POLICY_REVISION: &str = "harness.usage.default.v1";
const HELPER_POLICY_REVISION: &str = "harness.usage.helper.v1";
const DEFAULT_ROUTE_POLICY_REVISION: &str = "harness.routing.default.v1";
const HELPER_ROUTE_POLICY_REVISION: &str = "harness.routing.helper.v1";

#[must_use]
pub fn invocation_defaults_manifest(maximum_authority: Authority) -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(INVOCATION_DEFAULTS_PLUGIN).expect("static plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: vec![PluginId::parse("phenix.options").expect("static plugin id is valid")],
        services: vec![
            ServiceContribution {
                role: ServiceRole::Terminal,
                service: invocation_defaults_service(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ServiceContribution {
                role: ServiceRole::Terminal,
                service: invocation_clock_service(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ServiceContribution {
                role: ServiceRole::Terminal,
                service: context_recovery_service(),
                priority: 100,
                required_authority: Authority::default(),
            },
        ],
        resource_namespaces: Vec::new(),
        maximum_authority,
    }
}

#[must_use]
pub fn invocation_defaults_component_id() -> ComponentId {
    ComponentId::parse(INVOCATION_DEFAULTS_COMPONENT).expect("static component id is valid")
}

#[must_use]
pub fn invocation_defaults_component_manifest(maximum_authority: Authority) -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: invocation_defaults_component_id(),
        owner: PluginId::parse(INVOCATION_DEFAULTS_PLUGIN).expect("static plugin id is valid"),
        imports: vec![ComponentImport {
            interface: OptionsInterface::interface_id(),
            schema: InterfaceSchema::fallible_of::<OptionCommand, OptionResponse, String>(),
            required: true,
            authority: maximum_authority.clone(),
        }],
        exports: vec![
            ComponentExport {
                interface: InvocationDefaultsInterface::interface_id(),
                schema: InvocationDefaultsInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: InvocationClockInterface::interface_id(),
                schema: InvocationClockInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: ContextRecoveryInterface::interface_id(),
                schema: ContextRecoveryInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
        ],
        maximum_authority,
    }
}

#[must_use]
pub fn invocation_defaults_factory() -> Box<dyn PluginInstance> {
    Box::new(InvocationDefaultsPlugin)
}

struct InvocationDefaultsSdk<'host, 'runtime> {
    options: SdkClient<'host, 'runtime, OptionsInterface>,
}

type InvocationDefaultsContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, InvocationDefaultsSdk<'host, 'runtime>>;

fn context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> InvocationDefaultsContext<'host, 'runtime> {
    PluginContext::new(
        host,
        InvocationDefaultsSdk {
            options: SdkClient::new(host, invocation_defaults_component_id()),
        },
        (),
        (),
    )
}

struct InvocationDefaultsPlugin;

impl PluginInstance for InvocationDefaultsPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let context = context(host);
        if service == &invocation_defaults_service() {
            let command = context
                .kernel
                .decode_projected::<InvocationDefaultsCommand>(
                    &InvocationDefaultsInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let params = match command {
                InvocationDefaultsCommand::Resolve { request } => {
                    resolve_defaults(&context, &request)?
                }
                InvocationDefaultsCommand::ResolveHelper { request } => {
                    resolve_helper_defaults(&request)
                }
            };
            return context
                .kernel
                .encode_value(&InvocationDefaultsResponse::Params { params })
                .map_err(|error| error.to_string());
        }
        if service == &invocation_clock_service() {
            let command = context
                .kernel
                .decode_projected::<InvocationClockCommand>(
                    &InvocationClockInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            match command {
                InvocationClockCommand::Now => {
                    let millis = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|error| format!("system clock is before unix epoch: {error}"))?
                        .as_millis();
                    let now_ms = u64::try_from(millis)
                        .map_err(|_| "system clock does not fit u64 milliseconds".to_owned())?;
                    return context
                        .kernel
                        .encode_value(&InvocationClockResponse::Time { now_ms })
                        .map_err(|error| error.to_string());
                }
            }
        }
        if service == &context_recovery_service() {
            let command = context
                .kernel
                .decode_projected::<ContextRecoveryCommand>(
                    &ContextRecoveryInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let ContextRecoveryCommand::Assess { request } = command;
            let decision = assess_recovery(&request)?;
            return context
                .kernel
                .encode_value(&ContextRecoveryResponse::Decision { decision })
                .map_err(|error| error.to_string());
        }
        Err(format!(
            "unsupported invocation defaults service: {service}"
        ))
    }
}

fn assess_recovery(request: &ContextRecoveryRequest) -> Result<ContextRecoveryDecision, String> {
    let policy = RecoveryClassifierPolicy::default();
    if request.prompt.len() > policy.max_prompt_bytes as usize {
        return Err(format!(
            "recovery prompt exceeds {} bytes",
            policy.max_prompt_bytes
        ));
    }
    if request.state.anchors.len() > policy.max_anchors as usize {
        return Err(format!(
            "recovery anchors exceed {} entries",
            policy.max_anchors
        ));
    }
    if matches!(
        recovery_cold_gate(&request.state),
        RecoveryColdGate::CurrentContextSufficient
    ) {
        return Ok(ContextRecoveryDecision::Sufficient);
    }
    let query = bounded_utf8(request.prompt.trim(), policy.max_need_query_bytes as usize);
    if query.is_empty() {
        return Ok(ContextRecoveryDecision::Sufficient);
    }
    validate_recovery_decision(
        ContextRecoveryDecision::Missing {
            needs: vec![ContextNeed::Task { query }],
        },
        &policy,
    )
    .map_err(|error| format!("invalid recovery decision: {error:?}"))
}

fn bounded_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn resolve_defaults(
    context: &InvocationDefaultsContext<'_, '_>,
    request: &InvocationRequest,
) -> Result<InvocationParams, String> {
    let option_context = invocation_option_context(request)?;
    let response: OptionResponse = context
        .sdk
        .options
        .invoke_projected(&OptionCommand::Resolve {
            key: OptionKey::parse(ROUTING_PROFILE_OPTION)?,
            context: option_context,
        })
        .map_err(|error| format!("cannot resolve {ROUTING_PROFILE_OPTION}: {error}"))?;
    let OptionResponse::Value { option } = response else {
        return Err(format!(
            "options service returned a non-value response for {ROUTING_PROFILE_OPTION}"
        ));
    };
    let OptionValue::String(profile) = option.value else {
        return Err(format!("{ROUTING_PROFILE_OPTION} must be a string"));
    };
    let profile_id = RoutingProfileId::parse(profile)
        .map_err(|error| format!("invalid {ROUTING_PROFILE_OPTION}: {error}"))?;
    Ok(invocation_params(
        profile_id,
        request.callable_id.as_ref(),
        &request.tools,
        DEFAULT_POLICY_REVISION,
        DEFAULT_ROUTE_POLICY_REVISION,
        1,
    ))
}

fn invocation_option_context(request: &InvocationRequest) -> Result<OptionContext, String> {
    Ok(OptionContext {
        session: request
            .session_id
            .as_ref()
            .map(|session| OptionSubjectId::parse(session.as_str().to_owned()))
            .transpose()?,
        agent: request
            .callable_id
            .as_ref()
            .map(|callable| OptionSubjectId::parse(callable.as_str().to_owned()))
            .transpose()?,
    })
}

fn resolve_helper_defaults(request: &HelperInvocationRequest) -> InvocationParams {
    invocation_params(
        request.profile_id.clone(),
        Some(&request.callable_id),
        &request.tools,
        HELPER_POLICY_REVISION,
        HELPER_ROUTE_POLICY_REVISION,
        0,
    )
}

fn invocation_params(
    profile_id: RoutingProfileId,
    _callable_id: Option<&CallableId>,
    tools: &[ModelToolDescriptor],
    policy_revision: &str,
    route_policy_revision: &str,
    max_retries: u32,
) -> InvocationParams {
    let optional_tools = tools
        .iter()
        .map(|tool| tool.id.clone())
        .collect::<BTreeSet<_>>();
    InvocationParams {
        profile_id,
        policy: UsagePolicy {
            revision: policy_revision.into(),
            max_fresh_input_tokens: 128 * 1024,
            max_output_tokens: 16 * 1024,
            max_cost_microunits: None,
            max_retries,
            max_tool_result_bytes: 1024 * 1024,
            max_tool_schemas: 128,
            max_skills: 64,
            require_known_capacity: false,
            delegation: DelegationResourcePolicy::default(),
        },
        intent: InvocationIntent {
            output_reserve_tokens: 8 * 1024,
            required_context_capabilities: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
            required_tools: BTreeSet::new(),
            optional_tools,
            required_skills: BTreeSet::new(),
            optional_skills: BTreeSet::new(),
            requested_reasoning: None,
            deadline_at_ms: None,
        },
        route_policy: RouteSelectionPolicy {
            revision: route_policy_revision.into(),
            estimates: RoutingEstimateMode::Ignore,
            max_candidate_attempts: 8,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::Bytes;
    use phenix_sdk::{ContextRecoveryState, HelperInvocationKind};

    #[test]
    fn provider_exports_replaceable_defaults_clock_and_recovery_interfaces() {
        let authority = Authority::default();
        let manifest = invocation_defaults_manifest(authority.clone());
        assert_eq!(manifest.services.len(), 3);
        assert_eq!(manifest.dependencies.len(), 1);

        let component = invocation_defaults_component_manifest(authority);
        assert_eq!(component.imports.len(), 1);
        assert_eq!(
            component.imports[0].interface,
            OptionsInterface::interface_id()
        );
        assert_eq!(component.exports.len(), 3);
        assert!(component
            .exports
            .iter()
            .any(|export| export.interface == InvocationDefaultsInterface::interface_id()));
        assert!(component
            .exports
            .iter()
            .any(|export| export.interface == InvocationClockInterface::interface_id()));
        assert!(component
            .exports
            .iter()
            .any(|export| export.interface == ContextRecoveryInterface::interface_id()));
    }

    #[test]
    fn cold_context_requests_bounded_task_recovery() {
        let decision = assess_recovery(&ContextRecoveryRequest {
            profile_id: RoutingProfileId::parse("default").unwrap(),
            prompt: "work on prs".into(),
            state: ContextRecoveryState {
                anchors: Vec::new(),
                has_durable_session_history: false,
                has_explicit_resource: false,
            },
            at: 1,
        })
        .unwrap();
        assert!(matches!(
            decision,
            ContextRecoveryDecision::Missing { needs }
                if matches!(&needs[..], [ContextNeed::Task { query }] if query == "work on prs")
        ));
    }

    #[test]
    fn invocation_options_include_session_and_agent_identity() {
        let request = InvocationRequest {
            execution_id: "execution-1".into(),
            session_id: Some(phenix_core::SessionId::parse("session-1").unwrap()),
            parent_attempt_id: None,
            callable_id: Some(CallableId::parse("agent.coordinator").unwrap()),
            input: Bytes::from(b"prompt".to_vec()),
            tools: Vec::new(),
            continuation: Vec::new(),
        };
        let context = invocation_option_context(&request).unwrap();
        assert_eq!(
            context.session.as_ref().map(OptionSubjectId::as_str),
            Some("session-1")
        );
        assert_eq!(
            context.agent.as_ref().map(OptionSubjectId::as_str),
            Some("agent.coordinator")
        );
    }

    #[test]
    fn helper_defaults_preserve_pinned_profile_and_disable_retries() {
        let request = HelperInvocationRequest {
            execution_id: "execution-1".into(),
            parent_attempt_id: "attempt-1".into(),
            profile_id: RoutingProfileId::parse("router.pinned").unwrap(),
            kind: HelperInvocationKind::Helper,
            callable_id: CallableId::parse("memory.summarize").unwrap(),
            input: Bytes::from(b"input".to_vec()),
            tools: Vec::new(),
        };
        let params = resolve_helper_defaults(&request);
        assert_eq!(params.profile_id, request.profile_id);
        assert_eq!(params.policy.revision, HELPER_POLICY_REVISION);
        assert_eq!(params.policy.max_retries, 0);
    }
}
