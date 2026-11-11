mod support;
use phenix_graph::{
    Interface, PortDef,
    compose::{Edit, NetworkPatch},
    contract::Contract,
    network::NetworkError,
    process::LocalProcessDispatcher,
};
use phenix_kernel::{ContractDefinition, Kernel, Key, NetworkOffer, ResolutionError};
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
        if matches!(self, Self::WrongOrigin | Self::Interface) {
            into.add(NetworkOffer {
                id: "offer".into(),
                producer: Key::new(
                    if matches!(self, Self::WrongOrigin) {
                        "foreign"
                    } else {
                        "plugin"
                    },
                    "main",
                ),
                unit: "service".into(),
                contract: Key::new("plugin", "contract"),
            })?;
        }
        let patch = match self {
            Self::Compilation => NetworkPatch::new([Edit::AddSource("missing".into())]),
            Self::Catalog => support::patch("service", false, true, 0, "different@1"),
            _ => support::patch("service", false, false, 0, "diagnostics/test@1"),
        };
        let mut contribution = support::contribution("main", patch);
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
            contribution.patch = NetworkPatch::new([Edit::Add {
                path: "service".into(),
                definition: Arc::new(definition),
            }]);
        }
        if matches!(self, Self::Composition) {
            contribution.prepare = Some(Box::new(|_| Err("composer failed".into())));
        }
        into.add(contribution)
    }
}
fn failure(case: Case) -> ResolutionError {
    let snapshot = support::snapshot("plugin", case);
    let options = support::options(std::slice::from_ref(&snapshot));
    Kernel::build_with_options(
        "diagnostics",
        [snapshot],
        Arc::new(LocalProcessDispatcher),
        options,
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
        matches!(failure(Case::WrongOrigin), ResolutionError::WrongOrigin { offer, producer, unit } if offer == Key::new("plugin", "offer") && *producer == Key::new("foreign", "main") && unit == "service")
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
    assert!(cause.message.contains("claim"));
    let missing = ResolutionError::MissingUnit {
        offer: Key::new("plugin", "offer"),
        unit: "service".into(),
    };
    assert!(missing.to_string().contains("service"));
    assert!(missing.source().is_none());
}
