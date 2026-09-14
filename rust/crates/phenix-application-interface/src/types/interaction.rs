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

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ElicitationValidationError {
    #[error("unsupported_schema: {message}")]
    UnsupportedSchema { message: String },
    #[error("invalid elicitation value: {message}")]
    InvalidValue { message: String },
}

/// Validates an elicitation response against the schema supplied by the request.
///
/// `ElicitationResponse::Accepted` deliberately carries `PhenixValue` because its
/// concrete type depends on the sibling request schema. Language bindings may
/// therefore return their ordinary dynamic representation. This function is the
/// application-owned dependent-schema boundary that restores the exact structural
/// value before the response is consumed by runtime code.
pub fn normalize_elicitation_response(
    request: &ElicitationRequest,
    response: ElicitationResponse,
) -> Result<ElicitationResponse, ElicitationValidationError> {
    match response {
        ElicitationResponse::Accepted { value } => {
            normalize_elicitation_value(&request.schema, value)
                .map(|value| ElicitationResponse::Accepted { value })
        }
        ElicitationResponse::Declined => Ok(ElicitationResponse::Declined),
        ElicitationResponse::Cancelled => Ok(ElicitationResponse::Cancelled),
    }
}

fn normalize_elicitation_value(
    schema: &phenix_core::Type,
    value: PhenixValue,
) -> Result<PhenixValue, ElicitationValidationError> {
    if schema.parse(&value).is_ok() {
        return ensure_supported_elicitation_schema(schema).map(|()| value);
    }

    use phenix_core::Type;
    match schema {
        Type::String => invalid_type("string", &value),
        Type::Bool => invalid_type("bool", &value),
        Type::I64 => normalize_i64(value),
        Type::U64 => normalize_u64(value),
        Type::F64 => normalize_f64(value),
        Type::Option(item) if is_scalar_schema(item) => match value {
            PhenixValue::Unit => Ok(PhenixValue::Option(None)),
            PhenixValue::Option(None) => Ok(PhenixValue::Option(None)),
            PhenixValue::Option(Some(value)) => normalize_elicitation_value(item, *value)
                .map(|value| PhenixValue::Option(Some(Box::new(value)))),
            value => normalize_elicitation_value(item, value)
                .map(|value| PhenixValue::Option(Some(Box::new(value)))),
        },
        Type::Option(_) => unsupported("optional elicitation values require a scalar item schema"),
        Type::Table(fields) => normalize_table(fields, value),
        Type::Variant(variants) if variants.values().all(|schema| matches!(schema, Type::Unit)) => {
            normalize_unit_variant(variants, value)
        }
        Type::Variant(_) => unsupported("elicitation variants must contain unit variants only"),
        Type::List(item) if is_scalar_schema(item) || is_unit_variant_schema(item) => {
            let PhenixValue::List(values) = value else {
                return invalid_type("list", &value);
            };
            values
                .into_iter()
                .map(|value| normalize_elicitation_value(item, value))
                .collect::<Result<Vec<_>, _>>()
                .map(PhenixValue::List)
        }
        Type::List(_) => {
            unsupported("elicitation lists require a supported scalar or unit-variant item schema")
        }
        Type::Any => unsupported("elicitation does not accept unconstrained any schemas"),
        Type::Never => unsupported("elicitation does not accept never schemas"),
        Type::Unit => unsupported("unit is supported only as a variant payload"),
        Type::Bytes => unsupported("byte elicitation is not supported"),
        Type::Array { .. } => unsupported("fixed array elicitation is not supported"),
        Type::Map(_) => unsupported("map elicitation is not supported"),
        Type::Callable { .. } => unsupported("callable elicitation is not supported"),
        Type::Object { .. } => unsupported("object elicitation is not supported"),
    }
}

fn ensure_supported_elicitation_schema(
    schema: &phenix_core::Type,
) -> Result<(), ElicitationValidationError> {
    use phenix_core::Type;
    match schema {
        Type::String | Type::Bool | Type::I64 | Type::U64 | Type::F64 => Ok(()),
        Type::Option(item) if is_scalar_schema(item) => Ok(()),
        Type::Option(_) => unsupported("optional elicitation values require a scalar item schema"),
        Type::Table(fields) => fields
            .values()
            .try_for_each(ensure_supported_elicitation_schema),
        Type::Variant(variants) if variants.values().all(|schema| matches!(schema, Type::Unit)) => {
            Ok(())
        }
        Type::Variant(_) => unsupported("elicitation variants must contain unit variants only"),
        Type::List(item) if is_scalar_schema(item) || is_unit_variant_schema(item) => Ok(()),
        Type::List(_) => {
            unsupported("elicitation lists require a supported scalar or unit-variant item schema")
        }
        Type::Any => unsupported("elicitation does not accept unconstrained any schemas"),
        Type::Never => unsupported("elicitation does not accept never schemas"),
        Type::Unit => unsupported("unit is supported only as a variant payload"),
        Type::Bytes => unsupported("byte elicitation is not supported"),
        Type::Array { .. } => unsupported("fixed array elicitation is not supported"),
        Type::Map(_) => unsupported("map elicitation is not supported"),
        Type::Callable { .. } => unsupported("callable elicitation is not supported"),
        Type::Object { .. } => unsupported("object elicitation is not supported"),
    }
}

