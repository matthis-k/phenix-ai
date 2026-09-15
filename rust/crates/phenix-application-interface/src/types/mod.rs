//! Application-owned payloads. Runtime service records and protocol envelopes stay at adapters.
use phenix_core::{
    CallableId, ContractId, InvocationFailure, InvocationFailureClass, ModelId, PhenixSchema,
    PhenixValue, RoutingProfileId, SessionId, SkillId,
};
use phenix_sdk_macros::{PhenixContract, PhenixValue};

macro_rules! record {
    ($name:ident, $id:literal, { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug, PartialEq, PhenixValue, PhenixContract)]
        #[phenix(id = $id)]
        pub struct $name { $(pub $field: $ty),* }
    };
}
macro_rules! variants {
    ($name:ident, $id:literal, { $($variant:ident $( { $($field:ident: $ty:ty),* $(,)? } )?),* $(,)? }) => {
        #[derive(Clone, Debug, PartialEq, PhenixValue, PhenixContract)]
        #[phenix(id = $id)]
        pub enum $name { $($variant $( { $($field: $ty),* } )?),* }
    };
}

mod discovery;
mod interaction;
mod observable;
mod session;
pub use discovery::*;
pub use interaction::*;
pub use observable::*;
pub use session::*;

record!(Empty, "phenix.application.type.empty@1", {});
record!(Acknowledged, "phenix.application.type.acknowledged@1", {});

variants!(ApplicationError, "phenix.application.error@1", {
    UnsupportedCapability { capability: ContractId },
    InvalidInput { message: String },
    InvalidResponse { message: String },
    NotFound { resource: String },
    Unauthenticated { message: String },
    PermissionDenied { message: String },
    Conflict { message: String },
    Cancelled,
    Disconnected,
    Failed { message: String },
    UnknownValue { value: String },
    InvalidPath { message: String },
    SchemaMismatch { message: String },
    StaleReference { value: String },
    UnsupportedSnapshotPolicy { message: String },
    TransactionConflict { message: String },
    SubscriptionCapacity,
    Closed,
});

impl ApplicationError {
    #[must_use]
    pub const fn class(&self) -> &'static str {
        match self {
            Self::UnsupportedCapability { .. } => "unsupported_capability",
            Self::InvalidInput { .. } => "invalid_input",
            Self::InvalidResponse { .. } => "invalid_response",
            Self::NotFound { .. } => "not_found",
            Self::Unauthenticated { .. } => "unauthenticated",
            Self::PermissionDenied { .. } => "permission_denied",
            Self::Conflict { .. } => "conflict",
            Self::Cancelled => "cancelled",
            Self::Disconnected => "disconnected",
            Self::Failed { .. } => "failed",
            Self::UnknownValue { .. } => "unknown_value",
            Self::InvalidPath { .. } => "invalid_path",
            Self::SchemaMismatch { .. } => "schema_mismatch",
            Self::StaleReference { .. } => "stale_reference",
            Self::UnsupportedSnapshotPolicy { .. } => "unsupported_snapshot_policy",
            Self::TransactionConflict { .. } => "transaction_conflict",
            Self::SubscriptionCapacity => "subscription_capacity",
            Self::Closed => "closed",
        }
    }
}

impl From<InvocationFailure> for ApplicationError {
    fn from(error: InvocationFailure) -> Self {
        let message = error.message().to_owned();
        match error.class() {
            InvocationFailureClass::Resolution => Self::NotFound { resource: message },
            InvocationFailureClass::Authority => Self::PermissionDenied { message },
            InvocationFailureClass::Conversion => Self::InvalidResponse { message },
            InvocationFailureClass::Cancellation => Self::Cancelled,
            InvocationFailureClass::Host => Self::Conflict { message },
            InvocationFailureClass::Bridge => Self::Disconnected,
            InvocationFailureClass::Execution => Self::Failed { message },
        }
    }
}

