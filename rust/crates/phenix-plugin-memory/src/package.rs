use crate::{
    context_service_state::{MemoryContextServiceState, MEMORY_CONTEXT_STATE_KEY},
    implementation,
};
use phenix_core::{
    Authority, ComponentInterface, PluginContext, PluginHost, PluginInstance, PluginManifest,
    ServiceContribution, ServiceId, TransactionOp,
};
use phenix_sdk::{
    memory_context_service, memory_service, CandidateCompleteness, ContextAnchor, ContextNeed,
    MemoryAssociationObservation, MemoryCommand, MemoryContextCandidate, MemoryContextCommand,
    MemoryContextInterface, MemoryContextMatch, MemoryContextRecallRequest, MemoryContextResponse,
    MemoryFreshness, MemoryInterface, MemoryRecallQuery, MemoryRecord, MemoryResponse,
};
use std::collections::{BTreeMap, BTreeSet};

#[must_use]
pub fn memory_manifest() -> PluginManifest {
    let mut manifest = implementation::memory_manifest();
    manifest.services.push(ServiceContribution {
        role: phenix_core::ServiceRole::Terminal,
        service: memory_context_service(),
        priority: 100,
        required_authority: Authority::default(),
    });
    manifest
}

#[must_use]
pub fn memory_factory() -> Box<dyn PluginInstance> {
    Box::new(MemoryPackagePlugin {
        core: implementation::memory_factory(),
        context_state: MemoryContextServiceState::default(),
    })
}

struct MemoryPackagePlugin {
    core: Box<dyn PluginInstance>,
    context_state: MemoryContextServiceState,
}

impl PluginInstance for MemoryPackagePlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        self.core.start(host)?;
        let context = PluginContext::new(host, (), (), ());
        let snapshot = context
            .kernel
            .read_durable(&implementation::memory_namespace(), MEMORY_CONTEXT_STATE_KEY)
            .map_err(|error| error.to_string())?;
        self.context_state = MemoryContextServiceState::restore(snapshot.as_deref())
            .map_err(|error| format!("invalid durable memory.context state: {error:?}"))?;
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &memory_context_service() {
            return self.core.invoke(service, input, host);
        }
        let context = PluginContext::new(host, (), (), ());
        let command = context
            .kernel
            .decode_projected::<MemoryContextCommand>(&MemoryContextInterface::interface_id(), input)
            .map_err(|error| error.to_string())?;
        let response = self.handle_context(command, host)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }

    fn stop(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        self.core.stop(host)
    }
}

impl MemoryPackagePlugin {
    fn handle_context(
        &mut self,
        command: MemoryContextCommand,
        host: &PluginHost<'_>,
    ) -> Result<MemoryContextResponse, String> {
        if let MemoryContextCommand::Observe { observation } = &command {
            validate_observation(self.core.as_mut(), host, observation)?;
        }

        let mutates = matches!(
            &command,
            MemoryContextCommand::Observe { .. } | MemoryContextCommand::ConfirmUse { .. }
        );
        let previous = if mutates {
            read_context_snapshot(host)?
        } else {
            None
        };

        if let Some(response) = self.context_state.handle_local(command.clone()) {
            let response = response?;
            if mutates {
                persist_context_state(host, &mut self.context_state, previous)?;
            }
            return Ok(response);
        }

        match command {
            MemoryContextCommand::Recall { request } => recall_context(
                self.core.as_mut(),
                &self.context_state,
                host,
                request,
            ),
            _ => Err("memory.context command was not handled".into()),
        }
    }
}

fn validate_observation(
    core: &mut dyn PluginInstance,
    host: &PluginHost<'_>,
    observation: &MemoryAssociationObservation,
) -> Result<(), String> {
    if observation.event_id.trim().is_empty() || observation.request_id.trim().is_empty() {
        return Err("memory association observation requires event and request ids".into());
    }
    if observation.association.memory_id.trim().is_empty() {
        return Err("memory association requires a memory id".into());
    }
    if !valid_anchor(&observation.association.anchor) {
        return Err("memory association contains an invalid anchor".into());
    }
    if observation.association.source_refs.is_empty() {
        return Err("memory association requires exact source references".into());
    }

    let record = get_memory(core, host, &observation.association.memory_id)?
        .ok_or_else(|| format!("unknown memory: {}", observation.association.memory_id))?;
    if observation.association.observed_at < record.created_at {
        return Err("memory association observation predates the memory record".into());
    }
    if observation
        .association
        .source_refs
        .iter()
        .any(|reference| !record.source_refs.contains(reference))
    {
        return Err("memory association source is not exact record provenance".into());
    }
    Ok(())
}

