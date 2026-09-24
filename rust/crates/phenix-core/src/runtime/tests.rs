use super::*;
use crate::{
    ComponentExport, ComponentImport, ComponentManifest, InterfaceId, PluginManifest,
    ServiceContribution,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).unwrap()
}

fn service(value: &str) -> ServiceId {
    ServiceId::parse(value).unwrap()
}

fn test_host<'a>(
    kernel: &'a Kernel,
    generation: Arc<RuntimeGeneration>,
    plugin: &'a PluginId,
    authority: &Authority,
    prepared_mutations: &'a PreparedMutationScope,
) -> PluginHost<'a> {
    PluginHost {
        runtime: RuntimeServices {
            states: &kernel.states,
            instances: &kernel.instances,
            invocations: &kernel.invocations,
            events: &kernel.events,
            tasks: &kernel.tasks,
            persistence: &kernel.persistence,
            prepared_mutations,
            trace_sink: kernel.trace_sink.as_ref(),
            provenance: &kernel.provenance,
        },
        plugin,
        scope: CallScope::root(generation, plugin, authority, None),
        continuation: None,
    }
}

struct MarkerPlugin(Arc<AtomicBool>);
impl PluginInstance for MarkerPlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        assert_eq!(host.plugin().as_str(), "embedded");
        self.0.store(true, Ordering::Release);
        Ok(())
    }
}

struct StartupTaskScopePlugin;

impl PluginInstance for StartupTaskScopePlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        assert!(host.task_scope().is_none());
        assert!(host.cancellation_token().is_some());
        Ok(())
    }
}

struct NoopPlugin;

impl PluginInstance for NoopPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }
}

struct EchoPlugin;

impl PluginInstance for EchoPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if host.authority().permits(&capability("fs.write")) {
            return Err("provider regained caller write authority".into());
        }
        let cancellation = host
            .cancellation_token()
            .ok_or_else(|| "service invocation has no cancellation token".to_owned())?;
        if cancellation.is_cancelled() {
            return Err("fresh service invocation started cancelled".into());
        }
        Ok(input.to_vec())
    }
}

struct TaggedPlugin(&'static [u8]);

impl PluginInstance for TaggedPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        _input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        Ok(self.0.to_vec())
    }
}

struct PanicPlugin;

impl PluginInstance for PanicPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        _input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        panic!("fixture plugin crash")
    }
}

struct PersistencePlugin {
    namespace: ResourceNamespace,
}

impl PluginInstance for PersistencePlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        host.register_durable_schema(&DurableSchema::new(self.namespace.clone(), 1))
            .map_err(|error| error.to_string())?;
        host.transact_durable(
            &self.namespace,
            &[TransactionOp::Put {
                key: "seed".into(),
                value: b"ready".to_vec(),
            }],
        )
        .map_err(|error| error.to_string())
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        match input {
            b"read" => host
                .read_durable(&self.namespace, "seed")
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "missing seed".to_owned()),
            b"write" => {
                host.transact_durable(
                    &self.namespace,
                    &[TransactionOp::Put {
                        key: "changed".into(),
                        value: b"yes".to_vec(),
                    }],
                )
                .map_err(|error| error.to_string())?;
                Ok(b"written".to_vec())
            }
            _ => Err("unsupported input".into()),
        }
    }
}

#[test]
fn kernel_only_boots_without_agent_domain_services() {
    let mut kernel = Kernel::kernel_only();
    kernel.activate_all().unwrap();
    assert_eq!(kernel.config().manifests().count(), 0);
}

