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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "projection", rename_all = "snake_case")]
pub enum ToolObservationProjection {
    View {
        occurrence_id: String,
        value: PhenixValue,
        complete: bool,
        content_identity: ArtifactRevision,
        exact_source: ToolObservationExactSource,
    },
    Reference {
        occurrence_id: String,
        content_identity: ArtifactRevision,
        exact_source: ToolObservationExactSource,
    },
    Reused {
        occurrence_id: String,
        prior_occurrence_id: String,
        content_identity: ArtifactRevision,
        exact_source: ToolObservationExactSource,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolObservationProjectionError {
    Invalid(ToolObservationValidationError),
    ModelViewExceedsBound { observed: u64, limit: u64 },
    ExactRecoveryUnavailable,
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

    #[must_use]
    pub fn reusable_from(&self, prior: &Self) -> bool {
        self.status == prior.status
            && self.content_identity == prior.content_identity
            && matches!(
                (&self.invalidation, &prior.invalidation),
                (
                    ToolObservationInvalidation::Dependencies { context: current },
                    ToolObservationInvalidation::Dependencies { context: previous },
                ) if current == previous
            )
    }

    pub fn project(
        &self,
        prior: Option<&Self>,
        max_model_view_bytes: u64,
        reduction_enabled: bool,
    ) -> Result<ToolObservationProjection, ToolObservationProjectionError> {
        self.validate()
            .map_err(ToolObservationProjectionError::Invalid)?;

        if let Some(prior) = prior.filter(|prior| self.reusable_from(prior)) {
            return Ok(ToolObservationProjection::Reused {
                occurrence_id: self.occurrence_id.clone(),
                prior_occurrence_id: prior.occurrence_id.clone(),
                content_identity: self.content_identity.clone(),
                exact_source: self.exact_source.clone(),
            });
        }

        if self.model_view_bytes <= max_model_view_bytes {
            return Ok(ToolObservationProjection::View {
                occurrence_id: self.occurrence_id.clone(),
                value: self.model_view.clone(),
                complete: self.model_view_complete,
                content_identity: self.content_identity.clone(),
                exact_source: self.exact_source.clone(),
            });
        }

        if !reduction_enabled {
            return Err(ToolObservationProjectionError::ModelViewExceedsBound {
                observed: self.model_view_bytes,
                limit: max_model_view_bytes,
            });
        }

        if !matches!(
            self.exact_source,
            ToolObservationExactSource::Reference { .. }
        ) {
            return Err(ToolObservationProjectionError::ExactRecoveryUnavailable);
        }

        Ok(ToolObservationProjection::Reference {
            occurrence_id: self.occurrence_id.clone(),
            content_identity: self.content_identity.clone(),
            exact_source: self.exact_source.clone(),
        })
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
    #[test]
    fn unchanged_reusable_observation_projects_as_reference_to_prior_occurrence() {
        let context = reuse_context();
        let first = observation(ToolObservationInvalidation::Dependencies {
            context: context.clone(),
        });
        let mut second = observation(ToolObservationInvalidation::Dependencies { context });
        second.occurrence_id = "call-2".into();

        assert!(matches!(
            second.project(Some(&first), 1024, true).unwrap(),
            ToolObservationProjection::Reused {
                prior_occurrence_id,
                ..
            } if prior_occurrence_id == "call-1"
        ));
    }

    #[test]
    fn volatile_observation_never_reuses_matching_bytes() {
        let first = observation(ToolObservationInvalidation::Volatile);
        let mut second = observation(ToolObservationInvalidation::Volatile);
        second.occurrence_id = "call-2".into();

        assert!(matches!(
            second.project(Some(&first), 1024, true).unwrap(),
            ToolObservationProjection::View { .. }
        ));
    }

    #[test]
    fn oversized_view_requires_recovery_or_typed_exhaustion() {
        let mut value = observation(ToolObservationInvalidation::Volatile);
        value.model_view_bytes = 4096;

        assert!(matches!(
            value.project(None, 1024, false),
            Err(ToolObservationProjectionError::ModelViewExceedsBound {
                observed: 4096,
                limit: 1024,
            })
        ));
        assert!(matches!(
            value.project(None, 1024, true).unwrap(),
            ToolObservationProjection::Reference { .. }
        ));

        value.exact_source = ToolObservationExactSource::Unavailable {
            reason: "quota".into(),
        };
        assert_eq!(
            value.project(None, 1024, true),
            Err(ToolObservationProjectionError::ExactRecoveryUnavailable)
        );
    }

}
