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
            NodeKind::InvokeEach { .. } => {
                return Err(bad("InvokeEach recursive lowering is unsupported"));
            }
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
        let (a, b) = match &b.target {
            BoundaryTo::Local(p) => (&p.node, &p.port),
            BoundaryTo::Child { child, port } => (child, port),
        };
        charge(&mut bytes, a.len() + b.len() + 1)?;
    }
    for b in &d.outputs {
        let (a, b) = match &b.source {
            BoundaryFrom::Local(p) => (&p.node, &p.port),
            BoundaryFrom::Child { child, port } => (child, port),
        };
        charge(&mut bytes, a.len() + b.len() + 1)?;
    }
    for c in &d.children {
        charge(&mut bytes, c.name.len())?;
        charge(&mut bytes, c.definition.as_str().len())?;
    }
    for a in &d.anchors {
        charge(&mut bytes, a.name.len() + 2)?;
        match &a.target {
            AnchorTarget::LocalNode(node) => charge(&mut bytes, node.len())?,
            AnchorTarget::Child { child, anchor } => {
                charge(&mut bytes, child.len() + anchor.len())?
            }
        }
    }
    for o in &d.orders {
        charge(&mut bytes, o.name.len())?;
        for r in [&o.before, &o.after] {
            match r {
                AnchorRef::Local(name) => charge(&mut bytes, name.len() + 1)?,
                AnchorRef::Child { child, anchor } => {
                    charge(&mut bytes, child.len() + anchor.len() + 1)?
                }
            }
        }
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
    Order(LogicalId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalPort {
    pub node: LogicalId,
    pub port: String,
}

/// Descriptive public alias chain, outermost first, ending at a real body port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryProvenance {
    pub endpoint: LogicalPort,
    pub aliases: Vec<LogicalId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorProvenance {
    pub visibility: AnchorVisibility,
    pub node: LogicalId,
    pub aliases: Vec<LogicalId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderProvenance {
    pub before: Vec<LogicalId>,
    pub after: Vec<LogicalId>,
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
    pub inputs: BTreeMap<LogicalId, BoundaryProvenance>,
    pub outputs: BTreeMap<LogicalId, BoundaryProvenance>,
    pub anchors: BTreeMap<LogicalId, AnchorProvenance>,
    pub orders: BTreeMap<LogicalId, OrderProvenance>,
}

impl ExpandedGraph {
    /// Scoped structural selection for future patches; applies nothing and grants
    /// no authority. These diagnostics are not authenticated untrusted SDK input.
    pub fn resolve_anchor(
        &self,
        scope: &GraphInstanceId,
        reference: &AnchorRef,
    ) -> Result<&AnchorProvenance, Failure> {
        if !self.instances.contains_key(scope) {
            return Err(bad("unknown anchor scope"));
        }
        let (instance, name, public) = match reference {
            AnchorRef::Local(name) => (scope.clone(), name, false),
            AnchorRef::Child { child, anchor } => (scope.child(child)?, anchor, true),
        };
        if !self.instances.contains_key(&instance) {
            return Err(bad("unknown anchor child occurrence"));
        }
        if !identifier(name) {
            return Err(bad("invalid scoped anchor name"));
        }
        let anchor = self
            .anchors
            .get(&LogicalId {
                instance,
                authored: name.clone(),
            })
            .ok_or_else(|| bad("unknown scoped anchor"))?;
        if public && anchor.visibility != AnchorVisibility::Public {
            return Err(bad("private child anchor"));
        }
        Ok(anchor)
    }
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
            d.anchors.len(),
            d.orders.len(),
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
    validate_links(&defs)?;
    let mut output = ExpandedGraph {
        instances: BTreeMap::new(),
        nodes: BTreeMap::new(),
        sources: BTreeMap::new(),
        edges: BTreeMap::new(),
        inputs: BTreeMap::new(),
        outputs: BTreeMap::new(),
        anchors: BTreeMap::new(),
        orders: BTreeMap::new(),
    };
    expand(
        &defs,
        root,
        GraphInstanceId::new(root.clone()),
        budget,
        &mut BTreeSet::new(),
        &mut output,
        &mut ExpansionBytes {
            sizes,
            used: 0,
            items: 0,
        },
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
    items: usize,
}

fn child_definition<'a>(
    defs: &'a BTreeMap<GraphDefinitionId, GraphDefinition>,
    d: &GraphDefinition,
    name: &str,
) -> Result<&'a GraphDefinition, Failure> {
    let index = d
        .children
        .binary_search_by(|c| c.name.as_str().cmp(name))
        .map_err(|_| bad("unknown boundary child"))?;
    defs.get(&d.children[index].definition)
        .ok_or_else(|| bad("unknown child definition"))
}

fn input_contract<'a>(
    defs: &'a BTreeMap<GraphDefinitionId, GraphDefinition>,
    d: &'a GraphDefinition,
    endpoint: &BoundaryTo,
) -> Result<&'a InputPort, Failure> {
    match endpoint {
        BoundaryTo::Local(p) => d
            .body
            .nodes
            .iter()
            .find(|n| n.id == p.node)
            .and_then(|n| n.inputs.iter().find(|i| i.name == p.port))
            .ok_or_else(|| bad("unknown recursive input endpoint")),
        BoundaryTo::Child { child, port } => {
            let child = child_definition(defs, d, child)?;
            let index = child
                .inputs
                .binary_search_by(|b| b.port.name.cmp(port))
                .map_err(|_| bad("unknown child input boundary"))?;
            Ok(&child.inputs[index].port)
        }
    }
}

