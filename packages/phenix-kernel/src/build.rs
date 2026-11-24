//! Kernel candidate preparation and deterministic native composition.
use super::*;

struct ContributionCatalog {
    definitions: BTreeMap<Key, ContributionHandle<ContractDefinition>>,
    offers: BTreeMap<Key, ContributionHandle<NetworkOffer>>,
    requirements: BTreeMap<Key, ContributionHandle<ContractRequirement>>,
    pending: BTreeMap<Key, ContributionHandle<RegisteredContribution>>,
}

// Keep registry validation separate from graph resolution and plugin callbacks.
fn collect_catalog(snapshots: &[PluginSnapshot]) -> Result<ContributionCatalog, ResolutionError> {
    let mut owners = BTreeSet::new();
    for snapshot in snapshots {
        if snapshot.name().len() > phenix_graph::MAX_IDENTIFIER_BYTES {
            return Err(ConfigurationError::OwnerIdentifier {
                length: snapshot.name().len(),
                limit: phenix_graph::MAX_IDENTIFIER_BYTES,
            }
            .into());
        }
        if !owners.insert(snapshot.name()) {
            return Err(ResolutionError::DuplicateOwner(snapshot.name().into()));
        }
    }
    let mut definitions = BTreeMap::new();
    let mut offers = BTreeMap::new();
    let mut requirements = BTreeMap::new();
    let mut pending = BTreeMap::new();
    for snapshot in snapshots {
        for (_, kind, id) in snapshot.keys() {
            if id.len() > phenix_graph::MAX_IDENTIFIER_BYTES {
                return Err(ConfigurationError::ContributionIdentifier {
                    owner: snapshot.name().into(),
                    length: id.len(),
                    limit: phenix_graph::MAX_IDENTIFIER_BYTES,
                }
                .into());
            }
            let key = Key::new(snapshot.name(), id);
            macro_rules! collect {
                ($ty:ty, $map:ident) => {
                    if kind == <$ty as phenix_plugin::Contribution>::KIND {
                        let handle = snapshot.get::<$ty>(id).ok_or_else(|| {
                            ConfigurationError::ContributionType {
                                key: key.clone(),
                                expected: <$ty as phenix_plugin::Contribution>::KIND,
                            }
                        })?;
                        if $map.insert(key.clone(), handle).is_some() {
                            return Err(ConfigurationError::DuplicateContribution {
                                key: key.clone(),
                            }
                            .into());
                        }
                        continue;
                    }
                };
            }
            collect!(ContractDefinition, definitions);
            collect!(NetworkOffer, offers);
            collect!(ContractRequirement, requirements);
            if kind != <RegisteredContribution as phenix_plugin::Contribution>::KIND {
                continue;
            }
            let handle = snapshot.get::<RegisteredContribution>(id).ok_or_else(|| {
                ConfigurationError::ContributionType {
                    key: key.clone(),
                    expected: <RegisteredContribution as phenix_plugin::Contribution>::KIND,
                }
            })?;
            if pending.insert(key.clone(), handle).is_some() {
                return Err(ConfigurationError::DuplicateContribution { key: key.clone() }.into());
            }
        }
    }
    let mut identities = BTreeMap::new();
    for (key, definition) in &definitions {
        if let Some(previous) = identities.insert(definition.contract.id(), &definition.contract)
            && previous != &definition.contract
        {
            return Err(ResolutionError::ConflictingDefinition {
                definition: key.clone(),
                contract: definition.contract.id().into(),
            });
        }
    }
    for (key, offer) in &offers {
        if !definitions.contains_key(&offer.contract) {
            return Err(ConfigurationError::MissingDefinition {
                key: key.clone(),
                contract: offer.contract.clone(),
                role: DefinitionRole::Offer,
            }
            .into());
        }
        if offer.producer.owner != key.owner || !pending.contains_key(&offer.producer) {
            return Err(ResolutionError::WrongOrigin {
                offer: key.clone(),
                producer: Box::new(offer.producer.clone()),
                unit: offer.unit.clone(),
            });
        }
    }

    Ok(ContributionCatalog {
        definitions,
        offers,
        requirements,
        pending,
    })
}

struct ResolvedImports {
    imports: BTreeMap<Key, BTreeMap<String, ResolvedImport>>,
    dependencies: BTreeMap<Key, BTreeSet<Key>>,
}

