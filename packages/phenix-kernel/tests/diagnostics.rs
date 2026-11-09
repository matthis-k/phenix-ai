use phenix_graph::{
    CompileDiagnostic, CompilePhase, DeclaredValue, GraphContract, InterfaceError, InterfacePhase,
    InterfaceShape, LocalExecutor, TypeIdentity,
};
use phenix_kernel::{BuildOptions, ContractDefinition, Contribution, Kernel, Key, ResolutionError};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::{any::TypeId, error::Error, sync::Arc};

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
        let contract = GraphContract::new("diagnostics/test@1", InterfaceShape::new()).unwrap();
        into.add(ContractDefinition {
            id: "contract".into(),
            contract: contract.clone(),
        })?;
        if matches!(self, Self::Conflict) {
            into.add(ContractDefinition {
                id: "other".into(),
                contract: GraphContract::new(
                    contract.id(),
                    InterfaceShape::new()
                        .output("value", DeclaredValue::local::<i64>())
                        .unwrap(),
                )
                .unwrap(),
            })?;
        }
        if matches!(self, Self::WrongOrigin) {
            // The unit belongs to another composer even though it has the
            // requested shape and claim. Attaching an offer to main must fail.
            let claimed = contract.clone();
            Contribution::new("source", move |context| {
                let module = phenix_graph::GraphBuilder::new("empty");
                context.graph.instance("service", &module.module())?;
                context.graph.implements("service", &claimed)?;
                Ok(())
            })
            .register(into)?;
        }
        let case = *self;
        let main = Contribution::new("main", move |context| match case {
            Self::Composition => Err("composer failed".into()),
            Self::Compilation => context
                .graph
                .require_contract("missing", &contract)
                .map_err(Into::into),
            Self::Catalog => {
                let different = GraphContract::new(
                    contract.id(),
                    InterfaceShape::new()
                        .input_slot("run", "value", DeclaredValue::local::<i64>())
                        .unwrap()
                        .output("value", DeclaredValue::local::<i64>())
                        .unwrap(),
                )
                .unwrap();
                context.graph.node("service", |v: i64| v)?;
                context
                    .graph
                    .implements("service", &different)
                    .map_err(Into::into)
            }
            _ => Ok(()),
        });
        let main = match self {
            Self::WrongOrigin => main.offer("offer", "service", Key::new("plugin", "contract")),
            Self::Interface => {
                main.offer("offer", "invalid/../path", Key::new("plugin", "contract"))
            }
            _ => main,
        };
        main.register(into)
    }
}

fn failure(case: Case) -> ResolutionError {
    let mut catalog = PluginCatalog::new();
    let plugin = catalog.register("plugin", |case: Case| Ok(case)).unwrap();
    let mut plugins = catalog.instance();
    plugins.load(&plugin, case).unwrap();
    Kernel::build_with_options(
        "diagnostics",
        plugins.snapshots(),
        Arc::new(LocalExecutor),
        BuildOptions::default(),
    )
    .err()
    .expect("diagnostic build must fail")
}

#[test]
fn build_failures_identify_definition_composer_and_origin() {
    let conflict = failure(Case::Conflict);
    assert!(
        matches!(&conflict, ResolutionError::ConflictingDefinition { definition, contract }
        if *definition == Key::new("plugin", "other") && contract == "diagnostics/test@1")
    );
    assert!(
        conflict
            .to_string()
            .contains("conflicting contract definition")
    );
    let composition = failure(Case::Composition);
    assert!(
        matches!(&composition, ResolutionError::Composition { contribution, cause }
        if *contribution == Key::new("plugin", "main") && cause.to_string() == "composer failed")
    );
    assert!(composition.to_string().contains("composer failed"));
    assert_eq!(composition.source().unwrap().to_string(), "composer failed");
    let origin = failure(Case::WrongOrigin);
    assert!(
        matches!(&origin, ResolutionError::WrongOrigin { offer, producer, unit }
        if *offer == Key::new("plugin", "offer") && **producer == Key::new("plugin", "main") && unit == "invalid/../path")
    );
    assert!(
        origin
            .to_string()
            .contains("producer ownership origin mismatch")
    );
    assert!(
        origin
            .to_string()
            .contains("contribution generation origin mismatch")
    );
}

#[test]
fn compilation_preserves_structured_cause_and_source() {
    let error = failure(Case::Compilation);
    let ResolutionError::Compilation { cause } = &error else {
        panic!("expected compilation diagnostic, got {error:?}");
    };
    assert_eq!(cause.phase, CompilePhase::Index);
    assert!(cause.message.contains("missing"));
    assert_eq!(error.to_string(), cause.to_string());
    assert_eq!(
        error.source().unwrap().downcast_ref::<CompileDiagnostic>(),
        Some(cause)
    );
}

#[test]
fn interface_failures_identify_offer_or_catalog_and_preserve_source() {
    for (case, expected_offer) in [
        (Case::Interface, Some(Key::new("plugin", "offer"))),
        (Case::Catalog, None),
    ] {
        let error = failure(case);
        let ResolutionError::Interface { offer, unit, cause } = &error else {
            panic!("expected interface diagnostic, got {error:?}");
        };
        assert_eq!(*offer, expected_offer);
        if offer.is_some() {
            assert_eq!(unit, "invalid/../path");
            assert_eq!(cause.message.as_ref(), "invalid unit path");
        } else {
            assert_eq!(unit, cause.path.as_ref());
            assert!(
                error
                    .to_string()
                    .contains("host/graph contract catalog conflict")
            );
        }
        assert_eq!(
            error.source().unwrap().downcast_ref::<InterfaceError>(),
            Some(cause.as_ref())
        );
    }
}

#[test]
fn interface_fields_and_missing_unit_remain_machine_readable() {
    let cause = InterfaceError {
        contract: Some("diagnostics/test@1".into()),
        path: "service".into(),
        phase: InterfacePhase::Compiled,
        endpoint: Some("value".into()),
        expected: Some(TypeIdentity::Local(TypeId::of::<i64>())),
        actual: Some(TypeIdentity::Local(TypeId::of::<String>())),
        message: "incompatible endpoint".into(),
    };
    let error = ResolutionError::Interface {
        offer: Some(Key::new("plugin", "offer")),
        unit: "service".into(),
        cause: Box::new(cause.clone()),
    };
    assert_eq!(
        error.source().unwrap().downcast_ref::<InterfaceError>(),
        Some(&cause)
    );
    let missing = ResolutionError::MissingUnit {
        offer: Key::new("plugin", "offer"),
        unit: "service".into(),
    };
    assert!(
        missing
            .to_string()
            .contains("compiled unit not found for offer")
    );
    assert!(missing.to_string().contains("service"));
    assert!(missing.source().is_none());
}
