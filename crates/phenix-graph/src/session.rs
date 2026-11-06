//! Incremental execution of one wave. The same scheduler powers synchronous
//! local execution and future native pending-call integration. No plugin ABI or
//! host authority is stored here.
use super::*;
/// A reserved activation. Its inputs have already been removed from slots.
/// The host may complete activations out of order.
#[derive(Clone, Debug)]
pub struct Activation {
    pub id: u64,
    pub node: String,
    pub action: String,
    pub args: BTreeMap<String, Value>,
}
pub struct GraphSession<'g> {
    graph: &'g CompiledGraph,
    queues: BTreeMap<InputRef, VecDeque<Value>>,
    inflight: BTreeMap<u64, String>,
    next_id: u64,
    report: ExecutionReport,
    cancelled: bool,
}
impl CompiledGraph {
    /// Admit a trigger occurrence as a separate wave.
    pub fn start(&self, trigger: &str, payload: Value) -> Result<GraphSession<'_>, Failure> {
        let target = self.triggers.get(trigger)
            .ok_or_else(|| fail(format!("unknown trigger {trigger}")))?;
        let mut queues = BTreeMap::new();
        self.deliver(&mut queues, target, payload)?;
        Ok(GraphSession {
            graph: self, queues, inflight: BTreeMap::new(), next_id: 0,
            report: ExecutionReport::default(), cancelled: false,
        })
    }
}
impl GraphSession<'_> {
    /// Reserves one ready node's inputs. Multiple reservations may be in flight;
    /// their completions can arrive in any order.
    pub fn next_ready(&mut self, activation_limit: Option<usize>) -> Result<Option<Activation>, Failure> {
        if self.cancelled { return Ok(None); }
        let Some(node) = self.graph.nodes.values()
            .find(|node| CompiledGraph::ready(node, &self.queues)).cloned()
        else { return Ok(None) };
        if activation_limit.is_some_and(|max| {
            self.report.activations.saturating_add(self.inflight.len()) >= max
        }) {
            return Err(fail("activation limit exceeded"));
        }
        self.next_id = self.next_id.checked_add(1)
            .ok_or_else(|| fail("activation identity exhausted"))?;
        let id = self.next_id;
        let args = CompiledGraph::consume(&node, &mut self.queues);
        self.inflight.insert(id, node.path.clone());
        Ok(Some(Activation { id, node: node.path, action: node.action, args }))
    }
    /// Complete a previously reserved activation. Validate every output before
    /// delivering any of them, so invalid output schemas cannot partly fan out.
    pub fn complete(
        &mut self, id: u64, outputs: BTreeMap<String, Value>,
    ) -> Result<(), Failure> {
        let node = self.inflight.remove(&id)
            .ok_or_else(|| fail("unknown or already completed activation"))?;
        if self.cancelled {
            self.report.activations += 1;
            return Ok(()); // late native results are drained but not propagated
        }
        let mut checked = Vec::new();
        for (port, value) in outputs {
            let source = OutputRef { node: node.clone(), port };
            if !self.graph.output_schema(&source)?.accepts(&value)? {
                return Err(fail("output value rejected by schema"));
            }
            let destinations: Vec<_> = self.graph.edges.iter()
                .filter(|edge| edge.from == source)
                .map(|edge| edge.to.clone()).collect();
            for dest in &destinations {
                if !self.graph.input_schema(dest)?.accepts(&value)? {
                    return Err(fail("input value rejected by schema"));
                }
            }
            checked.push((source, value, destinations));
        }
        for (source, value, destinations) in checked {
            self.report.outputs.entry(source).or_default().push(value.clone());
            for destination in destinations {
                self.graph.deliver(&mut self.queues, &destination, value.clone())?;
            }
        }
        self.report.activations += 1;
        Ok(())
    }
    /// Stop issuing work immediately. Native in-flight activations retain their
    /// identity until the host finishes physical cancellation and calls complete.
    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.queues.clear();
    }
    pub fn cancelled(&self) -> bool { self.cancelled }
    pub fn in_flight(&self) -> usize { self.inflight.len() }
    /// No runnable work and no outstanding native activations.
    /// Pending incomplete input groups can remain as unconsumed values.
    pub fn settled(&self) -> bool {
        self.inflight.is_empty() && (
            self.cancelled || !self.graph.nodes.values()
                .any(|node| CompiledGraph::ready(node, &self.queues))
        )
    }
    pub fn finish(mut self) -> Result<ExecutionReport, Failure> {
        if !self.inflight.is_empty() {
            return Err(fail("wave has in-flight activations; drain before finishing"));
        }
        if !self.cancelled && !self.settled() {
            return Err(fail("wave has runnable nodes"));
        }
        self.report.unconsumed = self.queues.values().map(VecDeque::len).sum();
        Ok(self.report)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn schema() -> Schema {
        Schema::of(&phenix_contract::ValueSchema::I64).unwrap()
    }
    fn source_graph() -> CompiledGraph {
        let node = Node {
            path: "echo".into(), action: "echo".into(),
            inputs: [("run".into(), InputPort {
                slots: [("value".into(), schema())].into(),
                readiness: Readiness::All,
            })].into(),
            outputs: [("value".into(), schema())].into(),
        };
        let mut patch = Patch::new("base");
        patch.push(Edit::AddNode(node));
        patch.push(Edit::AddTrigger(Trigger {
            name: "run".into(),
            to: InputRef { node: "echo".into(), port: "run".into(), slot: "value".into() },
        }));
        CompiledGraph::compile(&[patch]).unwrap()
    }
    #[test]
    fn reserves_and_completes_without_another_scheduler() {
        let graph = source_graph();
        let mut session = graph.start("run", Value::I64(42)).unwrap();
        let activation = session.next_ready(Some(5)).unwrap().unwrap();
        assert_eq!(activation.args["run.value"], Value::I64(42));
        assert_eq!(session.in_flight(), 1);
        assert!(!session.settled());
        session.complete(activation.id, [("value".into(), Value::I64(42))].into()).unwrap();
        assert!(session.settled());
        assert_eq!(session.finish().unwrap().activations, 1);
    }
    #[test]
    fn cancellation_drains_late_work_without_emitting() {
        let graph = source_graph();
        let mut session = graph.start("run", Value::I64(42)).unwrap();
        let activation = session.next_ready(Some(5)).unwrap().unwrap();
        session.cancel();
        assert!(session.cancelled());
        assert!(session.next_ready(Some(5)).unwrap().is_none());
        assert_eq!(session.in_flight(), 1);
        session.complete(activation.id, [("value".into(), Value::I64(42))].into()).unwrap();
        assert!(session.settled());
        assert!(session.finish().unwrap().outputs.is_empty());
    }
    #[test]
    fn duplicate_completion_is_rejected() {
        let graph = source_graph();
        let mut session = graph.start("run", Value::I64(42)).unwrap();
        let activation = session.next_ready(Some(5)).unwrap().unwrap();
        session.complete(activation.id, [("value".into(), Value::I64(42))].into()).unwrap();
        assert!(session.complete(activation.id, BTreeMap::new()).is_err());
        assert!(session.settled());
        assert_eq!(session.finish().unwrap().activations, 1);
    }
}