// Resolve host-selected providers and dependency order without touching live state.
fn resolve_dependencies(
    policy: &ImportPolicy,
    catalog: &ContributionCatalog,
) -> Result<ResolvedImports, ResolutionError> {
    let ContributionCatalog {
        definitions,
        offers,
        requirements,
        pending,
    } = catalog;
    let mut imports: BTreeMap<Key, BTreeMap<String, ResolvedImport>> = BTreeMap::new();
    let mut dependencies: BTreeMap<Key, BTreeSet<Key>> = pending
        .iter()
        .map(|(key, contribution)| (key.clone(), contribution.requires.iter().cloned().collect()))
        .collect();
    for (requirement, offer) in policy
        .selections
        .iter()
        .chain(
            policy
                .allow
                .iter()
                .flat_map(|pairs| pairs.iter().map(|(r, o)| (r, o))),
        )
        .chain(policy.deny.iter().map(|(r, o)| (r, o)))
    {
        if !requirements.contains_key(requirement) {
            return Err(ConfigurationError::UnknownRequirement {
                requirement: requirement.clone(),
            }
            .into());
        }
        if !offers.contains_key(offer) {
            return Err(ConfigurationError::UnknownOffer {
                offer: offer.clone(),
            }
            .into());
        }
    }
    for (key, requirement) in requirements {
        if !definitions.contains_key(&requirement.contract) {
            return Err(ConfigurationError::MissingDefinition {
                key: key.clone(),
                contract: requirement.contract.clone(),
                role: DefinitionRole::Requirement,
            }
            .into());
        }
        if requirement.consumer.owner != key.owner || !pending.contains_key(&requirement.consumer) {
            return Err(ConfigurationError::InvalidConsumer {
                requirement: key.clone(),
                consumer: requirement.consumer.clone(),
            }
            .into());
        }
        let requested = &definitions[&requirement.contract].contract;
        let candidates: Vec<_> = offers
            .iter()
            .filter(|(offer_key, offer)| {
                let pair = (key.clone(), (*offer_key).clone());
                !policy.deny.contains(&pair)
                    && policy
                        .allow
                        .as_ref()
                        .is_none_or(|allow| allow.contains(&pair))
                    && requested
                        .check_definition(&definitions[&offer.contract].contract)
                        .is_ok()
            })
            .map(|(key, _)| key.clone())
            .collect();
        let selected = if let Some(selected) = policy.selections.get(key) {
            if !candidates.contains(selected) {
                let offer = &offers[selected];
                let reason =
                    match requested.check_definition(&definitions[&offer.contract].contract) {
                        Err(error) => IneligibleReason::IncompatibleContract(Box::new(error)),
                        Ok(()) => IneligibleReason::DeniedByPolicy,
                    };
                return Err(ResolutionError::Ineligible {
                    requirement: key.clone(),
                    offer: selected.clone(),
                    reason,
                });
            }
            selected.clone()
        } else {
            match candidates.as_slice() {
                [] => {
                    return Err(ResolutionError::Missing {
                        requirement: key.clone(),
                        contract: requirement.contract.clone(),
                    });
                }
                [selected] => selected.clone(),
                _ => {
                    return Err(ResolutionError::Ambiguous {
                        requirement: key.clone(),
                        offers: candidates,
                    });
                }
            }
        };
        let offer = &offers[&selected];
        // A composer can create its own provider before projecting a self-import.
        if offer.producer != requirement.consumer {
            dependencies
                .get_mut(&requirement.consumer)
                .unwrap()
                .insert(offer.producer.clone());
        }
        imports
            .entry(requirement.consumer.clone())
            .or_default()
            .insert(
                requirement.id.clone(),
                ResolvedImport {
                    offer: selected,
                    producer: offer.producer.clone(),
                    unit: offer.unit.clone(),
                    contract: requested.clone(),
                },
            );
    }
    for (key, contribution) in pending {
        let mut dependencies = BTreeSet::new();
        for dependency in &contribution.requires {
            if !dependencies.insert(dependency) {
                return Err(ConfigurationError::DuplicateDependency {
                    contribution: key.clone(),
                    dependency: dependency.clone(),
                }
                .into());
            }
            if !pending.contains_key(dependency) {
                return Err(ConfigurationError::MissingDependency {
                    contribution: key.clone(),
                    dependency: dependency.clone(),
                }
                .into());
            }
        }
    }

    Ok(ResolvedImports {
        imports,
        dependencies,
    })
}

/// Incremental lexical frontier. Process each dependency edge only when
/// its provider becomes ready; never rescan all remaining candidates per step.
struct ReadyContributions {
    unmet: BTreeMap<Key, usize>,
    dependents: BTreeMap<Key, Vec<Key>>,
    ready: BTreeSet<Key>,
}

