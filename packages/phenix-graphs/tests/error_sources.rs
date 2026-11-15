use phenix_graph::{
    ComponentDef, Value,
    compose::{ComposeError, Composer},
    connection::{
        ConnectionError, ConnectionQueue, ConnectionSet, FanoutTarget, Packet, PrepareError,
    },
    contract::Contract,
    interface::{InterfaceError, InterfacePhase},
    network::{NetworkBuilder, NetworkError},
    runtime::RequestError,
};
use std::{
    error::Error,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[test]
fn builder_and_binding_errors_preserve_interface_context() {
    let definition = Arc::new(ComponentDef::map(|value: i64| value));
    let contract = Contract::new("test/worker", definition.interface().clone()).unwrap();
    let mut builder = NetworkBuilder::new();
    let error = builder.claim(&contract).unwrap_err();
    let source = error
        .source()
        .unwrap()
        .downcast_ref::<InterfaceError>()
        .unwrap();
    let NetworkError::Interface(stored) = &error else {
        panic!("{error}")
    };
    assert!(std::ptr::eq(source, stored.as_ref()));
    assert_eq!(source.contract.as_deref(), Some("test/worker"));
    assert_eq!(source.phase, InterfacePhase::Definition);
    assert_eq!(source.path.as_ref(), "");
    assert_eq!(source.endpoint.as_deref(), Some("input input"));
    assert_eq!(source.expected, Some(Value::local(0_i64).identity()));
    assert_eq!(source.actual, None);
    assert!(source.source().is_none());

    builder.add("worker", definition).unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    let error = compiled
        .bind("worker", &contract)
        .err()
        .expect("unclaimed component");
    let source = error
        .source()
        .unwrap()
        .downcast_ref::<InterfaceError>()
        .unwrap();
    assert_eq!(source.contract.as_deref(), Some("test/worker"));
    assert_eq!(source.phase, InterfacePhase::Compiled);
    assert_eq!(source.path.as_ref(), "worker");
    assert_eq!(source.message.as_ref(), "missing explicit contract claim");
    assert_eq!(source.endpoint, None);
    assert_eq!(source.expected, None);
    assert_eq!(source.actual, None);
    assert!(source.source().is_none());
    assert!(
        compiled
            .bind("missing", &contract)
            .err()
            .unwrap()
            .source()
            .is_none()
    );
}

#[test]
fn composition_forwards_network_then_interface_without_inventing_leaf_causes() {
    let definition = Arc::new(ComponentDef::map(|value: i64| value));
    let contract = Contract::new("test/worker", definition.interface().clone()).unwrap();
    let mut builder = NetworkBuilder::new();
    builder.add("worker", definition).unwrap();
    builder.require_contract("worker", &contract).unwrap();
    let error = Composer::new(builder.build().unwrap())
        .err()
        .expect("unclaimed component");
    let source = error
        .source()
        .unwrap()
        .downcast_ref::<NetworkError>()
        .unwrap();
    let ComposeError::Network(stored) = &error else {
        panic!("{error}")
    };
    assert!(std::ptr::eq(source, stored));
    let source = source
        .source()
        .unwrap()
        .downcast_ref::<InterfaceError>()
        .unwrap();
    assert_eq!(source.contract.as_deref(), Some("test/worker"));
    assert_eq!(source.path.as_ref(), "worker");
    assert_eq!(source.phase, InterfacePhase::Compiled);
    assert!(source.source().is_none());
    assert!(
        ComposeError::InvalidEdit("diagnostic".into())
            .source()
            .is_none()
    );
    assert!(ComposeError::Panicked.source().is_none());
}

#[test]
fn preparation_forwards_connection_but_not_adapter_strings() {
    let packet = Packet::new(Value::local(7_i64));
    let connections = ConnectionSet::new(vec![ConnectionQueue::new(
        NonZeroUsize::new(1).unwrap(),
        packet.value().identity(),
    )]);
    let error = connections
        .prepare(&packet, &[FanoutTarget::identity(1)])
        .unwrap_err();
    let source = error
        .source()
        .unwrap()
        .downcast_ref::<ConnectionError>()
        .unwrap();
    let PrepareError::Connection(stored) = &error else {
        panic!("{error}")
    };
    assert!(std::ptr::eq(source, stored));
    assert_eq!(*source, ConnectionError::InvalidDestination);
    assert!(source.source().is_none());

    let error = connections
        .prepare(
            &packet,
            &[FanoutTarget {
                connection: 0,
                preparation: Some(Arc::new(|_: &Value| Err("adapter failed".into()))),
            }],
        )
        .unwrap_err();
    assert_eq!(error, PrepareError::Adapter("adapter failed".into()));
    assert!(error.source().is_none());
    assert!(connections.queues()[0].is_empty());
}

#[test]
fn request_injection_exposes_transport_and_retains_value_without_running_callbacks() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut builder = NetworkBuilder::new();
    let worker = builder
        .add(
            "worker",
            Arc::new(ComponentDef::map(move |value: i64| {
                observed.fetch_add(1, Ordering::SeqCst);
                value
            })),
        )
        .unwrap();
    builder
        .expose_input(
            "input",
            worker.input::<i64>("input").unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    let mut session = builder.build().unwrap().compile().unwrap().start();
    let error = session.begin_request(Some("missing")).unwrap_err();
    assert_eq!(error, RequestError::UnknownOutput);
    assert!(error.source().is_none());
    let request = session.begin_request(None).unwrap();
    let error = session
        .inject_request_local(request, "input", true)
        .unwrap_err();
    assert_eq!(error.value.borrow::<bool>(), Some(&true));
    let source = error
        .source()
        .unwrap()
        .downcast_ref::<RequestError>()
        .unwrap();
    assert!(std::ptr::eq(source, &error.reason));
    let source = source
        .source()
        .unwrap()
        .downcast_ref::<ConnectionError>()
        .unwrap();
    let RequestError::Transport(stored) = &error.reason else {
        panic!("{error}")
    };
    assert!(std::ptr::eq(source, stored));
    assert_eq!(*source, ConnectionError::WrongType);
    assert!(source.source().is_none());
    assert_eq!(session.request_work(request).unwrap().queued, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
