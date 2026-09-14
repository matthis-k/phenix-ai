#![forbid(unsafe_code)]

mod runner;

use phenix_core::{
    Authority, ComponentExport, ComponentImport, ComponentInterface, ComponentManifest,
    PluginContext, PluginHost, PluginInstance, PluginManifest, SdkClient, ServiceContribution,
    ServiceId, ServiceRole,
};
use phenix_sdk::{
    default_invocation_service, invocation_service, step_runner_service, DefaultInvocationCommand,
    DefaultInvocationInterface, InvocationCommand, InvocationDefaultsCommand,
    InvocationDefaultsInterface, InvocationDefaultsResponse, InvocationInterface, InvocationParams,
    InvocationRequest, StepRunnerCommand,
};

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
}

type InvocationContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, InvocationSdk<'host, 'runtime>>;

fn invocation_context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> InvocationContext<'host, 'runtime> {
    PluginContext::new(
        host,
        InvocationSdk {
            defaults: SdkClient::new(host, step_runner_component_id()),
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
        let step = StepRunnerCommand::Run {
            request: request.into_planned_step(params),
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
    }
}