impl ReadyContributions {
    fn new(dependencies: &BTreeMap<Key, BTreeSet<Key>>) -> Result<Self, ResolutionError> {
        let mut unmet = BTreeMap::new();
        let mut dependents: BTreeMap<Key, Vec<Key>> = BTreeMap::new();
        let mut ready = BTreeSet::new();
        for (key, requires) in dependencies {
            unmet.insert(key.clone(), requires.len());
            if requires.is_empty() {
                ready.insert(key.clone());
            }
            for required in requires {
                if !dependencies.contains_key(required) {
                    return Err(ConfigurationError::MissingDependency {
                        contribution: key.clone(),
                        dependency: required.clone(),
                    }
                    .into());
                }
                dependents
                    .entry(required.clone())
                    .or_default()
                    .push(key.clone());
            }
        }
        Ok(Self {
            unmet,
            dependents,
            ready,
        })
    }

    fn pop(&mut self) -> Option<Key> {
        let key = self.ready.pop_first()?;
        if let Some(dependents) = self.dependents.get(&key) {
            for dependent in dependents {
                let remaining = self.unmet.get_mut(dependent).expect("known dependent");
                *remaining -= 1;
                if *remaining == 0 {
                    self.ready.insert(dependent.clone());
                }
            }
        }
        Some(key)
    }
}

impl Kernel {
    /// Resolve all dependencies before composing and build one native session,
    /// without activating producers. Lexical keys break topological ties.
    pub fn build(
        name: impl Into<String>,
        snapshots: impl IntoIterator<Item = PluginSnapshot>,
        dispatcher: Arc<dyn ProcessDispatcher>,
    ) -> Result<Self, ResolutionError> {
        Self::build_with_options(name, snapshots, dispatcher, BuildOptions::default())
    }

