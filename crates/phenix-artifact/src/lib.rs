use object::{Object, ObjectSection};
use phenix_contract::{graph::v2::GraphContributionV2, Value};
use phenix_contract::{
    Code, ContractMetadata, Failure, GraphContribution, MetadataExecution, NATIVE_SECTION_BYTES,
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
    match extract_graph_metadata(bytes)? {
        None => Ok(None),
        Some(ParsedGraph::V1(graph)) => Ok(Some(graph)),
        Some(ParsedGraph::V2(_)) => Err(Failure::new(
            Code::DescriptorInvalid,
            "graph-v2 requires the v2 accessor",
        )),
    }
}

/// Inspect canonical graph-v2 metadata offline; a v1 section is not discarded.
pub fn extract_graph_v2(bytes: &[u8]) -> Result<Option<GraphContributionV2>, Failure> {
    match extract_graph_metadata(bytes)? {
        None => Ok(None),
        Some(ParsedGraph::V2(graph)) => Ok(Some(graph)),
        Some(ParsedGraph::V1(_)) => Err(Failure::new(
            Code::DescriptorInvalid,
            "graph-v1 requires the v1 accessor",
        )),
    }
}

#[derive(Debug)]
enum ParsedGraph {
    V1(GraphContribution),
    V2(GraphContributionV2),
}

impl ParsedGraph {
    fn validate_owner(&self, plugin: &str) -> Result<(), Failure> {
        let owner = match self {
            Self::V1(graph) => &graph.plugin,
            Self::V2(graph) => &graph.contributor,
        };
        if owner != plugin {
            return Err(Failure::new(
                Code::DescriptorInvalid,
                "graph owner differs from artifact",
            ));
        }
        Ok(())
    }
}

fn extract_graph_metadata(bytes: &[u8]) -> Result<Option<ParsedGraph>, Failure> {
    let object = object::File::parse(bytes)
        .map_err(|e| Failure::new(Code::ArtifactInvalid, e.to_string()))?;
    let mut sections = object
        .sections()
        .filter(|s| s.name().ok() == Some(".phenix.graph"));
    let Some(section) = sections.next() else {
        return Ok(None);
    };
    if sections.next().is_some() {
        return Err(Failure::new(
            Code::DescriptorInvalid,
            "duplicate graph section",
        ));
    }
    let data = section
        .data()
        .map_err(|e| Failure::new(Code::DescriptorInvalid, e.to_string()))?;
    extract_graph_section(data).map(Some)
}

fn extract_graph_section(data: &[u8]) -> Result<ParsedGraph, Failure> {
    if data.len() != NATIVE_SECTION_BYTES {
        return Err(Failure::new(
            Code::DescriptorInvalid,
            "graph section has invalid size",
        ));
    }
    let len = u32::from_be_bytes(
        data[..4]
            .try_into()
            .map_err(|_| Failure::new(Code::DescriptorInvalid, "graph section missing length"))?,
    ) as usize;
    let end = 4usize
        .checked_add(len)
        .ok_or_else(|| Failure::new(Code::DescriptorInvalid, "graph section overflow"))?;
    let payload = data
        .get(4..end)
        .ok_or_else(|| Failure::new(Code::DescriptorInvalid, "graph section truncated"))?;
    if data[end..].iter().any(|byte| *byte != 0) {
        return Err(Failure::new(
            Code::DescriptorInvalid,
            "nonzero graph section padding",
        ));
    }
    decode_graph_metadata(payload)
}