fn is_scalar_schema(schema: &phenix_core::Type) -> bool {
    matches!(
        schema,
        phenix_core::Type::String
            | phenix_core::Type::Bool
            | phenix_core::Type::I64
            | phenix_core::Type::U64
            | phenix_core::Type::F64
    )
}

fn is_unit_variant_schema(schema: &phenix_core::Type) -> bool {
    matches!(
        schema,
        phenix_core::Type::Variant(variants)
            if variants.values().all(|schema| matches!(schema, phenix_core::Type::Unit))
    )
}

fn normalize_i64(value: PhenixValue) -> Result<PhenixValue, ElicitationValidationError> {
    match value {
        PhenixValue::I64(value) => Ok(PhenixValue::I64(value)),
        PhenixValue::F64(value)
            if value.is_finite()
                && value.fract() == 0.0
                && value >= i64::MIN as f64
                && value <= i64::MAX as f64 =>
        {
            Ok(PhenixValue::I64(value as i64))
        }
        value => invalid_type("i64", &value),
    }
}

fn normalize_u64(value: PhenixValue) -> Result<PhenixValue, ElicitationValidationError> {
    match value {
        PhenixValue::U64(value) => Ok(PhenixValue::U64(value)),
        PhenixValue::I64(value) if value >= 0 => Ok(PhenixValue::U64(value as u64)),
        PhenixValue::F64(value)
            if value.is_finite()
                && value.fract() == 0.0
                && value >= 0.0
                && value <= u64::MAX as f64 =>
        {
            Ok(PhenixValue::U64(value as u64))
        }
        value => invalid_type("u64", &value),
    }
}

fn normalize_f64(value: PhenixValue) -> Result<PhenixValue, ElicitationValidationError> {
    match value {
        PhenixValue::F64(value) if value.is_finite() => Ok(PhenixValue::F64(value)),
        PhenixValue::I64(value) => Ok(PhenixValue::F64(value as f64)),
        PhenixValue::U64(value) => Ok(PhenixValue::F64(value as f64)),
        value => invalid_type("f64", &value),
    }
}

fn normalize_table(
    fields: &std::collections::BTreeMap<phenix_core::Key, phenix_core::Type>,
    value: PhenixValue,
) -> Result<PhenixValue, ElicitationValidationError> {
    let dynamic = match value {
        PhenixValue::Map(values) => values,
        PhenixValue::Table(values) => values
            .into_iter()
            .map(|(key, value)| (key.as_str().to_owned(), value))
            .collect(),
        value => return invalid_type("table", &value),
    };
    if let Some(key) = dynamic
        .keys()
        .find(|key| !fields.contains_key(key.as_str()))
    {
        return Err(ElicitationValidationError::InvalidValue {
            message: format!("unexpected table field {key}"),
        });
    }

    let mut normalized = std::collections::BTreeMap::new();
    for (key, field_schema) in fields {
        let value = match dynamic.get(key.as_str()) {
            Some(value) => normalize_elicitation_value(field_schema, value.clone())?,
            None if matches!(field_schema, phenix_core::Type::Option(_)) => {
                normalize_elicitation_value(field_schema, PhenixValue::Unit)?
            }
            None => {
                return Err(ElicitationValidationError::InvalidValue {
                    message: format!("missing table field {key}"),
                })
            }
        };
        normalized.insert(key.clone(), value);
    }
    Ok(PhenixValue::Table(normalized))
}

