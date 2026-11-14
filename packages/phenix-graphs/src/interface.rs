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
            ("name", schema.name.as_str()),
            ("structure", schema.structure.as_str()),
            ("codec", schema.codec.as_str()),
        ] {
            if text.trim().is_empty() || text.chars().any(char::is_control) {
                return Err(InterfaceError::new(format!(
                    "invalid portable schema {field}"
                )));
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
                    return Err(InterfaceError::new(format!("duplicate port name {name:?}")));
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
                    "incompatible port type"
                } else if opposite.contains_key(name) {
                    "port has wrong direction"
                } else {
                    "missing port"
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
                    let mut error = InterfaceError::new("extra port");
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
        Err(InterfaceError::new(
            "port name must be nonempty, bounded and contain no control characters",
        ))
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterfaceError {
    pub contract: Option<Box<str>>,
    pub path: Box<str>,
    pub phase: InterfacePhase,
    pub endpoint: Option<Box<str>>,
    pub expected: Option<TypeIdentity>,
    pub actual: Option<TypeIdentity>,
    pub message: Box<str>,
}
impl InterfaceError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            contract: None,
            path: "".into(),
            phase: InterfacePhase::Definition,
            endpoint: None,
            expected: None,
            actual: None,
            message: message.into().into_boxed_str(),
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
        write!(f, ": {}", self.message)?;
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
