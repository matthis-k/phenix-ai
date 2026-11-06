//! The one graph engine: symbolic contributions, deterministic compilation and typed
//! delivery. Native ABI dispatch and effect policy belong to the host, not this engine.
//! WIP: pending native tickets, coloring, module flattening and custom policies are
//! not yet ported from the separate prototype.
use phenix_contract::{Code, Failure, Value};
pub use phenix_contract::graph::{
    Edge, Edit, InputPort, InputRef, Node, OutputRef, Patch, Readiness, Schema, Trigger,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use sha2::{Digest, Sha256};
fn fail(msg: impl Into<String>) -> Failure { Failure::new(Code::SchemaRejected, msg) }

#[derive(Clone, Debug, Default)]
pub struct CompiledGraph {
    pub identity: String,
    nodes: BTreeMap<String, Node>,
    edges: BTreeSet<Edge>,
    triggers: BTreeMap<String, InputRef>,
    applied: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Target { Node(String), Edge(Edge), Trigger(String) }
fn target(edit: &Edit) -> Target {
    match edit {
        Edit::AddNode(n) | Edit::ReplaceNode(n) => Target::Node(n.path.clone()),
        Edit::RemoveNode(n) => Target::Node(n.clone()),
        Edit::Connect(e) | Edit::Disconnect(e) => Target::Edge(e.clone()),
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
            let id = pending.iter()
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
                        return Err(fail(format!("unordered writes by {owner} and {id} to {key:?}")));
                    }
                }
                graph.apply(edit)?;
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
        for endpoint in graph.triggers.values() {
            graph.input_schema(endpoint)?;
        }
        graph.identity = format!("{:x}", identity.finalize());
        Ok(graph)
    }

    fn apply(&mut self, edit: &Edit) -> Result<(), Failure> {
        match edit {
            Edit::AddNode(n) => {
                if self.nodes.contains_key(&n.path) { return Err(fail("node already exists")); }
                self.nodes.insert(n.path.clone(), n.clone());
            }
            Edit::ReplaceNode(n) => {
                let old = self.nodes.get(&n.path).ok_or_else(|| fail("replacement node absent"))?;
                if old.inputs != n.inputs || old.outputs != n.outputs {
                    return Err(fail("replacement changes node interface"));
                }
                self.nodes.insert(n.path.clone(), n.clone());
            }
            Edit::RemoveNode(name) => {
                if self.nodes.remove(name).is_none() { return Err(fail("node absent")); }
            }
            Edit::Connect(edge) => {
                if !self.edges.insert(edge.clone()) { return Err(fail("duplicate edge")); }
            }
            Edit::Disconnect(edge) => {
                if !self.edges.remove(edge) { return Err(fail("edge absent")); }
            }
            Edit::AddTrigger(t) => {
                if self.triggers.contains_key(&t.name) { return Err(fail("duplicate trigger")); }
                self.triggers.insert(t.name.clone(), t.to.clone());
            }
            Edit::RemoveTrigger(name) => {
                if self.triggers.remove(name).is_none() { return Err(fail("trigger absent")); }
            }
        }
        Ok(())
    }
    pub fn applied(&self) -> &[String] { &self.applied }
    pub fn nodes(&self) -> &BTreeMap<String, Node> { &self.nodes }
    pub fn edges(&self) -> &BTreeSet<Edge> { &self.edges }
    pub fn triggers(&self) -> &BTreeMap<String, InputRef> { &self.triggers }
    fn input_schema(&self, e: &InputRef) -> Result<&Schema, Failure> {
        self.nodes.get(&e.node)
            .and_then(|n| n.inputs.get(&e.port))
            .and_then(|p| p.slots.get(&e.slot))
            .ok_or_else(|| fail(format!("missing input {e:?}")))
    }
    fn output_schema(&self, e: &OutputRef) -> Result<&Schema, Failure> {
        self.nodes.get(&e.node)
            .and_then(|n| n.outputs.get(&e.port))
            .ok_or_else(|| fail(format!("missing output {e:?}")))
    }
    fn deliver(
        &self, queues: &mut BTreeMap<InputRef, VecDeque<Value>>,
        endpoint: &InputRef, value: Value,
    ) -> Result<(), Failure> {
        if !self.input_schema(endpoint)?.accepts(&value)? {
            return Err(fail("input value rejected by schema"));
        }
        queues.entry(endpoint.clone()).or_default().push_back(value);
        Ok(())
    }
    fn ready(node: &Node, queues: &BTreeMap<InputRef, VecDeque<Value>>) -> bool {
        node.inputs.iter().all(|(port, p)| {
            let available = |slot: &String| queues.get(&InputRef {
                node: node.path.clone(), port: port.clone(), slot: slot.clone()
            }).is_some_and(|q| !q.is_empty());
            match p.readiness {
                Readiness::All => p.slots.keys().all(available),
                Readiness::Any => p.slots.keys().any(available),
            }
        })
    }
    fn consume(
        node: &Node, queues: &mut BTreeMap<InputRef, VecDeque<Value>>,
    ) -> BTreeMap<String, Value> {
        let mut args = BTreeMap::new();
        for (port_name, port) in &node.inputs {
            for slot in port.slots.keys() {
                let key = InputRef {
                    node: node.path.clone(), port: port_name.clone(), slot: slot.clone()
                };
                if let Some(value) = queues.get_mut(&key).and_then(VecDeque::pop_front) {
                    args.insert(format!("{port_name}.{slot}"), value);
                    if port.readiness == Readiness::Any { break; }
                }
            }
        }
        args
    }

    /// Run the same incremental session used for native pending dispatch,
    /// draining local activations synchronously through the supplied host.
    pub fn execute(
        &self, trigger: &str, payload: Value, host: &mut impl ActionHost,
        activation_limit: Option<usize>,
    ) -> Result<ExecutionReport, Failure> {
        let mut session = self.start(trigger, payload)?;
        while let Some(activation) = session.next_ready(activation_limit)? {
            let outputs = host.invoke(&activation.action, activation.args)?;
            session.complete(activation.id, outputs)?;
        }
        session.finish()
    }
}
mod session;
pub use session::{Activation, GraphSession};

