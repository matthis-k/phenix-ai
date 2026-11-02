use crate::*;
use futures::{stream::FuturesUnordered, Stream};
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Wake, Waker},
};
use timely::dataflow::operators::vec::{Filter, Map};
use timely::{
    dataflow::channels::pact::Pipeline,
    dataflow::{
        operators::{generic::Operator, Concat, Input, Inspect, Probe},
        InputHandle, ProbeHandle,
    },
};
struct WakeWorker(timely::scheduling::SyncActivator);
impl Wake for WakeWorker {
    fn wake(self: Arc<Self>) {
        self.0.activate().unwrap();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.activate().unwrap();
    }
}

/// Worker lifetime is scoped by Timely; tests run all commands inside this callback.
pub fn run(f: impl FnOnce(&mut Engine<'_>)) {
    let allocator = timely::communication::Allocator::Thread(Default::default());
    let mut worker = timely::worker::Worker::new(Default::default(), allocator, None);
    {
        let mut input = InputHandle::new();
        let probe = ProbeHandle::new();
        let output = Rc::new(RefCell::new(Vec::new()));
        let sink = output.clone();
        let trace = Trace::default();
        let state = SharedPlugin::default();
        worker.dataflow::<u64,_,_>(|scope| {
            let (p, s) = (trace.clone(), trace.clone());
            let stream = scope.input_from(&mut input).container::<Vec<Message>>()
                .map(move |m| prepare(m, &p)).map(move |v| switch(v, &s));
            let mut branches = Vec::new();
            for tool in [false, true] {
                let state = state.clone(); let trace = trace.clone();
                let branch = stream.clone().filter(move |p| p.0.tool == tool);
                let scope = scope.clone();
                let result = branch.unary::<timely::container::CapacityContainerBuilder<Vec<ResultValue>>,_,_,_>(Pipeline, "PluginFuture", move |initial, info| {
                    drop(initial); // Never retain a global capability: only admitted work owns one.
                    let waker = Waker::from(Arc::new(WakeWorker(scope.worker().sync_activator_for(info.address.to_vec()))));
                    let mut futures = FuturesUnordered::new();
                    move |input, output| {
                        input.for_each(|cap, data| {
                            for v in data.drain(..) {
                                let cap = cap.retain(0); let fut = service(v, state.clone(), trace.clone());
                                futures.push(async move { (cap, fut.await) });
                            }
                        });
                        let mut cx = Context::from_waker(&waker);
                        while let std::task::Poll::Ready(Some((cap, result))) = Pin::new(&mut futures).poll_next(&mut cx) {
                            if let Some(result) = result { output.session(&cap).give(result); }
                            // Release capability only after physical future resolves, including cancellation.
                        }
                    }
                });
                branches.push(result);
            }
            let (a, c) = (trace.clone(), trace.clone());
            branches.remove(0).concat(branches.remove(0)).map(move |v| aggregate(v, &a))
                .map(move |v| complete(v, &c)).inspect(move |v| sink.borrow_mut().push(v.clone())).probe_with(&probe);
        });
        let mut engine = Engine {
            worker: &mut worker,
            input: Some(input),
            probe,
            output,
            trace,
            state,
            ingress: Ingress::default(),
        };
        f(&mut engine);
        // Callers must settle physical work; avoid silently blocking worker teardown.
        assert_eq!(engine.state.borrow().physical_pending(), 0);
    }
}
pub struct Engine<'a> {
    worker: &'a mut timely::worker::Worker,
    input: Option<InputHandle<u64, timely::container::CapacityContainerBuilder<Vec<Message>>>>,
    probe: ProbeHandle<u64>,
    pub output: Rc<RefCell<Vec<ResultValue>>>,
    pub trace: Trace,
    pub state: SharedPlugin,
    ingress: Ingress,
}
impl Engine<'_> {
    pub fn submit(&mut self, batch: &[Message], authorized: bool) -> Result<(), &'static str> {
        for m in self.ingress.admit("Prepare.in", batch, authorized)? {
            self.input.as_mut().unwrap().send(m);
        }
        self.input.as_mut().unwrap().flush();
        Ok(())
    }
    pub fn advance_to(&mut self, time: u64) {
        self.input.as_mut().unwrap().advance_to(time);
    }
    pub fn frontier_less_equal(&self, time: u64) -> bool {
        self.probe.less_equal(&time)
    }
    pub fn seal(&mut self) {
        self.input.take();
    }
    pub fn step(&mut self) {
        for _ in 0..32 {
            self.worker.step();
        }
    }
    pub fn drained(&self) -> bool {
        self.probe.done()
    }
}
