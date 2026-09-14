use phenix_application_interface::types::{
    ReviewDecision, ReviewDecisionInput, ReviewFile, ReviewHunk, ReviewRecord, ReviewState,
};
use phenix_core::{
    ComponentInterface, DurableSchema, InterfaceId, InterfaceSchema, PhenixValue, PluginContext,
    PluginHost, PluginInstance, Project, ResourceNamespace, ServiceId, TransactionOp, ValueCodec,
};
use phenix_sdk::{
    WorkspaceCommand, WorkspaceFileVersion, WorkspaceInterface, WorkspaceResponse, WorkspaceWrite,
    WORKSPACE_SERVICE,
};
use phenix_sdk_macros::PhenixValue as DerivePhenixValue;
use std::collections::{BTreeMap, BTreeSet};

pub const EXECUTION_REVIEW_SERVICE: &str = "phenix.execution.review@1";
const REVIEW_NAMESPACE: &str = "phenix.execution.review.state";
const STATE_KEY: &str = "reviews";

#[derive(Clone, Debug, PartialEq, DerivePhenixValue)]
pub struct PreparedReviewFile {
    pub uri: String,
    pub path: String,
    pub expected_version: WorkspaceFileVersion,
    pub content: String,
    pub hunks: Vec<ReviewHunk>,
}

#[derive(Clone, Debug, PartialEq, DerivePhenixValue)]
pub enum ExecutionReviewCommand {
    Prepare {
        id: String,
        session_id: phenix_core::SessionId,
        execution_id: String,
        files: Vec<PreparedReviewFile>,
    },
    Get {
        review_id: String,
    },
    ListPending,
    Decide {
        input: ReviewDecisionInput,
    },
}

#[derive(Clone, Debug, PartialEq, DerivePhenixValue)]
pub enum ExecutionReviewResponse {
    Review {
        review: ReviewRecord,
    },
    ReviewLookup {
        review: Option<ReviewRecord>,
    },
    Reviews {
        reviews: Vec<ReviewRecord>,
    },
    Conflict {
        message: String,
        review: Option<ReviewRecord>,
    },
}

pub struct ExecutionReviewInterface;

impl ComponentInterface for ExecutionReviewInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(EXECUTION_REVIEW_SERVICE)
            .expect("static execution review interface id is valid")
    }

    fn schema() -> InterfaceSchema {
        InterfaceSchema::of::<ExecutionReviewCommand, ExecutionReviewResponse>()
    }
}

#[derive(Clone, Debug, PartialEq, DerivePhenixValue)]
struct StoredReview {
    record: ReviewRecord,
    prepared_files: Vec<PreparedReviewFile>,
}

#[derive(Clone, Debug, Default, PartialEq, DerivePhenixValue)]
struct ReviewProjection {
    reviews: BTreeMap<String, StoredReview>,
}

type ReviewContext<'host, 'runtime> = PluginContext<'host, 'runtime, ()>;

fn context<'host, 'runtime>(host: &'host PluginHost<'runtime>) -> ReviewContext<'host, 'runtime> {
    PluginContext::new(host, (), (), ())
}

#[must_use]
pub fn execution_review_service() -> ServiceId {
    ServiceId::parse(EXECUTION_REVIEW_SERVICE).expect("static execution review service id is valid")
}

fn workspace_service() -> ServiceId {
    ServiceId::parse(WORKSPACE_SERVICE).expect("static workspace service id is valid")
}

pub(crate) fn execution_review_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(REVIEW_NAMESPACE).expect("static execution review namespace is valid")
}

pub(crate) fn execution_review_factory() -> Box<dyn PluginInstance> {
    Box::new(ExecutionReviewPlugin)
}

struct ExecutionReviewPlugin;

