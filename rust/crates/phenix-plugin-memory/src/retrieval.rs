use crate::error::{MemoryError, MemoryResult};
use phenix_sdk::{
    MemoryQueryOrder, MemoryRecallQuery, MemoryRecord, MemorySearchQuery, MemoryStructuredQuery,
    MemoryTimeBounds,
};
use std::collections::BTreeSet;

pub(crate) fn recall(
    records: Vec<MemoryRecord>,
    query: &MemoryRecallQuery,
) -> MemoryResult<Vec<MemoryRecord>> {
    validate_scope_limit("recall", &query.scopes, query.limit)?;
    lexical_search(
        records,
        &query.scopes,
        &query.kinds,
        &query.query,
        query.at,
        None,
        query.limit,
    )
}

pub(crate) fn search(
    records: Vec<MemoryRecord>,
    query: &MemorySearchQuery,
) -> MemoryResult<Vec<MemoryRecord>> {
    validate_scope_limit("search", &query.scopes, query.limit)?;
    validate_time_bounds(&query.time)?;
    lexical_search(
        records,
        &query.scopes,
        &query.kinds,
        &query.query,
        query.time.as_of,
        Some(&query.time),
        query.limit,
    )
}

pub(crate) fn query(
    records: Vec<MemoryRecord>,
    query: &MemoryStructuredQuery,
) -> MemoryResult<Vec<MemoryRecord>> {
    validate_scope_limit("query", &query.scopes, query.limit)?;
    validate_time_bounds(&query.time)?;

    let superseded = superseded_ids(&records, query.time.as_of);
    let mut records = records
        .into_iter()
        .filter(|record| {
            eligible(
                record,
                &query.scopes,
                &query.kinds,
                query.time.as_of,
                &superseded,
            ) && within_created_bounds(record, &query.time)
                && (query.ids.is_empty() || query.ids.contains(&record.id))
                && matches_source(
                    record,
                    query.source_service.as_ref(),
                    query.source_resource.as_deref(),
                )
        })
        .collect::<Vec<_>>();

    records.sort_by(|left, right| match query.order {
        MemoryQueryOrder::NewestFirst => right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.id.cmp(&right.id)),
        MemoryQueryOrder::OldestFirst => left
            .created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id)),
    });
    records.truncate(query.limit as usize);
    Ok(records)
}

fn lexical_search(
    records: Vec<MemoryRecord>,
    scopes: &[phenix_sdk::MemoryScope],
    kinds: &[phenix_sdk::MemoryKind],
    query: &str,
    at: u64,
    time: Option<&MemoryTimeBounds>,
    limit: u32,
) -> MemoryResult<Vec<MemoryRecord>> {
    let superseded = superseded_ids(&records, at);
    let normalized = query.trim().to_lowercase();
    let terms = normalized.split_whitespace().collect::<Vec<_>>();
    let mut candidates = records
        .into_iter()
        .filter(|record| eligible(record, scopes, kinds, at, &superseded))
        .filter(|record| time.is_none_or(|time| within_created_bounds(record, time)))
        .filter_map(|record| {
            recall_score(&record, &normalized, &terms).map(|score| (score, record))
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| left.id.cmp(&right.id))
    });
    candidates.truncate(limit as usize);
    Ok(candidates.into_iter().map(|(_, record)| record).collect())
}

fn validate_scope_limit(
    operation: &str,
    scopes: &[phenix_sdk::MemoryScope],
    limit: u32,
) -> MemoryResult<()> {
    if scopes.is_empty() {
        return Err(MemoryError::Invalid(format!(
            "{operation} requires at least one scope"
        )));
    }
    if !(1..=100).contains(&limit) {
        return Err(MemoryError::Invalid(format!(
            "{operation} limit must be between 1 and 100"
        )));
    }
    Ok(())
}

