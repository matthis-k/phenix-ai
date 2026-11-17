//! Replay a selected attempt without publishing registry or topology state.

use super::planning::SelectedPlan;
use super::{ComposeError, ConnectionKey, Edit, Footprint, InvalidEditReason, Registration};
use crate::Interface;
use crate::component::{Body, ComponentDef};
use crate::network::{
    ConnectionDef, InputProjection, NetworkDef, NetworkError, PortPath,
    collect_component_obligations,
};
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
    let invalid = |reason| ComposeError::InvalidEdit {
        registration: registration.id.clone(),
        reason: Box::new(reason),
    };
    match edit {
        Edit::Add { path, definition } | Edit::Replace { path, definition } => {
            if matches!(edit, Edit::Replace { .. }) && path.contains('/') {
                replace_nested(network, path, definition, registration, "")?;
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
                    return Err(invalid(InvalidEditReason::DuplicateComponent {
                        path: path.clone(),
                    }));
                }
            } else {
                let old = network.instances.get(path).ok_or_else(|| {
                    invalid(InvalidEditReason::MissingReplacement { path: path.clone() })
                })?;
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
                return Err(invalid(InvalidEditReason::DuplicateConnection {
                    connection: ConnectionKey::new(edge.from.clone(), edge.to.clone()),
                }));
            }
            network.connections.push(edge.clone());
        }
        Edit::Disconnect(key) => {
            let index = network
                .connections
                .iter()
                .position(|e| e.from == key.from && e.to == key.to)
                .ok_or_else(|| {
                    invalid(InvalidEditReason::MissingDisconnection {
                        connection: key.clone(),
                    })
                })?;
            network.connections.remove(index);
        }
        Edit::ExposeInput {
            name,
            targets,
            capacity,
        } => {
            if network.inputs.contains_key(name) {
                return Err(invalid(InvalidEditReason::DuplicateInputExport {
                    name: name.clone(),
                }));
            }
            let target = targets.first().ok_or_else(|| {
                invalid(InvalidEditReason::EmptyInputProjection { name: name.clone() })
            })?;
            let component = network.instances.get(&target.process).ok_or_else(|| {
                invalid(InvalidEditReason::MissingInputComponent {
                    name: name.clone(),
                    target: target.clone(),
                })
            })?;
            let mut port = component
                .interface()
                .input(&target.port)
                .ok_or_else(|| {
                    invalid(InvalidEditReason::MissingInputPort {
                        name: name.clone(),
                        target: target.clone(),
                    })
                })?
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
                return Err(invalid(InvalidEditReason::DuplicateOutputExport {
                    name: name.clone(),
                }));
            }
            let component = network.instances.get(&from.process).ok_or_else(|| {
                invalid(InvalidEditReason::MissingOutputComponent {
                    name: name.clone(),
                    target: from.clone(),
                })
            })?;
            let mut port = component
                .interface()
                .output(&from.port)
                .ok_or_else(|| {
                    invalid(InvalidEditReason::MissingOutputPort {
                        name: name.clone(),
                        target: from.clone(),
                    })
                })?
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
                return Err(invalid(InvalidEditReason::DuplicateSource {
                    path: path.clone(),
                }));
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
                .ok_or_else(|| {
                    invalid(InvalidEditReason::MissingSpliceConnection {
                        anchor: anchor.clone(),
                        connection: ConnectionKey::new(predecessor.clone(), anchor.to.clone()),
                    })
                })?;
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
    registration: &Registration,
    prefix: &str,
) -> Result<(), ComposeError> {
    let (parent, remainder) = path
        .split_once('/')
        .ok_or_else(|| ComposeError::PrivateEditScope(path.into()))?;
    let ancestor = network
        .instances
        .get(parent)
        .ok_or_else(|| ComposeError::InvalidEdit {
            registration: registration.id.clone(),
            reason: Box::new(InvalidEditReason::UnknownComposite {
                path: if prefix.is_empty() {
                    parent.to_owned()
                } else {
                    format!("{prefix}/{parent}")
                },
            }),
        })?
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
        let prefix = if prefix.is_empty() {
            parent.to_owned()
        } else {
            format!("{prefix}/{parent}")
        };
        replace_nested(&mut updated, remainder, definition, registration, &prefix)?;
    } else {
        let previous =
            updated
                .instances
                .get(remainder)
                .ok_or_else(|| ComposeError::InvalidEdit {
                    registration: registration.id.clone(),
                    reason: Box::new(InvalidEditReason::UnknownNestedComponent {
                        path: if prefix.is_empty() {
                            path.to_owned()
                        } else {
                            format!("{prefix}/{path}")
                        },
                    }),
                })?;
        if previous.interface() != definition.interface() {
            return Err(NetworkError::InterfaceMismatch(path.into()).into());
        }
        updated.instances.insert(
            remainder.into(),
            Arc::new(
                (**definition)
                    .clone()
                    .with_origin(registration.origin.clone()),
            ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GraphOrigin;
    use crate::compose::{Composer, NetworkPatch};
    use crate::network::NetworkBuilder;

    #[test]
    fn defensive_nested_lookups_keep_registration_and_full_hierarchical_path() {
        // Valid public manifests always name existing children. Exercise both
        // defensive lookups with an internally malformed replacement payload.
        for (path, reason) in [
            (
                "root/inner/missing",
                InvalidEditReason::UnknownNestedComponent {
                    path: "root/inner/missing".into(),
                },
            ),
            (
                "root/inner/missing/leaf",
                InvalidEditReason::UnknownComposite {
                    path: "root/inner/missing".into(),
                },
            ),
        ] {
            let mut malformed = NetworkBuilder::new().build().unwrap();
            malformed.replacement_points.insert("missing".into());
            let mut parent = NetworkBuilder::new();
            let inner = parent
                .add("inner", Arc::new(ComponentDef::composite(malformed)))
                .unwrap();
            parent.allow_replace(&inner).unwrap();
            let patch = NetworkPatch::new([
                Edit::Add {
                    path: "root".into(),
                    definition: Arc::new(ComponentDef::composite(parent.build().unwrap())),
                },
                Edit::Replace {
                    path: path.into(),
                    definition: Arc::new(ComponentDef::composite(
                        NetworkBuilder::new().build().unwrap(),
                    )),
                },
            ]);
            let mut composer = Composer::new(NetworkBuilder::new().build().unwrap()).unwrap();
            assert_eq!(
                composer
                    .register([Registration::new(
                        "nested",
                        [],
                        0,
                        GraphOrigin::default(),
                        patch.footprints(),
                        patch
                    )])
                    .unwrap_err(),
                ComposeError::InvalidEdit {
                    registration: "nested".into(),
                    reason: Box::new(reason),
                }
            );
            assert!(composer.current().network().instances().is_empty());
            assert!(composer.current().compiled().processes().is_empty());
            assert!(composer.current().active().is_empty());
            assert!(composer.revoke("nested").unwrap().is_empty());
        }
    }
}
