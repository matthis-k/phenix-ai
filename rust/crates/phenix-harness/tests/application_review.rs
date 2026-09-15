use phenix_application_interface::{
    types::{
        ReviewDecision, ReviewDecisionInput, ReviewHunk, ReviewRecord, ReviewState, SessionChange,
        SessionCreateInput, SessionInfo,
    },
    CreateSession, DecideReview, Operation,
};
use phenix_core::{ContractId, PhenixValue, Project, ValueCodec};
use phenix_harness::{application::ApplicationWorker, default_suite_authority, PhenixHarness};
use phenix_plugin_catalog::{
    execution_review_service, ExecutionReviewCommand, ExecutionReviewResponse, PreparedReviewFile,
    WorkspaceFileVersion,
};

fn invoke_application<O: Operation>(worker: &mut ApplicationWorker, input: O::Input) -> O::Output {
    let output = worker
        .invoke(&ContractId::parse(O::ID).unwrap(), input.to_value())
        .unwrap();
    O::Output::from_value(&output).unwrap()
}

#[test]
fn review_decision_uses_execution_truth_and_journals_the_returned_record() {
    let mut harness = PhenixHarness::default_suite().unwrap();
    harness.activate().unwrap();

    let prepare = ExecutionReviewCommand::Prepare {
        id: "review-1".into(),
        session_id: phenix_core::SessionId::parse("session-1").unwrap(),
        execution_id: "execution-1".into(),
        files: vec![PreparedReviewFile {
            uri: "file:///workspace/a.txt".into(),
            path: "a.txt".into(),
            expected_version: WorkspaceFileVersion::Absent,
            content: "new\n".into(),
            hunks: vec![ReviewHunk {
                id: "hunk-1".into(),
                old_start: 0,
                old_count: 0,
                new_start: 1,
                new_count: 1,
                unified_diff: "@@ -0,0 +1 @@\n+new".into(),
            }],
        }],
    };
    let prepare_output = harness
        .invoke(
            &execution_review_service(),
            &serde_json::to_vec(&PhenixValue::from(&prepare)).unwrap(),
            &default_suite_authority(),
            None,
        )
        .unwrap();
    let prepare_output: PhenixValue = serde_json::from_slice(&prepare_output).unwrap();
    assert!(matches!(
        ExecutionReviewResponse::try_from(Project(&prepare_output)).unwrap(),
        ExecutionReviewResponse::Review {
            review: ReviewRecord {
                state: ReviewState::Pending,
                revision: 0,
                ..
            }
        }
    ));

    let mut worker = ApplicationWorker::new(harness).unwrap();
    let created: SessionInfo = invoke_application::<CreateSession>(
        &mut worker,
        SessionCreateInput {
            working_directory: "/workspace".into(),
            title: None,
        },
    );
    assert_eq!(created.session_id.as_str(), "session-1");

    let decided: ReviewRecord = invoke_application::<DecideReview>(
        &mut worker,
        ReviewDecisionInput {
            review_id: "review-1".into(),
            expected_revision: 0,
            decision: ReviewDecision::Reject,
        },
    );
    assert_eq!(decided.revision, 1);
    assert_eq!(decided.state, ReviewState::Rejected);

    let projection = &worker.projection().state().sessions["session-1"];
    assert_eq!(projection.through_sequence, 1);
    assert!(matches!(
        &projection.updates[0].update,
        SessionChange::Review { review }
            if review.id == "review-1"
                && review.revision == 1
                && review.state == ReviewState::Rejected
    ));
}
