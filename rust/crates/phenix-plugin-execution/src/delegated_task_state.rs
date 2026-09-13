use phenix_sdk::{
    DelegatedWorkerResult, DelegatedWorkerTaskRecord, DelegationAdmissionError,
    DelegationResourcePolicy, DelegationTaskBinding, ExecutionAuthority, WorkerTaskRecord,
    WorkerTaskState,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct DelegatedTaskStore {
    tasks: BTreeMap<String, DelegatedWorkerTaskRecord>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DelegatedTaskStoreError {
    DuplicateTask { task_id: String },
    UnknownTask { task_id: String },
    AuthorityExpanded,
    BindingAuthorityMismatch,
    NotRunnable { task_id: String },
    InvalidState { task_id: String },
    ExecutionMismatch { task_id: String },
    Admission(DelegationAdmissionError),
}

impl DelegatedTaskStore {
    pub(crate) fn create(
        &mut self,
        task: WorkerTaskRecord,
        binding: DelegationTaskBinding,
        parent_authority: &ExecutionAuthority,
        policy: &DelegationResourcePolicy,
        now_ms: u64,
    ) -> Result<&DelegatedWorkerTaskRecord, DelegatedTaskStoreError> {
        if self.tasks.contains_key(&task.id) {
            return Err(DelegatedTaskStoreError::DuplicateTask { task_id: task.id });
        }
        if task.delegated_authority != binding.resources.authority {
            return Err(DelegatedTaskStoreError::BindingAuthorityMismatch);
        }
        if !task.delegated_authority.capabilities.is_subset(&parent_authority.capabilities) {
            return Err(DelegatedTaskStoreError::AuthorityExpanded);
        }
        binding.resources.validate_policy(policy).map_err(DelegatedTaskStoreError::Admission)?;
        binding.resources.validate_deadline(now_ms).map_err(DelegatedTaskStoreError::Admission)?;
        let id = task.id.clone();
        self.tasks.insert(
            id.clone(),
            DelegatedWorkerTaskRecord { task, binding, result: None },
        );
        Ok(&self.tasks[&id])
    }

    pub(crate) fn get(&self, task_id: &str) -> Option<&DelegatedWorkerTaskRecord> {
        self.tasks.get(task_id)
    }

    pub(crate) fn child_count(&self, parent_execution: &str) -> usize {
        self.tasks
            .values()
            .filter(|record| record.task.parent_execution == parent_execution)
            .count()
    }

    pub(crate) fn runnable(&self) -> Vec<String> {
        let completed: BTreeSet<&str> = self
            .tasks
            .values()
            .filter_map(|record| match record.task.state {
                WorkerTaskState::Completed { .. } => Some(record.task.id.as_str()),
                _ => None,
            })
            .collect();
        self.tasks
            .values()
            .filter(|record| {
                matches!(record.task.state, WorkerTaskState::Pending)
                    && record
                        .task
                        .depends_on
                        .iter()
                        .all(|dependency| completed.contains(dependency.as_str()))
            })
            .map(|record| record.task.id.clone())
            .collect()
    }

    pub(crate) fn start(
        &mut self,
        task_id: &str,
        execution_id: String,
        now_ms: u64,
    ) -> Result<&DelegatedWorkerTaskRecord, DelegatedTaskStoreError> {
        if !self.runnable().iter().any(|id| id == task_id) {
            return Err(DelegatedTaskStoreError::NotRunnable { task_id: task_id.to_owned() });
        }
        let record = self.tasks.get_mut(task_id).ok_or_else(|| DelegatedTaskStoreError::UnknownTask {
            task_id: task_id.to_owned(),
        })?;
        record.binding.resources.validate_deadline(now_ms).map_err(DelegatedTaskStoreError::Admission)?;
        record.task.state = WorkerTaskState::Running { execution_id };
        Ok(record)
    }

    pub(crate) fn complete(
        &mut self,
        task_id: &str,
        execution_id: &str,
        result: DelegatedWorkerResult,
    ) -> Result<&DelegatedWorkerTaskRecord, DelegatedTaskStoreError> {
        let record = self.tasks.get_mut(task_id).ok_or_else(|| DelegatedTaskStoreError::UnknownTask {
            task_id: task_id.to_owned(),
        })?;
        match &record.task.state {
            WorkerTaskState::Running { execution_id: active } if active == execution_id => {}
            WorkerTaskState::Running { .. } => {
                return Err(DelegatedTaskStoreError::ExecutionMismatch { task_id: task_id.to_owned() });
            }
            _ => return Err(DelegatedTaskStoreError::InvalidState { task_id: task_id.to_owned() }),
        }
        result.validate_against(&record.binding).map_err(DelegatedTaskStoreError::Admission)?;
        let result_refs = result
            .evidence
            .iter()
            .map(|reference| format!("{}@{}", reference.resource_id, reference.revision))
            .collect();
        record.result = Some(result);
        record.task.state = WorkerTaskState::Completed {
            execution_id: execution_id.to_owned(),
            result_refs,
        };
        Ok(record)
    }

    pub(crate) fn fail(
        &mut self,
        task_id: &str,
        execution_id: &str,
        cause: String,
    ) -> Result<&DelegatedWorkerTaskRecord, DelegatedTaskStoreError> {
        let record = self.tasks.get_mut(task_id).ok_or_else(|| DelegatedTaskStoreError::UnknownTask {
            task_id: task_id.to_owned(),
        })?;
        match &record.task.state {
            WorkerTaskState::Running { execution_id: active } if active == execution_id => {
                record.task.state = WorkerTaskState::Failed {
                    execution_id: execution_id.to_owned(),
                    cause,
                };
                Ok(record)
            }
            WorkerTaskState::Running { .. } => Err(DelegatedTaskStoreError::ExecutionMismatch {
                task_id: task_id.to_owned(),
            }),
            _ => Err(DelegatedTaskStoreError::InvalidState { task_id: task_id.to_owned() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{CapabilityGenerationId, ModelId, PluginId};
    use phenix_sdk::{BudgetReservation, DelegatedWorkResources, ModelTarget, RouteDecision, RoutingEstimate};
    use std::collections::BTreeMap;

    fn authority(values: &[&str]) -> ExecutionAuthority {
        ExecutionAuthority::new(values.iter().copied())
    }

    fn binding(authority: ExecutionAuthority) -> DelegationTaskBinding {
        DelegationTaskBinding {
            contract_fingerprint: "sha256:contract".into(),
            parent_policy_revision: "policy-1".into(),
            resources: DelegatedWorkResources {
                target: RouteDecision {
                    target: ModelTarget {
                        provider_plugin: PluginId::parse("provider.fixture").unwrap(),
                        model: ModelId::parse("model.fixture").unwrap(),
                        options: BTreeMap::new(),
                    },
                    capability_generation: CapabilityGenerationId::parse("generation-1").unwrap(),
                    policy_revision: "route-1".into(),
                    candidate_ordinal: 0,
                    estimate: None::<RoutingEstimate>,
                },
                authority,
                context: Vec::new(),
                budget: BudgetReservation {
                    input_tokens: 1_000,
                    output_tokens: 200,
                    cost_microunits: None,
                },
                deadline_at_ms: 10_000,
                depth: 1,
                attempts: 1,
                max_result_bytes: 64 * 1024,
            },
        }
    }

    fn policy() -> DelegationResourcePolicy {
        DelegationResourcePolicy {
            enabled: true,
            max_depth: 2,
            max_children: 2,
            max_attempts: 2,
            max_result_bytes: 64 * 1024,
        }
    }

    #[test]
    fn delegated_authority_cannot_expand_parent_authority() {
        let mut store = DelegatedTaskStore::default();
        let child = authority(&["workspace.read", "workspace.write"]);
        let task = WorkerTaskRecord {
            id: "task-1".into(),
            parent_execution: "root".into(),
            graph_generation: "g1".into(),
            description: "inspect".into(),
            depends_on: BTreeSet::new(),
            delegated_authority: child.clone(),
            state: WorkerTaskState::Pending,
        };
        assert_eq!(
            store.create(task, binding(child), &authority(&["workspace.read"]), &policy(), 0),
            Err(DelegatedTaskStoreError::AuthorityExpanded)
        );
    }
}
