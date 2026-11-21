//! Context-free edit payloads, footprints, and payload validation.

use super::ComposeError;
use crate::component::{Body, ComponentDef};
use crate::network::{ConnectionDef, NetworkError, PortPath};
use crate::{CompileLimit, GraphOrigin, LimitKind};
use std::collections::BTreeSet;
use std::num::NonZeroUsize;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionKey {
    pub from: PortPath,
    pub to: PortPath,
}
impl ConnectionKey {
    pub fn new(from: PortPath, to: PortPath) -> Self {
        Self { from, to }
    }
}

/// Replacement writes only `Component`, never its incident connections.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Footprint {
    Component(String),
    Connection(ConnectionKey),
    InputExport(String),
    OutputExport(String),
    Source(String),
}

#[derive(Clone, Debug)]
pub enum Edit {
    Add {
        path: String,
        definition: Arc<ComponentDef>,
    },
    Replace {
        path: String,
        definition: Arc<ComponentDef>,
    },
    Connect(ConnectionDef),
    Disconnect(ConnectionKey),
    ExposeInput {
        name: String,
        targets: Vec<PortPath>,
        capacity: NonZeroUsize,
    },
    ExposeOutput {
        name: String,
        from: PortPath,
    },
    /// A port on the exact author-approved nested composite, not a root export.
    ExposeNestedInput {
        parent: String,
        name: String,
        targets: Vec<PortPath>,
        capacity: NonZeroUsize,
    },
    ExposeNestedOutput {
        parent: String,
        name: String,
        from: PortPath,
    },
    AddSource(String),
    /// Insert an existing component between the original edge's endpoints.
    /// Dependent splices append in dependency order, retaining the original key.
    Splice {
        anchor: ConnectionKey,
        input: PortPath,
        output: PortPath,
    },
}
impl Edit {
    /// Import checked declaration boundaries as symbolic authoring data, not
    /// physical projections or authority over the resulting compilation.
    pub fn connect_bound<T>(
        network: &crate::CompiledNetwork,
        from: &crate::BoundOutput<T>,
        to: &crate::BoundInput<T>,
        capacity: NonZeroUsize,
    ) -> Result<Self, NetworkError> {
        from.validate(network)?;
        to.validate(network)?;
        if from.value_type() != to.value_type() {
            return Err(NetworkError::WrongType {
                path: to.symbolic_path(network)?.clone(),
                expected: to.value_type().clone(),
                actual: from.value_type().clone(),
            });
        }
        let from = from.symbolic_path(network)?.clone();
        let to = to.symbolic_path(network)?.clone();
        Ok(Self::Connect(ConnectionDef { from, to, capacity }))
    }

    pub fn expose_bound_input<T>(
        network: &crate::CompiledNetwork,
        name: impl Into<String>,
        targets: &[crate::BoundInput<T>],
        capacity: NonZeroUsize,
    ) -> Result<Self, NetworkError> {
        let targets = targets
            .iter()
            .map(|target| target.symbolic_path(network).cloned())
            .collect::<Result<_, _>>()?;
        Ok(Self::ExposeInput {
            name: name.into(),
            targets,
            capacity,
        })
    }

    pub fn expose_bound_output<T>(
        network: &crate::CompiledNetwork,
        name: impl Into<String>,
        from: &crate::BoundOutput<T>,
    ) -> Result<Self, NetworkError> {
        Ok(Self::ExposeOutput {
            name: name.into(),
            from: from.symbolic_path(network)?.clone(),
        })
    }

    pub(super) fn validate_payload(&self, origin: &GraphOrigin) -> Result<(), ComposeError> {
        match self {
            Self::Add { path, definition } => {
                for part in path.split('/') {
                    public_path(part)?;
                }
                definition_origin(definition, origin, path)?;
            }
            Self::Replace { path, definition } => {
                for part in path.split('/') {
                    public_path(part)?;
                }
                definition_origin(definition, origin, path)?;
            }
            Self::Connect(edge) => {
                for path in [&edge.from.process, &edge.to.process] {
                    for part in path.split('/') {
                        public_path(part)?;
                    }
                }
            }
            Self::Disconnect(key) => {
                public_path(&key.from.process)?;
                public_path(&key.to.process)?;
            }
            Self::ExposeInput { targets, .. } => {
                for target in targets {
                    public_path(&target.process)?;
                }
            }
            Self::ExposeOutput { from, .. } => public_path(&from.process)?,
            Self::ExposeNestedInput {
                parent,
                name,
                targets,
                ..
            } => {
                for part in parent.split('/').chain(std::iter::once(name.as_str())) {
                    public_path(part)?;
                }
                for target in targets {
                    for part in target.process.split('/') {
                        public_path(part)?;
                    }
                }
            }
            Self::ExposeNestedOutput { parent, name, from } => {
                for part in parent
                    .split('/')
                    .chain(std::iter::once(name.as_str()))
                    .chain(from.process.split('/'))
                {
                    public_path(part)?;
                }
            }
            Self::AddSource(path) => public_path(path)?,
            Self::Splice {
                anchor,
                input,
                output,
            } => {
                for path in [&anchor.from, &anchor.to, input, output] {
                    public_path(&path.process)?;
                }
            }
        }
        Ok(())
    }

