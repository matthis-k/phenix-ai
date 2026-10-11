use object::{Object, ObjectSection};
use phenix_contract::{
    Code, ContractMetadata, Descriptor, Failure, MetadataExecution, NATIVE_SECTION_BYTES,
    SECTION_BYTES,
};
use sha2::{Digest, Sha256};
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn extract(bytes: &[u8]) -> Result<Vec<u8>, Failure> {
    let invalid = |m: String| Failure::new(Code::DescriptorInvalid, m);
    let object = object::File::parse(bytes).map_err(|e| invalid(e.to_string()))?;
    if object.format() != object::BinaryFormat::Elf
        || object.kind() != object::ObjectKind::Dynamic
        || object.architecture() != object::Architecture::X86_64
        || !object.is_little_endian()
    {
        return Err(Failure::new(
            Code::ArtifactInvalid,
            "expected x86_64 Linux ELF shared library",
        ));
    }
    let mut sections = object
        .sections()
        .filter(|s| s.name().ok() == Some(".phenix"));
    let section = sections
        .next()
        .ok_or_else(|| invalid("missing .phenix descriptor section".into()))?;
    if sections.next().is_some() {
        return Err(invalid("duplicate descriptor section".into()));
    }
    let data = section.data().map_err(|e| invalid(e.to_string()))?;
    extract_section(data)
}

fn extract_section(data: &[u8]) -> Result<Vec<u8>, Failure> {
    let invalid = |m: &str| Failure::new(Code::DescriptorInvalid, m);
    if data.len() != SECTION_BYTES && data.len() != NATIVE_SECTION_BYTES {
        return Err(invalid("invalid descriptor section size"));
    }
    let size = u32::from_be_bytes(
        data[..4]
            .try_into()
            .map_err(|_| invalid("descriptor length"))?,
    ) as usize;
    let end = 4usize
        .checked_add(size)
        .ok_or_else(|| invalid("descriptor overflow"))?;
    let payload = data
        .get(4..end)
        .ok_or_else(|| invalid("descriptor outside section"))?;
    if data[end..].iter().any(|&b| b != 0) {
        return Err(invalid("nonzero descriptor padding"));
    }
    if payload.starts_with(&[0x50, 0x56, 2, 0]) {
        let metadata = ContractMetadata::decode(payload)?;
        if metadata.execution != MetadataExecution::StatePending {
            return Err(invalid("native metadata requires executable state profile"));
        }
    } else {
        if data.len() != SECTION_BYTES {
            return Err(invalid("M0 requires its exact 16 KiB section"));
        }
        Descriptor::decode(payload)?;
    }
    Ok(payload.to_vec())
}

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRecord {
    pub format: String,
    pub artifact: String,
    pub descriptor: String,
    pub target: String,
    pub artifact_sha256: String,
    pub descriptor_sha256: String,
}
#[derive(Debug)]
pub struct VerifiedPackage {
    artifact: PathBuf,
    record: PackageRecord,
    descriptor: Descriptor,
    metadata: Option<ContractMetadata>,
}
fn read(path: &Path, code: Code) -> Result<Vec<u8>, Failure> {
    std::fs::read(path).map_err(|e| Failure::new(code, e.to_string()))
}
fn relative(root: &Path, path: &str) -> Result<PathBuf, Failure> {
    let p = Path::new(path);
    if p.as_os_str().is_empty() || !p.components().all(|x| matches!(x, Component::Normal(_))) {
        return Err(Failure::new(
            Code::ArtifactInvalid,
            "package path must contain only relative normal components",
        ));
    }
    let absolute = root
        .join(p)
        .canonicalize()
        .map_err(|e| Failure::new(Code::ArtifactInvalid, e.to_string()))?;
    if !absolute.starts_with(root) {
        return Err(Failure::new(
            Code::ArtifactInvalid,
            "package path escapes trusted package",
        ));
    }
    Ok(absolute)
}
impl VerifiedPackage {
    pub fn artifact(&self) -> &Path {
        &self.artifact
    }
    pub fn record(&self) -> &PackageRecord {
        &self.record
    }
    pub fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }
    pub fn metadata(&self) -> Option<&ContractMetadata> {
        self.metadata.as_ref()
    }
    /// Only the preparation resolver calls this after collecting every frozen
    /// contract. No selection or executable callback occurs here.
    pub fn normalize(
        &mut self,
        imports: Vec<phenix_contract::ImportSlot>,
        definitions: &std::collections::BTreeMap<
            (String, u64),
            phenix_contract::ContractDefinition,
        >,
    ) -> Result<(), Failure> {
        let Some(metadata) = &self.metadata else {
            return Ok(());
        };
        let mut normalized = metadata.clone();
        normalized.requires = imports;
        self.descriptor = normalize_metadata(&normalized, definitions)?;
        Ok(())
    }
    /// Trusted control-plane caller attests a read-only package source. The CLI
    /// accepts Nix store sources only. Mutable paths cannot provide TOCTOU safety.
    pub fn verify_trusted_read_only(root: &Path) -> Result<Self, Failure> {
        let root = root
            .canonicalize()
            .map_err(|e| Failure::new(Code::ArtifactInvalid, e.to_string()))?;
        let record: PackageRecord =
            serde_json::from_slice(&read(&root.join("package.json"), Code::ArtifactInvalid)?)
                .map_err(|e| Failure::new(Code::DescriptorInvalid, e.to_string()))?;
        if !matches!(
            record.format.as_str(),
            "phenix.package.v1" | "phenix.native-package.v2"
        ) || record.target != "x86_64-unknown-linux-gnu"
        {
            return Err(Failure::new(
                Code::ArtifactInvalid,
                "unsupported package format or target",
            ));
        }
        let artifact = relative(&root, &record.artifact)?;
        let descriptor_path = relative(&root, &record.descriptor)?;
        let executable = read(&artifact, Code::ArtifactInvalid)?;
        let metadata = read(&descriptor_path, Code::DescriptorInvalid)?;
        if digest(&executable) != record.artifact_sha256 {
            return Err(Failure::new(
                Code::ArtifactInvalid,
                "artifact digest mismatch",
            ));
        }
        if digest(&metadata) != record.descriptor_sha256 {
            return Err(Failure::new(
                Code::DescriptorInvalid,
                "descriptor digest mismatch",
            ));
        }
        let extracted = extract(&executable)?;
        if extracted != metadata {
            return Err(Failure::new(
                Code::DescriptorInvalid,
                "packaged metadata differs from embedded metadata",
            ));
        }
        let (descriptor, metadata) = if record.format == "phenix.native-package.v2" {
            let metadata = ContractMetadata::decode(&metadata)?;
            if metadata.execution != MetadataExecution::StatePending {
                return Err(Failure::new(
                    Code::DescriptorInvalid,
                    "native metadata execution profile",
                ));
            }
            (empty_descriptor(&metadata), Some(metadata))
        } else {
            (Descriptor::decode(&metadata)?, None)
        };
        Ok(Self {
            artifact,
            record,
            descriptor,
            metadata,
        })
    }
    pub fn recheck(&self) -> Result<(), Failure> {
        if digest(&read(&self.artifact, Code::ArtifactInvalid)?) != self.record.artifact_sha256 {
            return Err(Failure::new(
                Code::ArtifactInvalid,
                "artifact changed after preparation",
            ));
        }
        Ok(())
    }
}

