struct RegisteredValue {
    owner: PluginId,
    schema: PhenixSchema,
    version: ValueVersion,
    snapshot_policy: SnapshotPolicy,
    current: PhenixValue,
}

struct StoreState {
    values: BTreeMap<ValueId, RegisteredValue>,
    subscriptions: BTreeMap<ObservationId, Arc<SubscriptionEntry>>,
    indexes: BTreeMap<ValueId, SubscriptionNode>,
    next_commit: u64,
    next_observation: u64,
    next_generation: u64,
    closed: bool,
    #[cfg(test)]
    snapshot_copies: usize,
    #[cfg(test)]
    match_visits: usize,
}

impl Default for StoreState {
    fn default() -> Self {
        Self {
            values: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
            indexes: BTreeMap::new(),
            next_commit: 1,
            next_observation: 1,
            next_generation: 1,
            closed: false,
            #[cfg(test)]
            snapshot_copies: 0,
            #[cfg(test)]
            match_visits: 0,
        }
    }
}

#[derive(Clone, Default)]
pub struct ObservableStore {
    state: Arc<Mutex<StoreState>>,
}

impl ObservableStore {
    pub fn register(&self, registration: ObservableRegistration) -> Result<(), ObservableError> {
        registration
            .schema
            .parse(&registration.initial)
            .map_err(|error| ObservableError::SchemaMismatch {
                value: Box::new(registration.id.clone()),
                path: Box::new(ValuePath::root()),
                message: error.to_string().into(),
            })?;
        let mut state = self.state.lock().expect("observable store lock poisoned");
        ensure_open(&state)?;
        if state.values.contains_key(&registration.id) {
            return Err(ObservableError::DuplicateValue(registration.id));
        }
        state.values.insert(
            registration.id,
            RegisteredValue {
                owner: registration.owner,
                schema: registration.schema,
                version: ValueVersion::default(),
                snapshot_policy: registration.snapshot_policy,
                current: registration.initial,
            },
        );
        Ok(())
    }

    pub fn unregister(&self, value: &ValueId) -> Result<(), ObservableError> {
        let mut state = self.state.lock().expect("observable store lock poisoned");
        ensure_open(&state)?;
        state
            .values
            .remove(value)
            .ok_or_else(|| ObservableError::UnknownValue(value.clone()))?;
        if let Some(index) = state.indexes.remove(value) {
            let mut ids = SmallVec::<[ObservationId; 8]>::new();
            index.collect_all(&mut ids);
            for id in ids {
                state.subscriptions.remove(&id);
            }
        }
        Ok(())
    }

    pub fn metadata(&self, value: &ValueId) -> Result<ObservableMetadata, ObservableError> {
        let state = self.state.lock().expect("observable store lock poisoned");
        ensure_open(&state)?;
        let registered = state
            .values
            .get(value)
            .ok_or_else(|| ObservableError::UnknownValue(value.clone()))?;
        Ok(ObservableMetadata {
            id: value.clone(),
            owner: registered.owner.clone(),
            schema: registered.schema.clone(),
            version: registered.version,
            snapshot_policy: registered.snapshot_policy,
        })
    }

    pub fn metadata_all(&self) -> Result<Vec<ObservableMetadata>, ObservableError> {
        let state = self.state.lock().expect("observable store lock poisoned");
        ensure_open(&state)?;
        Ok(state
            .values
            .iter()
            .map(|(id, registered)| ObservableMetadata {
                id: id.clone(),
                owner: registered.owner.clone(),
                schema: registered.schema.clone(),
                version: registered.version,
                snapshot_policy: registered.snapshot_policy,
            })
            .collect())
    }

    pub fn schema(&self, address: &ValueAddress) -> Result<PhenixSchema, ObservableError> {
        let state = self.state.lock().expect("observable store lock poisoned");
        ensure_open(&state)?;
        let registered = state
            .values
            .get(&address.value)
            .ok_or_else(|| ObservableError::UnknownValue(address.value.clone()))?;
        validate_path(&address.value, &registered.schema, &address.path).cloned()
    }

    pub fn get(
        &self,
        address: &ValueAddress,
    ) -> Result<(ValueVersion, PhenixValue), ObservableError> {
        let state = self.state.lock().expect("observable store lock poisoned");
        ensure_open(&state)?;
        let registered = state
            .values
            .get(&address.value)
            .ok_or_else(|| ObservableError::UnknownValue(address.value.clone()))?;
        validate_path(&address.value, &registered.schema, &address.path)?;
        let value = value_at(&registered.current, address.path.segments())
            .map_err(|message| ObservableError::TransactionConflict {
                value: address.value.clone(),
                message,
            })?
            .clone();
        Ok((registered.version, value))
    }

