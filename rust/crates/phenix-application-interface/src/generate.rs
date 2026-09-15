//! First Rust emitter. Input is a deserialized descriptor, never Rust source or plugin code.
use crate::ApplicationDescriptor;
use phenix_core::{ContractId, PhenixSchema};
use std::{
    collections::BTreeSet,
    fmt::{self, Write as _},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GenerationError {
    InvalidIdentifier(String),
    DuplicateIdentifier(String),
    MissingReference(ContractId),
    UnsupportedSchema(PhenixSchema),
}

impl fmt::Display for GenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot generate application API: {self:?}")
    }
}
impl std::error::Error for GenerationError {}

/// Emits typed payloads, operation markers, and async capability-checked wrappers.
/// Protocol-specific mapping and transport lifecycle belong in the caller's transport.
pub fn rust(descriptor: &ApplicationDescriptor) -> Result<String, GenerationError> {
    let mut emitter = Emitter {
        definitions: String::new(),
        names: BTreeSet::new(),
        next: 0,
    };
    emitter.line("// Generated from the fixed Phenix application descriptor. Do not edit.");
    emitter.line(&format!(
        "pub const INTERFACE_ID: &str = {:?};",
        descriptor.id.as_str()
    ));
    for (id, schema) in &descriptor.types {
        let name = type_name(id)?;
        emitter.define(&name, schema)?;
        emitter.line(&format!(
            "impl phenix_core::PhenixContract for {name} {{ fn contract_id() -> phenix_core::ContractId {{ phenix_core::ContractId::parse({:?}).expect(\"generated contract id is valid\") }} }}",
            id.as_str()
        ));
    }
    emitter.line("pub fn type_schemas() -> std::collections::BTreeMap<phenix_core::ContractId, phenix_core::PhenixSchema> { std::collections::BTreeMap::from([");
    for id in descriptor.types.keys() {
        let name = type_name(id)?;
        emitter.line(&format!("(<{name} as phenix_core::PhenixContract>::contract_id(), <{name} as phenix_core::HasPhenixSchema>::phenix_schema()),"));
    }
    emitter.line("]) }");
    for (id, operation) in &descriptor.operations {
        for reference in [&operation.input, &operation.output, &operation.error] {
            require_type(descriptor, reference)?;
        }
        require_capability(descriptor, &operation.capability)?;
        let name = symbol(id, "Operation")?;
        emitter.reserve(&name)?;
        let input = type_name(&operation.input)?;
        let output = type_name(&operation.output)?;
        emitter.line(&format!("pub struct {name};"));
        emitter.line(&format!(
            "impl phenix_application_interface::Operation for {name} {{ const ID: &'static str = {:?}; const CAPABILITY: &'static str = {:?}; type Input = {input}; type Output = {output}; }}",
            id.as_str(), operation.capability.as_str()
        ));
        emitter.line(&format!(
            "impl {name} {{ pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: {input}) -> Result<{output}, phenix_application_interface::types::ApplicationError> {{ client.invoke::<Self>(input).await }} }}"
        ));
    }
    // Event/callback aliases preserve their role and stable identity even when payloads are shared.
    for (id, event) in &descriptor.events {
        require_type(descriptor, &event.payload)?;
        require_capability(descriptor, &event.capability)?;
        emitter.role(id, "Event", &event.payload)?;
    }
    for (id, callback) in &descriptor.callbacks {
        require_type(descriptor, &callback.request)?;
        require_type(descriptor, &callback.response)?;
        require_capability(descriptor, &callback.capability)?;
        emitter.role(id, "CallbackRequest", &callback.request)?;
        emitter.role(id, "CallbackResponse", &callback.response)?;
    }
    for (id, capability) in &descriptor.capabilities {
        for dependency in &capability.dependencies {
            require_capability(descriptor, dependency)?;
        }
        let name = symbol(id, "Capability")?;
        emitter.reserve(&name)?;
        emitter.line(&format!(
            "pub struct {name}; impl {name} {{ pub const ID: &'static str = {:?}; }}",
            id.as_str()
        ));
    }
    Ok(emitter.definitions)
}

fn require_type(
    descriptor: &ApplicationDescriptor,
    id: &ContractId,
) -> Result<(), GenerationError> {
    if descriptor.types.contains_key(id) {
        return Ok(());
    }
    Err(GenerationError::MissingReference(id.clone()))
}
fn require_capability(
    descriptor: &ApplicationDescriptor,
    id: &ContractId,
) -> Result<(), GenerationError> {
    if descriptor.capabilities.contains_key(id) {
        return Ok(());
    }
    Err(GenerationError::MissingReference(id.clone()))
}
fn type_name(id: &ContractId) -> Result<String, GenerationError> {
    symbol(id, "Type")
}
fn symbol(id: &ContractId, suffix: &str) -> Result<String, GenerationError> {
    let mut name = String::new();
    for segment in id.as_str().split(['.', '-', '/', ':', '@']) {
        if segment.is_empty() {
            return Err(GenerationError::InvalidIdentifier(id.to_string()));
        }
        let mut chars = segment.chars();
        if let Some(first) = chars.next() {
            name.extend(first.to_uppercase());
        }
        name.extend(chars);
    }
    name.push_str(suffix);
    identifier(&name)?;
    Ok(name)
}
fn identifier(value: &str) -> Result<(), GenerationError> {
    if !value.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        || !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        || matches!(value, "self" | "Self" | "super" | "crate" | "_")
    {
        return Err(GenerationError::InvalidIdentifier(value.into()));
    }
    Ok(())
}

