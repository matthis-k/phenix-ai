//! Graph-v2 authoring IR, V0: typed module exports, sources, invokes and Flow.
//! This is an offline declaration, NOT yet an executable or authority-bearing plan.
//! Graph-v1 is retained for the current kernel until a native v2 parity test exists.
use crate::{identifier, operation_key, Code, Failure, ValueSchema};
use std::collections::{BTreeMap, BTreeSet};

fn invalid(message: impl Into<String>) -> Failure {
    Failure::new(Code::DescriptorInvalid, message)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PortRefV2 {
    pub node: String,
    pub port: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortDeclV2 {
    pub name: String,
    pub schema: ValueSchema,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKindV2 {
    Source { event: String },
    Invoke { operation: String, selector: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeDeclV2 {
    pub id: String,
    pub module: String,
    pub kind: NodeKindV2,
    pub inputs: Vec<PortDeclV2>,
    pub outputs: Vec<PortDeclV2>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleEntryV2 {
    pub name: String,
    pub schema: ValueSchema,
    pub targets: Vec<PortRefV2>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleExitV2 {
    pub name: String,
    pub schema: ValueSchema,
    pub source: PortRefV2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDeclV2 {
    pub id: String,
    pub revision: u32,
    pub members: Vec<String>,
    pub entries: Vec<ModuleEntryV2>,
    pub exits: Vec<ModuleExitV2>,
}

/// Logical edges preserve stable module references and are lowered only after
/// module versions, port schemas and the full graph have been checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutRefV2 {
    Node(PortRefV2),
    ModuleExit { module: String, port: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InRefV2 {
    Node(PortRefV2),
    ModuleEntry { module: String, port: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowV2 {
    pub id: String,
    pub from: OutRefV2,
    pub to: InRefV2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweredFlowV2 {
    pub origin: String,
    pub from: PortRefV2,
    pub to: PortRefV2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphContributionV2 {
    pub contributor: String,
    pub nodes: Vec<NodeDeclV2>,
    pub modules: Vec<ModuleDeclV2>,
    pub flows: Vec<FlowV2>,
}

impl GraphContributionV2 {
    /// Normalize authoring order, then reject invalid and ambiguous declarations.
    /// This V0 check is structural only. It does not grant invocation, event
    /// emission, reading, patching, streaming or native loading authority.
    pub fn normalized(&self) -> Result<Self, Failure> {
        let mut result = self.clone();
        result.nodes.sort_by(|a, b| a.id.cmp(&b.id));
        result.modules.sort_by(|a, b| a.id.cmp(&b.id));
        result.flows.sort_by(|a, b| a.id.cmp(&b.id));
        for node in &mut result.nodes {
            node.inputs.sort_by(|a, b| a.name.cmp(&b.name));
            node.outputs.sort_by(|a, b| a.name.cmp(&b.name));
        }
        for module in &mut result.modules {
            module.members.sort();
            module.entries.sort_by(|a, b| a.name.cmp(&b.name));
            module.exits.sort_by(|a, b| a.name.cmp(&b.name));
            for entry in &mut module.entries {
                entry.targets.sort();
            }
        }
        result.validate()?;
        Ok(result)
    }

    fn node(&self, id: &str) -> Result<&NodeDeclV2, Failure> {
        self.nodes
            .iter()
            .find(|node| node.id == id)
            .ok_or_else(|| invalid(format!("unknown internal node {id}")))
    }

    fn module(&self, id: &str) -> Result<&ModuleDeclV2, Failure> {
        self.modules
            .iter()
            .find(|module| module.id == id)
            .ok_or_else(|| invalid(format!("unknown module {id}")))
    }

    fn port(&self, reference: &PortRefV2, input: bool) -> Result<&ValueSchema, Failure> {
        let node = self.node(&reference.node)?;
        let ports = if input { &node.inputs } else { &node.outputs };
        ports
            .iter()
            .find(|port| port.name == reference.port)
            .map(|port| &port.schema)
            .ok_or_else(|| {
                invalid(format!(
                    "unknown {} port {}.{}",
                    if input { "input" } else { "output" },
                    reference.node,
                    reference.port
                ))
            })
    }

    fn output(&self, from: &OutRefV2) -> Result<(PortRefV2, ValueSchema), Failure> {
        match from {
            OutRefV2::Node(reference) => {
                Ok((reference.clone(), self.port(reference, false)?.clone()))
            }
            OutRefV2::ModuleExit { module, port } => {
                let output = self
                    .module(module)?
                    .exits
                    .iter()
                    .find(|export| &export.name == port)
                    .ok_or_else(|| invalid(format!("unknown module output {module}.{port}")))?;
                Ok((output.source.clone(), output.schema.clone()))
            }
        }
    }

    fn inputs(&self, to: &InRefV2) -> Result<(Vec<PortRefV2>, ValueSchema), Failure> {
        match to {
            InRefV2::Node(reference) => {
                Ok((vec![reference.clone()], self.port(reference, true)?.clone()))
            }
            InRefV2::ModuleEntry { module, port } => {
                let input = self
                    .module(module)?
                    .entries
                    .iter()
                    .find(|export| &export.name == port)
                    .ok_or_else(|| invalid(format!("unknown module input {module}.{port}")))?;
                Ok((input.targets.clone(), input.schema.clone()))
            }
        }
    }

    /// Preserve the logical edge origin. Boundary aliases do not become native
    /// Invoke nodes, roots or waves. Explicit fan-out lowers to multiple Flows.
    pub fn lowered_flows(&self) -> Result<Vec<LoweredFlowV2>, Failure> {
        let mut lowered = Vec::new();
        let mut physical = BTreeSet::new();
        for edge in &self.flows {
            let (from, source_schema) = self.output(&edge.from)?;
            let (targets, target_schema) = self.inputs(&edge.to)?;
            if source_schema != target_schema {
                return Err(invalid(format!("incompatible Flow schema on {}", edge.id)));
            }
            for to in targets {
                if self.port(&from, false)? != self.port(&to, true)? {
                    return Err(invalid(format!("invalid lowered Flow on {}", edge.id)));
                }
                if !physical.insert((from.clone(), to.clone())) {
                    return Err(invalid(format!("duplicate physical Flow on {}", edge.id)));
                }
                lowered.push(LoweredFlowV2 {
                    origin: edge.id.clone(),
                    from: from.clone(),
                    to,
                });
            }
        }
        lowered.sort_by(|a, b| (&a.from, &a.to, &a.origin).cmp(&(&b.from, &b.to, &b.origin)));
        Ok(lowered)
    }

    pub fn validate(&self) -> Result<(), Failure> {
        if !identifier(&self.contributor) || self.contributor == "root" {
            return Err(invalid("invalid graph contributor"));
        }
        let mut modules = BTreeSet::new();
        for module in &self.modules {
            if !identifier(&module.id)
                || module.revision == 0
                || !modules.insert(module.id.as_str())
            {
                return Err(invalid("invalid or duplicate graph module"));
            }
            if !unique(module.members.iter().map(String::as_str))
                || !unique(module.entries.iter().map(|port| port.name.as_str()))
                || !unique(module.exits.iter().map(|port| port.name.as_str()))
            {
                return Err(invalid(format!(
                    "duplicate module member/interface in {}",
                    module.id
                )));
            }
        }
        let mut nodes = BTreeSet::new();
        for node in &self.nodes {
            if !crate::valid_node_key(&node.id)
                || !node.id.starts_with(&format!("{}/", self.contributor))
                || !nodes.insert(node.id.as_str())
            {
                return Err(invalid("invalid or duplicate graph node"));
            }
            let module = self.module(&node.module)?;
            if !module.members.contains(&node.id) {
                return Err(invalid(format!(
                    "node {} is not owned by {}",
                    node.id, module.id
                )));
            }
            if !unique(node.inputs.iter().map(|port| port.name.as_str()))
                || !unique(node.outputs.iter().map(|port| port.name.as_str()))
            {
                return Err(invalid(format!("duplicate input/output in {}", node.id)));
            }
            for port in node.inputs.iter().chain(&node.outputs) {
                if !identifier(&port.name) {
                    return Err(invalid(format!("invalid port in {}", node.id)));
                }
                port.schema.encode()?;
            }
            match &node.kind {
                NodeKindV2::Source { event } => {
                    if !identifier(event)
                        || !node.inputs.is_empty()
                        || node.outputs.len() != 1
                        || node.outputs[0].name != "out"
                    {
                        return Err(invalid(format!("invalid source node {}", node.id)));
                    }
                }
                NodeKindV2::Invoke {
                    operation,
                    selector,
                } => {
                    if !operation_key(operation)
                        || (!selector.is_empty() && !identifier(selector))
                        || node.inputs.len() != 1
                        || node.inputs[0].name != "in"
                        || node.outputs.len() != 2
                        || node.outputs[0].name != "err"
                        || node.outputs[1].name != "ok"
                    {
                        return Err(invalid(format!("invalid invoke node {}", node.id)));
                    }
                }
            }
        }
        for module in &self.modules {
            let members: BTreeSet<&str> = module.members.iter().map(String::as_str).collect();
            let owned: BTreeSet<&str> = self
                .nodes
                .iter()
                .filter(|node| node.module == module.id)
                .map(|node| node.id.as_str())
                .collect();
            if members != owned {
                return Err(invalid(format!(
                    "module {} owns stale or foreign nodes",
                    module.id
                )));
            }
            for entry in &module.entries {
                if !identifier(&entry.name) || entry.targets.is_empty() {
                    return Err(invalid(format!(
                        "invalid module input {}.{}",
                        module.id, entry.name
                    )));
                }
                let mut targets = BTreeSet::new();
                for target in &entry.targets {
                    if !owned.contains(target.node.as_str())
                        || !targets.insert(target)
                        || self.port(target, true)? != &entry.schema
                    {
                        return Err(invalid(format!(
                            "invalid module input target {}.{}",
                            module.id, entry.name
                        )));
                    }
                }
            }
            for exit in &module.exits {
                if !identifier(&exit.name)
                    || !owned.contains(exit.source.node.as_str())
                    || self.port(&exit.source, false)? != &exit.schema
                {
                    return Err(invalid(format!(
                        "invalid module output {}.{}",
                        module.id, exit.name
                    )));
                }
            }
        }
        let mut flows = BTreeSet::new();
        for flow in &self.flows {
            if flow.id.is_empty() || !flows.insert(flow.id.as_str()) {
                return Err(invalid("empty or duplicate logical Flow identity"));
            }
        }
        let lowered = self.lowered_flows()?;
        let mut dependencies: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for node in &self.nodes {
            dependencies.insert(node.id.as_str(), Vec::new());
        }
        for edge in &lowered {
            dependencies
                .get_mut(edge.to.node.as_str())
                .ok_or_else(|| invalid("unknown graph Flow target"))?
                .push(edge.from.node.as_str());
        }
        let mut complete = BTreeSet::new();
        loop {
            let mut progressed = false;
            for (node, parents) in &dependencies {
                if !complete.contains(node)
                    && parents.iter().all(|parent| complete.contains(parent))
                {
                    complete.insert(*node);
                    progressed = true;
                }
            }
            if complete.len() == dependencies.len() {
                break;
            }
            if !progressed {
                return Err(invalid(
                    "instantaneous Flow cycle; use future Deferred::Next",
                ));
            }
        }
        Ok(())
    }
}

fn unique<'a>(keys: impl Iterator<Item = &'a str>) -> bool {
    let keys: Vec<_> = keys.collect();
    let set: BTreeSet<&str> = keys.iter().copied().collect();
    keys.len() == set.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_contribution_is_valid_but_bad_identity_is_not() {
        let graph = GraphContributionV2 {
            contributor: "test.graph".into(),
            nodes: Vec::new(),
            modules: Vec::new(),
            flows: Vec::new(),
        };
        graph.normalized().unwrap();
        let mut invalid = graph;
        invalid.contributor = "root".into();
        assert!(invalid.normalized().is_err());
    }
}