#[test]
fn startup_host_without_resolved_graph_exposes_no_task_scope() {
    let startup = PluginManifest {
        id: plugin("startup"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    };
    let mut kernel = Kernel::new(KernelConfig::new([startup]).unwrap());
    kernel
        .register_embedded_factory(plugin("startup"), || Box::new(StartupTaskScopePlugin))
        .unwrap();

    assert!(kernel.graph_generation().is_none());
    kernel.activate_all().unwrap();
}

#[test]
fn embedded_and_resource_only_plugins_share_lifecycle_contract() {
    let marker = Arc::new(AtomicBool::new(false));
    let embedded = PluginManifest {
        id: plugin("embedded"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    };
    let mut resources = PluginManifest::resource_only(plugin("resources"));
    resources
        .resource_namespaces
        .push(ResourceNamespace::parse("resources.static").unwrap());

    let mut kernel = Kernel::new(KernelConfig::new([resources, embedded]).unwrap());
    let marker_for_factory = Arc::clone(&marker);
    kernel
        .register_embedded_factory(plugin("embedded"), move || {
            Box::new(MarkerPlugin(Arc::clone(&marker_for_factory)))
        })
        .unwrap();

    kernel.activate_all().unwrap();

    assert!(marker.load(Ordering::Acquire));
    assert_eq!(kernel.state(&plugin("embedded")), Some(PluginState::Active));
    assert_eq!(
        kernel.state(&plugin("resources")),
        Some(PluginState::Active)
    );
    assert_eq!(
        kernel
            .config()
            .resource_owner(&ResourceNamespace::parse("resources.static").unwrap()),
        Some(&plugin("resources"))
    );
}

#[test]
fn invocation_uses_caller_authority_attenuated_by_provider_grant() {
    let read = capability("fs.read");
    let write = capability("fs.write");
    let provider = PluginManifest {
        id: plugin("echo"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: crate::ServiceRole::Terminal,
            service: service("echo@1"),
            priority: 1,
            required_authority: Authority::new([read.clone()]),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::new([read.clone()]),
    };
    let mut kernel = Kernel::new(KernelConfig::new([provider]).unwrap());
    kernel
        .register_embedded_factory(plugin("echo"), || Box::new(EchoPlugin))
        .unwrap();
    kernel.activate_all().unwrap();

    let output = kernel
        .invoke(
            &service("echo@1"),
            b"hello",
            &Authority::new([read, write]),
            None,
        )
        .unwrap();
    assert_eq!(output, b"hello");
    assert_eq!(kernel.tasks().active_call_count(&plugin("echo")), 0);
}

#[test]
fn unbound_service_selects_next_live_precomputed_terminal() {
    let provider = |id: &str, priority| PluginManifest {
        id: plugin(id),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: crate::ServiceRole::Terminal,
            service: service("fallback@1"),
            priority,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    };
    let preferred = provider("preferred", 20);
    let fallback = provider("fallback", 10);
    let mut kernel = Kernel::new(KernelConfig::new([preferred, fallback]).unwrap());
    kernel
        .register_embedded_factory(plugin("preferred"), || Box::new(TaggedPlugin(b"preferred")))
        .unwrap();
    kernel
        .register_embedded_factory(plugin("fallback"), || Box::new(TaggedPlugin(b"fallback")))
        .unwrap();
    kernel.activate_all().unwrap();

    kernel
        .states
        .insert(plugin("preferred"), PluginState::Stopped);

    assert_eq!(
        kernel
            .invoke(
                &service("fallback@1"),
                b"ignored",
                &Authority::default(),
                None,
            )
            .unwrap(),
        b"fallback"
    );

    assert!(matches!(
        kernel
            .invoke(
                &service("fallback@1"),
                b"ignored",
                &Authority::default(),
                Some(&plugin("preferred")),
            )
            .unwrap_err(),
        KernelError::BoundProviderUnavailable { .. }
    ));
}

#[test]
fn plugin_panic_is_normalized_and_closes_live_call_scope() {
    let provider = PluginManifest {
        id: plugin("panic-provider"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: crate::ServiceRole::Terminal,
            service: service("panic@1"),
            priority: 1,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    };
    let mut kernel = Kernel::new(KernelConfig::new([provider]).unwrap());
    kernel
        .register_embedded_factory(plugin("panic-provider"), || Box::new(PanicPlugin))
        .unwrap();
    kernel.activate_all().unwrap();

    let error = kernel
        .invoke(&service("panic@1"), b"boom", &Authority::default(), None)
        .unwrap_err();

    assert!(matches!(error, KernelError::ServiceInvoke { .. }));
    assert!(error.to_string().contains("plugin invocation panicked"));
    assert_eq!(
        kernel.tasks().active_call_count(&plugin("panic-provider")),
        0
    );
}

#[test]
fn persistence_host_rechecks_effective_authority_on_every_call() {
    let schema = capability(PERSISTENCE_SCHEMA);
    let read = capability(PERSISTENCE_READ);
    let write = capability(PERSISTENCE_WRITE);
    let namespace = ResourceNamespace::parse("storage.state").unwrap();
    let provider = PluginManifest {
        id: plugin("storage"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: crate::ServiceRole::Terminal,
            service: service("storage@1"),
            priority: 1,
            required_authority: Authority::default(),
        }],
        resource_namespaces: vec![namespace.clone()],
        maximum_authority: Authority::new([schema.clone(), read.clone(), write.clone()]),
    };
    let mut kernel = Kernel::new(KernelConfig::new([provider]).unwrap());
    let traces = Arc::new(RuntimeTraceBuffer::default());
    kernel.set_runtime_trace_sink(traces.clone());
    kernel
        .register_embedded_factory(plugin("storage"), move || {
            Box::new(PersistencePlugin {
                namespace: namespace.clone(),
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();

    assert_eq!(
        kernel
            .invoke(
                &service("storage@1"),
                b"read",
                &Authority::new([read.clone()]),
                None,
            )
            .unwrap(),
        b"ready"
    );

    let error = kernel
        .invoke(
            &service("storage@1"),
            b"write",
            &Authority::new([read, write]),
            None,
        )
        .unwrap();
    assert_eq!(error, b"written");

    let denied = kernel
        .invoke(
            &service("storage@1"),
            b"write",
            &Authority::new([capability(PERSISTENCE_READ)]),
            None,
        )
        .unwrap_err();
    assert!(matches!(denied, KernelError::ServiceInvoke { .. }));
    assert!(denied.to_string().contains(PERSISTENCE_WRITE));

    let recorded = traces.snapshot();
    assert!(recorded.iter().any(|trace| matches!(
        trace,
        RuntimeTraceEvent::DataMutation {
            resource,
            stage,
            outcome,
            ..
        } if resource == "storage.state" && stage == "commit" && outcome == "committed"
    )));
    assert!(recorded.iter().any(|trace| matches!(
        trace,
        RuntimeTraceEvent::DataMutation {
            resource,
            stage,
            outcome,
            ..
        } if resource == "storage.state" && stage == "authorization" && outcome == "denied"
    )));
}

#[test]
fn prepared_transaction_requires_write_authority_on_foreign_typed_import() {
    let write = capability(PERSISTENCE_WRITE);
    let caller_namespace = ResourceNamespace::parse("caller.state").unwrap();
    let owner_namespace = ResourceNamespace::parse("owner.state").unwrap();
    let interface = InterfaceId::parse("fixture.owner.persistence@1").unwrap();
    let caller = PluginManifest {
        id: plugin("caller"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: vec![caller_namespace.clone()],
        maximum_authority: Authority::new([write.clone()]),
    };
    let owner = PluginManifest {
        id: plugin("owner"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: vec![owner_namespace.clone()],
        maximum_authority: Authority::new([write.clone()]),
    };
    let caller_component = ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse("caller.component").unwrap(),
        owner: caller.id.clone(),
        imports: vec![ComponentImport {
            interface: interface.clone(),
            schema: Default::default(),
            required: true,
            authority: Authority::default(),
        }],
        exports: Vec::new(),
        maximum_authority: caller.maximum_authority.clone(),
    };
    let owner_component = ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse("owner.component").unwrap(),
        owner: owner.id.clone(),
        imports: Vec::new(),
        exports: vec![ComponentExport {
            interface,
            schema: Default::default(),
            priority: 1,
            required_authority: Authority::default(),
        }],
        maximum_authority: owner.maximum_authority.clone(),
    };
    let graph = ResolvedComponentGraph::compile(
        [caller.clone(), owner.clone()],
        [caller_component, owner_component],
        &Authority::new([write.clone()]),
    )
    .unwrap();
    let mut kernel = Kernel::new(KernelConfig::new([caller.clone(), owner.clone()]).unwrap());
    kernel
        .register_embedded_factory(caller.id.clone(), || Box::new(NoopPlugin))
        .unwrap();
    kernel
        .register_embedded_factory(owner.id.clone(), || Box::new(NoopPlugin))
        .unwrap();
    kernel.activate_all().unwrap();

    let caller_plugin = caller.id;
    let authority = Authority::new([write]);
    let prepared_mutations = PreparedMutationScope::new(kernel.graph_generation());
    let caller_mutation = prepared_mutations
        .prepare(
            &caller_plugin,
            &caller_namespace,
            &[],
            &authority,
            &TransactionContext::unscoped(),
        )
        .unwrap();
    let owner_transactions = TransactionContext::coordinated_by(&caller_plugin);
    let owner_mutation = prepared_mutations
        .prepare(
            &owner.id,
            &owner_namespace,
            &[TransactionOp::Put {
                key: "forbidden".into(),
                value: b"write".to_vec(),
            }],
            &authority,
            &owner_transactions,
        )
        .unwrap();
    let generation = Arc::new(RuntimeGeneration::bootstrap_with_component_graph(
        kernel.config().clone(),
        graph,
    ));
    let host = test_host(
        &kernel,
        generation,
        &caller_plugin,
        &authority,
        &prepared_mutations,
    );    let denied = host
        .transact_prepared(&[caller_mutation, owner_mutation])
        .unwrap_err();
    assert!(matches!(denied, KernelError::HostOperationDenied { .. }));
    assert!(denied
        .to_string()
        .contains("without authorized typed import"));
}

#[test]
fn prepared_mutation_cannot_be_transferred_to_another_authorized_importer() {
    let write = capability(PERSISTENCE_WRITE);
    let authority = Authority::new([write.clone()]);
    let owner_namespace = ResourceNamespace::parse("owner.state").unwrap();
    let first_namespace = ResourceNamespace::parse("first.state").unwrap();
    let second_namespace = ResourceNamespace::parse("second.state").unwrap();
    let interface = InterfaceId::parse("fixture.owner.transfer@1").unwrap();
    let first = PluginManifest {
        id: plugin("first"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: vec![first_namespace],
        maximum_authority: authority.clone(),
    };
    let second = PluginManifest {
        id: plugin("second"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: vec![second_namespace.clone()],
        maximum_authority: authority.clone(),
    };
    let owner = PluginManifest {
        id: plugin("owner"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: vec![owner_namespace.clone()],
        maximum_authority: authority.clone(),
    };
    let importing_component = |id: &str, owner: &PluginManifest| ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse(id).unwrap(),
        owner: owner.id.clone(),
        imports: vec![ComponentImport {
            interface: interface.clone(),
            schema: Default::default(),
            required: true,
            authority: authority.clone(),
        }],
        exports: Vec::new(),
        maximum_authority: authority.clone(),
    };
    let owner_component = ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse("owner.component").unwrap(),
        owner: owner.id.clone(),
        imports: Vec::new(),
        exports: vec![ComponentExport {
            interface: interface.clone(),
            schema: Default::default(),
            priority: 1,
            required_authority: Authority::default(),
        }],
        maximum_authority: authority.clone(),
    };
    let graph = ResolvedComponentGraph::compile(
        [first.clone(), second.clone(), owner.clone()],
        [
            importing_component("first.component", &first),
            importing_component("second.component", &second),
            owner_component,
        ],
        &authority,
    )
    .unwrap();
    let mut kernel =
        Kernel::new(KernelConfig::new([first.clone(), second.clone(), owner.clone()]).unwrap());
    for plugin in [&first.id, &second.id, &owner.id] {
        kernel
            .register_embedded_factory((*plugin).clone(), || Box::new(NoopPlugin))
            .unwrap();
    }
    kernel.activate_all().unwrap();

    let prepared_mutations = PreparedMutationScope::new(kernel.graph_generation());
    let owner_transactions = TransactionContext::coordinated_by(&first.id);
    let owner_mutation = prepared_mutations
        .prepare(
            &owner.id,
            &owner_namespace,
            &[TransactionOp::Put {
                key: "value".into(),
                value: b"prepared".to_vec(),
            }],
            &authority,
            &owner_transactions,
        )
        .unwrap();
    let second_mutation = prepared_mutations
        .prepare(
            &second.id,
            &second_namespace,
            &[],
            &authority,
            &TransactionContext::unscoped(),
        )
        .unwrap();
    let generation = Arc::new(RuntimeGeneration::bootstrap_with_component_graph(
        kernel.config().clone(),
        graph,
    ));
    let host = test_host(
        &kernel,
        generation,
        &second.id,
        &authority,
        &prepared_mutations,
    );
    let denied = host
        .transact_prepared(&[owner_mutation, second_mutation])
        .unwrap_err();
    assert!(matches!(denied, KernelError::HostOperationDenied { .. }));
    assert!(denied.to_string().contains("issued to another coordinator"));
}

#[test]
fn persistence_host_rejects_unowned_namespace_before_backend_access() {
    let namespace = ResourceNamespace::parse("owned.state").unwrap();
    let other_namespace = ResourceNamespace::parse("other.state").unwrap();
    let owner = PluginManifest {
        id: plugin("owner"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: vec![namespace],
        maximum_authority: Authority::new([capability(PERSISTENCE_SCHEMA)]),
    };
    let kernel = Kernel::new(KernelConfig::new([owner]).unwrap());
    let authority = Authority::new([capability(PERSISTENCE_SCHEMA)]);
    let owner_plugin = plugin("owner");
    let prepared_mutations = PreparedMutationScope::new(kernel.graph_generation());
    let generation = Arc::new(kernel.runtime_generation.clone());
    let host = test_host(
        &kernel,
        generation,
        &owner_plugin,
        &authority,
        &prepared_mutations,
    );    assert!(matches!(
        host.register_durable_schema(&DurableSchema::new(other_namespace, 1)),
        Err(KernelError::HostOperationDenied { .. })
    ));
}
