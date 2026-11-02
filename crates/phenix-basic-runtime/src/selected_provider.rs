//! Selected-provider acquisition and exact-token revocation, independent of ingress.
use super::*;
use crate::provider_management::ProviderManagementGroup;
use phenix_provider_contracts::{
    AuthenticationKind, CredentialIdentity, CredentialSecret, HttpModelInstall, HttpModelRelease,
    ModelSelection, ProviderError,
};
use zeroize::Zeroize;

#[derive(Debug)]
pub(crate) enum DomainError {
    Provider(ProviderError),
    Kernel(phenix_contract::Failure),
    Json(serde_json::Error),
    Invalid(String),
    Storage(String),
    Basic(BasicError),
    Admission(crate::session_lifecycle::AdmissionError),
}

impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provider(problem) => write!(f, "provider rejection: {:?}", problem.code),
            // Native resolver failures may contain credential bytes in their
            // prose. Keep the typed failure internally, never export its message.
            Self::Kernel(failure) => {
                write!(f, "selected provider kernel failure: {:?}", failure.code)
            }
            Self::Json(error) => write!(f, "invalid selection encoding: {error}"),
            Self::Basic(problem) => write!(
                f,
                "basic rejection: {:?}: {}",
                problem.code, problem.message
            ),
            Self::Invalid(message) | Self::Storage(message) => f.write_str(message),
            Self::Admission(error) => write!(f, "submission admission failed: {error:?}"),
        }
    }
}
impl From<DomainError> for String {
    fn from(error: DomainError) -> Self {
        error.to_string()
    }
}
impl From<String> for DomainError {
    fn from(message: String) -> Self {
        Self::Storage(message)
    }
}
impl From<&str> for DomainError {
    fn from(message: &str) -> Self {
        Self::Storage(message.into())
    }
}
pub(crate) fn invalid(message: &str) -> DomainError {
    DomainError::Invalid(message.into())
}
pub(crate) fn internal(failure: phenix_contract::Failure) -> DomainError {
    DomainError::Kernel(failure)
}
pub(crate) fn provider_rejection(problem: ProviderError) -> DomainError {
    DomainError::Provider(problem)
}

pub(crate) struct ModelCredentialResolver {
    pub(crate) kernel: Kernel,
    selected: Selection,
    pub(crate) identity: CredentialIdentity,
}

pub(crate) fn validate_model_resolver_spec(
    spec: &Composition,
) -> Result<CredentialIdentity, DomainError> {
    const CONTRACT: &str = "phenix.provider.credentials@1";
    const RESOLVE: &str = "phenix.provider.credentials@1/resolve";
    const ROTATE: &str = "phenix.provider.credentials@1/compare_exchange";
    let resource = spec
        .resource
        .as_deref()
        .ok_or("model resolver requires an explicit credential resource")?;
    let identity = CredentialIdentity::from_resource(resource).map_err(provider_rejection)?;
    let allowed_resource = std::collections::BTreeSet::from([resource.to_string()]);
    let rotate = spec.slots.len() == 1
        && spec.grants.iter().any(|grant| {
            grant.caller == spec.entry.provider
                && grant.interface == CONTRACT
                && grant.operation == "compare_exchange"
                && grant.resources == allowed_resource
        });
    let expected_authority = if rotate {
        std::collections::BTreeSet::from([RESOLVE.into(), ROTATE.into()])
    } else {
        std::collections::BTreeSet::from([RESOLVE.into()])
    };
    if spec.entry.interface != CONTRACT
        || spec.entry.operation != "resolve"
        || spec.authority.operations != expected_authority
        || spec.authority.resources != allowed_resource
        || !(spec.grants.len() == 1 && spec.slots.is_empty()
            || spec.grants.len() == 2 && spec.slots.len() == 1 && !rotate
            || spec.grants.len() == 3 && spec.slots.len() == 1 && rotate)
        || !spec.excluded.is_empty()
        || !spec.substitutions.is_empty()
    {
        return Err(
            "model resolver requires one selected root with exact private credential grants".into(),
        );
    }
    let grant = spec
        .grants
        .iter()
        .find(|grant| grant.caller == "root")
        .ok_or("missing selected resolver grant")?;
    if grant.interface != CONTRACT
        || grant.operation != "resolve"
        || grant.resources != allowed_resource
    {
        return Err("model resolver grant must match selected credential resource".into());
    }
    if let Some(binding) = spec.slots.first() {
        if binding.caller != spec.entry.provider
            || binding.provider == spec.entry.provider
            || binding.slot != "Credentials"
            || binding.export != "Credentials"
            || binding.index != 0
            || !spec.grants.iter().any(|g| {
                g.caller == binding.caller
                    && g.interface == CONTRACT
                    && g.operation == "resolve"
                    && g.resources == allowed_resource
            })
        {
            return Err("invalid nested model credential resolver grant".into());
        }
        if rotate
            && !spec.grants.iter().any(|g| {
                g.caller == binding.caller
                    && g.interface == CONTRACT
                    && g.operation == "compare_exchange"
                    && g.resources == allowed_resource
            })
        {
            return Err("invalid nested model credential rotation grant".into());
        }
    }
    Ok(identity)
}

