//! Generation-local provider registration staging for existing M1 graph events.
//! A declaration is data, not authorization or an executable provider.
use phenix_provider_builtins::merge_registration_pages;
use phenix_provider_contracts::{ProviderDescriptor, ProviderError, ProviderErrorCode, ProviderPage};

fn issue(code: ProviderErrorCode, message: &str) -> ProviderError {
    ProviderError { code, message: message.into() }
}

/// Private to the provider host. Never shared with Storage or the kernel.
#[derive(Default)]
pub(crate) struct Registry {
    pending: Vec<ProviderDescriptor>,
    published: Option<Vec<ProviderDescriptor>>,
}

impl Registry {
    pub(crate) fn is_populating(&self) -> bool {
        !self.pending.is_empty() && self.published.is_none()
    }

    /// Check the entire candidate snapshot before mutating owner state.
    /// Multiple selected contributors may supply distinct connections under
    /// the same provider identity; collisions are rejected, not overwritten.
    pub(crate) fn register(&mut self, provider: ProviderDescriptor) -> Result<(), ProviderError> {
        if self.published.is_some() {
            return Err(issue(ProviderErrorCode::Conflict, "provider catalog is already published"));
        }
        let mut candidate = self.pending.clone();
        candidate.push(provider);
        merge_registration_pages([ProviderPage {
            providers: candidate.clone(),
            revision: 1,
        }])
        .map_err(|_| issue(ProviderErrorCode::InvalidInput, "invalid or conflicting provider declaration"))?;
        self.pending = candidate;
        Ok(())
    }

    /// Called only after the population graph has completed successfully.
    /// Until then no partially registered provider can be selected.
    pub(crate) fn publish(&mut self) -> Result<ProviderPage, ProviderError> {
        if self.published.is_some() {
            return Err(issue(ProviderErrorCode::Conflict, "provider catalog is already published"));
        }
        if self.pending.is_empty() {
            return Err(issue(ProviderErrorCode::Unavailable, "no provider declarations registered"));
        }
        let providers = merge_registration_pages([ProviderPage {
            providers: self.pending.clone(),
            revision: 1,
        }])
        .map_err(|_| issue(ProviderErrorCode::InvalidInput, "invalid provider catalog"))?;
        self.published = Some(providers.clone());
        self.pending.clear();
        Ok(ProviderPage { providers, revision: 1 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_provider_builtins::registrations;

    #[test]
    fn registering_two_providers_publishes_a_deterministic_snapshot() {
        let providers = registrations();
        let mut registry = Registry::default();
        registry.register(providers[1].clone()).unwrap();
        assert!(registry.is_populating());
        registry.register(providers[0].clone()).unwrap();
        let catalog = registry.publish().unwrap();
        assert!(!registry.is_populating());
        assert_eq!(catalog.providers.len(), 2);
        assert_eq!(catalog.providers[0].id, "openai");
        assert_eq!(catalog.providers[1].id, "openrouter");
        assert_eq!(catalog.revision, 1);
        assert_eq!(registry.publish().unwrap_err().code, ProviderErrorCode::Conflict);
        assert_eq!(registry.register(providers[2].clone()).unwrap_err().code, ProviderErrorCode::Conflict);
    }

    #[test]
    fn rejected_registration_does_not_change_staged_snapshot() {
        let providers = registrations();
        let mut registry = Registry::default();
        registry.register(providers[0].clone()).unwrap();
        assert_eq!(registry.register(providers[0].clone()).unwrap_err().code, ProviderErrorCode::InvalidInput);
        let catalog = registry.publish().unwrap();
        assert_eq!(catalog.providers, vec![providers[0].clone()]);
    }

    #[test]
    fn split_provider_connections_merge_but_wrong_label_fails() {
        let providers = registrations();
        let mut api = providers[0].clone();
        api.connections.truncate(1);
        let mut oauth = providers[0].clone();
        oauth.connections.remove(0);
        let mut registry = Registry::default();
        registry.register(api).unwrap();
        let mut wrong_label = oauth.clone();
        wrong_label.label = "Spoofed OpenAI".into();
        assert_eq!(registry.register(wrong_label).unwrap_err().code, ProviderErrorCode::InvalidInput);
        registry.register(oauth).unwrap();
        assert_eq!(registry.publish().unwrap().providers[0].connections.len(), 2);
    }

    #[test]
    fn absent_declarations_never_publish() {
        assert_eq!(Registry::default().publish().unwrap_err().code, ProviderErrorCode::Unavailable);
    }
}