fn output_contract<'a>(
    defs: &'a BTreeMap<GraphDefinitionId, GraphDefinition>,
    d: &'a GraphDefinition,
    endpoint: &BoundaryFrom,
) -> Result<&'a OutputPort, Failure> {
    match endpoint {
        BoundaryFrom::Local(p) => d
            .body
            .nodes
            .iter()
            .find(|n| n.id == p.node)
            .and_then(|n| n.outputs.iter().find(|o| o.name == p.port))
            .ok_or_else(|| bad("unknown recursive output endpoint")),
        BoundaryFrom::Child { child, port } => {
            let child = child_definition(defs, d, child)?;
            let index = child
                .outputs
                .binary_search_by(|b| b.port.name.cmp(port))
                .map_err(|_| bad("unknown child output boundary"))?;
            Ok(&child.outputs[index].port)
        }
    }
}

fn public_child_anchor<'a>(
    defs: &'a BTreeMap<GraphDefinitionId, GraphDefinition>,
    d: &GraphDefinition,
    child: &str,
    name: &str,
) -> Result<&'a GraphAnchor, Failure> {
    let child = child_definition(defs, d, child)?;
    let index = child
        .anchors
        .binary_search_by(|a| a.name.as_str().cmp(name))
        .map_err(|_| bad("unknown child anchor"))?;
    let anchor = &child.anchors[index];
    if anchor.visibility != AnchorVisibility::Public {
        return Err(bad("private child anchor"));
    }
    Ok(anchor)
}