fn validate_time_bounds(time: &MemoryTimeBounds) -> MemoryResult<()> {
    if let Some(from) = time.created_from
        && let Some(until) = time.created_until
        && from >= until
    {
        return Err(MemoryError::Invalid(
            "created_from must be earlier than created_until".into(),
        ));
    }
    Ok(())
}

fn superseded_ids(records: &[MemoryRecord], at: u64) -> BTreeSet<String> {
    records
        .iter()
        .filter(|record| supersession_effective_at(record, at))
        .flat_map(|record| record.supersedes.iter().cloned())
        .collect()
}

fn eligible(
    record: &MemoryRecord,
    scopes: &[phenix_sdk::MemoryScope],
    kinds: &[phenix_sdk::MemoryKind],
    at: u64,
    superseded: &BTreeSet<String>,
) -> bool {
    scopes.contains(&record.scope)
        && (kinds.is_empty() || kinds.contains(&record.kind))
        && visible_at(record, at)
        && !superseded.contains(&record.id)
}

fn within_created_bounds(record: &MemoryRecord, time: &MemoryTimeBounds) -> bool {
    time.created_from
        .is_none_or(|from| record.created_at >= from)
        && time
            .created_until
            .is_none_or(|until| record.created_at < until)
}

fn matches_source(
    record: &MemoryRecord,
    service: Option<&phenix_core::ServiceId>,
    resource: Option<&str>,
) -> bool {
    if service.is_none() && resource.is_none() {
        return true;
    }
    record.source_refs.iter().any(|source| {
        service.is_none_or(|service| &source.service == service)
            && resource.is_none_or(|resource| source.resource == resource)
    })
}

fn visible_at(record: &MemoryRecord, at: u64) -> bool {
    if record.created_at > at || record.valid_from.is_some_and(|start| start > at) {
        return false;
    }
    record.valid_until.is_none_or(|end| at < end)
}

fn supersession_effective_at(record: &MemoryRecord, at: u64) -> bool {
    record.created_at <= at && record.valid_from.unwrap_or(record.created_at) <= at
}

fn recall_score(record: &MemoryRecord, query: &str, terms: &[&str]) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }
    if record.id.to_lowercase() == query {
        return Some(1_000);
    }
    if record
        .source_refs
        .iter()
        .any(|source| source.resource.to_lowercase() == query)
    {
        return Some(900);
    }
    lexical_score(&record.content, query, terms)
}