struct Emitter {
    definitions: String,
    names: BTreeSet<String>,
    next: usize,
}
impl Emitter {
    fn line(&mut self, line: &str) {
        writeln!(self.definitions, "{line}").expect("writing to a String cannot fail");
    }
    fn reserve(&mut self, name: &str) -> Result<(), GenerationError> {
        if self.names.insert(name.to_owned()) {
            return Ok(());
        }
        Err(GenerationError::DuplicateIdentifier(name.into()))
    }
    fn role(
        &mut self,
        id: &ContractId,
        role: &str,
        payload: &ContractId,
    ) -> Result<(), GenerationError> {
        let name = symbol(id, role)?;
        self.reserve(&name)?;
        self.line(&format!("pub type {name} = {};", type_name(payload)?));
        self.line(&format!(
            "pub const {}: &str = {:?};",
            symbol(id, role)?.to_uppercase(),
            id.as_str()
        ));
        Ok(())
    }
    fn nested(&mut self, schema: &PhenixSchema) -> Result<String, GenerationError> {
        let ty = match schema {
            PhenixSchema::Any => "phenix_core::PhenixValue".into(),
            PhenixSchema::Unit => "()".into(),
            PhenixSchema::Bool => "bool".into(),
            PhenixSchema::I64 => "i64".into(),
            PhenixSchema::U64 => "u64".into(),
            PhenixSchema::F64 => "f64".into(),
            PhenixSchema::String => "String".into(),
            PhenixSchema::Bytes => "phenix_core::Bytes".into(),
            PhenixSchema::Option(item) => format!("Option<{}>", self.nested(item)?),
            PhenixSchema::List(item) => format!("Vec<{}>", self.nested(item)?),
            PhenixSchema::Map(item) => {
                format!("std::collections::BTreeMap<String, {}>", self.nested(item)?)
            }
            PhenixSchema::Array { item, len } => format!("[{}; {len}]", self.nested(item)?),
            PhenixSchema::Table(_) | PhenixSchema::Variant(_) => {
                let name = format!("Structural{}", self.next);
                self.next += 1;
                self.define(&name, schema)?;
                name
            }
            PhenixSchema::Object { contract } => self.object(contract)?,
            PhenixSchema::Never | PhenixSchema::Callable { .. } => {
                return Err(GenerationError::UnsupportedSchema(schema.clone()));
            }
        };
        Ok(ty)
    }
    fn define(&mut self, name: &str, schema: &PhenixSchema) -> Result<(), GenerationError> {
        self.reserve(name)?;
        let declaration = match schema {
            PhenixSchema::Table(fields) => {
                let mut body = String::new();
                for (key, value) in fields {
                    identifier(key.as_str())?;
                    write!(body, "pub r#{}: {},", key, self.nested(value)?).expect("String write");
                }
                format!("pub struct {name} {{ {body} }}")
            }
            PhenixSchema::Variant(variants) => {
                let mut body = String::new();
                for (key, value) in variants {
                    identifier(key.as_str())?;
                    if *value == PhenixSchema::Unit {
                        write!(body, "r#{key},").expect("String write");
                    } else {
                        write!(body, "r#{key}({}),", self.nested(value)?).expect("String write");
                    }
                }
                format!("pub enum {name} {{ {body} }}")
            }
            _ => return Err(GenerationError::UnsupportedSchema(schema.clone())),
        };
        self.line("#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]");
        self.line(&declaration);
        Ok(())
    }

    fn object(&mut self, contract: &ContractId) -> Result<String, GenerationError> {
        let name = format!("Object{}", self.next);
        self.next += 1;
        self.reserve(&name)?;
        self.line("#[derive(Clone, Debug, PartialEq)]");
        self.line(&format!("pub struct {name}(pub phenix_core::ObjectRef);"));
        self.line(&format!(
            "impl phenix_core::ValueCodec for {name} {{ fn phenix_type() -> phenix_core::PhenixSchema {{ phenix_core::PhenixSchema::Object {{ contract: phenix_core::ContractId::parse({:?}).expect(\"generated object contract is valid\") }} }} fn to_value(&self) -> phenix_core::PhenixValue {{ phenix_core::PhenixValue::Object(self.0.clone()) }} fn from_value(value: &phenix_core::PhenixValue) -> Result<Self, phenix_core::ValueError> {{ <Self as phenix_core::ValueCodec>::phenix_type().parse(value)?; match value {{ phenix_core::PhenixValue::Object(reference) => Ok(Self(reference.clone())), _ => unreachable!(\"validated object value\"), }} }} }}",
            contract.as_str()
        ));
        self.line(&format!(
            "impl From<&{name}> for phenix_core::PhenixValue {{ fn from(value: &{name}) -> Self {{ <{name} as phenix_core::ValueCodec>::to_value(value) }} }}"
        ));
        self.line(&format!(
            "impl TryFrom<phenix_core::Exact<&phenix_core::PhenixValue>> for {name} {{ type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Exact<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> {{ <Self as phenix_core::ValueCodec>::from_value(value.0) }} }}"
        ));
        self.line(&format!(
            "impl TryFrom<phenix_core::Project<&phenix_core::PhenixValue>> for {name} {{ type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Project<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> {{ <Self as phenix_core::ValueCodec>::project_from_value(value.0) }} }}"
        ));
        Ok(name)
    }
}
