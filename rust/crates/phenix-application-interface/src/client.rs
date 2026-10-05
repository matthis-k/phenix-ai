use crate::{ApplicationDescriptor, descriptor::id, types::ApplicationError};
use phenix_core::{
    ContractId, InvocationOutcome, InvocationResult, PhenixContract, PhenixValue, ValueCodec,
};
use std::{collections::BTreeSet, future::Future};

pub trait Operation {
    const ID: &'static str;
    const CAPABILITY: &'static str;
    type Input: PhenixContract + ValueCodec;
    type Output: PhenixContract + ValueCodec;
}

/// Application feature support, never execution authority.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Capabilities(BTreeSet<ContractId>);

impl Capabilities {
    /// Unknown versions are ignored. An advertised feature must include its dependencies.
    pub fn negotiate(
        descriptor: &ApplicationDescriptor,
        advertised: impl IntoIterator<Item = ContractId>,
    ) -> Result<Self, ApplicationError> {
        let supported: BTreeSet<_> = advertised
            .into_iter()
            .filter(|capability| descriptor.capabilities.contains_key(capability))
            .collect();
        for capability in &supported {
            for dependency in &descriptor.capabilities[capability].dependencies {
                if !supported.contains(dependency) {
                    return Err(ApplicationError::UnsupportedCapability {
                        capability: dependency.clone(),
                    });
                }
            }
        }
        Ok(Self(supported))
    }

    pub fn require(&self, capability: &ContractId) -> Result<(), ApplicationError> {
        if self.0.contains(capability) {
            return Ok(());
        }
        Err(ApplicationError::UnsupportedCapability {
            capability: capability.clone(),
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = &ContractId> {
        self.0.iter()
    }
}

/// Lower Core's canonical invocation result into the fixed application contract.
///
/// Declared application failures remain typed structural values. Runtime failures
/// map by [`phenix_core::InvocationFailureClass`], never by rendered text.
pub fn map_invocation_result(result: InvocationResult) -> Result<PhenixValue, ApplicationError> {
    match result {
        Ok(InvocationOutcome::Success(value)) => Ok(value),
        Ok(InvocationOutcome::DomainError(value)) => {
            let error = ApplicationError::from_value(&value).map_err(|error| {
                ApplicationError::InvalidResponse {
                    message: format!("application domain error conversion failed: {error}"),
                }
            })?;
            Err(error)
        }
        Err(error) => Err(error.into()),
    }
}

/// Handwritten adapters own connection lifecycle and protocol semantics behind this boundary.
/// It imposes no executor, transport, runtime implementation, or Send requirement.
pub trait ApplicationTransport {
    fn invoke(
        &self,
        operation: &ContractId,
        input: PhenixValue,
    ) -> impl Future<Output = Result<PhenixValue, ApplicationError>>;
}

pub struct ApplicationClient<T> {
    transport: T,
    capabilities: Capabilities,
}

impl<T: ApplicationTransport> ApplicationClient<T> {
    pub fn new(transport: T, capabilities: Capabilities) -> Self {
        Self {
            transport,
            capabilities,
        }
    }

    pub async fn invoke<O: Operation>(
        &self,
        input: O::Input,
    ) -> Result<O::Output, ApplicationError> {
        self.capabilities.require(&id(O::CAPABILITY))?;
        let response = self.transport.invoke(&id(O::ID), input.to_value()).await?;
        O::Output::from_value(&response).map_err(|error| ApplicationError::InvalidResponse {
            message: error.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{InvocationFailure, InvocationFailureClass};

    #[test]
    fn application_domain_error_remains_structural() {
        let error = ApplicationError::Conflict {
            message: "workspace changed".to_owned(),
        };
        let result = map_invocation_result(Ok(InvocationOutcome::domain_error(error.to_value())));

        assert_eq!(result, Err(error));
    }

    #[test]
    fn application_runtime_failure_uses_core_classification() {
        let cancellation = map_invocation_result(Err(InvocationFailure::new(
            InvocationFailureClass::Cancellation,
            "same display text",
        )));
        let bridge = map_invocation_result(Err(InvocationFailure::new(
            InvocationFailureClass::Bridge,
            "same display text",
        )));

        assert_eq!(cancellation, Err(ApplicationError::Cancelled));
        assert_eq!(bridge, Err(ApplicationError::Disconnected));
    }

    #[test]
    fn malformed_application_domain_error_is_a_conversion_failure() {
        let result = map_invocation_result(Ok(InvocationOutcome::domain_error(
            PhenixValue::String("not an application error".to_owned()),
        )));

        assert!(matches!(
            result,
            Err(ApplicationError::InvalidResponse { message })
                if message.starts_with("application domain error conversion failed:")
        ));
    }
}