    pub fn footprints(&self) -> BTreeSet<Footprint> {
        use Footprint::*;
        match self {
            Self::Add { path, .. } | Self::Replace { path, .. } => [Component(path.clone())].into(),
            Self::Connect(edge) => [Connection(ConnectionKey::new(
                edge.from.clone(),
                edge.to.clone(),
            ))]
            .into(),
            Self::Disconnect(key) => [Connection(key.clone())].into(),
            Self::ExposeInput { name, .. } => [InputExport(name.clone())].into(),
            Self::ExposeOutput { name, .. } => [OutputExport(name.clone())].into(),
            Self::ExposeNestedInput { parent, name, .. } => {
                [InputExport(format!("{parent}/{name}"))].into()
            }
            Self::ExposeNestedOutput { parent, name, .. } => {
                [OutputExport(format!("{parent}/{name}"))].into()
            }
            Self::AddSource(path) => [Source(path.clone())].into(),
            // The planner adds the context-dependent predecessor edges.
            Self::Splice { anchor, output, .. } => [
                Connection(anchor.clone()),
                Connection(ConnectionKey::new(output.clone(), anchor.to.clone())),
            ]
            .into(),
        }
    }
}

/// Immutable owned payload. It contains no host authority or replay callbacks.
#[derive(Clone, Debug)]
pub struct NetworkPatch {
    edits: Arc<[Edit]>,
}
impl NetworkPatch {
    pub fn new(edits: impl IntoIterator<Item = Edit>) -> Self {
        Self {
            edits: edits.into_iter().collect(),
        }
    }
    pub fn edits(&self) -> &[Edit] {
        &self.edits
    }
    pub fn footprints(&self) -> BTreeSet<Footprint> {
        self.edits.iter().flat_map(Edit::footprints).collect()
    }
}

fn public_path(path: &str) -> Result<(), ComposeError> {
    if path.is_empty()
        || path.len() > crate::MAX_IDENTIFIER_BYTES
        || path.contains('/')
        || matches!(path, "." | "..")
        || path.chars().any(char::is_control)
    {
        Err(ComposeError::PrivateEditScope(path.into()))
    } else {
        Ok(())
    }
}
fn definition_origin(
    definition: &ComponentDef,
    origin: &GraphOrigin,
    path: &str,
) -> Result<(), ComposeError> {
    let limits = crate::CompileLimits::default();
    let mut remaining = limits.max_instances;
    let mut pending = vec![(definition, path.to_owned(), 0)];
    while let Some((definition, path, depth)) = pending.pop() {
        if remaining == 0 || depth >= limits.max_depth {
            return Err(NetworkError::LimitExceeded {
                limit: if remaining == 0 {
                    CompileLimit {
                        kind: LimitKind::Instances,
                        configured: limits.max_instances,
                        attempted: limits.max_instances.checked_add(1),
                    }
                } else {
                    CompileLimit {
                        kind: LimitKind::Depth,
                        configured: limits.max_depth as u64,
                        attempted: u64::try_from(depth)
                            .ok()
                            .and_then(|depth| depth.checked_add(1)),
                    }
                },
                path,
            }
            .into());
        }
        remaining -= 1;
        if definition.origin().is_some_and(|actual| actual != origin) {
            return Err(ComposeError::ForgedOrigin(path));
        }
        if let Body::Composite(child) = definition.body() {
            if child.instances().len() as u64 > remaining {
                return Err(NetworkError::LimitExceeded {
                    limit: CompileLimit {
                        kind: LimitKind::Instances,
                        configured: limits.max_instances,
                        attempted: u64::try_from(child.instances().len()).ok().and_then(
                            |children| (limits.max_instances - remaining).checked_add(children),
                        ),
                    },
                    path,
                }
                .into());
            }
            for (name, child) in child.instances() {
                pending.push((child, format!("{path}/{name}"), depth + 1));
            }
        }
    }
    Ok(())
}