fn recall_context(
    core: &mut dyn PluginInstance,
    state: &MemoryContextServiceState,
    host: &PluginHost<'_>,
    request: MemoryContextRecallRequest,
) -> Result<MemoryContextResponse, String> {
    validate_recall_request(&request)?;
    let candidate_limit = request.limit.saturating_mul(4).min(100).max(request.limit);
    let response = invoke_memory(
        core,
        host,
        MemoryCommand::Recall {
            query: MemoryRecallQuery {
                scopes: request.scopes.clone(),
                kinds: Vec::new(),
                query: request.prompt.clone(),
                at: request.at,
                limit: candidate_limit,
            },
        },
    )?;
    let MemoryResponse::Recall { records } = response else {
        return Err("memory recall returned the wrong response kind".into());
    };
    let lexical_saturated = records.len() >= candidate_limit as usize;
    let recalled: BTreeMap<String, MemoryRecord> = records
        .into_iter()
        .map(|record| (record.id.clone(), record))
        .collect();

    let mut candidates = Vec::new();
    for association in state.association_states() {
        if !anchor_compatible(&association.association.anchor, &request.known) {
            continue;
        }
        let exact_anchor = request.known.contains(&association.association.anchor);
        let exact_source = request.known.iter().any(|known| match known {
            ContextAnchor::Resource { service, resource } => association
                .association
                .source_refs
                .iter()
                .any(|reference| &reference.service == service && &reference.resource == resource),
            _ => false,
        });
        let recalled_record = recalled.get(&association.association.memory_id).cloned();
        if recalled_record.is_none() && !exact_anchor && !exact_source {
            continue;
        }
        let record = match recalled_record.clone() {
            Some(record) => record,
            None => {
                let Some(record) = get_memory(core, host, &association.association.memory_id)? else {
                    continue;
                };
                record
            }
        };
        if !request.scopes.contains(&record.scope)
            || !memory_is_current(core, host, &record, request.at)?
        {
            continue;
        }

        let mut signals = BTreeSet::new();
        if exact_anchor {
            signals.insert(MemoryContextMatch::ExactAnchor);
        }
        if exact_source {
            signals.insert(MemoryContextMatch::ExactSource);
        }
        if recalled_record.is_some() {
            if lexical_relevant(&request.prompt, &record.content) {
                signals.insert(MemoryContextMatch::Lexical);
            } else {
                signals.insert(MemoryContextMatch::Semantic);
            }
        }
        if association.confirmed_recoveries > 0
            && signals.iter().any(|signal| signal.evidence_class() >= 2)
        {
            signals.insert(MemoryContextMatch::ConfirmedUse);
        }
        if association.observation_count > 1 {
            signals.insert(MemoryContextMatch::RepeatedObservation);
        }
        if signals.is_empty() {
            continue;
        }

        candidates.push(MemoryContextCandidate {
            memory_id: association.association.memory_id.clone(),
            anchor: association.association.anchor.clone(),
            source_refs: association.association.source_refs.clone(),
            signals: signals.into_iter().collect(),
            observation_count: association.observation_count,
            confirmed_recoveries: association.confirmed_recoveries,
            last_observed_at: association.last_observed_at,
        });
    }

    candidates.sort_by(compare_candidates);
    let incomplete = lexical_saturated || candidates.len() > request.limit as usize;
    candidates.truncate(request.limit as usize);
    Ok(MemoryContextResponse::Recall {
        candidates,
        completeness: if incomplete {
            CandidateCompleteness::Incomplete {
                reason: "candidate_limit_reached".into(),
            }
        } else {
            CandidateCompleteness::Complete
        },
    })
}

fn compare_candidates(
    left: &MemoryContextCandidate,
    right: &MemoryContextCandidate,
) -> std::cmp::Ordering {
    right
        .evidence_class()
        .cmp(&left.evidence_class())
        .then_with(|| right.confirmed_recoveries.cmp(&left.confirmed_recoveries))
        .then_with(|| right.observation_count.cmp(&left.observation_count))
        .then_with(|| right.last_observed_at.cmp(&left.last_observed_at))
        .then_with(|| left.memory_id.cmp(&right.memory_id))
        .then_with(|| left.anchor.cmp(&right.anchor))
}

fn validate_recall_request(request: &MemoryContextRecallRequest) -> Result<(), String> {
    if request.request_id.trim().is_empty() {
        return Err("memory.context recall requires a request id".into());
    }
    if request.scopes.is_empty() {
        return Err("memory.context recall requires at least one scope".into());
    }
    if !(1..=4).contains(&request.needs.len()) {
        return Err("memory.context recall requires between 1 and 4 needs".into());
    }
    if !(1..=20).contains(&request.limit) {
        return Err("memory.context recall limit must be between 1 and 20".into());
    }
    if request.prompt.len() > 4096 {
        return Err("memory.context prompt exceeds 4096 bytes".into());
    }
    if request.known.len() > 32 {
        return Err("memory.context known anchors exceed 32".into());
    }
    let mut distinct = BTreeSet::new();
    for need in &request.needs {
        let query = need_query(need);
        if query.trim().is_empty() || query.len() > 512 {
            return Err("memory.context need query must be 1..=512 bytes".into());
        }
        if !distinct.insert(need.clone()) {
            return Err("memory.context needs must be distinct".into());
        }
    }
    Ok(())
}