pub(crate) fn prepare_model_resolver(
    spec: &Composition,
) -> Result<ModelCredentialResolver, DomainError> {
    let identity = validate_model_resolver_spec(spec)?;
    let kernel = Prepared::prepare(spec.clone())
        .map_err(internal)?
        .load()
        .map_err(internal)?;
    Ok(ModelCredentialResolver {
        kernel,
        selected: spec.entry.clone(),
        identity,
    })
}

impl ModelCredentialResolver {
    fn resolve_for(&self, identity: &CredentialIdentity) -> Result<CredentialSecret, DomainError> {
        if *identity != self.identity {
            return Err(invalid(
                "model credential identity is not launcher-authorized",
            ));
        }
        let root = self
            .kernel
            .admit_root(self.selected.clone())
            .map_err(internal)?;
        match root.invoke(&identity.to_value()).map_err(internal)? {
            Outcome::Success(value) => CredentialSecret::from_value(value).map_err(internal),
            Outcome::Domain(value) => Err(provider_rejection(
                ProviderError::from_value(value).map_err(internal)?,
            )),
        }
    }
}

pub(crate) fn installed_model_provider(composition: &Composition) -> Option<&str> {
    let mut bindings = composition.slots.iter().filter(|binding| {
        binding.caller == "phenix.basic.agent" && binding.slot == "Model" && binding.index == 0
    });
    let selected = bindings.next()?;
    if bindings.next().is_some() {
        return None;
    }
    Some(selected.provider.as_str())
}

pub(crate) fn has_model_selection_storage(spec: &Composition) -> bool {
    spec.grants.iter().any(|grant| {
        grant.caller == "root"
            && grant.interface == "phenix.basic.storage@1"
            && grant.operation == "read"
    })
}

pub(crate) fn can_switch_native_model(spec: &Composition) -> bool {
    let Some(resource) = spec.resource.as_deref() else {
        return false;
    };
    if installed_model_provider(spec).is_none() || !spec.authority.resources.contains(resource) {
        return false;
    }
    [
        ("phenix.basic.storage@1", "read"),
        ("phenix.basic.storage@1", "transact"),
        ("phenix.provider.http-model-session@2", "install"),
        ("phenix.provider.http-model-session@2", "release"),
    ]
    .into_iter()
    .all(|(interface, operation)| {
        spec.authority
            .operations
            .contains(&format!("{interface}/{operation}"))
            && spec
                .grants
                .iter()
                .filter(|grant| {
                    grant.caller == "root"
                        && grant.interface == interface
                        && grant.operation == operation
                        && grant.resources.len() == 1
                        && grant.resources.contains(resource)
                })
                .count()
                == 1
    })
}

