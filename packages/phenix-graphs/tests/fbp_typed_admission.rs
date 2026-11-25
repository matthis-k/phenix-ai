use phenix_graph::{
    ContractValue, InputKey, Interface, Schema, Value,
    connection::ConnectionError,
    network::{CompiledNetwork, NetworkBuilder},
    process::{Process, ProcessContext, ProcessEmission, Step},
    runtime::{RequestError, RequestQueuePolicy, RequestStatus, SessionOptions},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const LOCAL: InputKey<MoveOnly> = InputKey::new("public");
struct MoveOnly(String);

struct Echo(bool);
impl Process for Echo {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        if self.0 {
            self.0 = false;
            return Ok(Step::Complete);
        }
        let Some(value) = cx.take("in") else {
            return Ok(Step::Finished);
        };
        self.0 = true;
        Ok(Step::Emit(ProcessEmission::new("out", value)))
    }
}

fn network<T: 'static>(key: InputKey<T>) -> CompiledNetwork {
    let mut input = key.definition().unwrap();
    input.name = "in".into();
    let mut output = input.clone();
    output.name = "out".into();
    let mut builder = NetworkBuilder::new();
    let node = builder
        .node(
            "private",
            Arc::new(Interface::new([input], [output]).unwrap()),
            Arc::new(|| Echo(false)),
        )
        .unwrap();
    builder
        .expose_input_dynamic(
            "public",
            &node.input_endpoint("in").unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    builder
        .expose_output_dynamic("result", &node.output_endpoint("out").unwrap())
        .unwrap();
    builder.build().unwrap().compile().unwrap()
}

// 0 and 1 intentionally have the same complete schema but different Rust types.
struct Wire<const N: u8>(i64, Arc<AtomicUsize>);
impl<const N: u8> ContractValue for Wire<N> {
    fn schema() -> Schema {
        Schema {
            name: if N == 5 { "other" } else { "typed/request" }.into(),
            revision: if N == 2 { 2 } else { 1 },
            structure: if N == 3 {
                "record{other:i64}"
            } else {
                "record{value:i64}"
            }
            .into(),
            codec: if N == 4 { "other-codec" } else { "i64-le-v1" }.into(),
        }
    }
    fn encode(&self) -> Vec<u8> {
        self.1.fetch_add(1, Ordering::SeqCst);
        self.0.to_le_bytes().to_vec()
    }
    fn decode(bytes: &[u8]) -> Result<Self, phenix_graph::Failure> {
        Ok(Self(
            i64::from_le_bytes(
                bytes
                    .try_into()
                    .map_err(|cause| phenix_graph::Failure::external("bad i64", cause))?,
            ),
            Arc::default(),
        ))
    }
}

#[test]
fn move_only_local_and_canonical_encoded_values_execute() {
    let mut session = network(LOCAL).start();
    session.inject_key(LOCAL, MoveOnly("owned".into())).unwrap();
    session.run_until_idle(32);
    let packet = session.take_output("result").unwrap().unwrap();
    assert_eq!(packet.value().borrow::<MoveOnly>().unwrap().0, "owned");
    assert_eq!(packet.metadata().unwrap().request(), None);

    let encodes = Arc::new(AtomicUsize::new(0));
    let mut session = network(InputKey::<Wire<0>>::encoded("public")).start();
    session
        .inject_key(
            InputKey::<Wire<1>>::encoded("public"),
            Wire(41, encodes.clone()),
        )
        .unwrap();
    assert_eq!(encodes.load(Ordering::SeqCst), 1);
    session.run_until_idle(32);
    let packet = session.take_output("result").unwrap().unwrap();
    assert_eq!(packet.value().schema(), &Wire::<0>::schema().identity());
    assert_eq!(packet.value().bytes(), 41_i64.to_le_bytes());
    assert_eq!(Wire::<0>::decode(packet.value().bytes()).unwrap().0, 41);
    assert!(packet.value().borrow::<Wire<1>>().is_none());
    assert_eq!(encodes.load(Ordering::SeqCst), 1);
}

