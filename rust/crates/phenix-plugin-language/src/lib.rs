#![forbid(unsafe_code)]
mod component;
mod implementation;
pub use component::*;
pub use implementation::*;
pub use phenix_sdk::{
    CodeChangedNeighborhood, CodeEntityEditEvidence, CodeEntityEditResult,
    CodeEntityEditValidation, CodeEntityFacet, CodeEntityFacetReference, CodeEntityFacetRevisions,
    CodeEntityInsertPosition, CodeEntityLineage, CodeEntityLineageConfidence,
    CodeEntityLineageKind, CodeEntityProviderEditValidationFact,
    CodeEntityProviderEditValidationFactBatch, CodeEntityProviderRelationFact,
    CodeEntityProviderRelationFactBatch, CodeEntityRelationKind, CodeEntityRelationTarget,
    CodeEntityRelations, CodeEntityRevision, DiagnosticsResult, DocumentProvenance,
    FileRevisionFallback, LanguageCommand, LanguageDocumentIdentity, LanguageObservation,
    LanguageOperationKind, LanguageOperationResult, LanguageProviderEpoch, LanguageResponse,
    LogicalCodeEntity, ProviderEpoch, LANGUAGE_SERVICE,
};
