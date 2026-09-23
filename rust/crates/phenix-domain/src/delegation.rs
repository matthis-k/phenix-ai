use phenix_core::ArtifactRevision;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display, Formatter};

domain_id_type!(DelegationComponentId);
domain_id_type!(DelegationInterfaceId);
domain_id_type!(DelegationElementId);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractText(String);

impl ContractText {
    pub fn parse(value: impl Into<String>) -> Result<Self, EmptyContractText> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(EmptyContractText);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ContractText {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Display for ContractText {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmptyContractText;

impl Display for EmptyContractText {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("contract text must not be empty")
    }
}

impl std::error::Error for EmptyContractText {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DelegationInterface {
    pub id: DelegationInterfaceId,
    pub contract: ContractText,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DelegationElement {
    pub id: DelegationElementId,
    pub description: ContractText,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FixedDesign {
    Provides(DelegationInterface),
    Owns(DelegationElement),
    DependsOn(DelegationComponentId),
    Calls(DelegationComponentId),
    Invariant(ContractText),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImplementationTarget {
    Component,
    Interface(DelegationInterfaceId),
    OwnedElement(DelegationElementId),
    Internals,
    Tests,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImplementationRequirement {
    pub target: ImplementationTarget,
    pub requirement: ContractText,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChangeScope {
    OwnedElement(DelegationElementId),
    Internals,
    Tests,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangePermission {
    pub scope: ChangeScope,
    pub allowance: ContractText,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DelegationComponent {
    pub id: DelegationComponentId,
    pub responsibility: ContractText,
    pub fixed: Vec<FixedDesign>,
    pub implementation: Vec<ImplementationRequirement>,
    pub may_change: Vec<ChangePermission>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcceptanceCriterion {
    Behavior(ContractText),
    Command {
        name: ContractText,
        program: ContractText,
        arguments: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EscalationScope {
    Contract,
    Component(DelegationComponentId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscalationCondition {
    pub scope: EscalationScope,
    pub condition: ContractText,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DelegationContract {
    goal: ContractText,
    components: BTreeMap<DelegationComponentId, DelegationComponent>,
    acceptance: Vec<AcceptanceCriterion>,
    escalation: Vec<EscalationCondition>,
}

impl DelegationContract {
    pub fn new(
        goal: ContractText,
        components: impl IntoIterator<Item = DelegationComponent>,
        acceptance: Vec<AcceptanceCriterion>,
        escalation: Vec<EscalationCondition>,
    ) -> Result<Self, DelegationContractError> {
        let mut components_by_id = BTreeMap::new();
        for component in components {
            let id = component.id.clone();
            if components_by_id.insert(id.clone(), component).is_some() {
                return Err(DelegationContractError::DuplicateComponent(id));
            }
        }
        if components_by_id.is_empty() {
            return Err(DelegationContractError::NoComponents);
        }
        if acceptance.is_empty() {
            return Err(DelegationContractError::NoAcceptanceCriteria);
        }

        for component in components_by_id.values() {
            validate_component(component, &components_by_id)?;
        }
        for condition in &escalation {
            if let EscalationScope::Component(component) = &condition.scope {
                if !components_by_id.contains_key(component) {
                    return Err(DelegationContractError::UnknownEscalationComponent(
                        component.clone(),
                    ));
                }
            }
        }

        Ok(Self {
            goal,
            components: components_by_id,
            acceptance,
            escalation,
        })
    }

    /// Deterministic semantic identity for this typed delegation contract.
    ///
    /// This is deliberately not a wire serialization of the contract. It is a
    /// canonical, versioned projection used to bind durable runtime state to the
    /// exact typed design that produced it.
    #[must_use]
    pub fn revision(&self) -> ArtifactRevision {
        let mut canonical = Vec::new();
        write_str(&mut canonical, "phenix.delegation-contract.v1");
        write_text(&mut canonical, &self.goal);
        write_len(&mut canonical, self.components.len());
        for (id, component) in &self.components {
            write_str(&mut canonical, id.as_str());
            write_text(&mut canonical, &component.responsibility);

            write_len(&mut canonical, component.fixed.len());
            for fixed in &component.fixed {
                match fixed {
                    FixedDesign::Provides(interface) => {
                        canonical.push(0);
                        write_str(&mut canonical, interface.id.as_str());
                        write_text(&mut canonical, &interface.contract);
                    }
                    FixedDesign::Owns(element) => {
                        canonical.push(1);
                        write_str(&mut canonical, element.id.as_str());
                        write_text(&mut canonical, &element.description);
                    }
                    FixedDesign::DependsOn(target) => {
                        canonical.push(2);
                        write_str(&mut canonical, target.as_str());
                    }
                    FixedDesign::Calls(target) => {
                        canonical.push(3);
                        write_str(&mut canonical, target.as_str());
                    }
                    FixedDesign::Invariant(invariant) => {
                        canonical.push(4);
                        write_text(&mut canonical, invariant);
                    }
                }
            }

            write_len(&mut canonical, component.implementation.len());
            for requirement in &component.implementation {
                match &requirement.target {
                    ImplementationTarget::Component => canonical.push(0),
                    ImplementationTarget::Interface(target) => {
                        canonical.push(1);
                        write_str(&mut canonical, target.as_str());
                    }
                    ImplementationTarget::OwnedElement(target) => {
                        canonical.push(2);
                        write_str(&mut canonical, target.as_str());
                    }
                    ImplementationTarget::Internals => canonical.push(3),
                    ImplementationTarget::Tests => canonical.push(4),
                }
                write_text(&mut canonical, &requirement.requirement);
            }

            write_len(&mut canonical, component.may_change.len());
            for permission in &component.may_change {
                match &permission.scope {
                    ChangeScope::OwnedElement(target) => {
                        canonical.push(0);
                        write_str(&mut canonical, target.as_str());
                    }
                    ChangeScope::Internals => canonical.push(1),
                    ChangeScope::Tests => canonical.push(2),
                }
                write_text(&mut canonical, &permission.allowance);
            }
        }

        write_len(&mut canonical, self.acceptance.len());
        for criterion in &self.acceptance {
            match criterion {
                AcceptanceCriterion::Behavior(behavior) => {
                    canonical.push(0);
                    write_text(&mut canonical, behavior);
                }
                AcceptanceCriterion::Command {
                    name,
                    program,
                    arguments,
                } => {
                    canonical.push(1);
                    write_text(&mut canonical, name);
                    write_text(&mut canonical, program);
                    write_len(&mut canonical, arguments.len());
                    for argument in arguments {
                        write_str(&mut canonical, argument);
                    }
                }
            }
        }

        write_len(&mut canonical, self.escalation.len());
        for condition in &self.escalation {
            match &condition.scope {
                EscalationScope::Contract => canonical.push(0),
                EscalationScope::Component(component) => {
                    canonical.push(1);
                    write_str(&mut canonical, component.as_str());
                }
            }
            write_text(&mut canonical, &condition.condition);
        }

        ArtifactRevision::from_content(&canonical)
    }

    #[must_use]
    pub fn goal(&self) -> &ContractText {
        &self.goal
    }

    #[must_use]
    pub fn components(&self) -> &BTreeMap<DelegationComponentId, DelegationComponent> {
        &self.components
    }

    #[must_use]
    pub fn acceptance(&self) -> &[AcceptanceCriterion] {
        &self.acceptance
    }

    #[must_use]
    pub fn escalation(&self) -> &[EscalationCondition] {
        &self.escalation
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DelegationContractError {
    NoComponents,
    NoAcceptanceCriteria,
    DuplicateComponent(DelegationComponentId),
    DuplicateInterface {
        component: DelegationComponentId,
        interface: DelegationInterfaceId,
    },
    DuplicateOwnedElement {
        component: DelegationComponentId,
        element: DelegationElementId,
    },
    UnknownRelatedComponent {
        source: DelegationComponentId,
        target: DelegationComponentId,
    },
    UnknownInterfaceTarget {
        component: DelegationComponentId,
        target: DelegationInterfaceId,
    },
    UnknownOwnedElementTarget {
        component: DelegationComponentId,
        target: DelegationElementId,
    },
    UnknownEscalationComponent(DelegationComponentId),
}

impl Display for DelegationContractError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoComponents => {
                f.write_str("delegation contract requires at least one component")
            }
            Self::NoAcceptanceCriteria => {
                f.write_str("delegation contract requires at least one acceptance criterion")
            }
            Self::DuplicateComponent(id) => write!(f, "duplicate delegation component: {id}"),
            Self::DuplicateInterface {
                component,
                interface,
            } => write!(
                f,
                "component {component} declares interface {interface} twice"
            ),
            Self::DuplicateOwnedElement { component, element } => {
                write!(f, "component {component} owns element {element} twice")
            }
            Self::UnknownRelatedComponent { source, target } => {
                write!(
                    f,
                    "delegation component {source} references unknown component {target}"
                )
            }
            Self::UnknownInterfaceTarget { component, target } => {
                write!(
                    f,
                    "delegation component {component} references unknown interface {target}"
                )
            }
            Self::UnknownOwnedElementTarget { component, target } => write!(
                f,
                "delegation component {component} references unknown owned element {target}"
            ),
            Self::UnknownEscalationComponent(component) => {
                write!(f, "escalation references unknown component {component}")
            }
        }
    }
}

impl std::error::Error for DelegationContractError {}

fn validate_component(
    component: &DelegationComponent,
    components: &BTreeMap<DelegationComponentId, DelegationComponent>,
) -> Result<(), DelegationContractError> {
    let mut interfaces = BTreeSet::new();
    let mut owned_elements = BTreeSet::new();

    for design in &component.fixed {
        match design {
            FixedDesign::Provides(interface) => {
                if !interfaces.insert(interface.id.clone()) {
                    return Err(DelegationContractError::DuplicateInterface {
                        component: component.id.clone(),
                        interface: interface.id.clone(),
                    });
                }
            }
            FixedDesign::Owns(element) => {
                if !owned_elements.insert(element.id.clone()) {
                    return Err(DelegationContractError::DuplicateOwnedElement {
                        component: component.id.clone(),
                        element: element.id.clone(),
                    });
                }
            }
            FixedDesign::DependsOn(target) | FixedDesign::Calls(target) => {
                if !components.contains_key(target) {
                    return Err(DelegationContractError::UnknownRelatedComponent {
                        source: component.id.clone(),
                        target: target.clone(),
                    });
                }
            }
            FixedDesign::Invariant(_) => {}
        }
    }

    for requirement in &component.implementation {
        match &requirement.target {
            ImplementationTarget::Interface(target) if !interfaces.contains(target) => {
                return Err(DelegationContractError::UnknownInterfaceTarget {
                    component: component.id.clone(),
                    target: target.clone(),
                });
            }
            ImplementationTarget::OwnedElement(target) if !owned_elements.contains(target) => {
                return Err(DelegationContractError::UnknownOwnedElementTarget {
                    component: component.id.clone(),
                    target: target.clone(),
                });
            }
            ImplementationTarget::Component
            | ImplementationTarget::Interface(_)
            | ImplementationTarget::OwnedElement(_)
            | ImplementationTarget::Internals
            | ImplementationTarget::Tests => {}
        }
    }

    for permission in &component.may_change {
        if let ChangeScope::OwnedElement(target) = &permission.scope {
            if !owned_elements.contains(target) {
                return Err(DelegationContractError::UnknownOwnedElementTarget {
                    component: component.id.clone(),
                    target: target.clone(),
                });
            }
        }
    }

    Ok(())
}

fn write_len(output: &mut Vec<u8>, len: usize) {
    let len = u64::try_from(len).expect("delegation contract collection length fits u64");
    output.extend_from_slice(&len.to_be_bytes());
}

fn write_str(output: &mut Vec<u8>, value: &str) {
    write_len(output, value.len());
    output.extend_from_slice(value.as_bytes());
}

fn write_text(output: &mut Vec<u8>, value: &ContractText) {
    write_str(output, value.as_str());
}

#[cfg(test)]
mod tests;
