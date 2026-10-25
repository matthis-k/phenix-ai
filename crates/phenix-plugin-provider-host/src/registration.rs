//! Generation-local staging for existing M1 provider-population graph events.
//! A declaration is data, not authorization or an executable provider.
use phenix_provider_builtins::merge_registration_pages;
use phenix_provider_contracts::{
    ProviderDescriptor, ProviderError, ProviderErrorCode, ProviderPage,
};

fn issue(code: ProviderErrorCode, message: &str) -> ProviderError {
    ProviderError {
        code,
        message: message.into(),
    }
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Collecting(Vec<ProviderDescriptor>),
    Published,
}

/// Owner-private state. No Storage mirror, kernel registry or partial catalog.
#[derive(Default)]
pub(crate) struct Registry {
    phase: Phase,
}

impl Registry {
    pub(crate) fn is_populating(&self) -> bool {
        matches!(&self.phase, Phase::Collecting(_))
    }

    /// Called by the authorized controller *before* triggering graph nodes.
    pub(crate) fn begin(&mut self) -> Result<(), ProviderError> {
        if !matches!(&self.phase, Phase::Idle) {
            return Err(issue(
                ProviderErrorCode::Conflict,
                "provider population already begun",
            ));
        }
        self.phase = Phase::Collecting(Vec::new());
        Ok(())
    }

    /// Verify the entire prospective snapshot before changing the live state.
    /// Distinct connections can share a provider ID; conflicting ones cannot.
    pub(crate) fn register(&mut self, provider: ProviderDescriptor) -> Result<(), ProviderError> {
        let Phase::Collecting(pending) = &mut self.phase else {
            return Err(issue(
                ProviderErrorCode::Conflict,
                "provider population is not active",
            ));
        };
        let mut candidate = pending.clone();
        candidate.push(provider);
        merge_registration_pages([ProviderPage {
            providers: candidate.clone(),
            revision: 1,
        }])
        .map_err(|_| {
            issue(
                ProviderErrorCode::InvalidInput,
                "invalid or conflicting provider declaration",
            )
        })?;
        *pending = candidate;
        Ok(())
    }

    /// Called only after all triggering graph nodes completed successfully.
    /// A failed graph may abort; it cannot expose intermediate registrations.
    pub(crate) fn publish(&mut self) -> Result<ProviderPage, ProviderError> {
        let Phase::Collecting(pending) = &self.phase else {
            return Err(issue(
                ProviderErrorCode::Conflict,
                "provider population is not active",
            ));
        };
        if pending.is_empty() {
            return Err(issue(
                ProviderErrorCode::Unavailable,
                "no provider declarations registered",
            ));
        }
        let providers = merge_registration_pages([ProviderPage {
            providers: pending.clone(),
            revision: 1,
        }])
        .map_err(|_| issue(ProviderErrorCode::InvalidInput, "invalid provider catalog"))?;
        self.phase = Phase::Published;
        Ok(ProviderPage {
            providers,
            revision: 1,
        })
    }

    pub(crate) fn abort(&mut self) -> Result<(), ProviderError> {
        if !matches!(&self.phase, Phase::Collecting(_)) {
            return Err(issue(
                ProviderErrorCode::Conflict,
                "provider population is not active",
            ));
        }
        self.phase = Phase::Idle;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_provider_builtins::registrations;

    #[test]
    fn graph_population_is_invisible_until_published() {
        let providers = registrations();
        let mut registry = Registry::default();
        assert_eq!(
            registry.register(providers[0].clone()).unwrap_err().code,
            ProviderErrorCode::Conflict
        );
        registry.begin().unwrap();
        assert!(registry.is_populating());
        assert_eq!(
            registry.begin().unwrap_err().code,
            ProviderErrorCode::Conflict
        );
        registry.register(providers[1].clone()).unwrap();
        registry.register(providers[0].clone()).unwrap();
        let catalog = registry.publish().unwrap();
        assert!(!registry.is_populating());
        assert_eq!(catalog.providers.len(), 2);
        assert_eq!(catalog.providers[0].id, "openai");
        assert_eq!(catalog.providers[1].id, "openrouter");
        assert_eq!(catalog.revision, 1);
        assert_eq!(
            registry.publish().unwrap_err().code,
            ProviderErrorCode::Conflict
        );
        assert_eq!(
            registry.register(providers[2].clone()).unwrap_err().code,
            ProviderErrorCode::Conflict
        );
        assert_eq!(
            registry.abort().unwrap_err().code,
            ProviderErrorCode::Conflict
        );
    }

    #[test]
    fn rejected_registration_does_not_change_staged_snapshot() {
        let providers = registrations();
        let mut registry = Registry::default();
        registry.begin().unwrap();
        registry.register(providers[0].clone()).unwrap();
        assert_eq!(
            registry.register(providers[0].clone()).unwrap_err().code,
            ProviderErrorCode::InvalidInput
        );
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
        registry.begin().unwrap();
        registry.register(api).unwrap();
        let mut wrong_label = oauth.clone();
        wrong_label.label = "Spoofed OpenAI".into();
        assert_eq!(
            registry.register(wrong_label).unwrap_err().code,
            ProviderErrorCode::InvalidInput
        );
        registry.register(oauth).unwrap();
        assert_eq!(
            registry.publish().unwrap().providers[0].connections.len(),
            2
        );
    }

    #[test]
    fn aborted_event_wave_discards_every_staged_registration() {
        let providers = registrations();
        let mut registry = Registry::default();
        registry.begin().unwrap();
        assert_eq!(
            registry.publish().unwrap_err().code,
            ProviderErrorCode::Unavailable
        );
        registry.register(providers[0].clone()).unwrap();
        registry.abort().unwrap();
        assert!(!registry.is_populating());
        registry.begin().unwrap();
        registry.register(providers[1].clone()).unwrap();
        assert_eq!(
            registry.publish().unwrap().providers,
            vec![providers[1].clone()]
        );
    }
}
