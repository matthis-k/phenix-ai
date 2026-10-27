//! Offline base-profile compilation and private initial-wave readiness inside
//! the existing GraphExecution. Supports ExternalAction, Source, Invoke, Constant,
//! Switch, Merge, Join, Flow and Order. No patches or alternate runtime.
use crate::{dag, resolve::Prepared};
use phenix_contract::{graph::v2::*, Code, Failure, OperationSchema, Value, MAX_BYTES, MAX_COUNT};
use std::collections::{BTreeMap, BTreeSet};
pub(super) mod execute;

/// Base-profile longest Flow+Order path, counting vertices. Bounds recursion in
/// the shared DAG sorter without changing its established deterministic order.
pub const MAX_GRAPH_DEPTH: usize = 256;

/// Trusted host policy, never derived from authored graph labels. The host must
/// authenticate these attestations independently; current package metadata does
/// not attest port security. Missing entries deny compilation.
#[derive(Debug, Clone, Default)]
pub struct TrustedCompositionPolicy {
    /// (edge contributor, affected foreign authored node). This authorizes
    /// topology only, never delivery or invocation. Local endpoints need none.
    pub topology: BTreeSet<(String, String)>,
    pub source_owners: BTreeMap<String, SourceOwnerPolicy>,
    /// Exact source ID -> attested security (including integrity).
    pub sources: BTreeMap<String, PortSecurity>,
    /// (selected provider, operation key) -> attested native port labels.
    pub operations: BTreeMap<(String, String), OperationSecurity>,
    /// Exact (output node, output port, input node, input port).
    pub delivery: BTreeMap<(String, String, String, String), FlowGrant>,
}

#[derive(Debug, Clone)]
pub struct SourceOwnerPolicy {
    pub owner_ceiling: Classification,
    pub composition_ceiling: Classification,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationSecurity {
    pub input: PortSecurity,
    pub ok: PortSecurity,
    pub err: PortSecurity,
}

#[derive(Debug, Clone)]
pub struct FlowGrant {
    pub resources: BTreeSet<String>,
    pub maximum_classification: Classification,
}

/// Bounded local analysis, not a solver for arbitrary operation behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Satisfiability {
    ProvenSatisfiable,
    Dormant,
    Unknown,
}

/// Immutable potential-route plan. Delivery resources are static bounds, not
/// effective root authority or permission to dereference a transported reference.
#[derive(Debug, Clone)]
pub struct CompiledGraph {
    identity: String,
    declarations: Vec<GraphContributionV2>,
    routes: BTreeMap<String, String>,
    predecessors: BTreeMap<String, BTreeSet<String>>,
    source_reachability: BTreeMap<String, BTreeSet<String>>,
    join_satisfiability: BTreeMap<String, Satisfiability>,
    order: Vec<String>,
    policy: TrustedCompositionPolicy,
}

impl CompiledGraph {
    /// Deterministic Join diagnostics keyed by authored node identity.
    pub fn join_satisfiability(&self) -> &BTreeMap<String, Satisfiability> {
        &self.join_satisfiability
    }
    pub fn identity(&self) -> &str {
        &self.identity
    }
    pub fn declarations(&self) -> &[GraphContributionV2] {
        &self.declarations
    }
    pub fn routes(&self) -> &BTreeMap<String, String> {
        &self.routes
    }
    pub fn predecessors(&self) -> &BTreeMap<String, BTreeSet<String>> {
        &self.predecessors
    }
    /// Potential sources over Flow only. Order never establishes reachability.
    pub fn source_reachability(&self) -> &BTreeMap<String, BTreeSet<String>> {
        &self.source_reachability
    }
    pub fn order(&self) -> &[String] {
        &self.order
    }
    pub fn policy(&self) -> &TrustedCompositionPolicy {
        &self.policy
    }
}

fn fail(code: Code, message: &str) -> Failure {
    Failure::new(code, message)
}
fn denied(message: &str) -> Failure {
    fail(Code::Denied, message)
}
fn invalid(message: &str) -> Failure {
    fail(Code::DescriptorInvalid, message)
}

