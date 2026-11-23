//! Borrowed declaration validation and frozen obligation normalization.

use super::{NetworkDef, NetworkError, PortPath};
use crate::TypeIdentity;
use crate::component::{Body, ComponentDef};
use crate::contract::{Contract, catalog};
use crate::interface::{Interface, InterfacePhase, PortDef};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(super) fn check_type(
    path: &PortPath,
    expected: &TypeIdentity,
    actual: &TypeIdentity,
) -> Result<(), NetworkError> {
    if expected == actual {
        Ok(())
    } else {
        Err(NetworkError::WrongType {
            path: path.clone(),
            expected: expected.clone(),
            actual: actual.clone(),
        })
    }
}
pub(super) fn validate_process_name(name: &str) -> Result<(), NetworkError> {
    if name.is_empty()
        || name.len() > crate::MAX_IDENTIFIER_BYTES
        || matches!(name, "." | "..")
        || name.contains('/')
        || name.chars().any(char::is_control)
    {
        Err(NetworkError::InvalidName(name.into()))
    } else {
        Ok(())
    }
}
pub(super) fn port<'a>(
    instances: &'a BTreeMap<String, Arc<ComponentDef>>,
    path: &PortPath,
    input: bool,
) -> Result<&'a PortDef, NetworkError> {
    let component = instances
        .get(&path.process)
        .ok_or_else(|| NetworkError::UnknownProcess(path.process.clone()))?;
    if input {
        component
            .interface
            .input(&path.port)
            .ok_or_else(|| NetworkError::UnknownInput(path.clone()))
    } else {
        component
            .interface
            .output(&path.port)
            .ok_or_else(|| NetworkError::UnknownOutput(path.clone()))
    }
}
pub(super) fn validate_source(name: &str, component: &ComponentDef) -> Result<(), NetworkError> {
    if matches!(component.body, Body::Primitive(_)) && !component.interface.inputs().is_empty() {
        Err(NetworkError::InvalidSource(name.into()))
    } else {
        Ok(())
    }
}
pub(super) fn validate_claims(
    interface: &Interface,
    claims: &[Contract],
    path: &str,
) -> Result<(), NetworkError> {
    catalog(claims)?;
    for claim in claims {
        claim
            .check(interface, claims)
            .map_err(|e| e.context(InterfacePhase::Compiled, path, Some(claim.id())))?;
    }
    Ok(())
}
pub(super) fn declaration<'a>(
    component: &'a ComponentDef,
    path: &str,
) -> Result<(&'a Interface, &'a [Contract]), NetworkError> {
    if path.is_empty() {
        return Ok((&component.interface, &component.claims));
    }
    match &component.body {
        Body::Composite(network) => network_declaration(network, path),
        Body::Primitive(_) => Err(NetworkError::UnknownProcess(path.into())),
    }
}
fn network_declaration<'a>(
    network: &'a NetworkDef,
    path: &str,
) -> Result<(&'a Interface, &'a [Contract]), NetworkError> {
    if path.is_empty() {
        return Ok((&network.interface, &network.claims));
    }
    let (first, rest) = path.split_once('/').unwrap_or((path, ""));
    let component = network
        .instances
        .get(first)
        .ok_or_else(|| NetworkError::UnknownProcess(path.into()))?;
    declaration(component, rest)
}
pub(crate) fn collect_component_obligations(
    component: &ComponentDef,
    path: &str,
    result: &mut Vec<(String, Contract)>,
) {
    result.extend(component.claims.iter().cloned().map(|c| (path.into(), c)));
    if let Body::Composite(network) = &component.body {
        result.extend(
            network
                .requirements
                .iter()
                .map(|(p, c)| (qualify(path, p), c.clone())),
        );
    }
}
pub(super) fn qualify(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.into()
    } else if name.is_empty() {
        prefix.into()
    } else {
        format!("{prefix}/{name}")
    }
}
/// Remove identical obligations without masking conflicting contract shapes.
/// Builder round-trips and composer replay may checkpoint the same requirement
/// repeatedly. Keep any same-ID/different-shape entries for compilation to
/// reject at the existing contract-catalog validation boundary.
pub(crate) fn normalize_requirements(requirements: &mut Vec<(String, Contract)>) {
    let mut seen = BTreeMap::new();
    requirements.retain(|(path, contract)| {
        let key = (path.clone(), contract.id().to_owned());
        if let Some(previous) = seen.get(&key) {
            // Retain conflicting definitions so the existing compilation
            // boundary can reject them with its complete contract catalog.
            previous != contract
        } else {
            seen.insert(key, contract.clone());
            true
        }
    });
}