    /// Discover compatible offers, apply trusted eligibility and selections,
    /// and order composers before invoking any plugin code. A unique eligible
    /// offer is selected automatically; missing or ambiguous imports fail closed.
    pub fn build_with_options(
        _name: impl Into<String>,
        snapshots: impl IntoIterator<Item = PluginSnapshot>,
        dispatcher: Arc<dyn ProcessDispatcher>,
        mut options: BuildOptions,
    ) -> Result<Self, ResolutionError> {
        let mut planning_used = 0;
        let snapshots: Vec<_> = snapshots.into_iter().collect();
        // Composition can temporarily inspect every candidate revision, but
        // emitted packets and detached graph handles must retain only selected
        // executable owners. Never pin defeated snapshots in their leases.
        let host_lease = options.execution.lease.take();
        let preparation_lease: Arc<dyn Send + Sync> =
            Arc::new((host_lease.clone(), snapshots.clone()));
        let catalog = collect_catalog(&snapshots)?;
        let ResolvedImports {
            imports,
            dependencies,
        } = resolve_dependencies(&options.imports, &catalog)?;
        let ContributionCatalog {
            definitions,
            offers,
            requirements: _requirements,
            mut pending,
        } = catalog;
        let mut ready = ReadyContributions::new(&dependencies)?;
        // Distinct composers retain their exact plugin revision's ancestry.
        let origins: BTreeMap<_, _> = pending
            .iter()
            .map(|(key, contribution)| (key.clone(), contribution.graph_origin().child()))
            .collect();
        let mut leases = Vec::new();
        let mut keys = Vec::new();
        while let Some(key) = ready.pop() {
            leases.push(pending.remove(&key).expect("selected pending contribution"));
            keys.push(key);
        }
        if !pending.is_empty() {
            return Err(ResolutionError::Cycle(pending.keys().cloned().collect()));
        }
        // Candidate artifacts and builder are dropped before leases on build failure.
        let mut artifacts = BTreeMap::new();
        let base = NetworkBuilder::new()
            .build()
            .map_err(|cause| ResolutionError::Compilation { cause })?;
        let mut composer = Composer::new(base)
            .map_err(|cause| ResolutionError::CompositionSetup(Box::new(cause)))?;
        composer.set_selection_limits(options.selection_limits);
        composer.set_planning_limits(options.planning_limits);
        let registration_ids: BTreeMap<_, _> = keys
            .iter()
            .enumerate()
            .map(|(index, key)| (key.clone(), index.to_string()))
            .collect();
        let empty_imports = BTreeMap::new();
        for (key, contribution) in keys.iter().zip(&leases) {
            let authority = options.composition.get(key).cloned().unwrap_or_default();
            // A self-import may bind the immutable static payload, never a made-up
            // interface. The same host grants and origin checks govern this preview.
            let preview = if imports
                .get(key)
                .is_some_and(|entries| entries.values().any(|import| import.producer == *key))
            {
                let mut preview =
                    Composer::new(composer.current().network().clone()).map_err(|cause| {
                        ResolutionError::Composition {
                            contribution: key.clone(),
                            cause: Box::new(cause),
                        }
                    })?;
                preview.set_selection_limits(options.selection_limits);
                preview.set_planning_limits(options.planning_limits);
                preview
                    .register_with_planning_usage(
                        [Registration::new(
                            "self",
                            [],
                            authority.priority,
                            origins[key].clone(),
                            authority.grants.clone(),
                            contribution.patch.clone(),
                        )],
                        &mut planning_used,
                    )
                    .map_err(|cause| ResolutionError::Composition {
                        contribution: key.clone(),
                        cause: Box::new(cause),
                    })?;
                Some(preview)
            } else {
                None
            };
            let context_compiled = preview
                .as_ref()
                .map_or_else(
                    || composer.current().compiled(),
                    |preview| preview.current().compiled(),
                )
                .with_lease(preparation_lease.clone());
            let mut owned = match &contribution.prepare {
                Some(prepare) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    prepare(&CandidateContext {
                        compiled: &context_compiled,
                        imports: imports.get(key).unwrap_or(&empty_imports),
                        can_inspect_network: options.inspect_network.contains(key),
                    })
                }))
                .unwrap_or_else(|payload| {
                    Err(phenix_graph::process::callback_panicked(
                        phenix_graph::CallbackPhase::Prepare,
                        payload,
                    )
                    .with_lease(preparation_lease.clone())
                    .into())
                })
                .map_err(|cause| ResolutionError::Composition {
                    contribution: key.clone(),
                    cause: lease_callback_error(cause, preparation_lease.clone()),
                })?,
                None => CandidateArtifacts::default(),
            };
            let patch = owned
                .patch
                .take()
                .unwrap_or_else(|| contribution.patch.clone());
            artifacts.insert(key.clone(), owned);
            composer
                .register_with_planning_usage(
                    [Registration::new(
                        registration_ids[key].clone(),
                        dependencies[key]
                            .iter()
                            .map(|dependency| registration_ids[dependency].clone()),
                        authority.priority,
                        origins[key].clone(),
                        authority.grants,
                        patch,
                    )],
                    &mut planning_used,
                )
                .map_err(|cause| match cause {
                    phenix_graph::compose::ComposeError::Network(cause) => {
                        ResolutionError::Compilation { cause }
                    }
                    cause => ResolutionError::Composition {
                        contribution: key.clone(),
                        cause: Box::new(cause),
                    },
                })?;
        }
        let active = composer.current().active();
        let selected: BTreeSet<_> = keys
            .iter()
            .filter(|key| active.contains(&registration_ids[*key]))
            .cloned()
            .collect();
        let selected_owners: BTreeSet<_> = selected.iter().map(|key| key.owner.as_str()).collect();
        let execution_snapshots: Vec<_> = snapshots
            .iter()
            .filter(|snapshot| selected_owners.contains(snapshot.name()))
            .cloned()
            .collect();
        let code_lease: Arc<dyn Send + Sync> = Arc::new((host_lease, execution_snapshots));
        let compiled = composer.current().compiled().with_lease(code_lease.clone());
        artifacts.retain(|key, _| selected.contains(key));
        let (keys, leases): (Vec<_>, Vec<_>) = keys
            .into_iter()
            .zip(leases)
            .filter(|(key, _)| selected.contains(key))
            .unzip();
        // Check all registered definitions against all compiled claims and
        // requests, including units with no offer or selected requirement.
        compiled
            .check_contract_catalog(definitions.values().map(|entry| &entry.contract))
            .map_err(|cause| ResolutionError::Interface {
                offer: None,
                unit: String::new(),
                cause: Box::new(cause),
            })?;
        for (key, offer) in &offers {
            let contract = &definitions[&offer.contract].contract;
            let unit =
                compiled
                    .inspect(&offer.unit)
                    .map_err(|cause| ResolutionError::MissingUnit {
                        offer: key.clone(),
                        unit: offer.unit.clone(),
                        cause: Box::new(cause),
                    })?;
            if unit.origin.as_ref() != Some(&origins[&offer.producer]) {
                return Err(ResolutionError::WrongOrigin {
                    offer: key.clone(),
                    producer: Box::new(offer.producer.clone()),
                    unit: offer.unit.clone(),
                });
            }
            compiled
                .check_implementation(&offer.unit, contract)
                .map_err(|cause| ResolutionError::Interface {
                    offer: Some(key.clone()),
                    unit: offer.unit.clone(),
                    cause: Box::new(cause),
                })?;
        }
        let mut routes = BTreeMap::new();
        for (key, contribution) in keys.iter().zip(&leases) {
            if artifacts[key].activate.is_none() && !contribution.routes.is_empty() {
                return Err(ConfigurationError::RoutesWithoutProducer {
                    contribution: key.clone(),
                }
                .into());
            }
            let mut selected = BTreeSet::new();
            for name in &contribution.routes {
                if !compiled.inputs().contains_key(name) {
                    return Err(ConfigurationError::UnknownRoute {
                        contribution: key.clone(),
                        route: name.clone(),
                    }
                    .into());
                }
                if !selected.insert(name.clone()) {
                    return Err(ConfigurationError::DuplicateRoute {
                        contribution: key.clone(),
                        route: name.clone(),
                    }
                    .into());
                }
            }
            routes.insert(key.clone(), selected);
        }
        // The session propagates this lease into dispatched jobs and queued packets.
        // Do not let a caller-provided lease displace the plugin revision leases.
        options.execution.lease = Some(code_lease.clone());
        options.execution.dispatcher = dispatcher;
        let session_identity = next_kernel_session_id();
        options.execution.execution_guards.insert(
            0,
            Arc::new(KernelExecutionGuard {
                identity: session_identity,
            }),
        );
        let session = compiled.start_with_options(options.execution);
        if let Some(failure) = session.failure() {
            return Err(ResolutionError::Session(failure.clone()));
        }
        Ok(Self {
            session: ExclusiveSlot::new(session),
            session_identity,
            compiled,
            producers: Mutex::new(ProducerState::default()),
            producer_drain: Mutex::new(()),
            producer_ownership: AtomicUsize::new(0),
            driver_wake: Mutex::new(None),
            artifacts,
            _leases: leases,
            keys,
            started: false,
            routes,
            _snapshots: snapshots,
            code_lease,
        })
    }
}