fn normalize_unit_variant(
    variants: &std::collections::BTreeMap<phenix_core::Key, phenix_core::Type>,
    value: PhenixValue,
) -> Result<PhenixValue, ElicitationValidationError> {
    match value {
        PhenixValue::Variant { tag, value }
            if variants.contains_key(&tag) && matches!(value.as_ref(), PhenixValue::Unit) =>
        {
            Ok(PhenixValue::Variant { tag, value })
        }
        PhenixValue::Map(mut values) => {
            let Some(PhenixValue::String(kind)) = values.remove("kind") else {
                return Err(ElicitationValidationError::InvalidValue {
                    message: "unit variant requires string field kind".to_owned(),
                });
            };
            if !values.is_empty() {
                return Err(ElicitationValidationError::InvalidValue {
                    message: "unit variant does not accept payload fields".to_owned(),
                });
            }
            let tag = phenix_core::Key::parse(kind).map_err(|message| {
                ElicitationValidationError::InvalidValue {
                    message: message.to_owned(),
                }
            })?;
            if !variants.contains_key(&tag) {
                return Err(ElicitationValidationError::InvalidValue {
                    message: format!("unknown elicitation variant {tag}"),
                });
            }
            Ok(PhenixValue::Variant {
                tag,
                value: Box::new(PhenixValue::Unit),
            })
        }
        value => invalid_type("unit variant", &value),
    }
}

fn unsupported<T>(message: impl Into<String>) -> Result<T, ElicitationValidationError> {
    Err(ElicitationValidationError::UnsupportedSchema {
        message: message.into(),
    })
}

fn invalid_type<T>(expected: &str, value: &PhenixValue) -> Result<T, ElicitationValidationError> {
    Err(ElicitationValidationError::InvalidValue {
        message: format!("expected {expected}, got {}", value.kind()),
    })
}

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
        CapabilityGenerationId, CapabilityOwnerId, ClientConnectionId, HasPhenixSchema, Key,
        ReferenceId, Type, ValueCodec,
    };
    use std::collections::BTreeMap;

    fn elicitation(schema: Type) -> ElicitationRequest {
        ElicitationRequest {
            session_id: SessionId::parse("session-1").unwrap(),
            message: "fixture".into(),
            schema,
        }
    }

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
    fn elicitation_normalizes_dynamic_lua_shapes_to_the_original_schema() {
        let schema = Type::Table(BTreeMap::from([
            (Key::parse("count").unwrap(), Type::U64),
            (
                Key::parse("label").unwrap(),
                Type::Option(Box::new(Type::String)),
            ),
            (
                Key::parse("mode").unwrap(),
                Type::Variant(BTreeMap::from([
                    (Key::parse("fast").unwrap(), Type::Unit),
                    (Key::parse("safe").unwrap(), Type::Unit),
                ])),
            ),
        ]));
        let request = elicitation(schema.clone());
        let dynamic = PhenixValue::Map(BTreeMap::from([
            ("count".into(), PhenixValue::I64(7)),
            (
                "mode".into(),
                PhenixValue::Map(BTreeMap::from([(
                    "kind".into(),
                    PhenixValue::String("safe".into()),
                )])),
            ),
        ]));
        let normalized = normalize_elicitation_response(
            &request,
            ElicitationResponse::Accepted { value: dynamic },
        )
        .unwrap();
        let ElicitationResponse::Accepted { value } = normalized else {
            panic!("accepted response remains accepted");
        };
        schema.parse(&value).unwrap();
        let PhenixValue::Table(fields) = value else {
            panic!("record must normalize to a structural table");
        };
        assert_eq!(fields["count"], PhenixValue::U64(7));
        assert_eq!(fields["label"], PhenixValue::Option(None));
        assert!(matches!(
            &fields["mode"],
            PhenixValue::Variant { tag, value }
                if tag.as_str() == "safe" && matches!(value.as_ref(), PhenixValue::Unit)
        ));
    }

    #[test]
    fn elicitation_rejects_unsupported_compound_schemas_explicitly() {
        let request = elicitation(Type::Map(Box::new(Type::String)));
        let error = normalize_elicitation_response(
            &request,
            ElicitationResponse::Accepted {
                value: PhenixValue::Map(BTreeMap::new()),
            },
        )
        .unwrap_err();
        assert!(matches!(
            error,
            ElicitationValidationError::UnsupportedSchema { .. }
        ));
        assert!(error.to_string().starts_with("unsupported_schema:"));
    }

    #[test]
    fn elicitation_rejects_invalid_values_without_weakening_the_schema() {
        let request = elicitation(Type::U64);
        let error = normalize_elicitation_response(
            &request,
            ElicitationResponse::Accepted {
                value: PhenixValue::I64(-1),
            },
        )
        .unwrap_err();
        assert!(matches!(
            error,
            ElicitationValidationError::InvalidValue { .. }
        ));
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
