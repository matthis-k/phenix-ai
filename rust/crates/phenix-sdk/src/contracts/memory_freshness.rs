use super::language::{CodeEntityFacetReference, LANGUAGE_SERVICE};
use phenix_core::{CallableId, ServiceId};
use serde::{Deserialize, Serialize};

pub const MEMORY_VALIDATE_CALLABLE: &str = "memory.validate";

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
    phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum MemoryFreshness {
    #[default]
    Current,
    NeedsValidation,
    Historical,
}

#[derive(
    Clone,
    Debug,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
    phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct MemoryDependencyRevision {
    pub service: ServiceId,
    pub resource: String,
    pub revision: Option<String>,
}

impl MemoryDependencyRevision {
    #[must_use]
    pub fn for_code_facet(reference: &CodeEntityFacetReference) -> Self {
        Self {
            service: ServiceId::parse(LANGUAGE_SERVICE)
                .expect("static language service id is valid"),
            resource: reference.resource(),
            revision: Some(reference.revision.clone()),
        }
    }

    #[must_use]
    pub fn as_code_facet(&self) -> Option<CodeEntityFacetReference> {
        if self.service.as_str() != LANGUAGE_SERVICE {
            return None;
        }
        CodeEntityFacetReference::from_resource(&self.resource, self.revision.clone()?)
    }
}

#[derive(
    Clone,
    Debug,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
    phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct MemoryCanonicalReference {
    pub service: ServiceId,
    pub resource: String,
    pub revision: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryFreshnessRecord {
    pub memory_id: String,
    pub freshness: MemoryFreshness,
    pub changed_at: u64,
    pub dependencies: Vec<MemoryDependencyRevision>,
    pub canonical_reference: Option<MemoryCanonicalReference>,
}

#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
    phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRevalidationOutcome {
    KeepCurrent,
    NeedsValidation,
    Supersede,
    Expire,
    RetainHistorical,
}

#[must_use]
pub fn memory_validate_callable() -> CallableId {
    CallableId::parse(MEMORY_VALIDATE_CALLABLE).expect("static memory callable id is valid")
}

#[cfg(test)]
mod code_dependency_tests {
    use super::super::language::{CodeEntityFacet, LogicalCodeEntity};
    use super::*;

    #[test]
    fn relation_facet_dependency_round_trips_without_losing_identity() {
        let reference = CodeEntityFacetReference {
            entity: LogicalCodeEntity {
                id: "entity-1".into(),
                repository_id: "repo-1".into(),
            },
            facet: CodeEntityFacet::Relation {
                name: "callers".into(),
            },
            revision: "callers-revision-7".into(),
        };
        let dependency = MemoryDependencyRevision::for_code_facet(&reference);

        assert_eq!(dependency.as_code_facet(), Some(reference));
    }
}