impl PluginInstance for ExecutionReviewPlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        context(host)
            .kernel
            .register_durable_schema(&DurableSchema::new(execution_review_namespace(), 1))
            .map_err(|error| error.to_string())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &execution_review_service() {
            return Err(format!("unsupported execution review service: {service}"));
        }
        let context = context(host);
        let command = context
            .kernel
            .decode_projected::<ExecutionReviewCommand>(
                &ExecutionReviewInterface::interface_id(),
                input,
            )
            .map_err(|error| error.to_string())?;
        let response = handle(&context, command)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn handle(
    context: &ReviewContext<'_, '_>,
    command: ExecutionReviewCommand,
) -> Result<ExecutionReviewResponse, String> {
    match command {
        ExecutionReviewCommand::Prepare {
            id,
            session_id,
            execution_id,
            files,
        } => prepare(context, id, session_id, execution_id, files),
        ExecutionReviewCommand::Get { review_id } => {
            let (_, state) = read_state(context)?;
            Ok(ExecutionReviewResponse::ReviewLookup {
                review: state
                    .reviews
                    .get(&review_id)
                    .map(|stored| stored.record.clone()),
            })
        }
        ExecutionReviewCommand::ListPending => {
            let (_, state) = read_state(context)?;
            let reviews = state
                .reviews
                .values()
                .filter(|stored| stored.record.state == ReviewState::Pending)
                .map(|stored| stored.record.clone())
                .collect();
            Ok(ExecutionReviewResponse::Reviews { reviews })
        }
        ExecutionReviewCommand::Decide { input } => decide(context, input),
    }
}

fn prepare(
    context: &ReviewContext<'_, '_>,
    id: String,
    session_id: phenix_core::SessionId,
    execution_id: String,
    files: Vec<PreparedReviewFile>,
) -> Result<ExecutionReviewResponse, String> {
    validate_identity("review id", &id)?;
    validate_identity("review execution id", &execution_id)?;
    if files.is_empty() {
        return Err("review must contain at least one prepared file".into());
    }

    let mut paths = BTreeSet::new();
    let mut uris = BTreeSet::new();
    let mut review_files = Vec::with_capacity(files.len());
    for file in &files {
        validate_identity("review file uri", &file.uri)?;
        validate_identity("review file path", &file.path)?;
        if !paths.insert(file.path.clone()) {
            return Err(format!(
                "review contains duplicate workspace path: {}",
                file.path
            ));
        }
        if !uris.insert(file.uri.clone()) {
            return Err(format!("review contains duplicate file uri: {}", file.uri));
        }
        let mut hunk_ids = BTreeSet::new();
        for hunk in &file.hunks {
            validate_identity("review hunk id", &hunk.id)?;
            if !hunk_ids.insert(hunk.id.clone()) {
                return Err(format!(
                    "review file {} contains duplicate hunk id: {}",
                    file.uri, hunk.id
                ));
            }
        }
        review_files.push(ReviewFile {
            uri: file.uri.clone(),
            expected_version: version_label(&file.expected_version),
            hunks: file.hunks.clone(),
            conflict: None,
        });
    }

    let record = ReviewRecord {
        id: id.clone(),
        revision: 0,
        session_id,
        execution_id,
        files: review_files,
        state: ReviewState::Pending,
    };
    let stored = StoredReview {
        record: record.clone(),
        prepared_files: files,
    };

    let (old, mut state) = read_state(context)?;
    if let Some(existing) = state.reviews.get(&id) {
        if existing == &stored {
            return Ok(ExecutionReviewResponse::Review {
                review: existing.record.clone(),
            });
        }
        return Ok(ExecutionReviewResponse::Conflict {
            message: format!("review identity is immutable: {id}"),
            review: Some(existing.record.clone()),
        });
    }
    state.reviews.insert(id, stored);
    persist_state(context, old, &state)?;
    Ok(ExecutionReviewResponse::Review { review: record })
}

