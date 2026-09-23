use super::*;

fn text(value: &str) -> ContractText {
    ContractText::parse(value).unwrap()
}

fn component_id(value: &str) -> DelegationComponentId {
    DelegationComponentId::parse(value).unwrap()
}

fn bridge_component() -> DelegationComponent {
    DelegationComponent {
        id: component_id("bridge"),
        responsibility: text("Translate runtime requests into backend calls"),
        fixed: vec![
            FixedDesign::Provides(DelegationInterface {
                id: DelegationInterfaceId::parse("runtime-api").unwrap(),
                contract: text("Submit requests and return typed outcomes"),
            }),
            FixedDesign::Owns(DelegationElement {
                id: DelegationElementId::parse("request-map").unwrap(),
                description: text("In-flight request ownership"),
            }),
            FixedDesign::DependsOn(component_id("backend")),
            FixedDesign::Invariant(text("A request has exactly one terminal outcome")),
        ],
        implementation: vec![ImplementationRequirement {
            target: ImplementationTarget::Interface(
                DelegationInterfaceId::parse("runtime-api").unwrap(),
            ),
            requirement: text("Implement cancellation propagation"),
        }],
        may_change: vec![ChangePermission {
            scope: ChangeScope::Internals,
            allowance: text("Internal queue and helper types may change"),
        }],
    }
}

fn backend_component() -> DelegationComponent {
    DelegationComponent {
        id: component_id("backend"),
        responsibility: text("Execute provider requests"),
        fixed: vec![],
        implementation: vec![],
        may_change: vec![],
    }
}

fn acceptance() -> Vec<AcceptanceCriterion> {
    vec![AcceptanceCriterion::Command {
        name: text("domain tests"),
        program: text("cargo"),
        arguments: vec!["test".into(), "-p".into(), "phenix-domain".into()],
    }]
}

#[test]
fn valid_contract_keeps_design_and_worker_freedom_separate() {
    let contract = DelegationContract::new(
        text("Add cancellable bridge execution"),
        [bridge_component(), backend_component()],
        acceptance(),
        vec![EscalationCondition {
            scope: EscalationScope::Component(component_id("bridge")),
            condition: text("The runtime API must change"),
        }],
    )
    .unwrap();

    let bridge = &contract.components()[&component_id("bridge")];
    assert_eq!(bridge.implementation.len(), 1);
    assert_eq!(bridge.may_change.len(), 1);
    assert_eq!(contract.acceptance().len(), 1);
    assert_eq!(contract.escalation().len(), 1);
}

#[test]
fn contract_rejects_unknown_component_relationships() {
    let error = DelegationContract::new(
        text("Broken graph"),
        [bridge_component()],
        acceptance(),
        vec![],
    )
    .unwrap_err();

    assert_eq!(
        error,
        DelegationContractError::UnknownRelatedComponent {
            source: component_id("bridge"),
            target: component_id("backend"),
        }
    );
}

#[test]
fn contract_rejects_directives_for_undeclared_interfaces() {
    let mut component = backend_component();
    component.implementation.push(ImplementationRequirement {
        target: ImplementationTarget::Interface(
            DelegationInterfaceId::parse("missing-api").unwrap(),
        ),
        requirement: text("Implement it"),
    });

    let error = DelegationContract::new(text("Broken target"), [component], acceptance(), vec![])
        .unwrap_err();

    assert_eq!(
        error,
        DelegationContractError::UnknownInterfaceTarget {
            component: component_id("backend"),
            target: DelegationInterfaceId::parse("missing-api").unwrap(),
        }
    );
}

#[test]
fn contract_rejects_duplicate_fixed_interfaces() {
    let mut component = backend_component();
    let interface = DelegationInterface {
        id: DelegationInterfaceId::parse("backend-api").unwrap(),
        contract: text("Execute requests"),
    };
    component.fixed = vec![
        FixedDesign::Provides(interface.clone()),
        FixedDesign::Provides(interface),
    ];

    let error =
        DelegationContract::new(text("Ambiguous design"), [component], acceptance(), vec![])
            .unwrap_err();

    assert_eq!(
        error,
        DelegationContractError::DuplicateInterface {
            component: component_id("backend"),
            interface: DelegationInterfaceId::parse("backend-api").unwrap(),
        }
    );
}

#[test]
fn contract_requires_acceptance_criteria() {
    let error = DelegationContract::new(text("No proof"), [backend_component()], vec![], vec![])
        .unwrap_err();

    assert_eq!(error, DelegationContractError::NoAcceptanceCriteria);
}

#[test]
fn contract_revision_is_stable_across_component_input_order() {
    let left = DelegationContract::new(
        text("Add cancellable bridge execution"),
        [bridge_component(), backend_component()],
        acceptance(),
        vec![],
    )
    .unwrap();
    let right = DelegationContract::new(
        text("Add cancellable bridge execution"),
        [backend_component(), bridge_component()],
        acceptance(),
        vec![],
    )
    .unwrap();

    assert_eq!(left.revision(), right.revision());
    assert!(left.revision().as_ref().starts_with("sha256:"));
}

#[test]
fn contract_revision_changes_with_semantics() {
    let left = DelegationContract::new(
        text("Add cancellable bridge execution"),
        [bridge_component(), backend_component()],
        acceptance(),
        vec![],
    )
    .unwrap();

    let mut changed_backend = backend_component();
    changed_backend.responsibility = text("Execute provider requests with retries");
    let right = DelegationContract::new(
        text("Add cancellable bridge execution"),
        [bridge_component(), changed_backend],
        acceptance(),
        vec![],
    )
    .unwrap();

    assert_ne!(left.revision(), right.revision());
}