impl From<phenix_core::ObservableError> for ApplicationError {
    fn from(error: phenix_core::ObservableError) -> Self {
        use phenix_core::ObservableError;
        match error {
            ObservableError::UnknownValue(value) => Self::UnknownValue {
                value: value.to_string(),
            },
            ObservableError::DuplicateValue(value) => Self::Conflict {
                message: format!("observable value {value} is already registered"),
            },
            ObservableError::InvalidPath { message, .. } => Self::InvalidPath {
                message: message.to_string(),
            },
            ObservableError::SchemaMismatch { message, .. } => Self::SchemaMismatch {
                message: message.to_string(),
            },
            ObservableError::StaleReference(value) => Self::StaleReference {
                value: value.to_string(),
            },
            other @ ObservableError::UnsupportedSnapshotPolicy { .. } => {
                Self::UnsupportedSnapshotPolicy {
                    message: other.to_string(),
                }
            }
            ObservableError::TransactionConflict { message, .. } => {
                Self::TransactionConflict { message }
            }
            ObservableError::SubscriptionCapacity => Self::SubscriptionCapacity,
            ObservableError::Closed => Self::Closed,
        }
    }
}

impl From<phenix_core::CapabilityError> for ApplicationError {
    fn from(error: phenix_core::CapabilityError) -> Self {
        use phenix_core::CapabilityError;
        match error {
            CapabilityError::UnknownReference(reference) => Self::NotFound {
                resource: reference.id().to_string(),
            },
            CapabilityError::StaleReference(reference) => Self::StaleReference {
                value: reference.id().to_string(),
            },
            CapabilityError::DuplicateReference(reference) => Self::Conflict {
                message: format!(
                    "capability reference {} is already registered",
                    reference.id()
                ),
            },
            CapabilityError::SchemaMismatch { message } => Self::SchemaMismatch { message },
            CapabilityError::ProviderFailed { message } => Self::Failed { message },
            CapabilityError::Cancelled => Self::Cancelled,
            CapabilityError::Disconnected => Self::Disconnected,
            CapabilityError::QueueFull => Self::Conflict {
                message: "capability provider queue is full".to_owned(),
            },
        }
    }
}

impl std::fmt::Display for ApplicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedCapability { capability } => {
                write!(f, "unsupported capability: {capability}")
            }
            Self::NotFound { resource } => write!(f, "resource not found: {resource}"),
            Self::InvalidInput { message } => write!(f, "invalid input: {message}"),
            Self::InvalidResponse { message } => write!(f, "invalid response: {message}"),
            Self::Unauthenticated { message } => write!(f, "authentication required: {message}"),
            Self::PermissionDenied { message } => write!(f, "permission denied: {message}"),
            Self::Conflict { message } => write!(f, "conflict: {message}"),
            Self::Failed { message } => write!(f, "application failure: {message}"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Disconnected => f.write_str("disconnected"),
            Self::UnknownValue { value } => write!(f, "unknown observable value: {value}"),
            Self::InvalidPath { message } => write!(f, "invalid observable path: {message}"),
            Self::SchemaMismatch { message } => write!(f, "observable schema mismatch: {message}"),
            Self::StaleReference { value } => write!(f, "stale observable reference: {value}"),
            Self::UnsupportedSnapshotPolicy { message } => {
                write!(f, "unsupported observable snapshot policy: {message}")
            }
            Self::TransactionConflict { message } => {
                write!(f, "observable transaction conflict: {message}")
            }
            Self::SubscriptionCapacity => f.write_str("observable subscription capacity reached"),
            Self::Closed => f.write_str("observable store is closed"),
        }
    }
}
impl std::error::Error for ApplicationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_failure_classes_map_without_message_matching() {
        let cases = [
            (InvocationFailureClass::Resolution, "not_found"),
            (InvocationFailureClass::Authority, "permission_denied"),
            (InvocationFailureClass::Conversion, "invalid_response"),
            (InvocationFailureClass::Cancellation, "cancelled"),
            (InvocationFailureClass::Host, "conflict"),
            (InvocationFailureClass::Bridge, "disconnected"),
            (InvocationFailureClass::Execution, "failed"),
        ];

        for (runtime, application) in cases {
            let error =
                ApplicationError::from(InvocationFailure::new(runtime, "same display text"));
            assert_eq!(error.class(), application);
        }
    }

    #[test]
    fn observable_failures_keep_structural_classes() {
        let error = ApplicationError::from(phenix_core::ObservableError::SubscriptionCapacity);
        assert_eq!(error.class(), "subscription_capacity");
    }
}