impl Prepared {
    /// Compile against the established selected bindings and operation grants.
    /// Contributor identity is checked against real verified selected packages;
    /// this does not attest that supplied declaration bytes came from a package.
    pub fn compile_graph_v2(
        &self,
        declarations: &[GraphContributionV2],
        policy: &TrustedCompositionPolicy,
    ) -> Result<CompiledGraph, Failure> {
        for declaration in declarations {
            let verified = self
                .packages
                .get(&declaration.contributor)
                .and_then(|package| package.graph_v2())
                .ok_or_else(|| denied("graph-v2 declaration lacks verified package metadata"))?;
            if declaration.normalized()?.encode()? != verified.normalized()?.encode()? {
                return Err(denied(
                    "graph-v2 declaration differs from verified package metadata",
                ));
            }
        }
        compile(
            declarations,
            policy,
            |owner| {
                self.packages
                    .get(owner)
                    .is_some_and(|p| p.metadata().plugin == owner)
            },
            |owner, selector, key| {
                let provider = if selector.is_empty() {
                    owner.to_owned()
                } else {
                    self.bindings
                        .get(&(
                            owner.to_owned(),
                            phenix_contract::import_identity(selector, key),
                        ))
                        .cloned()
                        .ok_or_else(|| fail(Code::MissingBinding, "unselected graph-v2 import"))?
                };
                let operation = self
                    .operations
                    .get(&(provider.clone(), key.to_owned()))
                    .ok_or_else(|| fail(Code::MissingBinding, "graph-v2 operation absent"))?;
                if !self.permits(owner, key)
                    || !self
                        .grants
                        .contains_key(&(owner.to_owned(), key.to_owned()))
                {
                    return Err(denied("graph-v2 operation lacks prepared grant"));
                }
                Ok((
                    provider,
                    operation.schema.clone(),
                    self.grants[&(owner.to_owned(), key.to_owned())].clone(),
                ))
            },
        )
    }
}

