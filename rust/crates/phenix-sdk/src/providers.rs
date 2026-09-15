use phenix_core::{Authority, KernelAccess, KernelError, PluginContext, PluginId};
use phenix_provider_sdk::{
    provider_auth_service, Auth, AuthDescriptor, AuthKind, ProviderAuthCommand,
    ProviderAuthResponse,
};
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Clone, Copy)]
pub struct Providers<'host, 'runtime> {
    kernel: KernelAccess<'host, 'runtime>,
    authority: &'host Authority,
}

impl<'host, 'runtime> Providers<'host, 'runtime> {
    pub fn get(
        &self,
        id: impl Into<String>,
    ) -> Result<Provider<'host, 'runtime>, ProviderSdkError> {
        let id = PluginId::parse(id.into()).map_err(|_| ProviderSdkError::InvalidProviderId)?;
        Ok(Provider {
            id,
            providers: *self,
        })
    }
}

pub struct Provider<'host, 'runtime> {
    id: PluginId,
    providers: Providers<'host, 'runtime>,
}

impl<'host, 'runtime> Provider<'host, 'runtime> {
    pub fn id(&self) -> &PluginId {
        &self.id
    }

    pub fn add_auth(&self, auth: Auth) -> Result<AuthDescriptor, ProviderSdkError> {
        match self.invoke(&ProviderAuthCommand::Add { auth })? {
            ProviderAuthResponse::Added { auth } => Ok(auth),
            _ => Err(ProviderSdkError::UnexpectedResponse("adding provider auth")),
        }
    }

    pub fn auth_methods(&self) -> Result<Vec<AuthKind>, ProviderSdkError> {
        match self.invoke(&ProviderAuthCommand::Methods)? {
            ProviderAuthResponse::Methods { methods } => Ok(methods),
            _ => Err(ProviderSdkError::UnexpectedResponse(
                "listing provider auth methods",
            )),
        }
    }

    pub fn list_auth(&self) -> Result<Vec<AuthDescriptor>, ProviderSdkError> {
        match self.invoke(&ProviderAuthCommand::List)? {
            ProviderAuthResponse::Credentials { credentials } => Ok(credentials),
            _ => Err(ProviderSdkError::UnexpectedResponse(
                "listing provider auth",
            )),
        }
    }

    pub fn remove_auth(&self, kind: AuthKind) -> Result<Option<AuthDescriptor>, ProviderSdkError> {
        match self.invoke(&ProviderAuthCommand::Remove { kind })? {
            ProviderAuthResponse::Removed { auth } => Ok(auth),
            _ => Err(ProviderSdkError::UnexpectedResponse(
                "removing provider auth",
            )),
        }
    }

    fn invoke(
        &self,
        command: &ProviderAuthCommand,
    ) -> Result<ProviderAuthResponse, ProviderSdkError> {
        let input = serde_json::to_vec(command).map_err(ProviderSdkError::Encode)?;
        let output = self
            .providers
            .kernel
            .invoke_service_abi(
                &provider_auth_service(),
                &input,
                self.providers.authority,
                Some(&self.id),
            )
            .map_err(ProviderSdkError::Kernel)?;
        serde_json::from_slice(&output).map_err(ProviderSdkError::Decode)
    }
}

pub trait ProviderSdkExt<'host, 'runtime> {
    fn providers(&self) -> Providers<'host, 'runtime>;
}

impl<'host, 'runtime, Sdk, Settings, State> ProviderSdkExt<'host, 'runtime>
    for PluginContext<'host, 'runtime, Sdk, Settings, State>
{
    fn providers(&self) -> Providers<'host, 'runtime> {
        Providers {
            kernel: self.kernel,
            authority: self.call.authority,
        }
    }
}

#[derive(Debug)]
pub enum ProviderSdkError {
    InvalidProviderId,
    Kernel(KernelError),
    Encode(serde_json::Error),
    Decode(serde_json::Error),
    UnexpectedResponse(&'static str),
}

impl Display for ProviderSdkError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProviderId => f.write_str("provider id must not be empty"),
            Self::Kernel(error) => Display::fmt(error, f),
            Self::Encode(error) => write!(f, "cannot encode provider SDK request: {error}"),
            Self::Decode(error) => write!(f, "cannot decode provider SDK response: {error}"),
            Self::UnexpectedResponse(operation) => {
                write!(f, "unexpected provider SDK response while {operation}")
            }
        }
    }
}

impl Error for ProviderSdkError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Kernel(error) => Some(error),
            Self::Encode(error) | Self::Decode(error) => Some(error),
            Self::InvalidProviderId | Self::UnexpectedResponse(_) => None,
        }
    }
}
