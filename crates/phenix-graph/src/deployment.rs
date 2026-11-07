//! Deployment binds external execution and synchronous edge transfer choices
//! to *physical compiled endpoints*, without changing the logical graph.
use super::*;

pub struct Deployment {
    graph: Arc<CompiledGraph>,
    store: Store,
    default_executor: Option<Arc<dyn Executor>>,
    node_executors: Vec<(u16, Arc<dyn Executor>)>,
    adapters: Vec<(OutputAddr, InputAddr, Arc<dyn EdgeAdapter>)>,
    retention: Option<ReportRetention>,
}
impl Deployment {
    pub fn new(graph: Arc<CompiledGraph>) -> Self {
        Self { graph, store: Store::new(), default_executor: None,
            node_executors: vec![], adapters: vec![],
            retention: Some(ReportRetention::TerminalOutputs) }
    }
    pub(crate) fn compiled_graph(&self) -> Arc<CompiledGraph> { Arc::clone(&self.graph) }
    pub fn store(mut self, store: Store) -> Self { self.store = store; self }
    /// Local deployments retain only terminal outputs by default. Override
    /// this for full intermediate-payload inspection or metadata-only runs.
    pub fn report_retention(mut self, retention: ReportRetention) -> Self {
        self.retention = Some(retention); self
    }
    pub fn executor(mut self, executor: Arc<dyn Executor>) -> Self {
        self.default_executor = Some(executor);
        self
    }
    /// Configure one physical node, by a compiler-resolved hierarchical path.
    pub fn node_executor(&mut self, path: &str, executor: Arc<dyn Executor>)
        -> Result<&mut Self, String> {
        let Some(FindResult::Node(node)) = self.graph.find(path) else {
            return Err(format!("unknown physical node path {path:?}"));
        };
        if self.node_executors.iter().any(|(existing, _)| *existing == node.id()) {
            return Err(format!("executor already configured for node {path:?}"));
        }
        self.node_executors.push((node.id(), executor));
        Ok(self)
    }
    /// Resolve a physical edge without changing graph topology.
    fn resolve_edge(&self, src_path: &str, src_output: &str,
        dst_path: &str, dst_port: &str, dst_slot: &str)
        -> Result<(OutputAddr, InputAddr), String> {
        let Some(FindResult::Node(src)) = self.graph.find(src_path) else {
            return Err(format!("unknown source node {src_path:?}"));
        };
        let from = src.output(src_output).ok_or_else(|| format!("unknown output {src_path}.{src_output}"))?;
        let Some(FindResult::Node(dst)) = self.graph.find(dst_path) else {
            return Err(format!("unknown target node {dst_path:?}"));
        };
        let to = dst.input(dst_port, dst_slot)
            .ok_or_else(|| format!("unknown input {dst_path}.{dst_port}.{dst_slot}"))?;
        if !self.graph.by_output.get(&from).is_some_and(|edges| edges.iter().any(|e| e.to == to)) {
            return Err("the specified source and target have no logical connection".into());
        }
        Ok((from, to))
    }
    /// Configure a pure, synchronous, side-effect-free edge preparation stage.
    /// An adapter does not send data to an external process or network.
    pub fn edge_adapter(&mut self, src_path: &str, src_output: &str,
        dst_path: &str, dst_port: &str, dst_slot: &str, adapter: Arc<dyn EdgeAdapter>)
        -> Result<&mut Self, String> {
        let (from, to) = self.resolve_edge(src_path, src_output, dst_path, dst_port, dst_slot)?;
        if self.adapters.iter().any(|(a,b,_)| (*a,*b)==(from,to)) {
            return Err("edge adapter already configured".into());
        }
        self.adapters.push((from, to, adapter));
        Ok(self)
    }
    pub fn build(self) -> Result<Engine, String> {
        let mut engine = Engine::new(self.graph, self.store);
        if let Some(executor) = self.default_executor { engine = engine.with_executor(executor); }
        if let Some(retention) = self.retention { engine = engine.with_report_retention(retention); }
        for (node, backend) in self.node_executors {
            engine = engine.with_node_executor(node, backend)?;
        }
        for (from, to, adapter) in self.adapters {
            engine = engine.with_edge_adapter(from, to, adapter)?;
        }
        Ok(engine)
    }
}
