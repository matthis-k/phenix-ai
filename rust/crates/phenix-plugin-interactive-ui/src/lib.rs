#![forbid(unsafe_code)]

//! Typed display documents. No model tools or frontend actions are exposed yet.

use phenix_core::{
    Authority, ComponentExport, ComponentId, ComponentInterface, ComponentManifest, InterfaceId,
    InterfaceSchema, PluginContext, PluginExecution, PluginHost, PluginId, PluginInstance,
    PluginManifest, ServiceContribution, ServiceId, ServiceRole,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const INTERACTIVE_UI_PLUGIN: &str = "phenix.interactive-ui";
pub const UI_DOCUMENT_SERVICE: &str = "phenix.interactive-ui.document@1";
const MAX_NODES: usize = 128;
const MAX_DEPTH: usize = 12;
const MAX_TEXT_BYTES: usize = 4096;
const MAX_DOCUMENT_BYTES: usize = 65536;
const MAX_DOCUMENT_KEYS: usize = 512;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum UiNode {
    Text {
        id: String,
        text: String,
    },
    Label {
        id: String,
        text: String,
    },
    Badge {
        id: String,
        text: String,
    },
    Progress {
        id: String,
        text: String,
        fraction: f64,
    },
    Row {
        id: String,
        children: Vec<String>,
    },
    Column {
        id: String,
        children: Vec<String>,
    },
    Card {
        id: String,
        title: String,
        children: Vec<String>,
    },
    Table {
        id: String,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

impl UiNode {
    fn id(&self) -> &str {
        match self {
            Self::Text { id, .. }
            | Self::Label { id, .. }
            | Self::Badge { id, .. }
            | Self::Progress { id, .. }
            | Self::Row { id, .. }
            | Self::Column { id, .. }
            | Self::Card { id, .. }
            | Self::Table { id, .. } => id,
        }
    }
    fn children(&self) -> &[String] {
        match self {
            Self::Row { children, .. }
            | Self::Column { children, .. }
            | Self::Card { children, .. } => children,
            _ => &[],
        }
    }
}

/// Flat encoded nodes form a tree rooted at one node. No nodes are shared.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct UiDocument {
    pub version: u32,
    pub session_id: String,
    pub document_id: String,
    pub revision: u64,
    pub root: String,
    pub nodes: Vec<UiNode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum UiDocumentCommand {
    Put {
        document: UiDocument,
        expected_revision: Option<u64>,
    },
    Get {
        session_id: String,
        document_id: String,
    },
    Dismiss {
        session_id: String,
        document_id: String,
        expected_revision: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum UiDocumentResponse {
    Stored { revision: u64 },
    Document { document: Option<UiDocument> },
    Dismissed { revision: u64 },
}

pub struct UiDocumentInterface;
impl ComponentInterface for UiDocumentInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(UI_DOCUMENT_SERVICE).expect("static UI interface ID")
    }
    fn schema() -> InterfaceSchema {
        InterfaceSchema::of::<UiDocumentCommand, UiDocumentResponse>()
    }
}

#[must_use]
pub fn interactive_ui_service() -> ServiceId {
    ServiceId::parse(UI_DOCUMENT_SERVICE).expect("static UI service")
}
#[must_use]
pub fn interactive_ui_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(INTERACTIVE_UI_PLUGIN).expect("static plugin ID"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: ServiceRole::Terminal,
            service: interactive_ui_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}
#[must_use]
pub fn interactive_ui_component_manifest() -> ComponentManifest {
    ComponentManifest {
        id: ComponentId::parse(INTERACTIVE_UI_PLUGIN).expect("static component ID"),
        owner: PluginId::parse(INTERACTIVE_UI_PLUGIN).expect("static owner ID"),
        listeners: Vec::new(),
        imports: Vec::new(),
        exports: vec![ComponentExport {
            interface: UiDocumentInterface::interface_id(),
            schema: UiDocumentInterface::schema(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        maximum_authority: Authority::default(),
    }
}
#[must_use]
pub fn interactive_ui_factory() -> Box<dyn PluginInstance> {
    Box::<UiStore>::default()
}

#[derive(Default)]
struct UiStore {
    documents: BTreeMap<(String, String), UiDocument>,
    tombstones: BTreeMap<(String, String), u64>,
}
impl UiStore {
    fn apply(&mut self, command: UiDocumentCommand) -> Result<UiDocumentResponse, String> {
        match command {
            UiDocumentCommand::Put {
                document,
                expected_revision,
            } => {
                validate_document(&document)?;
                let key = (document.session_id.clone(), document.document_id.clone());
                if self.documents.get(&key) == Some(&document) {
                    return Ok(UiDocumentResponse::Stored {
                        revision: document.revision,
                    });
                }
                let previous = self
                    .documents
                    .get(&key)
                    .map(|d| d.revision)
                    .or_else(|| self.tombstones.get(&key).copied());
                if previous != expected_revision {
                    return Err("UI document revision conflict".into());
                }
                if previous.is_none()
                    && self.documents.len() + self.tombstones.len() >= MAX_DOCUMENT_KEYS
                {
                    return Err("UI document key limit reached".into());
                }
                let next = match previous {
                    None => 1,
                    Some(previous) => previous
                        .checked_add(1)
                        .ok_or_else(|| "UI document revision exhausted".to_owned())?,
                };
                if next != document.revision {
                    return Err("UI document revision must advance by one".into());
                }
                self.tombstones.remove(&key);
                self.documents.insert(key, document);
                Ok(UiDocumentResponse::Stored { revision: next })
            }
            UiDocumentCommand::Get {
                session_id,
                document_id,
            } => {
                validate_identity(&session_id)?;
                validate_identity(&document_id)?;
                Ok(UiDocumentResponse::Document {
                    document: self.documents.get(&(session_id, document_id)).cloned(),
                })
            }
            UiDocumentCommand::Dismiss {
                session_id,
                document_id,
                expected_revision,
            } => {
                validate_identity(&session_id)?;
                validate_identity(&document_id)?;
                let key = (session_id, document_id);
                let current = self.documents.get(&key).ok_or("unknown UI document")?;
                if current.revision != expected_revision {
                    return Err("UI document revision conflict".into());
                }
                let revision = expected_revision
                    .checked_add(1)
                    .ok_or("UI document revision exhausted")?;
                self.documents.remove(&key);
                self.tombstones.insert(key, revision);
                Ok(UiDocumentResponse::Dismissed { revision })
            }
        }
    }
}
impl PluginInstance for UiStore {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &interactive_ui_service() {
            return Err(format!("unsupported UI service: {service}"));
        }
        let context = PluginContext::new(host, (), (), ());
        let command = context
            .kernel
            .decode_projected::<UiDocumentCommand>(&UiDocumentInterface::interface_id(), input)
            .map_err(|error| error.to_string())?;
        let response = self.apply(command)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn validate_identity(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 256 || id.trim() != id || id.chars().any(char::is_control) {
        return Err("invalid UI identity".into());
    }
    Ok(())
}
fn validate_text(text: &str, total: &mut usize) -> Result<(), String> {
    if text.len() > MAX_TEXT_BYTES
        || text
            .chars()
            .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
    {
        return Err("invalid UI text".into());
    }
    *total = total.saturating_add(text.len());
    if *total > MAX_DOCUMENT_BYTES {
        return Err("UI document exceeds byte limit".into());
    }
    Ok(())
}
pub fn validate_document(document: &UiDocument) -> Result<(), String> {
    if document.version != 1 {
        return Err("unsupported UI document version".into());
    }
    validate_identity(&document.session_id)?;
    validate_identity(&document.document_id)?;
    validate_identity(&document.root)?;
    if document.nodes.is_empty() || document.nodes.len() > MAX_NODES {
        return Err("UI node count exceeds limits".into());
    }
    let mut index = BTreeMap::new();
    let mut total = 0;
    for node in &document.nodes {
        validate_identity(node.id())?;
        if index.insert(node.id(), node).is_some() {
            return Err("duplicate UI node ID".into());
        }
        match node {
            UiNode::Text { text, .. } | UiNode::Label { text, .. } | UiNode::Badge { text, .. } => {
                validate_text(text, &mut total)?
            }
            UiNode::Progress { text, fraction, .. } => {
                validate_text(text, &mut total)?;
                if !fraction.is_finite() || !(0.0..=1.0).contains(fraction) {
                    return Err("invalid UI progress fraction".into());
                }
            }
            UiNode::Card { title, .. } => validate_text(title, &mut total)?,
            UiNode::Table { columns, rows, .. } => {
                if columns.is_empty() || columns.len() > 12 || rows.len() > 100 {
                    return Err("invalid UI table dimensions".into());
                }
                for column in columns {
                    validate_text(column, &mut total)?;
                }
                for row in rows {
                    if row.len() != columns.len() {
                        return Err("UI table row width mismatch".into());
                    }
                    for cell in row {
                        validate_text(cell, &mut total)?;
                    }
                }
            }
            _ => {}
        }
        if matches!(
            node,
            UiNode::Row { .. } | UiNode::Column { .. } | UiNode::Card { .. }
        ) && node.children().is_empty()
        {
            return Err("empty UI layout".into());
        }
    }
    fn visit(
        id: &str,
        index: &BTreeMap<&str, &UiNode>,
        visited: &mut BTreeSet<String>,
        depth: usize,
    ) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err("UI layout nesting exceeds limit".into());
        }
        if !visited.insert(id.to_owned()) {
            return Err("UI layout shares a node or contains a cycle".into());
        }
        let node = index.get(id).ok_or("unknown UI child node")?;
        for child in node.children() {
            let nested = index.get(child.as_str()).ok_or("unknown UI child node")?;
            if matches!(node, UiNode::Row { .. }) {
                let text = match nested {
                    UiNode::Text { text, .. }
                    | UiNode::Label { text, .. }
                    | UiNode::Badge { text, .. } => text,
                    _ => return Err("UI row accepts only text, labels, and badges".into()),
                };
                if text.contains('\n') {
                    return Err("UI inline cell must have one line".into());
                }
            }
            visit(child, index, visited, depth + 1)?;
        }
        Ok(())
    }
    let mut visited = BTreeSet::new();
    visit(&document.root, &index, &mut visited, 0)?;
    if visited.len() != index.len() {
        return Err("unreachable UI nodes".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> UiDocument {
        UiDocument {
            version: 1,
            session_id: "s1".into(),
            document_id: "d1".into(),
            revision: 1,
            root: "root".into(),
            nodes: vec![
                UiNode::Column {
                    id: "root".into(),
                    children: vec!["title".into(), "status".into()],
                },
                UiNode::Text {
                    id: "title".into(),
                    text: "Build".into(),
                },
                UiNode::Progress {
                    id: "status".into(),
                    text: "Running".into(),
                    fraction: 0.5,
                },
            ],
        }
    }
    #[test]
    fn rejects_unreachable_duplicate_cycle_invalid_fraction_and_escapes() {
        let base = fixture();
        validate_document(&base).unwrap();
        let mut bad = base.clone();
        bad.nodes.push(UiNode::Text {
            id: "orphan".into(),
            text: "".into(),
        });
        assert!(validate_document(&bad).is_err());
        let mut bad = base.clone();
        bad.nodes[1] = UiNode::Text {
            id: "root".into(),
            text: "".into(),
        };
        assert!(validate_document(&bad).is_err());
        let mut bad = base.clone();
        bad.nodes[0] = UiNode::Column {
            id: "root".into(),
            children: vec!["root".into()],
        };
        assert!(validate_document(&bad).is_err());
        let mut bad = base.clone();
        bad.nodes[2] = UiNode::Progress {
            id: "status".into(),
            text: "".into(),
            fraction: f64::NAN,
        };
        assert!(validate_document(&bad).is_err());
        let mut bad = base;
        bad.nodes[1] = UiNode::Text {
            id: "title".into(),
            text: "\u{1b}[2J".into(),
        };
        assert!(validate_document(&bad).is_err());
    }
    #[test]
    fn revision_conflicts_and_tombstones() {
        let mut store = UiStore::default();
        let original = fixture();
        let put = |document, expected_revision| UiDocumentCommand::Put {
            document,
            expected_revision,
        };
        assert_eq!(
            store.apply(put(original.clone(), None)).unwrap(),
            UiDocumentResponse::Stored { revision: 1 }
        );
        assert_eq!(
            store.apply(put(original.clone(), None)).unwrap(),
            UiDocumentResponse::Stored { revision: 1 }
        );
        let mut updated = original.clone();
        updated.revision = 2;
        assert!(store.apply(put(updated.clone(), None)).is_err());
        assert!(store.apply(put(updated.clone(), Some(1))).is_ok());
        assert_eq!(
            store
                .apply(UiDocumentCommand::Dismiss {
                    session_id: "s1".into(),
                    document_id: "d1".into(),
                    expected_revision: 2,
                })
                .unwrap(),
            UiDocumentResponse::Dismissed { revision: 3 }
        );
        assert!(store.apply(put(original, None)).is_err());
        updated.revision = 4;
        assert!(store.apply(put(updated, Some(3))).is_ok());
    }
    #[test]
    fn isolation_and_contract_identity() {
        let mut store = UiStore::default();
        let a = fixture();
        let mut b = a.clone();
        b.session_id = "s2".into();
        store
            .apply(UiDocumentCommand::Put {
                document: a.clone(),
                expected_revision: None,
            })
            .unwrap();
        store
            .apply(UiDocumentCommand::Put {
                document: b.clone(),
                expected_revision: None,
            })
            .unwrap();
        assert_eq!(
            store
                .apply(UiDocumentCommand::Get {
                    session_id: "s1".into(),
                    document_id: "d1".into()
                })
                .unwrap(),
            UiDocumentResponse::Document { document: Some(a) }
        );
        assert_eq!(
            store
                .apply(UiDocumentCommand::Get {
                    session_id: "s2".into(),
                    document_id: "d1".into()
                })
                .unwrap(),
            UiDocumentResponse::Document { document: Some(b) }
        );
        assert_eq!(interactive_ui_manifest().id.as_str(), INTERACTIVE_UI_PLUGIN);
        assert_eq!(interactive_ui_component_manifest().exports.len(), 1);
    }
}