fn decide(
    context: &ReviewContext<'_, '_>,
    input: ReviewDecisionInput,
) -> Result<ExecutionReviewResponse, String> {
    let (old, mut state) = read_state(context)?;
    let Some(stored) = state.reviews.get(&input.review_id).cloned() else {
        return Ok(ExecutionReviewResponse::Conflict {
            message: format!("unknown review: {}", input.review_id),
            review: None,
        });
    };
    if stored.record.revision != input.expected_revision {
        return Ok(ExecutionReviewResponse::Conflict {
            message: format!(
                "stale review revision for {}: expected {}, current {}",
                input.review_id, input.expected_revision, stored.record.revision
            ),
            review: Some(stored.record),
        });
    }
    if stored.record.state != ReviewState::Pending {
        return Ok(ExecutionReviewResponse::Conflict {
            message: format!("review is not pending: {}", input.review_id),
            review: Some(stored.record),
        });
    }

    match input.decision {
        ReviewDecision::Reject => {
            let current = state
                .reviews
                .get_mut(&input.review_id)
                .expect("review exists above");
            current.record.revision = current
                .record
                .revision
                .checked_add(1)
                .ok_or_else(|| "review revision overflow".to_owned())?;
            current.record.state = ReviewState::Rejected;
            current.prepared_files.clear();
            let review = current.record.clone();
            persist_state(context, old, &state)?;
            Ok(ExecutionReviewResponse::Review { review })
        }
        ReviewDecision::Accept => accept(context, old, state, input.review_id, stored),
    }
}

fn accept(
    context: &ReviewContext<'_, '_>,
    old: Option<Vec<u8>>,
    mut state: ReviewProjection,
    review_id: String,
    stored: StoredReview,
) -> Result<ExecutionReviewResponse, String> {
    let command = WorkspaceCommand::WriteBatch {
        writes: stored
            .prepared_files
            .iter()
            .map(|file| WorkspaceWrite {
                path: file.path.clone(),
                content: file.content.clone(),
                expected_version: file.expected_version.clone(),
            })
            .collect(),
    };
    let input = context
        .kernel
        .encode_value(&command)
        .map_err(|error| error.to_string())?;
    let output = context
        .kernel
        .invoke_service_abi(&workspace_service(), &input, context.call.authority, None)
        .map_err(|error| error.to_string())?;
    let response = context
        .kernel
        .decode_projected::<WorkspaceResponse>(&WorkspaceInterface::interface_id(), &output)
        .map_err(|error| error.to_string())?;

    let current = state
        .reviews
        .get_mut(&review_id)
        .expect("review exists before workspace invocation");
    current.record.revision = current
        .record
        .revision
        .checked_add(1)
        .ok_or_else(|| "review revision overflow".to_owned())?;

    match response {
        WorkspaceResponse::WrittenBatch { .. } => {
            current.record.state = ReviewState::Accepted;
            current.prepared_files.clear();
        }
        WorkspaceResponse::VersionConflict { conflicts } => {
            for conflict in &conflicts {
                if let Some((index, _)) = stored
                    .prepared_files
                    .iter()
                    .enumerate()
                    .find(|(_, file)| file.path == conflict.path)
                {
                    if let Some(file) = current.record.files.get_mut(index) {
                        file.conflict = Some(format!(
                            "expected {}, observed {}",
                            version_label(&conflict.expected_version),
                            version_label(&conflict.observed_version)
                        ));
                    }
                }
            }
            let message = if conflicts.len() == 1 {
                format!("workspace version conflict for {}", conflicts[0].path)
            } else {
                format!("workspace version conflicts for {} files", conflicts.len())
            };
            current.record.state = ReviewState::Conflicted { message };
            current.prepared_files.clear();
        }
        other => {
            return Err(format!(
                "workspace returned an invalid response to review apply: {other:?}"
            ));
        }
    }

    let review = current.record.clone();
    persist_state(context, old, &state)?;
    Ok(ExecutionReviewResponse::Review { review })
}

