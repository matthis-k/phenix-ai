//! Offline event, action and graph declarations.
//! This is one portable declaration model. The kernel decides authority and
//! selected bindings; none of these strings grant execution rights.
use crate::{identifier, operation_key, Code, Failure, Value, ValueSchema, MAX_COUNT};

pub mod v2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDefinition {
    /// Stable semantic event identity, shared across contributing plugins.
    pub id: String,
    pub payload: ValueSchema,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDefinition {
    pub id: String,
    pub event: String,
    pub description: String,
    /// Operation keys required to submit this event. Admission checks grants.
    pub requires: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinPolicy {
    AllActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailurePolicy {
    Skip,
    FailRoot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNode {
    /// Local node name; the kernel qualifies it with the contributing plugin.
    pub id: String,
    /// Event identities. Edges alone never activate a node.
    pub triggers: Vec<String>,
    /// Fully qualified predecessor identities: plugin/node.
    pub after: Vec<String>,
    /// Operation key resolved through the existing selected binding map.
    pub operation: String,
    /// Empty for the owning plugin; otherwise a selected import slot (or slot#index).
    pub selector: String,
    /// Fully qualified property identities declared as written by this node.
    pub writes: Vec<String>,
    pub join: JoinPolicy,
    pub failure: FailurePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphContribution {
    pub plugin: String,
    pub events: Vec<EventDefinition>,
    pub actions: Vec<ActionDefinition>,
    pub nodes: Vec<GraphNode>,
}

/// Static SDK declarations contain no runtime handles or plugin code.
#[derive(Debug, Clone, Copy)]
pub struct StaticEventDefinition {
    pub id: &'static str,
    pub payload: crate::StaticSchema,
}
#[derive(Debug, Clone, Copy)]
pub struct StaticActionDefinition {
    pub id: &'static str,
    pub event: &'static str,
    pub description: &'static str,
    pub requires: &'static [&'static str],
}
#[derive(Debug, Clone, Copy)]
pub struct StaticGraphNode {
    pub id: &'static str,
    pub triggers: &'static [&'static str],
    pub after: &'static [&'static str],
    pub selector: &'static str,
    pub operation: &'static str,
    pub writes: &'static [&'static str],
    pub join: JoinPolicy,
    pub failure: FailurePolicy,
}
#[derive(Debug, Clone, Copy)]
pub struct StaticGraphContribution {
    pub plugin: &'static str,
    pub events: &'static [StaticEventDefinition],
    pub actions: &'static [StaticActionDefinition],
    pub nodes: &'static [StaticGraphNode],
}
impl StaticGraphContribution {
    pub fn owned(self) -> Result<GraphContribution, Failure> {
        GraphContribution {
            plugin: self.plugin.into(),
            events: self
                .events
                .iter()
                .map(|e| {
                    Ok(EventDefinition {
                        id: e.id.into(),
                        payload: e.payload.owned()?,
                    })
                })
                .collect::<Result<_, Failure>>()?,
            actions: self
                .actions
                .iter()
                .map(|a| ActionDefinition {
                    id: a.id.into(),
                    event: a.event.into(),
                    description: a.description.into(),
                    requires: a.requires.iter().map(|v| (*v).into()).collect(),
                })
                .collect(),
            nodes: self
                .nodes
                .iter()
                .map(|n| GraphNode {
                    id: n.id.into(),
                    triggers: n.triggers.iter().map(|v| (*v).into()).collect(),
                    after: n.after.iter().map(|v| (*v).into()).collect(),
                    selector: n.selector.into(),
                    operation: n.operation.into(),
                    writes: n.writes.iter().map(|v| (*v).into()).collect(),
                    join: n.join,
                    failure: n.failure,
                })
                .collect(),
        }
        .normalized()
    }
}

fn bad(message: impl Into<String>) -> Failure {
    Failure::new(Code::DescriptorInvalid, message)
}

fn sorted<'a>(values: impl Iterator<Item = &'a str>) -> bool {
    let values: Vec<_> = values.collect();
    values.len() <= MAX_COUNT && values.windows(2).all(|pair| pair[0] < pair[1])
}

pub fn graph_node_key(plugin: &str, name: &str) -> String {
    format!("{plugin}/{name}")
}

pub fn valid_node_key(key: &str) -> bool {
    key.split_once('/')
        .is_some_and(|(plugin, local)| identifier(plugin) && plugin != "root" && identifier(local))
}