// Check all links, even in unused definitions, without recursively walking aliases.
fn validate_links(defs: &BTreeMap<GraphDefinitionId, GraphDefinition>) -> Result<(), Failure> {
    let mut degree: BTreeMap<_, usize> = defs.keys().map(|id| (id, 0)).collect();
    let mut parents: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for d in defs.values() {
        for child in &d.children {
            if !defs.contains_key(&child.definition) {
                return Err(bad("unknown child definition"));
            }
            *degree.get_mut(&d.id).unwrap() += 1;
            parents.entry(&child.definition).or_default().push(&d.id);
        }
        for a in &d.anchors {
            if let AnchorTarget::Child { child, anchor } = &a.target {
                public_child_anchor(defs, d, child, anchor)?;
            }
        }
        for o in &d.orders {
            for r in [&o.before, &o.after] {
                if let AnchorRef::Child { child, anchor } = r {
                    public_child_anchor(defs, d, child, anchor)?;
                }
            }
        }
        for b in &d.inputs {
            let p = input_contract(defs, d, &b.target)?;
            if b.port.schema != p.schema
                || b.port.readiness != p.readiness
                || b.port.security != p.security
            {
                return Err(bad(
                    "graph boundary input schema, readiness or security mismatch",
                ));
            }
        }
        for b in &d.outputs {
            let p = output_contract(defs, d, &b.source)?;
            if b.port.schema != p.schema || b.port.security != p.security {
                return Err(bad("graph boundary output schema or security mismatch"));
            }
        }
        for w in &d.wiring {
            let from = output_contract(defs, d, &w.from)?;
            let to = input_contract(defs, d, &w.to)?;
            if from.schema != *to.candidate_schema()? || !from.security.can_flow_to(to.security) {
                return Err(bad("recursive flow schema or security mismatch"));
            }
        }
    }
    let mut ready: BTreeSet<_> = degree
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(id, _)| *id)
        .collect();
    let mut visited = 0;
    while let Some(id) = ready.pop_first() {
        visited += 1;
        for parent in parents.get(id).into_iter().flatten() {
            let n = degree.get_mut(parent).unwrap();
            *n -= 1;
            if *n == 0 {
                ready.insert(*parent);
            }
        }
    }
    if visited != defs.len() {
        return Err(bad("recursive definition cycle"));
    }
    Ok(())
}

fn reserve_items(
    bytes: &mut ExpansionBytes,
    n: usize,
    budget: ExpansionBudget,
) -> Result<(), Failure> {
    bytes.items = bytes
        .items
        .checked_add(n)
        .filter(|n| *n <= budget.items)
        .ok_or_else(|| bad("recursive expanded item budget"))?;
    Ok(())
}

fn charge_logical(bytes: &mut usize, id: &LogicalId) -> Result<(), Failure> {
    charge(
        bytes,
        id.instance.root().as_str().len() + id.authored.len() + 8,
    )?;
    for part in id.instance.path() {
        charge(bytes, part.len() + 4)?;
    }
    Ok(())
}

