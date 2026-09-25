use phenix_core::{
    ComponentInterface, Exact, InterfaceId, PhenixValue, Project, Type, ValueCodec, ValueError,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, num::NonZeroU64};

pub const LANGUAGE_SERVICE: &str = "phenix.language@1";

pub struct LanguageInterface;

impl ComponentInterface for LanguageInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(LANGUAGE_SERVICE).expect("static language interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<LanguageCommand, LanguageResponse>()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(rename_all = "snake_case")]
pub enum LanguageOperationKind {
    Definition,
    References,
    Implementations,
    Hover,
    DocumentSymbols,
    WorkspaceSymbols,
    Diagnostics,
    CallHierarchy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(rename_all = "snake_case")]
pub enum DocumentProvenance {
    WorkspaceBacked,
    FrontendUnsaved,
    MixedOrUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct LanguageDocumentIdentity {
    pub path: String,
    pub file_version: Option<String>,
    pub provenance: DocumentProvenance,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct ProviderEpoch(NonZeroU64);

impl ProviderEpoch {
    pub fn new(value: u64) -> Result<Self, &'static str> {
        value.try_into()
    }

    #[must_use]
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

impl TryFrom<u64> for ProviderEpoch {
    type Error = &'static str;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or("language provider epoch must be non-zero")
    }
}

impl From<ProviderEpoch> for u64 {
    fn from(value: ProviderEpoch) -> Self {
        value.get()
    }
}

impl ValueCodec for ProviderEpoch {
    fn phenix_type() -> Type {
        <u64 as ValueCodec>::phenix_type()
    }

    fn to_value(&self) -> PhenixValue {
        <u64 as ValueCodec>::to_value(&self.get())
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        let value = <u64 as ValueCodec>::from_value(value)?;
        Self::try_from(value).map_err(|error| ValueError::InvalidValue(error.into()))
    }

    fn project_from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        let value = <u64 as ValueCodec>::project_from_value(value)?;
        Self::try_from(value).map_err(|error| ValueError::InvalidValue(error.into()))
    }
}

impl From<&ProviderEpoch> for PhenixValue {
    fn from(value: &ProviderEpoch) -> Self {
        value.to_value()
    }
}

impl<'value> TryFrom<Exact<&'value PhenixValue>> for ProviderEpoch {
    type Error = ValueError;

    fn try_from(value: Exact<&'value PhenixValue>) -> Result<Self, Self::Error> {
        Self::from_value(value.0)
    }
}

impl<'value> TryFrom<Project<&'value PhenixValue>> for ProviderEpoch {
    type Error = ValueError;

    fn try_from(value: Project<&'value PhenixValue>) -> Result<Self, Self::Error> {
        Self::project_from_value(value.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct LanguageProviderEpoch {
    pub workspace_id: String,
    pub provider_id: String,
    pub epoch: ProviderEpoch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct LanguageOperationResult {
    pub operation: LanguageOperationKind,
    pub payload: PhenixValue,
    pub documents: Vec<LanguageDocumentIdentity>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum DiagnosticsResult {
    Diagnostics {
        payload: PhenixValue,
        documents: Vec<LanguageDocumentIdentity>,
    },
}

impl DiagnosticsResult {
    #[must_use]
    pub fn documents(&self) -> &[LanguageDocumentIdentity] {
        match self {
            Self::Diagnostics { documents, .. } => documents,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct LanguageObservation {
    pub id: String,
    pub execution_id: String,
    pub workspace_id: String,
    pub provider_id: String,
    pub provider_epoch: ProviderEpoch,
    pub result: LanguageOperationResult,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct LogicalCodeEntity {
    pub id: String,
    pub repository_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct CodeEntityFacetRevisions {
    pub existence: String,
    pub name_location: String,
    pub signature: Option<String>,
    pub body: Option<String>,
    #[serde(default)]
    pub relations: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct CodeEntityFacetChanges {
    pub existence: bool,
    pub name_location: bool,
    pub signature: bool,
    pub body: bool,
    #[serde(default)]
    pub relations: Vec<String>,
}

impl CodeEntityFacetChanges {
    #[must_use]
    pub fn between(
        previous: &CodeEntityFacetRevisions,
        current: &CodeEntityFacetRevisions,
    ) -> Self {
        let mut relation_names = previous
            .relations
            .keys()
            .chain(current.relations.keys())
            .cloned()
            .collect::<Vec<_>>();
        relation_names.sort();
        relation_names.dedup();
        let relations = relation_names
            .into_iter()
            .filter(|name| previous.relations.get(name) != current.relations.get(name))
            .collect();
        Self {
            existence: previous.existence != current.existence,
            name_location: previous.name_location != current.name_location,
            signature: previous.signature != current.signature,
            body: previous.body != current.body,
            relations,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "facet", rename_all = "snake_case", deny_unknown_fields)]
pub enum CodeEntityFacet {
    Existence,
    NameLocation,
    Signature,
    Body,
    Relation { name: String },
}

pub const CODE_ENTITY_FACET_RESOURCE_PREFIX: &str = "code-facet:";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct CodeEntityFacetResource {
    pub entity: LogicalCodeEntity,
    pub facet: CodeEntityFacet,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct CodeEntityFacetReference {
    pub entity: LogicalCodeEntity,
    pub facet: CodeEntityFacet,
    pub revision: String,
}

impl CodeEntityFacetReference {
    #[must_use]
    pub fn resource(&self) -> String {
        let resource = CodeEntityFacetResource {
            entity: self.entity.clone(),
            facet: self.facet.clone(),
        };
        format!(
            "{CODE_ENTITY_FACET_RESOURCE_PREFIX}{}",
            serde_json::to_string(&resource).expect("code facet resource is serializable")
        )
    }

    #[must_use]
    pub fn from_resource(resource: &str, revision: String) -> Option<Self> {
        let encoded = resource.strip_prefix(CODE_ENTITY_FACET_RESOURCE_PREFIX)?;
        let resource: CodeEntityFacetResource = serde_json::from_str(encoded).ok()?;
        Some(Self {
            entity: resource.entity,
            facet: resource.facet,
            revision,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct CodeEntityRevision {
    pub entity: LogicalCodeEntity,
    pub revision: String,
    pub sequence: u64,
    pub document: LanguageDocumentIdentity,
    pub symbol: Option<String>,
    pub name: String,
    pub signature_identity: Option<String>,
    pub body_identity: Option<String>,
    pub provider_id: String,
    pub provider_epoch: ProviderEpoch,
    pub facets: CodeEntityFacetRevisions,
}

impl CodeEntityRevision {
    #[must_use]
    pub fn facet_reference(&self, facet: CodeEntityFacet) -> Option<CodeEntityFacetReference> {
        let revision = match &facet {
            CodeEntityFacet::Existence => Some(self.facets.existence.as_str()),
            CodeEntityFacet::NameLocation => Some(self.facets.name_location.as_str()),
            CodeEntityFacet::Signature => self.facets.signature.as_deref(),
            CodeEntityFacet::Body => self.facets.body.as_deref(),
            CodeEntityFacet::Relation { name } => {
                self.facets.relations.get(name).map(String::as_str)
            }
        }?;
        Some(CodeEntityFacetReference {
            entity: self.entity.clone(),
            facet,
            revision: revision.to_owned(),
        })
    }
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum CodeEntityLineageKind {
    Rename,
    Move,
    Replacement,
    Extract,
    Split,
    Merge,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum CodeEntityLineageConfidence {
    Confirmed,
    Tentative,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct CodeEntityLineage {
    pub from_entity_id: String,
    pub to_entity_id: String,
    pub kind: CodeEntityLineageKind,
    pub confidence: CodeEntityLineageConfidence,
    #[serde(default)]
    pub evidence_observation_ids: Vec<String>,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum CodeIdentityContinuityStatus {
    Available,
    Rebuilding,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct CodeIdentityContinuityState {
    pub repository_id: String,
    pub status: CodeIdentityContinuityStatus,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum LanguageCommand {
    ActivateProvider {
        workspace_id: String,
        provider_id: String,
        epoch: ProviderEpoch,
    },
    EndProvider {
        workspace_id: String,
        provider_id: String,
        epoch: ProviderEpoch,
    },
    PublishDiagnostics {
        workspace_id: String,
        provider_id: String,
        epoch: ProviderEpoch,
        result: DiagnosticsResult,
    },
    CurrentDiagnostics {
        workspace_id: String,
    },
    Consume {
        observation_id: String,
        execution_id: String,
        workspace_id: String,
        provider_id: String,
        epoch: ProviderEpoch,
        result: LanguageOperationResult,
    },
    RecordEntityRevision {
        revision: CodeEntityRevision,
    },
    RecordEntityLineage {
        repository_id: String,
        lineage: CodeEntityLineage,
    },
    GetEntityLineage {
        repository_id: String,
        from_entity_id: String,
        to_entity_id: String,
        kind: CodeEntityLineageKind,
    },
    GetEntityRevision {
        repository_id: String,
        entity_id: String,
    },
    GetEntityFacet {
        repository_id: String,
        entity_id: String,
        facet: CodeEntityFacet,
    },
    GetEntityFacetChanges {
        repository_id: String,
        entity_id: String,
        from_revision: String,
    },
    SetIdentityContinuity {
        state: CodeIdentityContinuityState,
    },
    GetIdentityContinuity {
        repository_id: String,
    },
    GetObservation {
        observation_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum LanguageResponse {
    Provider {
        epoch: Option<LanguageProviderEpoch>,
    },
    Diagnostics {
        result: Option<DiagnosticsResult>,
    },
    Observation {
        observation: Option<LanguageObservation>,
    },
    EntityRevision {
        revision: Option<CodeEntityRevision>,
    },
    EntityLineage {
        lineage: Option<CodeEntityLineage>,
    },
    EntityFacet {
        reference: Option<CodeEntityFacetReference>,
    },
    EntityFacetChanges {
        changes: Option<CodeEntityFacetChanges>,
    },
    IdentityContinuity {
        state: Option<CodeIdentityContinuityState>,
    },
}

#[cfg(test)]
mod code_entity_facet_resource_tests {
    use super::*;

    #[test]
    fn facet_resource_round_trip_is_revision_independent_and_lossless() {
        let reference = CodeEntityFacetReference {
            entity: LogicalCodeEntity {
                id: "entity/with:delimiters".into(),
                repository_id: "repo/with:delimiters".into(),
            },
            facet: CodeEntityFacet::Relation {
                name: "callers/transitive".into(),
            },
            revision: "relation-revision-1".into(),
        };

        let resource = reference.resource();
        assert!(resource.starts_with(CODE_ENTITY_FACET_RESOURCE_PREFIX));
        assert_eq!(
            CodeEntityFacetReference::from_resource(&resource, reference.revision.clone()),
            Some(reference)
        );
    }

    #[test]
    fn non_code_resources_do_not_decode_as_code_facets() {
        assert!(CodeEntityFacetReference::from_resource("turn/1", "revision-1".into()).is_none());
    }
}