/// Regression coverage for the same lexical order and cycle frontier as the
/// former full-map dependency scan, including ready nodes becoming eligible.
#[cfg(test)]
mod ready_contribution_tests {
    use super::*;

    fn key(id: &str) -> Key {
        Key::new("plugin", id)
    }

    #[test]
    fn newly_ready_lexical_predecessor_takes_priority_over_existing_ready_nodes() {
        let dependencies = BTreeMap::from([
            (key("a"), BTreeSet::from([key("b")])),
            (key("b"), BTreeSet::new()),
            (key("c"), BTreeSet::new()),
            (key("d"), BTreeSet::from([key("b"), key("c")])),
        ]);
        let mut ready = ReadyContributions::new(&dependencies).unwrap();
        let order = std::iter::from_fn(|| ready.pop())
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>();
        assert_eq!(order, ["b", "a", "c", "d"]);
    }

    #[test]
    fn cycles_leave_exact_unprocessed_keys_after_independent_work() {
        let dependencies = BTreeMap::from([
            (key("a"), BTreeSet::from([key("b")])),
            (key("b"), BTreeSet::from([key("a")])),
            (key("c"), BTreeSet::new()),
            (key("d"), BTreeSet::from([key("c")])),
        ]);
        let mut ready = ReadyContributions::new(&dependencies).unwrap();
        let mut pending = dependencies.keys().cloned().collect::<BTreeSet<_>>();
        while let Some(key) = ready.pop() {
            pending.remove(&key);
        }
        assert_eq!(pending, BTreeSet::from([key("a"), key("b")]));
    }

    #[test]
    fn missing_dependency_fails_closed() {
        let dependencies = BTreeMap::from([(key("consumer"), BTreeSet::from([key("missing")]))]);
        assert!(matches!(
            ReadyContributions::new(&dependencies),
            Err(ResolutionError::Invalid(_))
        ));
    }

    #[test]
    fn long_dependency_chain_requires_no_recursion() {
        let dependencies = (0..512)
            .map(|index| {
                let requires = if index == 0 {
                    BTreeSet::new()
                } else {
                    BTreeSet::from([key(&format!("{:04}", index - 1))])
                };
                (key(&format!("{index:04}")), requires)
            })
            .collect();
        let mut ready = ReadyContributions::new(&dependencies).unwrap();
        for index in 0..512 {
            assert_eq!(ready.pop(), Some(key(&format!("{index:04}"))));
        }
        assert_eq!(ready.pop(), None);
    }
}