pub(crate) fn scrub_portable_secret_bytes(value: &mut Value) {
    match value {
        Value::Bytes(bytes) => bytes.as_mut_slice().zeroize(),
        Value::List(items) => items.iter_mut().for_each(scrub_portable_secret_bytes),
        Value::Map(fields) => fields.values_mut().for_each(scrub_portable_secret_bytes),
        _ => {}
    }
}

fn install(
    kernel: &Kernel,
    composition: &Composition,
    management: Option<&ProviderManagementGroup>,
    resolvers: &[ModelCredentialResolver],
    session: &str,
    desired: &ModelSelection,
) -> Result<u64, DomainError> {
    if !can_switch_native_model(composition) {
        return Err(invalid(
            "selected Model install/release authority is unavailable",
        ));
    }
    let provider = installed_model_provider(composition)
        .ok_or_else(|| invalid("installed Model slot is missing or ambiguous"))?;
    // Check both actual session roots before resolving secrets or mutating state.
    for operation in ["install", "release"] {
        kernel
            .admit_root(Selection {
                interface: "phenix.provider.http-model-session@2".into(),
                operation: operation.into(),
                provider: provider.into(),
            })
            .map_err(internal)?;
    }
    let management =
        management.ok_or_else(|| invalid("current provider management is unavailable"))?;
    let binding = management.current_http_binding(desired)?;
    let credential = match (binding.credential_identity(), binding.authentication_kind()) {
        (None, None) => None,
        (
            Some(identity),
            Some(
                AuthenticationKind::ApiKey
                | AuthenticationKind::OAuthAuthorizationCode
                | AuthenticationKind::OAuthDeviceCode,
            ),
        ) => {
            binding
                .resolution_scope_for(identity)
                .map_err(provider_rejection)?;
            let resolver = resolvers
                .iter()
                .find(|source| source.identity == *identity)
                .ok_or_else(|| invalid("selected credential resolver is not installed"))?;
            Some((identity.clone(), resolver.resolve_for(identity)?))
        }
        _ => return Err(invalid("selected model authentication is unsupported")),
    };
    if management.current_http_binding(desired)? != binding {
        return Err(invalid(
            "selected model authentication changed before install",
        ));
    }
    let install = HttpModelInstall::from_checked(&binding, session.into(), credential, None)
        .map_err(provider_rejection)?;
    let root = kernel
        .admit_root(Selection {
            interface: "phenix.provider.http-model-session@2".into(),
            operation: "install".into(),
            provider: provider.into(),
        })
        .map_err(internal)?;
    let mut payload = install.to_value();
    let outcome = root.invoke(&payload);
    scrub_portable_secret_bytes(&mut payload);
    match outcome.map_err(internal)? {
        Outcome::Success(value) => u64::from_value(value).map_err(internal),
        Outcome::Domain(value) => Err(provider_rejection(
            ProviderError::from_value(value).map_err(internal)?,
        )),
    }
}

