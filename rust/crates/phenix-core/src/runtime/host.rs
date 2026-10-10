use super::{
    dispatch::{
        ComponentDispatchTarget, ComponentInvocationPlan, invoke_component_service_with,
        invoke_resolved_chain_with, invoke_service_with,
    },
    *,
};

impl<'a> PluginHost<'a> {
    pub fn graph_generation(&self) -> Option<&GenerationId> {
        self.scope.generation.generation()
    }

    pub fn component_graph(&self) -> &ResolvedComponentGraph {
        self.scope.generation.component_graph()
    }

    pub fn entry_triggers(&self) -> &[crate::ComponentEntryTrigger] {
        self.scope.generation.entry_triggers()
    }

    /// A compiled declarative workflow bound to the caller's graph generation.
    pub fn workflow(
        &self,
        owner: &crate::ComponentId,
        name: &str,
    ) -> Option<&crate::CompiledWorkflow> {
        self.scope.generation.workflow(owner, name)
    }

    pub fn plugin(&self) -> &PluginId {
        self.plugin
    }

    pub fn authority(&self) -> &Authority {
        &self.scope.authority
    }

    /// Capture the current root constraints for an explicitly rooted child.
    ///
    /// Authority is the effective authority at this call site. Pinned bindings
    /// are copied from the root scope so a child cannot silently change its host
    /// execution world.
    pub fn root_execution_constraints(&self) -> RootExecutionConstraints {
        RootExecutionConstraints {
            authority: self.scope.authority.clone(),
            pinned_bindings: self.scope.pinned_bindings.as_ref().clone(),
        }
    }

    /// Opaque identity assigned by Core, shared by every delegated call
    /// belonging to the same root. Plugins cannot select their own root ID.
    pub fn root_id(&self) -> u64 {
        self.scope.root_id
    }

    pub fn cancellation_token(&self) -> Option<&CallCancellationToken> {
        self.scope.cancellation.as_ref()
    }

    #[doc(hidden)]
    pub fn record_runtime_trace(&self, event: RuntimeTraceEvent) {
        trace::record_runtime_trace(self.runtime.trace_sink, event);
    }

    #[doc(hidden)]
    pub fn runtime_trace(&self) -> Vec<RuntimeTraceEvent> {
        self.runtime.trace_sink.snapshot()
    }

    pub fn invoke_import<I: ComponentInterface>(
        &self,
        component: &ComponentId,
        request: &crate::PhenixValue,
    ) -> Result<crate::PhenixValue, ComponentInvocationError> {
        let resolved = self
            .scope
            .generation
            .component_graph()
            .component(component)
            .ok_or_else(|| crate::ComponentGraphError::UnknownComponent(component.clone()))?;
        if &resolved.owning_plugin != self.plugin {
            return Err(KernelError::HostOperationDenied {
                plugin: self.plugin.clone(),
                operation: format!("component import owned by {}", resolved.owning_plugin),
            }
            .into());
        }
        let interface = I::interface_id();
        let dispatch = self
            .scope
            .generation
            .dispatch_topology()
            .component_import(component, &interface)
            .ok_or_else(|| ComponentInvocationError::UnboundImport {
                component: component.clone(),
                interface: interface.clone(),
            })?;
        let plan = &dispatch.providers;
        let pin_key = (component.clone(), interface.clone());
        let pinned = self.scope.pinned_bindings.get(&pin_key);
        let (handle, fallback_reason) = if let Some(pinned) = pinned {
            if plan.primary() != pinned {
                return Err(KernelError::PinnedBindingChanged {
                    generation: self
                        .scope
                        .generation
                        .generation()
                        .expect("plugin import runs in a resolved generation")
                        .clone(),
                    component: component.clone(),
                    interface: interface.clone(),
                }
                .into());
            }
            if !self.provider_available(pinned) {
                return Err(KernelError::PluginNotActive(pinned.owning_plugin().clone()).into());
            }
            (pinned, None)
        } else if self.provider_available(plan.primary()) {
            (plan.primary(), None)
        } else if let Some(fallback) = plan
            .fallbacks()
            .iter()
            .find(|fallback| self.provider_available(fallback))
        {
            (fallback, Some(ProviderFallbackReason::PrimaryUnavailable))
        } else {
            return Err(
                KernelError::PluginNotActive(plan.primary().owning_plugin().clone()).into(),
            );
        };
        let service = &dispatch.service;
        let input = serde_json::to_vec(request)
            .map_err(|error| ComponentInvocationError::Encode(error.to_string()))?;
        let delegated_authority = self.scope.authority.attenuate(handle.effective_authority());
        let provider_provenance = ComponentProviderProvenance::from_plan(
            interface,
            plan,
            handle,
            fallback_reason,
            delegated_authority.clone(),
        );
        let scope = self.scope.delegated(
            delegated_authority.clone(),
            TransactionContext::coordinated_by(self.plugin),
        );
        let output = invoke_component_service_with(
            self.runtime,
            ComponentInvocationPlan {
                service,
                layers: &dispatch.layers,
                policy_identity: dispatch.policy_identity,
            },
            ComponentDispatchTarget {
                component: handle.exporter(),
                binding: handle.owning_plugin(),
                provider_provenance: Some(provider_provenance),
            },
            &input,
            scope,
        )?;
        serde_json::from_slice(&output)
            .map_err(|error| ComponentInvocationError::Decode(error.to_string()))
    }

