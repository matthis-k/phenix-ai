#![forbid(unsafe_code)]
mod component;
mod implementation;
pub use component::*;
pub use implementation::*;

mod service {
    use super::{
        RepositorySelectionReason, RepositoryWorkSnapshot, RepositoryWorkerInterface,
        RepositoryWorkerQueue,
    };
    use phenix_core::{
        Authority, ComponentInterface, PluginContext, PluginExecution, PluginHost, PluginId,
        PluginInstance, PluginManifest, ServiceContribution, ServiceId,
    };

    pub const REPOSITORY_WORK_QUEUE_SERVICE: &str = "phenix.repository.worker-queue@1";
    const REPOSITORY_WORKER_PLUGIN: &str = "phenix.repository-workers";

    #[derive(Clone, Debug, Eq, PartialEq, phenix_sdk_macros::PhenixValue)]
    pub(crate) enum RepositoryWorkQueueResponse {
        Selected { pr_number: u64, reason: String },
        Empty,
    }

    #[must_use]
    pub fn repository_worker_manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId::parse(REPOSITORY_WORKER_PLUGIN).expect("static plugin id is valid"),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                role: phenix_core::ServiceRole::Terminal,
                service: repository_work_queue_service(),
                priority: 100,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    #[must_use]
    pub fn repository_worker_factory() -> Box<dyn PluginInstance> {
        Box::new(RepositoryWorkerPlugin)
    }

    #[must_use]
    pub fn repository_work_queue_service() -> ServiceId {
        ServiceId::parse(REPOSITORY_WORK_QUEUE_SERVICE).expect("static service id is valid")
    }

    struct RepositoryWorkerPlugin;

    impl PluginInstance for RepositoryWorkerPlugin {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            service: &ServiceId,
            input: &[u8],
            host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            if service != &repository_work_queue_service() {
                return Err(format!("unsupported repository worker service: {service}"));
            }
            let context = PluginContext::new(host, (), (), ());
            let interface = RepositoryWorkerInterface::interface_id();
            let snapshot = context
                .kernel
                .decode_projected::<RepositoryWorkSnapshot>(&interface, input)
                .map_err(|error| error.to_string())?;
            let queue = RepositoryWorkerQueue::reconstruct(&snapshot);
            let result = match queue.select_work() {
                Some(selection) => RepositoryWorkQueueResponse::Selected {
                    pr_number: selection.pr_number,
                    reason: selection_reason_name(selection.reason).to_owned(),
                },
                None => RepositoryWorkQueueResponse::Empty,
            };
            context
                .kernel
                .encode_value(&result)
                .map_err(|error| error.to_string())
        }
    }

    fn selection_reason_name(reason: RepositorySelectionReason) -> &'static str {
        match reason {
            RepositorySelectionReason::BrokenValidation => "broken_validation",
            RepositorySelectionReason::BlockingFinding => "blocking_finding",
            RepositorySelectionReason::StaleOrIncomplete => "stale_or_incomplete",
            RepositorySelectionReason::MissingContractEvidence => "missing_contract_evidence",
            RepositorySelectionReason::DependencyBlocking => "dependency_blocking",
            RepositorySelectionReason::NextReady => "next_ready",
        }
    }
}

pub use service::{
    REPOSITORY_WORK_QUEUE_SERVICE, repository_work_queue_service, repository_worker_factory,
    repository_worker_manifest,
};

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{Authority, Kernel, KernelConfig, PhenixValue, PluginState, Project};
    use std::collections::BTreeSet;

    #[derive(Clone, Debug, Eq, PartialEq, phenix_sdk_macros::PhenixValue)]
    enum RepositoryWorkQueueView {
        Selected { pr_number: u64, reason: String },
        Empty,
    }

    fn success_validation() -> RepositoryValidation {
        RepositoryValidation {
            source: RepositoryCheckState::Success,
            rust: RepositoryCheckState::Success,
            product: RepositoryCheckState::Success,
            integration_system: RepositoryCheckState::Success,
            maintenance: RepositoryCheckState::Success,
            maintenance_autofix: RepositoryCheckState::Success,
        }
    }

    #[test]
    fn behavior_runs_through_the_generic_service_contract() {
        let manifest = repository_worker_manifest();
        let plugin = manifest.id.clone();
        let mut kernel = Kernel::new(KernelConfig::new([manifest]).unwrap());
        kernel
            .register_embedded_factory(plugin.clone(), repository_worker_factory)
            .unwrap();
        kernel.activate_all().unwrap();
        assert_eq!(kernel.state(&plugin), Some(PluginState::Active));

        let snapshot = RepositoryWorkSnapshot {
            pull_requests: vec![RepositoryPullRequestEvidence {
                number: 42,
                semantic_key: "plugin-split".into(),
                queue_order: 42,
                state: RepositoryPullRequestState::Open,
                draft: false,
                head_sha: "head".into(),
                base_sha: "main".into(),
                base_is_current: true,
                dependencies: BTreeSet::new(),
                contract_markdown: "- [ ] implementation".into(),
                checklist_evidence: vec![RepositoryChecklistEvidence {
                    item: "implementation".into(),
                    proven: true,
                }],
                discussions: Vec::new(),
                missing_regression: false,
                missing_spec_or_invariant: false,
                validation: success_validation(),
            }],
            issues: Vec::new(),
        };
        let input = serde_json::to_vec(&PhenixValue::from(&snapshot)).unwrap();
        let output = kernel
            .invoke(
                &repository_work_queue_service(),
                &input,
                &Authority::default(),
                None,
            )
            .unwrap();
        let output: PhenixValue = serde_json::from_slice(&output).unwrap();
        assert_eq!(
            RepositoryWorkQueueView::try_from(Project(&output)).unwrap(),
            RepositoryWorkQueueView::Selected {
                pr_number: 42,
                reason: "next_ready".to_owned(),
            }
        );
    }
}
