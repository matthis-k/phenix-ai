use super::ExplorationOpportunity;
use phenix_core::{ComponentInterface, InterfaceId, InterfaceSchema};
use phenix_sdk_macros::PhenixValue;
use serde::{Deserialize, Serialize};

pub const PLANNING_SERVICE: &str = "phenix.planning@1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct ObjectiveRecord {
    pub id: String,
    pub title: String,
    pub parent: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct PlanStep {
    pub id: String,
    pub description: String,
    pub dependencies: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct PlanRecord {
    pub id: String,
    pub objective_id: String,
    pub goal: String,
    pub steps: Vec<PlanStep>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ExplorationCandidate {
    pub task_id: String,
    pub description: String,
    pub separable: bool,
    pub requires_parent_transcript: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ExplorationCostEstimate {
    pub parent_input_tokens_if_inline: u64,
    pub inline_parent_reacquisition_tokens: u64,
    pub delegated_parent_reacquisition_tokens: u64,
    pub child_input_tokens: u64,
    pub child_output_tokens: u64,
    pub child_cost_microunits: Option<u64>,
    pub expected_result_input_tokens: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct DecisionRecord {
    pub id: String,
    pub objective_id: String,
    pub statement: String,
    pub rationale: String,
    pub dependencies: Vec<String>,
    pub supersedes: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(rename_all = "snake_case")]
pub enum HistoryKind {
    Objective,
    Plan,
    Decision,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct HistoryEntry {
    pub kind: HistoryKind,
    pub id: String,
    pub objective_id: String,
    pub summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum PlanningCommand {
    CreateObjective {
        id: String,
        title: String,
        parent: Option<String>,
    },
    CreatePlan {
        id: String,
        objective_id: String,
        goal: String,
        steps: Vec<PlanStep>,
    },
    PrepareExplorationOpportunity {
        candidate: ExplorationCandidate,
        estimate: ExplorationCostEstimate,
    },
    RecordDecision {
        id: String,
        objective_id: String,
        statement: String,
        rationale: String,
        dependencies: Vec<String>,
        supersedes: Option<String>,
    },
    GetObjective {
        id: String,
    },
    GetPlan {
        id: String,
    },
    GetDecision {
        id: String,
    },
    SearchHistory {
        objective_id: Option<String>,
        query: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum PlanningResponse {
    Objective { objective: Option<ObjectiveRecord> },
    Plan { plan: Option<PlanRecord> },
    ExplorationOpportunity { opportunity: ExplorationOpportunity },
    Decision { decision: Option<DecisionRecord> },
    History { entries: Vec<HistoryEntry> },
}

pub struct PlanningInterface;

impl ComponentInterface for PlanningInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(PLANNING_SERVICE).expect("static planning interface id is valid")
    }

    fn schema() -> InterfaceSchema {
        InterfaceSchema::of::<PlanningCommand, PlanningResponse>()
    }
}

impl crate::InterfaceMarker for PlanningInterface {
    fn interface_id() -> InterfaceId {
        <Self as ComponentInterface>::interface_id()
    }
}
