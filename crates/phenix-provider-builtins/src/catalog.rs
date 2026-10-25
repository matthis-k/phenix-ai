//! Shared construction of authorized, discovered model pages.
//!
//! A provider passes the result of its explicitly selected discovery adapter
//! only *after* the runtime has authorized the exact named account. Published
//! model data cannot override protocol, connection, endpoint, or profile.
use phenix_provider_contracts::{
    ModelOrigin, ModelPage, ProviderError, ProviderErrorCode, ProviderModel,
};
use std::collections::BTreeSet;

/// Provider-declared executable model eligibility; it never grants authority.
/// Standard providers with a verified executable model catalog may use `|_| true`.
pub fn discovered_model_page(
    profile: &str,
    connection: &str,
    protocol: &str,
    endpoint: &str,
    limit: u64,
    models: impl IntoIterator<Item = (String, String)>,
    eligible: impl Fn(&str) -> bool,
) -> Result<ModelPage, ProviderError> {
    let invalid = || ProviderError {
        code: ProviderErrorCode::InvalidInput,
        message: "invalid model discovery page".into(),
    };
    if [profile, connection, protocol, endpoint]
        .iter()
        .any(|value| value.is_empty() || value.len() > 128 || value.chars().any(char::is_control))
        || !(1..=256).contains(&limit)
    {
        return Err(invalid());
    }
    let mut revision = 14695981039346656037u64;
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    for (id, label) in models {
        if id.is_empty()
            || id.len() > 128
            || id.chars().any(char::is_control)
            || label.is_empty()
            || label.len() > 128
            || label.chars().any(char::is_control)
            || !seen.insert(id.clone())
            || seen.len() > 256
        {
            return Err(invalid());
        }
        // Revision covers all discovered models, not only selectable ones.
        // This preserves the existing API and subscription snapshot semantics.
        for byte in id.bytes().chain([0]) {
            revision = (revision ^ u64::from(byte)).wrapping_mul(1099511628211);
        }
        if eligible(&id) {
            if selected.len() >= limit as usize {
                return Err(ProviderError {
                    code: ProviderErrorCode::Unavailable,
                    message: "authorized model catalog exceeds requested page budget".into(),
                });
            }
            selected.push(ProviderModel {
                id,
                label,
                connection_id: connection.into(),
                protocol_id: protocol.into(),
                endpoint_id: endpoint.into(),
                capabilities: Vec::new(),
                origin: ModelOrigin::Discovered,
                selectable: true,
            });
        }
    }
    Ok(ModelPage {
        profile_id: Some(profile.into()),
        models: selected,
        next_cursor: None,
        revision: revision.max(1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(
        limit: u64,
        models: &[(&str, &str)],
        eligible: impl Fn(&str) -> bool,
    ) -> Result<ModelPage, ProviderError> {
        discovered_model_page(
            "work",
            "api",
            "openai-responses",
            "responses",
            limit,
            models
                .iter()
                .map(|(id, label)| ((*id).to_owned(), (*label).to_owned())),
            eligible,
        )
    }

    #[test]
    fn standard_catalog_is_bound_to_the_supplied_account_and_protocol() {
        let page = page(3, &[("gpt-5", "GPT 5"), ("gpt-4o", "GPT 4o")], |_| true)
            .unwrap();
        assert_eq!(page.profile_id.as_deref(), Some("work"));
        assert_eq!(page.models.len(), 2);
        assert!(page.models.iter().all(|model| {
            model.connection_id == "api"
                && model.protocol_id == "openai-responses"
                && model.endpoint_id == "responses"
                && model.selectable
        }));
        assert_ne!(page.revision, 0);
    }

    #[test]
    fn policy_filters_models_without_hiding_revision_changes() {
        let catalog = |second: &str| {
            page(4, &[("gpt-5", "GPT 5"), (second, second)], |id| {
                id.starts_with("gpt-")
            })
            .unwrap()
        };
        let one = catalog("embedding-1");
        let two = catalog("embedding-2");
        assert_eq!(one.models.len(), 1);
        assert_ne!(one.revision, two.revision);
    }

    #[test]
    fn fails_closed_for_duplicates_over_budget_and_untrusted_labels() {
        assert_eq!(
            page(0, &[("gpt-5", "GPT 5")], |_| true).unwrap_err().code,
            ProviderErrorCode::InvalidInput
        );
        assert_eq!(
            page(1, &[("gpt-5", "one"), ("gpt-5", "two")], |_| false)
                .unwrap_err()
                .code,
            ProviderErrorCode::InvalidInput
        );
        assert_eq!(
            page(1, &[("gpt-5", "one"), ("gpt-4o", "two")], |_| true)
                .unwrap_err()
                .code,
            ProviderErrorCode::Unavailable
        );
        assert_eq!(
            page(2, &[("gpt-5", "bad\nlabel")], |_| true)
                .unwrap_err()
                .code,
            ProviderErrorCode::InvalidInput
        );
    }
}
