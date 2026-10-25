//! Generation-local staging for existing M1 provider-population graph events.
//! A declaration is data, not authorization or an executable provider.
use phenix_provider_builtins::{
    merge_registration_pages, MAX_REGISTERED_CONNECTIONS, MAX_REGISTERED_PROVIDERS,
};
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
    Published(Vec<ProviderDescriptor>),
    Aborted,
}

/// Owner-private state. No Storage mirror, kernel registry or partial catalog.
#[derive(Default)]
pub(crate) struct Registry {
    phase: Phase,
}

impl Registry {
    pub(crate) fn blocks_pull_catalog(&self) -> bool {
        !matches!(&self.phase, Phase::Idle)
    }

    /// The published snapshot is the sole authoritative catalog for this
    /// native plugin generation. No second selected vector may diverge.
    pub(crate) fn selected(&self) -> Option<&[ProviderDescriptor]> {
        match &self.phase {
            Phase::Published(providers) => Some(providers),
            _ => None,
        }
    }

    /// Transitional pull adapter: select validated imported declarations only
    /// before event population starts. Never fall back during failed waves.
    pub(crate) fn select_legacy(
        &mut self,
        providers: Vec<ProviderDescriptor>,
    ) -> Result<(), ProviderError> {
        if !matches!(&self.phase, Phase::Idle) {
            return Err(issue(
                ProviderErrorCode::Conflict,
                "provider catalog already selected or population started",
            ));
        }
        if providers.is_empty() {
            return Err(issue(
                ProviderErrorCode::Unavailable,
                "no provider declarations registered",
            ));
        }
        self.phase = Phase::Published(providers);
        Ok(())
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
        // The staged catalog is already validated by induction. Only the
        // incoming provider (and the prior fragment with the same identity)
        // can introduce a new connection conflict. Do not clone or
        // revalidate every unrelated provider for every registration.
        let previous = pending.iter().position(|item| item.id == provider.id);
        let connection_count = pending
            .iter()
            .try_fold(0usize, |total, item| {
                total.checked_add(item.connections.len())
            })
            .and_then(|total| total.checked_add(provider.connections.len()));
        if connection_count.is_none_or(|count| count > MAX_REGISTERED_CONNECTIONS)
            || (previous.is_none() && pending.len() >= MAX_REGISTERED_PROVIDERS)
        {
            return Err(issue(
                ProviderErrorCode::InvalidInput,
                "invalid or conflicting provider declaration",
            ));
        }
        let mut fragment = Vec::with_capacity(if previous.is_some() { 2 } else { 1 });
        if let Some(index) = previous {
            fragment.push(pending[index].clone());
        }
        fragment.push(provider);
        let mut merged = merge_registration_pages([ProviderPage {
            providers: fragment,
            revision: 1,
        }])
        .map_err(|_| {
            issue(
                ProviderErrorCode::InvalidInput,
                "invalid or conflicting provider declaration",
            )
        })?;
        let normalized = merged
            .pop()
            .expect("validated nonempty registration yields one provider");
        if let Some(index) = previous {
            pending[index] = normalized;
        } else {
            pending.push(normalized);
        }
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
        self.phase = Phase::Published(providers.clone());
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
        // A failed wave cannot silently fall back to the old pull model or
        // restart within the same plugin generation with stale graph work.
        self.phase = Phase::Aborted;
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
        assert!(registry.blocks_pull_catalog());
        assert_eq!(
            registry.begin().unwrap_err().code,
            ProviderErrorCode::Conflict
        );
        registry.register(providers[1].clone()).unwrap();
        registry.register(providers[0].clone()).unwrap();
        let catalog = registry.publish().unwrap();
        assert!(registry.blocks_pull_catalog());
        assert_eq!(registry.selected(), Some(catalog.providers.as_slice()));
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
    fn pull_adapter_and_graph_population_use_one_generation_snapshot() {
        let providers = registrations();
        let mut pull = Registry::default();
        assert!(pull.selected().is_none());
        pull.select_legacy(vec![providers[0].clone()]).unwrap();
        assert_eq!(pull.selected(), Some(&providers[..1]));
        assert_eq!(pull.begin().unwrap_err().code, ProviderErrorCode::Conflict);
        assert_eq!(
            pull.select_legacy(vec![providers[1].clone()])
                .unwrap_err()
                .code,
            ProviderErrorCode::Conflict
        );

        let mut graph = Registry::default();
        graph.begin().unwrap();
        assert_eq!(
            graph
                .select_legacy(vec![providers[0].clone()])
                .unwrap_err()
                .code,
            ProviderErrorCode::Conflict
        );
        graph.register(providers[0].clone()).unwrap();
        assert!(graph.selected().is_none());
        let page = graph.publish().unwrap();
        assert_eq!(graph.selected(), Some(page.providers.as_slice()));
        assert_eq!(
            graph
                .select_legacy(vec![providers[1].clone()])
                .unwrap_err()
                .code,
            ProviderErrorCode::Conflict
        );

        let mut failed = Registry::default();
        failed.begin().unwrap();
        failed.abort().unwrap();
        assert!(failed.selected().is_none());
        assert_eq!(
            failed
                .select_legacy(vec![providers[0].clone()])
                .unwrap_err()
                .code,
            ProviderErrorCode::Conflict
        );
    }

    #[test]
    fn incremental_registration_matches_full_canonical_merge_in_any_order() {
        let input = registrations();
        for order in [[0, 1, 2, 3, 4, 5], [5, 2, 0, 4, 1, 3]] {
            let mut registry = Registry::default();
            registry.begin().unwrap();
            for index in order {
                registry.register(input[index].clone()).unwrap();
            }
            let page = registry.publish().unwrap();
            assert_eq!(
                page.providers,
                merge_registration_pages([ProviderPage {
                    providers: input.clone(),
                    revision: 1,
                }])
                .unwrap()
            );
        }
    }

    #[test]
    fn incremental_staging_enforces_shared_provider_limit_before_mutation() {
        let source = registrations().into_iter().next().unwrap();
        let mut registry = Registry::default();
        registry.begin().unwrap();
        for index in 0..MAX_REGISTERED_PROVIDERS {
            let mut declaration = source.clone();
            declaration.id = format!("test-{index:03}");
            registry.register(declaration).unwrap();
        }
        let mut extra = source.clone();
        extra.id = "over-limit".into();
        assert_eq!(
            registry.register(extra).unwrap_err().code,
            ProviderErrorCode::InvalidInput
        );
        let published = registry.publish().unwrap();
        assert_eq!(published.providers.len(), MAX_REGISTERED_PROVIDERS);
        assert!(published.providers.iter().all(|provider| provider.id.starts_with("test-")));
    }

    #[test]
    fn incremental_rejection_retains_entire_previous_catalog() {
        let input = registrations();
        let mut registry = Registry::default();
        registry.begin().unwrap();
        registry.register(input[0].clone()).unwrap();
        registry.register(input[1].clone()).unwrap();
        let mut duplicate = input[0].clone();
        duplicate.connections[0].label.push_str(" spoofed");
        assert_eq!(
            registry.register(duplicate).unwrap_err().code,
            ProviderErrorCode::InvalidInput
        );
        let published = registry.publish().unwrap();
        assert_eq!(published.providers.len(), 2);
        assert_eq!(
            published.providers,
            merge_registration_pages([ProviderPage {
                providers: vec![input[0].clone(), input[1].clone()],
                revision: 1,
            }])
            .unwrap()
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
        assert!(registry.blocks_pull_catalog());
        assert_eq!(
            registry.begin().unwrap_err().code,
            ProviderErrorCode::Conflict
        );
        assert_eq!(
            registry.register(providers[1].clone()).unwrap_err().code,
            ProviderErrorCode::Conflict
        );
        assert_eq!(
            registry.publish().unwrap_err().code,
            ProviderErrorCode::Conflict
        );
    }
}
