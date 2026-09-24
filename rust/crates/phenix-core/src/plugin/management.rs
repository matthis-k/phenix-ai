use crate::{
    ArtifactRevision, Authority, ComponentManifest, GraphReconciler, Kernel, KernelError,
    LiveReconciliationError, PluginArtifact, PluginArtifactInput, PluginArtifactStore,
    PluginBuildEvidence, PluginBuildExecutor, PluginBuildFailure, PluginBuildPlan, PluginExecution,
    PluginId, PluginManifest, ReconciliationResult, ResolvedHarness, ResolvedHarnessError,
    RuntimeId,
};
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "operation", content = "request", rename_all = "snake_case")]
pub enum PluginManagementRequest {
    Load(Box<PluginLoadRequest>),
    Unload(PluginUnloadRequest),
    Reconcile(PluginSetRequest),
}

impl PluginManagementRequest {
    #[must_use]
    pub fn load(request: PluginLoadRequest) -> Self {
        Self::Load(Box::new(request))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PluginLoadRequest {
    pub manifest: PluginManifest<PluginArtifactInput>,
    pub components: Vec<ComponentManifest>,
    pub expected_active_revision: Option<ArtifactRevision>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PluginUnloadRequest {
    pub plugin: PluginId,
    pub expected_active_revision: Option<ArtifactRevision>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PluginSetRequest {
    pub plugins: Vec<PluginManifest>,
    pub components: Vec<ComponentManifest>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginManagementPolicy {
    required_authority: Authority,
    build_authority: Authority,
}

impl PluginManagementPolicy {
    #[must_use]
    pub fn new(required_authority: Authority, build_authority: Authority) -> Self {
        Self {
            required_authority,
            build_authority,
        }
    }

    pub fn required_authority(&self) -> &Authority {
        &self.required_authority
    }

    pub fn build_authority(&self) -> &Authority {
        &self.build_authority
    }
}

/// Trusted request context supplied by the host, never by serialized plugin
/// management input.
pub struct PluginManagementContext<'a> {
    pub caller_authority: &'a Authority,
    pub policy: &'a PluginManagementPolicy,
    pub artifact_store: &'a mut dyn PluginArtifactStore,
    pub build_executor: &'a mut dyn PluginBuildExecutor,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PluginBuildReport {
    pub artifact: PluginArtifact,
    pub source: crate::PluginBuildSource,
    pub effective_authority: Authority,
    pub evidence: PluginBuildEvidence,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PluginManagementResult {
    pub reconciliation: ReconciliationResult,
    pub build: Option<PluginBuildReport>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PluginManagementError {
    Authorization {
        required: Authority,
    },
    UnknownPlugin(PluginId),
    StaleExpectedRevision {
        plugin: PluginId,
        expected: ArtifactRevision,
        active: Option<ArtifactRevision>,
    },
    Build(PluginBuildFailure),
    MissingBuildOutput {
        evidence: PluginBuildEvidence,
    },
    InvalidArtifact {
        message: String,
        build: Option<Box<PluginBuildReport>>,
    },
    RuntimeUnavailable {
        runtime: RuntimeId,
        build: Option<Box<PluginBuildReport>>,
    },
    Candidate {
        error: Box<ResolvedHarnessError>,
        build: Option<Box<PluginBuildReport>>,
    },
    Reconciliation {
        error: Box<LiveReconciliationError>,
        build: Option<Box<PluginBuildReport>>,
    },
}

impl Display for PluginManagementError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authorization { .. } => f.write_str("plugin management authorization denied"),
            Self::UnknownPlugin(plugin) => write!(f, "unknown plugin: {plugin}"),
            Self::StaleExpectedRevision {
                plugin,
                expected,
                active,
            } => {
                let active = active
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<none>".into());
                write!(
                    f,
                    "plugin {plugin} active revision {active} does not match expected {expected}"
                )
            }
            Self::Build(error) => write!(f, "plugin build failed: {error}"),
            Self::MissingBuildOutput { .. } => {
                f.write_str("plugin build did not produce its declared output")
            }
            Self::InvalidArtifact { message, .. } => {
                write!(f, "plugin artifact is invalid: {message}")
            }
            Self::RuntimeUnavailable { runtime, .. } => {
                write!(f, "plugin runtime is unavailable: {runtime}")
            }
            Self::Candidate { error, .. } => {
                write!(f, "plugin management candidate resolution failed: {error}")
            }
            Self::Reconciliation { error, .. } => {
                write!(f, "plugin management reconciliation failed: {error:?}")
            }
        }
    }
}

impl Error for PluginManagementError {}

impl GraphReconciler {
    /// Apply one kernel-owned desired-state plugin management request.
    ///
    /// This is the sole load, build, replace, unload, and reconcile lifecycle.
    /// Build inputs become concrete artifacts before candidate/runtime
    /// resolution and then share the ready-artifact activation path.
    pub fn manage(
        &mut self,
        kernel: &mut Kernel,
        request: PluginManagementRequest,
        authority_ceiling: &Authority,
        context: &mut PluginManagementContext<'_>,
    ) -> Result<PluginManagementResult, PluginManagementError> {
        self.preflight_live_reconciliation(kernel)
            .map_err(|error| PluginManagementError::Reconciliation {
                error: Box::new(error),
                build: None,
            })?;
        if !context
            .caller_authority
            .permits_all(context.policy.required_authority())
        {
            return Err(PluginManagementError::Authorization {
                required: context.policy.required_authority().clone(),
            });
        }
        context.artifact_store.preflight().map_err(|error| {
            PluginManagementError::InvalidArtifact {
                message: error.message,
                build: None,
            }
        })?;

        let (plugins, components, build) = match request {
            PluginManagementRequest::Load(request) => {
                let request = *request;
                check_load_expected_revision(self.active(), &request)?;
                let (manifest, build) = materialize_manifest(request.manifest, context)?;
                let (plugins, components) = apply_load(
                    self.active(),
                    ConcretePluginLoadRequest {
                        manifest,
                        components: request.components,
                    },
                );
                (plugins, components, build)
            }
            PluginManagementRequest::Unload(request) => {
                let (plugins, components) = apply_unload(self.active(), request)?;
                (plugins, components, None)
            }
            PluginManagementRequest::Reconcile(request) => {
                let (plugins, components) = apply_reconcile(request);
                (plugins, components, None)
            }
        };
        let candidate = self
            .active()
            .with_plugin_set(plugins, components, authority_ceiling)
            .map_err(|error| map_candidate_error(error, build.clone()))?;
        let reconciliation = self
            .activate_candidate_on_kernel(kernel, candidate)
            .map_err(|error| PluginManagementError::Reconciliation {
                error: Box::new(error),
                build: build.clone().map(Box::new),
            })?;
        Ok(PluginManagementResult {
            reconciliation,
            build,
        })
    }
}

fn materialize_manifest(
    manifest: PluginManifest<PluginArtifactInput>,
    context: &mut PluginManagementContext<'_>,
) -> Result<(PluginManifest, Option<PluginBuildReport>), PluginManagementError> {
    let (execution, build) = match manifest.execution {
        PluginExecution::Embedded => (PluginExecution::Embedded, None),
        PluginExecution::ResourceOnly => (PluginExecution::ResourceOnly, None),
        PluginExecution::Runtime { runtime, artifact } => {
            let (artifact, build) = materialize_artifact(artifact, context)?;
            (PluginExecution::Runtime { runtime, artifact }, build)
        }
    };
    Ok((
        PluginManifest {
            id: manifest.id,
            version: manifest.version,
            execution,
            dependencies: manifest.dependencies,
            services: manifest.services,
            resource_namespaces: manifest.resource_namespaces,
            maximum_authority: manifest.maximum_authority,
        },
        build,
    ))
}

fn materialize_artifact(
    input: PluginArtifactInput,
    context: &mut PluginManagementContext<'_>,
) -> Result<(PluginArtifact, Option<PluginBuildReport>), PluginManagementError> {
    match input {
        PluginArtifactInput::Ready(artifact) => {
            context
                .artifact_store
                .verify_ready(&artifact)
                .map_err(|error| PluginManagementError::InvalidArtifact {
                    message: error.message,
                    build: None,
                })?;
            Ok((artifact, None))
        }
        PluginArtifactInput::Build(plan) => build_artifact(plan, context),
    }
}

fn build_artifact(
    plan: PluginBuildPlan,
    context: &mut PluginManagementContext<'_>,
) -> Result<(PluginArtifact, Option<PluginBuildReport>), PluginManagementError> {
    let effective_authority = context
        .policy
        .build_authority()
        .attenuate(context.caller_authority)
        .attenuate(plan.requested_authority());
    let execution = context
        .build_executor
        .execute(&plan, &effective_authority)
        .map_err(PluginManagementError::Build)?;
    let output = execution
        .output
        .ok_or_else(|| PluginManagementError::MissingBuildOutput {
            evidence: execution.evidence.clone(),
        })?;
    if output.locator() != plan.artifact_output().as_ref() {
        return Err(PluginManagementError::MissingBuildOutput {
            evidence: execution.evidence,
        });
    }
    let artifact = PluginArtifact {
        locator: output.locator().to_owned(),
        revision: ArtifactRevision::from_content(output.content()),
        configuration: plan.configuration().clone(),
    };
    let report = PluginBuildReport {
        artifact: artifact.clone(),
        source: plan.source().clone(),
        effective_authority,
        evidence: execution.evidence,
    };
    context
        .artifact_store
        .store_built(&artifact, output.content())
        .map_err(|error| PluginManagementError::InvalidArtifact {
            message: error.message,
            build: Some(Box::new(report.clone())),
        })?;
    Ok((artifact, Some(report)))
}

fn map_candidate_error(
    error: ResolvedHarnessError,
    build: Option<PluginBuildReport>,
) -> PluginManagementError {
    match error {
        ResolvedHarnessError::Kernel(KernelError::RuntimeProviderUnavailable(runtime)) => {
            PluginManagementError::RuntimeUnavailable {
                runtime,
                build: build.map(Box::new),
            }
        }
        error => PluginManagementError::Candidate {
            error: Box::new(error),
            build: build.map(Box::new),
        },
    }
}

fn apply_load(
    active: &ResolvedHarness,
    request: ConcretePluginLoadRequest,
) -> (Vec<PluginManifest>, Vec<ComponentManifest>) {
    let plugin = request.manifest.id.clone();
    let mut plugins: Vec<_> = active.plugins().to_vec();
    match plugins.iter_mut().find(|manifest| manifest.id == plugin) {
        Some(slot) => *slot = request.manifest,
        None => plugins.push(request.manifest),
    }
    let mut components: Vec<_> = active
        .components()
        .iter()
        .filter(|component| component.owner != plugin)
        .cloned()
        .collect();
    components.extend(request.components);
    (plugins, components)
}

#[derive(Clone, Debug, PartialEq)]
struct ConcretePluginLoadRequest {
    manifest: PluginManifest,
    components: Vec<ComponentManifest>,
}

fn check_load_expected_revision(
    active: &ResolvedHarness,
    request: &PluginLoadRequest,
) -> Result<(), PluginManagementError> {
    let Some(expected) = &request.expected_active_revision else {
        return Ok(());
    };
    let plugin = &request.manifest.id;
    let existing = active
        .plugins()
        .iter()
        .find(|manifest| manifest.id == *plugin);
    check_expected_revision(plugin, existing, expected)
}

fn apply_unload(
    active: &ResolvedHarness,
    request: PluginUnloadRequest,
) -> Result<(Vec<PluginManifest>, Vec<ComponentManifest>), PluginManagementError> {
    let existing = active
        .plugins()
        .iter()
        .find(|manifest| manifest.id == request.plugin)
        .ok_or_else(|| PluginManagementError::UnknownPlugin(request.plugin.clone()))?;
    if let Some(expected) = &request.expected_active_revision {
        check_expected_revision(&request.plugin, Some(existing), expected)?;
    }
    let plugins = active
        .plugins()
        .iter()
        .filter(|manifest| manifest.id != request.plugin)
        .cloned()
        .collect();
    let components = active
        .components()
        .iter()
        .filter(|component| component.owner != request.plugin)
        .cloned()
        .collect();
    Ok((plugins, components))
}

fn apply_reconcile(request: PluginSetRequest) -> (Vec<PluginManifest>, Vec<ComponentManifest>) {
    (request.plugins, request.components)
}

fn check_expected_revision(
    plugin: &PluginId,
    active: Option<&PluginManifest>,
    expected: &ArtifactRevision,
) -> Result<(), PluginManagementError> {
    let revision = active.and_then(|manifest| match &manifest.execution {
        PluginExecution::Runtime { artifact, .. } => Some(artifact.revision.clone()),
        PluginExecution::Embedded | PluginExecution::ResourceOnly => None,
    });
    if revision.as_ref() == Some(expected) {
        return Ok(());
    }
    Err(PluginManagementError::StaleExpectedRevision {
        plugin: plugin.clone(),
        expected: expected.clone(),
        active: revision,
    })
}
