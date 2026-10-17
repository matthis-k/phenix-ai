//! P2-3aj: selected, opt-in M1 state-change preparation node.
//! The Basic session is the only authoritative source of ModelRequest.
//! The proposal is data, never authorization to invoke Model or write CAS.
use phenix_advanced_contracts::{
    model_request_sha256, validate_model_preparation_input, validate_model_preparation_proposal,
    ModelPreparationError, ModelPreparationInput, ModelPreparationProblem,
    ModelPreparationProposal,
};
use phenix_sdk::{FailurePolicy, JoinPolicy, Portable, StaticGraphContribution, StaticGraphNode};

pub const PENDING_SOURCE: &str = "phenix.basic.agent/pending_model_request";

#[derive(Debug, Clone, Portable)]
pub struct ChangeNotice {
    pub epoch: u64,
    pub properties: Vec<String>,
}

#[phenix_sdk::contract(id = "phenix.advanced.graph-prepare@1")]
pub trait GraphPrepare {
    fn run(&mut self, change: ChangeNotice) -> Result<String, ModelPreparationError>;
}

pub fn passthrough(
    input: ModelPreparationInput,
) -> Result<ModelPreparationProposal, ModelPreparationError> {
    validate_model_preparation_input(&input).map_err(|problem| preparation_error(problem))?;
    let proposal = ModelPreparationProposal {
        original_sha256: input.original_sha256.clone(),
        candidate: input.original.clone(),
        checkpoint: None,
    };
    validate_model_preparation_proposal(&input, &proposal)
        .map_err(|problem| preparation_error(problem))?;
    Ok(proposal)
}

fn preparation_error(problem: ModelPreparationProblem) -> ModelPreparationError {
    ModelPreparationError {
        problem,
        message: format!("source-bound graph preparation rejected: {problem:?}"),
    }
}

const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "phenix.advanced.graph-preparer",
    events: &[],
    actions: &[],
    nodes: &[StaticGraphNode {
        id: "prepare",
        triggers: &[PENDING_SOURCE_TRIGGER],
        after: &["phenix.basic.agent/prepare"],
        selector: "",
        operation: "phenix.advanced.graph-prepare@1/run",
        writes: &["phenix.advanced.graph-preparer/proposal"],
        join: JoinPolicy::AllActive,
        failure: FailurePolicy::FailRoot,
    }],
};
const PENDING_SOURCE_TRIGGER: &str = "state:phenix.basic.agent/pending_model_request";

#[phenix_sdk::plugin(
    id = "phenix.advanced.graph-preparer",
    version = "1.0.0",
    define(GraphPrepare),
    provide(GraphPrepare, slot = "Graph"),
    provide(phenix_advanced_contracts::ModelPreparation, slot = "Prepare"),
    require(phenix_basic_contracts::BasicPendingModelRequest, slot = "Pending"),
    graph(super::GRAPH)
)]
mod provider {
    use super::*;
    use phenix_advanced_contracts::{
        ModelPreparation, ModelPreparationInput, ModelPreparationProposal,
    };
    use phenix_basic_contracts::BasicPendingModelRequest;
    use phenix_sdk::{CallError, CallResult, Code, Failure, Host};

    #[derive(Default)]
    pub struct State {
        #[get]
        #[set]
        #[watch]
        proposal: Option<ModelPreparationProposal>,
    }

    fn authorize(
        host: &Host<'_>,
        input: &ModelPreparationInput,
    ) -> CallResult<(), ModelPreparationError> {
        let expected =
            phenix_basic_contracts::session_resource(&input.original.session).map_err(|_| {
                CallError::Domain(preparation_error(ModelPreparationProblem::ScopeDenied))
            })?;
        if host.resource()?.as_deref() != Some(expected.as_str()) {
            return Err(CallError::Domain(preparation_error(
                ModelPreparationProblem::ScopeDenied,
            )));
        }
        Ok(())
    }

