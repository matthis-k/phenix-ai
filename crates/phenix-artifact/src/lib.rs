use object::{Object, ObjectSection};
use phenix_contract::{Code, ContractMetadata, Failure, GraphContribution, MetadataExecution, NATIVE_SECTION_BYTES};
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
    if data.len() != 16 * 1024 && data.len() != NATIVE_SECTION_BYTES {
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
    let metadata = ContractMetadata::decode(payload)?;
    if metadata.execution != MetadataExecution::StatePending {
        return Err(invalid("native metadata requires executable profile"));
    }
    Ok(payload.to_vec())
}

/**
 * Inspect optional graph contributions in ELF bytes before dlopen.
 * The artifact SHA-256 binds both sections to the verified package.
 */
pub fn extract_graph(bytes: &[u8]) -> Result<Option<GraphContribution>, Failure> {
    let object = object::File::parse(bytes)
        .map_err(|e| Failure::new(Code::ArtifactInvalid, e.to_string()))?;
    let mut sections = object.sections().filter(|s| s.name().ok() == Some(".phenix.graph"));
    let Some(section) = sections.next() else {
        return Ok(None);
    };
    if sections.next().is_some() {
        return Err(Failure::new(Code::DescriptorInvalid, "duplicate graph section"));
    }
    let data = section.data()
        .map_err(|e| Failure::new(Code::DescriptorInvalid, e.to_string()))?;
    if data.len() != NATIVE_SECTION_BYTES {
        return Err(Failure::new(Code::DescriptorInvalid, "graph section has invalid size"));
    }
    let len = u32::from_be_bytes(data[..4].try_into().map_err(|_| {
        Failure::new(Code::DescriptorInvalid, "graph section missing length")
    })?) as usize;
    let end = 4usize.checked_add(len)
        .ok_or_else(|| Failure::new(Code::DescriptorInvalid, "graph section overflow"))?;
    let payload = data.get(4..end)
        .ok_or_else(|| Failure::new(Code::DescriptorInvalid, "graph section truncated"))?;
    if data[end..].iter().any(|byte| *byte != 0) {
        return Err(Failure::new(Code::DescriptorInvalid, "nonzero graph section padding"));
    }
    Ok(Some(GraphContribution::decode(payload)?))
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
    metadata: ContractMetadata,
    graph: Option<GraphContribution>,
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
    pub fn metadata(&self) -> &ContractMetadata {
        &self.metadata
    }
    pub fn graph(&self) -> Option<&GraphContribution> {
        self.graph.as_ref()
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
        if record.format != "phenix.native-package.v2"
            || record.target != "x86_64-unknown-linux-gnu"
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
        let graph = extract_graph(&executable)?;
        let extracted = extract(&executable)?;
        if extracted != metadata {
            return Err(Failure::new(
                Code::DescriptorInvalid,
                "packaged metadata differs from embedded metadata",
            ));
        }
        let metadata = ContractMetadata::decode(&metadata)?;
        if metadata.execution != MetadataExecution::StatePending {
            return Err(Failure::new(
                Code::DescriptorInvalid,
                "native metadata execution profile",
            ));
        }
        if graph.as_ref().is_some_and(|declaration| declaration.plugin != metadata.plugin) {
            return Err(Failure::new(Code::DescriptorInvalid, "graph owner differs from artifact"));
        }
        Ok(Self {
            artifact,
            record,
            metadata,
            graph,
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