    /// Dynamic selected-import entry used by the versioned native ABI.
    ///
    /// `component` must be owned by the *calling* plugin; `interface`
    /// must be one of that component's declared imports. Unlike an ordinary
    /// opt-in fallback import, this pins the resolved primary provider and
    /// refuses to reselect a fallback after generation activation. Authority,
    /// service layers, and provider provenance use the canonical dispatcher.
    #[doc(hidden)]
    pub fn invoke_import_wire(
        &self,
        component: &ComponentId,
        interface: &InterfaceId,
        input: &[u8],
    ) -> Result<Vec<u8>, ComponentInvocationError> {
        let resolved = self
            .scope
            .generation
            .component_graph()
            .component(component)
            .ok_or_else(|| crate::ComponentGraphError::UnknownComponent(component.clone()))?;
        if &resolved.owning_plugin != self.plugin {
            return Err(KernelError::HostOperationDenied {
                plugin: self.plugin.clone(),
                operation: format!("component import owned by {}", resolved.owning_plugin),
            }
            .into());
        }
        let dispatch = self
            .scope
            .generation
            .dispatch_topology()
            .component_import(component, interface)
            .ok_or_else(|| ComponentInvocationError::UnboundImport {
                component: component.clone(),
                interface: interface.clone(),
            })?;
        let plan = &dispatch.providers;
        let pin_key = (component.clone(), interface.clone());
        if self
            .scope
            .pinned_bindings
            .get(&pin_key)
            .is_some_and(|pinned| pinned != plan.primary())
        {
            return Err(KernelError::PinnedBindingChanged {
                generation: self
                    .scope
                    .generation
                    .generation()
                    .expect("native ABI import has a resolved generation")
                    .clone(),
                component: component.clone(),
                interface: interface.clone(),
            }
            .into());
        }
        let handle = plan.primary();
        if !self.provider_available(handle) {
            return Err(KernelError::PluginNotActive(handle.owning_plugin().clone()).into());
        }
        let delegated = self.scope.authority.attenuate(handle.effective_authority());
        let provenance = ComponentProviderProvenance::from_plan(
            interface.clone(),
            plan,
            handle,
            None,
            delegated.clone(),
        );
        let scope = self
            .scope
            .delegated(delegated, TransactionContext::coordinated_by(self.plugin));
        invoke_component_service_with(
            self.runtime,
            ComponentInvocationPlan {
                service: &dispatch.service,
                layers: &dispatch.layers,
                policy_identity: dispatch.policy_identity,
            },
            ComponentDispatchTarget {
                component: handle.exporter(),
                binding: handle.owning_plugin(),
                provider_provenance: Some(provenance),
            },
            input,
            scope,
        )
        .map_err(Into::into)
    }

    fn provider_available(&self, handle: &ResolvedImportHandle) -> bool {
        self.runtime.states.get(handle.owning_plugin()).copied() == Some(PluginState::Active)
            && self
                .runtime
                .invocations
                .contains_key(handle.owning_plugin())
    }

    #[doc(hidden)]
    pub fn invoke_service_abi(
        &self,
        service: &ServiceId,
        input: &[u8],
        requested_authority: &Authority,
        binding: Option<&PluginId>,
    ) -> Result<Vec<u8>, KernelError> {
        let delegated_authority = self.scope.authority.attenuate(requested_authority);
        let scope = self.scope.delegated(
            delegated_authority,
            TransactionContext::coordinated_by(self.plugin),
        );
        invoke_service_with(self.runtime, service, input, binding, scope)
    }

