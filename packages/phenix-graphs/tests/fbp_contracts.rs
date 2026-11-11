use phenix_graph::component::ComponentDef;
use phenix_graph::contract::Contract;
use phenix_graph::network::{NetworkBuilder, NetworkDef, NetworkError};
use phenix_graph::process::{Process, ProcessContext, ProcessFactory, Step};
use phenix_graph::{ContractValue, GraphOrigin, Interface, PortDef, Schema};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Noop;
impl Process for Noop {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        Ok(Step::Finished)
    }
}
fn factory() -> Arc<dyn ProcessFactory> {
    Arc::new(|| Noop)
}
fn io() -> Arc<Interface> {
    Arc::new(
        Interface::new(
            [PortDef::local::<i64>("in")],
            [PortDef::local::<i64>("out")],
        )
        .unwrap(),
    )
}
fn contract() -> Contract {
    Contract::new("test/worker", io()).unwrap()
}
fn leaf(claimed: bool) -> Arc<ComponentDef> {
    let definition = ComponentDef::primitive(io(), factory());
    Arc::new(if claimed {
        definition.claim(&contract()).unwrap()
    } else {
        definition
    })
}
fn module() -> NetworkDef {
    let mut builder = NetworkBuilder::new();
    let leaf = builder.add("leaf", leaf(true)).unwrap();
    builder
        .expose_input(
            "in",
            leaf.input::<i64>("in").unwrap(),
            NonZeroUsize::new(2).unwrap(),
        )
        .unwrap();
    builder
        .expose_output("out", leaf.output::<i64>("out").unwrap())
        .unwrap();
    builder.claim(&contract()).unwrap();
    builder.require_contract("leaf", &contract()).unwrap();
    builder.build().unwrap()
}

#[test]
fn definition_equality_is_exact_and_shape_never_infers_semantics() {
    let c = contract();
    assert_eq!(c, Contract::new("test/worker", io()).unwrap());
    c.check_shape(&io()).unwrap();
    assert!(c.check_implementation(&leaf(false)).is_err());
    c.check_implementation(&leaf(true)).unwrap();
    let extra = Arc::new(
        Interface::new(
            [PortDef::local::<i64>("in")],
            [
                PortDef::local::<i64>("out"),
                PortDef::local::<bool>("private"),
            ],
        )
        .unwrap(),
    );
    assert!(c.check_shape(&extra).is_err());
    let other = Contract::new(c.id(), extra.clone()).unwrap();
    assert_ne!(c, other);
    assert!(c.check_definition(&other).is_err());
    assert!(
        ComponentDef::primitive(io(), factory())
            .claim(&c)
            .unwrap()
            .claim(&other)
            .is_err()
    );
    assert!(
        ComponentDef::primitive(Arc::new(Interface::default()), factory())
            .claim(&c)
            .is_err()
    );
}

#[test]
fn exact_shape_errors_preserve_direction_endpoint_and_type_context() {
    for output in [false, true] {
        let port = PortDef::local::<i64>("value");
        let shape = if output {
            Interface::new([], [port.clone()])
        } else {
            Interface::new([port.clone()], [])
        }
        .unwrap();
        let c = Contract::new("test/shape", Arc::new(shape.clone())).unwrap();
        shape.check_shape(&shape).unwrap();
        let wrong_type = PortDef::local::<bool>("value");
        let mistyped = if output {
            Interface::new([], [wrong_type.clone()])
        } else {
            Interface::new([wrong_type.clone()], [])
        }
        .unwrap();
        let reversed = if output {
            Interface::new([port.clone()], [])
        } else {
            Interface::new([], [port.clone()])
        }
        .unwrap();
        let empty = Interface::default();
        for (expected, actual, message, expected_type, actual_type) in [
            (
                &shape,
                &empty,
                "missing port",
                Some(port.value.clone()),
                None,
            ),
            (&empty, &shape, "extra port", None, Some(port.value.clone())),
            (
                &shape,
                &mistyped,
                "incompatible port type",
                Some(port.value.clone()),
                Some(wrong_type.value),
            ),
            (
                &shape,
                &reversed,
                "port has wrong direction",
                Some(port.value.clone()),
                Some(port.value.clone()),
            ),
        ] {
            let error = expected.check_shape(actual).unwrap_err();
            assert_eq!(error.contract, None);
            assert_eq!(
                error.endpoint.as_deref(),
                Some(if output {
                    "output value"
                } else {
                    "input value"
                })
            );
            assert_eq!(error.message.as_ref(), message);
            assert_eq!(error.expected, expected_type);
            assert_eq!(error.actual, actual_type);
            if expected == &shape {
                let mut contextual = error;
                contextual.contract = Some(c.id().into());
                assert_eq!(c.check_shape(actual).unwrap_err(), contextual);
            }
        }
    }
}

