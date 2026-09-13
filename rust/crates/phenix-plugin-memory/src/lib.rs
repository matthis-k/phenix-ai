#![forbid(unsafe_code)]

mod association_store;
mod component;
mod error;
mod freshness;
mod implementation;
mod persistence;
mod retrieval;

pub use component::*;
pub use implementation::{memory_factory, memory_manifest};
pub use phenix_sdk::{
    context_compaction_service, context_expansion_service, memory_consolidate_callable,
    memory_extract_callable, memory_resolve_callable, memory_service, memory_summarize_callable,
    memory_validate_callable, CompactContextItem, ContextCheckpoint, ContextCompactionCommand,
    ContextCompactionInterface, ContextCompactionRequest, ContextCompactionResponse,
    ContextExpansionCommand, ContextExpansionInterface, ContextExpansionResponse,
    MemoryCanonicalReference, MemoryCommand, MemoryDependencyRevision, MemoryExpansion,
    MemoryFreshness, MemoryFreshnessRecord, MemoryInterface, MemoryKind, MemoryNode,
    MemoryRecallQuery, MemoryRecord, MemoryResponse, MemoryRevalidationOutcome, MemoryScope,
    MemorySourceReference, CONTEXT_COMPACTION_SERVICE, CONTEXT_EXPANSION_SERVICE,
    MEMORY_CONSOLIDATE_CALLABLE, MEMORY_EXTRACT_CALLABLE, MEMORY_RESOLVE_CALLABLE, MEMORY_SERVICE,
    MEMORY_SUMMARIZE_CALLABLE, MEMORY_VALIDATE_CALLABLE,
};

#[cfg(test)]
mod embedding_integration;
#[cfg(test)]
mod freshness_integration;
#[cfg(test)]
mod maintenance_integration;
#[cfg(test)]
mod provenance_integration;
#[cfg(test)]
mod reranking_integration;
#[cfg(test)]
mod restart_integration;
#[cfg(test)]
mod revalidation_failure_integration;
#[cfg(test)]
mod supersession_integration;
#[cfg(test)]
mod tests;
