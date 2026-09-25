use phenix_core::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, PluginContext, PluginExecution, PluginHost, PluginId, PluginInstance,
    PluginManifest, SdkClient, ServiceContribution, ServiceId,
};
use phenix_sdk::{
    derive_efficiency_task_record_from_attempts, efficiency_evaluation_service,
    EfficiencyDurableTaskEvidence, EfficiencyEvaluationCommand, EfficiencyEvaluationInterface,
    EfficiencyEvaluationResponse, StepAttemptCommand, StepAttemptInterface, StepAttemptResponse,
};

pub const EFFICIENCY_EVALUATION_PLUGIN: &str = "phenix.efficiency-evaluation";
pub const EFFICIENCY_EVALUATION_COMPONENT: &str = "phenix.efficiency-evaluation";

#[must_use]
pub fn efficiency_evaluation_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(EFFICIENCY_EVALUATION_PLUGIN)
            .expect("static efficiency evaluation plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: vec![
            PluginId::parse("phenix.execution").expect("static execution plugin id is valid")
        ],
        services: vec![ServiceContribution {
            role: phenix_core::ServiceRole::Terminal,
            service: efficiency_evaluation_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

#[must_use]
pub fn efficiency_evaluation_component_id() -> ComponentId {
    ComponentId::parse(EFFICIENCY_EVALUATION_COMPONENT)
        .expect("static efficiency evaluation component id is valid")
}

#[must_use]
pub fn efficiency_evaluation_component_manifest() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: efficiency_evaluation_component_id(),
        owner: PluginId::parse(EFFICIENCY_EVALUATION_PLUGIN)
            .expect("static efficiency evaluation plugin id is valid"),
        imports: vec![ComponentImport {
            interface: StepAttemptInterface::interface_id(),
            schema: StepAttemptInterface::schema(),
            required: true,
            authority: Authority::default(),
        }],
        exports: vec![ComponentExport {
            interface: EfficiencyEvaluationInterface::interface_id(),
            schema: EfficiencyEvaluationInterface::schema(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        maximum_authority: Authority::default(),
    }
}

#[must_use]
pub fn efficiency_evaluation_factory() -> Box<dyn PluginInstance> {
    Box::new(EfficiencyEvaluationPlugin)
}

struct EvaluationSdk<'host, 'runtime> {
    attempts: SdkClient<'host, 'runtime, StepAttemptInterface>,
}

type EvaluationContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, EvaluationSdk<'host, 'runtime>>;

fn context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> EvaluationContext<'host, 'runtime> {
    let component = efficiency_evaluation_component_id();
    PluginContext::new(
        host,
        EvaluationSdk {
            attempts: SdkClient::new(host, component),
        },
        (),
        (),
    )
}

struct EfficiencyEvaluationPlugin;

impl PluginInstance for EfficiencyEvaluationPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &efficiency_evaluation_service() {
            return Err(format!(
                "unsupported efficiency evaluation service: {service}"
            ));
        }
        let context = context(host);
        let command = context
            .kernel
            .decode_projected::<EfficiencyEvaluationCommand>(
                &EfficiencyEvaluationInterface::interface_id(),
                input,
            )
            .map_err(|error| error.to_string())?;
        let response = match command {
            EfficiencyEvaluationCommand::CollectTask { request } => {
                let listed: StepAttemptResponse = context
                    .sdk
                    .attempts
                    .invoke_projected(&StepAttemptCommand::ListRoot {
                        root_execution_id: request.root_execution_id.clone(),
                    })
                    .map_err(|error| format!("attempt evidence unavailable: {error}"))?;
                let StepAttemptResponse::Attempts { attempts } = listed else {
                    return Err("attempt service returned a non-list response".into());
                };
                let durable = EfficiencyDurableTaskEvidence {
                    task_fixture_revision: request.task_fixture_revision,
                    root_execution_id: request.root_execution_id,
                    policy_revision: request.policy_revision,
                    outcome_evaluator_identity: request.outcome_evaluator_identity,
                    price_revision: request.price_revision,
                    outcome: request.outcome_evidence.outcome,
                    outcome_evidence: request.outcome_evidence,
                    attempts,
                    root_elapsed_ms: request.root_elapsed_ms,
                };
                let record = derive_efficiency_task_record_from_attempts(&durable)
                    .map_err(|error| format!("efficiency evidence invalid: {error:?}"))?;
                EfficiencyEvaluationResponse::Task { record }
            }
        };
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_reads_attempts_and_exports_only_derived_evaluation() {
        let manifest = efficiency_evaluation_component_manifest();
        assert_eq!(manifest.imports.len(), 1);
        assert_eq!(
            manifest.imports[0].interface,
            StepAttemptInterface::interface_id()
        );
        assert!(manifest.imports[0].required);
        assert_eq!(manifest.exports.len(), 1);
        assert_eq!(
            manifest.exports[0].interface,
            EfficiencyEvaluationInterface::interface_id()
        );
    }

    #[test]
    fn plugin_declares_execution_as_its_only_source_dependency() {
        let manifest = efficiency_evaluation_manifest();
        assert_eq!(manifest.dependencies.len(), 1);
        assert_eq!(manifest.dependencies[0].as_str(), "phenix.execution");
    }
}