/// State triggers reuse the graph's existing trigger list and canonical
/// encoding. The qualified property key is explicit; an ordering edge alone
/// never subscribes to a value.
pub fn state_change_property(trigger: &str) -> Option<&str> {
    trigger
        .strip_prefix("state:")
        .filter(|key| valid_node_key(key))
}

/// The hook receives the committed epoch and its matching property IDs.
/// Multiple triggers on one node still invoke that node once per epoch.
/// Values are read through selected, authorized property imports.
pub fn state_change_schema() -> ValueSchema {
    ValueSchema::Struct(std::collections::BTreeMap::from([
        ("epoch".into(), ValueSchema::U64),
        (
            "properties".into(),
            ValueSchema::List(Box::new(ValueSchema::String)),
        ),
    ]))
}

impl GraphContribution {
    /// Normalize authored order and check every local declaration before packaging.
    pub fn normalized(&self) -> Result<Self, Failure> {
        let mut result = self.clone();
        result.events.sort_by(|a, b| a.id.cmp(&b.id));
        result.actions.sort_by(|a, b| a.id.cmp(&b.id));
        result.nodes.sort_by(|a, b| a.id.cmp(&b.id));
        for action in &mut result.actions {
            action.requires.sort();
        }
        for node in &mut result.nodes {
            node.triggers.sort();
            node.after.sort();
            node.writes.sort();
        }
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), Failure> {
        if !identifier(&self.plugin)
            || self.plugin == "root"
            || !sorted(self.events.iter().map(|e| e.id.as_str()))
            || !sorted(self.actions.iter().map(|e| e.id.as_str()))
            || !sorted(self.nodes.iter().map(|n| n.id.as_str()))
        {
            return Err(bad("graph contribution identity, order or duplicate"));
        }
        for event in &self.events {
            if !identifier(&event.id) {
                return Err(bad("invalid event identity"));
            }
            event.payload.encode()?;
        }
        for action in &self.actions {
            if !identifier(&action.id)
                || !identifier(&action.event)
                || action.description.trim().is_empty()
                || action.description.len() > 4096
                || !sorted(action.requires.iter().map(String::as_str))
                || action.requires.iter().any(|key| !operation_key(key))
            {
                return Err(bad("invalid action declaration"));
            }
        }
        for node in &self.nodes {
            if !identifier(&node.id)
                || !operation_key(&node.operation)
                || (!node.selector.is_empty() && !identifier(&node.selector.replace('#', ".")))
                || !sorted(node.after.iter().map(String::as_str))
                || !sorted(node.triggers.iter().map(String::as_str))
                || !sorted(node.writes.iter().map(String::as_str))
                || node
                    .triggers
                    .iter()
                    .any(|id| !identifier(id) && state_change_property(id).is_none())
                || node.after.iter().any(|id| !valid_node_key(id))
                || node.writes.iter().any(|id| !valid_node_key(id))
            {
                return Err(bad("invalid graph node declaration"));
            }
            if node.after.contains(&graph_node_key(&self.plugin, &node.id)) {
                return Err(bad("self-dependent graph node"));
            }
        }
        Ok(())
    }

