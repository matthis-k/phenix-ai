#![forbid(unsafe_code)]

mod runner;

use phenix_core::{
    Authority, ComponentExport, ComponentImport, ComponentInterface, ComponentManifest,
    PluginContext, PluginHost, PluginInstance, PluginManifest, SdkClient, ServiceContribution,
    ServiceId, ServiceRole,
};
use phenix_sdk::{
    context_service, default_invocation_service, invocation_service, step_runner_service,
    ContextCommand, ContextInterface, ContextResponse, DefaultInvocationCommand,
    DefaultInvocationInterface, ExecutionCommand, ExecutionInterface, ExecutionResponse,
    InvocationClockCommand, InvocationClockInterface, InvocationClockResponse, InvocationCommand,
    InvocationDefaultsCommand, InvocationDefaultsInterface, InvocationDefaultsResponse,
    InvocationInterface, InvocationParams, InvocationRequest, PlannedStepRequest,
    StepAttemptCommand, StepAttemptInterface, StepAttemptResponse, StepRunnerCommand,
};
use std::collections::BTreeSet;

pub use runner::{step_runner_component_id, STEP_RUNNER_COMPONENT, STEP_RUNNER_PLUGIN};

#[must_use]
pub fn step_runner_manifest(maximum_authority: Authority) -> PluginManifest {
    let mut manifest = runner::step_runner_manifest(maximum_authority);
    for service in [invocation_service(), default_invocation_service()] {
        manifest.services.push(ServiceContribution {
            role: ServiceRole::Terminal,
            service,
            priority: 100,
            required_authority: Authority::default(),
        });
    }
    manifest
}

#[must_use]
pub fn step_runner_component_manifest(maximum_authority: Authority) -> ComponentManifest {
    let mut manifest = runner::step_runner_component_manifest(maximum_authority);
    manifest.imports.push(ComponentImport {
        interface: InvocationDefaultsInterface::interface_id(),
        schema: InvocationDefaultsInterface::schema(),
        required: false,
        authority: manifest.maximum_authority.clone(),
    });
    manifest.imports.push(ComponentImport {
        interface: InvocationClockInterface::interface_id(),
        schema: InvocationClockInterface::schema(),
        required: true,
        authority: manifest.maximum_authority.clone(),
    });
    for interface in [
        (
            InvocationInterface::interface_id(),
            InvocationInterface::schema(),
        ),
        (
            DefaultInvocationInterface::interface_id(),
            DefaultInvocationInterface::schema(),
        ),
    ] {
        manifest.exports.push(ComponentExport {
            interface: interface.0,
            schema: interface.1,
            priority: 100,
            required_authority: Authority::default(),
        });
    }
    manifest
}

#[must_use]
pub fn step_runner_factory() -> Box<dyn PluginInstance> {
    Box::new(InvocationPackage {
        runner: runner::step_runner_factory(),
    })
}

struct InvocationSdk<'host, 'runtime> {
    defaults: SdkClient<'host, 'runtime, InvocationDefaultsInterface>,
    clock: SdkClient<'host, 'runtime, InvocationClockInterface>,
    context: SdkClient<'host, 'runtime, ContextInterface>,
    execution: SdkClient<'host, 'runtime, ExecutionInterface>,
    attempts: SdkClient<'host, 'runtime, StepAttemptInterface>,
}

type InvocationContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, InvocationSdk<'host, 'runtime>>;

fn invocation_context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> InvocationContext<'host, 'runtime> {
    let component = step_runner_component_id();
    PluginContext::new(
        host,
        InvocationSdk {
            defaults: SdkClient::new(host, component.clone()),
            clock: SdkClient::new(host, component.clone()),
            context: SdkClient::new(host, component.clone()),
            execution: SdkClient::new(host, component.clone()),
            attempts: SdkClient::new(host, component),
        },
        (),
        (),
    )
}

struct InvocationPackage {
    runner: Box<dyn PluginInstance>,
}

