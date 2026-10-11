//! Host-control messages request attenuation, never transfer a host capability.
use crate::{identifier, operation_key, Code, Failure, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub const SCOPED_IMPORT: &str = "phenix.host.scoped.v1";
pub const RESOURCE_CONTEXT: &str = "phenix.host.resource.v1";
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    pub operations: BTreeSet<String>,
    pub resources: BTreeSet<String>,
}
impl Authority {
    pub fn valid(&self) -> bool {
        self.operations.iter().all(|key| operation_key(key))
            && self.resources.iter().all(|id| identifier(id))
    }
    pub fn intersect(&self, other: &Self) -> Self {
        Self {
            operations: self
                .operations
                .intersection(&other.operations)
                .cloned()
                .collect(),
            resources: self
                .resources
                .intersection(&other.resources)
                .cloned()
                .collect(),
        }
    }
    pub fn to_value(&self) -> Value {
        Value::map([
            (
                "operations",
                Value::List(self.operations.iter().cloned().map(Value::String).collect()),
            ),
            (
                "resources",
                Value::List(self.resources.iter().cloned().map(Value::String).collect()),
            ),
        ])
    }
    pub fn from_value(value: Value) -> Result<Self, Failure> {
        let Value::Map(mut fields) = value else {
            return Err(invalid());
        };
        fn strings(value: Option<Value>) -> Result<BTreeSet<String>, Failure> {
            let Some(Value::List(values)) = value else {
                return Err(invalid());
            };
            let mut result = BTreeSet::new();
            let mut previous = String::new();
            for value in values {
                let Value::String(text) = value else {
                    return Err(invalid());
                };
                if text <= previous {
                    return Err(invalid());
                }
                previous = text.clone();
                result.insert(text);
            }
            Ok(result)
        }
        let result = Self {
            operations: strings(fields.remove("operations"))?,
            resources: strings(fields.remove("resources"))?,
        };
        if !fields.is_empty() || !result.valid() {
            return Err(invalid());
        }
        Ok(result)
    }
}
#[derive(Debug, Clone)]
pub struct ScopedRequest {
    pub authority: Authority,
    pub resource: Option<String>,
}
impl ScopedRequest {
    pub fn encode(&self, operation: &str, request: Vec<u8>) -> Result<Vec<u8>, Failure> {
        if !operation_key(operation)
            || !self.authority.valid()
            || self.resource.as_ref().is_some_and(|id| !identifier(id))
        {
            return Err(invalid());
        }
        Value::map([
            ("authority", self.authority.to_value()),
            ("operation", Value::String(operation.into())),
            ("request", Value::Bytes(request)),
            (
                "resource",
                self.resource
                    .as_ref()
                    .map_or(Value::Null, |id| Value::String(id.clone())),
            ),
        ])
        .encode()
    }
    pub fn decode(bytes: &[u8]) -> Result<(String, Vec<u8>, Self), Failure> {
        let Value::Map(mut fields) = Value::decode(bytes)? else {
            return Err(invalid());
        };
        let authority = Authority::from_value(fields.remove("authority").ok_or_else(invalid)?)?;
        let Some(Value::String(operation)) = fields.remove("operation") else {
            return Err(invalid());
        };
        let Some(Value::Bytes(request)) = fields.remove("request") else {
            return Err(invalid());
        };
        let resource = match fields.remove("resource") {
            Some(Value::Null) => None,
            Some(Value::String(id)) if identifier(&id) => Some(id),
            _ => return Err(invalid()),
        };
        if !fields.is_empty() || !operation_key(&operation) {
            return Err(invalid());
        }
        Ok((
            operation,
            request,
            Self {
                authority,
                resource,
            },
        ))
    }
}
fn invalid() -> Failure {
    Failure::new(Code::SchemaRejected, "invalid host scope request")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attenuation_never_adds_authority() {
        let parent = Authority {
            operations: ["test.one@1/run".into()].into(),
            resources: ["red".into()].into(),
        };
        let child = Authority {
            operations: ["test.one@1/run".into(), "test.two@1/run".into()].into(),
            resources: ["red".into(), "blue".into()].into(),
        };
        assert_eq!(parent.intersect(&child), parent);
        assert!(Authority::default().intersect(&child).resources.is_empty());
        let request = ScopedRequest {
            authority: child,
            resource: Some("red".into()),
        };
        let (_, bytes, decoded) =
            ScopedRequest::decode(&request.encode("test.one@1/run", vec![0]).unwrap()).unwrap();
        assert_eq!(bytes, vec![0]);
        assert_eq!(decoded.authority, request.authority);
    }
    #[test]
    fn scope_wire_rejects_duplicate_and_unknown_fields() {
        let mut value = Authority::default().to_value();
        let Value::Map(ref mut fields) = value else {
            unreachable!()
        };
        fields.insert(
            "resources".into(),
            Value::List(vec![
                Value::String("red".into()),
                Value::String("red".into()),
            ]),
        );
        assert!(Authority::from_value(value).is_err());
        let mut value = Authority::default().to_value();
        let Value::Map(ref mut fields) = value else {
            unreachable!()
        };
        fields.insert("grant".into(), Value::Bool(true));
        assert!(Authority::from_value(value).is_err());
    }
}
