mod support;

use phenix_graph::{
    ComponentDef,
    contract::Contract,
    interface::{InterfaceError, InterfacePhase},
    network::{NetworkBuilder, NetworkError},
    process::LocalProcessDispatcher,
};
use phenix_kernel::host::LifecycleError;
use phenix_plugin::PluginCatalog;
use phenix_storage_graph::derived::{DerivedHost, Error, GraphDeriveExt};
use std::{
    error::Error as _,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use support::{Fixture, Number, Plugin, authored, local, options, sum};

fn assert_lookup_error(error: &Error, expected: &NetworkError) {
    let source = error
        .source()
        .unwrap()
        .downcast_ref::<NetworkError>()
        .unwrap();
    let Error::Network(stored) = error else {
        panic!("{error}")
    };
    assert!(std::ptr::eq(source, stored.as_ref()));
    assert_eq!(source, expected);
    assert!(source.source().is_none());
}

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
fn real_query_binding_preserves_lookup_errors_but_string_failures_have_no_source() {
    let fixture = Fixture::new(false);
    let (plugin, spec) = sum(&fixture, false);
    let runtime = local(plugin);
    let (_, foreign_spec) = sum(&fixture, false);
    let error = runtime
        .bind_query(&foreign_spec)
        .err()
        .expect("foreign declaration");
    assert_lookup_error(&error, &NetworkError::UnknownDefinition);

    let error = runtime.bind_query(&spec).unwrap().query().unwrap_err();
    assert!(matches!(error, Error::Graph(_)));
    assert!(error.source().is_none());
    let error = Error::from("genuine string diagnostic".to_owned());
    assert!(matches!(&error, Error::Graph(message) if message == "genuine string diagnostic"));
    assert!(error.source().is_none());
}

#[test]
fn query_tap_and_root_lookup_errors_survive_runtime_and_preparation_binding() {
    let fixture = Fixture::new(false);
    let mut graph = NetworkBuilder::new();
    let value = graph
        .derive("sum", (&fixture.a,), |value: Number| value.0)
        .unwrap();
    let spec = graph.query_spec("query", &value).unwrap();
    let definition = graph.build().unwrap();
    for (tap_count, root_count, expected) in [
        (0, 1, NetworkError::UnknownDefinition),
        (2, 1, NetworkError::AmbiguousDefinition),
        (1, 0, NetworkError::UnknownDefinition),
        (1, 2, NetworkError::AmbiguousDefinition),
    ] {
        let mut graph = NetworkBuilder::new();
        for index in 0..tap_count {
            graph
                .add(
                    format!("tap{index}"),
                    definition.instances()["query"].clone(),
                )
                .unwrap();
        }
        for index in 0..root_count {
            graph
                .add(
                    format!("root{index}"),
                    definition.instances()["sum.sources"].clone(),
                )
                .unwrap();
        }
        let mut plugin = authored(graph, spec.clone(), None);
        let activations = Arc::new(AtomicUsize::new(0));
        let starts = activations.clone();
        let author = plugin.author.clone();
        plugin.author = Arc::new(move || {
            let mut artifacts = author()?;
            let starts = starts.clone();
            artifacts.activate(move || {
                starts.fetch_add(1, Ordering::SeqCst);
                Err("binding failure must prevent activation".into())
            })?;
            Ok(artifacts)
        });
        let runtime = local(plugin.clone());
        let error = runtime
            .bind_query(&spec)
            .err()
            .expect("invalid declaration membership");
        assert_lookup_error(&error, &expected);
        assert_eq!(activations.load(Ordering::SeqCst), 0);

        let options = options(&plugin);
        let mut catalog = PluginCatalog::new();
        let kind = catalog
            .register("domain", |plugin: Plugin| Ok(plugin))
            .unwrap();
        let mut plugins = catalog.instance();
        plugins.load(&kind, plugin).unwrap();
        let host = DerivedHost::new(plugins);
        let mut checked = false;
        let error = host
            .prepare(
                vec![],
                options,
                Arc::new(LocalProcessDispatcher),
                "lookup failure",
                |binding| {
                    let error = spec
                        .validate_binding(binding)
                        .expect_err("invalid declaration membership");
                    assert_lookup_error(&error, &expected);
                    checked = true;
                    Err(error.into())
                },
            )
            .err()
            .expect("preparation binding must fail");
        assert!(matches!(error, LifecycleError::Binding(_)));
        assert!(checked);
        let mut source = error.source();
        loop {
            let cause = source.expect("derived error must remain in binding error chain");
            if let Some(error) = cause.downcast_ref::<Error>() {
                assert_lookup_error(error, &expected);
                break;
            }
            source = cause.source();
        }
        assert_eq!(activations.load(Ordering::SeqCst), 0);
        assert!(host.active().is_none());
    }
}
