//! Engine-native terminal merge, separate from the eager fixture. Buffering is
//! operator state; no custom ready queue, edge closure events or work counters.
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use timely::dataflow::channels::pact::Pipeline;
use timely::dataflow::{
    operators::{generic::Operator, Concat, Input, Inspect, Probe},
    InputHandle, ProbeHandle,
};
#[test]
fn timely_frontier_exactly_one_failure_precedence_and_inactive_branch() {
    let results = Rc::new(RefCell::new(Vec::new()));
    let sink = results.clone();
    let mut worker = timely::worker::Worker::new(
        Default::default(),
        timely::communication::Allocator::Thread(Default::default()),
        None,
    );
    let (mut left, mut right) = (InputHandle::new(), InputHandle::new());
    let probe = ProbeHandle::new();
    worker.dataflow::<u64,_,_>(|scope|{
        let a=scope.input_from(&mut left).container::<Vec<Result<i64,String>>>();
        let b=scope.input_from(&mut right).container::<Vec<Result<i64,String>>>();
        let mut stash=BTreeMap::new();
        a.concat(b).unary_frontier::<timely::container::CapacityContainerBuilder<Vec<(u64,Result<i64,String>)>>,_,_,_>(Pipeline,"TerminalExactlyOne",|initial,_|{
            drop(initial);
            move |input,output|{
                input.0.for_each(|cap,data|{let e=stash.entry(*cap.time()).or_insert_with(||(cap.retain(0),Vec::new()));e.1.append(data);});
                let done:Vec<_>=stash.keys().filter(|t|!input.1.frontier().less_equal(t)).copied().collect();
                for time in done{let(cap,values)=stash.remove(&time).unwrap();
                    let out=if let Some(err)=values.iter().find_map(|v|v.as_ref().err()){Err(err.clone())}
                        else if values.len()!=1{Err("multiple values".into())}else{values[0].clone()};
                    output.session(&cap).give((time,out));
                }
            }
        }).inspect(move |v|sink.borrow_mut().push(v.clone())).probe_with(&probe);
    });
    left.send(Ok(1));
    left.advance_to(1);
    left.flush();
    for _ in 0..32 {
        worker.step();
    }
    assert!(
        results.borrow().is_empty(),
        "value cannot beat unresolved late failure"
    );
    right.send(Err("late failure".into()));
    right.advance_to(1);
    right.flush();
    for _ in 0..32 {
        worker.step();
    }
    assert_eq!(results.borrow()[0], (0, Err("late failure".into())));
    left.send(Ok(2));
    right.send(Ok(3));
    left.advance_to(2);
    right.advance_to(2);
    left.flush();
    right.flush();
    for _ in 0..32 {
        worker.step();
    }
    assert_eq!(results.borrow()[1], (1, Err("multiple values".into())));
    left.send(Ok(4));
    left.advance_to(3);
    left.flush();
    right.advance_to(3);
    right.flush();
    for _ in 0..32 {
        worker.step();
    }
    assert_eq!(
        results.borrow()[2],
        (2, Ok(4)),
        "inactive producer needs no fabricated delivery"
    );
    drop(left);
    drop(right);
    for _ in 0..32 {
        worker.step();
    }
    assert!(probe.done());
}
#[tokio::test(flavor = "current_thread")]
async fn dfir_union_counterexample_to_terminal_exactly_one() {
    let (tx, rx) = dfir_rs::util::unbounded_channel::<Result<u64, String>>();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let mut graph =
        dfir_rs::dfir_syntax! {source_stream(rx)->for_each(|v|sink.borrow_mut().push(v));};
    tx.send(Ok(1)).unwrap();
    graph.run_available_sync();
    assert_eq!(seen.borrow().len(), 1);
    tx.send(Err("late failure".into())).unwrap();
    graph.run_available_sync();
    assert_eq!(*seen.borrow(), vec![Ok(1), Err("late failure".into())]);
    // Already published success cannot be retracted into Phenix's single terminal failure.
}
#[test]
fn timely_feedback_preserves_root_across_waves() {
    use crate::message;
    use timely::dataflow::operators::{
        vec::{Filter, Map},
        ConnectLoop, Feedback,
    };
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let mut worker = timely::worker::Worker::new(
        Default::default(),
        timely::communication::Allocator::Thread(Default::default()),
        None,
    );
    let mut input = InputHandle::new();
    let probe = ProbeHandle::new();
    worker.dataflow::<u64, _, _>(|scope| {
        let (handle, cycle) = scope.feedback(1);
        let all = scope
            .input_from(&mut input)
            .container::<Vec<crate::Message>>()
            .concat(cycle);
        all.clone()
            .filter(|m| m.id.wave < 2)
            .map(|mut m| {
                m.id.wave += 1;
                m
            })
            .connect_loop(handle);
        all.filter(|m| m.id.wave == 2)
            .inspect(move |m| sink.borrow_mut().push(m.clone()))
            .probe_with(&probe);
    });
    input.send(message(41, false, false));
    drop(input);
    for _ in 0..128 {
        worker.step();
    }
    assert!(probe.done());
    let out = &seen.borrow()[0];
    assert_eq!(out.id.root, 41);
    assert_eq!(out.id.activation, 41);
    assert_eq!(out.id.wave, 2);
}
#[tokio::test(flavor = "current_thread")]
async fn dfir_defer_tick_preserves_root_across_waves() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let mut graph = dfir_rs::dfir_syntax! {
        all=union()->tee();
        source_iter([crate::message(41,false,false)])->all;
        all->filter(|m:&crate::Message|m.id.wave<2)->map(|mut m:crate::Message|{m.id.wave+=1;m})->defer_tick()->all;
        all->filter(|m:&crate::Message|m.id.wave==2)->for_each(|m|sink.borrow_mut().push(m));
    };
    graph.run_available_sync();
    let out = &seen.borrow()[0];
    assert_eq!(out.id.root, 41);
    assert_eq!(out.id.activation, 41);
    assert_eq!(out.id.wave, 2);
}
#[tokio::test(flavor = "current_thread")]
async fn scalar_timestamps_do_not_isolate_independent_root_completion() {
    crate::timely_engine::run(|e| {
        let a = crate::message(1, false, true);
        let b = crate::message(2, true, false);
        e.submit(&[a.clone()], true).unwrap();
        e.advance_to(1);
        e.submit(&[b.clone()], true).unwrap();
        e.seal();
        e.step();
        assert_eq!(e.output.borrow()[0].id, b.id);
        assert!(
            e.frontier_less_equal(1),
            "unrelated earlier pending root blocks scalar frontier"
        );
        e.state.borrow_mut().release(a.id);
        e.step();
        assert!(e.drained());
    });
}
#[tokio::test(flavor = "current_thread")]
async fn dfir_native_keyed_join_correlates_interleaved_ports() {
    let (tx, rx) = dfir_rs::util::unbounded_channel::<(u64, i64)>();
    let (ty, ry) = dfir_rs::util::unbounded_channel::<(u64, i64)>();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let mut graph = dfir_rs::dfir_syntax! {
        source_stream(rx)->[0]joined;source_stream(ry)->[1]joined;
        joined=join::<'static>()->for_each(|v|sink.borrow_mut().push(v));
    };
    tx.send((1, 10)).unwrap();
    ty.send((2, 200)).unwrap();
    graph.run_available_sync();
    assert!(seen.borrow().is_empty());
    ty.send((1, 100)).unwrap();
    tx.send((2, 20)).unwrap();
    graph.run_available_sync();
    let mut out = seen.borrow().clone();
    out.sort();
    assert_eq!(out, vec![(1, (10, 100)), (2, (20, 200))]);
}
