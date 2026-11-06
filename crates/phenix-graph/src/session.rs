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
    failed: Option<Failure>,
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
            report: ExecutionReport::default(), cancelled: false, failed: None,
        })
    }
}
impl GraphSession<'_> {
    /// Reserves one ready node's inputs. Multiple reservations may be in flight;
    /// their completions can arrive in any order.
    pub fn next_ready(&mut self, activation_limit: Option<usize>) -> Result<Option<Activation>, Failure> {
        if self.cancelled { return Ok(None); }
        if let Some(failure) = &self.failed { return Err(failure.clone()); }
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
        if self.cancelled || self.failed.is_some() {
            self.report.activations += 1;
            return Ok(()); // late native results are drained but not propagated
        }
        let validation = (|| {
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
            Ok::<_, Failure>(checked)
        })();
        let checked = match validation {
            Ok(checked) => checked,
            Err(error) => {
                self.failed = Some(error.clone());
                self.queues.clear();
                return Err(error);
            }
        };
        for (source, value, destinations) in checked {
            self.report.outputs.entry(source).or_default().push(value.clone());
            for destination in destinations {
                self.graph.deliver(&mut self.queues, &destination, value.clone())?;
            }
        }
        self.report.activations += 1;
        Ok(())
    }
    /// Record a failed host activation without converting it into a graph output.
    /// Other in-flight activations remain reserved until the host drains them.
    pub fn fail(&mut self, id: u64, failure: Failure) -> Result<(), Failure> {
        self.inflight.remove(&id)
            .ok_or_else(|| fail("unknown or already completed activation"))?;
        self.report.activations += 1;
        if self.failed.is_none() {
            self.failed = Some(failure);
            self.queues.clear();
        }
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
            self.cancelled || self.failed.is_some() || !self.graph.nodes.values()
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
        if let Some(failure) = self.failed { return Err(failure); }
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
    #[test]
    fn invalid_output_aborts_wave_and_is_not_silently_finished() {
        let graph = source_graph();
        let mut session = graph.start("run", Value::I64(5)).unwrap();
        let activation = session.next_ready(Some(5)).unwrap().unwrap();
        assert!(session.complete(activation.id,
            [("value".into(), Value::String("invalid".into()))].into()).is_err());
        assert!(session.settled());
        assert!(session.next_ready(None).is_err());
        assert!(session.finish().is_err());
    }

    #[test]
    fn one_wave_reserves_both_diamond_branches_and_joins_out_of_order() {
        let single = |path: &str, action: &str| Node {
            path: path.into(), action: action.into(),
            inputs: [("run".into(), InputPort {
                slots: [("value".into(), schema())].into(),
                readiness: Readiness::All,
            })].into(),
            outputs: [("value".into(), schema())].into(),
        };
        let join = Node {
            path: "d".into(), action: "join".into(),
            inputs: [("run".into(), InputPort {
                slots: [("left".into(), schema()), ("right".into(), schema())].into(),
                readiness: Readiness::All,
            })].into(),
            outputs: [("value".into(), schema())].into(),
        };
        let edge = |from: &str, to: &str, slot: &str| Edit::Connect(Edge {
            from: OutputRef { node: from.into(), port: "value".into() },
            to: InputRef { node: to.into(), port: "run".into(), slot: slot.into() },
        });
        let mut base = Patch::new("base");
        base.push(Edit::AddNode(single("a", "source")));
        base.push(Edit::AddNode(join));
        base.push(Edit::AddTrigger(Trigger { name: "run".into(),
            to: InputRef { node: "a".into(), port: "run".into(), slot: "value".into() } }));
        let mut b = Patch::new("branch.b").after("base");
        b.push(Edit::AddNode(single("b", "double")));
        b.push(edge("a", "b", "value"));
        b.push(edge("b", "d", "left"));
        let mut c = Patch::new("branch.c").after("base");
        c.push(Edit::AddNode(single("c", "triple")));
        c.push(edge("a", "c", "value"));
        c.push(edge("c", "d", "right"));
        let graph = CompiledGraph::compile(&[c, base, b]).unwrap();
        let mut session = graph.start("run", Value::I64(5)).unwrap();
        let source = session.next_ready(Some(10)).unwrap().unwrap();
        session.complete(source.id, [("value".into(), Value::I64(5))].into()).unwrap();
        let branch1 = session.next_ready(Some(10)).unwrap().unwrap();
        let branch2 = session.next_ready(Some(10)).unwrap().unwrap();
        assert_eq!((&*branch1.node, &*branch2.node), ("b", "c"));
        assert_eq!(session.in_flight(), 2);
        assert!(session.next_ready(Some(10)).unwrap().is_none());
        // Results may arrive in reverse order without exposing a partial join.
        session.complete(branch2.id, [("value".into(), Value::I64(15))].into()).unwrap();
        assert!(session.next_ready(Some(10)).unwrap().is_none());
        session.complete(branch1.id, [("value".into(), Value::I64(10))].into()).unwrap();
        let joined = session.next_ready(Some(10)).unwrap().unwrap();
        assert_eq!(joined.node, "d");
        assert_eq!(joined.args["run.left"], Value::I64(10));
        assert_eq!(joined.args["run.right"], Value::I64(15));
        session.complete(joined.id, [("value".into(), Value::I64(25))].into()).unwrap();
        assert!(session.settled());
        assert_eq!(session.finish().unwrap().activations, 4);
    }

    #[test]
    fn host_failure_blocks_new_work_until_inflight_drains() {
        let graph = source_graph();
        let mut session = graph.start("run", Value::I64(5)).unwrap();
        let activation = session.next_ready(None).unwrap().unwrap();
        session.fail(activation.id, fail("native invocation failure")).unwrap();
        assert!(session.settled());
        assert!(session.next_ready(None).is_err());
        assert!(session.finish().is_err());
    }

}
