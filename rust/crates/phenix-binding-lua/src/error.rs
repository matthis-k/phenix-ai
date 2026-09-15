use phenix_client_acp::{ClientError, RequestRejection};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ErrorKind {
    Transport,
    Protocol,
    Cancelled,
    Rejected,
    UnsupportedCapability,
    QueueFull,
    Conversion,
}

impl ErrorKind {
    pub(crate) const fn as_str(&self) -> &'static str {
        match self {
            Self::Transport => "transport",
            Self::Protocol => "protocol",
            Self::Cancelled => "cancelled",
            Self::Rejected => "rejected",
            Self::UnsupportedCapability => "unsupported_capability",
            Self::QueueFull => "queue_full",
            Self::Conversion => "conversion",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BindingError {
    pub(crate) kind: ErrorKind,
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) details: Box<Option<serde_json::Value>>,
}

impl BindingError {
    pub(crate) fn local(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            code: kind.as_str().to_owned(),
            kind,
            message: message.into(),
            details: Box::new(None),
        }
    }

    pub(crate) fn transport(message: impl Into<String>) -> Self {
        Self::local(ErrorKind::Transport, message)
    }

    pub(crate) fn conversion(message: impl Into<String>) -> Self {
        Self::local(ErrorKind::Conversion, message)
    }

    pub(crate) fn unsupported(operation: &phenix_core::ContractId) -> Self {
        Self::local(
            ErrorKind::UnsupportedCapability,
            format!("application operation {operation} is not a negotiated ACP extension"),
        )
    }

    pub(crate) fn from_client(error: ClientError) -> Self {
        let message = error.to_string();
        let (kind, code, details) = match error {
            ClientError::Transport(_) => {
                (ErrorKind::Transport, "transport".to_owned(), Box::new(None))
            }
            ClientError::Protocol(_) | ClientError::OutOfOrderUpdate { .. } => {
                (ErrorKind::Protocol, "protocol".to_owned(), Box::new(None))
            }
            ClientError::Cancelled { details, .. } => {
                (ErrorKind::Cancelled, "cancelled".to_owned(), details)
            }
            ClientError::Rejected(rejection) => (
                ErrorKind::Rejected,
                rejection.class.unwrap_or_else(|| "rejected".to_owned()),
                Box::new(rejection.details),
            ),
            ClientError::UnsupportedCapability { .. } => (
                ErrorKind::UnsupportedCapability,
                "unsupported_capability".to_owned(),
                Box::new(None),
            ),
            ClientError::UpdateQueueFull => (
                ErrorKind::QueueFull,
                "queue_full".to_owned(),
                Box::new(None),
            ),
        };
        Self {
            kind,
            code,
            message,
            details,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_and_rejection_keep_distinct_codes() {
        let cancelled = BindingError::from_client(ClientError::Cancelled {
            message: "same display text".to_owned(),
            details: Box::new(None),
        });
        let rejected = BindingError::from_client(ClientError::Rejected(Box::new(
            RequestRejection {
                code: agent_client_protocol::ErrorCode::InternalError,
                class: Some("permission_denied".to_owned()),
                message: "same display text".to_owned(),
                details: Some(serde_json::json!({ "message": "same display text" })),
            },
        )));

        assert_eq!(cancelled.kind, ErrorKind::Cancelled);
        assert_eq!(cancelled.code, "cancelled");
        assert_eq!(rejected.kind, ErrorKind::Rejected);
        assert_eq!(rejected.code, "permission_denied");
    }
}
