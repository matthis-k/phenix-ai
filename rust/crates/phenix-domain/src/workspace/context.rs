use super::PlanId;
use crate::{ConfigRevisionId, DecisionId, ExecutionId, ObjectiveId, WorkspaceId};
use serde::{Deserialize, Serialize};
use std::collections::{btree_map::Entry, BTreeMap};
use std::fmt::{self, Display, Formatter};
use std::path::PathBuf;

domain_id_type!(ContextResourceId);
domain_id_type!(ContextRevision);
domain_id_type!(FileObservationId);
domain_id_type!(LanguageObservationId);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ExactReference {
    Objective(ObjectiveId),
    Plan(PlanId),
    Decision(DecisionId),
    Execution(ExecutionId),
    Event(u64),
    FileObservation(FileObservationId),
    LanguageObservation(LanguageObservationId),
    Context {
        resource_id: ContextResourceId,
        revision: ContextRevision,
    },
}
impl Display for ExactReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Objective(id) => write!(f, "objective:{id}"),
            Self::Plan(id) => write!(f, "plan:{id}"),
            Self::Decision(id) => write!(f, "decision:{id}"),
            Self::Execution(id) => write!(f, "execution:{id}"),
            Self::Event(sequence) => write!(f, "event:{sequence}"),
            Self::FileObservation(id) => write!(f, "file-observation:{id}"),
            Self::LanguageObservation(id) => write!(f, "lsp-observation:{id}"),
            Self::Context {
                resource_id,
                revision,
            } => write!(f, "context:{resource_id}@{revision}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextResourceKind {
    Skill,
    ProjectDocument,
    Objective,
    Plan,
    Decision,
    Artifact,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextTier {
    MandatoryContent,
    MandatoryMetadata,
    DiscoverableContent,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextScope {
    Workspace { workspace_id: WorkspaceId },
    Execution { execution_id: ExecutionId },
    Objective { objective_id: ObjectiveId },
    Path { path: PathBuf },
    Configuration { revision: ConfigRevisionId },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContextDescriptor {
    pub id: ContextResourceId,
    pub kind: ContextResourceKind,
    pub title: String,
    pub description: String,
    pub scope: ContextScope,
    pub revision: ContextRevision,
    pub estimated_cost: u64,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContextResourceRevision {
    pub descriptor: ContextDescriptor,
    pub tier: ContextTier,
    pub source_ref: ExactReference,
    pub content_identity: ContextRevision,
    pub content: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContextCatalog {
    current: BTreeMap<ContextResourceId, ContextRevision>,
    revisions: BTreeMap<(ContextResourceId, ContextRevision), ContextResourceRevision>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextCatalogError {
    ConflictingRevision {
        id: ContextResourceId,
        revision: ContextRevision,
    },
    UnknownResource(ContextResourceId),
    UnknownRevision {
        id: ContextResourceId,
        revision: ContextRevision,
    },
}
impl Display for ContextCatalogError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConflictingRevision { id, revision } => write!(
                f,
                "context resource {id} revision {revision} changed after registration"
            ),
            Self::UnknownResource(id) => write!(f, "unknown context resource: {id}"),
            Self::UnknownRevision { id, revision } => {
                write!(f, "unknown context resource revision: {id}@{revision}")
            }
        }
    }
}
impl std::error::Error for ContextCatalogError {}
impl ContextCatalog {
    pub fn register_revision(
        &mut self,
        revision: ContextResourceRevision,
    ) -> Result<(), ContextCatalogError> {
        let id = revision.descriptor.id.clone();
        let descriptor_revision = revision.descriptor.revision.clone();
        let key = (id.clone(), descriptor_revision.clone());
        match self.revisions.entry(key) {
            Entry::Occupied(entry) if entry.get() != &revision => {
                return Err(ContextCatalogError::ConflictingRevision {
                    id,
                    revision: descriptor_revision,
                });
            }
            Entry::Occupied(_) => {}
            Entry::Vacant(entry) => {
                entry.insert(revision);
            }
        }
        self.current.insert(id, descriptor_revision);
        Ok(())
    }
    pub fn current_revision(
        &self,
        id: &ContextResourceId,
    ) -> Result<&ContextResourceRevision, ContextCatalogError> {
        let revision = self
            .current
            .get(id)
            .ok_or_else(|| ContextCatalogError::UnknownResource(id.clone()))?;
        self.resolve_revision(id, revision)
    }
    pub fn resolve_revision(
        &self,
        id: &ContextResourceId,
        revision: &ContextRevision,
    ) -> Result<&ContextResourceRevision, ContextCatalogError> {
        self.revisions
            .get(&(id.clone(), revision.clone()))
            .ok_or_else(|| ContextCatalogError::UnknownRevision {
                id: id.clone(),
                revision: revision.clone(),
            })
    }
    pub fn descriptors(&self) -> impl Iterator<Item = &ContextDescriptor> {
        self.current
            .iter()
            .filter_map(|(id, revision)| self.revisions.get(&(id.clone(), revision.clone())))
            .map(|resource| &resource.descriptor)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextInjectionRequester {
    Agent,
    User,
    Orchestration,
    ContextPolicy,
    Hook,
    Frontend,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextInjectionLifetime {
    SingleRequest,
    Execution,
    Objective,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContextInjection {
    pub execution_id: ExecutionId,
    pub source_ref: ExactReference,
    pub source_revision: ContextRevision,
    pub requested_by: ContextInjectionRequester,
    pub reason: String,
    pub lifetime: ContextInjectionLifetime,
    pub content_identity: ContextRevision,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_ids_reject_blank_wire_values() {
        assert!(serde_json::from_str::<ContextResourceId>("\"\"").is_err());
        assert!(serde_json::from_str::<ContextRevision>("\"  \"").is_err());
        assert!(serde_json::from_str::<FileObservationId>("\"\n\"").is_err());
        assert!(serde_json::from_str::<LanguageObservationId>("\"\t\"").is_err());
    }
}