fn retain_boundary(
    id: LogicalId,
    endpoint: &LogicalPort,
    inner: &[LogicalId],
    bytes: &mut ExpansionBytes,
    budget: ExpansionBudget,
) -> Result<BoundaryProvenance, Failure> {
    reserve_items(bytes, inner.len() + 1, budget)?;
    for alias in std::iter::once(&id)
        .chain(std::iter::once(&id))
        .chain(inner)
        .chain(std::iter::once(&endpoint.node))
    {
        charge_logical(&mut bytes.used, alias)?;
    }
    charge(&mut bytes.used, endpoint.port.len())?;
    let aliases = std::iter::once(id).chain(inner.iter().cloned()).collect();
    Ok(BoundaryProvenance {
        endpoint: endpoint.clone(),
        aliases,
    })
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
    let added = d.body.nodes.len()
        + d.body.sources.len()
        + d.body.edges.len()
        + d.wiring.len()
        + d.orders.len();
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
    reserve_items(bytes, added, budget)?;
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
    // Base items, including wiring, were reserved before descending; emit body
    // edges now and resolve aliases/wiring only after child maps are complete.
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
    // Child maps are complete before resolving parent aliases. No shared
    // occurrence identity or recursive alias traversal is introduced.
    for a in &d.anchors {
        let local;
        let (node, inner): (_, &[LogicalId]) = match &a.target {
            AnchorTarget::LocalNode(node) => {
                local = logical(node);
                (&local, &[])
            }
            AnchorTarget::Child { child, anchor } => {
                let inner = out
                    .anchors
                    .get(&LogicalId {
                        instance: instance.child(child)?,
                        authored: anchor.clone(),
                    })
                    .ok_or_else(|| bad("unknown child anchor"))?;
                if inner.visibility != AnchorVisibility::Public {
                    return Err(bad("private child anchor"));
                }
                (&inner.node, inner.aliases.as_slice())
            }
        };
        let id = logical(&a.name);
        reserve_items(bytes, inner.len() + 1, budget)?;
        for part in std::iter::once(&id)
            .chain(std::iter::once(&id))
            .chain(inner)
            .chain(std::iter::once(node))
        {
            charge_logical(&mut bytes.used, part)?;
        }
        let provenance = AnchorProvenance {
            visibility: a.visibility,
            node: node.clone(),
            aliases: std::iter::once(id.clone())
                .chain(inner.iter().cloned())
                .collect(),
        };
        out.anchors.insert(id, provenance);
    }
    for o in &d.orders {
        let before = out.resolve_anchor(&instance, &o.before)?;
        let after = out.resolve_anchor(&instance, &o.after)?;
        reserve_items(bytes, before.aliases.len() + after.aliases.len(), budget)?;
        let id = logical(&o.name);
        for part in
            before
                .aliases
                .iter()
                .chain(&after.aliases)
                .chain([&id, &before.node, &after.node])
        {
            charge_logical(&mut bytes.used, part)?;
        }
        let edge = LogicalEdge::Order {
            before: before.node.clone(),
            after: after.node.clone(),
        };
        let provenance = OrderProvenance {
            before: before.aliases.clone(),
            after: after.aliases.clone(),
        };
        out.edges.insert(EdgeOrigin::Order(id.clone()), edge);
        out.orders.insert(id, provenance);
    }
    for b in &d.inputs {
        let local;
        let (endpoint, aliases): (_, &[LogicalId]) = match &b.target {
            BoundaryTo::Local(p) => {
                local = LogicalPort {
                    node: logical(&p.node),
                    port: p.port.clone(),
                };
                (&local, &[])
            }
            BoundaryTo::Child { child, port } => {
                let key = LogicalId {
                    instance: instance.child(child)?,
                    authored: port.clone(),
                };
                let b = out
                    .inputs
                    .get(&key)
                    .ok_or_else(|| bad("unknown child input boundary"))?;
                (&b.endpoint, &b.aliases)
            }
        };
        let id = logical(&b.port.name);
        let provenance = retain_boundary(id.clone(), endpoint, aliases, bytes, budget)?;
        out.inputs.insert(id, provenance);
    }
    for b in &d.outputs {
        let local;
        let (endpoint, aliases): (_, &[LogicalId]) = match &b.source {
            BoundaryFrom::Local(p) => {
                local = LogicalPort {
                    node: logical(&p.node),
                    port: p.port.clone(),
                };
                (&local, &[])
            }
            BoundaryFrom::Child { child, port } => {
                let key = LogicalId {
                    instance: instance.child(child)?,
                    authored: port.clone(),
                };
                let b = out
                    .outputs
                    .get(&key)
                    .ok_or_else(|| bad("unknown child output boundary"))?;
                (&b.endpoint, &b.aliases)
            }
        };
        let id = logical(&b.port.name);
        let provenance = retain_boundary(id.clone(), endpoint, aliases, bytes, budget)?;
        out.outputs.insert(id, provenance);
    }
    for wire in &d.wiring {
        let from = match &wire.from {
            BoundaryFrom::Local(p) => LogicalPort {
                node: logical(&p.node),
                port: p.port.clone(),
            },
            BoundaryFrom::Child { child, port } => out
                .outputs
                .get(&LogicalId {
                    instance: instance.child(child)?,
                    authored: port.clone(),
                })
                .ok_or_else(|| bad("unknown child output boundary"))?
                .endpoint
                .clone(),
        };
        let to = match &wire.to {
            BoundaryTo::Local(p) => LogicalPort {
                node: logical(&p.node),
                port: p.port.clone(),
            },
            BoundaryTo::Child { child, port } => out
                .inputs
                .get(&LogicalId {
                    instance: instance.child(child)?,
                    authored: port.clone(),
                })
                .ok_or_else(|| bad("unknown child input boundary"))?
                .endpoint
                .clone(),
        };
        out.edges.insert(
            EdgeOrigin::Wiring(logical(&wire.name)),
            match wire.next {
                Some(next) => LogicalEdge::Deferred { from, to, next },
                None => LogicalEdge::Flow { from, to },
            },
        );
    }
    active.remove(id);
    Ok(())
}