#[test]
fn root_composite_and_nested_primitive_are_authoritatively_inspectable() {
    let mut root = NetworkBuilder::new();
    let component = root
        .add("module", Arc::new(ComponentDef::composite(module())))
        .unwrap();
    root.expose_input(
        "in",
        component.input::<i64>("in").unwrap(),
        NonZeroUsize::new(1).unwrap(),
    )
    .unwrap();
    root.expose_output("out", component.output::<i64>("out").unwrap())
        .unwrap();
    root.claim(&contract()).unwrap();
    root.require_contract("", &contract()).unwrap();
    let compiled = root.build().unwrap().compile().unwrap();
    for path in ["", "module", "module/leaf"] {
        let snapshot = compiled.inspect(path).unwrap();
        assert_eq!(snapshot.interface, io());
        assert_eq!(snapshot.claims, [contract()]);
        compiled.check_implementation(path, &contract()).unwrap();
        let bound = compiled.bind(path, &contract()).unwrap();
        assert_eq!(
            bound
                .input::<i64>("in")
                .unwrap()
                .resolve(&compiled)
                .unwrap(),
            [(0, "in".into())]
        );
        assert_eq!(
            bound
                .output::<i64>("out")
                .unwrap()
                .resolve(&compiled)
                .unwrap(),
            (0, "out")
        );
    }
    assert_eq!(compiled.inspect("module").unwrap().physical_index, None);
    assert_eq!(
        compiled.inspect("module/leaf").unwrap().physical_index,
        Some(0)
    );
    assert!(compiled.inspect("module/missing").is_err());
}

#[test]
fn detached_snapshots_cannot_change_authoritative_claims() {
    let mut builder = NetworkBuilder::new();
    let handle = builder.add("worker", leaf(false)).unwrap();
    let mut snapshot = builder.inspect(&handle).unwrap();
    snapshot.claims.push(contract());
    assert!(builder.check_implementation(&handle, &contract()).is_err());
    let compiled = builder.build().unwrap().compile().unwrap();
    let mut snapshot = compiled.inspect("worker").unwrap();
    snapshot.claims.push(contract());
    assert!(
        compiled
            .check_implementation("worker", &contract())
            .is_err()
    );
    assert!(compiled.bind("worker", &contract()).is_err());
}

#[test]
fn restricted_builder_binding_uses_actual_scope_revision_and_ports() {
    let promised = Contract::new(
        "test/output",
        Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap()),
    )
    .unwrap();
    let definition = Arc::new(
        ComponentDef::primitive(promised.required().clone(), factory())
            .claim(&promised)
            .unwrap(),
    );
    let mut builder = NetworkBuilder::new();
    let handle = builder.add("worker", definition.clone()).unwrap();
    let bound = builder.bind(&handle, &promised).unwrap();
    assert!(matches!(
        bound.input::<i64>("in"),
        Err(NetworkError::UnknownInput(_))
    ));
    assert!(bound.output::<bool>("out").is_err());
    let endpoint = bound.output::<i64>("out").unwrap();
    let mut other = NetworkBuilder::new();
    other.add("worker", definition.clone()).unwrap();
    assert!(matches!(
        other.bind(&bound, &promised),
        Err(NetworkError::ForeignScope)
    ));
    assert!(matches!(
        other.expose_output("out", endpoint.clone()),
        Err(NetworkError::ForeignScope)
    ));
    builder.replace(&handle, definition).unwrap();
    assert!(matches!(
        bound.output::<i64>("out"),
        Err(NetworkError::StaleHandle)
    ));
    assert!(matches!(
        builder.expose_output("out", endpoint),
        Err(NetworkError::StaleHandle)
    ));
}

