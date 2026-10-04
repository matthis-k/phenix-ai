use crate::{
    runtime_provider_service, ArtifactRevision, Authority, CallableId, ComponentEntryTrigger,
    ComponentExport, ComponentId, ComponentImport, ComponentManifest, EntryTriggerKind,
    GraphReconciler, InterfaceId, Kernel, KernelError, PluginArtifact, PluginArtifactInput,
    PluginArtifactStore, PluginArtifactStoreError, PluginBuildExecution, PluginBuildExecutor,
    PluginBuildFailure, PluginBuildPlan, PluginExecution, PluginHost, PluginId, PluginInstance,
    PluginLoadRequest, PluginManagementContext, PluginManagementError, PluginManagementPolicy,
    PluginManagementRequest, PluginManagementResult, PluginManifest, PluginRuntimeProvider,
    PluginSetRequest, PluginState, PluginUnloadRequest, ResolvedHarness, ResolvedHarnessActivation,
    RuntimeId, RuntimePluginCandidate, ServiceContribution, ServiceId, ServiceRole,
};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn runtime(value: &str) -> RuntimeId {
    RuntimeId::parse(value).unwrap()
}

fn service(value: &str) -> ServiceId {
    ServiceId::parse(value).unwrap()
}

fn interface(value: &str) -> InterfaceId {
    InterfaceId::parse(value).unwrap()
}

fn artifact(revision: &str) -> PluginArtifact {
    PluginArtifact {
        locator: "fixture.plugin".into(),
        revision: ArtifactRevision::from_content(revision.as_bytes()),
        configuration: BTreeMap::new(),
    }
}

fn ready(manifest: PluginManifest) -> PluginManifest<PluginArtifactInput> {
    manifest.map_artifact(PluginArtifactInput::Ready)
}

struct TestArtifactStore;

impl PluginArtifactStore for TestArtifactStore {
    fn preflight(&mut self) -> Result<(), PluginArtifactStoreError> {
        Ok(())
    }

    fn verify_ready(&mut self, _artifact: &PluginArtifact) -> Result<(), PluginArtifactStoreError> {
        Ok(())
    }

    fn store_built(
        &mut self,
        artifact: &PluginArtifact,
        _content: &[u8],
    ) -> Result<String, PluginArtifactStoreError> {
        Ok(artifact.locator.clone())
    }
}

struct UnexpectedBuildExecutor;

impl PluginBuildExecutor for UnexpectedBuildExecutor {
    fn execute(
        &mut self,
        _plan: &PluginBuildPlan,
        _effective_authority: &Authority,
    ) -> Result<PluginBuildExecution, PluginBuildFailure> {
        panic!("ready-artifact management must not execute a build")
    }
}

fn manage(
    reconciler: &mut GraphReconciler,
    kernel: &mut Kernel,
    request: PluginManagementRequest,
    authority_ceiling: &Authority,
) -> Result<PluginManagementResult, PluginManagementError> {
    let policy = PluginManagementPolicy::new(Authority::default(), Authority::default());
    let mut artifact_store = TestArtifactStore;
    let mut build_executor = UnexpectedBuildExecutor;
    reconciler.manage(
        kernel,
        request,
        authority_ceiling,
        &mut PluginManagementContext {
            caller_authority: authority_ceiling,
            policy: &policy,
            artifact_store: &mut artifact_store,
            build_executor: &mut build_executor,
        },
    )
}

fn trial(
    reconciler: &mut GraphReconciler,
    kernel: &mut Kernel,
    request: PluginManagementRequest,
    authority_ceiling: &Authority,
    constraints: &crate::RootExecutionConstraints,
) -> Result<crate::PluginTrialResult, PluginManagementError> {
    let policy = PluginManagementPolicy::new(Authority::default(), Authority::default());
    let mut artifact_store = TestArtifactStore;
    let mut build_executor = UnexpectedBuildExecutor;
    reconciler.trial_management(
        kernel,
        request,
        authority_ceiling,
        constraints,
        &mut PluginManagementContext {
            caller_authority: authority_ceiling,
            policy: &policy,
            artifact_store: &mut artifact_store,
            build_executor: &mut build_executor,
        },
    )
}

