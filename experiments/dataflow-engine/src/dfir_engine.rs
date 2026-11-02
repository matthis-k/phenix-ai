use crate::*;
/// DFIR owns routing, polling and async wakeups. No Phenix completion predicate is synthesized.
pub struct Engine {
    graph: dfir_rs::scheduled::context::DfirErased,
    tx: dfir_rs::tokio::sync::mpsc::UnboundedSender<Message>,
    pub output: Rc<RefCell<Vec<ResultValue>>>,
    pub trace: Trace,
    pub state: SharedPlugin,
    ingress: Ingress,
}
impl Engine {
    pub fn new() -> Self {
        let (tx, rx) = dfir_rs::util::unbounded_channel::<Message>();
        let output = Rc::new(RefCell::new(Vec::new()));
        let sink = output.clone();
        let trace = Trace::default();
        let (p, s, a, c) = (trace.clone(), trace.clone(), trace.clone(), trace.clone());
        let (mt, tt) = (trace.clone(), trace.clone());
        let state = SharedPlugin::default();
        let (ms, ts) = (state.clone(), state.clone());
        let graph = dfir_rs::dfir_syntax! {
            input = source_stream(rx) -> map(|m| prepare(m, &p)) -> map(|v| switch(v, &s)) -> tee();
            input -> filter(|v: &Prepared| !v.0.tool) -> map(|v| service(v, ms.clone(), mt.clone())) -> resolve_futures() -> filter_map(|v| v) -> merged;
            input -> filter(|v: &Prepared| v.0.tool) -> map(|v| service(v, ts.clone(), tt.clone())) -> resolve_futures() -> filter_map(|v| v) -> merged;
            merged = union() -> map(|v| aggregate(v, &a)) -> map(|v| complete(v, &c)) -> for_each(|v| sink.borrow_mut().push(v));
        };
        Self {
            graph: graph.into_erased(),
            tx,
            output,
            trace,
            state,
            ingress: Ingress::default(),
        }
    }
    pub fn submit(&mut self, batch: &[Message], authorized: bool) -> Result<(), &'static str> {
        for m in self.ingress.admit("Prepare.in", batch, authorized)? {
            self.tx.send(m).unwrap();
        }
        Ok(())
    }
    pub fn step(&mut self) {
        self.graph.run_available_sync();
    }
}
impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}
