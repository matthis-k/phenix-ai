mod support;
use phenix_graph::{
    Interface, PortDef,
    compose::{Edit, NetworkPatch},
    contract::Contract,
    network::NetworkError,
    process::LocalProcessDispatcher,
};
use phenix_kernel::{BuildOptions, ContractDefinition, Kernel, Key, ResolutionError};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::{error::Error, sync::Arc};
#[derive(Clone, Copy)]
enum Case {
    Conflict,
    Composition,
    Compilation,
    WrongOrigin,
    Interface,
    Catalog,
}
impl PluginInstance for Case {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let contract = support::contract("diagnostics/test@1");
        into.add(ContractDefinition {
            id: "contract".into(),
            contract: contract.clone(),
        })?;
        if matches!(self, Self::Conflict) {
            into.add(ContractDefinition {
                id: "other".into(),
                contract: Contract::new(
                    contract.id(),
                    Arc::new(Interface::new([], [PortDef::local::<String>("value")]).unwrap()),
                )
                .unwrap(),
            })?;
        }
        let mut patch = match self {
            Self::Compilation => NetworkPatch::new([Edit::AddSource("missing".into())]),
            Self::Catalog => support::patch("service", false, true, 0, "different@1"),
            _ => support::patch("service", false, false, 0, "diagnostics/test@1"),
        };
        if matches!(self, Self::Catalog) {
            let conflicting = Contract::new(
                contract.id(),
                Arc::new(Interface::new([PortDef::local::<i64>("run")], []).unwrap()),
            )
            .unwrap();
            let definition = phenix_graph::component::ComponentDef::primitive(
                conflicting.required().clone(),
                Arc::new(|| support::Transform {
                    increment: 0,
                    emitted: false,
                }),
            )
            .claim(&conflicting)
            .unwrap();
            patch = NetworkPatch::new([Edit::Add {
                path: "service".into(),
                definition: Arc::new(definition),
            }]);
        }
        let mut contribution = support::contribution("main", patch);
        if matches!(self, Self::Interface) {
            contribution = contribution.offer("offer", "service", Key::new("plugin", "contract"));
        }
        if matches!(self, Self::Composition) {
            contribution = contribution.prepare(|_| Err("composer failed".into()));
        }
        contribution.register(into)
    }
}
fn failure(case: Case) -> ResolutionError {
    struct Foreign;
    impl PluginInstance for Foreign {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            support::contribution("main", NetworkPatch::new([]))
                .after([Key::new("plugin", "main")])
                .offer("offer", "service", Key::new("plugin", "contract"))
                .register(into)
        }
    }
    let mut options = BuildOptions::default();
    support::grant(
        &mut options,
        Key::new("plugin", "main"),
        [match case {
            Case::Compilation => phenix_graph::compose::Footprint::Source("missing".into()),
            _ => phenix_graph::compose::Footprint::Component("service".into()),
        }],
    );
    let snapshot = support::snapshot("plugin", case);
    let mut snapshots = vec![snapshot];
    if matches!(case, Case::WrongOrigin) {
        snapshots.push(support::snapshot("foreign", Foreign));
    }
    Kernel::build_with_options(
        "diagnostics",
        snapshots,
        options.with_dispatcher(Arc::new(LocalProcessDispatcher)),
    )
    .err()
    .unwrap()
}
#[test]
fn build_failures_identify_definition_composer_and_origin() {
    assert!(
        matches!(failure(Case::Conflict), ResolutionError::ConflictingDefinition { definition, contract } if definition == Key::new("plugin", "other") && contract == "diagnostics/test@1")
    );
    let composition = failure(Case::Composition);
    assert!(
        matches!(&composition, ResolutionError::Composition { contribution, .. } if *contribution == Key::new("plugin", "main"))
    );
    assert_eq!(composition.source().unwrap().to_string(), "composer failed");
    assert!(
        matches!(failure(Case::WrongOrigin), ResolutionError::WrongOrigin { offer, producer, unit } if offer == Key::new("foreign", "offer") && *producer == Key::new("foreign", "main") && unit == "service")
    );
}
#[test]
fn compilation_preserves_structured_cause_and_source() {
    let error = failure(Case::Compilation);
    let ResolutionError::Compilation { cause } = &error else {
        panic!("{error:?}")
    };
    assert_eq!(
        error.source().unwrap().downcast_ref::<NetworkError>(),
        Some(cause)
    );
    assert!(matches!(cause, NetworkError::UnknownProcess(path) if path == "missing"));
}
#[test]
fn interface_failures_identify_offer_or_catalog_and_preserve_source() {
    for (case, expected_offer) in [
        (Case::Interface, Some(Key::new("plugin", "offer"))),
        (Case::Catalog, None),
    ] {
        let error = failure(case);
        let ResolutionError::Interface { offer, cause, .. } = &error else {
            panic!("{error:?}")
        };
        assert_eq!(*offer, expected_offer);
        assert_eq!(
            error.source().unwrap().downcast_ref::<NetworkError>(),
            Some(cause.as_ref())
        );
    }
}
#[test]
fn interface_fields_and_missing_unit_remain_machine_readable() {
    let error = failure(Case::Interface);
    let ResolutionError::Interface { cause, unit, .. } = error else {
        unreachable!()
    };
    assert_eq!(unit, "service");
    let NetworkError::Interface(cause) = *cause else {
        unreachable!()
    };
    assert!(matches!(
        cause.reason.as_ref(),
        phenix_graph::interface::InterfaceReason::MissingExplicitClaim { contract }
            if contract.as_ref() == "diagnostics/test@1"
    ));
    let missing = ResolutionError::MissingUnit {
        offer: Key::new("plugin", "offer"),
        unit: "service".into(),
        cause: Box::new(NetworkError::UnknownProcess("service".into())),
    };
    assert!(missing.to_string().contains("service"));
    assert!(matches!(
        missing.source().unwrap().downcast_ref::<NetworkError>(),
        Some(NetworkError::UnknownProcess(path)) if path == "service"
    ));
}