pub(crate) struct Installation {
    selection: Selection,
    payload: Value,
    release_failed: std::cell::Cell<bool>,
}
#[derive(Default)]
pub(crate) struct SelectedState {
    pub(crate) seen: std::collections::BTreeSet<String>,
    quarantined: std::collections::BTreeSet<String>,
}
impl SelectedState {
    /// Own the full turn boundary; callbacks retain their existing pump loops.
    /// Observation cleanup runs after physical drain/release, before quarantine.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_with<C, T, E: From<DomainError>>(
        &mut self,
        kernel: &Kernel,
        composition: &Composition,
        management: Option<&ProviderManagementGroup>,
        resolvers: &[ModelCredentialResolver],
        session: &str,
        submission: Submission,
        context: &mut C,
        run: impl FnOnce(&mut C, Option<&mut SessionAdmission>, Option<&Installation>) -> Result<T, E>,
        suppress: impl FnOnce(&mut C, &mut Result<T, E>, &Result<(), E>) -> Result<(), E>,
    ) -> Result<T, E> {
        self.check(session)?;
        let (_observation, mut owner, installation) = if submission.is_cancel() {
            let observation = SessionOwnership::acquire(session).map_err(|error| {
                DomainError::Admission(crate::session_lifecycle::AdmissionError::Ownership(
                    error.into(),
                ))
            })?;
            (Some(observation), None, None)
        } else {
            let (owner, installation) = self.admit(
                kernel,
                composition,
                management,
                resolvers,
                session,
                submission,
            )?;
            (None, Some(owner), installation)
        };
        let mut result = run(context, owner.as_mut(), installation.as_ref());
        let cleanup = installation
            .as_ref()
            .map_or(Ok(()), |installed| installed.release(kernel, None))
            .map_err(E::from);
        let suppression = suppress(context, &mut result, &cleanup);
        finalize(
            result,
            || cleanup.and(suppression),
            || self.quarantine(session),
        )
    }

    fn admit(
        &mut self,
        kernel: &Kernel,
        composition: &Composition,
        management: Option<&ProviderManagementGroup>,
        resolvers: &[ModelCredentialResolver],
        session: &str,
        submission: Submission,
    ) -> Result<(SessionAdmission, Option<Installation>), DomainError> {
        self.check(session)?;
        let owner = SessionAdmission::acquire(kernel, session, submission)
            .map_err(DomainError::Admission)?;
        let installation = self.acquire(kernel, composition, management, resolvers, session)?;
        Ok((owner, installation))
    }
    pub(crate) fn check(&self, session: &str) -> Result<(), DomainError> {
        if self.quarantined.contains(session) {
            return Err(invalid("selected model session is quarantined"));
        }
        Ok(())
    }
    pub(crate) fn is_quarantined(&self, session: &str) -> bool {
        self.quarantined.contains(session)
    }
    fn quarantine(&mut self, session: &str) {
        self.quarantined.insert(session.into());
    }
    fn acquire(
        &mut self,
        kernel: &Kernel,
        composition: &Composition,
        management: Option<&ProviderManagementGroup>,
        resolvers: &[ModelCredentialResolver],
        session: &str,
    ) -> Result<Option<Installation>, DomainError> {
        self.check(session)?;
        if !has_model_selection_storage(composition) {
            return Ok(None);
        }
        match crate::model_selection_state::read(kernel, composition, session)? {
            Some((desired, _)) => {
                self.seen.insert(session.into());
                let token = install(
                    kernel,
                    composition,
                    management,
                    resolvers,
                    session,
                    &desired,
                )?;
                Installation::new(composition, session, token).map(Some)
            }
            None if self.seen.contains(session) => {
                Err(invalid("selected model record was withdrawn"))
            }
            None => Ok(None),
        }
    }
}

pub(crate) fn prepare_resolvers(
    specs: &[Composition],
) -> Result<Vec<ModelCredentialResolver>, DomainError> {
    if specs.len() > 32 {
        return Err(invalid("too many authorized credential compositions"));
    }
    let resolvers = specs
        .iter()
        .map(prepare_model_resolver)
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = std::collections::BTreeSet::new();
    if resolvers.iter().any(|source| {
        !seen.insert(
            source
                .identity
                .resource()
                .expect("validated credential identity"),
        )
    }) {
        return Err(invalid("duplicate model credential root"));
    }
    Ok(resolvers)
}

impl Installation {
    fn new(composition: &Composition, session: &str, token: u64) -> Result<Self, DomainError> {
        Ok(Self {
            release_failed: std::cell::Cell::new(false),
            selection: Selection {
                interface: "phenix.provider.http-model-session@2".into(),
                operation: "release".into(),
                provider: installed_model_provider(composition)
                    .ok_or_else(|| invalid("installed Model slot is missing or ambiguous"))?
                    .into(),
            },
            payload: HttpModelRelease {
                session: session.into(),
                installation: token,
            }
            .to_value(),
        })
    }
    fn release(
        &self,
        kernel: &Kernel,
        execution: Option<&GraphExecution<'_>>,
    ) -> Result<(), DomainError> {
        let result = self.release_inner(kernel, execution);
        if result.is_err() {
            self.release_failed.set(true);
        }
        if execution.is_none() && result.is_ok() && self.release_failed.get() {
            return Err(invalid("selected Model release previously failed"));
        }
        result
    }

