use crate::{
    ArtifactRevision, Authority, BuildArgument, BuildArtifactOutput, BuildEnvironment,
    BuildEnvironmentName, BuildExecutable, BuildSourceIdentity, BuildSourceRevision,
    BuildWorkingDirectory, GraphReconciler, Kernel, KernelError, PermissionId, PluginArtifact,
    PluginArtifactInput, PluginArtifactStore, PluginArtifactStoreError, PluginBuildEvidence,
    PluginBuildExecution, PluginBuildExecutor, PluginBuildFailure, PluginBuildOutput,
    PluginBuildPlan, PluginBuildSource, PluginBuildStep, PluginExecution, PluginHost, PluginId,
    PluginInstance, PluginLoadRequest, PluginManagementContext, PluginManagementError,
    PluginManagementPolicy, PluginManagementRequest, PluginManifest, PluginRuntimeAdapter,
    PluginRuntimeCandidate, PluginRuntimeId, ResolvedGeneration, ResolvedGenerationActivation,
    ServiceContribution, ServiceRole, plugin_runtime_adapter_service,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

fn capability(value: &str) -> PermissionId {
    PermissionId::parse(value).unwrap()
}

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn runtime() -> PluginRuntimeId {
    PluginRuntimeId::parse("fixture.runtime").unwrap()
}

fn plan(requested_authority: Authority) -> PluginBuildPlan {
    PluginBuildPlan::new(
        PluginBuildSource {
            identity: "git:fixture/plugin".parse::<BuildSourceIdentity>().unwrap(),
            revision: "commit:abc123".parse::<BuildSourceRevision>().unwrap(),
        },
        vec![PluginBuildStep {
            executable: "toolchain".parse::<BuildExecutable>().unwrap(),
            argv: ["--define=x; touch /tmp/nope", "$(not-a-shell)"]
                .into_iter()
                .map(|argument| argument.parse::<BuildArgument>().unwrap())
                .collect(),
            working_directory: "source/plugin".parse::<BuildWorkingDirectory>().unwrap(),
            environment: BuildEnvironment::new([(
                "BUILD_MODE".parse::<BuildEnvironmentName>().unwrap(),
                "release;literal".into(),
            )])
            .unwrap(),
        }],
        "dist/plugin.wasm".parse::<BuildArtifactOutput>().unwrap(),
        BTreeMap::new(),
        requested_authority,
    )
    .unwrap()
}

fn build_manifest(plan: PluginBuildPlan) -> PluginManifest<PluginArtifactInput> {
    PluginManifest {
        id: plugin("fixture.guest"),
        version: 1,
        execution: PluginExecution::Runtime {
            runtime: runtime(),
            artifact: PluginArtifactInput::Build(plan),
        },
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::new([capability("plugin.runtime")]),
    }
}

fn ready_manifest(content: &[u8]) -> PluginManifest<PluginArtifactInput> {
    PluginManifest {
        id: plugin("fixture.guest"),
        version: 1,
        execution: PluginExecution::Runtime {
            runtime: runtime(),
            artifact: PluginArtifactInput::Ready(PluginArtifact {
                locator: "dist/plugin.wasm".into(),
                revision: ArtifactRevision::from_content(content),
                configuration: BTreeMap::new(),
            }),
        },
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::new([capability("plugin.runtime")]),
    }
}

fn bridge_manifest() -> PluginManifest {
    PluginManifest {
        id: plugin("fixture.bridge"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            service: plugin_runtime_adapter_service(&runtime()),
            role: ServiceRole::Terminal,
            priority: 0,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::new([capability("runtime.provider")]),
    }
}

fn active_fixture(plugins: impl IntoIterator<Item = PluginManifest>) -> (GraphReconciler, Kernel) {
    // Plugin-management tests resolve candidates with this capability. The
    // long-lived kernel must start with the same ceiling; reconciliation may
    // attenuate it, but must never widen it.
    let authority_ceiling = Authority::new([capability("plugin.runtime")]);
    let active = ResolvedGeneration::resolve(plugins, [], [], &authority_ceiling).unwrap();
    let mut kernel = Kernel::new(active.kernel_config().clone());
    kernel.activate_resolved_generation(&active).unwrap();
    (GraphReconciler::new(active), kernel)
}

#[derive(Clone)]
enum ExecutorOutcome {
    Output(Vec<u8>),
    Missing,
    Failure,
}

struct RecordingExecutor {
    events: Arc<Mutex<Vec<&'static str>>>,
    outcome: ExecutorOutcome,
    effective_authority: Arc<Mutex<Option<Authority>>>,
}

impl PluginBuildExecutor for RecordingExecutor {
    fn execute(
        &mut self,
        plan: &PluginBuildPlan,
        effective_authority: &Authority,
    ) -> Result<PluginBuildExecution, PluginBuildFailure> {
        self.events.lock().unwrap().push("build");
        *self.effective_authority.lock().unwrap() = Some(effective_authority.clone());
        assert_eq!(plan.steps()[0].executable.as_ref(), "toolchain");
        assert_eq!(
            plan.steps()[0].argv[0].as_ref(),
            "--define=x; touch /tmp/nope"
        );
        assert_eq!(plan.steps()[0].working_directory.as_ref(), "source/plugin");
        assert_eq!(
            plan.steps()[0]
                .environment
                .iter()
                .next()
                .map(|(name, value)| (name.as_ref(), value)),
            Some(("BUILD_MODE", "release;literal"))
        );
        let evidence = PluginBuildEvidence::bounded(
            vec!["isolated-stage:fixture".into()],
            vec!["compiler diagnostic".into()],
        );
        match &self.outcome {
            ExecutorOutcome::Output(content) => Ok(PluginBuildExecution {
                output: Some(PluginBuildOutput::new(
                    plan.artifact_output().as_ref().into(),
                    content.clone(),
                )),
                evidence,
            }),
            ExecutorOutcome::Missing => Ok(PluginBuildExecution {
                output: None,
                evidence,
            }),
            ExecutorOutcome::Failure => Err(PluginBuildFailure {
                message: "compiler failed".into(),
                evidence,
            }),
        }
    }
}

struct RecordingStore {
    events: Arc<Mutex<Vec<&'static str>>>,
    preflight_error: bool,
}

impl PluginArtifactStore for RecordingStore {
    fn preflight(&mut self) -> Result<(), PluginArtifactStoreError> {
        self.events.lock().unwrap().push("cas_preflight");
        if self.preflight_error {
            return Err(PluginArtifactStoreError {
                message: "CAS unavailable".into(),
            });
        }
        Ok(())
    }

    fn verify_ready(&mut self, _artifact: &PluginArtifact) -> Result<(), PluginArtifactStoreError> {
        self.events.lock().unwrap().push("cas_verify");
        Ok(())
    }

    fn store_built(
        &mut self,
        artifact: &PluginArtifact,
        content: &[u8],
    ) -> Result<String, PluginArtifactStoreError> {
        self.events.lock().unwrap().push("cas_store");
        assert_eq!(artifact.revision, ArtifactRevision::from_content(content));
        Ok(artifact.locator.clone())
    }
}

struct RelocatingStore;

impl PluginArtifactStore for RelocatingStore {
    fn preflight(&mut self) -> Result<(), PluginArtifactStoreError> {
        Ok(())
    }

    fn verify_ready(&mut self, _artifact: &PluginArtifact) -> Result<(), PluginArtifactStoreError> {
        Ok(())
    }

    fn store_built(
        &mut self,
        artifact: &PluginArtifact,
        content: &[u8],
    ) -> Result<String, PluginArtifactStoreError> {
        assert_eq!(artifact.revision, ArtifactRevision::from_content(content));
        Ok(format!("cas://{}", artifact.revision.as_ref()))
    }
}

fn manage(
    reconciler: &mut GraphReconciler,
    kernel: &mut Kernel,
    manifest: PluginManifest<PluginArtifactInput>,
    caller: &Authority,
    policy: &PluginManagementPolicy,
    store: &mut RecordingStore,
    executor: &mut RecordingExecutor,
) -> Result<crate::PluginManagementResult, PluginManagementError> {
    reconciler.manage(
        kernel,
        PluginManagementRequest::load(PluginLoadRequest {
            manifest,
            components: Vec::new(),
            entry_triggers: Vec::new(),
            process_arguments: Vec::new(),
            expected_active_revision: None,
        }),
        &Authority::new([capability("plugin.runtime")]),
        &mut PluginManagementContext {
            caller_authority: caller,
            policy,
            artifact_store: store,
            build_executor: executor,
        },
    )
}

fn policy(build_authority: Authority) -> PluginManagementPolicy {
    PluginManagementPolicy::new(Authority::default(), build_authority)
}

#[test]
fn built_artifact_uses_the_store_canonical_locator_before_candidate_resolution() {
    let caller = Authority::new([capability("plugin.runtime")]);
    let policy = policy(Authority::default());
    let (reconciler, _kernel) = active_fixture([]);
    let mut store = RelocatingStore;
    let events = Arc::new(Mutex::new(Vec::new()));
    let content = b"relocated immutable wasm".to_vec();
    let mut executor = RecordingExecutor {
        events,
        outcome: ExecutorOutcome::Output(content.clone()),
        effective_authority: Arc::new(Mutex::new(None)),
    };

    let error = reconciler
        .prepare_management(
            PluginManagementRequest::load(PluginLoadRequest {
                manifest: build_manifest(plan(Authority::default())),
                components: Vec::new(),
                entry_triggers: Vec::new(),
                process_arguments: Vec::new(),
                expected_active_revision: None,
            }),
            &caller,
            &mut PluginManagementContext {
                caller_authority: &caller,
                policy: &policy,
                artifact_store: &mut store,
                build_executor: &mut executor,
            },
        )
        .unwrap_err();

    let PluginManagementError::RuntimeUnavailable {
        build: Some(report),
        ..
    } = error
    else {
        panic!("unknown runtime should fail after storing the built artifact");
    };
    assert_eq!(
        report.artifact.locator,
        format!(
            "cas://{}",
            ArtifactRevision::from_content(&content).as_ref()
        )
    );
    assert_eq!(
        report.artifact.revision,
        ArtifactRevision::from_content(&content)
    );
}

#[test]
fn structured_build_is_authority_bounded_and_runtime_unavailable_preserves_evidence() {
    let shared = capability("build.shared");
    let caller = Authority::new([
        shared.clone(),
        capability("caller.only"),
        capability("plugin.runtime"),
    ]);
    let policy = policy(Authority::new([shared.clone(), capability("policy.only")]));
    let requested = Authority::new([shared.clone(), capability("request.only")]);
    let (mut reconciler, mut kernel) = active_fixture([]);
    let active_generation = reconciler.active().generation().clone();
    let events = Arc::new(Mutex::new(Vec::new()));
    let effective_authority = Arc::new(Mutex::new(None));
    let content = b"deterministic wasm".to_vec();
    let mut executor = RecordingExecutor {
        events: Arc::clone(&events),
        outcome: ExecutorOutcome::Output(content.clone()),
        effective_authority: Arc::clone(&effective_authority),
    };
    let mut store = RecordingStore {
        events: Arc::clone(&events),
        preflight_error: false,
    };

    let error = manage(
        &mut reconciler,
        &mut kernel,
        build_manifest(plan(requested)),
        &caller,
        &policy,
        &mut store,
        &mut executor,
    )
    .unwrap_err();

    let PluginManagementError::RuntimeUnavailable {
        runtime: unavailable,
        build: Some(report),
    } = error
    else {
        panic!("successful build should fail directly at runtime resolution");
    };
    assert_eq!(unavailable, runtime());
    assert_eq!(
        report.artifact.revision,
        ArtifactRevision::from_content(&content)
    );
    assert_eq!(report.evidence.diagnostics(), &["compiler diagnostic"]);
    assert_eq!(
        *effective_authority.lock().unwrap(),
        Some(Authority::new([shared]))
    );
    assert!(
        !report
            .effective_authority
            .permits(&capability("plugin.runtime"))
    );
    assert_eq!(
        *events.lock().unwrap(),
        ["cas_preflight", "build", "cas_store"]
    );
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
    assert_eq!(reconciler.active().generation(), &active_generation);
}

#[test]
fn missing_output_and_failed_build_leave_the_active_graph_unchanged() {
    for outcome in [ExecutorOutcome::Missing, ExecutorOutcome::Failure] {
        let (mut reconciler, mut kernel) = active_fixture([]);
        let active_generation = reconciler.active().generation().clone();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut executor = RecordingExecutor {
            events: Arc::clone(&events),
            outcome,
            effective_authority: Arc::new(Mutex::new(None)),
        };
        let mut store = RecordingStore {
            events,
            preflight_error: false,
        };
        let error = manage(
            &mut reconciler,
            &mut kernel,
            build_manifest(plan(Authority::default())),
            &Authority::default(),
            &policy(Authority::default()),
            &mut store,
            &mut executor,
        )
        .unwrap_err();

        match error {
            PluginManagementError::MissingBuildOutput { evidence } => {
                assert_eq!(evidence.provenance(), &["isolated-stage:fixture"]);
            }
            PluginManagementError::Build(failure) => {
                assert_eq!(failure.evidence.diagnostics(), &["compiler diagnostic"]);
            }
            error => panic!("unexpected build failure: {error:?}"),
        }
        assert_eq!(kernel.graph_generation(), Some(&active_generation));
        assert_eq!(reconciler.active().generation(), &active_generation);
    }
}

struct RejectingBridge;

impl PluginRuntimeAdapter for RejectingBridge {
    fn prepare(
        &mut self,
        _candidate: PluginRuntimeCandidate<'_>,
    ) -> Result<Box<dyn PluginInstance>, String> {
        Err("runtime rejected artifact".into())
    }
}

impl PluginInstance for RejectingBridge {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn plugin_runtime_adapter(&mut self) -> Option<&mut dyn PluginRuntimeAdapter> {
        Some(self)
    }
}

#[test]
fn runtime_rejection_after_build_preserves_build_report_and_rolls_back() {
    let bridge = bridge_manifest();
    let (mut reconciler, mut kernel) = active_fixture([bridge.clone()]);
    kernel
        .register_embedded_factory(bridge.id, || Box::new(RejectingBridge))
        .unwrap();
    kernel.activate_all().unwrap();
    let active_generation = reconciler.active().generation().clone();
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut executor = RecordingExecutor {
        events: Arc::clone(&events),
        outcome: ExecutorOutcome::Output(b"rejected wasm".to_vec()),
        effective_authority: Arc::new(Mutex::new(None)),
    };
    let mut store = RecordingStore {
        events,
        preflight_error: false,
    };

    let error = manage(
        &mut reconciler,
        &mut kernel,
        build_manifest(plan(Authority::default())),
        &Authority::default(),
        &policy(Authority::default()),
        &mut store,
        &mut executor,
    )
    .unwrap_err();

    let PluginManagementError::Reconciliation {
        error,
        build: Some(report),
    } = error
    else {
        panic!("runtime rejection should retain the completed build report");
    };
    assert!(matches!(
        *error,
        crate::LiveReconciliationError::Runtime(KernelError::PluginRuntimePreparation { .. })
    ));
    assert_eq!(report.evidence.provenance(), ["isolated-stage:fixture"]);
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
    assert_eq!(reconciler.active().generation(), &active_generation);
}

#[test]
fn ready_and_build_inputs_share_the_concrete_downstream_generation() {
    let content = b"same immutable artifact".to_vec();
    let bridge = bridge_manifest();
    let (mut built_reconciler, mut built_kernel) = active_fixture([bridge.clone()]);
    let (mut ready_reconciler, mut ready_kernel) = active_fixture([bridge]);
    let build_events = Arc::new(Mutex::new(Vec::new()));
    let mut build_executor = RecordingExecutor {
        events: Arc::clone(&build_events),
        outcome: ExecutorOutcome::Output(content.clone()),
        effective_authority: Arc::new(Mutex::new(None)),
    };
    let mut build_store = RecordingStore {
        events: build_events,
        preflight_error: false,
    };
    let ready_events = Arc::new(Mutex::new(Vec::new()));
    let mut unused_executor = RecordingExecutor {
        events: Arc::clone(&ready_events),
        outcome: ExecutorOutcome::Failure,
        effective_authority: Arc::new(Mutex::new(None)),
    };
    let mut ready_store = RecordingStore {
        events: Arc::clone(&ready_events),
        preflight_error: false,
    };
    let policy = policy(Authority::default());

    let built = manage(
        &mut built_reconciler,
        &mut built_kernel,
        build_manifest(plan(Authority::default())),
        &Authority::default(),
        &policy,
        &mut build_store,
        &mut build_executor,
    )
    .unwrap();
    let ready = manage(
        &mut ready_reconciler,
        &mut ready_kernel,
        ready_manifest(&content),
        &Authority::default(),
        &policy,
        &mut ready_store,
        &mut unused_executor,
    )
    .unwrap();

    assert_eq!(
        built_reconciler.active().plugins(),
        ready_reconciler.active().plugins()
    );
    assert_eq!(
        built.reconciliation.active_generation,
        ready.reconciliation.active_generation
    );
    assert_eq!(
        built.build.unwrap().artifact.revision,
        ArtifactRevision::from_content(&content)
    );
    assert_eq!(
        ArtifactRevision::from_content(&content),
        ArtifactRevision::from_content(&content)
    );
    assert_ne!(
        ArtifactRevision::from_content(&content),
        ArtifactRevision::from_content(b"different artifact")
    );
    assert_eq!(
        *ready_events.lock().unwrap(),
        ["cas_preflight", "cas_verify"]
    );
}

#[test]
fn cas_failure_prevents_build_execution() {
    let (mut reconciler, mut kernel) = active_fixture([]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut executor = RecordingExecutor {
        events: Arc::clone(&events),
        outcome: ExecutorOutcome::Output(Vec::new()),
        effective_authority: Arc::new(Mutex::new(None)),
    };
    let mut store = RecordingStore {
        events: Arc::clone(&events),
        preflight_error: true,
    };

    let error = manage(
        &mut reconciler,
        &mut kernel,
        build_manifest(plan(Authority::default())),
        &Authority::default(),
        &policy(Authority::default()),
        &mut store,
        &mut executor,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        PluginManagementError::InvalidArtifact { build: None, .. }
    ));
    assert_eq!(*events.lock().unwrap(), ["cas_preflight"]);
}

#[test]
fn stale_expected_revision_prevents_build_execution() {
    let content = b"active artifact";
    let concrete = ready_manifest(content).map_artifact(|input| match input {
        PluginArtifactInput::Ready(artifact) => artifact,
        PluginArtifactInput::Build(_) => unreachable!(),
    });
    let (mut reconciler, mut kernel) = active_fixture([bridge_manifest(), concrete]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut executor = RecordingExecutor {
        events: Arc::clone(&events),
        outcome: ExecutorOutcome::Output(Vec::new()),
        effective_authority: Arc::new(Mutex::new(None)),
    };
    let mut store = RecordingStore {
        events: Arc::clone(&events),
        preflight_error: false,
    };
    let policy = policy(Authority::default());
    let caller = Authority::default();
    let error = reconciler
        .manage(
            &mut kernel,
            PluginManagementRequest::load(PluginLoadRequest {
                manifest: build_manifest(plan(Authority::default())),
                components: Vec::new(),
                entry_triggers: Vec::new(),
                process_arguments: Vec::new(),
                expected_active_revision: Some(ArtifactRevision::from_content(b"stale")),
            }),
            &Authority::new([capability("plugin.runtime")]),
            &mut PluginManagementContext {
                caller_authority: &caller,
                policy: &policy,
                artifact_store: &mut store,
                build_executor: &mut executor,
            },
        )
        .unwrap_err();

    assert!(matches!(
        error,
        PluginManagementError::StaleExpectedRevision { .. }
    ));
    assert_eq!(*events.lock().unwrap(), ["cas_preflight"]);
}

#[test]
fn authorization_denial_precedes_cas_and_build() {
    let (mut reconciler, mut kernel) = active_fixture([]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut executor = RecordingExecutor {
        events: Arc::clone(&events),
        outcome: ExecutorOutcome::Output(Vec::new()),
        effective_authority: Arc::new(Mutex::new(None)),
    };
    let mut store = RecordingStore {
        events: Arc::clone(&events),
        preflight_error: false,
    };
    let required = Authority::new([capability("plugins.manage")]);
    let policy = PluginManagementPolicy::new(required.clone(), Authority::default());
    let caller = Authority::default();

    let error = reconciler
        .manage(
            &mut kernel,
            PluginManagementRequest::load(PluginLoadRequest {
                manifest: build_manifest(plan(Authority::default())),
                components: Vec::new(),
                entry_triggers: Vec::new(),
                process_arguments: Vec::new(),
                expected_active_revision: None,
            }),
            &Authority::new([capability("plugin.runtime")]),
            &mut PluginManagementContext {
                caller_authority: &caller,
                policy: &policy,
                artifact_store: &mut store,
                build_executor: &mut executor,
            },
        )
        .unwrap_err();

    assert_eq!(error, PluginManagementError::Authorization { required });
    assert!(events.lock().unwrap().is_empty());
}