    #[provide(ModelPreparation::prepare)]
    fn prepare(
        host: &Host<'_>,
        input: ModelPreparationInput,
    ) -> CallResult<ModelPreparationProposal, ModelPreparationError> {
        authorize(host, &input)?;
        passthrough(input).map_err(CallError::Domain)
    }

    #[provide(GraphPrepare::run)]
    fn on_prepared(
        state: &mut State,
        host: &Host<'_>,
        change: ChangeNotice,
    ) -> CallResult<String, ModelPreparationError> {
        if change.epoch == 0 || change.properties != [PENDING_SOURCE] {
            return Err(CallError::Domain(preparation_error(
                ModelPreparationProblem::SourceChanged,
            )));
        }
        let location = BasicPendingModelRequest::get::locate(host, "Pending")?;
        let snapshot = location.read(host).map_err(|err| {
            Failure::new(
                Code::PluginFailed,
                format!("Basic request state read failed: {err}"),
            )
        })?;
        let original = snapshot
            .value
            .as_ref()
            .as_ref()
            .ok_or_else(|| {
                CallError::Domain(preparation_error(ModelPreparationProblem::SourceChanged))
            })?
            .clone();
        let original_sha256 = model_request_sha256(&original).map_err(CallError::Kernel)?;
        let input = ModelPreparationInput {
            original,
            original_sha256,
        };
        let proposal = prepare(host, input)?;
        let mut edit = state.proposal.edit(host).map_err(|err| {
            Failure::new(
                Code::PluginFailed,
                format!("Advanced proposal edit failed: {err}"),
            )
        })?;
        *edit = Some(proposal.clone());
        edit.commit(host).map_err(|err| {
            Failure::new(
                Code::PluginFailed,
                format!("Advanced proposal commit failed: {err}"),
            )
        })?;
        if let Some(path) = std::env::var_os("PHENIX_P2_PREPARER_WITNESS") {
            std::fs::write(path, proposal.original_sha256.as_bytes()).map_err(|err| {
                Failure::new(
                    Code::PluginFailed,
                    format!("Advanced test witness failed: {err}"),
                )
            })?;
        }
        Ok(proposal.original_sha256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_basic_contracts::{Limits, Message, ModelRequest, Role};

    fn request() -> ModelPreparationInput {
        let original = ModelRequest {
            session: "s1".into(),
            run: "run-1".into(),
            history: vec![Message {
                role: Role::User,
                text: "one".into(),
                calls: vec![],
                call_id: String::new(),
                error: None,
            }],
            tools: vec![],
            skills: vec![],
            context: vec![],
            limits: Limits::default(),
        };
        let original_sha256 = model_request_sha256(&original).unwrap();
        ModelPreparationInput {
            original,
            original_sha256,
        }
    }

    #[test]
    fn source_bound_no_op_never_widens_permissions() {
        let input = request();
        let proposal = passthrough(input.clone()).unwrap();
        assert_eq!(proposal.original_sha256, input.original_sha256);
        assert_eq!(proposal.candidate, input.original);
        assert!(proposal.checkpoint.is_none());
    }

    #[test]
    fn forged_source_is_rejected() {
        let mut input = request();
        input.original.history[0].text.push('!');
        assert_eq!(
            passthrough(input).unwrap_err().problem,
            ModelPreparationProblem::SourceChanged
        );
    }

    #[test]
    fn graph_only_watches_the_authoritative_basic_projection() {
        let graph = GRAPH.owned().unwrap();
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(
            graph.nodes[0].triggers,
            ["state:phenix.basic.agent/pending_model_request"]
        );
        assert_eq!(
            graph.nodes[0].writes,
            ["phenix.advanced.graph-preparer/proposal"]
        );
        assert_eq!(
            graph.nodes[0].operation,
            "phenix.advanced.graph-prepare@1/run"
        );
        assert_eq!(graph.nodes[0].after, ["phenix.basic.agent/prepare"]);
    }
}
