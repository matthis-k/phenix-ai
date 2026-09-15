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

#[derive(Clone, Debug, PartialEq, PhenixValue, PhenixContract, thiserror::Error)]
#[phenix(id = "phenix.application.error@1")]
pub enum ApplicationError {
    #[error("unsupported capability: {capability}")]
    UnsupportedCapability { capability: ContractId },
    #[error("invalid input: {message}")]
    InvalidInput { message: String },
    #[error("invalid response: {message}")]
    InvalidResponse { message: String },
    #[error("resource not found: {resource}")]
    NotFound { resource: String },
    #[error("authentication required: {message}")]
    Unauthenticated { message: String },
    #[error("permission denied: {message}")]
    PermissionDenied { message: String },
    #[error("conflict: {message}")]
    Conflict { message: String },
    #[error("cancelled")]
    Cancelled,
    #[error("disconnected")]
    Disconnected,
    #[error("application failure: {message}")]
    Failed { message: String },
    #[error("unknown observable value: {value}")]
    UnknownValue { value: String },
    #[error("invalid observable path: {message}")]
    InvalidPath { message: String },
    #[error("observable schema mismatch: {message}")]
    SchemaMismatch { message: String },
    #[error("stale observable reference: {value}")]
    StaleReference { value: String },
    #[error("unsupported observable snapshot policy: {message}")]
    UnsupportedSnapshotPolicy { message: String },
    #[error("observable transaction conflict: {message}")]
    TransactionConflict { message: String },
    #[error("observable subscription capacity reached")]
    SubscriptionCapacity,
    #[error("observable store is closed")]
    Closed,
}

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
