//! Durable workspace selection for one authorized session.
//!
//! Selection never grants file access and never changes process cwd. Workspace
//! file and process providers independently authorize each external effect.
use phenix_workspace_contracts::{WorkspaceError, WorkspaceProblem};
use std::path::{Component, Path};

const KEY: &str = "workspace.selection.v1";

fn error(problem: WorkspaceProblem, message: &str) -> WorkspaceError {
    WorkspaceError {
        problem,
        message: message.into(),
    }
}

fn validate_cwd(cwd: Option<&str>) -> Result<(), WorkspaceError> {
    let Some(value) = cwd else {
        return Ok(());
    };
    if value.is_empty()
        || value.len() > 4096
        || value.chars().any(char::is_control)
        || !Path::new(value).is_absolute()
        || value.split('/').any(|part| part == "." || part == "..")
        || Path::new(value)
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(error(
            WorkspaceProblem::InvalidInput,
            "workspace cwd must be an absolute path without traversal",
        ));
    }
    Ok(())
}

fn storage_failure(
    failure: phenix_sdk::CallError<phenix_basic_contracts::BasicError>,
) -> phenix_sdk::CallError<WorkspaceError> {
    match failure {
        phenix_sdk::CallError::Kernel(failure) => phenix_sdk::CallError::Kernel(failure),
        phenix_sdk::CallError::Domain(problem) => {
            let kind = match problem.code {
                phenix_basic_contracts::ErrorCode::Conflict => WorkspaceProblem::Conflict,
                phenix_basic_contracts::ErrorCode::Denied => WorkspaceProblem::Denied,
                _ => WorkspaceProblem::Unavailable,
            };
            phenix_sdk::CallError::Domain(error(kind, "workspace storage operation failed"))
        }
    }
}

#[phenix_sdk::plugin(
    id = "phenix.workspace.selection",
    version = "1.0.0",
    provide(phenix_workspace_contracts::Workspace, slot = "Workspace"),
    require(phenix_basic_contracts::Storage, slot = "Storage")
)]
mod plugin {
    use super::{error, storage_failure, validate_cwd, KEY};
    use phenix_basic_contracts::{
        decode, encode, ErrorCode, RecordKey, Storage, StorageEdit, StorageTransaction,
    };
    use phenix_sdk::{CallError, CallResult, Host};
    use phenix_workspace_contracts::{
        Workspace, WorkspaceError, WorkspaceProblem, WorkspaceQuery, WorkspaceSnapshot,
        WorkspaceUpdate,
    };
    use std::collections::BTreeMap;

    // Workspace selection is plugin-owned. Persistent snapshots restore and
    // reconcile it across instances; they are not a replacement for State.
    #[derive(Default)]
    pub struct State {
        selections: BTreeMap<String, WorkspaceSnapshot>,
    }

    impl State {
        fn apply_persisted(&mut self, snapshot: WorkspaceSnapshot) -> WorkspaceSnapshot {
            self.selections
                .insert(snapshot.session.clone(), snapshot.clone());
            snapshot
        }
    }

    fn check(host: &Host<'_>, session: &str) -> CallResult<(), WorkspaceError> {
        phenix_basic_contracts::check_session(host, session).map_err(|err| match err {
            CallError::Kernel(failure) => CallError::Kernel(failure),
            CallError::Domain(_) => CallError::Domain(error(
                WorkspaceProblem::Denied,
                "workspace session is not authorized",
            )),
        })
    }

    fn load(host: &Host<'_>, session: &str) -> CallResult<WorkspaceSnapshot, WorkspaceError> {
        let request = RecordKey {
            session: session.into(),
            key: KEY.into(),
        };
        match host.invoke::<Storage::read>("Storage", &request) {
            Ok(record) => {
                let snapshot: WorkspaceSnapshot = decode(&record.data).map_err(|_| {
                    CallError::Domain(error(
                        WorkspaceProblem::Unavailable,
                        "invalid stored workspace selection",
                    ))
                })?;
                if snapshot.session != session || snapshot.revision != record.revision {
                    return Err(CallError::Domain(error(
                        WorkspaceProblem::Unavailable,
                        "stored workspace revision mismatch",
                    )));
                }
                validate_cwd(snapshot.cwd.as_deref()).map_err(CallError::Domain)?;
                Ok(snapshot)
            }
            Err(CallError::Domain(problem)) if problem.code == ErrorCode::NotFound => {
                Ok(WorkspaceSnapshot {
                    session: session.into(),
                    cwd: None,
                    revision: 0,
                })
            }
            Err(other) => Err(storage_failure(other)),
        }
    }