    fn release_inner(
        &self,
        kernel: &Kernel,
        execution: Option<&GraphExecution<'_>>,
    ) -> Result<(), DomainError> {
        let outcome = if let Some(execution) = execution {
            match execution.invoke_selected(&self.selection, &self.payload) {
                Err(failure) if failure.code == phenix_contract::Code::ReentrancyRejected => {
                    return Ok(())
                }
                outcome => outcome,
            }
        } else {
            kernel
                .admit_root(self.selection.clone())
                .map_err(internal)?
                .invoke(&self.payload)
        };
        match outcome.map_err(internal)? {
            Outcome::Success(value) => <()>::from_value(value).map_err(internal),
            Outcome::Domain(value) => Err(provider_rejection(
                ProviderError::from_value(value).map_err(internal)?,
            )),
        }
    }
    fn cancel(
        &self,
        kernel: &Kernel,
        execution: &mut GraphExecution<'_>,
    ) -> Result<(), DomainError> {
        let cancellation = execution.cancel().map_err(internal);
        let release = self.release(kernel, Some(execution));
        cancellation.and(release)
    }
}

/// Drain on every exit, including observer and admission-marker failures.
/// The original error wins; revocation faults still trigger quarantine.
pub(crate) fn drain(execution: &mut GraphExecution<'_>) {
    let _ = execution.cancel();
    loop {
        match execution.pump(|_| {}) {
            Ok(GraphStatus::Finished) => break,
            Ok(_) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) if execution.completion().is_some() => break,
            Err(_) => {
                let _ = execution.cancel();
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

/// Keeps frontend pump loops while preventing abandonment on observer errors.
pub(crate) struct Execution<'a, 'b> {
    graph: GraphExecution<'a>,
    kernel: &'a Kernel,
    installation: Option<&'b Installation>,
    finished: bool,
    cancelling: bool,
}
impl<'a, 'b> Execution<'a, 'b> {
    pub(crate) fn new(
        graph: GraphExecution<'a>,
        kernel: &'a Kernel,
        installation: Option<&'b Installation>,
    ) -> Self {
        Self {
            graph,
            kernel,
            installation,
            finished: false,
            cancelling: false,
        }
    }
    pub(crate) fn cancel(&mut self) -> Result<(), DomainError> {
        if self.cancelling {
            return Ok(());
        }
        self.cancelling = true;
        if let Some(installation) = self.installation {
            installation.cancel(self.kernel, &mut self.graph)
        } else {
            self.graph.cancel().map_err(internal)
        }
    }
    pub(crate) fn finish(&mut self) {
        self.finished = true;
    }
}
impl<'a> std::ops::Deref for Execution<'a, '_> {
    type Target = GraphExecution<'a>;
    fn deref(&self) -> &Self::Target {
        &self.graph
    }
}
impl std::ops::DerefMut for Execution<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.graph
    }
}
impl Drop for Execution<'_, '_> {
    fn drop(&mut self) {
        if !self.finished {
            if self.graph.completion().is_none() {
                let _ = self.cancel();
            }
            drain(&mut self.graph);
        }
    }
}

