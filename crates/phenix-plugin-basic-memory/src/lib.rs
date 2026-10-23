//! Session-scoped Skills and Context providers over independently selected
//! Storage. These are declarations, not an implicit filesystem or prompt scan.
use phenix_basic_contracts::{error, BasicError, ContextItem, ErrorCode, Skill};
use std::collections::BTreeSet;

const SKILLS_KEY: &str = "basic.skills.v1";
const CONTEXT_KEY: &str = "basic.context.v1";

fn validate<'a>(ids: impl Iterator<Item = &'a str>) -> Result<(), BasicError> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if id.is_empty() || !seen.insert(id) {
            return Err(error(ErrorCode::InvalidInput, "empty or duplicate item id"));
        }
    }
    Ok(())
}

pub fn validate_skills(skills: &[Skill]) -> Result<(), BasicError> {
    validate(skills.iter().map(|skill| skill.id.as_str()))
}
pub fn validate_context(items: &[ContextItem]) -> Result<(), BasicError> {
    validate(items.iter().map(|item| item.id.as_str()))
}

/// Storage records are not trusted merely because their Portable encoding
/// decodes. Reassert the same item invariants enforced by the write path.
fn validate_loaded_items<'a>(
    expected_session: &str,
    stored_session: &str,
    ids: impl Iterator<Item = &'a str>,
) -> Result<(), BasicError> {
    if expected_session != stored_session || validate(ids).is_err() {
        return Err(error(
            ErrorCode::StorageFailed,
            "stored Basic items violate session or identity invariants",
        ));
    }
    Ok(())
}

#[phenix_sdk::plugin(
    id = "phenix.basic.memory",
    version = "1.0.0",
    provide(phenix_basic_contracts::Skills, slot = "Skills"),
    provide(phenix_basic_contracts::Context, slot = "Context"),
    require(phenix_basic_contracts::Storage, slot = "Storage")
)]
mod plugin {
    use super::{
        validate_context, validate_loaded_items, validate_skills, CONTEXT_KEY, SKILLS_KEY,
    };
    use phenix_basic_contracts::{
        decode, encode, error, BasicError, Context, ContextItem, ContextSet, ErrorCode, RecordKey,
        Scope, Skill, SkillSet, Skills, Storage, StorageEdit, StorageTransaction,
    };
    use phenix_sdk::{CallError, CallResult, Code, Failure, Host, Portable};

    #[derive(Default)]
    pub struct State;

    fn load<T: Portable>(
        host: &Host<'_>,
        session: &str,
        key: &str,
    ) -> CallResult<Option<(u64, T)>, BasicError> {
        match host.invoke::<Storage::read>(
            "Storage",
            &RecordKey {
                session: session.into(),
                key: key.into(),
            },
        ) {
            Ok(record) => {
                let value = decode(&record.data).map_err(|_| {
                    CallError::Domain(error(ErrorCode::StorageFailed, "corrupt item record"))
                })?;
                Ok(Some((record.revision, value)))
            }
            Err(CallError::Domain(err)) if err.code == ErrorCode::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn save<T: Portable>(
        host: &Host<'_>,
        session: &str,
        key: &str,
        value: &T,
    ) -> CallResult<(), BasicError> {
        // The wire contract has no expected revision for callers. Read then
        // CAS once. Competing writers receive Conflict instead of lost updates.
        let expected = host.invoke::<Storage::read>(
            "Storage",
            &RecordKey {
                session: session.into(),
                key: key.into(),
            },
        );
        let revision = match expected {
            Ok(record) => record.revision,
            Err(CallError::Domain(err)) if err.code == ErrorCode::NotFound => 0,
            Err(err) => return Err(err),
        };
        let data = encode(value)?;
        let result = host.invoke::<Storage::transact>(
            "Storage",
            &StorageTransaction {
                session: session.into(),
                edits: vec![StorageEdit {
                    key: key.into(),
                    expected: revision,
                    data: data.clone(),
                }],
            },
        )?;
        if result.len() != 1 || !result[0].matches_commit(revision, &data) {
            return Err(CallError::Kernel(Failure::new(
                Code::PluginFailed,
                "storage returned invalid memory commit",
            )));
        }
        Ok(())
    }

    #[provide(Skills::list)]
    fn skills_list(
        _: &mut State,
        host: &Host<'_>,
        request: Scope,
    ) -> CallResult<Vec<Skill>, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        match load::<SkillSet>(host, &request.session, SKILLS_KEY)? {
            Some((_, value)) => {
                validate_loaded_items(
                    &request.session,
                    &value.session,
                    value.skills.iter().map(|item| item.id.as_str()),
                )
                .map_err(CallError::Domain)?;
                Ok(value.skills)
            }
            None => Ok(vec![]),
        }
    }

    #[provide(Skills::set)]
    fn skills_set(_: &mut State, host: &Host<'_>, request: SkillSet) -> CallResult<(), BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        validate_skills(&request.skills).map_err(CallError::Domain)?;
        save(host, &request.session, SKILLS_KEY, &request)
    }

    #[provide(Context::list)]
    fn context_list(
        _: &mut State,
        host: &Host<'_>,
        request: Scope,
    ) -> CallResult<Vec<ContextItem>, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        match load::<ContextSet>(host, &request.session, CONTEXT_KEY)? {
            Some((_, value)) => {
                validate_loaded_items(
                    &request.session,
                    &value.session,
                    value.items.iter().map(|item| item.id.as_str()),
                )
                .map_err(CallError::Domain)?;
                Ok(value.items)
            }
            None => Ok(vec![]),
        }
    }

    #[provide(Context::set)]
    fn context_set(
        _: &mut State,
        host: &Host<'_>,
        request: ContextSet,
    ) -> CallResult<(), BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        validate_context(&request.items).map_err(CallError::Domain)?;
        save(host, &request.session, CONTEXT_KEY, &request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loaded_items_revalidate_session_and_item_identity() {
        assert!(validate_loaded_items("s1", "s1", ["skill-a", "skill-b"].into_iter()).is_ok());
        assert_eq!(
            validate_loaded_items("s1", "s2", ["skill-a"].into_iter())
                .unwrap_err()
                .code,
            ErrorCode::StorageFailed
        );
        for ids in [vec!["skill-a", "skill-a"], vec!["", "skill-b"]] {
            assert_eq!(
                validate_loaded_items("s1", "s1", ids.into_iter())
                    .unwrap_err()
                    .code,
                ErrorCode::StorageFailed
            );
        }
        assert!(validate_loaded_items("s1", "s1", std::iter::empty()).is_ok());
    }

    #[test]
    fn unique_ids_are_required_before_storage_effects() {
        assert_eq!(
            validate_skills(&[Skill {
                id: "".into(),
                description: "".into(),
                instructions: "".into(),
            }])
            .unwrap_err()
            .code,
            ErrorCode::InvalidInput
        );
        assert_eq!(
            validate_context(&[
                ContextItem {
                    id: "dup".into(),
                    text: "a".into()
                },
                ContextItem {
                    id: "dup".into(),
                    text: "b".into()
                },
            ])
            .unwrap_err()
            .code,
            ErrorCode::InvalidInput
        );
    }
}