/// Metadata-only packages are a distinct product and cannot be passed to dlopen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractPackageRecord {
    pub format: String,
    pub descriptor: String,
    pub descriptor_sha256: String,
}
#[derive(Debug)]
pub struct VerifiedContractPackage {
    metadata: phenix_contract::ContractMetadata,
    descriptor_sha256: String,
    package_sha256: String,
}
impl VerifiedContractPackage {
    pub fn metadata(&self) -> &phenix_contract::ContractMetadata {
        &self.metadata
    }
    pub fn descriptor_sha256(&self) -> &str {
        &self.descriptor_sha256
    }
    pub fn package_sha256(&self) -> &str {
        &self.package_sha256
    }
    /// The caller attests a trusted read-only source, as for native packages.
    /// No executable is discovered, extracted or loaded.
    pub fn verify_trusted_read_only(root: &Path) -> Result<Self, Failure> {
        let root = root
            .canonicalize()
            .map_err(|e| Failure::new(Code::ArtifactInvalid, e.to_string()))?;
        let record: ContractPackageRecord =
            serde_json::from_slice(&read(&root.join("package.json"), Code::ArtifactInvalid)?)
                .map_err(|e| Failure::new(Code::DescriptorInvalid, e.to_string()))?;
        if record.format != "phenix.contract-package.v2" {
            return Err(Failure::new(
                Code::ArtifactInvalid,
                "unsupported contract package format",
            ));
        }
        let bytes = read(
            &relative(&root, &record.descriptor)?,
            Code::DescriptorInvalid,
        )?;
        let descriptor_sha256 = digest(&bytes);
        if descriptor_sha256 != record.descriptor_sha256 {
            return Err(Failure::new(
                Code::DescriptorInvalid,
                "contract descriptor digest mismatch",
            ));
        }
        let metadata = phenix_contract::ContractMetadata::decode(&bytes)?;
        if metadata.execution != phenix_contract::MetadataExecution::None {
            return Err(Failure::new(
                Code::DescriptorInvalid,
                "contract package must contain definitions only",
            ));
        }
        let mut hash = Sha256::new();
        hash.update(b"phenix.contract-package.v2\0");
        hash.update(Sha256::digest(&bytes));
        let package_sha256 = format!("{:x}", hash.finalize());
        Ok(Self {
            metadata,
            descriptor_sha256,
            package_sha256,
        })
    }
}

