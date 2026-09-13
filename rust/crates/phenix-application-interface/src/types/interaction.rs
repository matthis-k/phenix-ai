use super::*;

macro_rules! interaction_handler_ref {
    ($name:ident, $contract:literal, $input:ty => $output:ty) => {
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub struct $name(pub phenix_core::CallableRef);

        impl $name {
            #[must_use]
            pub fn reference(&self) -> &phenix_core::CallableRef {
                &self.0
            }

            #[must_use]
            pub fn into_reference(self) -> phenix_core::CallableRef {
                self.0
            }
        }

        impl phenix_core::ValueCodec for $name {
            fn phenix_type() -> phenix_core::Type {
                phenix_core::Type::Callable {
                    contract: phenix_core::ContractId::parse($contract)
                        .expect("static interaction callable contract is valid"),
                    input: Box::new(<$input as phenix_core::HasPhenixSchema>::phenix_schema()),
                    output: Box::new(<$output as phenix_core::HasPhenixSchema>::phenix_schema()),
                }
            }

            fn to_value(&self) -> PhenixValue {
                PhenixValue::Callable(self.0.clone())
            }

            fn from_value(value: &PhenixValue) -> Result<Self, phenix_core::ValueError> {
                <Self as phenix_core::ValueCodec>::phenix_type().parse(value)?;
                match value {
                    PhenixValue::Callable(reference) => Ok(Self(reference.clone())),
                    _ => unreachable!("validated callable value"),
                }
            }
        }
    };
}

// Sequence numbers increase within the declared scope. Resume snapshots include their watermark.
record!(SessionUpdate, "phenix.application.type.session-update@1", {
    session_id: SessionId,
    sequence: u64,
    update: SessionChange,
});
variants!(SessionChange, "phenix.application.type.session-change@1", {
    Message { message: Message },
    TextDelta { execution_id: String, text: String },
    Renamed { title: String },
    Closed,
    Execution { execution_id: String, update: ExecutionChange },
    Diagnostic { diagnostic: Diagnostic },
    Review { review: ReviewRecord },
});
record!(ExecutionUpdate, "phenix.application.type.execution-update@1", {
    session_id: SessionId,
    execution_id: String,
    sequence: u64,
    update: ExecutionChange,
});
variants!(ExecutionChange, "phenix.application.type.execution-change@1", {
    State { state: ExecutionState },
    ToolCall { call_id: String, callable_id: CallableId, input: PhenixValue },
    ToolResult { call_id: String, output: PhenixValue },
    ToolFailed { call_id: String, error: ApplicationError },
    Progress { message: String, fraction: Option<f64> },
});
record!(PermissionRequest, "phenix.application.type.permission-request@1", {
    session_id: SessionId,
    execution_id: String,
    call_id: String,
    description: String,
});
variants!(PermissionResponse, "phenix.application.type.permission-response@1", {
    AllowOnce, Deny, Cancelled,
});
record!(ElicitationRequest, "phenix.application.type.elicitation-request@1", {
    session_id: SessionId,
    message: String,
    schema: PhenixSchema,
});
variants!(ElicitationResponse, "phenix.application.type.elicitation-response@1", {
    Accepted { value: PhenixValue }, Declined, Cancelled,
});
interaction_handler_ref!(
    PermissionHandlerRef,
    "phenix.application.permission@1",
    PermissionRequest => PermissionResponse
);
interaction_handler_ref!(
    ElicitationHandlerRef,
    "phenix.application.elicitation@1",
    ElicitationRequest => ElicitationResponse
);
record!(InteractionHandlers, "phenix.application.type.interaction-handlers@1", {
    permission: Option<PermissionHandlerRef>,
    elicitation: Option<ElicitationHandlerRef>,
});
record!(SetInteractionHandlersInput, "phenix.application.type.set-interaction-handlers-input@1", {
    handlers: InteractionHandlers,
});
record!(ReviewHunk, "phenix.application.type.review-hunk@1", {
    id: String,
    old_start: u64,
    old_count: u64,
    new_start: u64,
    new_count: u64,
    unified_diff: String,
});
record!(ReviewFile, "phenix.application.type.review-file@1", {
    uri: String,
    expected_version: String,
    hunks: Vec<ReviewHunk>,
    conflict: Option<String>,
});
variants!(ReviewState, "phenix.application.type.review-state@1", {
    Pending,
    Accepted,
    Rejected,
    Conflicted { message: String },
});
record!(ReviewRecord, "phenix.application.type.review-record@1", {
    id: String,
    revision: u64,
    session_id: SessionId,
    execution_id: String,
    files: Vec<ReviewFile>,
    state: ReviewState,
});
variants!(ReviewDecision, "phenix.application.type.review-decision@1", {
    Accept,
    Reject,
});
record!(ReviewDecisionInput, "phenix.application.type.review-decision-input@1", {
    review_id: String,
    expected_revision: u64,
    decision: ReviewDecision,
});

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        CapabilityGenerationId, CapabilityOwnerId, ClientConnectionId, PhenixValue, ReferenceId,
        ValueCodec,
    };

    #[test]
    fn interaction_handlers_encode_exact_callable_contracts() {
        let permission = PermissionHandlerRef(phenix_core::CallableRef::new(
            phenix_core::ContractId::parse("phenix.application.permission@1").unwrap(),
            CapabilityOwnerId::Client(ClientConnectionId::parse("client-1").unwrap()),
            CapabilityGenerationId::parse("generation-1").unwrap(),
            ReferenceId::parse("permission-handler").unwrap(),
        ));
        assert_eq!(
            <PermissionHandlerRef as ValueCodec>::phenix_type(),
            phenix_core::Type::Callable {
                contract: phenix_core::ContractId::parse("phenix.application.permission@1")
                    .unwrap(),
                input: Box::new(PermissionRequest::phenix_schema()),
                output: Box::new(PermissionResponse::phenix_schema()),
            }
        );
        assert_eq!(
            PermissionHandlerRef::from_value(&permission.to_value()).unwrap(),
            permission
        );
    }

    #[test]
    fn review_record_round_trips_without_frontend_patch_state() {
        let record = ReviewRecord {
            id: "review-1".into(),
            revision: 0,
            session_id: SessionId::parse("session-1").unwrap(),
            execution_id: "execution-1".into(),
            files: Vec::new(),
            state: ReviewState::Pending,
        };
        assert_eq!(
            ReviewRecord::from_value(&record.to_value()).unwrap(),
            record
        );
    }
}
