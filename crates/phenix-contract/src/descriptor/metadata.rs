//! Offline M1 definitions and contributions. This module never selects providers.
use crate::{identifier, operation_key, Code, Failure, Value, ValueSchema, MAX_COUNT};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationSchema {
    pub name: String,
    pub request: ValueSchema,
    pub response: ValueSchema,
    pub error: ValueSchema,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractDefinition {
    pub contract: String,
    pub revision: u64,
    pub operations: Vec<OperationSchema>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    One,
    Optional,
    Many { minimum: u64 },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSlot {
    pub slot: String,
    pub contract: String,
    pub provider: Option<String>,
    pub revision_min: u64,
    pub revision_max: u64,
    pub cardinality: Cardinality,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSlot {
    pub slot: String,
    pub contract: String,
    pub revision: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyDefinition {
    pub id: String,
    pub schema: ValueSchema,
    pub get: bool,
    pub set: bool,
    pub watch: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataExecution {
    None,
    StatePending,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractMetadata {
    pub plugin: String,
    pub version: String,
    pub execution: MetadataExecution,
    pub definitions: Vec<ContractDefinition>,
    pub requires: Vec<ImportSlot>,
    pub provides: Vec<ExportSlot>,
    pub properties: Vec<PropertyDefinition>,
}
fn invalid(message: &str) -> Failure {
    Failure::new(Code::DescriptorInvalid, message)
}
fn string(s: &str) -> Value {
    Value::String(s.into())
}
fn contract(s: &str) -> bool {
    operation_key(&format!("{s}/probe"))
}
fn ordered<'a>(keys: impl Iterator<Item = &'a str>) -> bool {
    let keys: Vec<_> = keys.collect();
    keys.len() <= MAX_COUNT && keys.windows(2).all(|p| p[0] < p[1])
}
impl ContractMetadata {
    /// Authored order is normalized once; duplicate identities are never merged.
    pub fn encode(&self) -> Result<Vec<u8>, Failure> {
        let mut metadata = self.clone();
        metadata
            .definitions
            .sort_by(|a, b| a.contract.cmp(&b.contract));
        for definition in &mut metadata.definitions {
            definition.operations.sort_by(|a, b| a.name.cmp(&b.name));
        }
        metadata.requires.sort_by(|a, b| a.slot.cmp(&b.slot));
        metadata.provides.sort_by(|a, b| a.slot.cmp(&b.slot));
        metadata.properties.sort_by(|a, b| a.id.cmp(&b.id));
        metadata.validate()?;
        metadata.value()?.encode()
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        let mut root = Fields::new(Value::decode(bytes)?)?;
        if root.text("format")? != "phenix.contract-metadata.v2" {
            return Err(invalid("metadata profile"));
        }
        let plugin = root.text("plugin")?;
        let version = root.text("version")?;
        let execution = match root.text("execution")?.as_str() {
            "none" => MetadataExecution::None,
            "state-pending" => MetadataExecution::StatePending,
            _ => return Err(invalid("metadata execution profile")),
        };
        let definitions = root
            .list("definitions")?
            .into_iter()
            .map(|v| {
                let mut f = Fields::new(v)?;
                let contract = f.text("contract")?;
                let revision = f.number("revision")?;
                let operations = f
                    .list("operations")?
                    .into_iter()
                    .map(|v| {
                        let mut f = Fields::new(v)?;
                        let op = OperationSchema {
                            name: f.text("name")?,
                            request: f.schema("request")?,
                            response: f.schema("response")?,
                            error: f.schema("error")?,
                        };
                        f.finish()?;
                        Ok(op)
                    })
                    .collect::<Result<_, Failure>>()?;
                f.finish()?;
                Ok(ContractDefinition {
                    contract,
                    revision,
                    operations,
                })
            })
            .collect::<Result<_, Failure>>()?;
        let requires = root
            .list("requires")?
            .into_iter()
            .map(|v| {
                let mut f = Fields::new(v)?;
                let slot = f.text("slot")?;
                let contract = f.text("contract")?;
                let provider = match f.take("provider")? { Value::Null=>None,Value::String(id)=>Some(id),_=>return Err(invalid("import provider identity")) };
                let revision_min = f.number("revision_min")?;
                let revision_max = f.number("revision_max")?;
                let kind = f.text("cardinality")?;
                let minimum = f.number("minimum")?;
                let cardinality = match (kind.as_str(), minimum) {
                    ("one", 1) => Cardinality::One,
                    ("optional", 0) => Cardinality::Optional,
                    ("many", minimum) if minimum <= MAX_COUNT as u64 => {
                        Cardinality::Many { minimum }
                    }
                    _ => return Err(invalid("slot cardinality")),
                };
                f.finish()?;
                Ok(ImportSlot {
                    slot,
                    contract,
                    provider,
                    revision_min,
                    revision_max,
                    cardinality,
                })
            })
            .collect::<Result<_, Failure>>()?;
        let provides = root
            .list("provides")?
            .into_iter()
            .map(|v| {
                let mut f = Fields::new(v)?;
                let export = ExportSlot {
                    slot: f.text("slot")?,
                    contract: f.text("contract")?,
                    revision: f.number("revision")?,
                };
                f.finish()?;
                Ok(export)
            })
            .collect::<Result<_, Failure>>()?;
        let properties = root
            .list("properties")?
            .into_iter()
            .map(|v| {
                let mut f = Fields::new(v)?;
                let property = PropertyDefinition {
                    id: f.text("id")?,
                    schema: f.schema("schema")?,
                    get: f.boolean("get")?,
                    set: f.boolean("set")?,
                    watch: f.boolean("watch")?,
                };
                f.finish()?;
                Ok(property)
            })
            .collect::<Result<_, Failure>>()?;
        root.finish()?;
        let metadata = Self {
            plugin,
            version,
            execution,
            definitions,
            requires,
            provides,
            properties,
        };
        metadata.validate()?;
        if metadata.encode()? != bytes {
            return Err(invalid("noncanonical metadata"));
        }
        Ok(metadata)
    }
    pub fn validate(&self) -> Result<(), Failure> {
        if !identifier(&self.plugin)
            || self.plugin == "root"
            || !identifier(&self.version)
            || !ordered(self.definitions.iter().map(|d| d.contract.as_str()))
            || !ordered(self.requires.iter().map(|d| d.slot.as_str()))
            || !ordered(self.provides.iter().map(|d| d.slot.as_str()))
            || !ordered(self.properties.iter().map(|d| d.id.as_str()))
        {
            return Err(invalid("invalid identity, order or duplicate contribution"));
        }
        if self.execution == MetadataExecution::None
            && (!self.requires.is_empty()
                || !self.provides.is_empty()
                || !self.properties.is_empty())
        {
            return Err(invalid(
                "definition-only metadata contains executable contributions",
            ));
        }
        for definition in &self.definitions {
            if !contract(&definition.contract)
                || definition.revision == 0
                || definition.operations.is_empty()
                || !ordered(definition.operations.iter().map(|o| o.name.as_str()))
            {
                return Err(invalid("invalid contract definition"));
            }
            for op in &definition.operations {
                if !identifier(&op.name) {
                    return Err(invalid("invalid operation name"));
                }
                op.request.encode()?;
                op.response.encode()?;
                op.error.encode()?;
            }
        }
        for slot in &self.requires {
            if !identifier(&slot.slot)
                || !contract(&slot.contract)
                || slot.provider.as_ref().is_some_and(|id| !identifier(id) || id=="root")
                || slot.revision_min == 0
                || slot.revision_max < slot.revision_min
                || matches!(slot.cardinality, Cardinality::Many { minimum } if minimum > MAX_COUNT as u64)
            {
                return Err(invalid("invalid import slot"));
            }
        }
        for slot in &self.provides {
            if !identifier(&slot.slot) || !contract(&slot.contract) || slot.revision == 0 {
                return Err(invalid("invalid export slot"));
            }
        }
        for property in &self.properties {
            if !identifier(&property.id) || (!property.get && !property.set && !property.watch) {
                return Err(invalid("invalid property"));
            }
            property.schema.encode()?;
        }
        Ok(())
    }
    fn value(&self) -> Result<Value, Failure> {
        let definitions = self
            .definitions
            .iter()
            .map(|d| {
                let operations = d
                    .operations
                    .iter()
                    .map(|o| {
                        Ok(Value::map([
                            ("name", string(&o.name)),
                            ("request", Value::Bytes(o.request.encode()?)),
                            ("response", Value::Bytes(o.response.encode()?)),
                            ("error", Value::Bytes(o.error.encode()?)),
                        ]))
                    })
                    .collect::<Result<Vec<_>, Failure>>()?;
                Ok(Value::map([
                    ("contract", string(&d.contract)),
                    ("revision", Value::U64(d.revision)),
                    ("operations", Value::List(operations)),
                ]))
            })
            .collect::<Result<Vec<_>, Failure>>()?;
        let requires = self
            .requires
            .iter()
            .map(|s| {
                let (kind, minimum) = match s.cardinality {
                    Cardinality::One => ("one", 1),
                    Cardinality::Optional => ("optional", 0),
                    Cardinality::Many { minimum } => ("many", minimum),
                };
                Value::map([
                    ("slot", string(&s.slot)),
                    ("contract", string(&s.contract)),
                    ("provider", s.provider.as_ref().map_or(Value::Null,|id|string(id))),
                    ("revision_min", Value::U64(s.revision_min)),
                    ("revision_max", Value::U64(s.revision_max)),
                    ("cardinality", string(kind)),
                    ("minimum", Value::U64(minimum)),
                ])
            })
            .collect();
        let provides = self
            .provides
            .iter()
            .map(|s| {
                Value::map([
                    ("slot", string(&s.slot)),
                    ("contract", string(&s.contract)),
                    ("revision", Value::U64(s.revision)),
                ])
            })
            .collect();
        let properties = self
            .properties
            .iter()
            .map(|p| {
                Ok(Value::map([
                    ("id", string(&p.id)),
                    ("schema", Value::Bytes(p.schema.encode()?)),
                    ("get", Value::Bool(p.get)),
                    ("set", Value::Bool(p.set)),
                    ("watch", Value::Bool(p.watch)),
                ]))
            })
            .collect::<Result<_, Failure>>()?;
        Ok(Value::map([
            ("format", string("phenix.contract-metadata.v2")),
            ("plugin", string(&self.plugin)),
            ("version", string(&self.version)),
            (
                "execution",
                string(match self.execution {
                    MetadataExecution::None => "none",
                    MetadataExecution::StatePending => "state-pending",
                }),
            ),
            ("definitions", Value::List(definitions)),
            ("requires", Value::List(requires)),
            ("provides", Value::List(provides)),
            ("properties", Value::List(properties)),
        ]))
    }
}
struct Fields(BTreeMap<String, Value>);
impl Fields {
    fn new(value: Value) -> Result<Self, Failure> {
        if let Value::Map(fields) = value {
            Ok(Self(fields))
        } else {
            Err(invalid("expected metadata map"))
        }
    }
    fn take(&mut self, key: &str) -> Result<Value, Failure> {
        self.0
            .remove(key)
            .ok_or_else(|| invalid("missing metadata field"))
    }
    fn text(&mut self, key: &str) -> Result<String, Failure> {
        if let Value::String(s) = self.take(key)? {
            Ok(s)
        } else {
            Err(invalid("expected string"))
        }
    }
    fn number(&mut self, key: &str) -> Result<u64, Failure> {
        if let Value::U64(n) = self.take(key)? {
            Ok(n)
        } else {
            Err(invalid("expected unsigned integer"))
        }
    }
    fn boolean(&mut self, key: &str) -> Result<bool, Failure> {
        if let Value::Bool(b) = self.take(key)? {
            Ok(b)
        } else {
            Err(invalid("expected bool"))
        }
    }
    fn list(&mut self, key: &str) -> Result<Vec<Value>, Failure> {
        if let Value::List(v) = self.take(key)? {
            Ok(v)
        } else {
            Err(invalid("expected list"))
        }
    }
    fn schema(&mut self, key: &str) -> Result<ValueSchema, Failure> {
        if let Value::Bytes(bytes) = self.take(key)? {
            ValueSchema::decode(&bytes)
        } else {
            Err(invalid("expected canonical schema bytes"))
        }
    }
    fn finish(self) -> Result<(), Failure> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(invalid("unknown metadata field"))
        }
    }
}

/// Stable synthetic property contract identities. No access is granted here.
pub fn property_contract(owner: &str, property: &str) -> Result<String, Failure> {
    let name = format!("{owner}.property.{property}");
    if !crate::identifier(&name) {
        return Err(Failure::new(
            Code::DescriptorInvalid,
            "property identity exceeds protocol limits",
        ));
    }
    Ok(format!("{name}@1"))
}
pub fn property_definitions(
    metadata: &ContractMetadata,
) -> Result<Vec<crate::ContractDefinition>, Failure> {
    use crate::{ContractDefinition, OperationSchema, ValueSchema as S};
    metadata
        .properties
        .iter()
        .map(|p| {
            let reference = S::Ref(p.schema.id()?);
            let error = S::Variants(vec![
                "Conflict".into(),
                "Denied".into(),
                "OwnerGone".into(),
                "SchemaMismatch".into(),
                "StaleReference".into(),
            ]);
            let mut operations = vec![];
            if p.get {
                operations.push(OperationSchema {
                    name: "get".into(),
                    request: reference.clone(),
                    response: p.schema.clone(),
                    error: error.clone(),
                });
            }
            if p.set {
                operations.push(OperationSchema {
                    name: "set".into(),
                    request: S::Struct(std::collections::BTreeMap::from([
                        ("expected".into(), S::U64),
                        ("location".into(), reference.clone()),
                        ("value".into(), p.schema.clone()),
                    ])),
                    response: S::U64,
                    error: error.clone(),
                });
            }
            if p.watch {
                operations.push(OperationSchema {
                    name: "watch".into(),
                    request: reference,
                    response: S::U64,
                    error,
                });
            }
            Ok(ContractDefinition {
                contract: property_contract(&metadata.plugin, &p.id)?,
                revision: 1,
                operations,
            })
        })
        .collect()
}
