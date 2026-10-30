//! Unauthenticated structural output deliberately has no flat artifact encoding.
use super::*;
use crate::graph::v2::{NodeDecl, SourceDecl};
use crate::{MAX_BYTES, MAX_COUNT};
use std::collections::{BTreeMap, BTreeSet};

// Walk borrowed data before normalization can clone any caller-controlled trees.
fn charge(bytes: &mut usize, n: usize) -> Result<(), Failure> {
    *bytes = bytes
        .checked_add(n)
        .filter(|n| *n <= MAX_BYTES)
        .ok_or_else(|| bad("recursive byte budget"))?;
    Ok(())
}

#[cfg(test)]
#[test]
fn byte_charge_boundaries() {
    let mut bytes = MAX_BYTES - 1;
    charge(&mut bytes, 1).unwrap();
    assert!(charge(&mut bytes, 1).is_err());
    assert!(charge(&mut bytes, usize::MAX).is_err());
}

fn schema_bytes(s: &crate::ValueSchema, bytes: &mut usize, depth: usize) -> Result<(), Failure> {
    use crate::ValueSchema::*;
    if depth > crate::MAX_DEPTH {
        return Err(bad("recursive schema depth"));
    }
    charge(bytes, 1)?;
    match s {
        List(s) | Option(s) => schema_bytes(s, bytes, depth + 1)?,
        Struct(fields) => {
            for (name, s) in fields {
                charge(bytes, name.len() + 4)?;
                schema_bytes(s, bytes, depth + 1)?;
            }
        }
        Tagged(fields) => {
            for (name, s) in fields {
                charge(bytes, name.len() + 4)?;
                if let Some(s) = s {
                    schema_bytes(s, bytes, depth + 1)?;
                }
            }
        }
        Variants(names) => {
            for name in names {
                charge(bytes, name.len() + 4)?;
            }
        }
        Ref(_) => charge(bytes, 32)?,
        _ => {}
    }
    Ok(())
}

fn value_bytes(v: &crate::Value, bytes: &mut usize, depth: usize) -> Result<(), Failure> {
    use crate::Value::*;
    if depth > crate::MAX_DEPTH {
        return Err(bad("recursive literal depth"));
    }
    charge(bytes, 5)?;
    match v {
        String(s) => charge(bytes, s.len())?,
        Bytes(s) => charge(bytes, s.len())?,
        List(items) => {
            for v in items {
                value_bytes(v, bytes, depth + 1)?;
            }
        }
        Map(fields) => {
            for (name, v) in fields {
                charge(bytes, name.len() + 4)?;
                value_bytes(v, bytes, depth + 1)?;
            }
        }
        Ref(_) => charge(bytes, 112)?,
        I64(_) | U64(_) => charge(bytes, 8)?,
        _ => {}
    }
    Ok(())
}