#[test]
fn actual_export_identity_and_mode_remain_authoritative() {
    let mut local = network(InputKey::<i64>::new("public")).start();
    let mut wire = network(InputKey::<i64>::encoded("public")).start();
    let error = local
        .inject_key(InputKey::<i64>::encoded("public"), 7)
        .unwrap_err();
    assert_eq!(error.reason, ConnectionError::WrongType);
    assert_eq!(error.value, Value::of(7_i64));
    let error = wire
        .inject_key(InputKey::<i64>::new("public"), 8)
        .unwrap_err();
    assert_eq!(error.reason, ConnectionError::WrongType);
    assert_eq!(error.value.borrow::<i64>(), Some(&8));
    let error = local
        .inject_key(InputKey::<String>::new("public"), "wrong".into())
        .unwrap_err();
    assert_eq!(error.reason, ConnectionError::WrongType);
    assert_eq!(error.value.borrow::<String>().unwrap(), "wrong");
    for name in ["missing", "private.in", "private/in", "in"] {
        let error = local
            .inject_key(InputKey::<MoveOnly>::new(name), MoveOnly(name.into()))
            .unwrap_err();
        assert_eq!(error.reason, ConnectionError::InvalidDestination);
        assert_eq!(error.value.borrow::<MoveOnly>().unwrap().0, name);
    }
}

fn wrong_schema<const N: u8>() {
    let mut session = network(InputKey::<Wire<0>>::encoded("public")).start();
    let encodes = Arc::new(AtomicUsize::new(0));
    let error = session
        .inject_key(
            InputKey::<Wire<N>>::encoded("public"),
            Wire(9, encodes.clone()),
        )
        .unwrap_err();
    assert_eq!(error.reason, ConnectionError::WrongType);
    assert_eq!(error.value.schema(), &Wire::<N>::schema().identity());
    assert_eq!(error.value.bytes(), 9_i64.to_le_bytes());
    assert_eq!(encodes.load(Ordering::SeqCst), 1);
}

#[test]
fn every_schema_field_is_checked_not_just_the_name() {
    wrong_schema::<2>();
    wrong_schema::<3>();
    wrong_schema::<4>();
    wrong_schema::<5>();
}

#[test]
fn full_closed_and_terminal_return_the_constructed_value() {
    let key = InputKey::<Wire<0>>::encoded("public");
    let encodes = Arc::new(AtomicUsize::new(0));
    let mut session = network(key).start();
    // One activation reservation and one bounded FIFO slot, not extra credit.
    for value in [1, 2] {
        session
            .inject_key(key, Wire(value, encodes.clone()))
            .unwrap();
    }
    let error = session
        .inject_key(key, Wire(3, encodes.clone()))
        .unwrap_err();
    assert_eq!(error.reason, ConnectionError::Full);
    assert_eq!(error.value.bytes(), 3_i64.to_le_bytes());
    session.run_until_idle(32);
    session.inject("public", error.value).unwrap(); // Retry the envelope, not the codec.
    assert_eq!(encodes.load(Ordering::SeqCst), 3);
    session.close_input("public").unwrap();
    let error = session
        .inject_key(key, Wire(4, encodes.clone()))
        .unwrap_err();
    assert_eq!(error.reason, ConnectionError::WriterClosed);
    assert_eq!(error.value.bytes(), 4_i64.to_le_bytes());
    session.cancel();
    let error = session
        .inject_key(key, Wire(5, encodes.clone()))
        .unwrap_err();
    assert_eq!(error.reason, ConnectionError::Cancelled);
    assert_eq!(error.value.bytes(), 5_i64.to_le_bytes());
    let error = session
        .inject_key(
            InputKey::<Wire<0>>::encoded("missing"),
            Wire(6, encodes.clone()),
        )
        .unwrap_err();
    assert_eq!(error.reason, ConnectionError::Cancelled);
    assert_eq!(error.value.bytes(), 6_i64.to_le_bytes());
    assert_eq!(encodes.load(Ordering::SeqCst), 6);
}