fn embedded_manifest(id: &str, service: Option<ServiceId>) -> PluginManifest {
    PluginManifest {
        id: plugin(id),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: service
            .map(|service| ServiceContribution {
                service,
                role: ServiceRole::Terminal,
                priority: 0,
                required_authority: Authority::default(),
            })
            .into_iter()
            .collect(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn bridge_manifest(id: &str, provided_runtime: &RuntimeId) -> PluginManifest {
    PluginManifest {
        id: plugin(id),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            service: runtime_provider_service(provided_runtime),
            role: ServiceRole::Terminal,
            priority: 0,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn guest_manifest(id: &str, runtime: RuntimeId, revision: &str) -> PluginManifest {
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
        maximum_authority: Authority::default(),
    }
}

fn consumer_component(owner: &PluginId, interface: &InterfaceId) -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse("consumer.component").unwrap(),
        owner: owner.clone(),
        imports: vec![ComponentImport {
            interface: interface.clone(),
            schema: Default::default(),
            required: true,
            authority: Authority::default(),
        }],
        exports: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn provider_component(owner: &PluginId, interface: &InterfaceId) -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse("provider.component").unwrap(),
        owner: owner.clone(),
        imports: Vec::new(),
        exports: vec![ComponentExport {
            interface: interface.clone(),
            schema: Default::default(),
            priority: 1,
            required_authority: Authority::default(),
        }],
        maximum_authority: Authority::default(),
    }
}

struct TrackingInstance {
    starts: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
    response: Vec<u8>,
}

impl PluginInstance for TrackingInstance {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        self.starts.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        _input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        Ok(self.response.clone())
    }

    fn stop(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        self.stops.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

struct Guest {
    starts: Arc<AtomicUsize>,
    revision: String,
}

impl PluginInstance for Guest {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        self.starts.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        _input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        Ok(self.revision.clone().into_bytes())
    }
}

struct Bridge {
    fail: Arc<AtomicBool>,
    guest_starts: Arc<AtomicUsize>,
}

impl PluginRuntimeProvider for Bridge {
    fn prepare(
        &mut self,
        candidate: RuntimePluginCandidate<'_>,
    ) -> Result<Box<dyn PluginInstance>, String> {
        if self.fail.load(Ordering::Acquire) {
            return Err("candidate rejected".into());
        }
        Ok(Box::new(Guest {
            starts: Arc::clone(&self.guest_starts),
            revision: candidate.artifact.revision.to_string(),
        }))
    }
}

impl PluginInstance for Bridge {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn runtime_provider(&mut self) -> Option<&mut dyn PluginRuntimeProvider> {
        Some(self)
    }
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

struct StartFailBridge {
    fail: Arc<AtomicBool>,
}

impl PluginRuntimeProvider for StartFailBridge {
    fn prepare(
        &mut self,
        _candidate: RuntimePluginCandidate<'_>,
    ) -> Result<Box<dyn PluginInstance>, String> {
        Ok(Box::new(StartFailGuest {
            fail: Arc::clone(&self.fail),
        }))
    }
}

impl PluginInstance for StartFailBridge {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn runtime_provider(&mut self) -> Option<&mut dyn PluginRuntimeProvider> {
        Some(self)
    }
}

fn activate_runtime_fixture(
    initial: &ResolvedHarness,
    bridge: &PluginManifest,
    fail: Arc<AtomicBool>,
    guest_starts: Arc<AtomicUsize>,
) -> Kernel {
    let mut kernel = Kernel::new(initial.kernel_config().clone());
    kernel.activate_resolved_harness(initial).unwrap();
    let fail_for_factory = Arc::clone(&fail);
    let starts_for_factory = Arc::clone(&guest_starts);
    kernel
        .register_embedded_factory(bridge.id.clone(), move || {
            Box::new(Bridge {
                fail: Arc::clone(&fail_for_factory),
                guest_starts: Arc::clone(&starts_for_factory),
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

#[test]
fn trial_management_stages_candidate_without_changing_the_active_generation() {
    let echo = service("fixture.trial.echo@1");
    let first = embedded_manifest("fixture.trial.plugin", Some(echo.clone()));
    let mut second = first.clone();
    second.version = 2;
    let initial = ResolvedHarness::resolve([first.clone()], [], [], &Authority::default()).unwrap();
    let active_generation = initial.generation().clone();
    let starts = Arc::new(AtomicUsize::new(0));
    let stops = Arc::new(AtomicUsize::new(0));

    let mut kernel = Kernel::new(initial.kernel_config().clone());
    kernel.activate_resolved_harness(&initial).unwrap();
    let starts_for_factory = Arc::clone(&starts);
    let stops_for_factory = Arc::clone(&stops);
    kernel
        .register_embedded_factory(first.id.clone(), move || {
            Box::new(TrackingInstance {
                starts: Arc::clone(&starts_for_factory),
                stops: Arc::clone(&stops_for_factory),
                response: b"trial".to_vec(),
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();

    let constraints = kernel
        .capture_root_execution_constraints(&Authority::default(), [])
        .unwrap();
    let mut reconciler = GraphReconciler::new(initial);
    let result = trial(
        &mut reconciler,
        &mut kernel,
        PluginManagementRequest::load(PluginLoadRequest {
            manifest: ready(second),
            components: Vec::new(),
            entry_triggers: Vec::new(),
            expected_active_revision: None,
        }),
        &Authority::default(),
        &constraints,
    )
    .unwrap();

    assert_ne!(result.generation, active_generation);
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
    assert_eq!(reconciler.active().generation(), &active_generation);
    assert_eq!(
        reconciler
            .resident(&result.generation)
            .expect("trial candidate remains inspectable")
            .generation(),
        &result.generation
    );
    assert!(kernel
        .resident_generation_ids()
        .contains(&result.generation));
    assert_eq!(starts.load(Ordering::Relaxed), 2);
    assert_eq!(stops.load(Ordering::Relaxed), 0);
    assert_eq!(
        kernel
            .invoke_in_generation(&result.generation, &echo, b"probe", &constraints, None,)
            .unwrap(),
        b"trial"
    );
}

#[test]
fn trial_management_can_add_plugin_owned_model_tool_triggers() {
    let tool_interface = interface("fixture.trial.tool@1");
    let first = embedded_manifest(
        "fixture.trial.tool-plugin",
        Some(ServiceId::parse(tool_interface.as_str()).unwrap()),
    );
    let component = provider_component(&first.id, &tool_interface);
    let initial = ResolvedHarness::resolve(
        [first.clone()],
        [component.clone()],
        [],
        &Authority::default(),
    )
    .unwrap();
    let active_generation = initial.generation().clone();
    let starts = Arc::new(AtomicUsize::new(0));
    let stops = Arc::new(AtomicUsize::new(0));

    let mut kernel = Kernel::new(initial.kernel_config().clone());
    kernel.activate_resolved_harness(&initial).unwrap();
    let starts_for_factory = Arc::clone(&starts);
    let stops_for_factory = Arc::clone(&stops);
    kernel
        .register_embedded_factory(first.id.clone(), move || {
            Box::new(TrackingInstance {
                starts: Arc::clone(&starts_for_factory),
                stops: Arc::clone(&stops_for_factory),
                response: b"tool".to_vec(),
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();

    let mut second = first;
    second.version = 2;
    let trigger = ComponentEntryTrigger {
        component: component.id.clone(),
        interface: tool_interface,
        trigger: EntryTriggerKind::ToolCall {
            callable_id: CallableId::parse("fixture.trial.tool").unwrap(),
            description: "Generation-local trial tool".into(),
        },
        required_authority: Authority::default(),
    };
    let constraints = kernel
        .capture_root_execution_constraints(&Authority::default(), [])
        .unwrap();
    let mut reconciler = GraphReconciler::new(initial);
    let result = trial(
        &mut reconciler,
        &mut kernel,
        PluginManagementRequest::load(PluginLoadRequest {
            manifest: ready(second),
            components: vec![component],
            entry_triggers: vec![trigger.clone()],
            expected_active_revision: None,
        }),
        &Authority::default(),
        &constraints,
    )
    .unwrap();

    assert_eq!(reconciler.active().generation(), &active_generation);
    assert!(reconciler.active().entry_triggers().is_empty());
    assert_eq!(
        reconciler
            .resident(&result.generation)
            .expect("trial generation remains resident")
            .entry_triggers(),
        &[trigger]
    );
    assert_eq!(starts.load(Ordering::Relaxed), 2);
    assert_eq!(stops.load(Ordering::Relaxed), 0);
}

#[test]
fn load_activates_a_new_guest_in_a_new_generation() {
    let runtime = runtime("vendor.runtime");
    let bridge = bridge_manifest("fixture.bridge", &runtime);
    let guest = guest_manifest("fixture.guest", runtime, "sha256:guest-v1");
    let guest_component = ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse("fixture.guest.component").unwrap(),
        owner: guest.id.clone(),
        imports: Vec::new(),
        exports: Vec::new(),
        maximum_authority: Authority::default(),
    };
    let initial =
        ResolvedHarness::resolve([bridge.clone()], [], [], &Authority::default()).unwrap();
    let initial_generation = initial.generation().clone();
    let guest_starts = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(AtomicBool::new(false));
    let mut kernel = activate_runtime_fixture(
        &initial,
        &bridge,
        Arc::clone(&fail),
        Arc::clone(&guest_starts),
    );
    let mut reconciler = GraphReconciler::new(initial);

    let result = manage(
        &mut reconciler,
        &mut kernel,
        PluginManagementRequest::load(PluginLoadRequest {
            manifest: ready(guest.clone()),
            components: vec![guest_component.clone()],
            entry_triggers: Vec::new(),
            expected_active_revision: None,
        }),
        &Authority::default(),
    )
    .unwrap();

    assert_ne!(result.reconciliation.active_generation, initial_generation);
    assert_eq!(
        kernel.graph_generation(),
        Some(&result.reconciliation.active_generation)
    );
    assert_eq!(kernel.config().manifest(&guest.id), Some(&guest));
    assert!(kernel
        .component_graph()
        .component(&guest_component.id)
        .is_some());
    assert_eq!(kernel.state(&guest.id), Some(PluginState::Active));
    assert_eq!(guest_starts.load(Ordering::Relaxed), 1);
}

#[test]
fn unload_stops_an_active_guest_and_commits_a_new_generation() {
    let runtime = runtime("vendor.runtime");
    let bridge = bridge_manifest("fixture.bridge", &runtime);
    let guest = guest_manifest("fixture.guest", runtime, "sha256:guest-v1");
    let initial = ResolvedHarness::resolve(
        [bridge.clone(), guest.clone()],
        [],
        [],
        &Authority::default(),
    )
    .unwrap();
    let initial_generation = initial.generation().clone();
    let guest_starts = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(AtomicBool::new(false));
    let mut kernel = activate_runtime_fixture(
        &initial,
        &bridge,
        Arc::clone(&fail),
        Arc::clone(&guest_starts),
    );
    assert_eq!(guest_starts.load(Ordering::Relaxed), 1);
    let mut reconciler = GraphReconciler::new(initial);

    let result = manage(
        &mut reconciler,
        &mut kernel,
        PluginManagementRequest::Unload(PluginUnloadRequest {
            plugin: guest.id.clone(),
            expected_active_revision: None,
        }),
        &Authority::default(),
    )
    .unwrap();

    assert_ne!(result.reconciliation.active_generation, initial_generation);
    assert_eq!(
        kernel.graph_generation(),
        Some(&result.reconciliation.active_generation)
    );
    assert!(kernel.config().manifest(&guest.id).is_none());
    assert_eq!(kernel.state(&guest.id), None);
    assert_eq!(kernel.state(&bridge.id), Some(PluginState::Active));
}

#[test]
fn loading_an_active_plugin_with_a_new_artifact_is_a_replacement() {
    let runtime = runtime("vendor.runtime");
    let bridge = bridge_manifest("fixture.bridge", &runtime);
    let first_guest = guest_manifest("fixture.guest", runtime.clone(), "sha256:guest-v1");
    let second_guest = guest_manifest("fixture.guest", runtime, "sha256:guest-v2");
    let initial =
        ResolvedHarness::resolve([bridge.clone(), first_guest], [], [], &Authority::default())
            .unwrap();
    let initial_generation = initial.generation().clone();
    let guest_starts = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(AtomicBool::new(false));
    let mut kernel = activate_runtime_fixture(
        &initial,
        &bridge,
        Arc::clone(&fail),
        Arc::clone(&guest_starts),
    );
    assert_eq!(guest_starts.load(Ordering::Relaxed), 1);
    let mut reconciler = GraphReconciler::new(initial);

    let result = manage(
        &mut reconciler,
        &mut kernel,
        PluginManagementRequest::load(PluginLoadRequest {
            manifest: ready(second_guest.clone()),
            components: Vec::new(),
            entry_triggers: Vec::new(),
            expected_active_revision: None,
        }),
        &Authority::default(),
    )
    .unwrap();

    assert_ne!(result.reconciliation.active_generation, initial_generation);
    assert_eq!(
        kernel.config().manifest(&second_guest.id),
        Some(&second_guest)
    );
    assert_eq!(kernel.state(&second_guest.id), Some(PluginState::Active));
    assert_eq!(guest_starts.load(Ordering::Relaxed), 2);
}

#[test]
fn stale_expected_revision_is_rejected_before_commit() {
    let runtime = runtime("vendor.runtime");
    let bridge = bridge_manifest("fixture.bridge", &runtime);
    let active_guest = guest_manifest("fixture.guest", runtime.clone(), "sha256:guest-v1");
    let candidate_guest = guest_manifest("fixture.guest", runtime, "sha256:guest-v2");
    let initial = ResolvedHarness::resolve(
        [bridge.clone(), active_guest],
        [],
        [],
        &Authority::default(),
    )
    .unwrap();
    let active_generation = initial.generation().clone();
    let guest_starts = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(AtomicBool::new(false));
    let mut kernel = activate_runtime_fixture(
        &initial,
        &bridge,
        Arc::clone(&fail),
        Arc::clone(&guest_starts),
    );
    let mut reconciler = GraphReconciler::new(initial);

    let error = manage(
        &mut reconciler,
        &mut kernel,
        PluginManagementRequest::load(PluginLoadRequest {
            manifest: ready(candidate_guest),
            components: Vec::new(),
            entry_triggers: Vec::new(),
            expected_active_revision: Some(ArtifactRevision::from_content(b"unexpected")),
        }),
        &Authority::default(),
    )
    .unwrap_err();

    assert_eq!(
        error,
        PluginManagementError::StaleExpectedRevision {
            plugin: plugin("fixture.guest"),
            expected: ArtifactRevision::from_content(b"unexpected"),
            active: Some(ArtifactRevision::from_content(b"sha256:guest-v1")),
        }
    );
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
    assert_eq!(reconciler.active().generation(), &active_generation);
}

#[test]
fn expected_revision_rejects_load_when_plugin_is_not_active() {
    let runtime = runtime("vendor.runtime");
    let bridge = bridge_manifest("fixture.bridge", &runtime);
    let guest = guest_manifest("fixture.guest", runtime, "sha256:guest-v1");
    let initial =
        ResolvedHarness::resolve([bridge.clone()], [], [], &Authority::default()).unwrap();
    let active_generation = initial.generation().clone();
    let mut kernel = Kernel::new(initial.kernel_config().clone());
    kernel.activate_resolved_harness(&initial).unwrap();
    let mut reconciler = GraphReconciler::new(initial);

    let error = manage(
        &mut reconciler,
        &mut kernel,
        PluginManagementRequest::load(PluginLoadRequest {
            manifest: ready(guest),
            components: Vec::new(),
            entry_triggers: Vec::new(),
            expected_active_revision: Some(ArtifactRevision::from_content(b"guest-v0")),
        }),
        &Authority::default(),
    )
    .unwrap_err();

    assert_eq!(
        error,
        PluginManagementError::StaleExpectedRevision {
            plugin: plugin("fixture.guest"),
            expected: ArtifactRevision::from_content(b"guest-v0"),
            active: None,
        }
    );
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
}

#[test]
fn failed_start_keeps_the_previous_generation_active() {
    let runtime = runtime("vendor.runtime");
    let bridge = bridge_manifest("fixture.bridge", &runtime);
    let active_guest = guest_manifest("fixture.guest", runtime.clone(), "sha256:guest-v1");
    let candidate_guest = guest_manifest("fixture.guest", runtime, "sha256:guest-v2");
    let initial = ResolvedHarness::resolve(
        [bridge.clone(), active_guest],
        [],
        [],
        &Authority::default(),
    )
    .unwrap();
    let active_generation = initial.generation().clone();
    let fail = Arc::new(AtomicBool::new(false));
    let mut kernel = Kernel::new(initial.kernel_config().clone());
    kernel.activate_resolved_harness(&initial).unwrap();
    let fail_for_factory = Arc::clone(&fail);
    kernel
        .register_embedded_factory(bridge.id.clone(), move || {
            Box::new(StartFailBridge {
                fail: Arc::clone(&fail_for_factory),
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();
    fail.store(true, Ordering::Release);
    let mut reconciler = GraphReconciler::new(initial);

    let error = manage(
        &mut reconciler,
        &mut kernel,
        PluginManagementRequest::load(PluginLoadRequest {
            manifest: ready(candidate_guest),
            components: Vec::new(),
            entry_triggers: Vec::new(),
            expected_active_revision: None,
        }),
        &Authority::default(),
    )
    .unwrap_err();

    let PluginManagementError::Reconciliation { error, build: None } = error else {
        panic!("candidate start should fail during reconciliation");
    };
    assert!(matches!(
        *error,
        crate::LiveReconciliationError::Runtime(KernelError::PluginStart { .. })
    ));
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
    assert_eq!(reconciler.active().generation(), &active_generation);
}

#[test]
fn unload_is_rejected_when_required_imports_become_unsatisfied() {
    let echo_interface = interface("fixture.echo@1");
    let consumer_owner = embedded_manifest("fixture.consumer", None);
    let provider_owner = embedded_manifest("fixture.provider", None);
    let initial = ResolvedHarness::resolve(
        [consumer_owner.clone(), provider_owner.clone()],
        [
            consumer_component(&consumer_owner.id, &echo_interface),
            provider_component(&provider_owner.id, &echo_interface),
        ],
        [],
        &Authority::default(),
    )
    .unwrap();
    let active_generation = initial.generation().clone();
    let mut kernel = Kernel::new(initial.kernel_config().clone());
    kernel.activate_resolved_harness(&initial).unwrap();
    let mut reconciler = GraphReconciler::new(initial);

    let error = manage(
        &mut reconciler,
        &mut kernel,
        PluginManagementRequest::Unload(PluginUnloadRequest {
            plugin: provider_owner.id.clone(),
            expected_active_revision: None,
        }),
        &Authority::default(),
    )
    .unwrap_err();

    let PluginManagementError::Candidate { error, build: None } = error else {
        panic!("missing import should fail candidate resolution");
    };
    assert!(matches!(
        *error,
        crate::ResolvedHarnessError::ComponentGraph(
            crate::ComponentGraphError::MissingRequiredImport {
                component,
                interface: missing_interface,
            }
        ) if component == ComponentId::parse("consumer.component").unwrap()
            && missing_interface == interface("fixture.echo@1")
    ));
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
    assert_eq!(reconciler.active().generation(), &active_generation);
}

#[test]
fn unknown_runtime_is_rejected_before_commit() {
    let vendor_runtime = runtime("vendor.runtime");
    let bridge = bridge_manifest("fixture.bridge", &vendor_runtime);
    let initial =
        ResolvedHarness::resolve([bridge.clone()], [], [], &Authority::default()).unwrap();
    let active_generation = initial.generation().clone();
    let mut kernel = Kernel::new(initial.kernel_config().clone());
    kernel.activate_resolved_harness(&initial).unwrap();
    let missing = runtime("missing.runtime");
    let invalid_guest = guest_manifest("fixture.guest", missing.clone(), "sha256:guest-v1");
    let mut reconciler = GraphReconciler::new(initial);

    let error = manage(
        &mut reconciler,
        &mut kernel,
        PluginManagementRequest::load(PluginLoadRequest {
            manifest: ready(invalid_guest),
            components: Vec::new(),
            entry_triggers: Vec::new(),
            expected_active_revision: None,
        }),
        &Authority::default(),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        PluginManagementError::RuntimeUnavailable { runtime: found, build: None }
            if found == missing
    ));
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
}

#[test]
fn runtime_provider_cycle_is_rejected_during_desired_set_reconcile() {
    let runtime_a = runtime("runtime.a");
    let runtime_b = runtime("runtime.b");
    let bridge_a = bridge_manifest("bridge.a", &runtime_a);
    let mut bridge_a = bridge_a;
    bridge_a.execution = PluginExecution::Runtime {
        runtime: runtime_b.clone(),
        artifact: artifact("sha256:a"),
    };
    let bridge_b = bridge_manifest("bridge.b", &runtime_b);
    let mut bridge_b = bridge_b;
    bridge_b.execution = PluginExecution::Runtime {
        runtime: runtime_a,
        artifact: artifact("sha256:b"),
    };
    let initial = ResolvedHarness::resolve([], [], [], &Authority::default()).unwrap();
    let active_generation = initial.generation().clone();
    let mut kernel = Kernel::new(initial.kernel_config().clone());
    kernel.activate_resolved_harness(&initial).unwrap();
    let mut reconciler = GraphReconciler::new(initial);

    let error = manage(
        &mut reconciler,
        &mut kernel,
        PluginManagementRequest::Reconcile(PluginSetRequest {
            plugins: vec![bridge_a, bridge_b],
            components: Vec::new(),
            entry_triggers: Vec::new(),
        }),
        &Authority::default(),
    )
    .unwrap_err();

    let PluginManagementError::Candidate { error, build: None } = error else {
        panic!("runtime cycle should fail candidate resolution");
    };
    assert!(matches!(
        *error,
        crate::ResolvedHarnessError::Kernel(KernelError::DependencyCycle(_))
    ));
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
}

#[test]
fn removing_a_runtime_provider_with_dependents_is_rejected() {
    let runtime = runtime("vendor.runtime");
    let bridge = bridge_manifest("fixture.bridge", &runtime);
    let guest = guest_manifest("fixture.guest", runtime.clone(), "sha256:guest-v1");
    let initial =
        ResolvedHarness::resolve([bridge.clone(), guest], [], [], &Authority::default()).unwrap();
    let active_generation = initial.generation().clone();
    let mut kernel = Kernel::new(initial.kernel_config().clone());
    kernel.activate_resolved_harness(&initial).unwrap();
    let mut reconciler = GraphReconciler::new(initial);

    let error = manage(
        &mut reconciler,
        &mut kernel,
        PluginManagementRequest::Unload(PluginUnloadRequest {
            plugin: bridge.id.clone(),
            expected_active_revision: None,
        }),
        &Authority::default(),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        PluginManagementError::RuntimeUnavailable { runtime: found, build: None }
            if found == runtime
    ));
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
}

#[test]
fn old_and_new_invocations_are_pinned_to_their_generations() {
    let service = service("fixture.echo@1");
    let first = embedded_manifest("fixture.guest", Some(service.clone()));
    let initial = ResolvedHarness::resolve([first.clone()], [], [], &Authority::default()).unwrap();
    let initial_generation = initial.generation().clone();
    let starts = Arc::new(AtomicUsize::new(0));
    let stops = Arc::new(AtomicUsize::new(0));
    let mut kernel = Kernel::new(initial.kernel_config().clone());
    kernel
        .register_embedded_factory(first.id.clone(), {
            let starts = Arc::clone(&starts);
            let stops = Arc::clone(&stops);
            move || {
                Box::new(TrackingInstance {
                    starts: Arc::clone(&starts),
                    stops: Arc::clone(&stops),
                    response: b"v1".to_vec(),
                })
            }
        })
        .unwrap();
    kernel.activate_resolved_harness(&initial).unwrap();
    kernel.activate_all().unwrap();

    assert_eq!(
        kernel
            .invoke(&service, b"before", &Authority::default(), None)
            .unwrap(),
        b"v1"
    );
    let mut second = first.clone();
    second.version = 2;
    kernel.preload_embedded_instance(
        first.id.clone(),
        Box::new(TrackingInstance {
            starts: Arc::clone(&starts),
            stops: Arc::clone(&stops),
            response: b"v2".to_vec(),
        }),
    );
    let mut reconciler = GraphReconciler::new(initial);
    let result = manage(
        &mut reconciler,
        &mut kernel,
        PluginManagementRequest::load(PluginLoadRequest {
            manifest: ready(second),
            components: Vec::new(),
            entry_triggers: Vec::new(),
            expected_active_revision: None,
        }),
        &Authority::default(),
    )
    .unwrap();

    assert_eq!(
        kernel
            .invoke(&service, b"after", &Authority::default(), None)
            .unwrap(),
        b"v2"
    );
    let provenance = kernel.service_invocation_provenance();
    assert_eq!(provenance.len(), 2);
    assert_eq!(
        provenance[0].graph_generation,
        Some(initial_generation.clone())
    );
    assert_eq!(
        provenance[1].graph_generation,
        Some(result.reconciliation.active_generation.clone())
    );
    assert_ne!(
        provenance[0].graph_generation,
        provenance[1].graph_generation
    );
    assert_eq!(starts.load(Ordering::Relaxed), 2);
    assert_eq!(stops.load(Ordering::Relaxed), 1);
}

#[test]
fn unload_of_an_unknown_plugin_is_rejected() {
    let runtime = runtime("vendor.runtime");
    let bridge = bridge_manifest("fixture.bridge", &runtime);
    let initial =
        ResolvedHarness::resolve([bridge.clone()], [], [], &Authority::default()).unwrap();
    let active_generation = initial.generation().clone();
    let mut kernel = Kernel::new(initial.kernel_config().clone());
    kernel.activate_resolved_harness(&initial).unwrap();
    let mut reconciler = GraphReconciler::new(initial);

    let error = manage(
        &mut reconciler,
        &mut kernel,
        PluginManagementRequest::Unload(PluginUnloadRequest {
            plugin: plugin("fixture.missing"),
            expected_active_revision: None,
        }),
        &Authority::default(),
    )
    .unwrap_err();

    assert_eq!(
        error,
        PluginManagementError::UnknownPlugin(plugin("fixture.missing"))
    );
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
}
