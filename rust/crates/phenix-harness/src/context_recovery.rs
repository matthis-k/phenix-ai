use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceTarget {
    pub workspace_id: String,
    pub canonical_root: PathBuf,
    pub canonical_repository: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBindingState {
    Bootstrap,
    Bound,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidatePreparationMode {
    DiscoveryOnly,
    NormalServicesSuspended,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PreparedWorkspaceCandidate {
    pub target: WorkspaceTarget,
    pub request_id: String,
    pub preparation_mode: CandidatePreparationMode,
    pub workspace_identity_validated: bool,
    pub repository_identity_validated: bool,
    pub live_query_validated: bool,
}

impl PreparedWorkspaceCandidate {
    #[must_use]
    pub fn is_usable(&self) -> bool {
        self.workspace_identity_validated
            && self.repository_identity_validated
            && self.live_query_validated
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootAdmissionReceiptState {
    Prepared,
    Committed,
    Aborted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RootAdmissionReceipt {
    pub receipt_id: String,
    pub request_id: String,
    pub target: WorkspaceTarget,
    pub execution_id: String,
    pub state: RootAdmissionReceiptState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RootAdmissionReceiptError {
    AlreadyCommitted,
    AlreadyAborted,
    TargetChanged,
    RequestChanged,
    ExecutionChanged,
}

impl RootAdmissionReceipt {
    pub fn commit(&mut self) -> Result<(), RootAdmissionReceiptError> {
        match self.state {
            RootAdmissionReceiptState::Prepared => {
                self.state = RootAdmissionReceiptState::Committed;
                Ok(())
            }
            RootAdmissionReceiptState::Committed => Err(RootAdmissionReceiptError::AlreadyCommitted),
            RootAdmissionReceiptState::Aborted => Err(RootAdmissionReceiptError::AlreadyAborted),
        }
    }

    pub fn abort(&mut self) -> Result<(), RootAdmissionReceiptError> {
        match self.state {
            RootAdmissionReceiptState::Prepared => {
                self.state = RootAdmissionReceiptState::Aborted;
                Ok(())
            }
            RootAdmissionReceiptState::Committed => Err(RootAdmissionReceiptError::AlreadyCommitted),
            RootAdmissionReceiptState::Aborted => Err(RootAdmissionReceiptError::AlreadyAborted),
        }
    }

    pub fn reconcile(&self, observed: &RootAdmissionReceipt) -> Result<(), RootAdmissionReceiptError> {
        if self.request_id != observed.request_id {
            return Err(RootAdmissionReceiptError::RequestChanged);
        }
        if self.target != observed.target {
            return Err(RootAdmissionReceiptError::TargetChanged);
        }
        if self.execution_id != observed.execution_id {
            return Err(RootAdmissionReceiptError::ExecutionChanged);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ContextRecoveryOutcome {
    Current,
    Selected { target: WorkspaceTarget },
    Ambiguous { candidates: Vec<WorkspaceTarget> },
    NotFound,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeRecoveryError {
    RuntimeAlreadyBound,
    CandidateNotUsable,
    RequestMismatch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryRuntimeManagerState {
    pub binding: RuntimeBindingState,
    pub prepared_receipt: Option<RootAdmissionReceipt>,
}

impl Default for RecoveryRuntimeManagerState {
    fn default() -> Self {
        Self {
            binding: RuntimeBindingState::Bootstrap,
            prepared_receipt: None,
        }
    }
}

impl RecoveryRuntimeManagerState {
    pub fn prepare_root_admission(
        &mut self,
        candidate: &PreparedWorkspaceCandidate,
        receipt_id: String,
        execution_id: String,
    ) -> Result<RootAdmissionReceipt, RuntimeRecoveryError> {
        if self.binding != RuntimeBindingState::Bootstrap {
            return Err(RuntimeRecoveryError::RuntimeAlreadyBound);
        }
        if !candidate.is_usable() {
            return Err(RuntimeRecoveryError::CandidateNotUsable);
        }
        if let Some(existing) = &self.prepared_receipt {
            if existing.request_id != candidate.request_id {
                return Err(RuntimeRecoveryError::RequestMismatch);
            }
            return Ok(existing.clone());
        }

        let receipt = RootAdmissionReceipt {
            receipt_id,
            request_id: candidate.request_id.clone(),
            target: candidate.target.clone(),
            execution_id,
            state: RootAdmissionReceiptState::Prepared,
        };
        self.prepared_receipt = Some(receipt.clone());
        Ok(receipt)
    }

    pub fn mark_bound(&mut self, receipt: &RootAdmissionReceipt) -> Result<(), RuntimeRecoveryError> {
        let Some(expected) = &self.prepared_receipt else {
            return Err(RuntimeRecoveryError::RequestMismatch);
        };
        if expected.request_id != receipt.request_id
            || expected.target != receipt.target
            || expected.execution_id != receipt.execution_id
        {
            return Err(RuntimeRecoveryError::RequestMismatch);
        }
        if receipt.state != RootAdmissionReceiptState::Committed {
            return Err(RuntimeRecoveryError::RequestMismatch);
        }
        self.binding = RuntimeBindingState::Bound;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: &str) -> WorkspaceTarget {
        WorkspaceTarget {
            workspace_id: id.into(),
            canonical_root: PathBuf::from(format!("/work/{id}")),
            canonical_repository: Some(format!("github.com/example/{id}")),
        }
    }

    fn candidate() -> PreparedWorkspaceCandidate {
        PreparedWorkspaceCandidate {
            target: target("phenix"),
            request_id: "request-1".into(),
            preparation_mode: CandidatePreparationMode::DiscoveryOnly,
            workspace_identity_validated: true,
            repository_identity_validated: true,
            live_query_validated: true,
        }
    }

    #[test]
    fn workspace_replacement_is_bootstrap_only() {
        let mut manager = RecoveryRuntimeManagerState {
            binding: RuntimeBindingState::Bound,
            prepared_receipt: None,
        };
        assert_eq!(
            manager.prepare_root_admission(&candidate(), "receipt-1".into(), "execution-1".into()),
            Err(RuntimeRecoveryError::RuntimeAlreadyBound)
        );
    }

    #[test]
    fn repeated_prepare_for_same_request_reuses_pinned_target() {
        let mut manager = RecoveryRuntimeManagerState::default();
        let first = manager
            .prepare_root_admission(&candidate(), "receipt-1".into(), "execution-1".into())
            .unwrap();
        let mut changed = candidate();
        changed.target = target("other");
        let second = manager
            .prepare_root_admission(&changed, "receipt-2".into(), "execution-2".into())
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(second.target.workspace_id, "phenix");
    }

    #[test]
    fn committed_receipt_is_required_before_binding() {
        let mut manager = RecoveryRuntimeManagerState::default();
        let mut receipt = manager
            .prepare_root_admission(&candidate(), "receipt-1".into(), "execution-1".into())
            .unwrap();
        assert!(manager.mark_bound(&receipt).is_err());
        receipt.commit().unwrap();
        manager.mark_bound(&receipt).unwrap();
        assert_eq!(manager.binding, RuntimeBindingState::Bound);
    }
}
