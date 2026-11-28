//! Provider-reported accounting, independent of transport and credentials.
//! Unknown values are absent, not zero. Reports describe observations; they
//! neither authorize a request nor implement an automatic retry policy.

/// Token accounting for one model response, not a cumulative account quota.
/// Cache and reasoning tokens may be subsets of the input/output counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

/// An independently resetting provider-reported quota. Scope and unit are
/// open identifiers (e.g. account/requests, model/tokens, subscription/credits).
/// Some providers expose only remaining units or a reset, so all amounts
/// are optional. Timestamps are Unix seconds, never guessed locally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotaWindow {
    pub id: String,
    pub scope: String,
    pub unit: String,
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    pub resets_at: Option<u64>,
}

/// One observation. Absence means the provider did not report that value.
/// Repeated window IDs update the same window within its provider/account scope.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderStatus {
    pub observed_at: Option<u64>,
    pub usage: Option<Usage>,
    pub windows: Vec<QuotaWindow>,
    pub retry_at: Option<u64>,
}

/// Identity is attached by the selected model adapter rather than trusted
/// from the provider payload. No account secrets or arbitrary headers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderReport {
    pub provider: String,
    pub model: String,
    pub status: ProviderStatus,
}
