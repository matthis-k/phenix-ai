//! Portable import/export surfaces for independently compiled Rust plugins.
//!
//! This is composition metadata only. It grants no authority, carries no Rust
//! object pointers, and does not implement a second graph compiler.
use crate::{identifier, Code, Failure, Portable, StaticSchema, Value, ValueSchema};
use std::collections::BTreeMap;

fn invalid(message: impl Into<String>) -> Failure {
    Failure::new(Code::SchemaRejected, message)
}

/// A public graph port. The same name can appear once for each direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PortDirection {
    Input,
    Output,
}
impl PortDirection {
    fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
    fn parse(text: &str) -> Result<Self, Failure> {
        match text {
            "input" => Ok(Self::Input),
            "output" => Ok(Self::Output),
            _ => Err(invalid("invalid public port direction")),
        }
    }
}

/// Explicit semantic interface plus the existing graph module's public ports.
/// No graph node types, ABI handles, or plugin identities are stored here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterfaceSurface {
    id: String,
    ports: BTreeMap<(PortDirection, String), ValueSchema>,
}

fn valid_interface(id: &str) -> bool {
    let Some((name, major)) = id.rsplit_once('@') else {
        return false;
    };
    identifier(name)
        && major
            .parse::<u32>()
            .is_ok_and(|version| version > 0 && major == version.to_string())
}

