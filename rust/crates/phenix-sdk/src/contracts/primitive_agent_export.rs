use phenix_core::{ArtifactRevision, Bytes};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTINUATION_PACKET_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContinuationExportMode {
    Full,
    Delta { base_packet_digest: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContinuationRecipient {
    pub identity: String,
    pub resolver_binding: String,
    #[serde(default)]
    pub reference_schemes: BTreeSet<String>,
    pub tokenizer_identity: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContinuationExportBudget {
    pub max_bytes: u64,
    pub max_tokens: Option<u64>,
    pub tokenizer_identity: Option<String>,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationFreshness {
    Current,
    NeedsValidation,
    Historical,
    Exact,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "evidence", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContinuationEvidence {
    ResolvableReference {
        resolver_binding: String,
        digest: String,
        media_type: String,
        bytes: u64,
        locator: String,
    },
    InlineExact {
        digest: String,
        media_type: String,
        content: Bytes,
    },
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationItemKind {
    Goal,
    Constraint,
    Decision,
    Blocker,
    Memory,
    CodeEntity,
    Evidence,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContinuationItem {
    pub id: String,
    pub kind: ContinuationItemKind,
    pub content: String,
    pub freshness: ContinuationFreshness,
    #[serde(default)]
    pub evidence: Vec<ContinuationEvidence>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContinuationPacket {
    pub schema_version: u32,
    pub source_snapshot: String,
    pub recipient_identity: String,
    pub resolver_binding: String,
    pub base_packet_digest: Option<String>,
    pub packet_digest: String,
    pub items: Vec<ContinuationItem>,
    #[serde(default)]
    pub omitted_item_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContinuationDeltaOperation {
    Upsert { item: ContinuationItem },
    Remove { item_id: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContinuationDelta {
    pub schema_version: u32,
    pub source_snapshot: String,
    pub recipient_identity: String,
    pub resolver_binding: String,
    pub base_packet_digest: String,
    pub target_packet_digest: String,
    pub operations: Vec<ContinuationDeltaOperation>,
    #[serde(default)]
    pub omitted_item_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContinuationDeltaError {
    IncompatibleSchema { expected: u32, observed: u32 },
    RecipientMismatch,
    ResolverMismatch,
    BasePacketMismatch { expected: String, observed: String },
    RemoveMissingItem { item_id: String },
    TargetDigestMismatch { expected: String, observed: String },
    Encoding { message: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContinuationExportRequest {
    pub execution_id: String,
    pub checkpoint_id: String,
    pub recipient: ContinuationRecipient,
    pub budget: ContinuationExportBudget,
    pub mode: ContinuationExportMode,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContinuationExportResult {
    Packet {
        packet: ContinuationPacket,
    },
    Delta {
        delta: ContinuationDelta,
    },
    BudgetExhausted {
        required_bytes: u64,
        max_bytes: u64,
    },
    TokenBudgetExhausted {
        required_tokens: u64,
        max_tokens: u64,
        tokenizer_identity: String,
    },
    UnsupportedTokenBound {
        tokenizer_identity: Option<String>,
    },
    UnavailableExactEvidence {
        item_id: String,
    },
    StaleSnapshot {
        expected: String,
        observed: String,
    },
    IncompatiblePacketFormat {
        schema_version: u32,
    },
    BasePacketMismatch {
        expected: String,
        observed: Option<String>,
    },
}

impl ContinuationPacket {
    pub fn encoded_bytes(&self) -> Result<u64, String> {
        u64::try_from(
            serde_json::to_vec(self)
                .map_err(|error| error.to_string())?
                .len(),
        )
        .map_err(|_| "continuation packet length does not fit u64".to_owned())
    }

    pub fn canonical_digest(&self) -> Result<String, String> {
        #[derive(Serialize)]
        struct DigestMaterial<'a> {
            schema_version: u32,
            source_snapshot: &'a str,
            recipient_identity: &'a str,
            resolver_binding: &'a str,
            base_packet_digest: &'a Option<String>,
            items: &'a [ContinuationItem],
            omitted_item_ids: &'a [String],
        }

        let encoded = serde_json::to_vec(&DigestMaterial {
            schema_version: self.schema_version,
            source_snapshot: &self.source_snapshot,
            recipient_identity: &self.recipient_identity,
            resolver_binding: &self.resolver_binding,
            base_packet_digest: &self.base_packet_digest,
            items: &self.items,
            omitted_item_ids: &self.omitted_item_ids,
        })
        .map_err(|error| error.to_string())?;
        Ok(ArtifactRevision::from_content(&encoded).to_string())
    }

    pub fn refresh_digest(&mut self) -> Result<(), String> {
        self.packet_digest = self.canonical_digest()?;
        Ok(())
    }
}

fn encoded_len<T: Serialize>(value: &T) -> Result<u64, String> {
    u64::try_from(
        serde_json::to_vec(value)
            .map_err(|error| error.to_string())?
            .len(),
    )
    .map_err(|_| "continuation export length does not fit u64".to_owned())
}

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    PartialEq,
    Serialize,
    Deserialize,
    phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct ContinuationExportMeasurements {
    pub full_tokens: Option<u64>,
    pub delta_tokens: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContinuationProjectionCandidate {
    pub item: ContinuationItem,
    pub required: bool,
    #[serde(default)]
    pub verified_reference_digests: BTreeSet<String>,
    #[serde(default)]
    pub inline_fallbacks: BTreeMap<String, ContinuationEvidence>,
}

#[derive(
    Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct ContinuationSourceState {
    pub goal: Option<ContinuationProjectionCandidate>,
    #[serde(default)]
    pub constraints: Vec<ContinuationProjectionCandidate>,
    #[serde(default)]
    pub decisions: Vec<ContinuationProjectionCandidate>,
    #[serde(default)]
    pub blockers: Vec<ContinuationProjectionCandidate>,
    #[serde(default)]
    pub fresh_memory: Vec<ContinuationProjectionCandidate>,
    #[serde(default)]
    pub code_entities: Vec<ContinuationProjectionCandidate>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContinuationSourceStateError {
    KindMismatch {
        item_id: String,
        expected: ContinuationItemKind,
        observed: ContinuationItemKind,
    },
    MemoryNotCurrent {
        item_id: String,
        freshness: ContinuationFreshness,
    },
}

pub fn assemble_continuation_candidates(
    state: &ContinuationSourceState,
) -> Result<Vec<ContinuationProjectionCandidate>, ContinuationSourceStateError> {
    let mut candidates = Vec::new();
    let mut append = |candidate: &ContinuationProjectionCandidate,
                      expected: ContinuationItemKind|
     -> Result<(), ContinuationSourceStateError> {
        if candidate.item.kind != expected {
            return Err(ContinuationSourceStateError::KindMismatch {
                item_id: candidate.item.id.clone(),
                expected,
                observed: candidate.item.kind,
            });
        }
        candidates.push(candidate.clone());
        Ok(())
    };

    if let Some(goal) = &state.goal {
        append(goal, ContinuationItemKind::Goal)?;
    }
    for constraint in &state.constraints {
        append(constraint, ContinuationItemKind::Constraint)?;
    }
    for decision in &state.decisions {
        append(decision, ContinuationItemKind::Decision)?;
    }
    for blocker in &state.blockers {
        append(blocker, ContinuationItemKind::Blocker)?;
    }
    for memory in &state.fresh_memory {
        if memory.item.freshness != ContinuationFreshness::Current {
            return Err(ContinuationSourceStateError::MemoryNotCurrent {
                item_id: memory.item.id.clone(),
                freshness: memory.item.freshness,
            });
        }
        append(memory, ContinuationItemKind::Memory)?;
    }
    for entity in &state.code_entities {
        append(entity, ContinuationItemKind::CodeEntity)?;
    }
    Ok(candidates)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContinuationProjectionRequest {
    pub export: ContinuationExportRequest,
    #[serde(default)]
    pub source_state: ContinuationSourceState,
    pub expected_source_snapshot: String,
    #[serde(default)]
    pub candidates: Vec<ContinuationProjectionCandidate>,
    pub base_packet: Option<ContinuationPacket>,
    pub acknowledged_base_digest: Option<String>,
    pub measurements: ContinuationExportMeasurements,
}

pub fn build_continuation_packet(
    request: &ContinuationExportRequest,
    source_snapshot: &str,
    candidates: &[ContinuationProjectionCandidate],
) -> Result<ContinuationExportResult, String> {
    let mut items = BTreeMap::<String, ContinuationItem>::new();
    let mut omitted_item_ids = Vec::new();

    for candidate in candidates {
        if candidate.item.id.trim().is_empty() {
            return Err("continuation item id must not be empty".into());
        }
        if items.contains_key(&candidate.item.id) {
            return Err(format!(
                "duplicate continuation item id: {}",
                candidate.item.id
            ));
        }

        let mut item = candidate.item.clone();
        let mut evidence = Vec::with_capacity(item.evidence.len());
        let mut unavailable = false;
        for current in &item.evidence {
            match current {
                ContinuationEvidence::InlineExact { .. } => evidence.push(current.clone()),
                ContinuationEvidence::ResolvableReference {
                    resolver_binding,
                    digest,
                    ..
                } if resolver_binding == &request.recipient.resolver_binding
                    && candidate.verified_reference_digests.contains(digest) =>
                {
                    evidence.push(current.clone());
                }
                ContinuationEvidence::ResolvableReference { digest, .. } => {
                    match candidate.inline_fallbacks.get(digest) {
                        Some(
                            fallback @ ContinuationEvidence::InlineExact {
                                digest: fallback_digest,
                                ..
                            },
                        ) if fallback_digest == digest => evidence.push(fallback.clone()),
                        _ => {
                            unavailable = true;
                            break;
                        }
                    }
                }
            }
        }

        if unavailable {
            if candidate.required {
                return Ok(ContinuationExportResult::UnavailableExactEvidence {
                    item_id: candidate.item.id.clone(),
                });
            }
            omitted_item_ids.push(candidate.item.id.clone());
            continue;
        }

        item.evidence = evidence;
        items.insert(item.id.clone(), item);
    }

    omitted_item_ids.sort();
    let mut packet = ContinuationPacket {
        schema_version: CONTINUATION_PACKET_SCHEMA_VERSION,
        source_snapshot: source_snapshot.to_owned(),
        recipient_identity: request.recipient.identity.clone(),
        resolver_binding: request.recipient.resolver_binding.clone(),
        base_packet_digest: match &request.mode {
            ContinuationExportMode::Full => None,
            ContinuationExportMode::Delta { base_packet_digest } => {
                Some(base_packet_digest.clone())
            }
        },
        packet_digest: String::new(),
        items: items.into_values().collect(),
        omitted_item_ids,
    };
    packet.refresh_digest()?;
    Ok(ContinuationExportResult::Packet { packet })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContinuationImportRequest {
    pub execution_id: String,
    pub recipient: ContinuationRecipient,
    pub packet: ContinuationPacket,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContinuationImportProjection {
    pub execution_id: String,
    pub packet_digest: String,
    pub source_snapshot: String,
    pub candidates: Vec<super::ContextCandidate>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContinuationImportError {
    IncompatibleSchema { observed: u32 },
    RecipientMismatch,
    ResolverMismatch,
    PacketDigestMismatch { expected: String, observed: String },
    InvalidInlineEvidence { item_id: String },
    Encoding { message: String },
}

pub fn project_continuation_import(
    request: &ContinuationImportRequest,
) -> Result<ContinuationImportProjection, ContinuationImportError> {
    if request.packet.schema_version != CONTINUATION_PACKET_SCHEMA_VERSION {
        return Err(ContinuationImportError::IncompatibleSchema {
            observed: request.packet.schema_version,
        });
    }
    if request.packet.recipient_identity != request.recipient.identity {
        return Err(ContinuationImportError::RecipientMismatch);
    }
    if request.packet.resolver_binding != request.recipient.resolver_binding {
        return Err(ContinuationImportError::ResolverMismatch);
    }
    let observed_digest = request
        .packet
        .canonical_digest()
        .map_err(|message| ContinuationImportError::Encoding { message })?;
    if observed_digest != request.packet.packet_digest {
        return Err(ContinuationImportError::PacketDigestMismatch {
            expected: request.packet.packet_digest.clone(),
            observed: observed_digest,
        });
    }

    let mut candidates = Vec::with_capacity(request.packet.items.len());
    for item in &request.packet.items {
        for evidence in &item.evidence {
            if let ContinuationEvidence::InlineExact {
                digest, content, ..
            } = evidence
            {
                let observed = ArtifactRevision::from_content(content.as_ref()).to_string();
                if &observed != digest {
                    return Err(ContinuationImportError::InvalidInlineEvidence {
                        item_id: item.id.clone(),
                    });
                }
            }
        }
        let encoded =
            serde_json::to_vec(item).map_err(|error| ContinuationImportError::Encoding {
                message: error.to_string(),
            })?;
        let content_identity = ArtifactRevision::from_content(&encoded).to_string();
        let estimated_tokens = u64::try_from(encoded.len()).unwrap_or(u64::MAX);
        candidates.push(super::ContextCandidate {
            id: format!("continuation:{}:{}", request.packet.packet_digest, item.id),
            source: super::ContextSource::Continuation {
                packet_digest: request.packet.packet_digest.clone(),
                item_id: item.id.clone(),
            },
            content_identity,
            content: Bytes::from(encoded),
            estimated_tokens,
            mandatory: matches!(
                item.kind,
                ContinuationItemKind::Goal
                    | ContinuationItemKind::Constraint
                    | ContinuationItemKind::Blocker
            ),
            retention: super::ContextRetention::DropAllowed,
            cache: super::CachePlacement::Epoch,
            recovery: None,
        });
    }

    Ok(ContinuationImportProjection {
        execution_id: request.execution_id.clone(),
        packet_digest: request.packet.packet_digest.clone(),
        source_snapshot: request.packet.source_snapshot.clone(),
        candidates,
    })
}

pub fn derive_continuation_delta(
    base: &ContinuationPacket,
    target: &ContinuationPacket,
) -> Result<ContinuationDelta, ContinuationDeltaError> {
    if base.schema_version != CONTINUATION_PACKET_SCHEMA_VERSION
        || target.schema_version != CONTINUATION_PACKET_SCHEMA_VERSION
    {
        return Err(ContinuationDeltaError::IncompatibleSchema {
            expected: CONTINUATION_PACKET_SCHEMA_VERSION,
            observed: target.schema_version,
        });
    }
    if base.recipient_identity != target.recipient_identity {
        return Err(ContinuationDeltaError::RecipientMismatch);
    }
    if base.resolver_binding != target.resolver_binding {
        return Err(ContinuationDeltaError::ResolverMismatch);
    }

    let base_digest = base
        .canonical_digest()
        .map_err(|message| ContinuationDeltaError::Encoding { message })?;
    let target_digest = target
        .canonical_digest()
        .map_err(|message| ContinuationDeltaError::Encoding { message })?;
    let base_items = base
        .items
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect::<BTreeMap<_, _>>();
    let target_items = target
        .items
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect::<BTreeMap<_, _>>();
    let mut operations = Vec::new();

    for item_id in base_items.keys() {
        if !target_items.contains_key(item_id) {
            operations.push(ContinuationDeltaOperation::Remove {
                item_id: (*item_id).to_owned(),
            });
        }
    }
    for (item_id, item) in &target_items {
        if base_items
            .get(item_id)
            .is_none_or(|base_item| *base_item != *item)
        {
            operations.push(ContinuationDeltaOperation::Upsert {
                item: (*item).clone(),
            });
        }
    }

    Ok(ContinuationDelta {
        schema_version: CONTINUATION_PACKET_SCHEMA_VERSION,
        source_snapshot: target.source_snapshot.clone(),
        recipient_identity: target.recipient_identity.clone(),
        resolver_binding: target.resolver_binding.clone(),
        base_packet_digest: base_digest,
        target_packet_digest: target_digest,
        operations,
        omitted_item_ids: target.omitted_item_ids.clone(),
    })
}

fn validate_token_budget(
    request: &ContinuationExportRequest,
    measured_tokens: Option<u64>,
) -> Option<ContinuationExportResult> {
    let max_tokens = request.budget.max_tokens?;
    let expected = request.budget.tokenizer_identity.as_deref();
    let recipient = request.recipient.tokenizer_identity.as_deref();
    if expected.is_none() || expected != recipient {
        return Some(ContinuationExportResult::UnsupportedTokenBound {
            tokenizer_identity: request.budget.tokenizer_identity.clone(),
        });
    }
    let measured_tokens = match measured_tokens {
        Some(value) => value,
        None => {
            return Some(ContinuationExportResult::UnsupportedTokenBound {
                tokenizer_identity: request.budget.tokenizer_identity.clone(),
            })
        }
    };
    if measured_tokens > max_tokens {
        return Some(ContinuationExportResult::TokenBudgetExhausted {
            required_tokens: measured_tokens,
            max_tokens,
            tokenizer_identity: expected.expect("checked above").to_owned(),
        });
    }
    None
}

pub fn select_continuation_export(
    request: &ContinuationExportRequest,
    mut full_packet: ContinuationPacket,
    delta: Option<ContinuationDelta>,
    acknowledged_base_digest: Option<&str>,
    measurements: ContinuationExportMeasurements,
) -> Result<ContinuationExportResult, String> {
    if full_packet.schema_version != CONTINUATION_PACKET_SCHEMA_VERSION {
        return Ok(ContinuationExportResult::IncompatiblePacketFormat {
            schema_version: full_packet.schema_version,
        });
    }
    // The digest is derived state. Recompute it at publication so a stale caller-side
    // storage field can never leak into a packet or authorize a mismatched delta.
    full_packet.refresh_digest()?;
    if full_packet.recipient_identity != request.recipient.identity
        || full_packet.resolver_binding != request.recipient.resolver_binding
    {
        return Ok(ContinuationExportResult::UnavailableExactEvidence {
            item_id: "recipient_binding".into(),
        });
    }
    match &request.mode {
        ContinuationExportMode::Delta { base_packet_digest } => {
            if acknowledged_base_digest == Some(base_packet_digest.as_str()) {
                if let Some(delta) = delta {
                    if delta.base_packet_digest == *base_packet_digest
                        && delta.target_packet_digest == full_packet.packet_digest
                        && delta.recipient_identity == request.recipient.identity
                        && delta.resolver_binding == request.recipient.resolver_binding
                        && delta.schema_version == CONTINUATION_PACKET_SCHEMA_VERSION
                    {
                        let required_bytes = encoded_len(&delta)?;
                        if required_bytes <= request.budget.max_bytes
                            && validate_token_budget(request, measurements.delta_tokens).is_none()
                        {
                            return Ok(ContinuationExportResult::Delta { delta });
                        }
                    }
                }
            }
        }
        ContinuationExportMode::Full => {}
    }

    let required_bytes = full_packet.encoded_bytes()?;
    if required_bytes > request.budget.max_bytes {
        return Ok(ContinuationExportResult::BudgetExhausted {
            required_bytes,
            max_bytes: request.budget.max_bytes,
        });
    }
    if let Some(result) = validate_token_budget(request, measurements.full_tokens) {
        return Ok(result);
    }
    Ok(ContinuationExportResult::Packet {
        packet: full_packet,
    })
}

impl ContinuationDelta {
    pub fn apply_to(
        &self,
        base: &ContinuationPacket,
    ) -> Result<ContinuationPacket, ContinuationDeltaError> {
        if self.schema_version != CONTINUATION_PACKET_SCHEMA_VERSION
            || base.schema_version != self.schema_version
        {
            return Err(ContinuationDeltaError::IncompatibleSchema {
                expected: CONTINUATION_PACKET_SCHEMA_VERSION,
                observed: self.schema_version,
            });
        }
        if self.recipient_identity != base.recipient_identity {
            return Err(ContinuationDeltaError::RecipientMismatch);
        }
        if self.resolver_binding != base.resolver_binding {
            return Err(ContinuationDeltaError::ResolverMismatch);
        }

        let observed_base = base
            .canonical_digest()
            .map_err(|message| ContinuationDeltaError::Encoding { message })?;
        if observed_base != self.base_packet_digest {
            return Err(ContinuationDeltaError::BasePacketMismatch {
                expected: self.base_packet_digest.clone(),
                observed: observed_base,
            });
        }

        let mut items = base
            .items
            .iter()
            .cloned()
            .map(|item| (item.id.clone(), item))
            .collect::<BTreeMap<_, _>>();
        for operation in &self.operations {
            match operation {
                ContinuationDeltaOperation::Upsert { item } => {
                    items.insert(item.id.clone(), item.clone());
                }
                ContinuationDeltaOperation::Remove { item_id } => {
                    if items.remove(item_id).is_none() {
                        return Err(ContinuationDeltaError::RemoveMissingItem {
                            item_id: item_id.clone(),
                        });
                    }
                }
            }
        }

        let mut packet = ContinuationPacket {
            schema_version: self.schema_version,
            source_snapshot: self.source_snapshot.clone(),
            recipient_identity: self.recipient_identity.clone(),
            resolver_binding: self.resolver_binding.clone(),
            base_packet_digest: Some(self.base_packet_digest.clone()),
            packet_digest: String::new(),
            items: items.into_values().collect(),
            omitted_item_ids: self.omitted_item_ids.clone(),
        };
        let observed_target = packet
            .canonical_digest()
            .map_err(|message| ContinuationDeltaError::Encoding { message })?;
        if observed_target != self.target_packet_digest {
            return Err(ContinuationDeltaError::TargetDigestMismatch {
                expected: self.target_packet_digest.clone(),
                observed: observed_target,
            });
        }
        packet.packet_digest = observed_target;
        Ok(packet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet() -> ContinuationPacket {
        ContinuationPacket {
            schema_version: CONTINUATION_PACKET_SCHEMA_VERSION,
            source_snapshot: "snapshot-1".into(),
            recipient_identity: "recipient-1".into(),
            resolver_binding: "resolver-1".into(),
            base_packet_digest: None,
            packet_digest: "sha256:packet".into(),
            items: vec![ContinuationItem {
                id: "goal".into(),
                kind: ContinuationItemKind::Goal,
                content: "finish the task".into(),
                freshness: ContinuationFreshness::Exact,
                evidence: Vec::new(),
            }],
            omitted_item_ids: Vec::new(),
        }
    }

    fn request(mode: ContinuationExportMode, max_bytes: u64) -> ContinuationExportRequest {
        ContinuationExportRequest {
            execution_id: "execution-1".into(),
            checkpoint_id: "checkpoint-1".into(),
            recipient: ContinuationRecipient {
                identity: "recipient-1".into(),
                resolver_binding: "resolver-1".into(),
                reference_schemes: BTreeSet::new(),
                tokenizer_identity: None,
            },
            budget: ContinuationExportBudget {
                max_bytes,
                max_tokens: None,
                tokenizer_identity: None,
            },
            mode,
        }
    }

    fn source_candidate(
        id: &str,
        kind: ContinuationItemKind,
        freshness: ContinuationFreshness,
    ) -> ContinuationProjectionCandidate {
        ContinuationProjectionCandidate {
            item: ContinuationItem {
                id: id.into(),
                kind,
                content: format!("{kind:?} content"),
                freshness,
                evidence: Vec::new(),
            },
            required: matches!(
                kind,
                ContinuationItemKind::Goal
                    | ContinuationItemKind::Constraint
                    | ContinuationItemKind::Blocker
            ),
            verified_reference_digests: BTreeSet::new(),
            inline_fallbacks: BTreeMap::new(),
        }
    }

    #[test]
    fn source_state_assembles_semantic_continuation_sections_in_stable_order() {
        let state = ContinuationSourceState {
            goal: Some(source_candidate(
                "goal",
                ContinuationItemKind::Goal,
                ContinuationFreshness::Exact,
            )),
            constraints: vec![source_candidate(
                "constraint",
                ContinuationItemKind::Constraint,
                ContinuationFreshness::Exact,
            )],
            decisions: vec![source_candidate(
                "decision",
                ContinuationItemKind::Decision,
                ContinuationFreshness::Current,
            )],
            blockers: vec![source_candidate(
                "blocker",
                ContinuationItemKind::Blocker,
                ContinuationFreshness::Current,
            )],
            fresh_memory: vec![source_candidate(
                "memory",
                ContinuationItemKind::Memory,
                ContinuationFreshness::Current,
            )],
            code_entities: vec![source_candidate(
                "code",
                ContinuationItemKind::CodeEntity,
                ContinuationFreshness::Current,
            )],
        };

        let candidates = assemble_continuation_candidates(&state).unwrap();
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.item.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "goal",
                "constraint",
                "decision",
                "blocker",
                "memory",
                "code"
            ]
        );
    }

    #[test]
    fn source_state_rejects_stale_memory_from_the_fresh_memory_slot() {
        let state = ContinuationSourceState {
            fresh_memory: vec![source_candidate(
                "memory",
                ContinuationItemKind::Memory,
                ContinuationFreshness::Historical,
            )],
            ..ContinuationSourceState::default()
        };

        assert_eq!(
            assemble_continuation_candidates(&state),
            Err(ContinuationSourceStateError::MemoryNotCurrent {
                item_id: "memory".into(),
                freshness: ContinuationFreshness::Historical,
            })
        );
    }

    #[test]
    fn projection_inlines_required_evidence_when_recipient_reference_is_unavailable() {
        let request = request(ContinuationExportMode::Full, u64::MAX);
        let digest = ArtifactRevision::from_content(b"exact evidence").to_string();
        let candidate = ContinuationProjectionCandidate {
            item: ContinuationItem {
                id: "decision".into(),
                kind: ContinuationItemKind::Decision,
                content: "keep the invariant".into(),
                freshness: ContinuationFreshness::Exact,
                evidence: vec![ContinuationEvidence::ResolvableReference {
                    resolver_binding: "resolver-1".into(),
                    digest: digest.clone(),
                    media_type: "text/plain".into(),
                    bytes: 14,
                    locator: "artifact://decision".into(),
                }],
            },
            required: true,
            verified_reference_digests: BTreeSet::new(),
            inline_fallbacks: BTreeMap::from([(
                digest.clone(),
                ContinuationEvidence::InlineExact {
                    digest,
                    media_type: "text/plain".into(),
                    content: Bytes::from(b"exact evidence".to_vec()),
                },
            )]),
        };

        let result = build_continuation_packet(&request, "snapshot-1", &[candidate]).unwrap();
        let ContinuationExportResult::Packet { packet } = result else {
            panic!("expected packet");
        };
        assert!(matches!(
            packet.items[0].evidence[0],
            ContinuationEvidence::InlineExact { .. }
        ));
    }

    #[test]
    fn projection_omits_optional_unresolvable_evidence_but_fails_required_evidence() {
        let request = request(ContinuationExportMode::Full, u64::MAX);
        let candidate = ContinuationProjectionCandidate {
            item: ContinuationItem {
                id: "optional-memory".into(),
                kind: ContinuationItemKind::Memory,
                content: "derived memory".into(),
                freshness: ContinuationFreshness::Current,
                evidence: vec![ContinuationEvidence::ResolvableReference {
                    resolver_binding: "other-resolver".into(),
                    digest: "sha256:missing".into(),
                    media_type: "text/plain".into(),
                    bytes: 12,
                    locator: "memory://missing".into(),
                }],
            },
            required: false,
            verified_reference_digests: BTreeSet::new(),
            inline_fallbacks: BTreeMap::new(),
        };

        let result =
            build_continuation_packet(&request, "snapshot-1", std::slice::from_ref(&candidate))
                .unwrap();
        let ContinuationExportResult::Packet { packet } = result else {
            panic!("expected packet");
        };
        assert!(packet.items.is_empty());
        assert_eq!(packet.omitted_item_ids, vec!["optional-memory"]);

        let mut required = candidate;
        required.required = true;
        assert_eq!(
            build_continuation_packet(&request, "snapshot-1", &[required]).unwrap(),
            ContinuationExportResult::UnavailableExactEvidence {
                item_id: "optional-memory".into()
            }
        );
    }

    #[test]
    fn derived_delta_round_trips_exact_target_packet() {
        let mut base = packet();
        base.refresh_digest().unwrap();
        let mut target = base.clone();
        target.source_snapshot = "snapshot-2".into();
        target.base_packet_digest = Some(base.packet_digest.clone());
        target.items.push(ContinuationItem {
            id: "blocker".into(),
            kind: ContinuationItemKind::Blocker,
            content: "new blocker".into(),
            freshness: ContinuationFreshness::Current,
            evidence: Vec::new(),
        });
        target.refresh_digest().unwrap();

        let delta = derive_continuation_delta(&base, &target).unwrap();
        assert_eq!(delta.apply_to(&base).unwrap(), target);
    }

    #[test]
    fn imported_packet_projects_only_to_untrusted_context_candidates() {
        let mut packet = packet();
        packet.items.push(ContinuationItem {
            id: "decision".into(),
            kind: ContinuationItemKind::Decision,
            content: "remote decision".into(),
            freshness: ContinuationFreshness::Historical,
            evidence: Vec::new(),
        });
        packet.refresh_digest().unwrap();
        let projection = project_continuation_import(&ContinuationImportRequest {
            execution_id: "execution-1".into(),
            recipient: ContinuationRecipient {
                identity: packet.recipient_identity.clone(),
                resolver_binding: packet.resolver_binding.clone(),
                reference_schemes: BTreeSet::new(),
                tokenizer_identity: None,
            },
            packet: packet.clone(),
        })
        .unwrap();

        assert_eq!(projection.source_snapshot, "snapshot-1");
        assert_eq!(projection.candidates.len(), 2);
        assert!(projection.candidates.iter().all(|candidate| matches!(
            candidate.source,
            super::super::ContextSource::Continuation { .. }
        )));
        let decision = projection
            .candidates
            .iter()
            .find(|candidate| candidate.id.ends_with(":decision"))
            .unwrap();
        assert!(!decision.mandatory);
        let decoded: ContinuationItem = serde_json::from_slice(decision.content.as_ref()).unwrap();
        assert_eq!(decoded.freshness, ContinuationFreshness::Historical);
    }

    #[test]
    fn import_rejects_tampered_inline_evidence_and_packet_digest() {
        let mut packet = packet();
        let digest = ArtifactRevision::from_content(b"original").to_string();
        packet.items[0]
            .evidence
            .push(ContinuationEvidence::InlineExact {
                digest,
                media_type: "text/plain".into(),
                content: Bytes::from(b"tampered".to_vec()),
            });
        packet.refresh_digest().unwrap();
        let recipient = ContinuationRecipient {
            identity: packet.recipient_identity.clone(),
            resolver_binding: packet.resolver_binding.clone(),
            reference_schemes: BTreeSet::new(),
            tokenizer_identity: None,
        };
        assert!(matches!(
            project_continuation_import(&ContinuationImportRequest {
                execution_id: "execution-1".into(),
                recipient: recipient.clone(),
                packet: packet.clone(),
            }),
            Err(ContinuationImportError::InvalidInlineEvidence { .. })
        ));

        packet.items[0].evidence.clear();
        packet.packet_digest = "sha256:stale".into();
        assert!(matches!(
            project_continuation_import(&ContinuationImportRequest {
                execution_id: "execution-1".into(),
                recipient,
                packet,
            }),
            Err(ContinuationImportError::PacketDigestMismatch { .. })
        ));
    }

    #[test]
    fn packet_without_code_entities_remains_a_valid_continuation() {
        let mut packet = packet();
        packet.items.push(ContinuationItem {
            id: "blocker".into(),
            kind: ContinuationItemKind::Blocker,
            content: "need current file contents".into(),
            freshness: ContinuationFreshness::Current,
            evidence: Vec::new(),
        });
        packet.refresh_digest().unwrap();
        let projection = project_continuation_import(&ContinuationImportRequest {
            execution_id: "execution-1".into(),
            recipient: ContinuationRecipient {
                identity: packet.recipient_identity.clone(),
                resolver_binding: packet.resolver_binding.clone(),
                reference_schemes: BTreeSet::new(),
                tokenizer_identity: None,
            },
            packet,
        })
        .unwrap();
        assert_eq!(projection.candidates.len(), 2);
        assert!(projection.candidates.iter().all(|candidate| {
            !String::from_utf8_lossy(candidate.content.as_ref()).contains("code_entity")
        }));
    }

    #[test]
    fn sender_falls_back_to_full_packet_when_exact_base_is_unavailable() {
        let mut full = packet();
        full.refresh_digest().unwrap();
        let base_digest = ArtifactRevision::from_content(b"base").to_string();
        let delta = ContinuationDelta {
            schema_version: CONTINUATION_PACKET_SCHEMA_VERSION,
            source_snapshot: "snapshot-2".into(),
            recipient_identity: "recipient-1".into(),
            resolver_binding: "resolver-1".into(),
            base_packet_digest: base_digest.clone(),
            target_packet_digest: full.packet_digest.clone(),
            operations: Vec::new(),
            omitted_item_ids: Vec::new(),
        };
        let request = request(
            ContinuationExportMode::Delta {
                base_packet_digest: base_digest,
            },
            u64::MAX,
        );

        assert!(matches!(
            select_continuation_export(
                &request,
                full,
                Some(delta),
                None,
                ContinuationExportMeasurements::default()
            )
            .unwrap(),
            ContinuationExportResult::Packet { .. }
        ));
    }

    #[test]
    fn sender_uses_delta_only_for_the_exact_acknowledged_base() {
        let mut full = packet();
        full.refresh_digest().unwrap();
        let base_digest = ArtifactRevision::from_content(b"base").to_string();
        let delta = ContinuationDelta {
            schema_version: CONTINUATION_PACKET_SCHEMA_VERSION,
            source_snapshot: "snapshot-2".into(),
            recipient_identity: "recipient-1".into(),
            resolver_binding: "resolver-1".into(),
            base_packet_digest: base_digest.clone(),
            target_packet_digest: full.packet_digest.clone(),
            operations: Vec::new(),
            omitted_item_ids: Vec::new(),
        };
        let request = request(
            ContinuationExportMode::Delta {
                base_packet_digest: base_digest.clone(),
            },
            u64::MAX,
        );

        assert_eq!(
            select_continuation_export(
                &request,
                full,
                Some(delta.clone()),
                Some(&base_digest),
                ContinuationExportMeasurements::default(),
            )
            .unwrap(),
            ContinuationExportResult::Delta { delta }
        );
    }

    #[test]
    fn token_measurement_is_bound_to_the_selected_representation() {
        let mut full = packet();
        full.refresh_digest().unwrap();
        let base_digest = ArtifactRevision::from_content(b"base").to_string();
        let delta = ContinuationDelta {
            schema_version: CONTINUATION_PACKET_SCHEMA_VERSION,
            source_snapshot: "snapshot-2".into(),
            recipient_identity: "recipient-1".into(),
            resolver_binding: "resolver-1".into(),
            base_packet_digest: base_digest.clone(),
            target_packet_digest: full.packet_digest.clone(),
            operations: Vec::new(),
            omitted_item_ids: Vec::new(),
        };
        let mut request = request(
            ContinuationExportMode::Delta {
                base_packet_digest: base_digest.clone(),
            },
            u64::MAX,
        );
        request.recipient.tokenizer_identity = Some("tokenizer-1".into());
        request.budget.tokenizer_identity = Some("tokenizer-1".into());
        request.budget.max_tokens = Some(10);

        assert_eq!(
            select_continuation_export(
                &request,
                full,
                Some(delta.clone()),
                Some(&base_digest),
                ContinuationExportMeasurements {
                    full_tokens: Some(20),
                    delta_tokens: Some(5),
                },
            )
            .unwrap(),
            ContinuationExportResult::Delta { delta }
        );
    }

    #[test]
    fn sender_recomputes_the_published_full_packet_digest() {
        let mut full = packet();
        full.packet_digest = "stale-caller-value".into();
        let request = request(ContinuationExportMode::Full, u64::MAX);

        let result = select_continuation_export(
            &request,
            full,
            None,
            None,
            ContinuationExportMeasurements::default(),
        )
        .unwrap();
        let ContinuationExportResult::Packet { packet } = result else {
            panic!("expected full packet");
        };
        assert_eq!(packet.packet_digest, packet.canonical_digest().unwrap());
        assert_ne!(packet.packet_digest, "stale-caller-value");
    }

    #[test]
    fn sender_does_not_select_a_delta_for_a_different_target_packet() {
        let mut full = packet();
        full.refresh_digest().unwrap();
        let base_digest = ArtifactRevision::from_content(b"base").to_string();
        let delta = ContinuationDelta {
            schema_version: CONTINUATION_PACKET_SCHEMA_VERSION,
            source_snapshot: "snapshot-2".into(),
            recipient_identity: "recipient-1".into(),
            resolver_binding: "resolver-1".into(),
            base_packet_digest: base_digest.clone(),
            target_packet_digest: ArtifactRevision::from_content(b"different-target").to_string(),
            operations: Vec::new(),
            omitted_item_ids: Vec::new(),
        };
        let request = request(
            ContinuationExportMode::Delta {
                base_packet_digest: base_digest.clone(),
            },
            u64::MAX,
        );

        assert!(matches!(
            select_continuation_export(
                &request,
                full,
                Some(delta),
                Some(&base_digest),
                ContinuationExportMeasurements::default()
            )
            .unwrap(),
            ContinuationExportResult::Packet { .. }
        ));
    }

    #[test]
    fn full_export_enforces_complete_byte_budget_without_truncation() {
        let mut full = packet();
        full.refresh_digest().unwrap();
        let required_bytes = full.encoded_bytes().unwrap();
        let request = request(ContinuationExportMode::Full, required_bytes - 1);

        assert_eq!(
            select_continuation_export(
                &request,
                full,
                None,
                None,
                ContinuationExportMeasurements::default()
            )
            .unwrap(),
            ContinuationExportResult::BudgetExhausted {
                required_bytes,
                max_bytes: required_bytes - 1,
            }
        );
    }

    #[test]
    fn hard_token_bound_requires_matching_tokenizer_and_measurement() {
        let mut full = packet();
        full.refresh_digest().unwrap();
        let mut request = request(ContinuationExportMode::Full, u64::MAX);
        request.budget.max_tokens = Some(10);
        request.budget.tokenizer_identity = Some("tok-v1".into());
        request.recipient.tokenizer_identity = Some("tok-v1".into());

        assert_eq!(
            select_continuation_export(
                &request,
                full.clone(),
                None,
                None,
                ContinuationExportMeasurements::default()
            )
            .unwrap(),
            ContinuationExportResult::UnsupportedTokenBound {
                tokenizer_identity: Some("tok-v1".into()),
            }
        );
        assert_eq!(
            select_continuation_export(
                &request,
                full,
                None,
                None,
                ContinuationExportMeasurements {
                    full_tokens: Some(11),
                    delta_tokens: None,
                },
            )
            .unwrap(),
            ContinuationExportResult::TokenBudgetExhausted {
                required_tokens: 11,
                max_tokens: 10,
                tokenizer_identity: "tok-v1".into(),
            }
        );
    }

    #[test]
    fn canonical_digest_ignores_its_storage_field_but_binds_packet_content() {
        let mut packet = packet();
        packet.packet_digest = "stale".into();
        let first = packet.canonical_digest().unwrap();
        packet.packet_digest = "different-stale-value".into();
        assert_eq!(packet.canonical_digest().unwrap(), first);
        packet.items[0].content = "different task".into();
        assert_ne!(packet.canonical_digest().unwrap(), first);
    }

    #[test]
    fn exact_delta_round_trip_applies_upserts_and_removals() {
        let mut base = packet();
        base.items.push(ContinuationItem {
            id: "blocker".into(),
            kind: ContinuationItemKind::Blocker,
            content: "old blocker".into(),
            freshness: ContinuationFreshness::Current,
            evidence: Vec::new(),
        });
        base.refresh_digest().unwrap();
        let base_digest = base.packet_digest.clone();

        let mut target = ContinuationPacket {
            schema_version: CONTINUATION_PACKET_SCHEMA_VERSION,
            source_snapshot: "snapshot-2".into(),
            recipient_identity: base.recipient_identity.clone(),
            resolver_binding: base.resolver_binding.clone(),
            base_packet_digest: Some(base_digest.clone()),
            packet_digest: String::new(),
            items: vec![
                ContinuationItem {
                    id: "decision".into(),
                    kind: ContinuationItemKind::Decision,
                    content: "continue".into(),
                    freshness: ContinuationFreshness::Current,
                    evidence: Vec::new(),
                },
                base.items[0].clone(),
            ],
            omitted_item_ids: vec!["optional-memory".into()],
        };
        target.refresh_digest().unwrap();

        let delta = ContinuationDelta {
            schema_version: CONTINUATION_PACKET_SCHEMA_VERSION,
            source_snapshot: target.source_snapshot.clone(),
            recipient_identity: target.recipient_identity.clone(),
            resolver_binding: target.resolver_binding.clone(),
            base_packet_digest: base_digest,
            target_packet_digest: target.packet_digest.clone(),
            operations: vec![
                ContinuationDeltaOperation::Remove {
                    item_id: "blocker".into(),
                },
                ContinuationDeltaOperation::Upsert {
                    item: target.items[0].clone(),
                },
            ],
            omitted_item_ids: target.omitted_item_ids.clone(),
        };

        assert_eq!(delta.apply_to(&base).unwrap(), target);
    }

    #[test]
    fn delta_rejects_the_wrong_exact_base() {
        let mut base = packet();
        base.refresh_digest().unwrap();
        let delta = ContinuationDelta {
            schema_version: CONTINUATION_PACKET_SCHEMA_VERSION,
            source_snapshot: "snapshot-2".into(),
            recipient_identity: base.recipient_identity.clone(),
            resolver_binding: base.resolver_binding.clone(),
            base_packet_digest: ArtifactRevision::from_content(b"other-base").to_string(),
            target_packet_digest: ArtifactRevision::from_content(b"target").to_string(),
            operations: Vec::new(),
            omitted_item_ids: Vec::new(),
        };

        assert!(matches!(
            delta.apply_to(&base),
            Err(ContinuationDeltaError::BasePacketMismatch { .. })
        ));
    }

    #[test]
    fn encoded_byte_budget_counts_complete_packet_metadata() {
        let packet = packet();
        let encoded = serde_json::to_vec(&packet).unwrap();
        assert_eq!(packet.encoded_bytes().unwrap(), encoded.len() as u64);
    }

    #[test]
    fn delta_mode_binds_exact_base_digest() {
        let mode = ContinuationExportMode::Delta {
            base_packet_digest: "sha256:base".into(),
        };
        let encoded = serde_json::to_value(&mode).unwrap();
        assert_eq!(encoded["base_packet_digest"], "sha256:base");
    }

    #[test]
    fn resolvable_reference_names_concrete_resolver_binding() {
        let evidence = ContinuationEvidence::ResolvableReference {
            resolver_binding: "recipient-files".into(),
            digest: "sha256:evidence".into(),
            media_type: "text/plain".into(),
            bytes: 12,
            locator: "src/lib.rs".into(),
        };
        let encoded = serde_json::to_value(&evidence).unwrap();
        assert_eq!(encoded["resolver_binding"], "recipient-files");
    }
}