fn read_state(
    context: &ReviewContext<'_, '_>,
) -> Result<(Option<Vec<u8>>, ReviewProjection), String> {
    let old = context
        .kernel
        .read_durable(&execution_review_namespace(), STATE_KEY)
        .map_err(|error| error.to_string())?;
    let state = old
        .as_deref()
        .map(|bytes| {
            let value: PhenixValue =
                serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
            ReviewProjection::from_value(&value).map_err(|error| error.to_string())
        })
        .transpose()?
        .unwrap_or_default();
    Ok((old, state))
}

fn persist_state(
    context: &ReviewContext<'_, '_>,
    old: Option<Vec<u8>>,
    state: &ReviewProjection,
) -> Result<(), String> {
    context
        .kernel
        .transact_durable(
            &execution_review_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: STATE_KEY.into(),
                    expected: old,
                },
                TransactionOp::Put {
                    key: STATE_KEY.into(),
                    value: serde_json::to_vec(&state.to_value())
                        .map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())
}

fn version_label(version: &WorkspaceFileVersion) -> String {
    match version {
        WorkspaceFileVersion::Absent => "absent".to_owned(),
        WorkspaceFileVersion::Present { content_hash } => content_hash.clone(),
    }
}

fn validate_identity(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        Authority, CapabilityId, Kernel, KernelConfig, LocalPersistence, PluginExecution, PluginId,
        PluginManifest, ServiceContribution,
    };
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn capability(value: &str) -> CapabilityId {
        CapabilityId::parse(value).unwrap()
    }

    fn authority() -> Authority {
        Authority::new([
            capability("kernel.persistence.schema"),
            capability("kernel.persistence.read"),
            capability("kernel.persistence.write"),
            capability("workspace.write"),
        ])
    }

    fn temp_path(name: &str, suffix: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "phenix-{name}-{}-{nonce}{suffix}",
            std::process::id()
        ))
    }

    fn review_manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId::parse("fixture.execution-review").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                role: phenix_core::ServiceRole::Terminal,
                service: execution_review_service(),
                priority: 100,
                required_authority: Authority::default(),
            }],
            resource_namespaces: vec![execution_review_namespace()],
            maximum_authority: authority(),
        }
    }

    fn kernel(db: &PathBuf, root: &PathBuf) -> Kernel {
        let review = review_manifest();
        let review_id = review.id.clone();
        let workspace = phenix_plugin_workspace::workspace_manifest();
        let workspace_id = workspace.id.clone();
        let persistence = LocalPersistence::open(db).unwrap();
        let mut kernel =
            Kernel::with_persistence(KernelConfig::new([review, workspace]).unwrap(), persistence);
        kernel
            .register_embedded_factory(review_id, execution_review_factory)
            .unwrap();
        let root = root.clone();
        kernel
            .register_embedded_factory(workspace_id, move || {
                phenix_plugin_workspace::workspace_factory_for(root.clone())
            })
            .unwrap();
        kernel.activate_all().unwrap();
        kernel
    }

    fn invoke(kernel: &mut Kernel, command: ExecutionReviewCommand) -> ExecutionReviewResponse {
        let input = PhenixValue::from(&command);
        let output = kernel
            .invoke(
                &execution_review_service(),
                &serde_json::to_vec(&input).unwrap(),
                &authority(),
                None,
            )
            .unwrap();
        let output: PhenixValue = serde_json::from_slice(&output).unwrap();
        ExecutionReviewResponse::try_from(Project(&output)).unwrap()
    }

    fn prepared(expected_version: WorkspaceFileVersion, content: &str) -> PreparedReviewFile {
        PreparedReviewFile {
            uri: "file:///workspace/a.txt".into(),
            path: "a.txt".into(),
            expected_version,
            content: content.into(),
            hunks: vec![ReviewHunk {
                id: "hunk-1".into(),
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 1,
                unified_diff: "@@ -1 +1 @@\n-old\n+new".into(),
            }],
        }
    }

    fn prepare(kernel: &mut Kernel, file: PreparedReviewFile) -> ReviewRecord {
        match invoke(
            kernel,
            ExecutionReviewCommand::Prepare {
                id: "review-1".into(),
                session_id: phenix_core::SessionId::parse("session-1").unwrap(),
                execution_id: "execution-1".into(),
                files: vec![file],
            },
        ) {
            ExecutionReviewResponse::Review { review } => review,
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    fn accepted_review_applies_exact_version_and_restores_durably() {
        let root = temp_path("review-accept", "");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.txt"), "old").unwrap();
        let db = temp_path("review-accept", ".sqlite");
        let version = WorkspaceFileVersion::Present {
            content_hash: format!("{:x}", Sha256::digest(b"old")),
        };
        {
            let mut kernel = kernel(&db, &root);
            let review = prepare(&mut kernel, prepared(version, "new"));
            assert_eq!(review.state, ReviewState::Pending);
            let response = invoke(
                &mut kernel,
                ExecutionReviewCommand::Decide {
                    input: ReviewDecisionInput {
                        review_id: review.id.clone(),
                        expected_revision: review.revision,
                        decision: ReviewDecision::Accept,
                    },
                },
            );
            assert!(matches!(
                response,
                ExecutionReviewResponse::Review {
                    review: ReviewRecord {
                        state: ReviewState::Accepted,
                        revision: 1,
                        ..
                    }
                }
            ));
            assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "new");
        }
        {
            let mut kernel = kernel(&db, &root);
            let response = invoke(
                &mut kernel,
                ExecutionReviewCommand::Get {
                    review_id: "review-1".into(),
                },
            );
            assert!(matches!(
                response,
                ExecutionReviewResponse::ReviewLookup {
                    review: Some(ReviewRecord {
                        state: ReviewState::Accepted,
                        revision: 1,
                        ..
                    })
                }
            ));
        }
        let _ = fs::remove_file(db);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_conflict_is_terminal_and_never_mutates_file() {
        let root = temp_path("review-conflict", "");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.txt"), "current").unwrap();
        let db = temp_path("review-conflict", ".sqlite");
        let mut kernel = kernel(&db, &root);
        let review = prepare(
            &mut kernel,
            prepared(
                WorkspaceFileVersion::Present {
                    content_hash: "stale".into(),
                },
                "new",
            ),
        );
        let response = invoke(
            &mut kernel,
            ExecutionReviewCommand::Decide {
                input: ReviewDecisionInput {
                    review_id: review.id,
                    expected_revision: 0,
                    decision: ReviewDecision::Accept,
                },
            },
        );
        assert!(matches!(
            response,
            ExecutionReviewResponse::Review {
                review: ReviewRecord {
                    state: ReviewState::Conflicted { .. },
                    revision: 1,
                    ..
                }
            }
        ));
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "current");
        let _ = fs::remove_file(db);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reject_discards_prepared_mutation_and_stale_actions_conflict() {
        let root = temp_path("review-reject", "");
        fs::create_dir_all(&root).unwrap();
        let db = temp_path("review-reject", ".sqlite");
        let mut kernel = kernel(&db, &root);
        let review = prepare(&mut kernel, prepared(WorkspaceFileVersion::Absent, "new"));
        let rejected = invoke(
            &mut kernel,
            ExecutionReviewCommand::Decide {
                input: ReviewDecisionInput {
                    review_id: review.id.clone(),
                    expected_revision: 0,
                    decision: ReviewDecision::Reject,
                },
            },
        );
        assert!(matches!(
            rejected,
            ExecutionReviewResponse::Review {
                review: ReviewRecord {
                    state: ReviewState::Rejected,
                    revision: 1,
                    ..
                }
            }
        ));
        assert!(!root.join("a.txt").exists());
        let stale = invoke(
            &mut kernel,
            ExecutionReviewCommand::Decide {
                input: ReviewDecisionInput {
                    review_id: review.id,
                    expected_revision: 0,
                    decision: ReviewDecision::Accept,
                },
            },
        );
        assert!(matches!(stale, ExecutionReviewResponse::Conflict { .. }));
        let _ = fs::remove_file(db);
        let _ = fs::remove_dir_all(root);
    }
}
