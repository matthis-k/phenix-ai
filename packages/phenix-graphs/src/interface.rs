//! Canonical structural interfaces and boundary diagnostics.
use crate::{ContractValue, TypeIdentity};
use std::any::TypeId;
use std::collections::BTreeMap;
use std::fmt;

/// One named port, independent of direction. Portable identity includes the
/// complete schema; local identity is a Rust type, never a diagnostic label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortDef {
    pub name: String,
    pub value: TypeIdentity,
}
impl PortDef {
    pub fn local<T: 'static>(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: TypeIdentity::Local(TypeId::of::<T>()),
        }
    }
    pub fn portable(
        name: impl Into<String>,
        schema: &crate::Schema,
    ) -> Result<Self, InterfaceError> {
        for (field, text) in [
            (PortableSchemaField::Name, schema.name.as_str()),
            (PortableSchemaField::Structure, schema.structure.as_str()),
            (PortableSchemaField::Codec, schema.codec.as_str()),
        ] {
            if text.trim().is_empty() || text.chars().any(char::is_control) {
                return Err(InterfaceError::new(
                    InterfaceReason::InvalidPortableSchema {
                        field,
                        value: text.into(),
                    },
                ));
            }
        }
        let port = Self {
            name: name.into(),
            value: TypeIdentity::Wire(schema.identity()),
        };
        validate_port_name(&port.name)?;
        Ok(port)
    }
    pub fn wire<T: ContractValue>(name: impl Into<String>) -> Result<Self, InterfaceError> {
        Self::portable(name, &T::schema())
    }
}

