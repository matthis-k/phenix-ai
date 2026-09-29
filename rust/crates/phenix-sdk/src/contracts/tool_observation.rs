use phenix_core::{ArtifactRevision, ContentReference, PhenixValue};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolObservationStatus {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ToolObservationExactSource {
    Reference { reference: ContentReference },
    Unavailable { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct ToolObservationReuseContext {
    pub provider_identity: String,
    pub implementation_identity: String,
    pub configuration_identity: String,
    pub scope_identity: String,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub enum ToolObservationInvalidation {
    Volatile,
    Dependencies {
        context: ToolObservationReuseContext,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct ToolObservation {
    pub occurrence_id: String,
    pub status: ToolObservationStatus,
    pub model_view: PhenixValue,
    pub model_view_bytes: u64,
    pub model_view_complete: bool,
    pub content_identity: ArtifactRevision,
    pub exact_source: ToolObservationExactSource,
    pub source_revision: Option<String>,
    pub invalidation: ToolObservationInvalidation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolObservationValidationError {
    MissingOccurrenceId,
    MissingRecoveryFailureReason,
    ExactReferenceIdentityMismatch {
        expected: ArtifactRevision,
        observed: ArtifactRevision,
    },
}

impl ToolObservation {
    pub fn validate(&self) -> Result<(), ToolObservationValidationError> {
        if self.occurrence_id.trim().is_empty() {
            return Err(ToolObservationValidationError::MissingOccurrenceId);
        }
        match &self.exact_source {
            ToolObservationExactSource::Reference { reference }
                if reference.digest != self.content_identity =>
            {
                Err(
                    ToolObservationValidationError::ExactReferenceIdentityMismatch {
                        expected: self.content_identity.clone(),
                        observed: reference.digest.clone(),
                    },
                )
            }
            ToolObservationExactSource::Unavailable { reason } if reason.trim().is_empty() => {
                Err(ToolObservationValidationError::MissingRecoveryFailureReason)
            }
            _ => Ok(()),
        }
    }

    #[must_use]
    pub fn reusable_under(&self, current: &ToolObservationReuseContext) -> bool {
        matches!(
            &self.invalidation,
            ToolObservationInvalidation::Dependencies { context } if context == current
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::ContentLocator;

    fn revision(value: &str) -> ArtifactRevision {
        value.parse().unwrap()
    }

    fn reference(digest: ArtifactRevision) -> ContentReference {
        ContentReference {
            digest,
            media_type: "application/octet-stream".into(),
            bytes: 4,
            locator: ContentLocator::Service {
                service: "phenix.artifacts@1".into(),
                resource: "artifact:test".into(),
            },
        }
    }

    fn reuse_context() -> ToolObservationReuseContext {
        ToolObservationReuseContext {
            provider_identity: "provider".into(),
            implementation_identity: "implementation".into(),
            configuration_identity: "configuration".into(),
            scope_identity: "scope".into(),
            dependencies: BTreeMap::from([("workspace:file".into(), "rev-1".into())]),
        }
    }

    fn observation(invalidation: ToolObservationInvalidation) -> ToolObservation {
        let content_identity =
            revision("sha256:0000000000000000000000000000000000000000000000000000000000000000");
        ToolObservation {
            occurrence_id: "call-1".into(),
            status: ToolObservationStatus::Succeeded,
            model_view: PhenixValue::String("view".into()),
            model_view_bytes: 4,
            model_view_complete: false,
            exact_source: ToolObservationExactSource::Reference {
                reference: reference(content_identity.clone()),
            },
            content_identity,
            source_revision: Some("workspace-rev-1".into()),
            invalidation,
        }
    }

    #[test]
    fn exact_reference_must_match_observed_content_identity() {
        let mut value = observation(ToolObservationInvalidation::Volatile);
        value.exact_source = ToolObservationExactSource::Reference {
            reference: reference(revision(
                "sha256:1111111111111111111111111111111111111111111111111111111111111111",
            )),
        };

        assert!(matches!(
            value.validate(),
            Err(ToolObservationValidationError::ExactReferenceIdentityMismatch { .. })
        ));
    }

    #[test]
    fn unavailable_exact_source_requires_an_explicit_reason() {
        let mut value = observation(ToolObservationInvalidation::Volatile);
        value.exact_source = ToolObservationExactSource::Unavailable {
            reason: String::new(),
        };

        assert_eq!(
            value.validate(),
            Err(ToolObservationValidationError::MissingRecoveryFailureReason)
        );
    }

    #[test]
    fn volatile_observations_are_never_reused_from_matching_content_alone() {
        let value = observation(ToolObservationInvalidation::Volatile);

        assert!(!value.reusable_under(&reuse_context()));
    }

    #[test]
    fn reusable_observations_require_the_full_declared_invalidation_context() {
        let context = reuse_context();
        let value = observation(ToolObservationInvalidation::Dependencies {
            context: context.clone(),
        });
        assert!(value.reusable_under(&context));

        let mut changed = context;
        changed
            .dependencies
            .insert("workspace:file".into(), "rev-2".into());
        assert!(!value.reusable_under(&changed));
    }
}
