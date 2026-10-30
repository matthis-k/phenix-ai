//! C1 structural composition, not an artifact codec or authority proof.
use super::{EdgeKind, GraphContributionV2, InPortRef, InputPort, OutPortRef, OutputPort};
use crate::graph::{bad, sorted};
use crate::{identifier, Failure};

/// Stable qualified definition name; construction rejects ambiguous spellings.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GraphDefinitionId(String);

impl GraphDefinitionId {
    pub fn new(name: &str) -> Result<Self, Failure> {
        let (owner, local) = name
            .split_once('/')
            .ok_or_else(|| bad("graph definition requires owner/local"))?;
        if owner == "root" || !identifier(owner) || !identifier(local) {
            return Err(bad("invalid qualified graph definition name"));
        }
        Ok(Self(name.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn owner(&self) -> &str {
        self.0.split_once('/').unwrap().0
    }
}

/// Composition-qualified root occurrence and explicit nested occurrence path.
/// This is neither a flat NodeId nor a runtime activation identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GraphInstanceId {
    root: GraphDefinitionId,
    path: Vec<String>,
}

impl GraphInstanceId {
    pub fn new(root: GraphDefinitionId) -> Self {
        Self { root, path: vec![] }
    }

    pub fn child(&self, name: &str) -> Result<Self, Failure> {
        if !identifier(name) {
            return Err(bad("invalid graph instance child name"));
        }
        let mut child = self.clone();
        child.path.push(name.into());
        Ok(child)
    }

    pub fn root(&self) -> &GraphDefinitionId {
        &self.root
    }

    pub fn path(&self) -> &[String] {
        &self.path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphInstance {
    pub id: GraphInstanceId,
    pub definition: GraphDefinitionId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryInput {
    pub port: InputPort,
    pub target: BoundaryTo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryOutput {
    pub port: OutputPort,
    pub source: BoundaryFrom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorVisibility {
    Public,
    Private,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorTarget {
    LocalNode(String),
    Child { child: String, anchor: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphAnchor {
    pub name: String,
    pub visibility: AnchorVisibility,
    pub target: AnchorTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorRef {
    Local(String),
    Child { child: String, anchor: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryOrder {
    pub name: String,
    pub before: AnchorRef,
    pub after: AnchorRef,
}

/// Closed-body definition with explicit immediate-child placement and aliases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphDefinition {
    pub id: GraphDefinitionId,
    pub body: GraphContributionV2,
    pub inputs: Vec<BoundaryInput>,
    pub outputs: Vec<BoundaryOutput>,
    pub children: Vec<GraphChild>,
    pub wiring: Vec<BoundaryFlow>,
    pub anchors: Vec<GraphAnchor>,
    pub orders: Vec<BoundaryOrder>,
}

impl GraphDefinition {
    pub fn normalized(&self) -> Result<Self, Failure> {
        let mut result = self.clone();
        result.body = result.body.normalized()?;
        result.inputs.sort_by(|a, b| a.port.name.cmp(&b.port.name));
        result.outputs.sort_by(|a, b| a.port.name.cmp(&b.port.name));
        result.children.sort_by(|a, b| a.name.cmp(&b.name));
        result.wiring.sort_by(|a, b| a.name.cmp(&b.name));
        result.anchors.sort_by(|a, b| a.name.cmp(&b.name));
        result.orders.sort_by(|a, b| a.name.cmp(&b.name));
        result.validate()?;
        Ok(result)
    }

    /// Shape validation only. Does not authenticate the contributor or grant use.
    pub fn validate(&self) -> Result<(), Failure> {
        self.body.validate()?;
        if !sorted(self.children.iter().map(|c| c.name.as_str()))
            || !sorted(self.wiring.iter().map(|w| w.name.as_str()))
            || self.children.iter().any(|c| !identifier(&c.name))
            || self.wiring.iter().any(|w| !identifier(&w.name))
        {
            return Err(bad("invalid child or boundary wire identity"));
        }
        if self.id.owner() != self.body.contributor
            || !sorted(self.inputs.iter().map(|p| p.port.name.as_str()))
            || !sorted(self.outputs.iter().map(|p| p.port.name.as_str()))
        {
            return Err(bad("graph definition owner, boundary order or duplicate"));
        }
        let local = |id: &str| {
            self.body
                .nodes
                .binary_search_by(|n| n.id.as_str().cmp(id))
                .is_ok()
        };
        if !sorted(self.anchors.iter().map(|a| a.name.as_str()))
            || !sorted(self.orders.iter().map(|o| o.name.as_str()))
            || self.anchors.iter().any(|a| !identifier(&a.name))
            || self.orders.iter().any(|o| !identifier(&o.name))
        {
            return Err(bad("invalid anchor or Order identity"));
        }
        let child = |name: &str, anchor: &str| {
            identifier(anchor)
                && self
                    .children
                    .binary_search_by(|c| c.name.as_str().cmp(name))
                    .is_ok()
        };
        for anchor in &self.anchors {
            let valid = match &anchor.target {
                AnchorTarget::LocalNode(node) => local(node),
                AnchorTarget::Child {
                    child: name,
                    anchor,
                } => child(name, anchor),
            };
            if !valid {
                return Err(bad("unknown anchor node or child"));
            }
        }
        for order in &self.orders {
            for reference in [&order.before, &order.after] {
                let valid = match reference {
                    AnchorRef::Local(name) => {
                        identifier(name)
                            && self.anchors.binary_search_by(|a| a.name.cmp(name)).is_ok()
                    }
                    AnchorRef::Child {
                        child: name,
                        anchor,
                    } => child(name, anchor),
                };
                if !valid {
                    return Err(bad("unknown scoped Order anchor"));
                }
            }
        }
        for edge in &self.body.edges {
            let (from, to) = match &edge.kind {
                EdgeKind::Flow { from, to } | EdgeKind::Deferred { from, to, .. } => {
                    (&from.node, &to.node)
                }
                EdgeKind::Order { before, after } => (before, after),
            };
            if !local(from) || !local(to) {
                return Err(bad("graph definition has unexposed body crossing"));
            }
        }
        for boundary in &self.inputs {
            if !identifier(&boundary.port.name) {
                return Err(bad("invalid graph input boundary name"));
            }
            let target = match &boundary.target {
                BoundaryTo::Local(p) => p,
                BoundaryTo::Child { child, port } => {
                    if !identifier(port) || !self.children.iter().any(|c| c.name == *child) {
                        return Err(bad("unknown graph input boundary child or alias"));
                    }
                    continue;
                }
            };
            let endpoint = self
                .body
                .nodes
                .iter()
                .find(|n| n.id == target.node)
                .and_then(|n| n.inputs.iter().find(|p| p.name == target.port))
                .ok_or_else(|| bad("unknown graph boundary input endpoint"))?;
            let mut expected = endpoint.clone();
            expected.name = boundary.port.name.clone();
            if !identifier(&boundary.port.name) || boundary.port != expected {
                return Err(bad(
                    "graph boundary input schema, readiness or security mismatch",
                ));
            }
        }
        for boundary in &self.outputs {
            if !identifier(&boundary.port.name) {
                return Err(bad("invalid graph output boundary name"));
            }
            let source = match &boundary.source {
                BoundaryFrom::Local(p) => p,
                BoundaryFrom::Child { child, port } => {
                    if !identifier(port) || !self.children.iter().any(|c| c.name == *child) {
                        return Err(bad("unknown graph output boundary child or alias"));
                    }
                    continue;
                }
            };
            let endpoint = self
                .body
                .nodes
                .iter()
                .find(|n| n.id == source.node)
                .and_then(|n| n.outputs.iter().find(|p| p.name == source.port))
                .ok_or_else(|| bad("unknown graph boundary output endpoint"))?;
            let mut expected = endpoint.clone();
            expected.name = boundary.port.name.clone();
            if !identifier(&boundary.port.name) || boundary.port != expected {
                return Err(bad("graph boundary output schema or security mismatch"));
            }
        }
        Ok(())
    }
}

mod lowering;
pub use lowering::*;

#[cfg(test)]
mod tests;
