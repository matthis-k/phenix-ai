use crate::{
    ComponentInterface, ComponentInvocationError, Exact, KernelError, Key, PhenixValue, Project,
    SdkClient, ValueError,
};
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    fmt::{self, Debug, Display, Formatter},
};

const DOMAIN_ERROR_TAG: &str = "_phenix/domain_error";

/// Canonical semantic result of a resolved Phenix call.
///
/// Success and declared domain errors remain structural values. Runtime failures
/// are carried separately through [`InvocationResult`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", content = "value", rename_all = "snake_case")]
pub enum InvocationOutcome {
    Success(PhenixValue),
    DomainError(PhenixValue),
}

impl InvocationOutcome {
    #[must_use]
    pub fn success(value: PhenixValue) -> Self {
        Self::Success(value)
    }

    #[must_use]
    pub fn domain_error(value: PhenixValue) -> Self {
        Self::DomainError(value)
    }

    /// Lower one semantic outcome to the structural component transport.
    ///
    /// Success remains the legacy bare value. Domain errors use a reserved outer
    /// variant so old success-only callers remain wire compatible.
    #[must_use]
    pub fn into_transport_value(self) -> PhenixValue {
        match self {
            Self::Success(value) => value,
            Self::DomainError(value) => PhenixValue::Variant {
                tag: Key::parse(DOMAIN_ERROR_TAG)
                    .expect("static domain error transport tag is valid"),
                value: Box::new(value),
            },
        }
    }

    /// Lift the structural component transport to its semantic outcome.
    #[must_use]
    pub fn from_transport_value(value: PhenixValue) -> Self {
        match value {
            PhenixValue::Variant { tag, value } if tag.as_str() == DOMAIN_ERROR_TAG => {
                Self::DomainError(*value)
            }
            value => Self::Success(value),
        }
    }
}

/// Stable machine-readable classification for failures owned by the runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationFailureClass {
    Resolution,
    Authority,
    Conversion,
    Cancellation,
    Host,
    Bridge,
    Execution,
}

/// Runtime-owned invocation failure.
///
/// `message` is diagnostic detail only. Callers branch on `class`, never on the
/// rendered text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InvocationFailure {
    class: InvocationFailureClass,
    message: String,
}

impl InvocationFailure {
    #[must_use]
    pub fn new(class: InvocationFailureClass, message: impl Into<String>) -> Self {
        Self {
            class,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn class(&self) -> InvocationFailureClass {
        self.class
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl Display for InvocationFailure {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.class, self.message)
    }
}

impl Error for InvocationFailure {}

impl From<ComponentInvocationError> for InvocationFailure {
    fn from(error: ComponentInvocationError) -> Self {
        let class = match &error {
            ComponentInvocationError::InterfaceMismatch { .. }
            | ComponentInvocationError::UnboundImport { .. }
            | ComponentInvocationError::Graph(_)
            | ComponentInvocationError::InvalidInterface { .. } => {
                InvocationFailureClass::Resolution
            }
            ComponentInvocationError::Encode(_) | ComponentInvocationError::Decode(_) => {
                InvocationFailureClass::Conversion
            }
            ComponentInvocationError::Kernel(error) => kernel_failure_class(error),
        };
        Self::new(class, error.to_string())
    }
}

fn kernel_failure_class(error: &KernelError) -> InvocationFailureClass {
    match error {
        KernelError::ServiceDenied { .. } | KernelError::HostOperationDenied { .. } => {
            InvocationFailureClass::Authority
        }
        KernelError::ServiceCancelled { .. } => InvocationFailureClass::Cancellation,
        KernelError::ContinuationUnavailable
        | KernelError::ContinuationAlreadyUsed(_)
        | KernelError::CausalServiceReentry(_) => InvocationFailureClass::Host,
        KernelError::RuntimeProviderUnavailable(_)
        | KernelError::RuntimeProviderNotExecutable { .. }
        | KernelError::RuntimeProviderContractUnavailable { .. }
        | KernelError::RuntimePrepare { .. } => InvocationFailureClass::Bridge,
        KernelError::UnknownDependency { .. }
        | KernelError::DependencyCycle(_)
        | KernelError::UnknownPlugin(_)
        | KernelError::NoEligibleProvider(_)
        | KernelError::BoundProviderUnavailable { .. }
        | KernelError::RequiredLayerUnavailable { .. }
        | KernelError::PluginNotActive(_)
        | KernelError::EmbeddedFactoryMissing(_)
        | KernelError::WrongExecutionKind(_)
        | KernelError::ComponentGraph(_)
        | KernelError::ResolvedGenerationMissing => InvocationFailureClass::Resolution,
        _ => InvocationFailureClass::Execution,
    }
}

/// Canonical invocation result. Domain errors are values. Runtime failures are
/// classifications owned by Core.
pub type InvocationResult = Result<InvocationOutcome, InvocationFailure>;

/// Error returned by a typed consumer after interpreting an invocation result.
#[derive(Debug, PartialEq)]
pub enum CallError<DomainError> {
    Domain(DomainError),
    Runtime(InvocationFailure),
    Conversion(ValueError),
}

impl<DomainError> From<InvocationFailure> for CallError<DomainError> {
    fn from(error: InvocationFailure) -> Self {
        Self::Runtime(error)
    }
}

impl<DomainError: Display> Display for CallError<DomainError> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Domain(error) => Display::fmt(error, f),
            Self::Runtime(error) => Display::fmt(error, f),
            Self::Conversion(error) => write!(f, "structural call conversion failed: {error}"),
        }
    }
}