fn definition_bytes(d: &GraphDefinition) -> Result<usize, Failure> {
    use crate::graph::v2::{NodeKind, SourceKind};
    let mut bytes = 0;
    charge(&mut bytes, d.id.as_str().len())?;
    charge(&mut bytes, d.body.contributor.len())?;
    for s in &d.body.sources {
        charge(&mut bytes, s.id.len())?;
        charge(&mut bytes, s.owner.len())?;
        match &s.kind {
            SourceKind::ExternalAction { action: s }
            | SourceKind::CustomEvent { event: s }
            | SourceKind::KernelEvent { event: s }
            | SourceKind::CommittedWatch { property: s } => charge(&mut bytes, s.len())?,
            SourceKind::DeferredEntry => {}
        }
        schema_bytes(&s.payload, &mut bytes, 0)?;
    }
    for n in &d.body.nodes {
        charge(&mut bytes, n.id.len())?;
        match &n.kind {
            NodeKind::Invoke {
                selector,
                operation,
            } => {
                charge(&mut bytes, selector.len())?;
                charge(&mut bytes, operation.len())?;
            }
            NodeKind::Source { source } => charge(&mut bytes, source.len())?,
            NodeKind::Constant { value } => value_bytes(value, &mut bytes, 0)?,
            _ => {}
        }
    }
    for (name, schema) in d
        .body
        .nodes
        .iter()
        .flat_map(|n| {
            n.inputs
                .iter()
                .map(|p| (&p.name, &p.schema))
                .chain(n.outputs.iter().map(|p| (&p.name, &p.schema)))
        })
        .chain(d.inputs.iter().map(|b| (&b.port.name, &b.port.schema)))
        .chain(d.outputs.iter().map(|b| (&b.port.name, &b.port.schema)))
    {
        charge(&mut bytes, name.len())?;
        schema_bytes(schema, &mut bytes, 0)?;
    }
    for b in &d.inputs {
        charge(&mut bytes, b.target.node.len())?;
        charge(&mut bytes, b.target.port.len())?;
    }
    for b in &d.outputs {
        charge(&mut bytes, b.source.node.len())?;
        charge(&mut bytes, b.source.port.len())?;
    }
    for c in &d.children {
        charge(&mut bytes, c.name.len())?;
        charge(&mut bytes, c.definition.as_str().len())?;
    }
    for e in &d.body.edges {
        charge(&mut bytes, e.id.len())?;
        match &e.kind {
            EdgeKind::Flow { from, to } | EdgeKind::Deferred { from, to, .. } => {
                for s in [&from.node, &from.port, &to.node, &to.port] {
                    charge(&mut bytes, s.len())?;
                }
            }
            EdgeKind::Order { before, after } => {
                charge(&mut bytes, before.len())?;
                charge(&mut bytes, after.len())?;
            }
        }
    }
    for w in &d.wiring {
        charge(&mut bytes, w.name.len())?;
        let (a, b) = match &w.from {
            BoundaryFrom::Local(p) => (&p.node, &p.port),
            BoundaryFrom::Child { child, port } => (child, port),
        };
        charge(&mut bytes, a.len())?;
        charge(&mut bytes, b.len())?;
        let (a, b) = match &w.to {
            BoundaryTo::Local(p) => (&p.node, &p.port),
            BoundaryTo::Child { child, port } => (child, port),
        };
        charge(&mut bytes, a.len())?;
        charge(&mut bytes, b.len())?;
    }
    Ok(bytes)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphChild {
    pub name: String,
    pub definition: GraphDefinitionId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundaryFrom {
    Local(OutPortRef),
    Child { child: String, port: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundaryTo {
    Local(InPortRef),
    Child { child: String, port: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryFlow {
    pub name: String,
    pub from: BoundaryFrom,
    pub to: BoundaryTo,
    /// None delivers in the current wave; Some crosses to the specified wave.
    pub next: Option<crate::graph::v2::DeferredWave>,
}

/// Structural identity; the authored ID is never rewritten into an owner name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LogicalId {
    pub instance: GraphInstanceId,
    pub authored: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeOrigin {
    Body(LogicalId),
    Wiring(LogicalId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalPort {
    pub node: LogicalId,
    pub port: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogicalEdge {
    Flow {
        from: LogicalPort,
        to: LogicalPort,
    },
    Deferred {
        from: LogicalPort,
        to: LogicalPort,
        next: crate::graph::v2::DeferredWave,
    },
    Order {
        before: LogicalId,
        after: LogicalId,
    },
}

/// Provenance is descriptive, not authenticated. Node/source declarations retain
/// their original IDs; references inside them are scoped to their occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandedGraph {
    pub instances: BTreeMap<GraphInstanceId, GraphDefinitionId>,
    pub nodes: BTreeMap<LogicalId, NodeDecl>,
    pub sources: BTreeMap<LogicalId, SourceDecl>,
    pub edges: BTreeMap<EdgeOrigin, LogicalEdge>,
}

#[derive(Debug, Clone, Copy)]
pub struct ExpansionBudget {
    pub depth: usize,
    pub occurrences: usize,
    pub items: usize,
}

/// Pure bounded occurrence expansion. This does not prepare executable graphs,
/// verify packages, resolve operations, authorize placement or admit readiness.
pub fn lower_recursive(
    definitions: &[GraphDefinition],
    root: &GraphDefinitionId,
    budget: ExpansionBudget,
) -> Result<ExpandedGraph, Failure> {
    if budget.depth == 0
        || budget.depth > 64
        || budget.occurrences == 0
        || budget.occurrences > MAX_COUNT
        || budget.items > MAX_COUNT
    {
        return Err(bad("invalid recursive expansion budget"));
    }
    // Count before cloning or sorting caller-controlled collections.
    let mut count = definitions.len();
    for d in definitions {
        for n in [
            d.body.nodes.len(),
            d.body.sources.len(),
            d.body.edges.len(),
            d.inputs.len(),
            d.outputs.len(),
            d.children.len(),
            d.wiring.len(),
        ]
        .into_iter()
        .chain(
            d.body
                .nodes
                .iter()
                .flat_map(|n| [n.inputs.len(), n.outputs.len()]),
        ) {
            count = count
                .checked_add(n)
                .filter(|n| *n <= MAX_COUNT)
                .ok_or_else(|| bad("recursive input count budget"))?;
        }
    }
    if count > MAX_COUNT {
        return Err(bad("recursive input count budget"));
    }
    let mut defs = BTreeMap::new();
    let mut bytes = 0usize;
    for d in definitions {
        charge(&mut bytes, definition_bytes(d)?)?;
    }
    let mut sizes = BTreeMap::new();
    bytes = 0;
    for d in definitions {
        let metadata_bytes = definition_bytes(d)?;
        let d = d.normalized()?;
        // Include canonical framing as well as metadata not present in the body.
        let size = metadata_bytes
            .checked_add(d.body.encode()?.len())
            .ok_or_else(|| bad("recursive input byte budget"))?;
        charge(&mut bytes, size)?;
        sizes.insert(d.id.clone(), size);
        if defs.insert(d.id.clone(), d).is_some() {
            return Err(bad("duplicate recursive definition"));
        }
    }
    if !defs.contains_key(root) {
        return Err(bad("unknown recursive definition"));
    }
    let mut output = ExpandedGraph {
        instances: BTreeMap::new(),
        nodes: BTreeMap::new(),
        sources: BTreeMap::new(),
        edges: BTreeMap::new(),
    };
    expand(
        &defs,
        root,
        GraphInstanceId::new(root.clone()),
        budget,
        &mut BTreeSet::new(),
        &mut output,
        &mut ExpansionBytes { sizes, used: 0 },
    )?;

    // Kahn's algorithm includes same-wave Flow and Order dependencies, not
    // Deferred admissions. Parallel edges count separately; no graph recursion.
    let mut degree: BTreeMap<_, usize> = output.nodes.keys().cloned().map(|n| (n, 0)).collect();
    let mut next: BTreeMap<LogicalId, Vec<LogicalId>> = BTreeMap::new();
    for edge in output.edges.values() {
        let (from, to) = match edge {
            LogicalEdge::Flow { from, to } | LogicalEdge::Deferred { from, to, .. } => {
                let source = output
                    .nodes
                    .get(&from.node)
                    .and_then(|n| n.outputs.iter().find(|p| p.name == from.port))
                    .ok_or_else(|| bad("unknown recursive output endpoint"))?;
                let target = output
                    .nodes
                    .get(&to.node)
                    .and_then(|n| n.inputs.iter().find(|p| p.name == to.port))
                    .ok_or_else(|| bad("unknown recursive input endpoint"))?;
                if source.schema != *target.candidate_schema()?
                    || !source.security.can_flow_to(target.security)
                {
                    return Err(bad("recursive flow schema or security mismatch"));
                }
                if matches!(edge, LogicalEdge::Deferred { .. }) {
                    continue;
                }
                (&from.node, &to.node)
            }
            LogicalEdge::Order { before, after } => {
                if !output.nodes.contains_key(before) || !output.nodes.contains_key(after) {
                    return Err(bad("unknown recursive Order endpoint"));
                }
                (before, after)
            }
        };
        *degree
            .get_mut(to)
            .ok_or_else(|| bad("unknown dependency target"))? += 1;
        next.entry(from.clone()).or_default().push(to.clone());
    }
    let mut ready: BTreeSet<_> = degree
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(n, _)| n.clone())
        .collect();
    let mut visited = 0;
    while let Some(node) = ready.pop_first() {
        visited += 1;
        for to in next.get(&node).into_iter().flatten() {
            let n = degree.get_mut(to).unwrap();
            *n -= 1;
            if *n == 0 {
                ready.insert(to.clone());
            }
        }
    }
    if visited != degree.len() {
        return Err(bad("recursive dependency cycle"));
    }
    Ok(output)
}

struct ExpansionBytes {
    sizes: BTreeMap<GraphDefinitionId, usize>,
    used: usize,
}

fn expand(
    defs: &BTreeMap<GraphDefinitionId, GraphDefinition>,
    id: &GraphDefinitionId,
    instance: GraphInstanceId,
    budget: ExpansionBudget,
    active: &mut BTreeSet<GraphDefinitionId>,
    out: &mut ExpandedGraph,
    bytes: &mut ExpansionBytes,
) -> Result<(), Failure> {
    if instance.path().len() >= budget.depth || out.instances.len() >= budget.occurrences {
        return Err(bad("recursive occurrence or depth budget"));
    }
    if !active.insert(id.clone()) {
        return Err(bad("recursive definition cycle"));
    }
    let d = defs
        .get(id)
        .ok_or_else(|| bad("unknown recursive definition"))?;
    let added = d.body.nodes.len() + d.body.sources.len() + d.body.edges.len() + d.wiring.len();
    charge(&mut bytes.used, bytes.sizes[id])?;
    // Keys and dependency checking clone occurrence paths a bounded number of
    // times per item; count paths even for empty occurrences.
    let mut path_bytes = instance.root().as_str().len();
    for p in instance.path() {
        charge(&mut path_bytes, p.len() + 4)?;
    }
    let identity_bytes = path_bytes
        .checked_mul(1 + added * 8)
        .ok_or_else(|| bad("recursive expanded byte budget"))?;
    charge(&mut bytes.used, identity_bytes)?;
    if added
        > budget
            .items
            .saturating_sub(out.nodes.len() + out.sources.len() + out.edges.len())
    {
        return Err(bad("recursive expanded item budget"));
    }
    out.instances.insert(instance.clone(), id.clone());
    let logical = |authored: &str| LogicalId {
        instance: instance.clone(),
        authored: authored.into(),
    };
    for node in &d.body.nodes {
        out.nodes.insert(logical(&node.id), node.clone());
    }
    for source in &d.body.sources {
        out.sources.insert(logical(&source.id), source.clone());
    }
    // Reserve this occurrence's edges before descending so descendant budgets
    // account for all ancestors, not just their already-emitted nodes.
    for edge in &d.body.edges {
        let lowered = match &edge.kind {
            EdgeKind::Flow { from, to } => LogicalEdge::Flow {
                from: LogicalPort {
                    node: logical(&from.node),
                    port: from.port.clone(),
                },
                to: LogicalPort {
                    node: logical(&to.node),
                    port: to.port.clone(),
                },
            },
            EdgeKind::Order { before, after } => LogicalEdge::Order {
                before: logical(before),
                after: logical(after),
            },
            EdgeKind::Deferred { from, to, next } => LogicalEdge::Deferred {
                from: LogicalPort {
                    node: logical(&from.node),
                    port: from.port.clone(),
                },
                to: LogicalPort {
                    node: logical(&to.node),
                    port: to.port.clone(),
                },
                next: *next,
            },
        };
        out.edges
            .insert(EdgeOrigin::Body(logical(&edge.id)), lowered);
    }
    for wire in &d.wiring {
        let child = |name: &str| -> Result<(&GraphDefinition, GraphInstanceId), Failure> {
            let c = d
                .children
                .iter()
                .find(|c| c.name == name)
                .ok_or_else(|| bad("unknown boundary child"))?;
            Ok((
                defs.get(&c.definition)
                    .ok_or_else(|| bad("unknown child definition"))?,
                instance.child(name)?,
            ))
        };
        let from = match &wire.from {
            BoundaryFrom::Local(p) => LogicalPort {
                node: logical(&p.node),
                port: p.port.clone(),
            },
            BoundaryFrom::Child { child: name, port } => {
                let (def, occurrence) = child(name)?;
                let p = &def
                    .outputs
                    .iter()
                    .find(|b| b.port.name == *port)
                    .ok_or_else(|| bad("unknown child output boundary"))?
                    .source;
                LogicalPort {
                    node: LogicalId {
                        instance: occurrence,
                        authored: p.node.clone(),
                    },
                    port: p.port.clone(),
                }
            }
        };
        let to = match &wire.to {
            BoundaryTo::Local(p) => LogicalPort {
                node: logical(&p.node),
                port: p.port.clone(),
            },
            BoundaryTo::Child { child: name, port } => {
                let (def, occurrence) = child(name)?;
                let p = &def
                    .inputs
                    .iter()
                    .find(|b| b.port.name == *port)
                    .ok_or_else(|| bad("unknown child input boundary"))?
                    .target;
                LogicalPort {
                    node: LogicalId {
                        instance: occurrence,
                        authored: p.node.clone(),
                    },
                    port: p.port.clone(),
                }
            }
        };
        out.edges.insert(
            EdgeOrigin::Wiring(logical(&wire.name)),
            match wire.next {
                Some(next) => LogicalEdge::Deferred { from, to, next },
                None => LogicalEdge::Flow { from, to },
            },
        );
    }
    for c in &d.children {
        expand(
            defs,
            &c.definition,
            instance.child(&c.name)?,
            budget,
            active,
            out,
            bytes,
        )?;
    }
    active.remove(id);
    Ok(())
}