fn empty_descriptor(metadata: &ContractMetadata) -> Descriptor {
    Descriptor {
        abi_major: 3,
        abi_minor: 0,
        features: 3,
        execution: "state-pending".into(),
        format: "phenix.normalized.v2".into(),
        schema_version: 2,
        plugin_id: metadata.plugin.clone(),
        plugin_version: metadata.version.clone(),
        authority_ceiling: vec![],
        contracts: vec![],
        imports: vec![],
        exports: vec![],
    }
}
/// Stable synthetic property contract identities. No access is granted here.
pub fn property_contract(owner: &str, property: &str) -> Result<String, Failure> {
    let name = format!("{owner}.property.{property}");
    if !phenix_contract::identifier(&name) {
        return Err(Failure::new(
            Code::DescriptorInvalid,
            "property identity exceeds protocol limits",
        ));
    }
    Ok(format!("{name}@1"))
}
pub fn property_definitions(
    metadata: &ContractMetadata,
) -> Result<Vec<phenix_contract::ContractDefinition>, Failure> {
    use phenix_contract::{ContractDefinition, OperationSchema, ValueSchema as S};
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
fn normalize_metadata(
    metadata: &ContractMetadata,
    definitions: &std::collections::BTreeMap<(String, u64), phenix_contract::ContractDefinition>,
) -> Result<Descriptor, Failure> {
    use phenix_contract::{Cardinality, Contract, Contribution, SchemaSpec};
    let mut descriptor = empty_descriptor(metadata);
    let mut declarations = std::collections::BTreeMap::new();
    let mut add = |slot: &str,
                   name: &str,
                   revision: u64,
                   provide: Option<bool>,
                   optional: bool|
     -> Result<Vec<Contribution>, Failure> {
        let definition = definitions.get(&(name.into(), revision)).ok_or_else(|| {
            Failure::new(
                Code::MissingBinding,
                format!("missing contract definition {name} revision {revision}"),
            )
        })?;
        let mut contributions = vec![];
        for op in &definition.operations {
            let key = format!("{name}/{}", op.name);
            let schema = |s: &phenix_contract::ValueSchema| -> Result<SchemaSpec, Failure> {
                Ok(SchemaSpec::Live { bytes: s.encode()? })
            };
            let contract = Contract {
                id: format!("{}/{key}/Declare", metadata.plugin),
                kind: "phenix.contract.v2".into(),
                owner: metadata.plugin.clone(),
                provenance: name.into(),
                role: "Declare".into(),
                key: key.clone(),
                request: schema(&op.request)?,
                response: schema(&op.response)?,
                error: schema(&op.error)?,
            };
            if let Some(previous) = declarations.insert(key.clone(), contract.clone()) {
                if previous != contract {
                    return Err(Failure::new(
                        Code::SchemaRejected,
                        "selected revisions have conflicting schemas",
                    ));
                }
            }
            if let Some(provide) = provide {
                let role = if provide { "Provide" } else { "Require" };
                contributions.push(Contribution {
                    slot: Some(slot.into()),
                    optional,
                    implementation: None,
                    id: format!("{}/{slot}/{key}/{role}", metadata.plugin),
                    kind: "phenix.operation.v2".into(),
                    operation: key,
                    owner: metadata.plugin.clone(),
                    provenance: slot.into(),
                    role: role.into(),
                });
            }
        }
        Ok(contributions)
    };
    for p in &metadata.provides {
        descriptor
            .exports
            .extend(add(&p.slot, &p.contract, p.revision, Some(true), false)?);
    }
    for p in &metadata.properties {
        descriptor.exports.extend(add(
            &format!("property.{}", p.id),
            &property_contract(&metadata.plugin, &p.id)?,
            1,
            Some(true),
            false,
        )?);
    }
    for r in &metadata.requires {
        descriptor.imports.extend(add(
            &r.slot,
            &r.contract,
            r.revision_min,
            Some(false),
            r.cardinality == Cardinality::Optional,
        )?);
    }
    // A native definition declares a delegation ceiling, without selecting or
    // importing a provider. Runtime grants still intersect every ancestor.
    for definition in &metadata.definitions {
        add("", &definition.contract, definition.revision, None, false)?;
    }
    descriptor.contracts = declarations.into_values().collect();
    descriptor.authority_ceiling = descriptor.contracts.iter().map(|c| c.key.clone()).collect();
    descriptor.imports.sort_by(|a, b| a.id.cmp(&b.id));
    descriptor.exports.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(descriptor)
}
