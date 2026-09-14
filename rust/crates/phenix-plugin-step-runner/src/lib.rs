#![forbid(unsafe_code)]

mod runner;

use phenix_core::{
    Authority, ComponentExport, ComponentInterface, ComponentManifest, PluginContext, PluginHost,
    PluginInstance, PluginManifest, ServiceContribution, ServiceId, ServiceRole,
};
use phenix_sdk::{
    invocation_service, step_runner_service, InvocationCommand, InvocationInterface,
    StepRunnerCommand,
};

pub use runner::{step_runner_component_id, STEP_RUNNER_COMPONENT, STEP_RUNNER_PLUGIN};

#[must_use]
pub fn step_runner_manifest(maximum_authority: Authority) -> PluginManifest {
    let mut manifest = runner::step_runner_manifest(maximum_authority);
    manifest.services.push(ServiceContribution {
        role: ServiceRole::Terminal,
        service: invocation_service(),
        priority: 100,
        required_authority: Authority::default(),
    });
    manifest
}

#[must_use]
pub fn step_runner_component_manifest(maximum_authority: Authority) -> ComponentManifest {
    let mut manifest = runner::step_runner_component_manifest(maximum_authority);
    manifest.exports.push(ComponentExport {
        interface: InvocationInterface::interface_id(),
        schema: InvocationInterface::schema(),
        priority: 100,
        required_authority: Authority::default(),
    });
    manifest
}

#[must_use]
pub fn step_runner_factory() -> Box<dyn PluginInstance> {
    Box::new(InvocationPackage {
        runner: runner::step_runner_factory(),
    })
}

struct InvocationPackage {
    runner: Box<dyn PluginInstance>,
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
        if service != &invocation_service() {
            return self.runner.invoke(service, input, host);
        }

        let context: PluginContext<'_, '_, ()> = PluginContext::new(host, (), (), ());
        let command = context
            .kernel
            .decode_projected::<InvocationCommand>(&InvocationInterface::interface_id(), input)
            .map_err(|error| error.to_string())?;
        let step = match command {
            InvocationCommand::Invoke { request, params } => StepRunnerCommand::Run {
                request: request.into_planned_step(params),
            },
        };
        let encoded = context
            .kernel
            .encode_value(&step)
            .map_err(|error| error.to_string())?;
        self.runner.invoke(&step_runner_service(), &encoded, host)
    }

    fn stop(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        self.runner.stop(host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_package_exports_direct_and_prepared_invocation() {
        let authority = Authority::default();
        let manifest = step_runner_manifest(authority.clone());
        assert!(manifest
            .services
            .iter()
            .any(|service| service.service == invocation_service()));
        assert!(manifest
            .services
            .iter()
            .any(|service| service.service == step_runner_service()));

        let component = step_runner_component_manifest(authority);
        assert!(component
            .exports
            .iter()
            .any(|export| export.interface == InvocationInterface::interface_id()));
        assert!(component
            .exports
            .iter()
            .any(|export| export.interface == phenix_sdk::StepRunnerInterface::interface_id()));
    }
}