fn decode_graph_metadata(payload: &[u8]) -> Result<ParsedGraph, Failure> {
    let value = Value::decode(payload)?;
    match &value {
        Value::Map(fields) => match fields.get("format") {
            Some(Value::String(format)) if format == "phenix.graph.v1" => {
                GraphContribution::decode(payload).map(ParsedGraph::V1)
            }
            Some(Value::String(format)) if format == "phenix.graph.v2" => {
                GraphContributionV2::decode(payload).map(ParsedGraph::V2)
            }
            _ => Err(Failure::new(
                Code::DescriptorInvalid,
                "unknown graph format",
            )),
        },
        _ => Err(Failure::new(Code::DescriptorInvalid, "expected graph map")),
    }
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
    graph_v2: Option<GraphContributionV2>,
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
    /// Offline metadata only; presence does not authorize graph-v2 execution.
    pub fn graph_v2(&self) -> Option<&GraphContributionV2> {
        self.graph_v2.as_ref()
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
        let graph = extract_graph_metadata(&executable)?;
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
        if let Some(graph) = &graph {
            graph.validate_owner(&metadata.plugin)?;
        }
        let (graph, graph_v2) = match graph {
            Some(ParsedGraph::V1(graph)) => (Some(graph), None),
            Some(ParsedGraph::V2(graph)) => (None, Some(graph)),
            None => (None, None),
        };
        Ok(Self {
            artifact,
            record,
            metadata,
            graph,
            graph_v2,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<ContractGraphRecord>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractGraphRecord {
    pub file: String,
    pub sha256: String,
}
#[derive(Debug)]
pub struct VerifiedContractPackage {
    metadata: phenix_contract::ContractMetadata,
    descriptor_sha256: String,
    package_sha256: String,
    graph: Option<phenix_contract::GraphContribution>,
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
    pub fn graph(&self) -> Option<&phenix_contract::GraphContribution> {
        self.graph.as_ref()
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
        let graph = if let Some(record) = record.graph {
            let bytes = read(&relative(&root, &record.file)?, Code::DescriptorInvalid)?;
            if digest(&bytes) != record.sha256 {
                return Err(Failure::new(
                    Code::DescriptorInvalid,
                    "definition graph digest mismatch",
                ));
            }
            let graph = match decode_graph_metadata(&bytes)? {
                ParsedGraph::V1(graph) => graph,
                ParsedGraph::V2(_) => {
                    return Err(Failure::new(
                        Code::DescriptorInvalid,
                        "graph-v2 definition packages are not supported",
                    ));
                }
            };
            if graph.plugin != metadata.plugin || !graph.nodes.is_empty() {
                return Err(Failure::new(
                    Code::DescriptorInvalid,
                    "definition graph owner or executable nodes",
                ));
            }
            hash.update(b"graph\0");
            hash.update(Sha256::digest(&bytes));
            Some(graph)
        } else {
            None
        };
        let package_sha256 = format!("{:x}", hash.finalize());
        Ok(Self {
            metadata,
            descriptor_sha256,
            package_sha256,
            graph,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v2() -> GraphContributionV2 {
        GraphContributionV2 {
            contributor: "test.plugin".into(),
            sources: vec![],
            nodes: vec![],
            edges: vec![],
            patches: vec![],
        }
    }

    fn section(payload: &[u8]) -> Vec<u8> {
        let mut data = vec![0; NATIVE_SECTION_BYTES];
        data[..4].copy_from_slice(&(payload.len() as u32).to_be_bytes());
        data[4..4 + payload.len()].copy_from_slice(payload);
        data
    }

    #[test]
    fn canonical_graph_versions_and_owner() {
        let graph = v2();
        let payload = graph.encode().unwrap();
        let parsed = extract_graph_section(&section(&payload)).unwrap();
        parsed.validate_owner("test.plugin").unwrap();
        let error = parsed.validate_owner("other.plugin").unwrap_err();
        assert_eq!(error.code, Code::DescriptorInvalid);
        let ParsedGraph::V2(decoded) = parsed else {
            panic!("expected v2")
        };
        assert_eq!(decoded, graph);
        assert_eq!(decoded.encode().unwrap(), payload);

        let graph = GraphContribution {
            plugin: "test.plugin".into(),
            events: vec![],
            actions: vec![],
            nodes: vec![],
        };
        let parsed = extract_graph_section(&section(&graph.encode().unwrap())).unwrap();
        parsed.validate_owner("test.plugin").unwrap();
        assert!(parsed.validate_owner("other.plugin").is_err());
        let ParsedGraph::V1(decoded) = parsed else {
            panic!("expected v1")
        };
        assert_eq!(decoded, graph);
    }

    #[test]
    fn rejects_invalid_graph_sections() {
        let payload = v2().encode().unwrap();
        let valid = section(&payload);
        assert!(extract_graph_section(&valid[..valid.len() - 1]).is_err());
        assert!(extract_graph_section(&[]).is_err());
        let mut data = valid.clone();
        *data.last_mut().unwrap() = 1;
        assert!(extract_graph_section(&data).is_err());
        let mut data = valid;
        data[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(extract_graph_section(&data).is_err());
        for payload in [&[][..], &[0xff][..], &payload[..payload.len() - 1]] {
            assert!(extract_graph_section(&section(payload)).is_err());
        }
        let Value::Map(mut fields) = Value::decode(&payload).unwrap() else {
            unreachable!()
        };
        fields.insert("format".into(), Value::String("phenix.graph.v99".into()));
        let unknown = Value::Map(fields.clone()).encode().unwrap();
        assert!(extract_graph_section(&section(&unknown)).is_err());
        fields.remove("format");
        assert!(extract_graph_section(&section(&Value::Map(fields).encode().unwrap())).is_err());
    }
}
