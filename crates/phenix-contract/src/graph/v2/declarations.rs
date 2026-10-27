//! Canonical base profile. IDs are strings using the existing contract grammar;
//! authored source/node/edge identities are fully qualified `plugin/local`.
//! Validation checks declaration shape, not selected bindings, grants, cycles,
//! cross-contribution references, or authenticated artifact ownership.
use super::{validate_inspectable_literal, Classification, IntegrityClass, PortSecurity};
use crate::graph::{bad, empty, fields, list, sorted, take, text, valid_node_key, Fields};
use crate::{identifier, operation_key, Failure, SchemaId, Value, ValueSchema};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphContributionV2 {
    pub contributor: String,
    pub sources: Vec<SourceDecl>,
    pub nodes: Vec<NodeDecl>,
    pub edges: Vec<EdgeDecl>,
    pub patches: Vec<GraphPatch>,
}

/// No patch is supported before the base vertical slice. Nonempty wire lists
/// fail decoding; they are never ignored or stripped during normalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphPatch {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDecl {
    pub id: String,
    pub kind: SourceKind,
    pub payload: ValueSchema,
    pub security: PortSecurity,
    pub owner: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceKind {
    ExternalAction {
        action: String,
    },
    CustomEvent {
        event: String,
    },
    KernelEvent {
        event: String,
    },
    CommittedWatch {
        property: String,
    },
    /// Reserved for compiler-generated admissions; rejected in this profile.
    DeferredEntry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeDecl {
    pub id: String,
    pub kind: NodeKind,
    pub inputs: Vec<InputPort>,
    pub outputs: Vec<OutputPort>,
    pub execution: ExecutionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    /// Empty selector means the owner; otherwise slot or slot#index.
    Invoke {
        selector: String,
        operation: String,
    },
    Source {
        source: String,
    },
    Switch {
        tagged_schema: SchemaId,
    },
    Merge {
        policy: MergePolicy,
    },
    Join {
        policy: JoinPolicy,
    },
    Constant {
        value: Value,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergePolicy {
    ExactlyOne,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinPolicy {
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputPort {
    pub name: String,
    pub schema: ValueSchema,
    pub readiness: InputReadiness,
    pub security: PortSecurity,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputPort {
    pub name: String,
    pub schema: ValueSchema,
    pub security: PortSecurity,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputReadiness {
    One,
    Optional,
    StaticMany { min: u32, max: u32 },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainErrorPolicy {
    RouteErr,
    SkipDependents,
    FailRoot,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionPolicy {
    pub on_domain_error: DomainErrorPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutPortRef {
    pub node: String,
    pub port: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InPortRef {
    pub node: String,
    pub port: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeDecl {
    pub id: String,
    pub kind: EdgeKind,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeKind {
    Flow {
        from: OutPortRef,
        to: InPortRef,
    },
    Order {
        before: String,
        after: String,
    },
    Deferred {
        from: OutPortRef,
        to: InPortRef,
        next: DeferredWave,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferredWave {
    Next,
}

fn owned(id: &str, contributor: &str) -> bool {
    valid_node_key(id)
        && id
            .split_once('/')
            .is_some_and(|(owner, _)| owner == contributor)
}

fn selector_valid(selector: &str) -> bool {
    if selector.is_empty() {
        return true;
    }
    match selector.split_once('#') {
        None => identifier(selector),
        Some((slot, index)) => {
            identifier(slot) && index.parse::<u32>().is_ok_and(|n| n.to_string() == index)
        }
    }
}

impl GraphContributionV2 {
    pub fn normalized(&self) -> Result<Self, Failure> {
        let mut result = self.clone();
        result.sources.sort_by(|a, b| a.id.cmp(&b.id));
        result.nodes.sort_by(|a, b| a.id.cmp(&b.id));
        result.edges.sort_by(|a, b| a.id.cmp(&b.id));
        for node in &mut result.nodes {
            node.inputs.sort_by(|a, b| a.name.cmp(&b.name));
            node.outputs.sort_by(|a, b| a.name.cmp(&b.name));
        }
        result.validate()?;
        Ok(result)
    }

    /// Checks canonical declaration order and local shape, including byte/depth
    /// bounds through the existing Value and ValueSchema primitives.
    pub fn validate(&self) -> Result<(), Failure> {
        if !identifier(&self.contributor)
            || self.contributor == "root"
            || !sorted(self.sources.iter().map(|s| s.id.as_str()))
            || !sorted(self.nodes.iter().map(|n| n.id.as_str()))
            || !sorted(self.edges.iter().map(|e| e.id.as_str()))
        {
            return Err(bad("invalid graph-v2 identity, order or duplicate"));
        }
        for source in &self.sources {
            if !owned(&source.id, &self.contributor) || source.owner != self.contributor {
                return Err(bad("graph-v2 source owner mismatch"));
            }
            match &source.kind {
                SourceKind::ExternalAction { action } if identifier(action) => {}
                SourceKind::CustomEvent { event } | SourceKind::KernelEvent { event }
                    if identifier(event) => {}
                SourceKind::CommittedWatch { property } if valid_node_key(property) => {}
                SourceKind::DeferredEntry => {
                    return Err(bad("unsupported graph-v2 deferred entry"))
                }
                _ => return Err(bad("invalid graph-v2 source kind identity")),
            }
            source.payload.encode()?;
        }
        for node in &self.nodes {
            if !owned(&node.id, &self.contributor)
                || !sorted(node.inputs.iter().map(|p| p.name.as_str()))
                || !sorted(node.outputs.iter().map(|p| p.name.as_str()))
            {
                return Err(bad("invalid graph-v2 node identity, ports or duplicate"));
            }
            for port in &node.inputs {
                if !identifier(&port.name) {
                    return Err(bad("invalid input port name"));
                }
                port.schema.encode()?;
                if port.readiness != InputReadiness::One {
                    return Err(bad("unsupported graph-v2 input readiness"));
                }
            }
            for port in &node.outputs {
                if !identifier(&port.name) {
                    return Err(bad("invalid output port name"));
                }
                port.schema.encode()?;
            }
            match &node.kind {
                NodeKind::Invoke {
                    selector,
                    operation,
                } => {
                    if !selector_valid(selector)
                        || !operation_key(operation)
                        || node.inputs.len() != 1
                        || node.inputs[0].name != "in"
                        || node.outputs.len() != 2
                        || node.outputs[0].name != "err"
                        || node.outputs[1].name != "ok"
                    {
                        return Err(bad("Invoke requires one One in request and err/ok outputs"));
                    }
                }
                NodeKind::Source { source } => {
                    let declaration = self
                        .sources
                        .iter()
                        .find(|s| &s.id == source)
                        .ok_or_else(|| bad("undeclared local graph-v2 source"))?;
                    if !node.inputs.is_empty()
                        || node.outputs.len() != 1
                        || node.outputs[0].name != "out"
                        || node.outputs[0].schema != declaration.payload
                        || node.outputs[0].security != declaration.security
                    {
                        return Err(bad("Source requires matching out and no inputs"));
                    }
                }
                NodeKind::Constant { value } => {
                    if node.inputs.len() != 1
                        || node.inputs[0].name != "start"
                        || node.inputs[0].schema != ValueSchema::Struct(Default::default())
                        || node.outputs.len() != 1
                        || node.outputs[0].name != "out"
                    {
                        return Err(bad("Constant requires One unit start and out"));
                    }
                    validate_inspectable_literal(
                        node.outputs[0].security,
                        &node.outputs[0].schema,
                        value,
                    )?;
                    if !node.inputs[0]
                        .security
                        .can_flow_to(node.outputs[0].security)
                    {
                        return Err(bad("Constant control input cannot downgrade security"));
                    }
                }
                NodeKind::Switch { tagged_schema } => {
                    if node.inputs.len() != 1 || node.inputs[0].name != "in" {
                        return Err(bad("Switch requires one One in"));
                    }
                    let input = &node.inputs[0];
                    let ValueSchema::Tagged(cases) = &input.schema else {
                        return Err(bad("Switch requires Tagged input"));
                    };
                    if input.schema.id()? != *tagged_schema
                        || node.outputs.len() != cases.len()
                        || node.outputs.iter().any(|output| {
                            cases.get(&output.name).is_none_or(|schema| {
                                output.schema
                                    != schema
                                        .clone()
                                        .unwrap_or_else(|| ValueSchema::Struct(Default::default()))
                            }) || !input.security.can_flow_to(output.security)
                        })
                    {
                        return Err(bad(
                            "Switch requires exhaustive matching case schemas and security",
                        ));
                    }
                }
                NodeKind::Merge {
                    policy: MergePolicy::ExactlyOne,
                } => {
                    if node.inputs.len() != 1
                        || node.inputs[0].name != "in"
                        || node.outputs.len() != 1
                        || node.outputs[0].name != "out"
                        || node.inputs[0].schema != node.outputs[0].schema
                        || !node.inputs[0]
                            .security
                            .can_flow_to(node.outputs[0].security)
                    {
                        return Err(bad(
                            "Merge requires matching One in and out without security downgrade",
                        ));
                    }
                }
                NodeKind::Join { .. } => return Err(bad("unsupported graph-v2 intrinsic")),
            }
        }
        for edge in &self.edges {
            if !owned(&edge.id, &self.contributor) {
                return Err(bad("graph-v2 edge owner mismatch"));
            }
            match &edge.kind {
                EdgeKind::Flow { from, to } => {
                    if !valid_node_key(&from.node)
                        || !identifier(&from.port)
                        || !valid_node_key(&to.node)
                        || !identifier(&to.port)
                    {
                        return Err(bad("invalid graph-v2 port reference"));
                    }
                    // Local references must name explicit ports. Foreign endpoints
                    // are resolved with the other contributions during compilation.
                    if owned(&from.node, &self.contributor)
                        && !self.nodes.iter().any(|n| {
                            n.id == from.node && n.outputs.iter().any(|p| p.name == from.port)
                        })
                    {
                        return Err(bad("unknown local output port"));
                    }
                    if owned(&to.node, &self.contributor)
                        && !self
                            .nodes
                            .iter()
                            .any(|n| n.id == to.node && n.inputs.iter().any(|p| p.name == to.port))
                    {
                        return Err(bad("unknown local input port"));
                    }
                }
                EdgeKind::Order { before, after } => {
                    if !valid_node_key(before) || !valid_node_key(after) || before == after {
                        return Err(bad("invalid graph-v2 order endpoints"));
                    }
                    for endpoint in [before, after] {
                        if owned(endpoint, &self.contributor)
                            && !self.nodes.iter().any(|n| &n.id == endpoint)
                        {
                            return Err(bad("unknown local order endpoint"));
                        }
                    }
                }
                EdgeKind::Deferred { .. } => return Err(bad("unsupported graph-v2 deferred edge")),
            }
        }
        self.value()?.encode()?;
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, Failure> {
        self.normalized()?.value()?.encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        let mut root = fields(Value::decode(bytes)?)?;
        if text(&mut root, "format")? != "phenix.graph.v2" {
            return Err(bad("unknown graph-v2 format"));
        }
        let contributor = text(&mut root, "contributor")?;
        let sources = list(&mut root, "sources")?
            .into_iter()
            .map(decode_source)
            .collect::<Result<_, _>>()?;
        let nodes = list(&mut root, "nodes")?
            .into_iter()
            .map(decode_node)
            .collect::<Result<_, _>>()?;
        let edges = list(&mut root, "edges")?
            .into_iter()
            .map(decode_edge)
            .collect::<Result<_, _>>()?;
        if !list(&mut root, "patches")?.is_empty() {
            return Err(bad("unsupported graph-v2 patches"));
        }
        empty(root)?;
        let result = Self {
            contributor,
            sources,
            nodes,
            edges,
            patches: vec![],
        };
        result.validate()?;
        if result.encode()? != bytes {
            return Err(bad("noncanonical graph-v2 declaration"));
        }
        Ok(result)
    }

    fn value(&self) -> Result<Value, Failure> {
        Ok(Value::map([
            ("format", string("phenix.graph.v2")),
            ("contributor", string(&self.contributor)),
            (
                "sources",
                Value::List(
                    self.sources
                        .iter()
                        .map(encode_source)
                        .collect::<Result<_, _>>()?,
                ),
            ),
            (
                "nodes",
                Value::List(
                    self.nodes
                        .iter()
                        .map(encode_node)
                        .collect::<Result<_, _>>()?,
                ),
            ),
            (
                "edges",
                Value::List(
                    self.edges
                        .iter()
                        .map(encode_edge)
                        .collect::<Result<_, _>>()?,
                ),
            ),
            ("patches", Value::List(vec![])),
        ]))
    }
}

fn string(s: &str) -> Value {
    Value::String(s.into())
}
fn security(s: PortSecurity) -> Value {
    Value::map([
        (
            "confidentiality",
            string(match s.confidentiality {
                Classification::Public => "public",
                Classification::Internal => "internal",
                Classification::Confidential => "confidential",
                Classification::Secret => "secret",
            }),
        ),
        (
            "integrity",
            string(match s.integrity {
                IntegrityClass::Untrusted => "untrusted",
                IntegrityClass::Validated => "validated",
                IntegrityClass::Trusted => "trusted",
            }),
        ),
    ])
}
fn decode_security(value: Value) -> Result<PortSecurity, Failure> {
    let mut f = fields(value)?;
    let confidentiality = match text(&mut f, "confidentiality")?.as_str() {
        "public" => Classification::Public,
        "internal" => Classification::Internal,
        "confidential" => Classification::Confidential,
        "secret" => Classification::Secret,
        _ => return Err(bad("unknown confidentiality")),
    };
    let integrity = match text(&mut f, "integrity")?.as_str() {
        "untrusted" => IntegrityClass::Untrusted,
        "validated" => IntegrityClass::Validated,
        "trusted" => IntegrityClass::Trusted,
        _ => return Err(bad("unknown integrity")),
    };
    empty(f)?;
    Ok(PortSecurity {
        confidentiality,
        integrity,
    })
}
fn schema(f: &mut Fields, key: &str) -> Result<ValueSchema, Failure> {
    match take(f, key)? {
        Value::Bytes(bytes) => ValueSchema::decode(&bytes),
        _ => Err(bad("schema must be canonical bytes")),
    }
}
fn encode_source(s: &SourceDecl) -> Result<Value, Failure> {
    let (kind, key, id) = match &s.kind {
        SourceKind::ExternalAction { action } => ("external-action", "action", action),
        SourceKind::CustomEvent { event } => ("custom-event", "event", event),
        SourceKind::KernelEvent { event } => ("kernel-event", "event", event),
        SourceKind::CommittedWatch { property } => ("committed-watch", "property", property),
        SourceKind::DeferredEntry => return Err(bad("unsupported graph-v2 deferred entry")),
    };
    Ok(Value::map([
        ("id", string(&s.id)),
        ("owner", string(&s.owner)),
        (
            "kind",
            Value::map([("kind", string(kind)), (key, string(id))]),
        ),
        ("payload", Value::Bytes(s.payload.encode()?)),
        ("security", security(s.security)),
    ]))
}
fn decode_source(value: Value) -> Result<SourceDecl, Failure> {
    let mut f = fields(value)?;
    let id = text(&mut f, "id")?;
    let owner = text(&mut f, "owner")?;
    let payload = schema(&mut f, "payload")?;
    let security = decode_security(take(&mut f, "security")?)?;
    let mut k = fields(take(&mut f, "kind")?)?;
    let kind = match text(&mut k, "kind")?.as_str() {
        "external-action" => SourceKind::ExternalAction {
            action: text(&mut k, "action")?,
        },
        "custom-event" => SourceKind::CustomEvent {
            event: text(&mut k, "event")?,
        },
        "kernel-event" => SourceKind::KernelEvent {
            event: text(&mut k, "event")?,
        },
        "committed-watch" => SourceKind::CommittedWatch {
            property: text(&mut k, "property")?,
        },
        _ => return Err(bad("unknown or unsupported graph-v2 source kind")),
    };
    empty(k)?;
    empty(f)?;
    Ok(SourceDecl {
        id,
        kind,
        payload,
        security,
        owner,
    })
}
fn encode_node(n: &NodeDecl) -> Result<Value, Failure> {
    let kind = match &n.kind {
        NodeKind::Invoke {
            selector,
            operation,
        } => Value::map([
            ("kind", string("invoke")),
            ("selector", string(selector)),
            ("operation", string(operation)),
        ]),
        NodeKind::Source { source } => {
            Value::map([("kind", string("source")), ("source", string(source))])
        }
        NodeKind::Constant { value } => {
            Value::map([("kind", string("constant")), ("value", value.clone())])
        }
        NodeKind::Switch { tagged_schema } => Value::map([
            ("kind", string("switch")),
            (
                "tagged_schema",
                Value::Bytes(tagged_schema.as_bytes().to_vec()),
            ),
        ]),
        NodeKind::Merge {
            policy: MergePolicy::ExactlyOne,
        } => Value::map([("kind", string("merge")), ("policy", string("exactly-one"))]),
        _ => return Err(bad("unsupported graph-v2 intrinsic")),
    };
    let inputs = n
        .inputs
        .iter()
        .map(|p| {
            Ok(Value::map([
                ("name", string(&p.name)),
                ("schema", Value::Bytes(p.schema.encode()?)),
                ("security", security(p.security)),
                ("readiness", string("one")),
            ]))
        })
        .collect::<Result<_, Failure>>()?;
    let outputs = n
        .outputs
        .iter()
        .map(|p| {
            Ok(Value::map([
                ("name", string(&p.name)),
                ("schema", Value::Bytes(p.schema.encode()?)),
                ("security", security(p.security)),
            ]))
        })
        .collect::<Result<_, Failure>>()?;
    Ok(Value::map([
        ("id", string(&n.id)),
        ("kind", kind),
        ("inputs", Value::List(inputs)),
        ("outputs", Value::List(outputs)),
        (
            "execution",
            Value::map([(
                "on_domain_error",
                string(match n.execution.on_domain_error {
                    DomainErrorPolicy::RouteErr => "route-err",
                    DomainErrorPolicy::SkipDependents => "skip-dependents",
                    DomainErrorPolicy::FailRoot => "fail-root",
                }),
            )]),
        ),
    ]))
}
fn decode_node(value: Value) -> Result<NodeDecl, Failure> {
    let mut f = fields(value)?;
    let id = text(&mut f, "id")?;
    let mut k = fields(take(&mut f, "kind")?)?;
    let kind = match text(&mut k, "kind")?.as_str() {
        "invoke" => NodeKind::Invoke {
            selector: text(&mut k, "selector")?,
            operation: text(&mut k, "operation")?,
        },
        "source" => NodeKind::Source {
            source: text(&mut k, "source")?,
        },
        "constant" => NodeKind::Constant {
            value: take(&mut k, "value")?,
        },
        "switch" => NodeKind::Switch {
            tagged_schema: match take(&mut k, "tagged_schema")? {
                Value::Bytes(bytes) => SchemaId::from_bytes(&bytes)?,
                _ => return Err(bad("Switch schema ID must be bytes")),
            },
        },
        "merge" => NodeKind::Merge {
            policy: match text(&mut k, "policy")?.as_str() {
                "exactly-one" => MergePolicy::ExactlyOne,
                _ => return Err(bad("unsupported Merge policy")),
            },
        },
        _ => return Err(bad("unknown or unsupported graph-v2 intrinsic")),
    };
    empty(k)?;
    let inputs = list(&mut f, "inputs")?
        .into_iter()
        .map(|v| {
            let mut p = fields(v)?;
            let name = text(&mut p, "name")?;
            let schema = schema(&mut p, "schema")?;
            let security = decode_security(take(&mut p, "security")?)?;
            if text(&mut p, "readiness")? != "one" {
                return Err(bad("unknown or unsupported input readiness"));
            }
            empty(p)?;
            Ok(InputPort {
                name,
                schema,
                security,
                readiness: InputReadiness::One,
            })
        })
        .collect::<Result<_, Failure>>()?;
    let outputs = list(&mut f, "outputs")?
        .into_iter()
        .map(|v| {
            let mut p = fields(v)?;
            let name = text(&mut p, "name")?;
            let schema = schema(&mut p, "schema")?;
            let security = decode_security(take(&mut p, "security")?)?;
            empty(p)?;
            Ok(OutputPort {
                name,
                schema,
                security,
            })
        })
        .collect::<Result<_, Failure>>()?;
    let mut e = fields(take(&mut f, "execution")?)?;
    let on_domain_error = match text(&mut e, "on_domain_error")?.as_str() {
        "route-err" => DomainErrorPolicy::RouteErr,
        "skip-dependents" => DomainErrorPolicy::SkipDependents,
        "fail-root" => DomainErrorPolicy::FailRoot,
        _ => return Err(bad("unknown domain error policy")),
    };
    empty(e)?;
    empty(f)?;
    Ok(NodeDecl {
        id,
        kind,
        inputs,
        outputs,
        execution: ExecutionPolicy { on_domain_error },
    })
}
fn port_ref(node: &str, port: &str) -> Value {
    Value::map([("node", string(node)), ("port", string(port))])
}
fn decode_ref(value: Value) -> Result<(String, String), Failure> {
    let mut f = fields(value)?;
    let result = (text(&mut f, "node")?, text(&mut f, "port")?);
    empty(f)?;
    Ok(result)
}
fn encode_edge(e: &EdgeDecl) -> Result<Value, Failure> {
    let kind = match &e.kind {
        EdgeKind::Flow { from, to } => Value::map([
            ("kind", string("flow")),
            ("from", port_ref(&from.node, &from.port)),
            ("to", port_ref(&to.node, &to.port)),
        ]),
        EdgeKind::Order { before, after } => Value::map([
            ("kind", string("order")),
            ("before", string(before)),
            ("after", string(after)),
        ]),
        EdgeKind::Deferred { .. } => return Err(bad("unsupported graph-v2 deferred edge")),
    };
    Ok(Value::map([("id", string(&e.id)), ("kind", kind)]))
}
fn decode_edge(value: Value) -> Result<EdgeDecl, Failure> {
    let mut f = fields(value)?;
    let id = text(&mut f, "id")?;
    let mut k = fields(take(&mut f, "kind")?)?;
    let kind = match text(&mut k, "kind")?.as_str() {
        "flow" => {
            let (node, port) = decode_ref(take(&mut k, "from")?)?;
            let from = OutPortRef { node, port };
            let (node, port) = decode_ref(take(&mut k, "to")?)?;
            EdgeKind::Flow {
                from,
                to: InPortRef { node, port },
            }
        }
        "order" => EdgeKind::Order {
            before: text(&mut k, "before")?,
            after: text(&mut k, "after")?,
        },
        _ => return Err(bad("unknown or unsupported graph-v2 edge kind")),
    };
    empty(k)?;
    empty(f)?;
    Ok(EdgeDecl { id, kind })
}

#[cfg(test)]
mod tests;