#[test]
fn compiled_binding_restricts_ports_types_and_generation() {
    let promised = Contract::new(
        "test/output",
        Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap()),
    )
    .unwrap();
    let mut builder = NetworkBuilder::new();
    builder
        .add(
            "worker",
            Arc::new(
                ComponentDef::primitive(promised.required().clone(), factory())
                    .claim(&promised)
                    .unwrap(),
            ),
        )
        .unwrap();
    let network = builder.build().unwrap();
    let a = network.compile().unwrap();
    let b = network.compile().unwrap();
    let bound = a.bind("worker", &promised).unwrap();
    assert!(bound.input::<i64>("in").is_err());
    assert!(bound.output::<bool>("out").is_err());
    let output = bound.output::<i64>("out").unwrap();
    assert_eq!(output.resolve(&a).unwrap(), (0, "out"));
    assert!(matches!(
        output.resolve(&b),
        Err(NetworkError::ForeignScope)
    ));
}

#[test]
fn all_nested_claims_and_registered_definitions_share_one_namespace() {
    let conflict = Contract::new(contract().id(), Arc::new(Interface::default())).unwrap();
    let mut root = NetworkBuilder::new();
    root.module("nested", module()).unwrap();
    let compiled = root.build().unwrap().compile().unwrap();
    compiled.check_contract_catalog([&contract()]).unwrap();
    assert!(compiled.check_contract_catalog([&conflict]).is_err());
    let mut root = NetworkBuilder::new();
    root.module("nested", module()).unwrap();
    root.add(
        "contradiction",
        Arc::new(
            ComponentDef::primitive(Arc::new(Interface::default()), factory())
                .claim(&conflict)
                .unwrap(),
        ),
    )
    .unwrap();
    assert!(root.build().unwrap().compile().is_err());
    let mut root = NetworkBuilder::new();
    root.require_contract("absent", &conflict).unwrap();
    root.module("nested", module()).unwrap();
    assert!(root.build().unwrap().compile().is_err());
}

#[test]
fn final_requirements_allow_late_authoring_and_reject_missing_claims() {
    let mut builder = NetworkBuilder::new();
    builder.require_contract("late", &contract()).unwrap();
    builder.add("late", leaf(true)).unwrap();
    builder.build().unwrap().compile().unwrap();
    for path in ["", "missing", "worker"] {
        let mut builder = NetworkBuilder::new();
        builder.require_contract(path, &contract()).unwrap();
        builder.add("worker", leaf(false)).unwrap();
        assert!(builder.build().unwrap().compile().is_err());
    }
}

#[test]
fn replacement_preserves_claims_and_removed_private_requirements() {
    let mut builder = NetworkBuilder::new();
    let handle = builder.add("worker", leaf(true)).unwrap();
    assert!(builder.replace(&handle, leaf(false)).is_err());
    builder.check_implementation(&handle, &contract()).unwrap();

    let mut builder = NetworkBuilder::new();
    let handle = builder.module("module", module()).unwrap();
    // Identical external shape and explicit public claim are not sufficient:
    // replacing the composite would erase its private leaf requirement.
    assert!(builder.replace(&handle, leaf(true)).is_err());
    builder.build().unwrap().compile().unwrap();
}

#[test]
fn physical_lookup_uses_stable_declaration_keys_without_spawning_factories() {
    let spawned = Arc::new(AtomicUsize::new(0));
    let count = spawned.clone();
    let definition = Arc::new(ComponentDef::primitive(
        io(),
        Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            Noop
        }),
    ));
    let key = definition.definition_key();
    assert_eq!(
        key,
        definition
            .as_ref()
            .clone()
            .with_origin(GraphOrigin::default())
            .definition_key()
    );
    let other = ComponentDef::primitive(io(), factory());
    assert_ne!(key, other.definition_key());
    let mut child = NetworkBuilder::new();
    child.add("physical", definition.clone()).unwrap();
    let child = child.build().unwrap();
    let mut root = NetworkBuilder::new();
    root.module("nested", child.clone()).unwrap();
    let compiled = root.build().unwrap().compile().unwrap();
    let found = compiled.lookup_definition(key).unwrap();
    assert_eq!(found.path, "nested/physical");
    assert_eq!(found.index, 0);
    assert!(matches!(
        compiled.lookup_definition(other.definition_key()),
        Err(NetworkError::UnknownDefinition)
    ));
    let mut root = NetworkBuilder::new();
    root.module("a", child.clone()).unwrap();
    root.module("b", child).unwrap();
    assert!(matches!(
        root.build()
            .unwrap()
            .compile()
            .unwrap()
            .lookup_definition(key),
        Err(NetworkError::AmbiguousDefinition)
    ));
    assert_eq!(spawned.load(Ordering::SeqCst), 0);
}

