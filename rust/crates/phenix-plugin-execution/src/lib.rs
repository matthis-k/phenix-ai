#![forbid(unsafe_code)]

mod agent_loop;
#[cfg(test)]
mod agent_loop_regression;
mod component;
mod configuration;
#[cfg(test)]
mod configuration_regression;
#[cfg(test)]
mod generation_regression;
mod implementation;
mod review;
mod tool_schedule;

pub use agent_loop::{
    agent_loop_service, AgentLoopCommand, AgentLoopPolicy, AgentLoopResponse, AgentLoopUsage,
    AGENT_LOOP_SERVICE, DEFAULT_MAX_PARALLEL_TOOL_CALLS,
};
pub use component::*;
pub use configuration::{
    execution_configuration_service, AgentDefinition, CallablePolicy,
    ExecutionConfigurationCommand, ExecutionConfigurationResponse, OrchestrationDefinition,
    OrchestrationNode, EXECUTION_CONFIGURATION_SERVICE,
};
pub use review::{
    execution_review_service, ExecutionReviewCommand, ExecutionReviewInterface,
    ExecutionReviewResponse, PreparedReviewFile, EXECUTION_REVIEW_SERVICE,
};
pub use tool_schedule::{ScheduledToolBatch, ToolCallPlan, ToolConcurrency, ToolScheduler};

use phenix_core::{
    Authority, PluginHost, PluginInstance, PluginManifest, ServiceContribution, ServiceId,
    SharedPluginInvocation,
};
use std::sync::Arc;

#[must_use]
pub fn execution_manifest(maximum_authority: Authority) -> PluginManifest {
    let mut manifest = implementation::execution_manifest(maximum_authority);
    manifest.services.push(ServiceContribution {
        role: phenix_core::ServiceRole::Terminal,
        service: configuration::execution_configuration_service(),
        priority: 100,
        required_authority: Authority::default(),
    });
    manifest.services.push(ServiceContribution {
        role: phenix_core::ServiceRole::Terminal,
        service: agent_loop::agent_loop_service(),
        priority: 100,
        required_authority: Authority::default(),
    });
    manifest.services.push(ServiceContribution {
        role: phenix_core::ServiceRole::Terminal,
        service: review::execution_review_service(),
        priority: 100,
        required_authority: Authority::default(),
    });
    manifest
        .resource_namespaces
        .push(configuration::execution_configuration_namespace());
    manifest
        .resource_namespaces
        .push(review::execution_review_namespace());
    manifest
}

#[must_use]
pub fn execution_factory() -> Box<dyn PluginInstance> {
    Box::new(ExecutionPackagePlugin {
        execution: implementation::execution_factory(),
        configuration: configuration::configuration_factory(),
        agent_loop: agent_loop::agent_loop_factory(),
        review: review::execution_review_factory(),
    })
}

struct ExecutionPackagePlugin {
    execution: Box<dyn PluginInstance>,
    configuration: Box<dyn PluginInstance>,
    agent_loop: Box<dyn PluginInstance>,
    review: Box<dyn PluginInstance>,
}

struct ExecutionPackageSharedInvocation;

impl SharedPluginInvocation for ExecutionPackageSharedInvocation {
    fn supports(&self, service: &ServiceId) -> bool {
        service == &agent_loop::agent_loop_service()
    }

    fn invoke(
        &self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        agent_loop::invoke_agent_loop(service, input, host)
    }
}

impl PluginInstance for ExecutionPackagePlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        self.execution.start(host)?;
        self.configuration.start(host)?;
        self.agent_loop.start(host)?;
        self.review.start(host)
    }

    fn shared_invocation(&self) -> Option<Arc<dyn SharedPluginInvocation>> {
        Some(Arc::new(ExecutionPackageSharedInvocation))
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service == &configuration::execution_configuration_service() {
            return self.configuration.invoke(service, input, host);
        }
        if service == &agent_loop::agent_loop_service() {
            return self.agent_loop.invoke(service, input, host);
        }
        if service == &review::execution_review_service() {
            return self.review.invoke(service, input, host);
        }

        self.execution.invoke(service, input, host)
    }

    fn stop(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        self.review.stop(host)?;
        self.agent_loop.stop(host)?;
        self.configuration.stop(host)?;
        self.execution.stop(host)
    }
}
