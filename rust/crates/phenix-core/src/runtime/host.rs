use super::{
    dispatch::{
        invoke_component_service_with, invoke_resolved_chain_with, invoke_service_with,
        ComponentDispatchTarget, ServiceDispatchGuards,
    },
    *,
};

impl<'a> PluginHost<'a> {
    pub fn graph_generation(&self) -> Option<&GraphGenerationId> {
        self.graph_generation
    }

    pub fn plugin(&self) -> &PluginId {
        self.plugin
    }

    pub fn authority(&self) -> &Authority {
        self.authority
    }

    pub fn cancellation_token(&self) -> Option<&CallCancellationToken> {
        self.call_cancellation.as_ref()
    }

    pub fn invoke_import<I: ComponentInterface>(
        &self,
        component: &ComponentId,
        request: &crate::PhenixValue,
    ) -> Result<crate::PhenixValue, ComponentInvocationError> {
        let resolved = self
            .component_graph
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
        let plan = self
            .component_graph
            .provider_plan(component, &interface)?
            .ok_or_else(|| ComponentInvocationError::UnboundImport {
                component: component.clone(),
                interface: interface.clone(),
            })?;
        let (handle, fallback_reason) = if self.provider_available(plan.primary()) {
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
        let service = ServiceId::parse(interface.as_str().to_owned()).map_err(|message| {
            ComponentInvocationError::InvalidInterface {
                interface: interface.clone(),
                message: message.into(),
            }
        })?;
        let input = serde_json::to_vec(request)
            .map_err(|error| ComponentInvocationError::Encode(error.to_string()))?;
        let delegated_authority = self.authority.attenuate(handle.effective_authority());
        let provider_provenance = ComponentProviderProvenance::from_plan(
            interface,
            &plan,
            handle,
            fallback_reason,
            delegated_authority.clone(),
        );
        let output = self.prepared_mutations.with_coordinator(self.plugin, || {
            invoke_component_service_with(
                InvocationContext {
                    graph_generation: self.graph_generation,
                    component_graph: self.component_graph,
                    config: self.config,
                    states: self.states,
                    instances: self.instances,
                    events: self.events,
                    tasks: self.tasks,
                    persistence: self.persistence,
                    prepared_mutations: self.prepared_mutations,
                    provenance: self.provenance,
                },
                &service,
                ComponentDispatchTarget {
                    component: handle.exporter(),
                    binding: handle.owning_plugin(),
                    provider_provenance: Some(provider_provenance),
                },
                &input,
                &delegated_authority,
                ServiceDispatchGuards {
                    call_stack: &self.call_stack,
                    active_services: &self.active_services,
                    active_component_endpoints: &self.active_component_endpoints,
                    terminal_component: Some(handle.exporter()),
                },
            )
        })?;
        serde_json::from_slice(&output)
            .map_err(|error| ComponentInvocationError::Decode(error.to_string()))
    }

    fn provider_available(&self, handle: &ResolvedImportHandle) -> bool {
        self.states.get(handle.owning_plugin()).copied() == Some(PluginState::Active)
            && self.instances.contains_key(handle.owning_plugin())
    }

    #[doc(hidden)]
    pub fn invoke_service_abi(
        &self,
        service: &ServiceId,
        input: &[u8],
        requested_authority: &Authority,
        binding: Option<&PluginId>,
    ) -> Result<Vec<u8>, KernelError> {
        let delegated_authority = self.authority.attenuate(requested_authority);
        self.prepared_mutations.with_coordinator(self.plugin, || {
            invoke_service_with(
                InvocationContext {
                    graph_generation: self.graph_generation,
                    component_graph: self.component_graph,
                    config: self.config,
                    states: self.states,
                    instances: self.instances,
                    events: self.events,
                    tasks: self.tasks,
                    persistence: self.persistence,
                    prepared_mutations: self.prepared_mutations,
                    provenance: self.provenance,
                },
                service,
                input,
                &delegated_authority,
                binding,
                ServiceDispatchGuards {
                    call_stack: &self.call_stack,
                    active_services: &self.active_services,
                    active_component_endpoints: &self.active_component_endpoints,
                    terminal_component: None,
                },
            )
        })
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
        let service = continuation.chain.service.clone();
        if continuation.used.swap(true, Ordering::AcqRel) {
            return Err(KernelError::ContinuationAlreadyUsed(service));
        }
        let delegated_authority = self.authority.attenuate(requested_authority);
        invoke_resolved_chain_with(
            InvocationContext {
                graph_generation: self.graph_generation,
                component_graph: self.component_graph,
                config: self.config,
                states: self.states,
                instances: self.instances,
                events: self.events,
                tasks: self.tasks,
                persistence: self.persistence,
                prepared_mutations: self.prepared_mutations,
                provenance: self.provenance,
            },
            &continuation.chain,
            continuation.next_position,
            input,
            &delegated_authority,
            ServiceDispatchGuards {
                call_stack: &self.call_stack,
                active_services: &self.active_services,
                active_component_endpoints: &self.active_component_endpoints,
                terminal_component: continuation.terminal_component.as_ref(),
            },
            &continuation.trace,
        )
    }

    pub fn task_scope(&self) -> Option<TaskScope<'_>> {
        Some(TaskScope::new_owned(
            self.tasks,
            self.graph_generation?,
            self.authority,
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
        self.events
            .admit_in_generation(&event, self.authority, self.graph_generation)
    }

    pub fn register_durable_schema(&self, schema: &DurableSchema) -> Result<(), KernelError> {
        self.require_persistence_operation(PERSISTENCE_SCHEMA, &schema.namespace)?;
        self.persistence
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
        self.persistence
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
        self.persistence
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
        self.require_persistence_operation(PERSISTENCE_WRITE, namespace)?;
        self.require_not_cancelled("durable transaction")?;
        self.persistence
            .lock()
            .expect("kernel persistence mutex poisoned")
            .transact(self.plugin, namespace, operations)
            .map_err(|error| self.persistence_error(error.to_string()))
    }

    pub fn prepare_durable_transaction(
        &self,
        namespace: &ResourceNamespace,
        operations: &[TransactionOp],
    ) -> Result<crate::PreparedMutationHandle, KernelError> {
        self.require_persistence_operation(PERSISTENCE_WRITE, namespace)?;
        self.require_active_plugin(self.plugin)?;
        self.require_not_cancelled("prepare durable transaction")?;
        self.require_prepared_scope_generation()?;
        self.prepared_mutations
            .prepare(self.plugin, namespace, operations, self.authority)
            .map_err(|message| self.persistence_error(message))
    }

    pub fn transact_prepared(
        &self,
        handles: &[crate::PreparedMutationHandle],
    ) -> Result<(), KernelError> {
        self.require_capability(PERSISTENCE_WRITE)?;
        self.require_not_cancelled("commit prepared durable transactions")?;
        self.require_prepared_scope_generation()?;
        if handles.is_empty() {
            return Err(KernelError::HostOperationDenied {
                plugin: self.plugin.clone(),
                operation: "commit prepared durable transactions without participants".into(),
            });
        }

        let participants = self.prepared_mutations.consume(handles).map_err(|_| {
            KernelError::HostOperationDenied {
                plugin: self.plugin.clone(),
                operation: "prepared mutation is unavailable in this invocation scope".into(),
            }
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

        let write = CapabilityId::parse(PERSISTENCE_WRITE)
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
            if self.config.resource_owner(&transaction.namespace) != Some(&transaction.owner) {
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
                .component_graph
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
        self.persistence
            .lock()
            .expect("kernel persistence mutex poisoned")
            .transact_many(&transactions)
            .map_err(|error| self.persistence_error(error.to_string()))
    }

    fn require_prepared_scope_generation(&self) -> Result<(), KernelError> {
        if self.prepared_mutations.generation() == self.graph_generation {
            return Ok(());
        }
        self.prepared_mutations.clear();
        Err(KernelError::HostOperationDenied {
            plugin: self.plugin.clone(),
            operation: "prepared mutation scope belongs to another graph generation".into(),
        })
    }

    fn require_not_cancelled(&self, operation: &str) -> Result<(), KernelError> {
        if !self
            .call_cancellation
            .as_ref()
            .is_some_and(CallCancellationToken::is_cancelled)
        {
            return Ok(());
        }
        self.prepared_mutations.clear();
        Err(KernelError::HostOperationDenied {
            plugin: self.plugin.clone(),
            operation: format!("{operation} after call cancellation"),
        })
    }

    fn require_active_plugin(&self, plugin: &PluginId) -> Result<(), KernelError> {
        if self.states.get(plugin).copied() == Some(PluginState::Active)
            && self.instances.contains_key(plugin)
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
        if self.config.resource_owner(namespace) == Some(self.plugin) {
            return Ok(());
        }
        Err(KernelError::HostOperationDenied {
            plugin: self.plugin.clone(),
            operation: format!("{capability}:{}", namespace.as_str()),
        })
    }

    fn require_capability(&self, capability: &str) -> Result<(), KernelError> {
        let capability = CapabilityId::parse(capability).expect("kernel capability is valid");
        if self.authority.permits(&capability) {
            return Ok(());
        }
        Err(KernelError::HostOperationDenied {
            plugin: self.plugin.clone(),
            operation: capability.as_str().to_owned(),
        })
    }

    fn persistence_error(&self, message: String) -> KernelError {
        KernelError::Persistence {
            plugin: self.plugin.clone(),
            message,
        }
    }
}