    /// A canonical, portable declaration payload for offline inspection.
    /// Graph metadata has one representation; this does not encode a capability.
    pub fn encode(&self) -> Result<Vec<u8>, Failure> {
        let declaration = self.normalized()?;
        let events = declaration
            .events
            .iter()
            .map(|e| {
                Ok(Value::map([
                    ("id", Value::String(e.id.clone())),
                    ("payload", Value::Bytes(e.payload.encode()?)),
                ]))
            })
            .collect::<Result<Vec<_>, Failure>>()?;
        let actions = declaration
            .actions
            .iter()
            .map(|a| {
                Value::map([
                    ("id", Value::String(a.id.clone())),
                    ("event", Value::String(a.event.clone())),
                    ("description", Value::String(a.description.clone())),
                    ("requires", strings(&a.requires)),
                ])
            })
            .collect();
        let nodes = declaration
            .nodes
            .iter()
            .map(|n| {
                Value::map([
                    ("id", Value::String(n.id.clone())),
                    ("operation", Value::String(n.operation.clone())),
                    ("selector", Value::String(n.selector.clone())),
                    ("triggers", strings(&n.triggers)),
                    ("after", strings(&n.after)),
                    ("writes", strings(&n.writes)),
                    ("join", Value::String("all-active".into())),
                    (
                        "failure",
                        Value::String(
                            match n.failure {
                                FailurePolicy::Skip => "skip",
                                FailurePolicy::FailRoot => "fail-root",
                            }
                            .into(),
                        ),
                    ),
                ])
            })
            .collect();
        Value::map([
            ("format", Value::String("phenix.graph.v1".into())),
            ("plugin", Value::String(declaration.plugin)),
            ("events", Value::List(events)),
            ("actions", Value::List(actions)),
            ("nodes", Value::List(nodes)),
        ])
        .encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        let mut root = fields(Value::decode(bytes)?)?;
        if text(&mut root, "format")? != "phenix.graph.v1" {
            return Err(bad("unknown graph declaration format"));
        }
        let plugin = text(&mut root, "plugin")?;
        let events = list(&mut root, "events")?
            .into_iter()
            .map(|v| {
                let mut f = fields(v)?;
                let id = text(&mut f, "id")?;
                let payload = match take(&mut f, "payload")? {
                    Value::Bytes(bytes) => ValueSchema::decode(&bytes)?,
                    _ => return Err(bad("event payload schema is not bytes")),
                };
                empty(f)?;
                Ok(EventDefinition { id, payload })
            })
            .collect::<Result<Vec<_>, Failure>>()?;
        let actions = list(&mut root, "actions")?
            .into_iter()
            .map(|v| {
                let mut f = fields(v)?;
                let id = text(&mut f, "id")?;
                let event = text(&mut f, "event")?;
                let description = text(&mut f, "description")?;
                let requires = text_list(&mut f, "requires")?;
                empty(f)?;
                Ok(ActionDefinition {
                    id,
                    event,
                    description,
                    requires,
                })
            })
            .collect::<Result<Vec<_>, Failure>>()?;
        let nodes = list(&mut root, "nodes")?
            .into_iter()
            .map(|v| {
                let mut f = fields(v)?;
                let id = text(&mut f, "id")?;
                let operation = text(&mut f, "operation")?;
                let selector = text(&mut f, "selector")?;
                let triggers = text_list(&mut f, "triggers")?;
                let after = text_list(&mut f, "after")?;
                let writes = text_list(&mut f, "writes")?;
                if text(&mut f, "join")? != "all-active" {
                    return Err(bad("unknown graph join policy"));
                }
                let failure = match text(&mut f, "failure")?.as_str() {
                    "skip" => FailurePolicy::Skip,
                    "fail-root" => FailurePolicy::FailRoot,
                    _ => return Err(bad("unknown graph failure policy")),
                };
                empty(f)?;
                Ok(GraphNode {
                    id,
                    operation,
                    selector,
                    triggers,
                    after,
                    writes,
                    join: JoinPolicy::AllActive,
                    failure,
                })
            })
            .collect::<Result<Vec<_>, Failure>>()?;
        empty(root)?;
        let declaration = Self {
            plugin,
            events,
            actions,
            nodes,
        };
        declaration.validate()?;
        if declaration.encode()? != bytes {
            return Err(bad("noncanonical graph declaration"));
        }
        Ok(declaration)
    }
}
fn strings(values: &[String]) -> Value {
    Value::List(values.iter().cloned().map(Value::String).collect())
}
type Fields = std::collections::BTreeMap<String, Value>;
fn fields(value: Value) -> Result<Fields, Failure> {
    match value {
        Value::Map(map) => Ok(map),
        _ => Err(bad("expected graph map")),
    }
}
fn take(fields: &mut Fields, key: &str) -> Result<Value, Failure> {
    fields
        .remove(key)
        .ok_or_else(|| bad(format!("missing graph field {key}")))
}
fn text(fields: &mut Fields, key: &str) -> Result<String, Failure> {
    match take(fields, key)? {
        Value::String(s) => Ok(s),
        _ => Err(bad(format!("graph field {key} must be text"))),
    }
}
fn list(fields: &mut Fields, key: &str) -> Result<Vec<Value>, Failure> {
    match take(fields, key)? {
        Value::List(v) if v.len() <= MAX_COUNT => Ok(v),
        _ => Err(bad(format!("graph field {key} must be bounded list"))),
    }
}
fn text_list(fields: &mut Fields, key: &str) -> Result<Vec<String>, Failure> {
    list(fields, key)?
        .into_iter()
        .map(|v| match v {
            Value::String(s) => Ok(s),
            _ => Err(bad("graph list element must be text")),
        })
        .collect()
}
fn empty(fields: Fields) -> Result<(), Failure> {
    if fields.is_empty() {
        Ok(())
    } else {
        Err(bad("unexpected graph declaration field"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> GraphContribution {
        GraphContribution {
            plugin: "search".into(),
            events: vec![EventDefinition {
                id: "prompt.submitted".into(),
                payload: ValueSchema::String,
            }],
            actions: vec![ActionDefinition {
                id: "prompt".into(),
                event: "prompt.submitted".into(),
                description: "Submit a prompt".into(),
                requires: vec!["agent.input@1/submit".into()],
            }],
            nodes: vec![GraphNode {
                id: "retrieve".into(),
                triggers: vec!["prompt.submitted".into()],
                after: vec![],
                operation: "search.api@1/retrieve".into(),
                selector: String::new(),
                writes: vec!["search/results".into()],
                join: JoinPolicy::AllActive,
                failure: FailurePolicy::FailRoot,
            }],
        }
    }
    #[test]
    fn static_sdk_declarations_project_into_canonical_schema() {
        const GRAPH: StaticGraphContribution = StaticGraphContribution {
            plugin: "search",
            events: &[StaticEventDefinition {
                id: "prompt.submitted",
                payload: crate::StaticSchema::String,
            }],
            actions: &[StaticActionDefinition {
                id: "prompt",
                event: "prompt.submitted",
                description: "Submit a prompt",
                requires: &["agent.input@1/submit"],
            }],
            nodes: &[StaticGraphNode {
                id: "retrieve",
                triggers: &["prompt.submitted"],
                after: &[],
                operation: "search.api@1/retrieve",
                selector: "",
                writes: &["search/results"],
                join: JoinPolicy::AllActive,
                failure: FailurePolicy::FailRoot,
            }],
        };
        assert_eq!(
            GRAPH.owned().unwrap().encode().unwrap(),
            sample().encode().unwrap()
        );
    }
    #[test]
    fn portable_roundtrip_and_authored_order() {
        let source = sample();
        let bytes = source.encode().unwrap();
        assert_eq!(GraphContribution::decode(&bytes).unwrap(), source);
        let mut reversed = source.clone();
        reversed.nodes.push(GraphNode {
            id: "archive".into(),
            triggers: vec![],
            after: vec![],
            operation: "search.api@1/archive".into(),
            selector: String::new(),
            writes: vec![],
            join: JoinPolicy::AllActive,
            failure: FailurePolicy::Skip,
        });
        reversed.nodes.reverse();
        assert_eq!(
            reversed.encode().unwrap(),
            reversed.normalized().unwrap().encode().unwrap()
        );
    }
    #[test]
    fn state_trigger_uses_existing_canonical_graph_encoding() {
        let mut graph = sample();
        graph.nodes[0].triggers = vec!["state:skills.api/items".into()];
        let bytes = graph.encode().unwrap();
        assert_eq!(GraphContribution::decode(&bytes).unwrap(), graph);
        assert_eq!(
            state_change_property("state:skills.api/items"),
            Some("skills.api/items")
        );
        assert!(state_change_property("state:skills.api/").is_none());
        assert!(state_change_property("state:root/items").is_none());
        let mut malformed = graph;
        malformed.nodes[0].triggers = vec!["state:invalid".into()];
        assert!(malformed.encode().is_err());
        assert_eq!(
            state_change_schema(),
            ValueSchema::Struct(std::collections::BTreeMap::from([
                ("epoch".into(), ValueSchema::U64),
                (
                    "properties".into(),
                    ValueSchema::List(Box::new(ValueSchema::String)),
                ),
            ]))
        );
    }

    #[test]
    fn duplicates_and_malformed_payloads_reject() {
        let mut bad = sample();
        let duplicate = bad.nodes[0].clone();
        bad.nodes.push(duplicate);
        assert!(bad.encode().is_err());
        let mut bad = sample();
        bad.actions[0].requires.push("agent.input@1/submit".into());
        assert!(bad.encode().is_err());
        let bytes = sample().encode().unwrap();
        let Value::Map(mut map) = Value::decode(&bytes).unwrap() else {
            panic!()
        };
        map.insert("unknown".into(), Value::Null);
        assert!(GraphContribution::decode(&Value::Map(map).encode().unwrap()).is_err());
    }
}
