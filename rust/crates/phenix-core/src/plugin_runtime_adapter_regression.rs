use crate::{
    plugin_runtime_adapter_service, ArtifactRevision, Authority, GraphReconciler, Kernel,
    KernelConfig, KernelError, LiveReconciliationError, PermissionId, PluginArtifact,
    PluginExecution, PluginHost, PluginId, PluginInstance, PluginManifest, PluginRuntimeAdapter,
    PluginRuntimeCandidate, PluginRuntimeId, ResolvedGeneration, ResolvedGenerationActivation,
    ServiceContribution, ServiceRole,
};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn runtime(value: &str) -> PluginRuntimeId {
    PluginRuntimeId::parse(value).unwrap()
}

fn artifact(revision: &str) -> PluginArtifact {
    PluginArtifact {
        locator: "fixture.plugin".into(),
        revision: ArtifactRevision::from_content(revision.as_bytes()),
        configuration: BTreeMap::new(),
    }
}

fn adapter_manifest(
    id: &str,
    runtime: &PluginRuntimeId,
    execution: PluginExecution,
) -> PluginManifest {
    PluginManifest {
        id: plugin(id),
        version: 1,
        execution,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            service: plugin_runtime_adapter_service(runtime),
            role: ServiceRole::Terminal,
            priority: 0,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn guest_manifest(
    id: &str,
    runtime: PluginRuntimeId,
    revision: &str,
    authority: Authority,
) -> PluginManifest {
    PluginManifest {
        id: plugin(id),
        version: 1,
        execution: PluginExecution::Runtime {
            runtime,
            artifact: artifact(revision),
        },
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: authority,
    }
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

struct FixturePluginRuntimeAdapter {
    fail: Arc<AtomicBool>,
    prepared_authority: Arc<Mutex<Option<Authority>>>,
    started_authority: Arc<Mutex<Option<Authority>>>,
}

impl PluginRuntimeAdapter for FixturePluginRuntimeAdapter {
    fn prepare(
        &mut self,
        candidate: PluginRuntimeCandidate<'_>,
    ) -> Result<Box<dyn PluginInstance>, String> {
        *self.prepared_authority.lock().unwrap() = Some(candidate.guest_authority.clone());
        if self.fail.load(Ordering::Acquire) {
            return Err("candidate rejected".into());
        }
        Ok(Box::new(PreparedGuest {
            started_authority: Arc::clone(&self.started_authority),
        }))
    }
}

impl PluginInstance for FixturePluginRuntimeAdapter {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn plugin_runtime_adapter(&mut self) -> Option<&mut dyn PluginRuntimeAdapter> {
        Some(self)
    }
}

#[test]
fn arbitrary_runtime_identity_resolves_through_an_embedded_plugin_runtime_adapter() {
    let runtime = runtime("vendor.runtime");
    let adapter_plugin = adapter_manifest("fixture.bridge", &runtime, PluginExecution::Embedded);
    let guest = guest_manifest(
        "fixture.guest",
        runtime.clone(),
        "sha256:guest-v1",
        Authority::default(),
    );

    let config = KernelConfig::new([guest.clone(), adapter_plugin.clone()]).unwrap();
    let binding = config.plugin_runtime_binding(&guest.id).unwrap();

    assert_eq!(binding.runtime, runtime);
    assert_eq!(binding.adapter_plugin, adapter_plugin.id);
    assert_eq!(
        binding.artifact_revision,
        ArtifactRevision::from_content(b"sha256:guest-v1")
    );
    assert_eq!(config.activation_order(), &[adapter_plugin.id, guest.id]);
}

#[test]
fn unknown_runtime_is_rejected_during_graph_resolution() {
    let runtime = runtime("missing.runtime");
    let guest = guest_manifest(
        "fixture.guest",
        runtime.clone(),
        "sha256:guest-v1",
        Authority::default(),
    );

    assert_eq!(
        KernelConfig::new([guest]).unwrap_err(),
        KernelError::PluginRuntimeAdapterUnavailable(runtime)
    );
}

#[test]
fn plugin_runtime_adapter_cycles_are_rejected_by_the_normal_dependency_graph() {
    let runtime_a = runtime("runtime.a");
    let runtime_b = runtime("runtime.b");
    let bridge_a = adapter_manifest(
        "bridge.a",
        &runtime_a,
        PluginExecution::Runtime {
            runtime: runtime_b.clone(),
            artifact: artifact("sha256:a"),
        },
    );
    let bridge_b = adapter_manifest(
        "bridge.b",
        &runtime_b,
        PluginExecution::Runtime {
            runtime: runtime_a,
            artifact: artifact("sha256:b"),
        },
    );

    assert!(matches!(
        KernelConfig::new([bridge_a, bridge_b]),
        Err(KernelError::DependencyCycle(_))
    ));
}

#[test]
fn guest_authority_is_independent_from_adapter_authority() {
    let runtime = runtime("vendor.runtime");
    let guest_permission = PermissionId::parse("guest.read").unwrap();
    let adapter_permission = PermissionId::parse("bridge.exec").unwrap();
    let guest_authority = Authority::new([guest_permission.clone()]);
    let mut adapter_plugin =
        adapter_manifest("fixture.bridge", &runtime, PluginExecution::Embedded);
    adapter_plugin.maximum_authority = Authority::new([adapter_permission.clone()]);
    let guest = guest_manifest(
        "fixture.guest",
        runtime,
        "sha256:guest-v1",
        guest_authority.clone(),
    );
    let prepared_authority = Arc::new(Mutex::new(None));
    let started_authority = Arc::new(Mutex::new(None));
    let fail = Arc::new(AtomicBool::new(false));
    let mut kernel = Kernel::new(KernelConfig::new([adapter_plugin.clone(), guest]).unwrap());
    let prepared_for_factory = Arc::clone(&prepared_authority);
    let started_for_factory = Arc::clone(&started_authority);
    let fail_for_factory = Arc::clone(&fail);
    kernel
        .register_embedded_factory(adapter_plugin.id, move || {
            Box::new(FixturePluginRuntimeAdapter {
                fail: Arc::clone(&fail_for_factory),
                prepared_authority: Arc::clone(&prepared_for_factory),
                started_authority: Arc::clone(&started_for_factory),
            })
        })
        .unwrap();

    kernel.activate_all().unwrap();

    let prepared = prepared_authority.lock().unwrap().clone().unwrap();
    let started = started_authority.lock().unwrap().clone().unwrap();
    assert!(prepared.permits(&guest_permission));
    assert!(!prepared.permits(&adapter_permission));
    assert_eq!(prepared, guest_authority);
    assert_eq!(started, guest_authority);
}

#[test]
fn artifact_revision_and_resolved_adapter_plugin_are_pinned_by_generation() {
    let runtime = runtime("vendor.runtime");
    let adapter_plugin = adapter_manifest("fixture.bridge", &runtime, PluginExecution::Embedded);
    let first_guest = guest_manifest(
        "fixture.guest",
        runtime.clone(),
        "sha256:guest-v1",
        Authority::default(),
    );
    let second_guest = guest_manifest(
        "fixture.guest",
        runtime,
        "sha256:guest-v2",
        Authority::default(),
    );
    let first = ResolvedGeneration::resolve(
        [adapter_plugin.clone(), first_guest],
        [],
        [],
        &Authority::default(),
    )
    .unwrap();
    let second = ResolvedGeneration::resolve(
        [adapter_plugin, second_guest],
        [],
        [],
        &Authority::default(),
    )
    .unwrap();

    assert_ne!(first.generation(), second.generation());
    assert_eq!(
        first
            .kernel_config()
            .plugin_runtime_binding(&plugin("fixture.guest"))
            .unwrap()
            .adapter_plugin,
        plugin("fixture.bridge")
    );
    assert_eq!(
        first
            .kernel_config()
            .plugin_runtime_binding(&plugin("fixture.guest"))
            .unwrap()
            .artifact_revision,
        ArtifactRevision::from_content(b"sha256:guest-v1")
    );
}

#[test]
fn failed_runtime_prepare_keeps_the_active_generation() {
    let runtime = runtime("vendor.runtime");
    let adapter_plugin = adapter_manifest("fixture.bridge", &runtime, PluginExecution::Embedded);
    let initial_guest = guest_manifest(
        "fixture.guest",
        runtime.clone(),
        "sha256:guest-v1",
        Authority::default(),
    );
    let candidate_guest = guest_manifest(
        "fixture.guest",
        runtime,
        "sha256:guest-v2",
        Authority::default(),
    );
    let initial = ResolvedGeneration::resolve(
        [adapter_plugin.clone(), initial_guest],
        [],
        [],
        &Authority::default(),
    )
    .unwrap();
    let candidate = ResolvedGeneration::resolve(
        [adapter_plugin.clone(), candidate_guest],
        [],
        [],
        &Authority::default(),
    )
    .unwrap();
    let active_generation = initial.generation().clone();
    let fail = Arc::new(AtomicBool::new(false));
    let prepared_authority = Arc::new(Mutex::new(None));
    let started_authority = Arc::new(Mutex::new(None));
    let mut kernel = Kernel::new(initial.kernel_config().clone());
    kernel.activate_resolved_generation(&initial).unwrap();
    let fail_for_factory = Arc::clone(&fail);
    let prepared_for_factory = Arc::clone(&prepared_authority);
    let started_for_factory = Arc::clone(&started_authority);
    kernel
        .register_embedded_factory(adapter_plugin.id, move || {
            Box::new(FixturePluginRuntimeAdapter {
                fail: Arc::clone(&fail_for_factory),
                prepared_authority: Arc::clone(&prepared_for_factory),
                started_authority: Arc::clone(&started_for_factory),
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();
    fail.store(true, Ordering::Release);
    let mut reconciler = GraphReconciler::new(initial);

    let error = reconciler
        .activate_candidate_on_kernel(&mut kernel, candidate)
        .unwrap_err();

    assert!(matches!(
        error,
        LiveReconciliationError::Runtime(KernelError::PluginRuntimePreparation { .. })
    ));
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
    assert_eq!(reconciler.active().generation(), &active_generation);
}

struct StartFailGuest {
    fail: Arc<AtomicBool>,
}

impl PluginInstance for StartFailGuest {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        if self.fail.load(Ordering::Acquire) {
            return Err("candidate start rejected".into());
        }
        Ok(())
    }
}

struct StartFailPluginRuntimeAdapter {
    fail: Arc<AtomicBool>,
}

impl PluginRuntimeAdapter for StartFailPluginRuntimeAdapter {
    fn prepare(
        &mut self,
        _candidate: PluginRuntimeCandidate<'_>,
    ) -> Result<Box<dyn PluginInstance>, String> {
        Ok(Box::new(StartFailGuest {
            fail: Arc::clone(&self.fail),
        }))
    }
}

impl PluginInstance for StartFailPluginRuntimeAdapter {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn plugin_runtime_adapter(&mut self) -> Option<&mut dyn PluginRuntimeAdapter> {
        Some(self)
    }
}

#[test]
fn unknown_runtime_candidate_keeps_the_active_generation() {
    let active_runtime = runtime("vendor.runtime");
    let adapter_plugin =
        adapter_manifest("fixture.bridge", &active_runtime, PluginExecution::Embedded);
    let guest = guest_manifest(
        "fixture.guest",
        active_runtime,
        "sha256:guest-v1",
        Authority::default(),
    );
    let active = ResolvedGeneration::resolve(
        [adapter_plugin.clone(), guest],
        [],
        [],
        &Authority::default(),
    )
    .unwrap();
    let active_generation = active.generation().clone();
    let fail = Arc::new(AtomicBool::new(false));
    let prepared_authority = Arc::new(Mutex::new(None));
    let started_authority = Arc::new(Mutex::new(None));
    let mut kernel = Kernel::new(active.kernel_config().clone());
    kernel.activate_resolved_generation(&active).unwrap();
    kernel
        .register_embedded_factory(adapter_plugin.id, move || {
            Box::new(FixturePluginRuntimeAdapter {
                fail: Arc::clone(&fail),
                prepared_authority: Arc::clone(&prepared_authority),
                started_authority: Arc::clone(&started_authority),
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();

    let missing = runtime("missing.runtime");
    let invalid_guest = guest_manifest(
        "fixture.guest",
        missing.clone(),
        "sha256:guest-v2",
        Authority::default(),
    );
    let error =
        ResolvedGeneration::resolve([invalid_guest], [], [], &Authority::default()).unwrap_err();

    assert!(matches!(
        error,
        crate::GenerationResolutionError::Kernel(KernelError::PluginRuntimeAdapterUnavailable(runtime))
            if runtime == missing
    ));
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
}

#[test]
fn failed_runtime_start_keeps_the_active_generation() {
    let runtime = runtime("vendor.runtime");
    let adapter_plugin = adapter_manifest("fixture.bridge", &runtime, PluginExecution::Embedded);
    let initial_guest = guest_manifest(
        "fixture.guest",
        runtime.clone(),
        "sha256:guest-v1",
        Authority::default(),
    );
    let candidate_guest = guest_manifest(
        "fixture.guest",
        runtime,
        "sha256:guest-v2",
        Authority::default(),
    );
    let initial = ResolvedGeneration::resolve(
        [adapter_plugin.clone(), initial_guest],
        [],
        [],
        &Authority::default(),
    )
    .unwrap();
    let candidate = ResolvedGeneration::resolve(
        [adapter_plugin.clone(), candidate_guest],
        [],
        [],
        &Authority::default(),
    )
    .unwrap();
    let active_generation = initial.generation().clone();
    let fail = Arc::new(AtomicBool::new(false));
    let fail_for_factory = Arc::clone(&fail);
    let mut kernel = Kernel::new(initial.kernel_config().clone());
    kernel.activate_resolved_generation(&initial).unwrap();
    kernel
        .register_embedded_factory(adapter_plugin.id, move || {
            Box::new(StartFailPluginRuntimeAdapter {
                fail: Arc::clone(&fail_for_factory),
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();
    fail.store(true, Ordering::Release);
    let mut reconciler = GraphReconciler::new(initial);

    let error = reconciler
        .activate_candidate_on_kernel(&mut kernel, candidate)
        .unwrap_err();

    assert!(matches!(
        error,
        LiveReconciliationError::Runtime(KernelError::PluginStart { .. })
    ));
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
    assert_eq!(reconciler.active().generation(), &active_generation);
}
