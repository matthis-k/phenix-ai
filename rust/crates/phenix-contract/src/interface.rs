use crate::{HasPhenixSchema, InterfaceId, PhenixSchema, SchemaCompatibility, SchemaMismatch};
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InterfaceSchema {
    request: PhenixSchema,
    response: PhenixSchema,
    #[serde(default = "never_schema", skip_serializing_if = "is_never_schema")]
    error: PhenixSchema,
}

impl InterfaceSchema {
    pub fn new(request: PhenixSchema, response: PhenixSchema) -> Self {
        Self {
            request,
            response,
            error: PhenixSchema::Never,
        }
    }

    pub fn of<Request: HasPhenixSchema, Response: HasPhenixSchema>() -> Self {
        Self::new(Request::phenix_schema(), Response::phenix_schema())
    }

    pub fn fallible_of<
        Request: HasPhenixSchema,
        Response: HasPhenixSchema,
        DomainError: HasPhenixSchema,
    >() -> Self {
        Self {
            request: Request::phenix_schema(),
            response: Response::phenix_schema(),
            error: DomainError::phenix_schema(),
        }
    }

    pub fn request(&self) -> &PhenixSchema {
        &self.request
    }

    pub fn response(&self) -> &PhenixSchema {
        &self.response
    }

    pub fn error(&self) -> &PhenixSchema {
        &self.error
    }

    pub fn accepts_provider(&self, provider: &Self) -> InterfaceCompatibility {
        let request = provider.request.accepts(&self.request);
        let response = self.response.accepts(&provider.response);
        let error = self.error.accepts(&provider.error);
        let request_exact = matches!(request, SchemaCompatibility::Exact);
        let response_exact = matches!(response, SchemaCompatibility::Exact);
        let error_exact = matches!(error, SchemaCompatibility::Exact);
        let request_mismatch = match request {
            SchemaCompatibility::Incompatible(error) => Some(error),
            SchemaCompatibility::Exact | SchemaCompatibility::Compatible => None,
        };
        let response_mismatch = match response {
            SchemaCompatibility::Incompatible(error) => Some(error),
            SchemaCompatibility::Exact | SchemaCompatibility::Compatible => None,
        };
        let error_mismatch = match error {
            SchemaCompatibility::Incompatible(error) => Some(error),
            SchemaCompatibility::Exact | SchemaCompatibility::Compatible => None,
        };
        if request_mismatch.is_some() || response_mismatch.is_some() || error_mismatch.is_some() {
            return InterfaceCompatibility::Incompatible(InterfaceSchemaMismatch {
                request: request_mismatch,
                response: response_mismatch,
                error: error_mismatch,
            });
        }
        if request_exact && response_exact && error_exact {
            InterfaceCompatibility::Exact
        } else {
            InterfaceCompatibility::Compatible
        }
    }
}

fn never_schema() -> PhenixSchema {
    PhenixSchema::Never
}

fn is_never_schema(schema: &PhenixSchema) -> bool {
    schema == &PhenixSchema::Never
}

impl Default for InterfaceSchema {
    fn default() -> Self {
        Self::new(PhenixSchema::Any, PhenixSchema::Any)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InterfaceCompatibility {
    Exact,
    Compatible,
    Incompatible(InterfaceSchemaMismatch),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterfaceSchemaMismatch {
    request: Option<SchemaMismatch>,
    response: Option<SchemaMismatch>,
    error: Option<SchemaMismatch>,
}

impl InterfaceSchemaMismatch {
    pub fn request(&self) -> Option<&SchemaMismatch> {
        self.request.as_ref()
    }

    pub fn response(&self) -> Option<&SchemaMismatch> {
        self.response.as_ref()
    }

    pub fn error(&self) -> Option<&SchemaMismatch> {
        self.error.as_ref()
    }
}

impl Display for InterfaceSchemaMismatch {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut separator = "";
        for (label, mismatch) in [
            ("request", &self.request),
            ("response", &self.response),
            ("error", &self.error),
        ] {
            if let Some(mismatch) = mismatch {
                write!(f, "{separator}{label} mismatch: {mismatch}")?;
                separator = "; ";
            }
        }
        if separator.is_empty() {
            f.write_str("interface schemas are compatible")?;
        }
        Ok(())
    }
}

impl Error for InterfaceSchemaMismatch {}

pub trait ComponentInterface {
    fn interface_id() -> InterfaceId;

    fn schema() -> InterfaceSchema {
        InterfaceSchema::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Key;
    use std::collections::BTreeMap;

    fn key(value: &str) -> Key {
        Key::parse(value.to_owned()).unwrap()
    }

    fn variants(values: &[&str]) -> PhenixSchema {
        PhenixSchema::Variant(
            values
                .iter()
                .map(|value| (key(value), PhenixSchema::String))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    #[test]
    fn interface_error_schema_is_directional_and_defaults_to_never() {
        let infallible = InterfaceSchema::new(PhenixSchema::Unit, PhenixSchema::Unit);
        assert_eq!(infallible.error(), &PhenixSchema::Never);

        let consumer = InterfaceSchema {
            request: PhenixSchema::Unit,
            response: PhenixSchema::Unit,
            error: variants(&["conflict", "denied"]),
        };
        let provider = InterfaceSchema {
            request: PhenixSchema::Unit,
            response: PhenixSchema::Unit,
            error: variants(&["conflict"]),
        };
        assert!(matches!(
            consumer.accepts_provider(&provider),
            InterfaceCompatibility::Compatible
        ));

        let incompatible_provider = InterfaceSchema {
            error: variants(&["conflict", "disconnected"]),
            ..provider
        };
        let InterfaceCompatibility::Incompatible(mismatch) =
            consumer.accepts_provider(&incompatible_provider)
        else {
            panic!("provider error schema must be rejected");
        };
        assert!(mismatch.request().is_none());
        assert!(mismatch.response().is_none());
        assert!(mismatch.error().is_some());
    }

    #[test]
    fn omitted_error_schema_decodes_as_infallible() {
        let schema: InterfaceSchema = serde_json::from_value(serde_json::json!({
            "request": { "type": "unit" },
            "response": { "type": "unit" }
        }))
        .unwrap();
        assert_eq!(schema.error(), &PhenixSchema::Never);
        assert_eq!(
            serde_json::to_value(schema).unwrap(),
            serde_json::json!({
                "request": { "type": "unit" },
                "response": { "type": "unit" }
            })
        );
    }
}