pub(super) fn validate_network(network: &NetworkDef) -> Result<(), NetworkError> {
    validate_claims(&network.interface, &network.claims, "")?;
    for name in &network.replacement_points {
        if !network.instances.contains_key(name) {
            return Err(NetworkError::UnknownProcess(name.clone()));
        }
    }
    for (name, component) in &network.instances {
        validate_process_name(name)?;
        validate_claims(&component.interface, &component.claims, name)?;
        if let Body::Composite(child) = &component.body {
            component
                .interface
                .check_shape(&child.interface)
                .map_err(|e| e.context(InterfacePhase::Compiled, name, None))?;
        }
    }
    let mut edges = BTreeSet::new();
    for edge in &network.connections {
        let from = port(&network.instances, &edge.from, false)?;
        let to = port(&network.instances, &edge.to, true)?;
        check_type(&edge.to, &from.value, &to.value)?;
        if !edges.insert((&edge.from, &edge.to)) {
            return Err(NetworkError::DuplicateConnection {
                from: edge.from.clone(),
                to: edge.to.clone(),
            });
        }
    }
    if !network.interface.inputs().keys().eq(network.inputs.keys())
        || !network
            .interface
            .outputs()
            .keys()
            .eq(network.outputs.keys())
    {
        return Err(NetworkError::IncompletePublicBoundary);
    }
    for (name, projection) in &network.inputs {
        if projection.targets.is_empty() {
            return Err(NetworkError::InvalidProjection(name.clone()));
        }
        let mut targets = BTreeSet::new();
        for path in &projection.targets {
            let actual = port(&network.instances, path, true)?;
            check_type(path, &network.interface.inputs()[name].value, &actual.value)?;
            if !targets.insert(path) {
                return Err(NetworkError::InvalidProjection(name.clone()));
            }
        }
    }
    for (name, path) in &network.outputs {
        let actual = port(&network.instances, path, false)?;
        check_type(
            path,
            &network.interface.outputs()[name].value,
            &actual.value,
        )?;
    }
    let mut sources = BTreeSet::new();
    for name in &network.sources {
        let component = network
            .instances
            .get(name)
            .ok_or_else(|| NetworkError::UnknownProcess(name.clone()))?;
        validate_source(name, component)?;
        if !sources.insert(name) {
            return Err(NetworkError::InvalidSource(name.clone()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interface::InterfaceReason;
    use crate::network::NetworkBuilder;

    #[test]
    fn malformed_composite_body_retains_canonical_compiled_diagnostic() {
        // Public composite constructors derive the interface from the body.
        // Exercise the defensive check with an internally inconsistent payload.
        let mut network = NetworkBuilder::new().build().unwrap();
        let mut component = ComponentDef::composite(network.clone());
        component.interface =
            Arc::new(Interface::new([PortDef::local::<i64>("required")], []).unwrap());
        network
            .instances
            .insert("broken".into(), Arc::new(component));
        let mut path = "broken".to_owned();
        for parent in [None, Some("inner"), Some("outer")] {
            if let Some(parent) = parent {
                let mut builder = NetworkBuilder::new();
                builder.module(parent, network).unwrap();
                network = builder.build().unwrap();
                path = format!("{parent}/{path}");
            }
            let NetworkError::Interface(error) = network.compile().unwrap_err() else {
                panic!("expected canonical interface error");
            };
            assert_eq!(error.phase, InterfacePhase::Compiled);
            assert_eq!(error.path.as_ref(), path);
            assert_eq!(error.endpoint.as_deref(), Some("input required"));
            assert_eq!(*error.reason, InterfaceReason::MissingPort);
            assert_eq!(
                error.expected,
                Some(PortDef::local::<i64>("required").value)
            );
            assert_eq!(error.actual, None);
        }
    }
}
