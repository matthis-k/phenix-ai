mod support;

use phenix_graph::{
    ComponentDef,
    contract::Contract,
    interface::{InterfaceError, InterfacePhase},
    network::{NetworkBuilder, NetworkError},
};
use phenix_storage_graph::derived::{Error, GraphDeriveExt};
use std::{error::Error as _, sync::Arc};
use support::{Fixture, Number, local, sum};

#[test]
fn derived_authoring_preserves_duplicate_and_foreign_scope_errors() {
    let fixture = Fixture::new(false);
    let mut graph = NetworkBuilder::new();
    let values = graph.sources("values", (&fixture.a,)).unwrap();
    let error = graph
        .sources("values", (&fixture.a,))
        .err()
        .expect("duplicate source");
    let source = error
        .source()
        .unwrap()
        .downcast_ref::<NetworkError>()
        .unwrap();
    let Error::Network(stored) = &error else {
        panic!("{error}")
    };
    assert!(std::ptr::eq(source, stored.as_ref()));
    assert_eq!(*source, NetworkError::DuplicateInstance("values".into()));
    assert!(source.source().is_none());

    let mut other = NetworkBuilder::new();
    let error = other
        .derive_from("foreign", &values, |value: Number| value.0)
        .err()
        .expect("foreign source");
    let source = error
        .source()
        .unwrap()
        .downcast_ref::<NetworkError>()
        .unwrap();
    assert_eq!(*source, NetworkError::ForeignScope);
    assert!(source.source().is_none());
    assert!(other.build().unwrap().instances().is_empty());
    assert_eq!(graph.build().unwrap().instances().len(), 1);
}

#[test]
fn network_conversion_keeps_interface_context_and_downcasts_through_both_envelopes() {
    let definition = Arc::new(ComponentDef::map(|value: i64| value));
    let contract = Contract::new("test/derived", definition.interface().clone()).unwrap();
    let mut graph = NetworkBuilder::new();
    graph.add("worker", definition).unwrap();
    let compiled = graph.build().unwrap().compile().unwrap();
    let network = compiled
        .bind("worker", &contract)
        .err()
        .expect("unclaimed component");
    let expected = network
        .source()
        .unwrap()
        .downcast_ref::<InterfaceError>()
        .unwrap()
        .clone();
    let error = Error::from(network);
    let source = error
        .source()
        .unwrap()
        .downcast_ref::<NetworkError>()
        .unwrap();
    let source = source
        .source()
        .unwrap()
        .downcast_ref::<InterfaceError>()
        .unwrap();
    assert_eq!(source, &expected);
    assert_eq!(source.contract.as_deref(), Some("test/derived"));
    assert_eq!(source.path.as_ref(), "worker");
    assert_eq!(source.phase, InterfacePhase::Compiled);
    assert!(source.source().is_none());
}

#[test]
fn real_query_binding_and_string_failures_do_not_invent_sources() {
    let fixture = Fixture::new(false);
    let (plugin, spec) = sum(&fixture, false);
    let runtime = local(plugin);
    let (_, foreign_spec) = sum(&fixture, false);
    let error = runtime
        .bind_query(&foreign_spec)
        .err()
        .expect("foreign declaration");
    assert!(matches!(error, Error::WrongGeneration));
    assert!(error.source().is_none());

    let error = runtime.bind_query(&spec).unwrap().query().unwrap_err();
    assert!(matches!(error, Error::Graph(_)));
    assert!(error.source().is_none());
    let error = Error::from("genuine string diagnostic".to_owned());
    assert!(matches!(&error, Error::Graph(message) if message == "genuine string diagnostic"));
    assert!(error.source().is_none());
}