/// Immutable, symmetric boundary with deterministic unique names per direction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Interface {
    inputs: BTreeMap<String, PortDef>,
    outputs: BTreeMap<String, PortDef>,
}
impl Interface {
    pub fn new(
        inputs: impl IntoIterator<Item = PortDef>,
        outputs: impl IntoIterator<Item = PortDef>,
    ) -> Result<Self, InterfaceError> {
        fn collect(
            ports: impl IntoIterator<Item = PortDef>,
        ) -> Result<BTreeMap<String, PortDef>, InterfaceError> {
            let mut result = BTreeMap::new();
            for port in ports {
                validate_port_name(&port.name)?;
                let name = port.name.clone();
                if result.insert(name.clone(), port).is_some() {
                    return Err(InterfaceError::new(InterfaceReason::DuplicatePort {
                        name: name.into(),
                    }));
                }
            }
            Ok(result)
        }
        Ok(Self {
            inputs: collect(inputs)?,
            outputs: collect(outputs)?,
        })
    }
    pub fn inputs(&self) -> &BTreeMap<String, PortDef> {
        &self.inputs
    }
    pub fn outputs(&self) -> &BTreeMap<String, PortDef> {
        &self.outputs
    }
    pub fn input(&self, name: &str) -> Option<&PortDef> {
        self.inputs.get(name)
    }
    pub fn output(&self, name: &str) -> Option<&PortDef> {
        self.outputs.get(name)
    }
    /// Requires exactly the same named inputs, outputs, and type identities.
    pub fn check_shape(&self, actual: &Self) -> Result<(), InterfaceError> {
        for (direction, required, provided, opposite) in [
            ("input", &self.inputs, &actual.inputs, &actual.outputs),
            ("output", &self.outputs, &actual.outputs, &actual.inputs),
        ] {
            for (name, port) in required {
                let found = provided.get(name);
                if found == Some(port) {
                    continue;
                }
                let mut error = InterfaceError::new(if found.is_some() {
                    InterfaceReason::IncompatibleType
                } else if opposite.contains_key(name) {
                    InterfaceReason::WrongDirection
                } else {
                    InterfaceReason::MissingPort
                });
                error.endpoint = Some(format!("{direction} {name}").into());
                error.expected = Some(port.value.clone());
                error.actual = found
                    .or_else(|| opposite.get(name))
                    .map(|p| p.value.clone());
                return Err(error);
            }
        }
        for (direction, required, provided) in [
            ("input", &self.inputs, &actual.inputs),
            ("output", &self.outputs, &actual.outputs),
        ] {
            for (name, port) in provided {
                if !required.contains_key(name) {
                    let mut error = InterfaceError::new(InterfaceReason::ExtraPort);
                    error.endpoint = Some(format!("{direction} {name}").into());
                    error.actual = Some(port.value.clone());
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}

fn validate_port_name(name: &str) -> Result<(), InterfaceError> {
    if name.is_empty()
        || name.len() > crate::MAX_IDENTIFIER_BYTES
        || name.chars().any(char::is_control)
    {
        Err(InterfaceError::new(InterfaceReason::InvalidPortName {
            name: name.into(),
        }))
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterfacePhase {
    Definition,
    Authoring,
    Compiled,
}

/// The portable schema fields validated when defining a port. Revision is data,
/// but is not subject to these text-field checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortableSchemaField {
    Name,
    Structure,
    Codec,
}
impl fmt::Display for PortableSchemaField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Name => "name",
            Self::Structure => "structure",
            Self::Codec => "codec",
        })
    }
}

/// An owning diagnostic reason. Shape failures use `InterfaceError`'s endpoint
/// and expected/actual identities rather than duplicating that context here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InterfaceReason {
    InvalidPortableSchema {
        field: PortableSchemaField,
        value: Box<str>,
    },
    DuplicatePort {
        name: Box<str>,
    },
    IncompatibleType,
    WrongDirection,
    MissingPort,
    ExtraPort,
    InvalidPortName {
        name: Box<str>,
    },
    InvalidContractIdentity {
        id: Box<str>,
    },
    DifferingContractDefinition {
        expected: crate::contract::Contract,
        actual: crate::contract::Contract,
    },
    MissingExplicitClaim {
        contract: Box<str>,
    },
}
impl fmt::Display for InterfaceReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPortableSchema { field, .. } => {
                write!(f, "invalid portable schema {field}")
            }
            Self::DuplicatePort { name } => write!(f, "duplicate port name {name:?}"),
            Self::IncompatibleType => f.write_str("incompatible port type"),
            Self::WrongDirection => f.write_str("port has wrong direction"),
            Self::MissingPort => f.write_str("missing port"),
            Self::ExtraPort => f.write_str("extra port"),
            Self::InvalidPortName { .. } => {
                f.write_str("port name must be nonempty, bounded and contain no control characters")
            }
            Self::InvalidContractIdentity { .. } => f.write_str("invalid contract identity"),
            Self::DifferingContractDefinition { .. } => {
                f.write_str("different contract identity or definition")
            }
            Self::MissingExplicitClaim { .. } => f.write_str("missing explicit contract claim"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterfaceError {
    pub contract: Option<Box<str>>,
    pub path: Box<str>,
    pub phase: InterfacePhase,
    pub endpoint: Option<Box<str>>,
    pub expected: Option<TypeIdentity>,
    pub actual: Option<TypeIdentity>,
    pub reason: Box<InterfaceReason>,
}
impl InterfaceError {
    pub(crate) fn new(reason: InterfaceReason) -> Self {
        Self {
            contract: None,
            path: "".into(),
            phase: InterfacePhase::Definition,
            endpoint: None,
            expected: None,
            actual: None,
            reason: Box::new(reason),
        }
    }
    pub(crate) fn context(
        mut self,
        phase: InterfacePhase,
        path: &str,
        contract: Option<&str>,
    ) -> Self {
        self.phase = phase;
        self.path = path.into();
        self.contract = contract.map(Box::<str>::from);
        self
    }
}
impl fmt::Display for InterfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} interface at {:?}", self.phase, self.path)?;
        if let Some(id) = &self.contract {
            write!(f, " contract {id:?}")?;
        }
        if let Some(endpoint) = &self.endpoint {
            write!(f, " endpoint {endpoint}")?;
        }
        write!(f, ": {}", self.reason)?;
        if self.expected.is_some() || self.actual.is_some() {
            write!(
                f,
                " (expected {:?}, actual {:?})",
                self.expected, self.actual
            )?;
        }
        Ok(())
    }
}
impl std::error::Error for InterfaceError {}
