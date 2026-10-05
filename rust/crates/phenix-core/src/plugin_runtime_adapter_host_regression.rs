use crate::{
    ArtifactRevision, Authority, Kernel, KernelConfig, PermissionId, PluginArtifact,
    PluginExecution, PluginHost, PluginId, PluginInstance, PluginManifest, PluginRuntimeAdapter,
    PluginRuntimeCandidate, PluginRuntimeId, ResolvedGeneration, ResolvedGenerationActivation,
    ServiceContribution, ServiceRole, plugin_runtime_adapter_service,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn runtime(value: &str) -> PluginRuntimeId {
    PluginRuntimeId::parse(value).unwrap()
}

struct PreparedGuest {
    started_authority: Arc<Mutex<Option<Authority>>>,
}

impl PluginInstance for PreparedGuest {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        *self.started_authority.lock().unwrap() = Some(host.authority().clone());
        Ok(())
    }
}

struct HostAwarePluginRuntimeAdapter {
    adapter_authority: Arc<Mutex<Option<Authority>>>,
    guest_authority: Arc<Mutex<Option<Authority>>>,
    guest_started_authority: Arc<Mutex<Option<Authority>>>,
    adapter_had_cancellation: Arc<Mutex<bool>>,
}

impl PluginRuntimeAdapter for HostAwarePluginRuntimeAdapter {
    fn prepare(
        &mut self,
        _candidate: PluginRuntimeCandidate<'_>,
    ) -> Result<Box<dyn PluginInstance>, String> {
        Err("legacy runtime preparation path used".into())
    }

    fn prepare_with_host(
        &mut self,
        candidate: PluginRuntimeCandidate<'_>,
        host: &PluginHost<'_>,
    ) -> Result<Box<dyn PluginInstance>, String> {
        *self.adapter_authority.lock().unwrap() = Some(host.authority().clone());
        *self.guest_authority.lock().unwrap() = Some(candidate.guest_authority.clone());
        *self.adapter_had_cancellation.lock().unwrap() = host
            .cancellation_token()
            .is_some_and(|cancellation| !cancellation.is_cancelled());
        Ok(Box::new(PreparedGuest {
            started_authority: Arc::clone(&self.guest_started_authority),
        }))
    }
}

impl PluginInstance for HostAwarePluginRuntimeAdapter {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn plugin_runtime_adapter(&mut self) -> Option<&mut dyn PluginRuntimeAdapter> {
        Some(self)
    }
}

