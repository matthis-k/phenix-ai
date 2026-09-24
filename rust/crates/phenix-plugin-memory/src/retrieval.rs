use crate::error::{MemoryError, MemoryResult};
use phenix_sdk::{MemoryRecallQuery, MemoryRecord};
use std::collections::BTreeSet;

pub(crate) fn recall(
    records: Vec<MemoryRecord>,
    query: &MemoryRecallQuery,
) -> MemoryResult<Vec<MemoryRecord>> {
    if query.scopes.is_empty() {
        return Err(MemoryError::Invalid(
            "recall requires at least one scope".into(),
        ));
    }
    if !(1..=100).contains(&query.limit) {
        return Err(MemoryError::Invalid(
            "recall limit must be between 1 and 100".into(),
        ));
    }

    let superseded = records
        .iter()
        .filter(|record| supersession_effective_at(record, query.at))
        .flat_map(|record| record.supersedes.iter().cloned())
        .collect::<BTreeSet<_>>();
    let eligible_records = records
        .into_iter()
        .filter(|record| eligible(record, query, &superseded))
        .collect::<Vec<_>>();

    let search = LexicalCandidateSearch;
    let mut candidates = search
        .search(&eligible_records, query)
        .into_iter()
        .filter_map(|hit| {
            let record = eligible_records.get(hit.index)?;
            eligible(record, query, &superseded).then(|| (hit.score, record.clone()))
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| left.id.cmp(&right.id))
    });
    candidates.truncate(query.limit as usize);
    Ok(candidates.into_iter().map(|(_, record)| record).collect())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SearchHit {
    index: usize,
    score: u32,
}

trait CandidateSearch {
    fn search(&self, records: &[MemoryRecord], query: &MemoryRecallQuery) -> Vec<SearchHit>;
}

struct LexicalCandidateSearch;

impl CandidateSearch for LexicalCandidateSearch {
    fn search(&self, records: &[MemoryRecord], query: &MemoryRecallQuery) -> Vec<SearchHit> {
        let normalized = query.query.trim().to_lowercase();
        let terms = normalized.split_whitespace().collect::<Vec<_>>();
        records
            .iter()
            .enumerate()
            .filter_map(|(index, record)| {
                recall_score(record, &normalized, &terms).map(|score| SearchHit { index, score })
            })
            .collect()
    }
}

fn eligible(
    record: &MemoryRecord,
    query: &MemoryRecallQuery,
    superseded: &BTreeSet<String>,
) -> bool {
    query.scopes.contains(&record.scope)
        && (query.kinds.is_empty() || query.kinds.contains(&record.kind))
        && visible_at(record, query.at)
        && !superseded.contains(&record.id)
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
    use phenix_sdk::{MemoryKind, MemoryScope, MemorySourceReference};

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
            supersedes: Vec::new(),
            valid_from: None,
            valid_until: None,
            created_at,
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
}
