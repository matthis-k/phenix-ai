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
        Scope, Skill, SkillSet, Skills, Storage,
    };
    use phenix_sdk::{CallError, CallResult, Host, Portable};
    use std::collections::BTreeMap;

    /// Per-instance authoritative values. Storage restores and durably saves
    /// selected session settings; it is not the default read path.
    #[derive(Default)]
    pub struct State {
        skills: BTreeMap<String, Cached<SkillSet>>,
        context: BTreeMap<String, Cached<ContextSet>>,
    }

    struct Cached<T> {
        revision: u64,
        value: T,
    }

    impl State {
        fn skills(
            &mut self,
            host: &Host<'_>,
            session: &str,
        ) -> CallResult<&Cached<SkillSet>, BasicError> {
            if !self.skills.contains_key(session) {
                let snapshot = match load::<SkillSet>(host, session, SKILLS_KEY)? {
                    Some((revision, value)) => {
                        validate_loaded_items(
                            session,
                            &value.session,
                            value.skills.iter().map(|item| item.id.as_str()),
                        )
                        .map_err(CallError::Domain)?;
                        Cached { revision, value }
                    }
                    None => Cached {
                        revision: 0,
                        value: SkillSet {
                            session: session.into(),
                            skills: vec![],
                        },
                    },
                };
                self.skills.insert(session.into(), snapshot);
            }
            Ok(&self.skills[session])
        }

        fn context(
            &mut self,
            host: &Host<'_>,
            session: &str,
        ) -> CallResult<&Cached<ContextSet>, BasicError> {
            if !self.context.contains_key(session) {
                let snapshot = match load::<ContextSet>(host, session, CONTEXT_KEY)? {
                    Some((revision, value)) => {
                        validate_loaded_items(
                            session,
                            &value.session,
                            value.items.iter().map(|item| item.id.as_str()),
                        )
                        .map_err(CallError::Domain)?;
                        Cached { revision, value }
                    }
                    None => Cached {
                        revision: 0,
                        value: ContextSet {
                            session: session.into(),
                            items: vec![],
                        },
                    },
                };
                self.context.insert(session.into(), snapshot);
            }
            Ok(&self.context[session])
        }
    }

    #[cfg(test)]
    mod state_tests {
        use super::*;

        #[test]
        fn cached_session_data_has_separate_skills_and_context_revisions() {
            let mut state = State::default();
            state.skills.insert(
                "one".into(),
                Cached {
                    revision: 3,
                    value: SkillSet {
                        session: "one".into(),
                        skills: vec![],
                    },
                },
            );
            state.context.insert(
                "one".into(),
                Cached {
                    revision: 7,
                    value: ContextSet {
                        session: "one".into(),
                        items: vec![],
                    },
                },
            );
            state.skills.insert(
                "two".into(),
                Cached {
                    revision: 2,
                    value: SkillSet {
                        session: "two".into(),
                        skills: vec![],
                    },
                },
            );
            assert_eq!(state.skills["one"].revision, 3);
            assert_eq!(state.context["one"].revision, 7);
            assert_eq!(state.skills["two"].revision, 2);
            state.skills.remove("one");
            assert!(!state.skills.contains_key("one"));
            assert!(state.context.contains_key("one"));
            assert!(state.skills.contains_key("two"));
        }
    }

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
        expected: u64,
        value: &T,
    ) -> CallResult<u64, BasicError> {
        // The cached revision is the CAS precondition. Conflicting writers
        // cannot overwrite each other; no uncommitted state is published.
        let next = expected
            .checked_add(1)
            .ok_or_else(|| CallError::Domain(error(ErrorCode::Conflict, "revision overflow")))?;
        let data = encode(value)?;
        phenix_basic_contracts::transact_one(host, "Storage", session, key, expected, data)?;
        Ok(next)
    }

    #[provide(Skills::list)]
    fn skills_list(
        state: &mut State,
        host: &Host<'_>,
        request: Scope,
    ) -> CallResult<Vec<Skill>, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        Ok(state.skills(host, &request.session)?.value.skills.clone())
    }

    #[provide(Skills::set)]
    fn skills_set(
        state: &mut State,
        host: &Host<'_>,
        request: SkillSet,
    ) -> CallResult<(), BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        validate_skills(&request.skills).map_err(CallError::Domain)?;
        let revision = state.skills(host, &request.session)?.revision;
        match save(host, &request.session, SKILLS_KEY, revision, &request) {
            Ok(next) => {
                state.skills.insert(
                    request.session.clone(),
                    Cached {
                        revision: next,
                        value: request,
                    },
                );
                Ok(())
            }
            Err(error) => {
                // On conflict or ambiguous acknowledgment, reload at next use.
                state.skills.remove(&request.session);
                Err(error)
            }
        }
    }

    #[provide(Context::list)]
    fn context_list(
        state: &mut State,
        host: &Host<'_>,
        request: Scope,
    ) -> CallResult<Vec<ContextItem>, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        Ok(state.context(host, &request.session)?.value.items.clone())
    }

    #[provide(Context::set)]
    fn context_set(
        state: &mut State,
        host: &Host<'_>,
        request: ContextSet,
    ) -> CallResult<(), BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        validate_context(&request.items).map_err(CallError::Domain)?;
        let revision = state.context(host, &request.session)?.revision;
        match save(host, &request.session, CONTEXT_KEY, revision, &request) {
            Ok(next) => {
                state.context.insert(
                    request.session.clone(),
                    Cached {
                        revision: next,
                        value: request,
                    },
                );
                Ok(())
            }
            Err(error) => {
                state.context.remove(&request.session);
                Err(error)
            }
        }
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