    #[doc(hidden)]
    pub fn invoke_component_abi(
        &self,
        component: &ComponentId,
        service: &ServiceId,
        input: &[u8],
        requested_authority: &Authority,
        binding: &PluginId,
    ) -> Result<Vec<u8>, KernelError> {
        let resolved = self
            .scope
            .generation
            .component_graph()
            .component(component)
            .ok_or_else(|| crate::ComponentGraphError::UnknownComponent(component.clone()))?;
        if &resolved.owning_plugin != binding {
            return Err(KernelError::HostOperationDenied {
                plugin: self.plugin.clone(),
                operation: format!(
                    "component {component} is owned by {} rather than {binding}",
                    resolved.owning_plugin
                ),
            });
        }

        let service_plan = self.scope.generation.dispatch_topology().service(service);
        let layer_plan = service_plan.map_or(&[][..], |plan| plan.layers.as_slice());
        let policy_identity = service_plan.map_or_else(
            || self.scope.generation.config().policy_identity(),
            |plan| plan.policy_identity,
        );
        let delegated_authority = self.scope.authority.attenuate(requested_authority);
        let scope = self.scope.delegated(
            delegated_authority,
            TransactionContext::coordinated_by(self.plugin),
        );
        invoke_component_service_with(
            self.runtime,
            ComponentInvocationPlan {
                service,
                layers: layer_plan,
                policy_identity,
            },
            ComponentDispatchTarget {
                component,
                binding,
                provider_provenance: None,
            },
            input,
            scope,
        )
    }

    pub fn continue_service(
        &self,
        input: &[u8],
        requested_authority: &Authority,
    ) -> Result<Vec<u8>, KernelError> {
        let continuation = self
            .continuation
            .as_ref()
            .ok_or(KernelError::ContinuationUnavailable)?;
        let chain = self
            .scope
            .selected_chain
            .as_ref()
            .ok_or(KernelError::ContinuationUnavailable)?;
        let service = chain.service.clone();
        if continuation.used.swap(true, Ordering::AcqRel) {
            return Err(KernelError::ContinuationAlreadyUsed(service));
        }
        let delegated_authority = self.scope.authority.attenuate(requested_authority);
        let scope = self
            .scope
            .delegated(delegated_authority, self.scope.transactions.clone());
        invoke_resolved_chain_with(
            self.runtime,
            continuation.next_position,
            input,
            scope,
            continuation.terminal_component.as_ref(),
            &continuation.trace,
        )
    }

