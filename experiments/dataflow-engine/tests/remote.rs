use phenix_dataflow_experiment::*;
#[tokio::test(flavor = "current_thread")]
async fn dfir_process_delivery_and_worker_exit() {
    let mut e = dfir_engine::Engine::new();
    e.state.borrow_mut().remote = Some(env!("CARGO_BIN_EXE_remote-worker").into());
    e.submit(&[message(1, true, false)], true).unwrap();
    for _ in 0..400 {
        e.step();
        if !e.output.borrow().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert_eq!(e.output.borrow()[0].id, message(1, true, false).id);
    assert_eq!(e.output.borrow()[0].text, "tool:HELLO");
    let mut m = message(2, false, false);
    m.text = "EXIT".into();
    e.submit(&[m], true).unwrap();
    for _ in 0..400 {
        e.step();
        if e.trace.borrow().iter().any(|v| v.node == "TransportLost") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert_eq!(e.output.borrow().len(), 1);
    assert!(e.trace.borrow().iter().any(|v| v.node == "TransportLost"));
}
#[test]
fn timely_process_delivery_and_worker_exit() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    timely_engine::run(|e| {
        rt.block_on(async {
            e.state.borrow_mut().remote = Some(env!("CARGO_BIN_EXE_remote-worker").into());
            e.submit(&[message(1, true, false)], true).unwrap();
            let mut m = message(2, false, false);
            m.text = "EXIT".into();
            e.submit(&[m], true).unwrap();
            e.seal();
            e.step();
            assert!(!e.drained(), "transport retains real capabilities");
            for _ in 0..400 {
                e.step();
                if e.drained() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
            assert!(e.drained());
            assert_eq!(e.output.borrow().len(), 1);
            assert_eq!(e.output.borrow()[0].text, "tool:HELLO");
            assert!(e.trace.borrow().iter().any(|v| v.node == "TransportLost"));
        })
    });
}
#[test]
fn timely_lost_connection_deadline_kills_and_reaps_before_frontier_exhaustion() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    timely_engine::run(|e| {
        rt.block_on(async {
            e.state.borrow_mut().remote = Some(env!("CARGO_BIN_EXE_remote-worker").into());
            let mut m = message(9, false, false);
            m.text = "HANG".into();
            e.submit(&[m], true).unwrap();
            e.seal();
            e.step();
            assert!(!e.drained());
            for _ in 0..1500 {
                e.step();
                if e.drained() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
            assert!(e.drained());
            assert!(e.output.borrow().is_empty());
            let t = e.trace.borrow();
            let a = t.iter().position(|v| v.node == "PhysicalDrained").unwrap();
            let b = t.iter().position(|v| v.node == "TransportLost").unwrap();
            assert!(a < b);
        })
    });
}
#[test]
fn timely_native_exchange_crosses_worker_with_serialized_port_delivery() {
    use std::sync::{Arc, Mutex};
    use timely::dataflow::{
        operators::{vec::Map, Exchange, Input, Inspect, Probe},
        InputHandle,
    };
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    timely::execute(timely::Config::process(2), move |worker| {
        let mut input = InputHandle::new();
        let index = worker.index();
        let sink = sink.clone();
        let probe = worker.dataflow::<u64, _, _>(|scope| {
            scope
                .input_from(&mut input)
                .container::<Vec<Vec<u8>>>()
                .exchange(|_| 1)
                .map(move |wire| {
                    assert_eq!(index, 1);
                    let m: Message = serde_json::from_slice(&wire).unwrap();
                    (m.id, response(&m, ""))
                })
                .inspect(move |v| sink.lock().unwrap().push((index, v.clone())))
                .probe()
                .0
        });
        if index == 0 {
            input.send(serde_json::to_vec(&message(8, true, false)).unwrap());
        }
        drop(input);
        while !probe.done() {
            worker.step();
        }
    })
    .unwrap()
    .join()
    .into_iter()
    .for_each(|r| r.unwrap());
    let out = seen.lock().unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, 1);
    assert_eq!((out[0].1).0.root, 8);
}
#[tokio::test(flavor = "current_thread")]
async fn dfir_remote_protocol_faults_are_not_transport_success() {
    let mut e = dfir_engine::Engine::new();
    e.state.borrow_mut().remote = Some(env!("CARGO_BIN_EXE_remote-worker").into());
    for (root, mode, expected) in [
        (50, "WRONGID", "IdentityMismatch"),
        (51, "MALFORMED", "MalformedRemote"),
    ] {
        let mut m = message(root, false, false);
        m.text = mode.into();
        e.submit(&[m], true).unwrap();
        for _ in 0..400 {
            e.step();
            if e.trace
                .borrow()
                .iter()
                .any(|v| v.id.root == root && v.node == expected)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        assert!(e
            .trace
            .borrow()
            .iter()
            .any(|v| v.id.root == root && v.node == expected));
    }
    assert!(e.output.borrow().is_empty());
    assert!(e.state.borrow().effects.is_empty());
}
#[test]
fn timely_remote_protocol_faults_settle_without_committed_effects() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    timely_engine::run(|e| {
        rt.block_on(async {
            e.state.borrow_mut().remote = Some(env!("CARGO_BIN_EXE_remote-worker").into());
            for (root, mode) in [(50, "WRONGID"), (51, "MALFORMED")] {
                let mut m = message(root, false, false);
                m.text = mode.into();
                e.submit(&[m], true).unwrap();
            }
            e.seal();
            for _ in 0..400 {
                e.step();
                if e.drained() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
            assert!(e.drained());
            assert!(e.output.borrow().is_empty());
            assert!(e.state.borrow().effects.is_empty());
            let trace = e.trace.borrow();
            assert!(trace.iter().any(|v| v.node == "IdentityMismatch"));
            assert!(trace.iter().any(|v| v.node == "MalformedRemote"));
        })
    });
}