impl InterfaceSurface {
    pub fn new(id: impl Into<String>) -> Result<Self, Failure> {
        let id = id.into();
        if !valid_interface(&id) {
            return Err(invalid(format!("invalid interface identity {id:?}")));
        }
        Ok(Self {
            id,
            ports: BTreeMap::new(),
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Ordinary Rust API crates may be reused, but are not mandatory. A
    /// plugin's own T determines its canonical wire schema.
    pub fn input<T: Portable>(self, name: &str) -> Result<Self, Failure> {
        self.port(name, PortDirection::Input, T::SCHEMA)
    }
    pub fn output<T: Portable>(self, name: &str) -> Result<Self, Failure> {
        self.port(name, PortDirection::Output, T::SCHEMA)
    }
    pub fn port(
        mut self,
        name: &str,
        direction: PortDirection,
        schema: StaticSchema,
    ) -> Result<Self, Failure> {
        if !identifier(name) {
            return Err(invalid(format!("invalid public port name {name:?}")));
        }
        let schema = schema.owned()?;
        let key = (direction, name.to_owned());
        if self.ports.insert(key.clone(), schema).is_some() {
            return Err(invalid(format!(
                "duplicate public {} {name}",
                direction.as_str()
            )));
        }
        Ok(self)
    }

    pub fn ports(&self) -> impl Iterator<Item = (PortDirection, &str, &ValueSchema)> {
        self.ports
            .iter()
            .map(|((direction, name), schema)| (*direction, name.as_str(), schema))
    }

    /// The normal serialized value representation is reused for offline Nix
    /// checks. This encoding is canonical and independent of Rust struct names
    /// or declaration order.
    pub fn encode(&self) -> Result<Vec<u8>, Failure> {
        let ports = self
            .ports()
            .map(|(direction, name, schema)| {
                Ok(Value::map([
                    ("direction", Value::String(direction.as_str().into())),
                    ("name", Value::String(name.into())),
                    ("schema", Value::Bytes(schema.encode()?)),
                ]))
            })
            .collect::<Result<Vec<_>, Failure>>()?;
        Value::map([
            ("interface", Value::String(self.id.clone())),
            ("ports", Value::List(ports)),
        ])
        .encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        let Value::Map(mut record) = Value::decode(bytes)? else {
            return Err(invalid("interface must be a record"));
        };
        let Some(Value::String(id)) = record.remove("interface") else {
            return Err(invalid("missing interface identity"));
        };
        let Some(Value::List(ports)) = record.remove("ports") else {
            return Err(invalid("missing interface ports"));
        };
        if !record.is_empty() {
            return Err(invalid("unknown interface attributes"));
        }
        let mut surface = Self::new(id)?;
        for item in ports {
            let Value::Map(mut fields) = item else {
                return Err(invalid("invalid public port"));
            };
            let Some(Value::String(name)) = fields.remove("name") else {
                return Err(invalid("missing public port name"));
            };
            let Some(Value::String(direction)) = fields.remove("direction") else {
                return Err(invalid("missing public port direction"));
            };
            let Some(Value::Bytes(schema)) = fields.remove("schema") else {
                return Err(invalid("missing public port schema"));
            };
            if !fields.is_empty() || !identifier(&name) {
                return Err(invalid("invalid public port properties"));
            }
            let key = (PortDirection::parse(&direction)?, name);
            let schema = ValueSchema::decode(&schema)?;
            if surface.ports.insert(key.clone(), schema).is_some() {
                return Err(invalid(format!(
                    "duplicate public {} {}",
                    key.0.as_str(),
                    key.1
                )));
            }
        }
        Ok(surface)
    }

    /// Exact interface matching is intentionally conservative in this slice:
    /// structural subtyping and custom Serde codecs require separate proofs.
    pub fn check_implementation(&self, implementation: &Self) -> Result<(), Failure> {
        if self.id != implementation.id {
            return Err(invalid(format!(
                "interface mismatch: required {}, provided {}",
                self.id, implementation.id
            )));
        }
        for (key, expected) in &self.ports {
            match implementation.ports.get(key) {
                None => {
                    return Err(invalid(format!(
                        "interface {}: missing {} {}",
                        self.id,
                        key.0.as_str(),
                        key.1
                    )))
                }
                Some(actual) if actual != expected => {
                    return Err(invalid(format!(
                        "interface {}: incompatible {} {}",
                        self.id,
                        key.0.as_str(),
                        key.1
                    )))
                }
                Some(_) => {}
            }
        }
        for (direction, name) in implementation.ports.keys() {
            if !self.ports.contains_key(&(*direction, name.clone())) {
                return Err(invalid(format!(
                    "interface {}: unexpected {} {}",
                    self.id,
                    direction.as_str(),
                    name
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StaticField;

    // Independently defined Rust types with different field declaration order.
    struct ProviderRequest {
        prompt: String,
        tokens: u64,
    }
    struct ConsumerRequest {
        tokens: u64,
        prompt: String,
    }
    const REQUEST: StaticSchema = StaticSchema::Struct(&[
        StaticField {
            name: "prompt",
            schema: StaticSchema::String,
        },
        StaticField {
            name: "tokens",
            schema: StaticSchema::U64,
        },
    ]);
    impl Portable for ProviderRequest {
        const SCHEMA: StaticSchema = REQUEST;
        fn to_value(&self) -> Value {
            Value::map([
                ("prompt", Value::String(self.prompt.clone())),
                ("tokens", Value::U64(self.tokens)),
            ])
        }
        fn from_value(value: Value) -> Result<Self, Failure> {
            let Value::Map(mut fields) = value else {
                return Err(invalid("not record"));
            };
            let (Some(Value::String(prompt)), Some(Value::U64(tokens))) =
                (fields.remove("prompt"), fields.remove("tokens"))
            else {
                return Err(invalid("not a request"));
            };
            if !fields.is_empty() {
                return Err(invalid("extra fields"));
            }
            Ok(Self { prompt, tokens })
        }
    }
    impl Portable for ConsumerRequest {
        const SCHEMA: StaticSchema = REQUEST;
        fn to_value(&self) -> Value {
            Value::map([
                ("tokens", Value::U64(self.tokens)),
                ("prompt", Value::String(self.prompt.clone())),
            ])
        }
        fn from_value(value: Value) -> Result<Self, Failure> {
            let Value::Map(mut fields) = value else {
                return Err(invalid("not record"));
            };
            let (Some(Value::U64(tokens)), Some(Value::String(prompt))) =
                (fields.remove("tokens"), fields.remove("prompt"))
            else {
                return Err(invalid("not a request"));
            };
            if !fields.is_empty() {
                return Err(invalid("extra fields"));
            }
            Ok(Self { tokens, prompt })
        }
    }

    #[test]
    fn independently_declared_types_match_and_roundtrip() {
        let a = InterfaceSurface::new("model.generate@1")
            .unwrap()
            .input::<ProviderRequest>("request")
            .unwrap()
            .output::<String>("reply")
            .unwrap();
        let b = InterfaceSurface::new("model.generate@1")
            .unwrap()
            .input::<ConsumerRequest>("request")
            .unwrap()
            .output::<String>("reply")
            .unwrap();
        a.check_implementation(&b).unwrap();
        assert_eq!(a.encode().unwrap(), b.encode().unwrap());
        assert_eq!(InterfaceSurface::decode(&a.encode().unwrap()).unwrap(), a);
        let encoded = ProviderRequest {
            prompt: "hello".into(),
            tokens: 128,
        }
        .to_value()
        .encode()
        .unwrap();
        let other = ConsumerRequest::from_value(Value::decode(&encoded).unwrap()).unwrap();
        assert_eq!(other.prompt, "hello");
        assert_eq!(other.tokens, 128);
    }

    #[test]
    fn identity_and_shapes_both_matter() {
        let a = InterfaceSurface::new("model.generate@1")
            .unwrap()
            .input::<ProviderRequest>("request")
            .unwrap()
            .output::<String>("reply")
            .unwrap();
        let changed_identity = InterfaceSurface::new("other.generate@1")
            .unwrap()
            .input::<ConsumerRequest>("request")
            .unwrap()
            .output::<String>("reply")
            .unwrap();
        assert!(a.check_implementation(&changed_identity).is_err());
        let changed_type = InterfaceSurface::new("model.generate@1")
            .unwrap()
            .input::<u64>("request")
            .unwrap()
            .output::<String>("reply")
            .unwrap();
        assert!(a.check_implementation(&changed_type).is_err());
        let missing = InterfaceSurface::new("model.generate@1")
            .unwrap()
            .output::<String>("reply")
            .unwrap();
        assert!(a.check_implementation(&missing).is_err());
        assert!(InterfaceSurface::new("model.generate@0").is_err());
    }

    #[test]
    fn port_names_directions_and_duplicate_bindings_are_checked() {
        let base = InterfaceSurface::new("model.generate@1")
            .unwrap()
            .input::<u64>("request")
            .unwrap();
        assert!(base.clone().input::<u64>("request").is_err());
        let wrong = InterfaceSurface::new("model.generate@1")
            .unwrap()
            .output::<u64>("request")
            .unwrap();
        assert!(base.check_implementation(&wrong).is_err());
        assert!(InterfaceSurface::new("model.generate@1")
            .unwrap()
            .input::<u64>("")
            .is_err());
    }

    #[test]
    fn decode_rejects_malformed_schema_and_unknown_fields() {
        let malformed = Value::map([
            ("interface", Value::String("model.generate@1".into())),
            (
                "ports",
                Value::List(vec![Value::map([
                    ("direction", Value::String("input".into())),
                    ("name", Value::String("request".into())),
                    ("schema", Value::Bytes(vec![0, 1, 2])),
                ])]),
            ),
        ])
        .encode()
        .unwrap();
        assert!(InterfaceSurface::decode(&malformed).is_err());
    }
}
