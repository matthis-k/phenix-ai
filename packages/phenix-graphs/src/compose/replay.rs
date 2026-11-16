//! Replay a selected attempt without publishing registry or topology state.

use super::planning::SelectedPlan;
use super::{ComposeError, ConnectionKey, Edit, Footprint, Registration};
use crate::component::{Body, ComponentDef};
use crate::network::{
    ConnectionDef, InputProjection, NetworkDef, NetworkError, PortPath,
    collect_component_obligations,
};
use crate::{GraphOrigin, Interface};
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::Arc;

pub(super) fn replay_selected(
    base: &NetworkDef,
    registrations: &BTreeMap<String, Registration>,
    plan: &SelectedPlan,
) -> Result<NetworkDef, ComposeError> {
    let SelectedPlan { order, active, .. } = plan;
    let mut network = base.clone();
    let mut splices = BTreeMap::new();
    for id in order.iter().filter(|id| active.contains(*id)) {
        let registration = &registrations[id];
        for edit in registration.patch.edits() {
            replay(&mut network, edit, registration, &mut splices)?;
        }
    }
    Ok(network)
}

fn replay(
    network: &mut NetworkDef,
    edit: &Edit,
    registration: &Registration,
    splices: &mut BTreeMap<ConnectionKey, (PortPath, NonZeroUsize)>,
) -> Result<(), ComposeError> {
    let invalid = || ComposeError::InvalidEdit(format!("{}: {edit:?}", registration.id));
    match edit {
        Edit::Add { path, definition } | Edit::Replace { path, definition } => {
            if matches!(edit, Edit::Replace { .. }) && path.contains('/') {
                replace_nested(network, path, definition, &registration.origin)?;
                let root = path.split('/').next().unwrap();
                collect_component_obligations(
                    &network.instances[root],
                    root,
                    &mut network.requirements,
                );
                return Ok(());
            }
            if matches!(edit, Edit::Add { .. }) {
                if network.instances.contains_key(path) {
                    return Err(invalid());
                }
            } else {
                let old = network.instances.get(path).ok_or_else(invalid)?;
                if old.interface() != definition.interface() {
                    return Err(NetworkError::InterfaceMismatch(path.clone()).into());
                }
            }
            network.instances.insert(
                path.clone(),
                Arc::new(
                    (**definition)
                        .clone()
                        .with_origin(registration.origin.clone()),
                ),
            );
            collect_component_obligations(
                &network.instances[path],
                path,
                &mut network.requirements,
            );
        }
        Edit::Connect(edge) => {
            if network
                .connections
                .iter()
                .any(|e| e.from == edge.from && e.to == edge.to)
            {
                return Err(invalid());
            }
            network.connections.push(edge.clone());
        }
        Edit::Disconnect(key) => {
            let index = network
                .connections
                .iter()
                .position(|e| e.from == key.from && e.to == key.to)
                .ok_or_else(invalid)?;
            network.connections.remove(index);
        }
        Edit::ExposeInput {
            name,
            targets,
            capacity,
        } => {
            if network.inputs.contains_key(name) {
                return Err(invalid());
            }
            let target = targets.first().ok_or_else(invalid)?;
            let mut port = network
                .instances
                .get(&target.process)
                .and_then(|c| c.interface().input(&target.port))
                .ok_or_else(invalid)?
                .clone();
            port.name = name.clone();
            network.interface = Arc::new(
                Interface::new(
                    network.interface.inputs().values().cloned().chain([port]),
                    network.interface.outputs().values().cloned(),
                )
                .map_err(NetworkError::from)?,
            );
            network.inputs.insert(
                name.clone(),
                InputProjection {
                    targets: targets.clone(),
                    capacity: *capacity,
                },
            );
        }
        Edit::ExposeOutput { name, from } => {
            if network.outputs.contains_key(name) {
                return Err(invalid());
            }
            let mut port = network
                .instances
                .get(&from.process)
                .and_then(|c| c.interface().output(&from.port))
                .ok_or_else(invalid)?
                .clone();
            port.name = name.clone();
            network.interface = Arc::new(
                Interface::new(
                    network.interface.inputs().values().cloned(),
                    network.interface.outputs().values().cloned().chain([port]),
                )
                .map_err(NetworkError::from)?,
            );
            network.outputs.insert(name.clone(), from.clone());
        }
        Edit::AddSource(path) => {
            if network.sources.contains(path) {
                return Err(invalid());
            }
            network.sources.push(path.clone());
        }
        Edit::Splice {
            anchor,
            input,
            output,
        } => {
            let predecessor = splices
                .get(anchor)
                .map_or(&anchor.from, |(tail, _)| tail)
                .clone();
            let index = network
                .connections
                .iter()
                .position(|e| e.from == predecessor && e.to == anchor.to)
                .ok_or_else(invalid)?;
            let capacity = network.connections[index].capacity;
            for key in [
                ConnectionKey::new(predecessor.clone(), input.clone()),
                ConnectionKey::new(output.clone(), anchor.to.clone()),
            ] {
                let footprint = Footprint::Connection(key);
                if !registration.grants.contains(&footprint) {
                    return Err(ComposeError::Unauthorized {
                        registration: registration.id.clone(),
                        footprint,
                    });
                }
            }
            network.connections.remove(index);
            network.connections.push(ConnectionDef {
                from: predecessor,
                to: input.clone(),
                capacity,
            });
            network.connections.push(ConnectionDef {
                from: output.clone(),
                to: anchor.to.clone(),
                capacity,
            });
            splices.insert(anchor.clone(), (output.clone(), capacity));
        }
    }
    Ok(())
}

/// Copy-on-write replacement through explicit *immediate-child* seams. A
/// root-level host grant never bypasses a composite author's private boundary.
fn replace_nested(
    network: &mut NetworkDef,
    path: &str,
    definition: &Arc<ComponentDef>,
    origin: &GraphOrigin,
) -> Result<(), ComposeError> {
    let (parent, remainder) = path
        .split_once('/')
        .ok_or_else(|| ComposeError::PrivateEditScope(path.into()))?;
    let ancestor = network
        .instances
        .get(parent)
        .ok_or_else(|| ComposeError::InvalidEdit(format!("unknown composite {parent}")))?
        .clone();
    let Body::Composite(child) = ancestor.body() else {
        return Err(ComposeError::PrivateEditScope(path.into()));
    };
    let immediate = remainder.split('/').next().unwrap();
    if !child.replacement_points.contains(immediate) {
        return Err(ComposeError::PrivateEditScope(path.into()));
    }
    let mut updated = (**child).clone();
    if remainder.contains('/') {
        replace_nested(&mut updated, remainder, definition, origin)?;
    } else {
        let previous = updated
            .instances
            .get(remainder)
            .ok_or_else(|| ComposeError::InvalidEdit(format!("unknown nested component {path}")))?;
        if previous.interface() != definition.interface() {
            return Err(NetworkError::InterfaceMismatch(path.into()).into());
        }
        updated.instances.insert(
            remainder.into(),
            Arc::new((**definition).clone().with_origin(origin.clone())),
        );
        collect_component_obligations(
            &updated.instances[remainder],
            remainder,
            &mut updated.requirements,
        );
    }
    network.instances.insert(
        parent.into(),
        Arc::new(ancestor.with_updated_network(updated)),
    );
    Ok(())
}