    pub fn task_scope(&self) -> Option<TaskScope<'_>> {
        Some(TaskScope::new_owned(
            self.runtime.tasks,
            self.scope.generation.generation()?,
            &self.scope.authority,
            self.plugin,
        ))
    }

    pub fn dispatch_event(
        &self,
        event_type: EventTypeId,
        version: u32,
        causality_id: u64,
        kernel_policy_revision: u64,
        payload: Vec<u8>,
    ) -> Result<EventAdmissionReceipt, EventError> {
        let event = EventEnvelope {
            event_type,
            version,
            emitter: self.plugin.clone(),
            causality_id,
            kernel_policy_revision,
            payload,
        };
        self.runtime.events.admit_in_generation(
            &event,
            &self.scope.authority,
            self.scope.generation.generation(),
        )
    }

    pub fn register_durable_schema(&self, schema: &DurableSchema) -> Result<(), KernelError> {
        self.require_persistence_operation(PERSISTENCE_SCHEMA, &schema.namespace)?;
        self.runtime
            .persistence
            .lock()
            .expect("kernel persistence mutex poisoned")
            .register_schema(self.plugin, schema)
            .map_err(|error| self.persistence_error(error.to_string()))
    }

    pub fn migrate_durable_schema(
        &self,
        schema: &DurableSchema,
        migrations: &[SchemaMigration],
    ) -> Result<(), KernelError> {
        self.require_persistence_operation(PERSISTENCE_SCHEMA, &schema.namespace)?;
        self.require_capability(PERSISTENCE_WRITE)?;
        self.runtime
            .persistence
            .lock()
            .expect("kernel persistence mutex poisoned")
            .migrate_schema(self.plugin, schema, migrations)
            .map_err(|error| self.persistence_error(error.to_string()))
    }

    pub fn read_durable(
        &self,
        namespace: &ResourceNamespace,
        key: &str,
    ) -> Result<Option<Vec<u8>>, KernelError> {
        self.require_persistence_operation(PERSISTENCE_READ, namespace)?;
        self.runtime
            .persistence
            .lock()
            .expect("kernel persistence mutex poisoned")
            .read(self.plugin, namespace, key)
            .map_err(|error| self.persistence_error(error.to_string()))
    }

    pub fn transact_durable(
        &self,
        namespace: &ResourceNamespace,
        operations: &[TransactionOp],
    ) -> Result<(), KernelError> {
        let resource = namespace.as_str().to_owned();
        if let Err(error) = self.require_persistence_operation(PERSISTENCE_WRITE, namespace) {
            self.trace_data_mutation(
                resource,
                "authorization",
                operations.len(),
                "denied",
                Some(error.to_string()),
            );
            return Err(error);
        }
        self.trace_data_mutation(
            resource.clone(),
            "authorization",
            operations.len(),
            "allowed",
            None,
        );
        if let Err(error) = self.require_not_cancelled("durable transaction") {
            self.trace_data_mutation(
                resource,
                "cancellation_gate",
                operations.len(),
                "denied",
                Some(error.to_string()),
            );
            return Err(error);
        }
        self.trace_data_mutation(
            resource.clone(),
            "commit",
            operations.len(),
            "started",
            None,
        );
        let result = self
            .runtime
            .persistence
            .lock()
            .expect("kernel persistence mutex poisoned")
            .transact(self.plugin, namespace, operations)
            .map_err(|error| self.persistence_error(error.to_string()));
        match &result {
            Ok(()) => {
                self.trace_data_mutation(resource, "commit", operations.len(), "committed", None)
            }
            Err(error) => self.trace_data_mutation(
                resource,
                "commit",
                operations.len(),
                "failed",
                Some(error.to_string()),
            ),
        }
        result
    }

    pub fn prepare_durable_transaction(
        &self,
        namespace: &ResourceNamespace,
        operations: &[TransactionOp],
    ) -> Result<crate::PreparedMutationHandle, KernelError> {
        let resource = namespace.as_str().to_owned();
        self.trace_data_mutation(
            resource.clone(),
            "prepare",
            operations.len(),
            "started",
            None,
        );
        let result = (|| {
            self.require_persistence_operation(PERSISTENCE_WRITE, namespace)?;
            self.require_active_plugin(self.plugin)?;
            self.require_not_cancelled("prepare durable transaction")?;
            self.require_prepared_scope_generation()?;
            self.runtime
                .prepared_mutations
                .prepare(
                    self.plugin,
                    namespace,
                    operations,
                    &self.scope.authority,
                    &self.scope.transactions,
                )
                .map_err(|message| self.persistence_error(message))
        })();
        match &result {
            Ok(_) => {
                self.trace_data_mutation(resource, "prepare", operations.len(), "prepared", None)
            }
            Err(error) => self.trace_data_mutation(
                resource,
                "prepare",
                operations.len(),
                "failed",
                Some(error.to_string()),
            ),
        }
        result
    }

    pub fn transact_prepared(
        &self,
        handles: &[crate::PreparedMutationHandle],
    ) -> Result<(), KernelError> {
        let operation_count = handles.len();
        let resource = "prepared-multi".to_owned();
        self.trace_data_mutation(
            resource.clone(),
            "prepared_commit",
            operation_count,
            "started",
            None,
        );

        let result = (|| {
            self.require_capability(PERSISTENCE_WRITE)?;
            self.require_not_cancelled("commit prepared durable transactions")?;
            self.require_prepared_scope_generation()?;
            if handles.is_empty() {
                return Err(KernelError::HostOperationDenied {
                    plugin: self.plugin.clone(),
                    operation: "commit prepared durable transactions without participants".into(),
                });
            }

            let participants = self
                .runtime
                .prepared_mutations
                .consume(handles)
                .map_err(|_| KernelError::HostOperationDenied {
                    plugin: self.plugin.clone(),
                    operation: "prepared mutation is unavailable in this invocation scope".into(),
                })?;
            if participants
                .iter()
                .any(|participant| &participant.coordinator != self.plugin)
            {
                return Err(KernelError::HostOperationDenied {
                    plugin: self.plugin.clone(),
                    operation: "prepared mutation was issued to another coordinator".into(),
                });
            }
            if !participants
                .iter()
                .any(|participant| &participant.transaction.owner == self.plugin)
            {
                return Err(KernelError::HostOperationDenied {
                    plugin: self.plugin.clone(),
                    operation: "prepared commit requires a caller-owned participant".into(),
                });
            }

            let write = PermissionId::parse(PERSISTENCE_WRITE)
                .expect("kernel persistence write capability is valid");
            for participant in &participants {
                let transaction = &participant.transaction;
                if !participant.authority.permits(&write) {
                    return Err(KernelError::HostOperationDenied {
                        plugin: self.plugin.clone(),
                        operation: format!(
                            "prepared mutation from {} lacks attenuated persistence write authority",
                            transaction.owner
                        ),
                    });
                }
                if self
                    .scope
                    .generation
                    .config()
                    .resource_owner(&transaction.namespace)
                    != Some(&transaction.owner)
                {
                    return Err(KernelError::HostOperationDenied {
                        plugin: self.plugin.clone(),
                        operation: format!(
                            "{PERSISTENCE_WRITE}:{}:{}",
                            transaction.owner, transaction.namespace
                        ),
                    });
                }
                if &transaction.owner == self.plugin {
                    continue;
                }
                self.require_active_plugin(&transaction.owner)?;
                let authorized_import = self
                    .scope
                    .generation
                    .component_graph()
                    .components()
                    .filter(|component| &component.owning_plugin == self.plugin)
                    .flat_map(|component| component.imports.iter())
                    .flat_map(|import| import.binding.iter().chain(import.fallbacks.iter()))
                    .any(|binding| {
                        binding.owning_plugin() == &transaction.owner
                            && binding.effective_authority().permits(&write)
                    });
                if !authorized_import {
                    return Err(KernelError::HostOperationDenied {
                        plugin: self.plugin.clone(),
                        operation: format!(
                            "{PERSISTENCE_WRITE}:{}:{} without authorized typed import",
                            transaction.owner, transaction.namespace
                        ),
                    });
                }
            }

            let transactions: Vec<_> = participants
                .into_iter()
                .map(|participant| participant.transaction)
                .collect();
            self.runtime
                .persistence
                .lock()
                .expect("kernel persistence mutex poisoned")
                .transact_many(&transactions)
                .map_err(|error| self.persistence_error(error.to_string()))
        })();

        match &result {
            Ok(()) => self.trace_data_mutation(
                resource,
                "prepared_commit",
                operation_count,
                "committed",
                None,
            ),
            Err(error) => self.trace_data_mutation(
                resource,
                "prepared_commit",
                operation_count,
                "failed",
                Some(error.to_string()),
            ),
        }
        result
    }

    fn require_prepared_scope_generation(&self) -> Result<(), KernelError> {
        if self.runtime.prepared_mutations.generation() == self.scope.generation.generation() {
            return Ok(());
        }
        self.runtime.prepared_mutations.clear();
        Err(KernelError::HostOperationDenied {
            plugin: self.plugin.clone(),
            operation: "prepared mutation scope belongs to another graph generation".into(),
        })
    }

    fn require_not_cancelled(&self, operation: &str) -> Result<(), KernelError> {
        if !self
            .scope
            .cancellation
            .as_ref()
            .is_some_and(CallCancellationToken::is_cancelled)
        {
            return Ok(());
        }
        self.runtime.prepared_mutations.clear();
        Err(KernelError::HostOperationDenied {
            plugin: self.plugin.clone(),
            operation: format!("{operation} after call cancellation"),
        })
    }

    fn require_active_plugin(&self, plugin: &PluginId) -> Result<(), KernelError> {
        if self.runtime.states.get(plugin).copied() == Some(PluginState::Active)
            && self.runtime.instances.contains_key(plugin)
        {
            return Ok(());
        }
        Err(KernelError::PluginNotActive(plugin.clone()))
    }

    fn require_persistence_operation(
        &self,
        capability: &str,
        namespace: &ResourceNamespace,
    ) -> Result<(), KernelError> {
        self.require_capability(capability)?;
        if self.scope.generation.config().resource_owner(namespace) == Some(self.plugin) {
            return Ok(());
        }
        Err(KernelError::HostOperationDenied {
            plugin: self.plugin.clone(),
            operation: format!("{capability}:{}", namespace.as_str()),
        })
    }

    fn require_capability(&self, capability: &str) -> Result<(), KernelError> {
        let capability = PermissionId::parse(capability).expect("kernel capability is valid");
        if self.scope.authority.permits(&capability) {
            return Ok(());
        }
        Err(KernelError::HostOperationDenied {
            plugin: self.plugin.clone(),
            operation: capability.as_str().to_owned(),
        })
    }

    pub(super) fn trace_data_mutation(
        &self,
        resource: String,
        stage: &str,
        operation_count: usize,
        outcome: &str,
        error: Option<String>,
    ) {
        trace::record_runtime_trace(
            self.runtime.trace_sink,
            RuntimeTraceEvent::DataMutation {
                resource,
                stage: stage.to_owned(),
                operation_count,
                outcome: outcome.to_owned(),
                error,
            },
        );
    }

    fn persistence_error(&self, message: String) -> KernelError {
        KernelError::Persistence {
            plugin: self.plugin.clone(),
            message,
        }
    }
}
