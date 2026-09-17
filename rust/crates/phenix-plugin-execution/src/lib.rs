#![forbid(unsafe_code)]

mod agent_loop;
#[cfg(test)]
mod agent_loop_regression;
mod attempt_service;
mod component;
mod configuration;
#[cfg(test)]
mod configuration_regression;
mod delegated_task_state;
#[cfg(test)]
mod generation_regression;
mod implementation;
mod resource_service;
mod resource_transaction;
mod review;
mod step_transaction_service;
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
pub use phenix_sdk::{
    execution_resource_service, step_attempt_service, step_transaction_service,
    ExecutionResourceCommand, ExecutionResourceInterface, ExecutionResourceResponse,
    StepAttemptCommand, StepAttemptInterface, StepAttemptPhase, StepAttemptRecord,
    StepAttemptResponse, StepTransactionCommand, StepTransactionInterface, StepTransactionResponse,
    EXECUTION_RESOURCE_SERVICE, STEP_ATTEMPT_SERVICE, STEP_TRANSACTION_SERVICE,
};
pub use review::{
    execution_review_service, ExecutionReviewCommand, ExecutionReviewInterface,
    ExecutionReviewResponse, PreparedReviewFile, EXECUTION_REVIEW_SERVICE,
};
pub use tool_schedule::{ScheduledToolBatch, ToolCallPlan, ToolConcurrency, ToolScheduler};

use phenix_core::{
    Authority, PluginHost, PluginInstance, PluginManifest, ServiceContribution, ServiceId,
};

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
    manifest.services.push(ServiceContribution {
        role: phenix_core::ServiceRole::Terminal,
        service: execution_resource_service(),
        priority: 100,
        required_authority: Authority::default(),
    });
    manifest.services.push(ServiceContribution {
        role: phenix_core::ServiceRole::Terminal,
        service: step_attempt_service(),
        priority: 100,
        required_authority: Authority::default(),
    });
    manifest.services.push(ServiceContribution {
        role: phenix_core::ServiceRole::Terminal,
        service: step_transaction_service(),
        priority: 100,
        required_authority: Authority::default(),
    });
    manifest
        .resource_namespaces
        .push(configuration::execution_configuration_namespace());
    manifest
        .resource_namespaces
        .push(resource_service::execution_resource_namespace());
    manifest
        .resource_namespaces
        .push(attempt_service::attempt_namespace());
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
        resources: resource_service::resource_factory(),
        attempts: attempt_service::attempt_factory(),
        step_transactions: step_transaction_service::step_transaction_factory(),
        review: review::execution_review_factory(),
    })
}

struct ExecutionPackagePlugin {
    execution: Box<dyn PluginInstance>,
    configuration: Box<dyn PluginInstance>,
    agent_loop: Box<dyn PluginInstance>,
    resources: Box<dyn PluginInstance>,
    attempts: Box<dyn PluginInstance>,
    step_transactions: Box<dyn PluginInstance>,
    review: Box<dyn PluginInstance>,
}

impl PluginInstance for ExecutionPackagePlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        self.execution.start(host)?;
        self.configuration.start(host)?;
        self.resources.start(host)?;
        self.attempts.start(host)?;
        self.step_transactions.start(host)?;
        self.review.start(host)?;
        self.agent_loop.start(host)
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
        if service == &execution_resource_service() {
            return self.resources.invoke(service, input, host);
        }
        if service == &step_attempt_service() {
            return self.attempts.invoke(service, input, host);
        }
        if service == &step_transaction_service() {
            return self.step_transactions.invoke(service, input, host);
        }
        self.execution.invoke(service, input, host)
    }

    fn stop(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        self.agent_loop.stop(host)?;
        self.review.stop(host)?;
        self.step_transactions.stop(host)?;
        self.attempts.stop(host)?;
        self.resources.stop(host)?;
        self.configuration.stop(host)?;
        self.execution.stop(host)
    }
}

#[cfg(test)]
mod attempt_integration;
#[cfg(test)]
mod resource_integration;
#[cfg(test)]
mod root_reservation_integration;