pub trait ActionHost {
    fn invoke(
        &mut self, action: &str, args: BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>, Failure>;
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
        InputPort { slots: [("value".into(), schema())].into(), readiness: Readiness::All }
    }
    fn node(name: &str, action: &str) -> Node {
        Node {
            path: name.into(), action: action.into(),
            inputs: [("run".into(), single())].into(),
            outputs: [("value".into(), schema())].into(),
        }
    }
    fn input(name: &str, slot: &str) -> InputRef {
        InputRef { node: name.into(), port: "run".into(), slot: slot.into() }
    }
    fn edge(from: &str, to: &str, slot: &str) -> Edit {
        Edit::Connect(Edge {
            from: OutputRef { node: from.into(), port: "value".into() },
            to: input(to, slot),
        })
    }
    struct Host;
    impl ActionHost for Host {
        fn invoke(&mut self, action: &str, args: BTreeMap<String, Value>) -> Result<BTreeMap<String, Value>, Failure> {
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
        base.push(Edit::AddTrigger(Trigger { name: "run".into(), to: input("a", "value") }));
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
        let result = compiled.execute("run", Value::I64(5), &mut Host, Some(10)).unwrap();
        let output = OutputRef { node: "d".into(), port: "value".into() };
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
            name: "run".into(), to: input("source", "value"),
        }));
        let mut b = Patch::new("b").after("a");
        b.push(Edit::AddNode(node("twice", "double")));
        b.push(edge("source", "twice", "value"));
        let ordered = CompiledGraph::compile(&[a.clone(), b.clone()]).unwrap();
        let reordered = CompiledGraph::compile(&[b, a]).unwrap();
        assert_eq!(ordered.identity, reordered.identity);
        assert!(!ordered.identity.is_empty());
    }

}
