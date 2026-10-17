//! Selected P2 summarization contract. Summaries are untrusted proposed
//! content until the caller verifies all original Basic journal bytes and
//! persists a reversible checkpoint through the selected CAS provider.
use crate::JournalSpan;
use phenix_basic_contracts::{Event, ModelRequest};
use phenix_sdk::Portable;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct CompactionSummaryRequest {
    /// Full authoritative Basic Model request, before any elision.
    pub request: ModelRequest,
    /// A bounded, contiguous journal read through the selected EventJournal.
    pub journal: Vec<Event>,
    pub source: JournalSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct CompactionSummary {
    /// This acknowledgement is checked against the exact selected source.
    pub source: JournalSpan,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum CompactionSummaryProblem {
    InvalidSource,
    ScopeDenied,
    ProviderFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct CompactionSummaryError {
    pub problem: CompactionSummaryProblem,
    pub message: String,
}

/// The summarizer produces a proposed, source-cited summary, not permission
/// to erase events, publish a checkpoint or execute a Model request. The
/// selected graph/adapter independently verifies the full Basic history and
/// source hashes, uses CompactionStore CAS and remeasures the final request.
#[phenix_sdk::contract(id = "phenix.advanced.compaction-summary@1")]
pub trait CompactionSummarizer {
    fn summarize(
        &self,
        request: CompactionSummaryRequest,
    ) -> Result<CompactionSummary, CompactionSummaryError>;
}
