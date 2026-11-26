use phenix_graph::{
    ComponentDef, Value,
    compose::{ComposeError, Composer},
    connection::{
        ConnectionError, ConnectionQueue, ConnectionSet, FanoutTarget, Packet, PrepareError,
    },
    contract::Contract,
    interface::{InterfaceError, InterfacePhase, InterfaceReason},
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
    assert_eq!(*source.reason, InterfaceReason::MissingPort);
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
    assert_eq!(
        *source.reason,
        InterfaceReason::MissingExplicitClaim {
            contract: "test/worker".into(),
        }
    );
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
fn failure_preserves_interface_reason_and_exact_shape_context() {
    use phenix_graph::{Failure, FailureContext, FailureKind, Interface, PortDef};

    let required = Arc::new(Interface::new([PortDef::local::<i64>("value")], []).unwrap());
    let contract = Contract::new("test/shape", required).unwrap();
    for actual in [
        Interface::default(),
        Interface::new([PortDef::local::<bool>("value")], []).unwrap(),
        Interface::new([], [PortDef::local::<i64>("value")]).unwrap(),
    ] {
        let error = contract.check_shape(&actual).unwrap_err();
        let original = error.clone();
        let failure = Failure::external("invalid interface", error)
            .with_kind(FailureKind::Framework)
            .with_context(FailureContext {
                path: Some("worker".into()),
                ..Default::default()
            });
        let source = failure
            .source()
            .unwrap()
            .downcast_ref::<InterfaceError>()
            .unwrap();
        assert_eq!(source, &original);
        assert!(matches!(
            *source.reason,
            InterfaceReason::MissingPort
                | InterfaceReason::IncompatibleType
                | InterfaceReason::WrongDirection
        ));
        assert_eq!(source.phase, InterfacePhase::Definition);
        assert_eq!(source.expected, Some(Value::local(0_i64).identity()));
        assert_eq!(
            source.actual,
            actual
                .input("value")
                .or_else(|| actual.output("value"))
                .map(|port| port.value.clone())
        );
        assert_eq!(source.endpoint.as_deref(), Some("input value"));
        assert_eq!(source.contract.as_deref(), Some("test/shape"));
        assert_eq!(failure.kind(), FailureKind::Framework);
        assert_eq!(failure.context().path.as_deref(), Some("worker"));
        assert!(source.source().is_none());
    }
}

struct InvalidWire;
impl phenix_graph::ContractValue for InvalidWire {
    fn schema() -> phenix_graph::Schema {
        phenix_graph::Schema {
            name: "invalid-wire".into(),
            revision: 0,
            structure: "unit".into(),
            codec: "".into(),
        }
    }
    fn encode(&self) -> Vec<u8> {
        vec![]
    }
    fn decode(_: &[u8]) -> Result<Self, phenix_graph::Failure> {
        Ok(Self)
    }
}
impl phenix_graph::InputRecord for InvalidWire {
    fn input_ports() -> Result<Vec<phenix_graph::PortDef>, InterfaceError> {
        Ok(vec![
            phenix_graph::InputKey::<Self>::encoded("value").definition()?,
        ])
    }
    fn map_inputs(
        _: &mut phenix_graph::ProcessContext,
        _: &str,
        _: impl FnOnce(&mut phenix_graph::ProcessContext, Self) -> Result<Value, phenix_graph::Failure>,
    ) -> Result<Value, phenix_graph::Failure> {
        panic!("invalid schema must fail before reconstruction")
    }
}

#[test]
fn input_record_and_key_definition_failures_keep_concrete_interface_sources() {
    use phenix_graph::interface::PortableSchemaField;
    use phenix_graph::{FailureKind, InputKey, ProcessContext};

    let mut context = ProcessContext::new([("value".into(), Value::of(7_i64))].into(), vec![]);
    let record_failure = context.input_record::<InvalidWire>().unwrap_err();
    let key_failure = InputKey::<InvalidWire>::encoded("value")
        .map_input(&mut context, "", |_, value| Ok(Value::of(value)))
        .unwrap_err();
    for failure in [record_failure, key_failure] {
        assert_eq!(failure.kind(), FailureKind::Framework);
        let source = failure
            .source()
            .unwrap()
            .downcast_ref::<InterfaceError>()
            .unwrap();
        assert_eq!(
            *source.reason,
            InterfaceReason::InvalidPortableSchema {
                field: PortableSchemaField::Codec,
                value: "".into(),
            }
        );
        assert_eq!(source.phase, InterfacePhase::Definition);
        assert_eq!(source.path.as_ref(), "");
        assert_eq!(source.expected, None);
        assert_eq!(source.actual, None);
        assert!(source.source().is_none());
    }
    assert!(context.input("value").is_some());
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
        ComposeError::InvalidEdit {
            registration: "diagnostic".into(),
            reason: Box::new(phenix_graph::InvalidEditReason::MissingReplacement {
                path: "missing".into()
            }),
        }
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
                preparation: phenix_graph::connection::AdapterRef::Owned(Arc::new(|_: &Value| {
                    Err(phenix_graph::Failure::application_message("adapter failed"))
                })),
            }],
        )
        .unwrap_err();
    assert!(
        matches!(&error, PrepareError::Adapter(failure) if failure.summary() == "adapter failed")
    );
    let failure = error
        .source()
        .unwrap()
        .downcast_ref::<phenix_graph::Failure>()
        .unwrap();
    assert!(failure.source().is_none());
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