    #[provide(Workspace::get)]
    fn get(
        state: &mut State,
        host: &Host<'_>,
        request: WorkspaceQuery,
    ) -> CallResult<WorkspaceSnapshot, WorkspaceError> {
        check(host, &request.session)?;
        // Refresh before exposing a cached selection. Another authorized
        // instance may have committed a newer durable revision.
        let snapshot = load(host, &request.session)?;
        Ok(state.apply_persisted(snapshot))
    }

    #[provide(Workspace::set)]
    fn set(
        state: &mut State,
        host: &Host<'_>,
        request: WorkspaceUpdate,
    ) -> CallResult<WorkspaceSnapshot, WorkspaceError> {
        check(host, &request.session)?;
        validate_cwd(request.cwd.as_deref()).map_err(CallError::Domain)?;
        let mut current = load(host, &request.session)?;
        if current.revision != request.expected_revision {
            return Err(CallError::Domain(error(
                WorkspaceProblem::Conflict,
                "stale workspace selection revision",
            )));
        }
        current.revision = current.revision.checked_add(1).ok_or_else(|| {
            CallError::Domain(error(
                WorkspaceProblem::Conflict,
                "workspace revision overflow",
            ))
        })?;
        current.cwd = request.cwd;
        let blob = encode(&current)?;
        let records = host
            .invoke::<Storage::transact>(
                "Storage",
                &StorageTransaction {
                    session: request.session,
                    edits: vec![StorageEdit {
                        key: KEY.into(),
                        expected: request.expected_revision,
                        data: blob,
                    }],
                },
            )
            .map_err(storage_failure)?;
        if records.len() != 1 || records[0].revision != current.revision {
            return Err(CallError::Domain(error(
                WorkspaceProblem::Unavailable,
                "invalid workspace transaction response",
            )));
        }
        // Do not mutate live state until durable CAS and reply validation
        // succeed. Failed writes leave the previous selection untouched.
        Ok(state.apply_persisted(current))
    }
    #[cfg(test)]
    mod state_tests {
        use super::*;

        #[test]
        fn plugin_state_owns_restored_session_selections() {
            let mut state = State::default();
            let one = WorkspaceSnapshot {
                session: "one".into(),
                cwd: Some("/work/one".into()),
                revision: 1,
            };
            let two = WorkspaceSnapshot {
                session: "two".into(),
                cwd: None,
                revision: 2,
            };
            assert_eq!(state.apply_persisted(one.clone()), one);
            state.apply_persisted(two.clone());
            assert_eq!(state.selections.get("one"), Some(&one));
            assert_eq!(state.selections.get("two"), Some(&two));
            let changed = WorkspaceSnapshot {
                session: "one".into(),
                cwd: Some("/work/new".into()),
                revision: 3,
            };
            state.apply_persisted(changed.clone());
            assert_eq!(state.selections.get("one"), Some(&changed));
            assert_eq!(state.selections.get("two"), Some(&two));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_relative_parent_or_control_character_paths() {
        for invalid in [
            "",
            "relative",
            "../escape",
            "/tmp/../escape",
            "/tmp/./here",
            "/tmp/\nnew",
        ] {
            assert_eq!(
                validate_cwd(Some(invalid)).unwrap_err().problem,
                WorkspaceProblem::InvalidInput
            );
        }
        for valid in ["/", "/work/project", "/tmp/a space/repo"] {
            validate_cwd(Some(valid)).unwrap();
        }
        validate_cwd(None).unwrap();
    }
}