fn lexical_score(content: &str, query: &str, terms: &[&str]) -> Option<u32> {
    let content = content.to_lowercase();
    let mut score = 0;
    if content.contains(query) {
        score += 100;
    }
    for term in terms {
        if content.contains(term) {
            score += 10;
        }
    }
    (score > 0).then_some(score)
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{ServiceId, SessionId};
    use phenix_sdk::{MemoryKind, MemoryScope, MemorySourceReference, MemoryTimeBounds};

    fn record(id: &str, content: &str, created_at: u64) -> MemoryRecord {
        MemoryRecord {
            id: id.into(),
            kind: MemoryKind::Fact,
            scope: MemoryScope::Session {
                session_id: SessionId::parse("session-1").unwrap(),
            },
            content: content.into(),
            source_refs: vec![MemorySourceReference {
                service: ServiceId::parse("fixture.history@1").unwrap(),
                resource: format!("turn/{id}"),
                start: None,
                end: None,
            }],
            supporting_dependencies: Vec::new(),
            supersedes: Vec::new(),
            valid_from: None,
            valid_until: None,
            created_at,
        }
    }

    fn time(as_of: u64) -> MemoryTimeBounds {
        MemoryTimeBounds {
            as_of,
            created_from: None,
            created_until: None,
        }
    }

    #[test]
    fn supersession_becomes_effective_at_the_new_record_validity_start() {
        let old = record("old", "use transport A", 10);
        let mut new = record("new", "use transport B", 20);
        new.valid_from = Some(30);
        new.supersedes.push("old".into());
        let scope = old.scope.clone();

        let at_25 = recall(
            vec![old.clone(), new.clone()],
            &MemoryRecallQuery {
                scopes: vec![scope.clone()],
                kinds: Vec::new(),
                query: "transport".into(),
                at: 25,
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(at_25, vec![old]);

        let at_30 = recall(
            vec![new.clone(), record("old", "use transport A", 10)],
            &MemoryRecallQuery {
                scopes: vec![scope],
                kinds: Vec::new(),
                query: "transport".into(),
                at: 30,
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(at_30, vec![new]);
    }

    #[test]
    fn recall_filters_scope_before_lexical_ranking() {
        let allowed = record("allowed", "transport baseline", 10);
        let mut excluded = record("excluded", "transport transport transport", 20);
        excluded.scope = MemoryScope::Session {
            session_id: SessionId::parse("session-2").unwrap(),
        };

        let results = recall(
            vec![excluded, allowed.clone()],
            &MemoryRecallQuery {
                scopes: vec![allowed.scope.clone()],
                kinds: Vec::new(),
                query: "transport".into(),
                at: 30,
                limit: 10,
            },
        )
        .unwrap();

        assert_eq!(results, vec![allowed]);
    }

    #[test]
    fn lexical_recall_works_without_optional_semantic_providers() {
        let database = record("database", "sqlite durable state", 10);
        let unrelated = record("transport", "unix socket transport", 20);

        let results = recall(
            vec![unrelated, database.clone()],
            &MemoryRecallQuery {
                scopes: vec![database.scope.clone()],
                kinds: Vec::new(),
                query: "durable state".into(),
                at: 30,
                limit: 10,
            },
        )
        .unwrap();

        assert_eq!(results, vec![database]);
    }

    #[test]
    fn exact_memory_id_recall_does_not_depend_on_content_terms() {
        let target = record("transport-choice", "Use socket A", 10);
        let other = record("other", "transport-choice appears in prose", 20);

        let results = recall(
            vec![other, target.clone()],
            &MemoryRecallQuery {
                scopes: vec![target.scope.clone()],
                kinds: Vec::new(),
                query: target.id.clone(),
                at: 30,
                limit: 1,
            },
        )
        .unwrap();

        assert_eq!(results, vec![target]);
    }

    #[test]
    fn exact_source_reference_recall_finds_derived_memory() {
        let target = record("transport", "Use socket A", 10);

        let results = recall(
            vec![target.clone()],
            &MemoryRecallQuery {
                scopes: vec![target.scope.clone()],
                kinds: Vec::new(),
                query: "turn/transport".into(),
                at: 30,
                limit: 10,
            },
        )
        .unwrap();

        assert_eq!(results, vec![target]);
    }

    #[test]
    fn structured_query_filters_exact_source_without_semantic_scoring() {
        let target = record("target", "content without selector words", 10);
        let other = record("other", "target target target", 20);
        let results = query(
            vec![other, target.clone()],
            &MemoryStructuredQuery {
                scopes: vec![target.scope.clone()],
                kinds: Vec::new(),
                ids: Vec::new(),
                source_service: Some(ServiceId::parse("fixture.history@1").unwrap()),
                source_resource: Some("turn/target".into()),
                time: time(30),
                order: MemoryQueryOrder::NewestFirst,
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(results, vec![target]);
    }

    #[test]
    fn search_respects_created_time_bounds_before_ranking() {
        let old = record("old", "durable state", 10);
        let recent = record("recent", "durable state", 20);
        let results = search(
            vec![old, recent.clone()],
            &MemorySearchQuery {
                scopes: vec![recent.scope.clone()],
                kinds: Vec::new(),
                query: "durable state".into(),
                time: MemoryTimeBounds {
                    as_of: 30,
                    created_from: Some(15),
                    created_until: Some(25),
                },
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(results, vec![recent]);
    }
}