impl<DomainError> Error for CallError<DomainError>
where
    DomainError: Debug + Display + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Domain(_) => None,
            Self::Runtime(error) => Some(error),
            Self::Conversion(error) => Some(error),
        }
    }
}

impl<'host, 'runtime, I: ComponentInterface> SdkClient<'host, 'runtime, I> {
    /// Invoke a component import and retain its structural domain outcome.
    pub fn invoke_outcome_value(&self, request: &PhenixValue) -> InvocationResult {
        self.invoke_value(request)
            .map(InvocationOutcome::from_transport_value)
            .map_err(InvocationFailure::from)
    }

    /// Invoke and decode both success and domain error using consumer-owned views.
    pub fn invoke_fallible<Request, Response, DomainError>(
        &self,
        request: &Request,
    ) -> Result<Response, CallError<DomainError>>
    where
        for<'value> PhenixValue: From<&'value Request>,
        for<'value> Response: TryFrom<&'value PhenixValue, Error = ValueError>,
        for<'value> DomainError: TryFrom<&'value PhenixValue, Error = ValueError>,
    {
        let outcome = self.invoke_outcome_value(&PhenixValue::from(request))?;
        match outcome {
            InvocationOutcome::Success(value) => {
                Response::try_from(&value).map_err(CallError::Conversion)
            }
            InvocationOutcome::DomainError(value) => {
                let error = DomainError::try_from(&value).map_err(CallError::Conversion)?;
                Err(CallError::Domain(error))
            }
        }
    }

    /// Invoke and project both success and domain error into consumer-owned views.
    pub fn invoke_fallible_projected<Request, Response, DomainError>(
        &self,
        request: &Request,
    ) -> Result<Response, CallError<DomainError>>
    where
        for<'value> PhenixValue: From<&'value Request>,
        for<'value> Response: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
        for<'value> DomainError: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
    {
        let outcome = self.invoke_outcome_value(&PhenixValue::from(request))?;
        match outcome {
            InvocationOutcome::Success(value) => {
                Response::try_from(Project(&value)).map_err(CallError::Conversion)
            }
            InvocationOutcome::DomainError(value) => {
                let error =
                    DomainError::try_from(Project(&value)).map_err(CallError::Conversion)?;
                Err(CallError::Domain(error))
            }
        }
    }

    /// Invoke and require exact consumer-owned success and domain error views.
    pub fn invoke_fallible_exact<Request, Response, DomainError>(
        &self,
        request: &Request,
    ) -> Result<Response, CallError<DomainError>>
    where
        for<'value> PhenixValue: From<&'value Request>,
        for<'value> Response: TryFrom<Exact<&'value PhenixValue>, Error = ValueError>,
        for<'value> DomainError: TryFrom<Exact<&'value PhenixValue>, Error = ValueError>,
    {
        let outcome = self.invoke_outcome_value(&PhenixValue::from(request))?;
        match outcome {
            InvocationOutcome::Success(value) => {
                Response::try_from(Exact(&value)).map_err(CallError::Conversion)
            }
            InvocationOutcome::DomainError(value) => {
                let error = DomainError::try_from(Exact(&value)).map_err(CallError::Conversion)?;
                Err(CallError::Domain(error))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn variant(tag: &str, detail: &str) -> PhenixValue {
        PhenixValue::Variant {
            tag: Key::parse(tag).unwrap(),
            value: Box::new(PhenixValue::Table(BTreeMap::from([(
                Key::parse("detail").unwrap(),
                PhenixValue::String(detail.into()),
            )]))),
        }
    }

    #[test]
    fn structural_outcome_keeps_domain_identity() {
        let conflict = InvocationOutcome::domain_error(variant("conflict", "same display"));
        let disconnected = InvocationOutcome::domain_error(variant("disconnected", "same display"));

        assert_ne!(conflict, disconnected);
        assert_eq!(
            serde_json::from_value::<InvocationOutcome>(serde_json::to_value(&conflict).unwrap())
                .unwrap(),
            conflict
        );
    }

    #[test]
    fn domain_transport_roundtrip_preserves_structured_value() {
        let domain = variant("conflict", "same display");
        let transport = InvocationOutcome::domain_error(domain.clone()).into_transport_value();

        assert_eq!(
            InvocationOutcome::from_transport_value(transport),
            InvocationOutcome::DomainError(domain)
        );
    }

    #[test]
    fn legacy_success_transport_stays_bare() {
        let success = variant("passed", "value");
        let transport = InvocationOutcome::success(success.clone()).into_transport_value();

        assert_eq!(transport, success);
        assert_eq!(
            InvocationOutcome::from_transport_value(transport),
            InvocationOutcome::Success(success)
        );
    }

    #[test]
    fn runtime_failure_class_does_not_depend_on_message() {
        let authority = InvocationFailure::new(InvocationFailureClass::Authority, "same display");
        let cancellation =
            InvocationFailure::new(InvocationFailureClass::Cancellation, "same display");

        assert_ne!(authority, cancellation);
        assert_eq!(authority.class(), InvocationFailureClass::Authority);
        assert_eq!(cancellation.class(), InvocationFailureClass::Cancellation);
    }

    #[test]
    fn kernel_cancellation_maps_without_message_matching() {
        let error = ComponentInvocationError::Kernel(KernelError::ServiceCancelled {
            plugin: crate::PluginId::parse("fixture.provider").unwrap(),
            service: crate::ServiceId::parse("fixture.run@1").unwrap(),
        });
        let failure = InvocationFailure::from(error);

        assert_eq!(failure.class(), InvocationFailureClass::Cancellation);
    }
}