fn need_query(need: &ContextNeed) -> &str {
    match need {
        ContextNeed::Workspace { query }
        | ContextNeed::Repository { query }
        | ContextNeed::Project { query }
        | ContextNeed::Task { query }
        | ContextNeed::Session { query }
        | ContextNeed::Resource { query, .. } => query,
    }
}

fn anchor_compatible(anchor: &ContextAnchor, known: &[ContextAnchor]) -> bool {
    let same_kind: Vec<&ContextAnchor> = known
        .iter()
        .filter(|known| std::mem::discriminant(*known) == std::mem::discriminant(anchor))
        .collect();
    same_kind.is_empty() || same_kind.into_iter().any(|known| known == anchor)
}

fn valid_anchor(anchor: &ContextAnchor) -> bool {
    match anchor {
        ContextAnchor::Workspace { workspace_id } => !workspace_id.trim().is_empty(),
        ContextAnchor::Repository {
            canonical_remote,
            workspace_id,
        } => {
            !canonical_remote.trim().is_empty()
                && workspace_id
                    .as_ref()
                    .is_none_or(|workspace_id| !workspace_id.trim().is_empty())
        }
        ContextAnchor::Path { path } => !path.trim().is_empty(),
        ContextAnchor::Project { key } | ContextAnchor::Task { key } => !key.trim().is_empty(),
        ContextAnchor::Session { .. } => true,
        ContextAnchor::Resource { resource, .. } => !resource.trim().is_empty(),
    }
}

fn lexical_relevant(query: &str, content: &str) -> bool {
    let query_terms = terms(query);
    if query_terms.is_empty() {
        return false;
    }
    let content_terms = terms(content);
    query_terms.iter().any(|term| content_terms.contains(term))
}

fn terms(value: &str) -> BTreeSet<String> {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter_map(|term| {
            let term = term.trim().to_lowercase();
            (term.chars().count() > 1).then_some(term)
        })
        .collect()
}

fn memory_is_current(
    core: &mut dyn PluginInstance,
    host: &PluginHost<'_>,
    record: &MemoryRecord,
    at: u64,
) -> Result<bool, String> {
    if record.valid_from.is_some_and(|valid_from| at < valid_from)
        || record.valid_until.is_some_and(|valid_until| at >= valid_until)
    {
        return Ok(false);
    }
    let response = invoke_memory(
        core,
        host,
        MemoryCommand::GetFreshness {
            id: record.id.clone(),
        },
    )?;
    let MemoryResponse::Freshness { state } = response else {
        return Err("memory freshness returned the wrong response kind".into());
    };
    Ok(state.is_some_and(|state| state.freshness == MemoryFreshness::Current))
}

fn get_memory(
    core: &mut dyn PluginInstance,
    host: &PluginHost<'_>,
    id: &str,
) -> Result<Option<MemoryRecord>, String> {
    let response = invoke_memory(core, host, MemoryCommand::Get { id: id.to_owned() })?;
    let MemoryResponse::Memory { record } = response else {
        return Err("memory get returned the wrong response kind".into());
    };
    Ok(record)
}

fn invoke_memory(
    core: &mut dyn PluginInstance,
    host: &PluginHost<'_>,
    command: MemoryCommand,
) -> Result<MemoryResponse, String> {
    let context = PluginContext::new(host, (), (), ());
    let input = context
        .kernel
        .encode_value(&command)
        .map_err(|error| error.to_string())?;
    let output = core.invoke(&memory_service(), &input, host)?;
    context
        .kernel
        .decode_projected::<MemoryResponse>(&MemoryInterface::interface_id(), &output)
        .map_err(|error| error.to_string())
}

fn read_context_snapshot(host: &PluginHost<'_>) -> Result<Option<Vec<u8>>, String> {
    PluginContext::new(host, (), (), ())
        .kernel
        .read_durable(&implementation::memory_namespace(), MEMORY_CONTEXT_STATE_KEY)
        .map_err(|error| error.to_string())
}

fn persist_context_state(
    host: &PluginHost<'_>,
    state: &mut MemoryContextServiceState,
    previous: Option<Vec<u8>>,
) -> Result<(), String> {
    let next = match state.snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            *state = MemoryContextServiceState::restore(previous.as_deref())
                .map_err(|restore| format!("memory.context rollback failed: {restore:?}"))?;
            return Err(format!("memory.context snapshot failed: {error:?}"));
        }
    };
    let result = PluginContext::new(host, (), (), ())
        .kernel
        .transact_durable(
            &implementation::memory_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: MEMORY_CONTEXT_STATE_KEY.into(),
                    expected: previous.clone(),
                },
                TransactionOp::Put {
                    key: MEMORY_CONTEXT_STATE_KEY.into(),
                    value: next,
                },
            ],
        );
    if let Err(error) = result {
        *state = MemoryContextServiceState::restore(previous.as_deref())
            .map_err(|restore| format!("memory.context rollback failed: {restore:?}"))?;
        return Err(error.to_string());
    }
    Ok(())
}
