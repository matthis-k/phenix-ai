use super::*;
trait Harness {
    fn submit(&mut self, v: &[Message], auth: bool) -> Result<(), &'static str>;
    fn step(&mut self);
    fn state(&self) -> SharedPlugin;
    fn output(&self) -> Vec<ResultValue>;
    fn events(&self) -> Vec<Event>;
    fn seal(&mut self) {}
    fn drained(&self) -> Option<bool> {
        None
    }
}
impl Harness for timely_engine::Engine<'_> {
    fn submit(&mut self, v: &[Message], a: bool) -> Result<(), &'static str> {
        self.submit(v, a)
    }
    fn step(&mut self) {
        self.step()
    }
    fn state(&self) -> SharedPlugin {
        self.state.clone()
    }
    fn output(&self) -> Vec<ResultValue> {
        self.output.borrow().clone()
    }
    fn events(&self) -> Vec<Event> {
        self.trace.borrow().clone()
    }
    fn seal(&mut self) {
        self.seal()
    }
    fn drained(&self) -> Option<bool> {
        Some(self.drained())
    }
}
impl Harness for dfir_engine::Engine {
    fn submit(&mut self, v: &[Message], a: bool) -> Result<(), &'static str> {
        self.submit(v, a)
    }
    fn step(&mut self) {
        self.step()
    }
    fn state(&self) -> SharedPlugin {
        self.state.clone()
    }
    fn output(&self) -> Vec<ResultValue> {
        self.output.borrow().clone()
    }
    fn events(&self) -> Vec<Event> {
        self.trace.borrow().clone()
    }
}
fn both(mut test: impl FnMut(&mut dyn Harness)) {
    timely_engine::run(|e| test(e));
    test(&mut dfir_engine::Engine::new());
}
#[tokio::test(flavor = "current_thread")]
async fn external_trigger_branch_and_inactive_path() {
    both(|e| {
        e.submit(&[message(1, false, false)], true).unwrap();
        e.seal();
        e.step();
        assert_eq!(e.output()[0].text, "model:HELLO");
        let nodes: Vec<_> = e.events().iter().map(|v| v.node).collect();
        assert_eq!(
            nodes,
            [
                "Prepare",
                "Switch",
                "Model",
                "PhysicalDrained",
                "Aggregate",
                "Complete"
            ]
        );
        assert!(!nodes.contains(&"Tool"));
        if let Some(done) = e.drained() {
            assert!(done)
        }
    });
}
#[tokio::test(flavor = "current_thread")]
async fn pending_no_downstream_then_release() {
    both(|e| {
        let m = message(1, true, true);
        e.submit(&[m.clone()], true).unwrap();
        e.seal();
        e.step();
        assert!(e.output().is_empty());
        assert_eq!(e.state().borrow().physical_pending(), 1);
        if let Some(done) = e.drained() {
            assert!(!done)
        }
        e.state().borrow_mut().release(m.id);
        e.step();
        assert_eq!(e.output()[0].id, m.id);
        if let Some(done) = e.drained() {
            assert!(done)
        }
    });
}
#[tokio::test(flavor = "current_thread")]
async fn cancellation_fences_late_result_and_waits_for_physical_drain() {
    both(|e| {
        let m = message(1, true, true);
        e.submit(&[m.clone()], true).unwrap();
        e.seal();
        e.step();
        e.state().borrow_mut().cancel(m.id.root);
        e.step();
        assert!(e.output().is_empty());
        assert_eq!(e.state().borrow().physical_pending(), 1);
        if let Some(done) = e.drained() {
            assert!(!done)
        }
        e.state().borrow_mut().release(m.id);
        e.step();
        assert!(e.output().is_empty());
        assert!(e.state().borrow().effects.is_empty());
        assert_eq!(e.state().borrow().physical_pending(), 0);
        if let Some(done) = e.drained() {
            assert!(done)
        }
    });
}
#[tokio::test(flavor = "current_thread")]
async fn cancellation_before_service_poll() {
    both(|e| {
        let m = message(1, false, false);
        e.state().borrow_mut().cancel(1);
        e.submit(&[m], true).unwrap();
        e.seal();
        e.step();
        assert!(e.output().is_empty());
    });
}
#[tokio::test(flavor = "current_thread")]
async fn concurrent_roots_reverse_completion_no_mixing() {
    both(|e| {
        let a = message(1, false, true);
        let b = message(2, true, true);
        e.submit(&[a.clone(), b.clone()], true).unwrap();
        e.seal();
        e.step();
        e.state().borrow_mut().release(b.id);
        e.step();
        assert_eq!(e.output()[0].id, b.id);
        e.state().borrow_mut().release(a.id);
        e.step();
        assert_eq!(e.output()[1].id, a.id);
    });
}
#[tokio::test(flavor = "current_thread")]
async fn repeated_occurrences_and_explicit_retry_policy() {
    both(|e| {
        let a = message(1, false, false);
        let mut b = a.clone();
        b.id.occurrence = 1;
        e.submit(&[a.clone(), b], true).unwrap();
        assert_eq!(e.submit(&[a], true), Err("duplicate occurrence"));
        e.seal();
        e.step();
        assert_eq!(e.output().len(), 2);
        assert_eq!(e.state().borrow().effects.len(), 2);
    });
}
#[tokio::test(flavor = "current_thread")]
async fn unauthorized_and_duplicate_batch_are_atomic() {
    both(|e| {
        let a = message(1, false, false);
        assert_eq!(e.submit(&[a.clone()], false), Err("denied"));
        assert_eq!(
            e.submit(&[a.clone(), a.clone()], true),
            Err("duplicate occurrence")
        );
        e.step();
        assert!(e.events().is_empty());
        e.submit(&[a], true).unwrap();
        e.seal();
        e.step();
        assert_eq!(e.output().len(), 1);
    });
}
#[tokio::test(flavor = "current_thread")]
async fn state_read_write_does_not_activate_and_failed_commit_does_not_emit() {
    both(|e| {
        let state = e.state();
        assert!(state.borrow_mut().commit(9, "bad").is_err());
        e.step();
        assert!(e.events().is_empty());
        state.borrow_mut().commit(0, "!").unwrap();
        e.step();
        assert!(e.events().is_empty());
        // Domain owner explicitly emits only after a successful commit, outside scheduler.
        e.submit(&[message(1, false, false)], true).unwrap();
        e.seal();
        e.step();
        assert_eq!(e.output()[0].text, "model:HELLO!");
    });
}
#[tokio::test(flavor = "current_thread")]
async fn lost_worker_settles_as_failure_not_success_payload() {
    both(|e| {
        let m = message(1, false, true);
        e.submit(&[m.clone()], true).unwrap();
        e.seal();
        e.step();
        e.state().borrow_mut().disappear(m.id);
        e.step();
        assert!(e.output().is_empty());
        assert!(e.events().iter().any(|v| v.node == "WorkerLost"));
        if let Some(done) = e.drained() {
            assert!(done)
        } // Empty frontier is NOT successful root outcome.
    });
}
#[test]
fn named_typed_port_node_injection() {
    let trace = Trace::default();
    let mut ingress = Ingress::default();
    let m = message(1, false, false);
    assert_eq!(
        ingress.admit("state.read", &[m.clone()], true),
        Err("unknown port")
    );
    let admitted = ingress.admit("Prepare.in", &[m.clone()], true).unwrap();
    let p = prepare(admitted[0].clone(), &trace);
    assert_eq!(p.0.text, "HELLO");
    assert_eq!(p.0.id, m.id);
}
#[test]
fn serialized_delivery_preserves_identity_but_does_not_authorize() {
    let mut m = message(1, false, false);
    m.id.wave = 3;
    m.id.activation = 17;
    m.id.occurrence = 4;
    let wire = serde_json::to_vec(&m).unwrap();
    let decoded: Message = serde_json::from_slice(&wire).unwrap();
    assert_eq!(m, decoded);
    assert_eq!(
        Ingress::default().admit("Prepare.in", &[decoded], false),
        Err("denied")
    );
}
#[tokio::test(flavor = "current_thread")]
async fn dfir_idle_is_not_root_completion() {
    let mut e = dfir_engine::Engine::new();
    let m = message(1, false, true);
    e.submit(&[m.clone()], true).unwrap();
    e.step();
    assert!(e.output.borrow().is_empty());
    assert_eq!(e.state.borrow().physical_pending(), 1);
    e.step(); // run_available returns with physical work outstanding.
    e.state.borrow_mut().release(m.id);
    e.step();
    assert_eq!(e.output.borrow().len(), 1);
}
#[tokio::test(flavor = "current_thread")]
async fn task_tracker_requires_both_close_and_physical_task_exit() {
    let tracker = tokio_util::task::TaskTracker::new();
    let cancel = CancellationToken::new();
    let (tx, rx) = oneshot::channel::<()>();
    let token = cancel.clone();
    tracker.spawn(async move {
        token.cancelled().await;
        rx.await.unwrap();
    });
    tracker.close();
    cancel.cancel();
    tokio::task::yield_now().await;
    assert!(futures::poll!(Box::pin(tracker.wait())).is_pending());
    tx.send(()).unwrap();
    tracker.wait().await;
    assert!(tracker.is_empty());
}