impl InvocationPackage {
    fn invoke_explicit(
        &mut self,
        context: &InvocationContext<'_, '_>,
        host: &PluginHost<'_>,
        request: InvocationRequest,
        params: InvocationParams,
    ) -> Result<Vec<u8>, String> {
        let root_execution_id = root_execution_id(context, &request.execution_id)?;
        let prepared: ContextResponse = context
            .sdk
            .context
            .invoke_projected(&ContextCommand::PrepareInvocation {
                execution_id: request.execution_id.clone(),
                input: request.input.clone(),
            })
            .map_err(|error| format!("invocation context preparation failed: {error}"))?;
        let ContextResponse::InvocationPrepared { preparation } = prepared else {
            return Err("context service returned a non-preparation response".into());
        };

        let clock: InvocationClockResponse = context
            .sdk
            .clock
            .invoke_projected(&InvocationClockCommand::Now)
            .map_err(|error| format!("invocation clock unavailable: {error}"))?;
        let InvocationClockResponse::Time { now_ms } = clock;

        let allocated: StepAttemptResponse = context
            .sdk
            .attempts
            .invoke_projected(&StepAttemptCommand::AllocateIdentity {
                root_execution_id,
                execution_id: request.execution_id.clone(),
                parent_attempt_id: request.parent_attempt_id.clone(),
                policy_revision: params.policy.revision.clone(),
            })
            .map_err(|error| format!("invocation attempt allocation failed: {error}"))?;
        let StepAttemptResponse::Attribution { attribution } = allocated else {
            return Err(
                "step attempt service returned a non-attribution allocation response".into(),
            );
        };

        let task = params.intent.derive_task(&preparation);
        let step = StepRunnerCommand::Run {
            request: PlannedStepRequest {
                attribution,
                profile_id: params.profile_id,
                callable_id: request.callable_id,
                input: request.input,
                tools: request.tools,
                policy: params.policy,
                task,
                context_candidates: preparation.candidates,
                cache_epoch: preparation.projection.cache_epoch,
                route_policy: params.route_policy,
                now_ms,
            },
        };
        let encoded = context
            .kernel
            .encode_value(&step)
            .map_err(|error| error.to_string())?;
        self.runner.invoke(&step_runner_service(), &encoded, host)
    }
}

impl PluginInstance for InvocationPackage {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        self.runner.start(host)
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let context = invocation_context(host);
        if service == &invocation_service() {
            let command = context
                .kernel
                .decode_projected::<InvocationCommand>(&InvocationInterface::interface_id(), input)
                .map_err(|error| error.to_string())?;
            let InvocationCommand::Invoke { request, params } = command;
            return self.invoke_explicit(&context, host, request, params);
        }
        if service == &default_invocation_service() {
            let command = context
                .kernel
                .decode_projected::<DefaultInvocationCommand>(
                    &DefaultInvocationInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let DefaultInvocationCommand::Invoke { request } = command;
            let resolved: InvocationDefaultsResponse = context
                .sdk
                .defaults
                .invoke_projected(&InvocationDefaultsCommand::Resolve {
                    request: request.clone(),
                })
                .map_err(|error| format!("default invocation parameters unavailable: {error}"))?;
            let InvocationDefaultsResponse::Params { params } = resolved;
            return self.invoke_explicit(&context, host, request, params);
        }
        self.runner.invoke(service, input, host)
    }

    fn stop(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        self.runner.stop(host)
    }
}

fn root_execution_id(
    context: &InvocationContext<'_, '_>,
    execution_id: &str,
) -> Result<String, String> {
    if execution_id.trim().is_empty() {
        return Err("invocation execution id must not be empty".into());
    }
    let mut current = execution_id.to_owned();
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(current.clone()) {
            return Err("execution parent lineage contains a cycle".into());
        }
        let response: ExecutionResponse = context
            .sdk
            .execution
            .invoke_projected(&ExecutionCommand::GetExecution {
                id: current.clone(),
            })
            .map_err(|error| format!("execution lookup failed: {error}"))?;
        let ExecutionResponse::ExecutionLookup {
            execution: Some(execution),
        } = response
        else {
            return Err(format!("unknown invocation execution: {current}"));
        };
        match execution.parent_execution {
            Some(parent) => current = parent,
            None => return Ok(execution.id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_package_exports_direct_default_and_prepared_invocation() {
        let authority = Authority::default();
        let manifest = step_runner_manifest(authority.clone());
        assert!(manifest.dependencies.is_empty());
        for service in [
            invocation_service(),
            default_invocation_service(),
            step_runner_service(),
        ] {
            assert!(manifest
                .services
                .iter()
                .any(|contribution| contribution.service == service));
        }

        let component = step_runner_component_manifest(authority);
        for interface in [
            InvocationInterface::interface_id(),
            DefaultInvocationInterface::interface_id(),
            phenix_sdk::StepRunnerInterface::interface_id(),
        ] {
            assert!(component
                .exports
                .iter()
                .any(|export| export.interface == interface));
        }
        let defaults = component
            .imports
            .iter()
            .find(|import| import.interface == InvocationDefaultsInterface::interface_id())
            .expect("default invocation imports the defaults provider");
        assert!(!defaults.required);
        let clock = component
            .imports
            .iter()
            .find(|import| import.interface == InvocationClockInterface::interface_id())
            .expect("central invocation imports a clock provider");
        assert!(clock.required);
        assert!(component.imports.iter().any(|import| {
            import.interface == ContextInterface::interface_id() && import.required
        }));
        assert!(component.imports.iter().any(|import| {
            import.interface == ExecutionInterface::interface_id() && import.required
        }));
        assert!(component.imports.iter().any(|import| {
            import.interface == StepAttemptInterface::interface_id() && import.required
        }));
    }

    #[test]
    fn context_service_identity_is_not_redefined_by_invocation_package() {
        assert_eq!(context_service(), phenix_sdk::context_service());
    }
}