// Private resolver seam for pure tests, not an alternate public selection API.
fn compile(
    declarations: &[GraphContributionV2],
    policy: &TrustedCompositionPolicy,
    mut attested: impl FnMut(&str) -> bool,
    mut resolve: impl FnMut(
        &str,
        &str,
        &str,
    ) -> Result<(String, OperationSchema, BTreeSet<String>), Failure>,
) -> Result<CompiledGraph, Failure> {
    if declarations.len() > MAX_COUNT {
        return Err(invalid("aggregate graph-v2 declaration count limit"));
    }
    let mut counts = [0usize; 3];
    for declaration in declarations {
        for (total, count) in counts.iter_mut().zip([
            declaration.nodes.len(),
            declaration.edges.len(),
            declaration.sources.len(),
        ]) {
            *total = total
                .checked_add(count)
                .filter(|n| *n <= MAX_COUNT)
                .ok_or_else(|| invalid("aggregate graph-v2 count limit"))?;
        }
    }
    // Canonical encoding validates each bounded value tree. Cap the aggregate
    // before resolver calls, graph indexing, reachability or DAG traversal.
    let mut canonical_bytes = 0usize;
    for declaration in declarations {
        canonical_bytes = canonical_bytes
            .checked_add(declaration.encode()?.len())
            .filter(|n| *n <= MAX_BYTES)
            .ok_or_else(|| invalid("aggregate graph-v2 byte limit"))?;
    }
    if policy.delivery.values().any(|grant| {
        grant
            .resources
            .iter()
            .any(|resource| !phenix_contract::identifier(resource))
    }) {
        return Err(invalid("invalid delivery resource identity"));
    }
    let mut declarations = declarations
        .iter()
        .map(GraphContributionV2::normalized)
        .collect::<Result<Vec<_>, _>>()?;
    declarations.sort_by(|a, b| a.contributor.cmp(&b.contributor));
    if declarations
        .windows(2)
        .any(|p| p[0].contributor == p[1].contributor)
    {
        return Err(invalid("duplicate graph-v2 contributor"));
    }
    let mut nodes = BTreeMap::new();
    let mut actions = BTreeSet::new();
    let mut routes = BTreeMap::new();
    let mut operation_grants = BTreeMap::new();
    let mut predecessors = BTreeMap::<String, BTreeSet<String>>::new();
    let mut flow_predecessors = predecessors.clone();
    let mut source_reachability = predecessors.clone();
    for declaration in &declarations {
        if !attested(&declaration.contributor) {
            return Err(denied(
                "graph-v2 contributor is not a selected attested package",
            ));
        }
        for source in &declaration.sources {
            if let SourceKind::ExternalAction { action } = &source.kind {
                if !actions.insert(action.clone()) {
                    return Err(invalid("duplicate graph-v2 action"));
                }
            }
            if source.owner != declaration.contributor || !attested(&source.owner) {
                return Err(denied("source package owner mismatch"));
            }
            if !matches!(source.kind, SourceKind::ExternalAction { .. }) {
                return Err(denied("source kind pending admission authorization"));
            }
            let ceilings = policy
                .source_owners
                .get(&source.owner)
                .ok_or_else(|| denied("missing trusted source ceilings"))?;
            validate_source_ceiling(
                source.security,
                ceilings.owner_ceiling,
                ceilings.composition_ceiling,
            )?;
            if policy.sources.get(&source.id) != Some(&source.security) {
                return Err(denied("source security attestation mismatch"));
            }
        }
        for node in &declaration.nodes {
            predecessors.insert(node.id.clone(), BTreeSet::new());
            flow_predecessors.insert(node.id.clone(), BTreeSet::new());
            source_reachability.insert(
                node.id.clone(),
                match &node.kind {
                    NodeKind::Source { source } => BTreeSet::from([source.clone()]),
                    _ => BTreeSet::new(),
                },
            );
            if let NodeKind::Invoke {
                selector,
                operation,
            } = &node.kind
            {
                let (provider, schema, resources) =
                    resolve(&declaration.contributor, selector, operation)?;
                let labels = policy
                    .operations
                    .get(&(provider.clone(), operation.clone()))
                    .ok_or_else(|| denied("missing operation security attestation"))?;
                let input = &node.inputs[0];
                let err = &node.outputs[0];
                let ok = &node.outputs[1];
                if input.schema != schema.request
                    || ok.schema != schema.response
                    || err.schema != schema.error
                {
                    return Err(fail(
                        Code::SchemaRejected,
                        "selected operation port schema mismatch",
                    ));
                }
                if input.security != labels.input
                    || ok.security != labels.ok
                    || err.security != labels.err
                {
                    return Err(denied("operation security attestation mismatch"));
                }
                if !labels.input.can_flow_to(labels.ok) || !labels.input.can_flow_to(labels.err) {
                    return Err(denied("declassification or endorsement unsupported"));
                }
                routes.insert(node.id.clone(), provider);
                operation_grants.insert(node.id.clone(), resources);
            }
            if matches!(node.kind, NodeKind::Constant { .. })
                && node.inputs[0].security.confidentiality != Classification::Public
            {
                return Err(denied("nonpublic constant control unsupported"));
            }
            nodes.insert(node.id.clone(), (&declaration.contributor, node));
        }
    }
    let mut candidates = BTreeMap::<(String, String), usize>::new();
    for declaration in &declarations {
        for edge in &declaration.edges {
            let endpoints = match &edge.kind {
                EdgeKind::Flow { from, to } => [&from.node, &to.node],
                EdgeKind::Order { before, after } => [before, after],
                EdgeKind::Deferred { .. } => return Err(invalid("deferred unsupported")),
            };
            for endpoint in endpoints {
                let (owner, _) = nodes
                    .get(endpoint)
                    .ok_or_else(|| invalid("missing edge endpoint"))?;
                if *owner != &declaration.contributor
                    && !policy
                        .topology
                        .contains(&(declaration.contributor.clone(), endpoint.clone()))
                {
                    return Err(denied("foreign endpoint lacks contributor topology grant"));
                }
            }
            match &edge.kind {
                EdgeKind::Flow { from, to } => {
                    let (source_owner, source) = nodes
                        .get(&from.node)
                        .ok_or_else(|| invalid("missing flow source"))?;
                    let (target_owner, target) = nodes
                        .get(&to.node)
                        .ok_or_else(|| invalid("missing flow destination"))?;
                    let output = source
                        .outputs
                        .iter()
                        .find(|p| p.name == from.port)
                        .ok_or_else(|| invalid("missing output port"))?;
                    let input = target
                        .inputs
                        .iter()
                        .find(|p| p.name == to.port)
                        .ok_or_else(|| invalid("missing input port"))?;
                    if &output.schema != input.candidate_schema()? {
                        return Err(fail(Code::SchemaRejected, "flow schema mismatch"));
                    }
                    if !output.security.can_flow_to(input.security) {
                        return Err(denied("flow security direction rejected"));
                    }
                    // A foreign selected provider is also a trust boundary, even
                    // when both authored nodes belong to one graph contributor.
                    let source_principal = routes.get(&from.node).unwrap_or(source_owner);
                    let target_principal = routes.get(&to.node).unwrap_or(target_owner);
                    if source_owner != target_owner || source_principal != target_principal {
                        let key = (
                            from.node.clone(),
                            from.port.clone(),
                            to.node.clone(),
                            to.port.clone(),
                        );
                        let grant = policy.delivery.get(&key).ok_or_else(|| {
                            denied("cross-plugin flow lacks exact delivery grant")
                        })?;
                        if output.security.confidentiality > grant.maximum_classification
                            || grant.resources.is_empty()
                        {
                            return Err(denied(
                                "delivery classification or resource scope rejected",
                            ));
                        }
                    }
                    *candidates
                        .entry((to.node.clone(), to.port.clone()))
                        .or_default() += 1;
                    predecessors
                        .get_mut(&to.node)
                        .unwrap()
                        .insert(from.node.clone());
                    flow_predecessors
                        .get_mut(&to.node)
                        .unwrap()
                        .insert(from.node.clone());
                }
                EdgeKind::Order { before, after } => {
                    if !nodes.contains_key(before) || !nodes.contains_key(after) {
                        return Err(invalid("missing order endpoint"));
                    }
                    predecessors.get_mut(after).unwrap().insert(before.clone());
                }
                EdgeKind::Deferred { .. } => return Err(invalid("deferred unsupported")),
            }
        }
    }
    for (_, node) in nodes.values() {
        for input in &node.inputs {
            if candidates
                .get(&(node.id.clone(), input.name.clone()))
                .copied()
                .unwrap_or(0)
                > 1
                && !matches!(input.readiness, InputReadiness::StaticMany { .. })
                && !matches!(node.kind, NodeKind::Merge { .. })
            {
                return Err(invalid("One/Optional input permits at most one candidate"));
            }
        }
    }
    validate_depth(&predecessors)?;
    let order = dag::order(&predecessors).map_err(|_| invalid("graph-v2 Flow/Order cycle"))?;
    for id in &order {
        for predecessor in &flow_predecessors[id] {
            let inherited = source_reachability[predecessor].clone();
            source_reachability.get_mut(id).unwrap().extend(inherited);
        }
    }
    let mut join_satisfiability = BTreeMap::new();
    for (_, node) in nodes.values() {
        if !matches!(node.kind, NodeKind::Join { .. }) {
            continue;
        }
        let incoming: Vec<_> = node
            .inputs
            .iter()
            .filter(|input| {
                matches!(
                    input.readiness,
                    InputReadiness::One | InputReadiness::StaticMany { min: 1.., .. }
                )
            })
            .map(|input| {
                declarations
                    .iter()
                    .flat_map(|d| &d.edges)
                    .filter_map(|edge| match &edge.kind {
                        EdgeKind::Flow { from, to }
                            if to.node == node.id && to.port == input.name =>
                        {
                            Some(from)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let reachable: Vec<BTreeSet<String>> = incoming
            .iter()
            .map(|edges| {
                edges
                    .iter()
                    .flat_map(|from| source_reachability[&from.node].iter().cloned())
                    .collect()
            })
            .collect();
        if source_reachability[&node.id].is_empty() || reachable.iter().any(BTreeSet::is_empty) {
            join_satisfiability.insert(node.id.clone(), Satisfiability::Dormant);
            continue;
        }
        let mut common = source_reachability[&node.id].clone();
        for sources in &reachable {
            common.retain(|source| sources.contains(source));
        }
        let mut switch_cases = BTreeMap::new();
        for from in node
            .inputs
            .iter()
            .filter(|p| {
                matches!(
                    p.readiness,
                    InputReadiness::One | InputReadiness::StaticMany { min: 1.., .. }
                )
            })
            .zip(&incoming)
            .filter(|(p, _)| p.readiness == InputReadiness::One)
            .flat_map(|(_, edges)| edges)
        {
            if matches!(nodes[&from.node].1.kind, NodeKind::Switch { .. })
                && switch_cases
                    .insert(&from.node, &from.port)
                    .is_some_and(|port| port != &from.port)
            {
                return Err(invalid("UnsatisfiableJoin: direct Switch case exclusion"));
            }
        }
        if common.is_empty() {
            return Err(invalid("UnsatisfiableJoin: disjoint input sources"));
        }
        let status = if node
            .inputs
            .iter()
            .all(|p| p.readiness == InputReadiness::One)
            && incoming
                .iter()
                .flatten()
                .all(|from| matches!(nodes[&from.node].1.kind, NodeKind::Source { .. }))
        {
            Satisfiability::ProvenSatisfiable
        } else {
            Satisfiability::Unknown
        };
        join_satisfiability.insert(node.id.clone(), status);
    }
    let canonical = Value::List(
        declarations
            .iter()
            .map(|d| d.encode().map(Value::Bytes))
            .collect::<Result<_, _>>()?,
    );
    let route_values = Value::List(
        routes
            .iter()
            .map(|(node, provider)| {
                Value::List(vec![
                    text(node),
                    text(provider),
                    strings(&operation_grants[node]),
                ])
            })
            .collect(),
    );
    let mut bytes = b"phenix.graph.offline-plan.v2\0".to_vec();
    bytes.extend(Value::List(vec![canonical, route_values, policy_value(policy)]).encode()?);
    Ok(CompiledGraph {
        identity: phenix_artifact::digest(&bytes),
        declarations,
        routes,
        predecessors,
        source_reachability,
        join_satisfiability,
        order,
        policy: policy.clone(),
    })
}

fn validate_depth(predecessors: &BTreeMap<String, BTreeSet<String>>) -> Result<(), Failure> {
    let mut remaining = BTreeMap::new();
    let mut successors = BTreeMap::<&str, Vec<&str>>::new();
    let mut ready = Vec::new();
    let mut depths = BTreeMap::new();
    for (id, dependencies) in predecessors {
        remaining.insert(id.as_str(), dependencies.len());
        depths.insert(id.as_str(), 1usize);
        if dependencies.is_empty() {
            ready.push(id.as_str());
        }
        for dependency in dependencies {
            successors.entry(dependency).or_default().push(id);
        }
    }
    let mut visited = 0;
    while let Some(id) = ready.pop() {
        visited += 1;
        let depth = depths[id];
        if depth > MAX_GRAPH_DEPTH {
            return Err(invalid("graph-v2 depth limit"));
        }
        for successor in successors.get(id).into_iter().flatten() {
            let next_depth = depths.get_mut(successor).unwrap();
            *next_depth = (*next_depth).max(depth + 1);
            let count = remaining.get_mut(successor).unwrap();
            *count -= 1;
            if *count == 0 {
                ready.push(successor);
            }
        }
    }
    if visited != predecessors.len() {
        return Err(invalid("graph-v2 Flow/Order cycle"));
    }
    Ok(())
}

fn text(value: &str) -> Value {
    Value::String(value.into())
}
fn strings(values: &BTreeSet<String>) -> Value {
    Value::List(values.iter().map(|v| text(v)).collect())
}
fn security(value: PortSecurity) -> Value {
    Value::List(vec![
        Value::U64(value.confidentiality as u64),
        Value::U64(value.integrity as u64),
    ])
}
// BTree collections fix policy order. This digest is not a patch provenance or
// generation digest; all trusted policy entries (even unused ones) participate.
fn policy_value(policy: &TrustedCompositionPolicy) -> Value {
    Value::List(vec![
        Value::List(
            policy
                .topology
                .iter()
                .map(|(contributor, node)| Value::List(vec![text(contributor), text(node)]))
                .collect(),
        ),
        Value::List(
            policy
                .source_owners
                .iter()
                .map(|(owner, p)| {
                    Value::List(vec![
                        text(owner),
                        Value::U64(p.owner_ceiling as u64),
                        Value::U64(p.composition_ceiling as u64),
                    ])
                })
                .collect(),
        ),
        Value::List(
            policy
                .sources
                .iter()
                .map(|(id, s)| Value::List(vec![text(id), security(*s)]))
                .collect(),
        ),
        Value::List(
            policy
                .operations
                .iter()
                .map(|((provider, operation), s)| {
                    Value::List(vec![
                        text(provider),
                        text(operation),
                        security(s.input),
                        security(s.ok),
                        security(s.err),
                    ])
                })
                .collect(),
        ),
        Value::List(
            policy
                .delivery
                .iter()
                .map(|((from, out, to, input), g)| {
                    Value::List(vec![
                        text(from),
                        text(out),
                        text(to),
                        text(input),
                        strings(&g.resources),
                        Value::U64(g.maximum_classification as u64),
                    ])
                })
                .collect(),
        ),
    ])
}

#[cfg(test)]
mod tests;