    pub fn subscribe(
        &self,
        spec: ObservationSpec,
        handler: Arc<dyn ObservationHandler>,
    ) -> Result<ObservationSubscription, ObservableError> {
        let mut initial = None;
        let subscription = {
            let mut state = self.state.lock().expect("observable store lock poisoned");
            ensure_open(&state)?;
            let registered = state
                .values
                .get(&spec.address.value)
                .ok_or_else(|| ObservableError::UnknownValue(spec.address.value.clone()))?;
            validate_path(&spec.address.value, &registered.schema, &spec.address.path)?;
            if registered.snapshot_policy == SnapshotPolicy::CurrentOnly
                && (spec.mode == ObservationMode::Full || spec.initial == InitialObservation::Full)
            {
                return Err(ObservableError::UnsupportedSnapshotPolicy {
                    value: spec.address.value.clone(),
                    policy: registered.snapshot_policy,
                    mode: spec.mode,
                    initial: spec.initial,
                });
            }

            let id = ObservationId(state.next_observation);
            state.next_observation = state.next_observation.saturating_add(1);
            let generation = ObservationGeneration(state.next_generation);
            state.next_generation = state.next_generation.saturating_add(1);

            if spec.initial == InitialObservation::Full {
                let registered = state
                    .values
                    .get(&spec.address.value)
                    .expect("validated observable still registered");
                initial = Some((
                    Arc::new(registered.current.clone()),
                    registered.version,
                    spec.address.clone(),
                    id,
                    generation,
                ));
                #[cfg(test)]
                {
                    state.snapshot_copies += 1;
                }
            }

            state
                .indexes
                .entry(spec.address.value.clone())
                .or_default()
                .insert(spec.address.path.segments(), id, spec.scope);
            state.subscriptions.insert(
                id,
                Arc::new(SubscriptionEntry {
                    id,
                    spec,
                    generation,
                    handler: Arc::clone(&handler),
                }),
            );
            ObservationSubscription { id, generation }
        };

        if let Some((snapshot, version, address, id, generation)) = initial {
            handler.handle(ObservationDelivery {
                commit: None,
                value: &address.value,
                from_version: version,
                version,
                address: &address,
                observation: id,
                generation,
                changes: &[],
                snapshot: Some(&snapshot),
            });
        }
        Ok(subscription)
    }

    pub fn unsubscribe(&self, subscription: &ObservationSubscription) -> Result<bool, ObservableError> {
        let mut state = self.state.lock().expect("observable store lock poisoned");
        ensure_open(&state)?;
        let Some(entry) = state.subscriptions.get(&subscription.id) else {
            return Ok(false);
        };
        if entry.generation != subscription.generation {
            return Ok(false);
        }
        let entry = state
            .subscriptions
            .remove(&subscription.id)
            .expect("checked subscription exists");
        if let Some(index) = state.indexes.get_mut(&entry.spec.address.value) {
            index.remove(
                entry.spec.address.path.segments(),
                subscription.id,
                entry.spec.scope,
            );
            if index.is_empty() {
                state.indexes.remove(&entry.spec.address.value);
            }
        }
        Ok(true)
    }

    pub fn close(&self) {
        let mut state = self.state.lock().expect("observable store lock poisoned");
        state.closed = true;
        state.subscriptions.clear();
        state.indexes.clear();
    }

    pub fn transaction<'a, I, R>(
        &self,
        roots: I,
        apply: impl FnOnce(&mut ObservableTransaction<'_, 'a>) -> Result<R, ObservableError>,
    ) -> Result<R, ObservableError>
    where
        I: IntoIterator<Item = &'a ValueId>,
    {
        let mut declared = SmallVec::<[&ValueId; 4]>::new();
        for root in roots {
            if !declared.contains(&root) {
                declared.push(root);
            }
        }
        declared.sort_unstable();

        let (result, prepared) = {
            let mut state = self.state.lock().expect("observable store lock poisoned");
            ensure_open(&state)?;
            for value in &declared {
                if !state.values.contains_key(*value) {
                    return Err(ObservableError::UnknownValue((*value).clone()));
                }
            }

            let mut transaction = ObservableTransaction {
                state: &mut state,
                declared,
                undo: SmallVec::new(),
                touched: SmallVec::new(),
            };
            let output = match apply(&mut transaction) {
                Ok(output) => output,
                Err(error) => {
                    transaction.rollback();
                    return Err(error);
                }
            };

            match transaction.finish() {
                Ok(deliveries) => (output, deliveries),
                Err(error) => {
                    transaction.rollback();
                    return Err(error);
                }
            }
        };

        for root in &prepared {
            for admitted in &root.deliveries {
                admitted.entry.handler.handle(ObservationDelivery {
                    commit: Some(root.commit),
                    value: root.value,
                    from_version: root.from_version,
                    version: root.version,
                    address: &admitted.entry.spec.address,
                    observation: admitted.entry.id,
                    generation: admitted.entry.generation,
                    changes: &admitted.changes,
                    snapshot: if admitted.entry.spec.mode == ObservationMode::Full {
                        root.snapshot.as_ref()
                    } else {
                        None
                    },
                });
            }
        }

        Ok(result)
    }
}

fn ensure_open(state: &StoreState) -> Result<(), ObservableError> {
    if state.closed {
        Err(ObservableError::Closed)
    } else {
        Ok(())
    }
}
