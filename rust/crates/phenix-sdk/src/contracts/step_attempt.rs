use super::{AttemptOutcome, ProjectionRevision, RouteDecision, StepPlan, UsageAttribution};
use serde::{Deserialize, Serialize};

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
    EmptyIdentity {
        field: String,
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
        validate_identity("attempt_id", &attribution.attempt_id)?;
        Ok(Self {
            attribution,
            plan,
            phase: StepAttemptPhase::Planned,
            reservation_id: None,
            route: None,
            projection: None,
            dispatch_id: None,
            outcome: None,
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

    pub fn settle(&mut self, outcome: AttemptOutcome) -> Result<(), StepAttemptTransitionError> {
        self.require_phase(StepAttemptPhase::Dispatched)?;
        self.outcome = Some(outcome);
        self.phase = StepAttemptPhase::Settled;
        Ok(())
    }

    fn require_phase(
        &self,
        expected: StepAttemptPhase,
    ) -> Result<(), StepAttemptTransitionError> {
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
