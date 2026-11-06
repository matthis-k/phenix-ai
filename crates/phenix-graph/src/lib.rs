//! The one graph engine: symbolic contributions, deterministic compilation and typed
//! delivery. Native ABI dispatch and effect policy belong to the host, not this engine.
//! WIP: pending native tickets, coloring, module flattening and custom policies are
//! not yet ported from the separate prototype.
pub use phenix_contract::graph::{
    Edge, Edit, InputPort, InputRef, Node, OutputPolicy, OutputRef, Patch, Readiness, Schema,
    Trigger,
};
use phenix_contract::{Code, Failure, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
fn fail(msg: impl Into<String>) -> Failure {
    Failure::new(Code::SchemaRejected, msg)
}

#[derive(Clone, Debug, Default)]
pub struct CompiledGraph {
    pub identity: String,
    nodes: BTreeMap<String, Node>,
    owners: BTreeMap<String, String>,
    edges: BTreeSet<Edge>,
    output_policies: BTreeMap<OutputRef, OutputPolicy>,
    triggers: BTreeMap<String, InputRef>,
    applied: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Target {
    Node(String),
    Edge(Edge),
    OutputPort(OutputRef),
    Trigger(String),
}
fn target(edit: &Edit) -> Target {
    match edit {
        Edit::AddNode(n) | Edit::ReplaceNode(n) => Target::Node(n.path.clone()),
        Edit::RemoveNode(n) => Target::Node(n.clone()),
        Edit::Connect(e) | Edit::Disconnect(e) => Target::Edge(e.clone()),
        Edit::SetOutputPolicy { from, .. } => Target::OutputPort(from.clone()),
        Edit::AddTrigger(t) => Target::Trigger(t.name.clone()),
        Edit::RemoveTrigger(n) => Target::Trigger(n.clone()),
    }
}
impl CompiledGraph {
    /// Contributions may be submitted in any order. A patch is the only
    /// construction primitive, including the first patch over an empty graph.
    pub fn compile(patches: &[Patch]) -> Result<Self, Failure> {
        let mut pending = BTreeMap::<String, Patch>::new();
        for patch in patches {
            patch.validate()?;
            if pending.insert(patch.id.clone(), patch.clone()).is_some() {
                return Err(fail(format!("duplicate patch {}", patch.id)));
            }
        }
        for patch in pending.values() {
            for dep in &patch.after {
                if !pending.contains_key(dep) {
                    return Err(fail(format!("missing patch dependency {dep}")));
                }
            }
        }
        let mut graph = Self::default();
        let mut identity = Sha256::new();
        identity.update(b"phenix.graph.patch-plan.v1\0");
        let mut closed = BTreeSet::<String>::new();
        let mut ancestry = BTreeMap::<String, BTreeSet<String>>::new();
        let mut writers = BTreeMap::<Target, String>::new();
        while !pending.is_empty() {
            let id = pending
                .iter()
                .find(|(_, patch)| patch.after.is_subset(&closed))
                .map(|(id, _)| id.clone())
                .ok_or_else(|| fail("patch dependency cycle"))?;
            let patch = pending.remove(&id).expect("chosen patch");
            let bytes = patch.encode()?;
            identity.update((bytes.len() as u64).to_be_bytes());
            identity.update(&bytes);
            let mut previous = patch.after.clone();
            for dep in &patch.after {
                previous.extend(ancestry[dep].iter().cloned());
            }
            for edit in &patch.edits {
                let key = target(edit);
                if let Some(owner) = writers.get(&key) {
                    if owner != &id && !previous.contains(owner) {
                        return Err(fail(format!(
                            "unordered writes by {owner} and {id} to {key:?}"
                        )));
                    }
                }
                graph.apply(edit, &id)?;
                writers.insert(key, id.clone());
            }
            graph.applied.push(id.clone());
            ancestry.insert(id.clone(), previous);
            closed.insert(id);
        }
        for edge in &graph.edges {
            if graph.output_schema(&edge.from)? != graph.input_schema(&edge.to)? {
                return Err(fail(format!("edge type mismatch {edge:?}")));
            }
        }
        for source in graph.output_policies.keys() {
            graph.output_schema(source)?;
        }
        for endpoint in graph.triggers.values() {
            graph.input_schema(endpoint)?;
        }
        graph.identity = format!("{:x}", identity.finalize());
        Ok(graph)
    }

    fn apply(&mut self, edit: &Edit, patch_id: &str) -> Result<(), Failure> {
        match edit {
            Edit::AddNode(n) => {
                if self.nodes.contains_key(&n.path) {
                    return Err(fail("node already exists"));
                }
                self.nodes.insert(n.path.clone(), n.clone());
                self.owners.insert(n.path.clone(), patch_id.into());
            }
            Edit::ReplaceNode(n) => {
                let old = self
                    .nodes
                    .get(&n.path)
                    .ok_or_else(|| fail("replacement node absent"))?;
                if old.inputs != n.inputs || old.outputs != n.outputs {
                    return Err(fail("replacement changes node interface"));
                }
                self.nodes.insert(n.path.clone(), n.clone());
                self.owners.insert(n.path.clone(), patch_id.into());
            }
            Edit::RemoveNode(name) => {
                if self.nodes.remove(name).is_none() {
                    return Err(fail("node absent"));
                }
                self.owners.remove(name);
            }
            Edit::Connect(edge) => {
                if !self.edges.insert(edge.clone()) {
                    return Err(fail("duplicate edge"));
                }
            }
            Edit::Disconnect(edge) => {
                if !self.edges.remove(edge) {
                    return Err(fail("edge absent"));
                }
            }
            Edit::SetOutputPolicy { from, policy } => {
                self.output_policies.insert(from.clone(), policy.clone());
            }
            Edit::AddTrigger(t) => {
                if self.triggers.contains_key(&t.name) {
                    return Err(fail("duplicate trigger"));
                }
                self.triggers.insert(t.name.clone(), t.to.clone());
            }
            Edit::RemoveTrigger(name) => {
                if self.triggers.remove(name).is_none() {
                    return Err(fail("trigger absent"));
                }
            }
        }
        Ok(())
    }
    pub fn applied(&self) -> &[String] {
        &self.applied
    }
    pub fn nodes(&self) -> &BTreeMap<String, Node> {
        &self.nodes
    }
    /// Contributor responsible for the current node implementation.
    pub fn owner(&self, path: &str) -> Option<&str> {
        self.owners.get(path).map(String::as_str)
    }
    pub fn edges(&self) -> &BTreeSet<Edge> {
        &self.edges
    }
    /// Default is broadcast; explicit policies can select fewer edges.
    pub fn output_policy(&self, from: &OutputRef) -> OutputPolicy {
        self.output_policies
            .get(from)
            .cloned()
            .unwrap_or(OutputPolicy::Broadcast)
    }
    pub fn triggers(&self) -> &BTreeMap<String, InputRef> {
        &self.triggers
    }
    fn input_schema(&self, e: &InputRef) -> Result<&Schema, Failure> {
        self.nodes
            .get(&e.node)
            .and_then(|n| n.inputs.get(&e.port))
            .and_then(|p| p.slots.get(&e.slot))
            .ok_or_else(|| fail(format!("missing input {e:?}")))
    }
    fn output_schema(&self, e: &OutputRef) -> Result<&Schema, Failure> {
        self.nodes
            .get(&e.node)
            .and_then(|n| n.outputs.get(&e.port))
            .ok_or_else(|| fail(format!("missing output {e:?}")))
    }
    fn deliver(
        &self,
        queues: &mut BTreeMap<InputRef, VecDeque<Value>>,
        endpoint: &InputRef,
        value: Value,
    ) -> Result<(), Failure> {
        if !self.input_schema(endpoint)?.accepts(&value)? {
            return Err(fail("input value rejected by schema"));
        }
        queues.entry(endpoint.clone()).or_default().push_back(value);
        Ok(())
    }
    fn ready(node: &Node, queues: &BTreeMap<InputRef, VecDeque<Value>>) -> bool {
        node.inputs.iter().all(|(port, p)| {
            let available = |slot: &String| {
                queues
                    .get(&InputRef {
                        node: node.path.clone(),
                        port: port.clone(),
                        slot: slot.clone(),
                    })
                    .is_some_and(|q| !q.is_empty())
            };
            match p.readiness {
                Readiness::All => p.slots.keys().all(available),
                Readiness::Any => p.slots.keys().any(available),
            }
        })
    }
    fn consume(
        node: &Node,
        queues: &mut BTreeMap<InputRef, VecDeque<Value>>,
    ) -> BTreeMap<String, Value> {
        let mut args = BTreeMap::new();
        for (port_name, port) in &node.inputs {
            for slot in port.slots.keys() {
                let key = InputRef {
                    node: node.path.clone(),
                    port: port_name.clone(),
                    slot: slot.clone(),
                };
                if let Some(value) = queues.get_mut(&key).and_then(VecDeque::pop_front) {
                    args.insert(format!("{port_name}.{slot}"), value);
                    if port.readiness == Readiness::Any {
                        break;
                    }
                }
            }
        }
        args
    }

    /// Run the same incremental session used for native pending dispatch,
    /// draining local activations synchronously through the supplied host.
    pub fn execute(
        &self,
        trigger: &str,
        payload: Value,
        host: &mut impl ActionHost,
        activation_limit: Option<usize>,
    ) -> Result<ExecutionReport, Failure> {
        let mut session = self.start(trigger, payload)?;
        while let Some(activation) = session.next_ready(activation_limit)? {
            let outputs = host.invoke(&activation.action, activation.args)?;
            session.complete_with(
                activation.id,
                outputs,
                |from, selector, value, candidates| host.route(selector, from, value, candidates),
            )?;
        }
        session.finish()
    }
}
mod session;
pub use session::{Activation, GraphSession};

pub trait ActionHost {
    fn invoke(
        &mut self,
        action: &str,
        args: BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>, Failure>;

    /// Run an output policy after the node has emitted its value.
    /// A policy may inspect store state in its host implementation but may
    /// select only existing outgoing edges; GraphSession checks that invariant.
    fn route(
        &mut self,
        _selector: &str,
        _from: &OutputRef,
        _value: &Value,
        _candidates: &[Edge],
    ) -> Result<Vec<Edge>, Failure> {
        Err(fail("selective output policy has no host implementation"))
    }
}
#[derive(Clone, Debug, Default)]
pub struct ExecutionReport {
    pub activations: usize,
    pub outputs: BTreeMap<OutputRef, Vec<Value>>,
    pub unconsumed: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn schema() -> Schema {
        Schema::of(&phenix_contract::ValueSchema::I64).unwrap()
    }
    fn single() -> InputPort {
        InputPort {
            slots: [("value".into(), schema())].into(),
            readiness: Readiness::All,
        }
    }
    fn node(name: &str, action: &str) -> Node {
        Node {
            path: name.into(),
            action: action.into(),
            inputs: [("run".into(), single())].into(),
            outputs: [("value".into(), schema())].into(),
        }
    }
    fn input(name: &str, slot: &str) -> InputRef {
        InputRef {
            node: name.into(),
            port: "run".into(),
            slot: slot.into(),
        }
    }
    fn route_edge(from: &str, to: &str, slot: &str) -> Edge {
        Edge {
            from: OutputRef {
                node: from.into(),
                port: "value".into(),
            },
            to: input(to, slot),
        }
    }
    fn edge(from: &str, to: &str, slot: &str) -> Edit {
        Edit::Connect(route_edge(from, to, slot))
    }
    struct Host;
    impl ActionHost for Host {
        fn invoke(
            &mut self,
            action: &str,
            args: BTreeMap<String, Value>,
        ) -> Result<BTreeMap<String, Value>, Failure> {
            let get = |key: &str| match args.get(key) {
                Some(Value::I64(n)) => Ok(*n),
                _ => Err(fail("wrong input")),
            };
            let value = match action {
                "source" => get("run.value")?,
                "double" => 2 * get("run.value")?,
                "triple" => 3 * get("run.value")?,
                "join" => get("run.left")? + get("run.right")?,
                _ => return Err(fail("action absent")),
            };
            Ok([("value".into(), Value::I64(value))].into())
        }
    }
    #[test]
    fn two_contributions_form_diamond_in_any_order() {
        let mut base = Patch::new("base");
        base.push(Edit::AddNode(node("a", "source")));
        let mut join = node("d", "join");
        join.inputs.get_mut("run").unwrap().slots =
            [("left".into(), schema()), ("right".into(), schema())].into();
        base.push(Edit::AddNode(join));
        base.push(Edit::AddTrigger(Trigger {
            name: "run".into(),
            to: input("a", "value"),
        }));
        let mut b = Patch::new("branch.b").after("base");
        b.push(Edit::AddNode(node("b", "double")));
        b.push(edge("a", "b", "value"));
        b.push(edge("b", "d", "left"));
        let mut c = Patch::new("branch.c").after("base");
        c.push(Edit::AddNode(node("c", "triple")));
        c.push(edge("a", "c", "value"));
        c.push(edge("c", "d", "right"));
        let compiled = CompiledGraph::compile(&[c, base, b]).unwrap();
        assert_eq!(compiled.applied(), ["base", "branch.b", "branch.c"]);
        let result = compiled
            .execute("run", Value::I64(5), &mut Host, Some(10))
            .unwrap();
        let output = OutputRef {
            node: "d".into(),
            port: "value".into(),
        };
        assert_eq!(result.outputs[&output], vec![Value::I64(25)]);
    }
    #[test]
    fn missing_dependency_is_rejected() {
        assert!(CompiledGraph::compile(&[Patch::new("a").after("unknown")]).is_err());
    }
    #[test]
    fn identity_is_independent_of_contributor_arrival_order() {
        let mut a = Patch::new("a");
        a.push(Edit::AddNode(node("source", "source")));
        a.push(Edit::AddTrigger(Trigger {
            name: "run".into(),
            to: input("source", "value"),
        }));
        let mut b = Patch::new("b").after("a");
        b.push(Edit::AddNode(node("twice", "double")));
        b.push(edge("source", "twice", "value"));
        let ordered = CompiledGraph::compile(&[a.clone(), b.clone()]).unwrap();
        let reordered = CompiledGraph::compile(&[b, a]).unwrap();
        assert_eq!(ordered.identity, reordered.identity);
        assert!(!ordered.identity.is_empty());
    }

    #[test]
    fn replacement_provenance_names_the_replacing_patch() {
        let mut base = Patch::new("plugin.base");
        base.push(Edit::AddNode(node("n", "source")));
        let mut replacement = Patch::new("plugin.replace").after("plugin.base");
        replacement.push(Edit::ReplaceNode(node("n", "double")));
        let graph = CompiledGraph::compile(&[replacement, base]).unwrap();
        assert_eq!(graph.owner("n"), Some("plugin.replace"));
        assert_eq!(graph.nodes()["n"].action, "double");
    }

    fn selective_fanout() -> CompiledGraph {
        let mut base = Patch::new("base");
        base.push(Edit::AddNode(node("a", "source")));
        base.push(Edit::AddNode(node("b", "double")));
        base.push(Edit::AddNode(node("c", "triple")));
        base.push(edge("a", "b", "value"));
        base.push(edge("a", "c", "value"));
        base.push(Edit::AddTrigger(Trigger {
            name: "run".into(),
            to: input("a", "value"),
        }));
        let mut routing = Patch::new("route").after("base");
        routing.output_policy(
            OutputRef {
                node: "a".into(),
                port: "value".into(),
            },
            OutputPolicy::Select {
                selector: "choose.branch".into(),
            },
        );
        CompiledGraph::compile(&[routing, base]).unwrap()
    }

    struct SelectHost;
    impl ActionHost for SelectHost {
        fn invoke(
            &mut self,
            action: &str,
            args: BTreeMap<String, Value>,
        ) -> Result<BTreeMap<String, Value>, Failure> {
            Host.invoke(action, args)
        }

        fn route(
            &mut self,
            selector: &str,
            _from: &OutputRef,
            value: &Value,
            candidates: &[Edge],
        ) -> Result<Vec<Edge>, Failure> {
            assert_eq!(selector, "choose.branch");
            let chosen = match value {
                Value::I64(n) if *n > 0 => Some("b"),
                Value::I64(n) if *n < 0 => Some("c"),
                Value::I64(_) => None,
                _ => return Err(fail("invalid selector input")),
            };
            Ok(candidates
                .iter()
                .filter(|edge| Some(edge.to.node.as_str()) == chosen)
                .cloned()
                .collect())
        }
    }

    #[test]
    fn output_policy_selects_different_edges_for_each_value() {
        let graph = selective_fanout();
        for (input_value, node_name, expected) in [(5, "b", 10), (-5, "c", -15)] {
            let report = graph
                .execute("run", Value::I64(input_value), &mut SelectHost, None)
                .unwrap();
            assert_eq!(report.activations, 2);
            assert_eq!(
                report.outputs[&OutputRef {
                    node: node_name.into(),
                    port: "value".into()
                }],
                vec![Value::I64(expected)],
            );
            assert_eq!(report.outputs.len(), 2); // source + exactly one branch
        }
        let silent = graph
            .execute("run", Value::I64(0), &mut SelectHost, None)
            .unwrap();
        assert_eq!(silent.activations, 1);
        assert_eq!(silent.outputs.len(), 1);
    }

    #[test]
    fn selective_output_requires_a_policy_host_and_never_falls_back_to_broadcast() {
        let graph = selective_fanout();
        assert!(graph
            .execute("run", Value::I64(5), &mut Host, None)
            .is_err());
    }

    #[test]
    fn selected_destinations_must_be_existing_unique_edges() {
        let graph = selective_fanout();
        let source = OutputRef {
            node: "a".into(),
            port: "value".into(),
        };
        for invalid in [
            vec![route_edge("a", "not-connected", "value")],
            vec![route_edge("a", "b", "value"), route_edge("a", "b", "value")],
            vec![route_edge("wrong-source", "b", "value")],
        ] {
            let mut session = graph.start("run", Value::I64(5)).unwrap();
            let activation = session.next_ready(None).unwrap().unwrap();
            let outcome = session.complete_with(
                activation.id,
                [("value".into(), Value::I64(5))].into(),
                |_from, _selector, _value, _candidates| Ok(invalid.clone()),
            );
            assert!(outcome.is_err());
            assert!(session.settled());
            assert!(session.finish().is_err());
        }
        assert_eq!(
            graph.output_policy(&source),
            OutputPolicy::Select {
                selector: "choose.branch".into()
            },
        );
    }

    #[test]
    fn default_output_policy_remains_broadcast() {
        let mut patch = Patch::new("all");
        patch.push(Edit::AddNode(node("a", "source")));
        patch.push(Edit::AddNode(node("b", "double")));
        patch.push(Edit::AddNode(node("c", "triple")));
        patch.push(edge("a", "b", "value"));
        patch.push(edge("a", "c", "value"));
        patch.push(Edit::AddTrigger(Trigger {
            name: "run".into(),
            to: input("a", "value"),
        }));
        let graph = CompiledGraph::compile(&[patch]).unwrap();
        let report = graph
            .execute("run", Value::I64(5), &mut Host, None)
            .unwrap();
        assert_eq!(report.activations, 3);
        assert_eq!(report.outputs.len(), 3);
    }

    fn five_providers() -> CompiledGraph {
        let mut patch = Patch::new("five.providers");
        patch.push(Edit::AddNode(node("router", "source")));
        patch.push(Edit::AddTrigger(Trigger {
            name: "choose".into(),
            to: input("router", "value"),
        }));
        for index in 0..5 {
            let name = format!("provider.{index}");
            patch.push(Edit::AddNode(node(&name, "double")));
            patch.push(edge("router", &name, "value"));
        }
        patch.output_policy(
            OutputRef {
                node: "router".into(),
                port: "value".into(),
            },
            OutputPolicy::Select {
                selector: "choose.provider".into(),
            },
        );
        CompiledGraph::compile(&[patch]).unwrap()
    }

    struct ProviderHost;
    impl ActionHost for ProviderHost {
        fn invoke(
            &mut self,
            action: &str,
            args: BTreeMap<String, Value>,
        ) -> Result<BTreeMap<String, Value>, Failure> {
            Host.invoke(action, args)
        }

        fn route(
            &mut self,
            selector: &str,
            from: &OutputRef,
            value: &Value,
            candidates: &[Edge],
        ) -> Result<Vec<Edge>, Failure> {
            assert_eq!(selector, "choose.provider");
            assert_eq!(from.node, "router");
            assert_eq!(candidates.len(), 5);
            let Value::I64(number) = value else {
                return Err(fail("provider selector requires an integer"));
            };
            let path = format!("provider.{}", number.rem_euclid(5));
            Ok(candidates
                .iter()
                .filter(|edge| edge.to.node == path)
                .cloned()
                .collect())
        }
    }

    #[test]
    fn five_identically_typed_providers_select_exactly_one() {
        let graph = five_providers();
        assert_eq!(graph.nodes().len(), 6);
        for selected in 0..5_i64 {
            let result = graph
                .execute("choose", Value::I64(selected), &mut ProviderHost, None)
                .unwrap();
            assert_eq!(result.activations, 2); // router + one provider
            assert_eq!(result.outputs.len(), 2);
            let provider = OutputRef {
                node: format!("provider.{selected}"),
                port: "value".into(),
            };
            assert_eq!(result.outputs[&provider], vec![Value::I64(selected * 2)]);
        }
    }

    #[test]
    fn same_policy_function_may_choose_zero_one_or_all_five_edges() {
        let graph = five_providers();
        for count in [0, 1, 3, 5] {
            let mut session = graph.start("choose", Value::I64(7)).unwrap();
            let activation = session.next_ready(None).unwrap().unwrap();
            assert_eq!(activation.node, "router");
            session
                .complete_with(
                    activation.id,
                    [("value".into(), Value::I64(7))].into(),
                    |_from, _selector, _value, candidates| {
                        assert_eq!(candidates.len(), 5);
                        Ok(candidates.iter().take(count).cloned().collect())
                    },
                )
                .unwrap();
            let mut ran = 1;
            while let Some(provider) = session.next_ready(None).unwrap() {
                ran += 1;
                session
                    .complete(provider.id, [("value".into(), Value::I64(14))].into())
                    .unwrap();
            }
            assert_eq!(ran, count + 1);
            assert_eq!(session.finish().unwrap().activations, count + 1);
        }
    }

    #[test]
    fn selection_on_an_unconnected_port_can_return_nothing() {
        let mut patch = Patch::new("empty.selector");
        patch.push(Edit::AddNode(node("router", "source")));
        patch.push(Edit::AddTrigger(Trigger {
            name: "choose".into(),
            to: input("router", "value"),
        }));
        patch.output_policy(
            OutputRef {
                node: "router".into(),
                port: "value".into(),
            },
            OutputPolicy::Select {
                selector: "choose.provider".into(),
            },
        );
        let graph = CompiledGraph::compile(&[patch]).unwrap();
        let mut session = graph.start("choose", Value::I64(2)).unwrap();
        let activation = session.next_ready(None).unwrap().unwrap();
        session
            .complete_with(
                activation.id,
                [("value".into(), Value::I64(2))].into(),
                |_from, _selector, _value, edges| {
                    assert!(edges.is_empty());
                    Ok(Vec::new())
                },
            )
            .unwrap();
        assert!(session.settled());
        assert_eq!(session.finish().unwrap().activations, 1);
    }
}