pub(crate) fn finalize<T, E>(
    result: Result<T, E>,
    cleanup: impl FnOnce() -> Result<(), E>,
    quarantine: impl FnOnce(),
) -> Result<T, E> {
    let cleanup = cleanup();
    if cleanup.is_err() {
        quarantine();
    }
    match result {
        Err(error) => Err(error),
        Ok(value) => cleanup.map(|()| value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires packaged native compositions; run by selected-provider-admission check"]
    fn native_install_is_released_after_pre_model_admission_failure() {
        let load = |name| -> Composition {
            serde_json::from_slice(
                &std::fs::read(std::env::var(name).expect("missing packaged fixture path"))
                    .unwrap(),
            )
            .unwrap()
        };
        let management = ProviderManagementGroup::prepare(&[load("PHENIX_TEST_MANAGEMENT")])
            .unwrap()
            .unwrap();
        let desired = management
            .authorized_http_offers(&[])
            .unwrap()
            .remove(0)
            .selected;
        let binding = management.current_http_binding(&desired).unwrap();
        for (path, release_fault) in [
            ("PHENIX_TEST_SELECTED", false),
            ("PHENIX_TEST_SELECTED_RELEASE_FAULT", true),
        ] {
            let composition = load(path);
            let kernel = Prepared::prepare(composition.clone())
                .unwrap()
                .load()
                .unwrap();
            let session = composition
                .resource
                .as_deref()
                .unwrap()
                .strip_prefix("phenix.session.")
                .unwrap();
            crate::model_selection_state::compare_and_swap(
                &kernel,
                &composition,
                session,
                &binding,
                u64::from(release_fault),
            )
            .unwrap();
            let mut state = SelectedState::default();
            let submission = Submission::Dispatch {
                action: "phenix.basic.submit".into(),
                payload: phenix_sdk::Blob(
                    phenix_basic_contracts::PromptSubmitted {
                        session: session.into(),
                        text: "must not reach Model".into(),
                        limits: Default::default(),
                    }
                    .to_value()
                    .encode()
                    .unwrap(),
                ),
            };
            let mut primary = String::new();
            let result: Result<(), String> = state.run_with(
                &kernel,
                &composition,
                Some(&management),
                &[],
                session,
                submission,
                &mut primary,
                |primary, owner, installation| {
                    assert!(installation.is_some(), "real native installation required");
                    // Native overlap rejection proves install preceded admission.
                    assert!(install(
                        &kernel,
                        &composition,
                        Some(&management),
                        &[],
                        session,
                        &desired
                    )
                    .is_err());
                    let mut scope = basic_scope(&composition, session);
                    scope.authority.operations.clear();
                    scope.authority.resources.clear();
                    *primary = match owner.unwrap().begin(&kernel, scope) {
                        Err(error) => error,
                        Ok(_) => panic!("empty authority admitted a graph"),
                    };
                    Err(primary.clone())
                },
                |_, _, cleanup| {
                    assert_eq!(cleanup.is_err(), release_fault);
                    Ok(())
                },
            );
            assert_eq!(result.unwrap_err(), primary);
            assert_eq!(state.check(session).is_err(), release_fault);
            assert!(state.check("independent").is_ok());
            if !release_fault {
                let token = install(
                    &kernel,
                    &composition,
                    Some(&management),
                    &[],
                    session,
                    &desired,
                )
                .expect("successful release must permit real reinstall");
                Installation::new(&composition, session, token)
                    .unwrap()
                    .release(&kernel, None)
                    .unwrap();
            }
            kernel.shutdown().unwrap();
        }
        management.shutdown().unwrap();
    }

    #[test]
    fn kernel_failure_display_redacts_native_prose_but_retains_typed_failure() {
        let error = internal(phenix_contract::Failure::new(
            phenix_contract::Code::PluginFailed,
            "credential-sentinel-do-not-export",
        ));
        let rendered = error.to_string();
        assert!(rendered.contains("PluginFailed"));
        assert!(!rendered.contains("credential-sentinel"));
        let DomainError::Kernel(failure) = error else {
            panic!("lost typed failure")
        };
        assert_eq!(failure.code, phenix_contract::Code::PluginFailed);
    }

    #[test]
    fn quarantine_is_sticky_and_session_local() {
        let mut state = SelectedState::default();
        state.quarantine("failed");
        assert!(state.check("failed").is_err());
        assert!(state.check("independent").is_ok());
        state.seen.remove("failed");
        assert!(state.check("failed").is_err());
    }
}