#[test]
fn exact_origins_and_nearest_inheritance_survive_nested_inspection() {
    let parent = GraphOrigin::default();
    let override_origin = parent.child();
    let mut child = NetworkBuilder::new();
    child.add("inherited", leaf(true)).unwrap();
    child
        .add(
            "explicit",
            Arc::new(
                leaf(true)
                    .as_ref()
                    .clone()
                    .with_origin(override_origin.clone()),
            ),
        )
        .unwrap();
    let mut root = NetworkBuilder::new().with_origin(GraphOrigin::default());
    root.add(
        "nested",
        Arc::new(ComponentDef::composite(child.build().unwrap()).with_origin(parent.clone())),
    )
    .unwrap();
    let compiled = root.build().unwrap().compile().unwrap();
    assert!(compiled.inspect("").unwrap().origin.is_some());
    assert_eq!(
        compiled.inspect("nested").unwrap().origin,
        Some(parent.clone())
    );
    let inherited = compiled.inspect("nested/inherited").unwrap();
    assert_eq!(inherited.origin, Some(parent));
    assert_eq!(inherited.declared_origin, None);
    assert_eq!(
        compiled.inspect("nested/explicit").unwrap().origin,
        Some(override_origin.clone())
    );
    assert_eq!(
        compiled
            .processes()
            .iter()
            .find(|p| p.path() == "nested/explicit")
            .unwrap()
            .origin(),
        Some(&override_origin)
    );
}

struct Wire;
impl ContractValue for Wire {
    fn schema() -> Schema {
        Schema {
            name: "contract.packet".into(),
            revision: 1,
            structure: "unit".into(),
            codec: "unit-v1".into(),
        }
    }
    fn encode(&self) -> Vec<u8> {
        vec![]
    }
    fn decode(_: &[u8]) -> Result<Self, String> {
        Ok(Self)
    }
}

#[test]
fn wire_bindings_use_complete_schema_and_never_local_type_identity() {
    let interface = Arc::new(
        Interface::new(
            [PortDef::wire::<Wire>("in").unwrap()],
            [PortDef::wire::<Wire>("out").unwrap()],
        )
        .unwrap(),
    );
    let contract = Contract::new("test/wire", interface.clone()).unwrap();
    let mut builder = NetworkBuilder::new();
    let handle = builder
        .add(
            "wire",
            Arc::new(
                ComponentDef::primitive(interface, factory())
                    .claim(&contract)
                    .unwrap(),
            ),
        )
        .unwrap();
    let bound = builder.bind(&handle, &contract).unwrap();
    assert!(bound.input::<Wire>("in").is_err());
    assert!(bound.output::<Wire>("out").is_err());
    assert!(bound.wire_input::<Wire>("in").is_ok());
    assert!(bound.wire_output::<Wire>("out").is_ok());
    let compiled = builder.build().unwrap().compile().unwrap();
    let bound = compiled.bind("wire", &contract).unwrap();
    assert!(bound.input::<Wire>("in").is_err());
    assert!(bound.output::<Wire>("out").is_err());
    assert_eq!(
        bound
            .wire_input::<Wire>("in")
            .unwrap()
            .resolve(&compiled)
            .unwrap(),
        [(0, "in".into())]
    );
    assert_eq!(
        bound
            .wire_output::<Wire>("out")
            .unwrap()
            .resolve(&compiled)
            .unwrap(),
        (0, "out")
    );
    let mut changed = Wire::schema();
    changed.revision += 1;
    let conflict = Contract::new(
        "test/wire",
        Arc::new(
            Interface::new(
                [PortDef::portable("in", &changed).unwrap()],
                [PortDef::wire::<Wire>("out").unwrap()],
            )
            .unwrap(),
        ),
    )
    .unwrap();
    assert!(contract.check_shape(conflict.required()).is_err());
    assert!(compiled.check_contract_catalog([&conflict]).is_err());
}

#[test]
fn frozen_base_obligations_survive_composer_replacement_and_rollback() {
    use phenix_graph::compose::{Composer, Edit, Footprint, NetworkPatch, Registration};
    let mut builder = NetworkBuilder::new();
    builder.module("module", module()).unwrap();
    let base = builder.build().unwrap();
    let mut assembly = Composer::new(base).unwrap();
    let registration = Registration::new(
        "replace",
        [],
        0,
        GraphOrigin::default(),
        [Footprint::Component("module".into())],
        NetworkPatch::new([Edit::Replace {
            path: "module".into(),
            definition: leaf(true),
        }]),
    );
    assert!(assembly.register([registration]).is_err());
    assert!(assembly.current().compiled().inspect("module/leaf").is_ok());
}