#[test]
fn scoped_admission_preserves_metadata_authority_credit_and_lane_reuse() {
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let key = InputKey::<Wire<0>>::encoded("public");
        let compiled = network(key);
        let mut session = compiled.start_with_options(SessionOptions {
            request_queue_policy: policy,
            max_requests: NonZeroUsize::new(1).unwrap(),
            ..Default::default()
        });
        let mut foreign = compiled.start();
        let foreign_id = foreign.begin_request(None).unwrap();
        let encodes = Arc::new(AtomicUsize::new(0));
        let mut calls = 0;
        let mut payload = || {
            calls += 1;
            Wire(calls, encodes.clone())
        };
        let error = session
            .inject_request_key(foreign_id, key, payload())
            .unwrap_err();
        assert_eq!(error.reason, RequestError::Foreign);
        assert_eq!(error.value.bytes(), 1_i64.to_le_bytes());
        let id = session.begin_request(None).unwrap();
        assert_eq!(session.begin_request(None), Err(RequestError::Full));
        let error = session
            .inject_request_key(id, InputKey::<Wire<0>>::encoded("private.in"), payload())
            .unwrap_err();
        assert_eq!(
            error.reason,
            RequestError::Transport(ConnectionError::InvalidDestination)
        );
        assert_eq!(error.value.bytes(), 2_i64.to_le_bytes());
        let error = session
            .inject_request_key(id, InputKey::<i64>::new("public"), 0)
            .unwrap_err();
        assert_eq!(
            error.reason,
            RequestError::Transport(ConnectionError::WrongType)
        );
        assert_eq!(error.value.borrow::<i64>(), Some(&0));
        session.inject_request_key(id, key, payload()).unwrap();
        session.inject_request_key(id, key, payload()).unwrap();
        let error = session.inject_request_key(id, key, payload()).unwrap_err();
        assert_eq!(error.reason, RequestError::Transport(ConnectionError::Full));
        assert_eq!(error.value.bytes(), 5_i64.to_le_bytes());
        session.close_request(id).unwrap();
        let error = session.inject_request_key(id, key, payload()).unwrap_err();
        assert_eq!(error.reason, RequestError::Sealed);
        assert_eq!(error.value.bytes(), 6_i64.to_le_bytes());
        session.run_until_idle(32);
        for expected in [3_i64, 4] {
            let packet = session.take_request_output(id, "result").unwrap().unwrap();
            assert_eq!(packet.value().bytes(), expected.to_le_bytes());
            let metadata = packet.metadata().unwrap();
            assert_eq!(metadata.request(), Some(id));
            assert_eq!(metadata.capture(), Some("result"));
            assert_eq!(metadata.output(), "out");
        }
        assert_eq!(
            session.request_status(id),
            Ok(RequestStatus::Settled { captures: 2 })
        );
        session.release_request(id).unwrap();
        let next = session.begin_request(Some("result")).unwrap();
        assert_ne!(next, id);
        let error = session.inject_request_key(id, key, payload()).unwrap_err();
        assert_eq!(error.reason, RequestError::Stale);
        assert_eq!(error.value.bytes(), 7_i64.to_le_bytes());
        session.inject_request_key(next, key, payload()).unwrap();
        session.close_request(next).unwrap();
        session.run_until_idle(32);
        let packet = session
            .take_request_output(next, "result")
            .unwrap()
            .unwrap();
        assert_eq!(packet.value().bytes(), 8_i64.to_le_bytes());
        assert_eq!(packet.metadata().unwrap().request(), Some(next));
        session.release_request(next).unwrap();
        let terminal = session.begin_request(None).unwrap();
        session.cancel();
        let error = session
            .inject_request_key(terminal, key, payload())
            .unwrap_err();
        assert_eq!(error.reason, RequestError::Stale);
        assert_eq!(error.value.bytes(), 9_i64.to_le_bytes());
        assert_eq!(encodes.load(Ordering::SeqCst), 9);
    }
}

#[test]
fn scoped_local_move_only_values_retain_their_payload_on_rejection() {
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let mut session = network(LOCAL).start_with_options(SessionOptions {
            request_queue_policy: policy,
            ..Default::default()
        });
        let id = session.begin_request(Some("result")).unwrap();
        session
            .inject_request_key(id, LOCAL, MoveOnly("scoped".into()))
            .unwrap();
        session.close_request(id).unwrap();
        let error = session
            .inject_request_key(id, LOCAL, MoveOnly("retained".into()))
            .unwrap_err();
        assert_eq!(error.reason, RequestError::Sealed);
        assert_eq!(error.value.borrow::<MoveOnly>().unwrap().0, "retained");
        session.run_until_idle(32);
        let packet = session.take_request_output(id, "result").unwrap().unwrap();
        assert_eq!(packet.value().borrow::<MoveOnly>().unwrap().0, "scoped");
        assert_eq!(packet.metadata().unwrap().request(), Some(id));
        session.release_request(id).unwrap();
    }
}