#[test]
fn plugin_runtime_adapter_and_guest_receive_separate_host_authority() {
    let runtime = runtime("fixture.runtime");
    let adapter_permission = PermissionId::parse("bridge.exec").unwrap();
    let guest_permission = PermissionId::parse("guest.read").unwrap();
    let adapter_plugin = PluginManifest {
        id: plugin("fixture.bridge"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            service: plugin_runtime_adapter_service(&runtime),
            role: ServiceRole::Terminal,
            priority: 0,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::new([adapter_permission.clone()]),
    };
    let guest = PluginManifest {
        id: plugin("fixture.guest"),
        version: 1,
        execution: PluginExecution::Runtime {
            runtime,
            artifact: PluginArtifact {
                locator: "fixture.plugin".into(),
                revision: ArtifactRevision::from_content(b"plugin-runtime-adapter-host-isolation"),
                configuration: BTreeMap::new(),
            },
        },
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::new([guest_permission.clone()]),
    };

    let adapter_authority = Arc::new(Mutex::new(None));
    let guest_authority = Arc::new(Mutex::new(None));
    let guest_started_authority = Arc::new(Mutex::new(None));
    let adapter_had_cancellation = Arc::new(Mutex::new(false));
    let mut kernel = Kernel::new(KernelConfig::new([adapter_plugin.clone(), guest]).unwrap());
    let adapter_authority_for_factory = Arc::clone(&adapter_authority);
    let guest_authority_for_factory = Arc::clone(&guest_authority);
    let guest_started_authority_for_factory = Arc::clone(&guest_started_authority);
    let adapter_had_cancellation_for_factory = Arc::clone(&adapter_had_cancellation);
    kernel
        .register_embedded_factory(adapter_plugin.id.clone(), move || {
            Box::new(HostAwarePluginRuntimeAdapter {
                adapter_authority: Arc::clone(&adapter_authority_for_factory),
                guest_authority: Arc::clone(&guest_authority_for_factory),
                guest_started_authority: Arc::clone(&guest_started_authority_for_factory),
                adapter_had_cancellation: Arc::clone(&adapter_had_cancellation_for_factory),
            })
        })
        .unwrap();

    kernel.activate_all().unwrap();

    let adapter_authority = adapter_authority.lock().unwrap().clone().unwrap();
    assert!(adapter_authority.permits(&adapter_permission));
    assert!(!adapter_authority.permits(&guest_permission));

    let guest_authority = guest_authority.lock().unwrap().clone().unwrap();
    assert!(guest_authority.permits(&guest_permission));
    assert!(!guest_authority.permits(&adapter_permission));
    assert_eq!(
        guest_started_authority.lock().unwrap().clone().unwrap(),
        guest_authority
    );
    assert!(*adapter_had_cancellation.lock().unwrap());
    assert_eq!(kernel.tasks().active_call_count(&adapter_plugin.id), 0);
}

#[test]
fn resolved_generation_ceiling_attenuates_plugin_runtime_adapter_and_guest_lifecycle() {
    let runtime = runtime("fixture.runtime.ceiling");
    let adapter_permission = PermissionId::parse("bridge.exec").unwrap();
    let guest_permission = PermissionId::parse("guest.read").unwrap();
    let adapter_plugin = PluginManifest {
        id: plugin("fixture.bridge.ceiling"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            service: plugin_runtime_adapter_service(&runtime),
            role: ServiceRole::Terminal,
            priority: 0,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::new([adapter_permission.clone()]),
    };
    let guest = PluginManifest {
        id: plugin("fixture.guest.ceiling"),
        version: 1,
        execution: PluginExecution::Runtime {
            runtime,
            artifact: PluginArtifact {
                locator: "fixture.plugin".into(),
                revision: ArtifactRevision::from_content(b"plugin-runtime-adapter-ceiling"),
                configuration: BTreeMap::new(),
            },
        },
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::new([guest_permission.clone()]),
    };
    let ceiling = Authority::new([guest_permission.clone()]);
    let resolved =
        ResolvedGeneration::resolve([adapter_plugin.clone(), guest], [], [], &ceiling).unwrap();

    let adapter_authority = Arc::new(Mutex::new(None));
    let guest_authority = Arc::new(Mutex::new(None));
    let guest_started_authority = Arc::new(Mutex::new(None));
    let adapter_had_cancellation = Arc::new(Mutex::new(false));
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    kernel.activate_resolved_generation(&resolved).unwrap();

    let adapter_authority_for_factory = Arc::clone(&adapter_authority);
    let guest_authority_for_factory = Arc::clone(&guest_authority);
    let guest_started_authority_for_factory = Arc::clone(&guest_started_authority);
    let adapter_had_cancellation_for_factory = Arc::clone(&adapter_had_cancellation);
    kernel
        .register_embedded_factory(adapter_plugin.id.clone(), move || {
            Box::new(HostAwarePluginRuntimeAdapter {
                adapter_authority: Arc::clone(&adapter_authority_for_factory),
                guest_authority: Arc::clone(&guest_authority_for_factory),
                guest_started_authority: Arc::clone(&guest_started_authority_for_factory),
                adapter_had_cancellation: Arc::clone(&adapter_had_cancellation_for_factory),
            })
        })
        .unwrap();

    kernel.activate_all().unwrap();

    let adapter_authority = adapter_authority.lock().unwrap().clone().unwrap();
    assert!(!adapter_authority.permits(&adapter_permission));
    assert!(!adapter_authority.permits(&guest_permission));

    let guest_authority = guest_authority.lock().unwrap().clone().unwrap();
    assert!(guest_authority.permits(&guest_permission));
    assert!(!guest_authority.permits(&adapter_permission));
    assert_eq!(
        guest_started_authority.lock().unwrap().clone().unwrap(),
        guest_authority
    );
    assert!(*adapter_had_cancellation.lock().unwrap());
}
