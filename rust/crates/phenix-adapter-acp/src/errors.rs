use phenix_application_interface::types::ApplicationError;
use serde_json::{Value, json};

/// Present one typed application failure as ACP without collapsing its Phenix class.
#[must_use]
pub fn application_error_to_acp(error: ApplicationError) -> agent_client_protocol::Error {
    let class = error.class();
    match error {
        ApplicationError::UnsupportedCapability { capability } => {
            agent_client_protocol::Error::method_not_found().data(error_data(
                "unsupported_capability",
                json!({ "capability": capability.as_str() }),
            ))
        }
        ApplicationError::InvalidInput { message } => {
            agent_client_protocol::Error::invalid_params()
                .data(error_data("invalid_input", json!({ "message": message })))
        }
        ApplicationError::InvalidResponse { message } => {
            agent_client_protocol::Error::internal_error().data(error_data(
                "invalid_response",
                json!({ "message": message }),
            ))
        }
        ApplicationError::NotFound { resource } => {
            agent_client_protocol::Error::resource_not_found(Some(resource.clone()))
                .data(error_data("not_found", json!({ "resource": resource })))
        }
        ApplicationError::Unauthenticated { message } => {
            agent_client_protocol::Error::auth_required()
                .data(error_data("unauthenticated", json!({ "message": message })))
        }
        ApplicationError::PermissionDenied { message } => {
            agent_client_protocol::Error::internal_error().data(error_data(
                "permission_denied",
                json!({ "message": message }),
            ))
        }
        ApplicationError::Conflict { message } => agent_client_protocol::Error::internal_error()
            .data(error_data("conflict", json!({ "message": message }))),
        ApplicationError::Cancelled => agent_client_protocol::Error::request_cancelled()
            .data(error_data("cancelled", Value::Null)),
        ApplicationError::Disconnected => agent_client_protocol::Error::internal_error()
            .data(error_data("disconnected", Value::Null)),
        ApplicationError::Failed { message } => agent_client_protocol::Error::internal_error()
            .data(error_data("failed", json!({ "message": message }))),
        ApplicationError::UnknownValue { value } | ApplicationError::StaleReference { value } => {
            agent_client_protocol::Error::resource_not_found(Some(value.clone()))
                .data(error_data(class, json!({ "value": value })))
        }
        ApplicationError::InvalidPath { message }
        | ApplicationError::SchemaMismatch { message } => {
            agent_client_protocol::Error::invalid_params()
                .data(error_data(class, json!({ "message": message })))
        }
        ApplicationError::UnsupportedSnapshotPolicy { message }
        | ApplicationError::TransactionConflict { message } => {
            agent_client_protocol::Error::internal_error()
                .data(error_data(class, json!({ "message": message })))
        }
        ApplicationError::SubscriptionCapacity | ApplicationError::Closed => {
            agent_client_protocol::Error::internal_error().data(error_data(class, Value::Null))
        }
    }
}

fn error_data(class: &str, detail: Value) -> Value {
    json!({
        "phenix": {
            "class": class,
            "detail": detail,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::ErrorCode;
    use phenix_core::{InvocationFailure, InvocationFailureClass};

    fn class(error: &agent_client_protocol::Error) -> &str {
        error.data.as_ref().unwrap()["phenix"]["class"]
            .as_str()
            .unwrap()
    }

    #[test]
    fn acp_data_distinguishes_application_failures_sharing_json_rpc_code() {
        let denied = application_error_to_acp(ApplicationError::PermissionDenied {
            message: "same display text".to_owned(),
        });
        let conflict = application_error_to_acp(ApplicationError::Conflict {
            message: "same display text".to_owned(),
        });
        let failed = application_error_to_acp(ApplicationError::Failed {
            message: "same display text".to_owned(),
        });

        assert_eq!(denied.code, ErrorCode::InternalError);
        assert_eq!(conflict.code, ErrorCode::InternalError);
        assert_eq!(failed.code, ErrorCode::InternalError);
        assert_eq!(class(&denied), "permission_denied");
        assert_eq!(class(&conflict), "conflict");
        assert_eq!(class(&failed), "failed");
    }

    #[test]
    fn core_cancellation_and_bridge_classes_survive_application_and_acp_mapping() {
        let cancelled = application_error_to_acp(ApplicationError::from(InvocationFailure::new(
            InvocationFailureClass::Cancellation,
            "same display text",
        )));
        let disconnected = application_error_to_acp(ApplicationError::from(
            InvocationFailure::new(InvocationFailureClass::Bridge, "same display text"),
        ));

        assert_eq!(cancelled.code, ErrorCode::RequestCancelled);
        assert_eq!(class(&cancelled), "cancelled");
        assert_eq!(disconnected.code, ErrorCode::InternalError);
        assert_eq!(class(&disconnected), "disconnected");
    }

    #[test]
    fn malformed_payload_class_survives_application_and_acp_mapping() {
        let malformed = application_error_to_acp(ApplicationError::from(InvocationFailure::new(
            InvocationFailureClass::Conversion,
            "malformed payload",
        )));

        assert_eq!(malformed.code, ErrorCode::InternalError);
        assert_eq!(class(&malformed), "invalid_response");
    }
}
