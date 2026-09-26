use super::{
    AttemptOutcome, AttemptUsageRecord, BudgetActual, ProjectionRevision, ReacquisitionUsage,
    RouteDecision, StepPlan, UsageAttemptKind, UsageAttribution,
};
use phenix_core::{ComponentInterface, InterfaceId, ServiceId};
use serde::{Deserialize, Serialize};

pub const STEP_ATTEMPT_SERVICE: &str = "phenix.execution.attempts@1";

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum StepAttemptPhase {
    Planned,
    Reserved,
    Routed,
    ContextAdmitted,
    Dispatched,
    Settled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct StepAttemptRecord {
    pub attribution: UsageAttribution,
    pub plan: StepPlan,
    pub phase: StepAttemptPhase,
    pub reservation_id: Option<String>,
    pub route: Option<RouteDecision>,
    pub projection: Option<ProjectionRevision>,
    pub dispatch_id: Option<String>,
    pub outcome: Option<AttemptOutcome>,
    #[serde(default)]
    pub reacquisition: Vec<ReacquisitionUsage>,
    #[serde(default)]
    pub settled_actual: Option<BudgetActual>,
    #[serde(default)]
    pub usage: Option<AttemptUsageRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum StepAttemptTransitionError {
    PolicyRevisionMismatch {
        attribution: String,
        plan: String,
    },
    InvalidPhase {
        expected: StepAttemptPhase,
        actual: StepAttemptPhase,
    },
    InvalidAbortPhase {
        actual: StepAttemptPhase,
    },
    InvalidAbortOutcome {
        outcome: AttemptOutcome,
    },
    EmptyIdentity {
        field: String,
    },
    UsageAttributionMismatch,
    UsageOutcomeMismatch {
        expected: AttemptOutcome,
        observed: AttemptOutcome,
    },
    InvalidReacquisitionPhase {
        actual: StepAttemptPhase,
    },
    ReacquisitionIdentityConflict {
        reacquisition_id: String,
    },
}

impl StepAttemptRecord {
    pub fn new(
        attribution: UsageAttribution,
        plan: StepPlan,
    ) -> Result<Self, StepAttemptTransitionError> {
        if attribution.policy_revision != plan.policy_revision {
            return Err(StepAttemptTransitionError::PolicyRevisionMismatch {
                attribution: attribution.policy_revision,
                plan: plan.policy_revision,
            });
        }
        validate_identity("root_execution_id", &attribution.root_execution_id)?;
        validate_identity("execution_id", &attribution.execution_id)?;
        validate_identity("attempt_id", &attribution.attempt_id)?;
        if let Some(parent) = &attribution.parent_attempt_id {
            validate_identity("parent_attempt_id", parent)?;
        }
        Ok(Self {
            attribution,
            plan,
            phase: StepAttemptPhase::Planned,
            reservation_id: None,
            route: None,
            projection: None,
            dispatch_id: None,
            outcome: None,
            reacquisition: Vec::new(),
            settled_actual: None,
            usage: None,
        })
    }

    pub fn bind_reservation(
        &mut self,
        reservation_id: String,
    ) -> Result<(), StepAttemptTransitionError> {
        self.require_phase(StepAttemptPhase::Planned)?;
        validate_identity("reservation_id", &reservation_id)?;
        self.reservation_id = Some(reservation_id);
        self.phase = StepAttemptPhase::Reserved;
        Ok(())
    }

    pub fn bind_route(
        &mut self,
        decision: RouteDecision,
    ) -> Result<(), StepAttemptTransitionError> {
        self.require_phase(StepAttemptPhase::Reserved)?;
        self.route = Some(decision);
        self.phase = StepAttemptPhase::Routed;
        Ok(())
    }

    pub fn bind_projection(
        &mut self,
        projection: ProjectionRevision,
    ) -> Result<(), StepAttemptTransitionError> {
        self.require_phase(StepAttemptPhase::Routed)?;
        self.projection = Some(projection);
        self.phase = StepAttemptPhase::ContextAdmitted;
        Ok(())
    }

    pub fn mark_dispatched(
        &mut self,
        dispatch_id: String,
    ) -> Result<(), StepAttemptTransitionError> {
        self.require_phase(StepAttemptPhase::ContextAdmitted)?;
        validate_identity("dispatch_id", &dispatch_id)?;
        self.dispatch_id = Some(dispatch_id);
        self.phase = StepAttemptPhase::Dispatched;
        Ok(())
    }

    pub fn abort(&mut self, outcome: AttemptOutcome) -> Result<(), StepAttemptTransitionError> {
        if matches!(
            self.phase,
            StepAttemptPhase::Dispatched | StepAttemptPhase::Settled
        ) {
            return Err(StepAttemptTransitionError::InvalidAbortPhase { actual: self.phase });
        }
        if outcome == AttemptOutcome::Succeeded {
            return Err(StepAttemptTransitionError::InvalidAbortOutcome { outcome });
        }
        self.outcome = Some(outcome);
        self.phase = StepAttemptPhase::Settled;
        Ok(())
    }

    pub fn settle(&mut self, outcome: AttemptOutcome) -> Result<(), StepAttemptTransitionError> {
        self.require_phase(StepAttemptPhase::Dispatched)?;
        self.outcome = Some(outcome);
        self.phase = StepAttemptPhase::Settled;
        Ok(())
    }

    pub fn settle_with_usage(
        &mut self,
        outcome: AttemptOutcome,
        actual: BudgetActual,
        usage: AttemptUsageRecord,
    ) -> Result<(), StepAttemptTransitionError> {
        self.require_phase(StepAttemptPhase::Dispatched)?;
        if usage.attribution != self.attribution {
            return Err(StepAttemptTransitionError::UsageAttributionMismatch);
        }
        if usage.outcome != outcome {
            return Err(StepAttemptTransitionError::UsageOutcomeMismatch {
                expected: outcome,
                observed: usage.outcome,
            });
        }
        self.outcome = Some(outcome);
        self.settled_actual = Some(actual);
        self.usage = Some(usage);
        self.phase = StepAttemptPhase::Settled;
        Ok(())
    }

    pub fn record_reacquisition(
        &mut self,
        usage: ReacquisitionUsage,
    ) -> Result<(), StepAttemptTransitionError> {
        if self.phase != StepAttemptPhase::Settled {
            return Err(StepAttemptTransitionError::InvalidReacquisitionPhase {
                actual: self.phase,
            });
        }
        validate_identity("reacquisition_id", &usage.reacquisition_id)?;
        validate_identity("reacquisition_cause_identity", &usage.cause_identity)?;
        if let Some(source_attempt_id) = &usage.source_attempt_id {
            validate_identity("reacquisition_source_attempt_id", source_attempt_id)?;
        }
        if let Some(existing) = self
            .reacquisition
            .iter()
            .find(|existing| existing.reacquisition_id == usage.reacquisition_id)
        {
            if existing == &usage {
                return Ok(());
            }
            return Err(StepAttemptTransitionError::ReacquisitionIdentityConflict {
                reacquisition_id: usage.reacquisition_id,
            });
        }
        self.reacquisition.push(usage);
        Ok(())
    }

    fn require_phase(&self, expected: StepAttemptPhase) -> Result<(), StepAttemptTransitionError> {
        if self.phase == expected {
            Ok(())
        } else {
            Err(StepAttemptTransitionError::InvalidPhase {
                expected,
                actual: self.phase,
            })
        }
    }
}

fn validate_identity(field: &str, value: &str) -> Result<(), StepAttemptTransitionError> {
    if value.trim().is_empty() {
        Err(StepAttemptTransitionError::EmptyIdentity {
            field: field.to_owned(),
        })
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum StepAttemptCommand {
    AllocateIdentity {
        root_execution_id: String,
        execution_id: String,
        parent_attempt_id: Option<String>,
        policy_revision: String,
        kind: UsageAttemptKind,
    },
    AllocateDelegatedIdentity {
        root_execution_id: String,
        execution_id: String,
        parent_attempt_id: String,
        policy_revision: String,
        task_id: String,
    },
    Create {
        attribution: UsageAttribution,
        plan: StepPlan,
    },
    Get {
        attempt_id: String,
    },
    ListRoot {
        root_execution_id: String,
    },
    BindReservation {
        attempt_id: String,
        reservation_id: String,
    },
    BindRoute {
        attempt_id: String,
        decision: RouteDecision,
    },
    BindProjection {
        attempt_id: String,
        projection: ProjectionRevision,
    },
    MarkDispatched {
        attempt_id: String,
        dispatch_id: String,
    },
    Abort {
        attempt_id: String,
        outcome: AttemptOutcome,
    },
    Settle {
        attempt_id: String,
        outcome: AttemptOutcome,
    },
    RecordReacquisition {
        attempt_id: String,
        usage: ReacquisitionUsage,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum StepAttemptResponse {
    Attribution { attribution: UsageAttribution },
    Attempt { attempt: StepAttemptRecord },
    AttemptLookup { attempt: Option<StepAttemptRecord> },
    Attempts { attempts: Vec<StepAttemptRecord> },
}

pub struct StepAttemptInterface;

impl ComponentInterface for StepAttemptInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(STEP_ATTEMPT_SERVICE).expect("static step attempt interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<StepAttemptCommand, StepAttemptResponse>()
    }
}

#[must_use]
pub fn step_attempt_service() -> ServiceId {
    ServiceId::parse(STEP_ATTEMPT_SERVICE).expect("static step attempt service id is valid")
}
