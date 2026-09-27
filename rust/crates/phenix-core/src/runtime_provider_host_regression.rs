use crate::{
    runtime_provider_service, ArtifactRevision, Authority, CapabilityId, Kernel, KernelConfig,
    PluginArtifact, PluginExecution, PluginHost, PluginId, PluginInstance, PluginManifest,
    PluginRuntimeProvider, ResolvedHarness, ResolvedHarnessActivation, RuntimeId,
    RuntimePluginCandidate, ServiceContribution, ServiceRole,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn runtime(value: &str) -> RuntimeId {
    RuntimeId::parse(value).unwrap()
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

struct HostAwareBridge {
    provider_authority: Arc<Mutex<Option<Authority>>>,
    guest_authority: Arc<Mutex<Option<Authority>>>,
    guest_started_authority: Arc<Mutex<Option<Authority>>>,
    provider_had_cancellation: Arc<Mutex<bool>>,
}

impl PluginRuntimeProvider for HostAwareBridge {
    fn prepare(
        &mut self,
        _candidate: RuntimePluginCandidate<'_>,
    ) -> Result<Box<dyn PluginInstance>, String> {
        Err("legacy runtime preparation path used".into())
    }

    fn prepare_with_host(
        &mut self,
        candidate: RuntimePluginCandidate<'_>,
        host: &PluginHost<'_>,
    ) -> Result<Box<dyn PluginInstance>, String> {
        *self.provider_authority.lock().unwrap() = Some(host.authority().clone());
        *self.guest_authority.lock().unwrap() = Some(candidate.guest_authority.clone());
        *self.provider_had_cancellation.lock().unwrap() = host
            .cancellation_token()
            .is_some_and(|cancellation| !cancellation.is_cancelled());
        Ok(Box::new(PreparedGuest {
            started_authority: Arc::clone(&self.guest_started_authority),
        }))
    }
}

impl PluginInstance for HostAwareBridge {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn runtime_provider(&mut self) -> Option<&mut dyn PluginRuntimeProvider> {
        Some(self)
    }
}

#[test]
fn runtime_provider_and_guest_receive_separate_host_authority() {
    let runtime = runtime("fixture.runtime");
    let bridge_capability = CapabilityId::parse("bridge.exec").unwrap();
    let guest_capability = CapabilityId::parse("guest.read").unwrap();
    let bridge = PluginManifest {
        id: plugin("fixture.bridge"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            service: runtime_provider_service(&runtime),
            role: ServiceRole::Terminal,
            priority: 0,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::new([bridge_capability.clone()]),
    };
    let guest = PluginManifest {
        id: plugin("fixture.guest"),
        version: 1,
        execution: PluginExecution::Runtime {
            runtime,
            artifact: PluginArtifact {
                locator: "fixture.plugin".into(),
                revision: ArtifactRevision::from_content(b"runtime-provider-host-isolation"),
                configuration: BTreeMap::new(),
            },
        },
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::new([guest_capability.clone()]),
    };

    let provider_authority = Arc::new(Mutex::new(None));
    let guest_authority = Arc::new(Mutex::new(None));
    let guest_started_authority = Arc::new(Mutex::new(None));
    let provider_had_cancellation = Arc::new(Mutex::new(false));
    let mut kernel = Kernel::new(KernelConfig::new([bridge.clone(), guest]).unwrap());
    let provider_authority_for_factory = Arc::clone(&provider_authority);
    let guest_authority_for_factory = Arc::clone(&guest_authority);
    let guest_started_authority_for_factory = Arc::clone(&guest_started_authority);
    let provider_had_cancellation_for_factory = Arc::clone(&provider_had_cancellation);
    kernel
        .register_embedded_factory(bridge.id.clone(), move || {
            Box::new(HostAwareBridge {
                provider_authority: Arc::clone(&provider_authority_for_factory),
                guest_authority: Arc::clone(&guest_authority_for_factory),
                guest_started_authority: Arc::clone(&guest_started_authority_for_factory),
                provider_had_cancellation: Arc::clone(&provider_had_cancellation_for_factory),
            })
        })
        .unwrap();

    kernel.activate_all().unwrap();

    let provider_authority = provider_authority.lock().unwrap().clone().unwrap();
    assert!(provider_authority.permits(&bridge_capability));
    assert!(!provider_authority.permits(&guest_capability));

    let guest_authority = guest_authority.lock().unwrap().clone().unwrap();
    assert!(guest_authority.permits(&guest_capability));
    assert!(!guest_authority.permits(&bridge_capability));
    assert_eq!(
        guest_started_authority.lock().unwrap().clone().unwrap(),
        guest_authority
    );
    assert!(*provider_had_cancellation.lock().unwrap());
    assert_eq!(kernel.tasks().active_call_count(&bridge.id), 0);
}

#[test]
fn resolved_harness_ceiling_attenuates_runtime_provider_and_guest_lifecycle() {
    let runtime = runtime("fixture.runtime.ceiling");
    let bridge_capability = CapabilityId::parse("bridge.exec").unwrap();
    let guest_capability = CapabilityId::parse("guest.read").unwrap();
    let bridge = PluginManifest {
        id: plugin("fixture.bridge.ceiling"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            service: runtime_provider_service(&runtime),
            role: ServiceRole::Terminal,
            priority: 0,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::new([bridge_capability.clone()]),
    };
    let guest = PluginManifest {
        id: plugin("fixture.guest.ceiling"),
        version: 1,
        execution: PluginExecution::Runtime {
            runtime,
            artifact: PluginArtifact {
                locator: "fixture.plugin".into(),
                revision: ArtifactRevision::from_content(b"runtime-provider-ceiling"),
                configuration: BTreeMap::new(),
            },
        },
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::new([guest_capability.clone()]),
    };
    let ceiling = Authority::new([guest_capability.clone()]);
    let resolved = ResolvedHarness::resolve([bridge.clone(), guest], [], [], &ceiling).unwrap();

    let provider_authority = Arc::new(Mutex::new(None));
    let guest_authority = Arc::new(Mutex::new(None));
    let guest_started_authority = Arc::new(Mutex::new(None));
    let provider_had_cancellation = Arc::new(Mutex::new(false));
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    kernel.activate_resolved_harness(&resolved).unwrap();

    let provider_authority_for_factory = Arc::clone(&provider_authority);
    let guest_authority_for_factory = Arc::clone(&guest_authority);
    let guest_started_authority_for_factory = Arc::clone(&guest_started_authority);
    let provider_had_cancellation_for_factory = Arc::clone(&provider_had_cancellation);
    kernel
        .register_embedded_factory(bridge.id.clone(), move || {
            Box::new(HostAwareBridge {
                provider_authority: Arc::clone(&provider_authority_for_factory),
                guest_authority: Arc::clone(&guest_authority_for_factory),
                guest_started_authority: Arc::clone(&guest_started_authority_for_factory),
                provider_had_cancellation: Arc::clone(&provider_had_cancellation_for_factory),
            })
        })
        .unwrap();

    kernel.activate_all().unwrap();

    let provider_authority = provider_authority.lock().unwrap().clone().unwrap();
    assert!(!provider_authority.permits(&bridge_capability));
    assert!(!provider_authority.permits(&guest_capability));

    let guest_authority = guest_authority.lock().unwrap().clone().unwrap();
    assert!(guest_authority.permits(&guest_capability));
    assert!(!guest_authority.permits(&bridge_capability));
    assert_eq!(
        guest_started_authority.lock().unwrap().clone().unwrap(),
        guest_authority
    );
    assert!(*provider_had_cancellation.lock().unwrap());
}
