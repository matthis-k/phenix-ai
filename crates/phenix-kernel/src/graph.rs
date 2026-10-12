//! Offline graph composition. Runtime execution and event admission belong to G3.
//! The graph uses the shared DAG ordering algorithm but owns graph semantics.
use crate::dag;
use phenix_contract::{
    graph_node_key, ActionDefinition, Code, EventDefinition, Failure, GraphContribution, GraphNode,
    Value, ValueSchema,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionInfo {
    pub id: String,
    pub event: String,
    pub description: String,
    pub payload: ValueSchema,
    pub required_operations: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CompiledNode {
    pub owner: String,
    pub provider: String,
    pub definition: GraphNode,
}

#[derive(Debug, Clone)]
pub struct CompiledGraph {
    /// SHA-256 of sorted canonical contributions, domain separated from generations.
    pub identity: String,
    pub nodes: BTreeMap<String, CompiledNode>,
    pub events: BTreeMap<String, EventDefinition>,
    pub actions: BTreeMap<String, ActionInfo>,
    /// Static predecessor constraints; NOT an activation list.
    pub predecessors: BTreeMap<String, BTreeSet<String>>,
    pub order: Vec<String>,
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure::new(Code::DescriptorInvalid, message)
}

fn ancestor(start: &str, sought: &str, predecessors: &BTreeMap<String, BTreeSet<String>>) -> bool {
    let mut pending = vec![start];
    let mut visited = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !visited.insert(node) {
            continue;
        }
        for predecessor in &predecessors[node] {
            if predecessor == sought {
                return true;
            }
            pending.push(predecessor);
        }
    }
    false
}

impl CompiledGraph {
    /// Compile declarations without activating a plugin.
    ///
    /// The trusted resolver supplies a target lookup. It MUST authenticate the
    /// declaring plugin and selected slot against prepared bindings and grants.
    /// Keeping the lookup separate makes offline rejection independently testable.
    pub fn compile(
        declarations: &[GraphContribution],
        mut resolve: impl FnMut(&str, &str, &str) -> Result<String, Failure>,
    ) -> Result<Self, Failure> {
        let mut normalized = declarations
            .iter()
            .map(GraphContribution::normalized)
            .collect::<Result<Vec<_>, _>>()?;
        normalized.sort_by(|a, b| a.plugin.cmp(&b.plugin));
        if normalized
            .windows(2)
            .any(|pair| pair[0].plugin == pair[1].plugin)
        {
            return Err(invalid("duplicate contributing plugin"));
        }
        let mut events = BTreeMap::new();
        let mut action_defs = BTreeMap::<String, ActionDefinition>::new();
        let mut nodes = BTreeMap::new();
        let mut predecessors = BTreeMap::new();

        for declaration in &normalized {
            for event in &declaration.events {
                if events.insert(event.id.clone(), event.clone()).is_some() {
                    return Err(invalid(format!("duplicate event {}", event.id)));
                }
            }
            for action in &declaration.actions {
                if action_defs
                    .insert(action.id.clone(), action.clone())
                    .is_some()
                {
                    return Err(invalid(format!("duplicate action {}", action.id)));
                }
            }
            for node in &declaration.nodes {
                let id = graph_node_key(&declaration.plugin, &node.id);
                let provider = resolve(&declaration.plugin, &node.selector, &node.operation)?;
                if provider.is_empty() {
                    return Err(invalid(format!("unresolved graph operation {id}")));
                }
                predecessors.insert(id.clone(), node.after.iter().cloned().collect());
                nodes.insert(
                    id,
                    CompiledNode {
                        owner: declaration.plugin.clone(),
                        provider,
                        definition: node.clone(),
                    },
                );
            }
        }

        let mut actions = BTreeMap::new();
        for (id, definition) in action_defs {
            let event = events.get(&definition.event).ok_or_else(|| {
                invalid(format!(
                    "action {id} references absent event {}",
                    definition.event
                ))
            })?;
            actions.insert(
                id.clone(),
                ActionInfo {
                    id,
                    event: definition.event,
                    description: definition.description,
                    payload: event.payload.clone(),
                    required_operations: definition.requires,
                },
            );
        }
        for (id, node) in &nodes {
            for trigger in &node.definition.triggers {
                if !events.contains_key(trigger) {
                    return Err(invalid(format!("node {id} has absent trigger {trigger}")));
                }
            }
        }
        for (id, dependencies) in &predecessors {
            for dependency in dependencies {
                if !nodes.contains_key(dependency) {
                    return Err(invalid(format!(
                        "node {id} has missing anchor {dependency}"
                    )));
                }
            }
        }
        let order = dag::order(&predecessors).map_err(|cycle| {
            invalid(format!("reactive dependency cycle: {}", cycle.join(" -> ")))
        })?;

        // Multiple writers for a cell are valid only when graph order fixes
        // their relative order. A shared event may activate unrelated roots.
        let mut writers = BTreeMap::<String, Vec<String>>::new();
        for (id, node) in &nodes {
            for write in &node.definition.writes {
                writers.entry(write.clone()).or_default().push(id.clone());
            }
        }
        for (property, ids) in writers {
            for (index, left) in ids.iter().enumerate() {
                for right in ids.iter().skip(index + 1) {
                    if !ancestor(left, right, &predecessors)
                        && !ancestor(right, left, &predecessors)
                    {
                        return Err(invalid(format!(
                            "unordered writers for {property}: {left}, {right}"
                        )));
                    }
                }
            }
        }

        let canonical = normalized
            .iter()
            .map(|definition| Ok(Value::Bytes(definition.encode()?)))
            .collect::<Result<Vec<_>, Failure>>()?;
        let mut bytes = b"phenix.graph.identity.v1\0".to_vec();
        bytes.extend(Value::List(canonical).encode()?);
        let identity = phenix_artifact::digest(&bytes);
        Ok(Self {
            identity,
            nodes,
            events,
            actions,
            predecessors,
            order,
        })
    }

    /// G3 preparation: derive the active projection for one admitted event set.
    /// Declared ordering edges do not activate successors or dormant ancestors.
    /// Runtime execution is deliberately outside this method.
    pub fn activation_order(&self, event_ids: &BTreeSet<String>) -> Result<Vec<String>, Failure> {
        for id in event_ids {
            if !self.events.contains_key(id) {
                return Err(invalid(format!("unknown admitted event {id}")));
            }
        }
        let active: BTreeSet<_> = self
            .nodes
            .iter()
            .filter(|(_, node)| {
                node.definition
                    .triggers
                    .iter()
                    .any(|id| event_ids.contains(id))
            })
            .map(|(id, _)| id.clone())
            .collect();
        let projection = active
            .iter()
            .map(|id| {
                let dependencies = self.predecessors[id]
                    .intersection(&active)
                    .cloned()
                    .collect();
                (id.clone(), dependencies)
            })
            .collect();
        dag::order(&projection).map_err(|cycle| {
            invalid(format!(
                "invalid active graph projection: {}",
                cycle.join(" -> ")
            ))
        })
    }

    pub fn discover(&self) -> impl Iterator<Item = &ActionInfo> {
        self.actions.values()
    }
}

impl crate::resolve::Prepared {
    /// Only the established M1 resolver may turn a declaration into a
    /// dispatchable target. The graph compiler never grants permissions.
    pub fn compile_graph(
        &self,
        declarations: &[GraphContribution],
    ) -> Result<CompiledGraph, Failure> {
        let graph = CompiledGraph::compile(declarations, |caller, selector, key| {
            let provider = if selector.is_empty() {
                caller.to_owned()
            } else {
                let identity = phenix_contract::import_identity(selector, key);
                self.bindings
                    .get(&(caller.to_owned(), identity))
                    .cloned()
                    .ok_or_else(|| Failure::new(Code::MissingBinding, "unselected graph import"))?
            };
            if !self
                .operations
                .contains_key(&(provider.clone(), key.to_owned()))
            {
                return Err(Failure::new(Code::MissingBinding, "graph operation absent"));
            }
            if !self.permits(caller, key)
                || !self
                    .grants
                    .contains_key(&(caller.to_owned(), key.to_owned()))
            {
                return Err(Failure::new(
                    Code::Denied,
                    "graph operation lacks prepared grant",
                ));
            }
            Ok(provider)
        })?;
        for (id, node) in &graph.nodes {
            let operation = &self.operations[&(node.provider.clone(), node.definition.operation.clone())];
            for event in &node.definition.triggers {
                if operation.schema.request != graph.events[event].payload {
                    return Err(Failure::new(
                        Code::SchemaRejected,
                        format!("node {id} request schema does not match event {event}"),
                    ));
                }
            }
            for write in &node.definition.writes {
                let (owner, property) = write.split_once('/').expect("validated property key");
                let target = self.packages.get(owner)
                    .and_then(|package| package.metadata().properties.iter().find(|field| field.id == property))
                    .ok_or_else(|| Failure::new(Code::MissingBinding, format!("graph property {write} not selected")))?;
                if !target.set {
                    return Err(Failure::new(Code::Denied, format!("graph property {write} is read-only")));
                }
                let key = format!("{}/set", phenix_contract::property_contract(owner, property)?);
                if !self.permits(&node.owner, &key)
                    || !self.grants.contains_key(&(node.owner.clone(), key))
                {
                    return Err(Failure::new(Code::Denied, format!("graph property {write} has no write grant")));
                }
            }
        }
        Ok(graph)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_contract::{FailurePolicy, JoinPolicy};

    fn node(id: &str, trigger: &[&str], after: &[&str], writes: &[&str]) -> GraphNode {
        GraphNode {
            id: id.into(),
            operation: format!("examples.graph@1/{id}"),
            selector: "source".into(),
            triggers: trigger.iter().map(|v| (*v).into()).collect(),
            after: after.iter().map(|v| (*v).into()).collect(),
            writes: writes.iter().map(|v| (*v).into()).collect(),
            join: JoinPolicy::AllActive,
            failure: FailurePolicy::FailRoot,
        }
    }
    fn contribution(owner: &str, events: bool, nodes: Vec<GraphNode>) -> GraphContribution {
        GraphContribution {
            plugin: owner.into(),
            events: if events {
                vec![EventDefinition {
                    id: "event.prompt".into(),
                    payload: ValueSchema::String,
                }]
            } else {
                vec![]
            },
            actions: if events {
                vec![ActionDefinition {
                    id: "submit".into(),
                    event: "event.prompt".into(),
                    description: "Submit".into(),
                    requires: vec!["agent.input@1/submit".into()],
                }]
            } else {
                vec![]
            },
            nodes,
        }
    }
    fn compile(input: &[GraphContribution]) -> Result<CompiledGraph, Failure> {
        CompiledGraph::compile(input, |owner, selector, key| {
            if selector == "source" && owner != "forbidden" && key.starts_with("examples.graph@1/")
            {
                Ok("provider".into())
            } else {
                Err(Failure::new(
                    Code::Denied,
                    "graph target has no selected grant",
                ))
            }
        })
    }
    fn fixtures() -> Vec<GraphContribution> {
        vec![
            contribution("alpha", true, vec![node("a", &["event.prompt"], &[], &[])]),
            contribution("beta", false, vec![node("b", &["event.prompt"], &[], &[])]),
            contribution(
                "gamma",
                false,
                vec![node("c", &["event.prompt"], &["alpha/a", "beta/b"], &[])],
            ),
            contribution("delta", false, vec![node("d", &[], &["beta/b"], &[])]),
        ]
    }
    #[test]
    fn independently_contributed_multi_root_graph_is_permutation_invariant() {
        let nodes = fixtures();
        let graph = compile(&nodes).unwrap();
        let mut reverse = nodes;
        reverse.reverse();
        let other = compile(&reverse).unwrap();
        assert_eq!(graph.identity, other.identity);
        assert_eq!(graph.order, other.order);
        assert_eq!(graph.order, ["alpha/a", "beta/b", "delta/d", "gamma/c"]);
        assert_eq!(graph.predecessors["gamma/c"].len(), 2);
        assert_eq!(
            graph.discover().next().unwrap().payload,
            ValueSchema::String
        );
    }
    #[test]
    fn malformed_anchors_cycles_triggers_and_conflicts_fail_offline() {
        let mut bad = fixtures();
        bad[0].nodes[0].after.push("absent/anchor".into());
        assert!(compile(&bad).is_err());
        let mut bad = fixtures();
        bad[0].nodes[0].after.push("gamma/c".into());
        assert!(compile(&bad).unwrap_err().message.contains("cycle"));
        let mut bad = fixtures();
        bad[0].nodes[0].triggers.push("unknown".into());
        assert!(compile(&bad).is_err());
        let mut bad = fixtures();
        bad[0].nodes[0].writes = vec!["owner/cell".into()];
        bad[1].nodes[0].writes = vec!["owner/cell".into()];
        assert!(compile(&bad)
            .unwrap_err()
            .message
            .contains("unordered writers"));
    }
    #[test]
    fn duplicate_event_action_node_and_denied_target_fail() {
        let mut bad = fixtures();
        bad[1].events = bad[0].events.clone();
        assert!(compile(&bad).is_err());
        let mut bad = fixtures();
        bad[1].actions = bad[0].actions.clone();
        assert!(compile(&bad).is_err());
        let mut bad = fixtures();
        let duplicate = bad[0].nodes[0].clone();
        bad[0].nodes.push(duplicate);
        assert!(compile(&bad).is_err());
        let mut bad = fixtures();
        bad[1].plugin = "forbidden".into();
        assert_eq!(compile(&bad).unwrap_err().code, Code::Denied);
    }
    #[test]
    fn after_edges_do_not_trigger_and_dormant_predecessors_never_block() {
        let input = vec![
            contribution("alpha", true, vec![node("a", &["event.prompt"], &[], &[])]),
            contribution("beta", false, vec![node("b", &[], &[], &[])]),
            contribution(
                "gamma",
                false,
                vec![node("c", &["event.prompt"], &["alpha/a", "beta/b"], &[])],
            ),
            contribution("delta", false, vec![node("d", &[], &["alpha/a"], &[])]),
        ];
        let graph = compile(&input).unwrap();
        let active = BTreeSet::from(["event.prompt".into()]);
        assert_eq!(
            graph.activation_order(&active).unwrap(),
            ["alpha/a", "gamma/c"]
        );
        assert!(graph
            .activation_order(&BTreeSet::from(["unknown".into()]))
            .is_err());
        assert!(graph.activation_order(&BTreeSet::new()).unwrap().is_empty());
    }

    #[test]
    fn ordered_writers_are_permitted() {
        let input = vec![contribution(
            "alpha",
            true,
            vec![
                node("a", &["event.prompt"], &[], &["alpha/value"]),
                node("b", &["event.prompt"], &["alpha/a"], &["alpha/value"]),
            ],
        )];
        assert!(compile(&input).is_ok());
    }
}
