//! Opaque composition provenance, separate from contracts and authorization.
use crate::*;

/// Allocation identity for one host-selected composition generation.
/// Cloning preserves identity; constructing another token never does. This is
/// provenance supplied by a trusted host, not a sandbox or permission grant.
#[derive(Clone, Debug)]
pub struct GraphOrigin(Arc<()>);

impl Default for GraphOrigin {
    fn default() -> Self {
        Self(Arc::new(()))
    }
}

impl PartialEq for GraphOrigin {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for GraphOrigin {}

impl FindResult<'_> {
    /// The origin of this actual final declaration, not of its contract or path.
    /// Module instances report their instantiation/replacement origin. Composite
    /// roots and low-level declarations without scoped composition have none.
    pub fn origin(&self) -> Option<&GraphOrigin> {
        let (graph, declaration) = match self {
            Self::Node(node) => {
                let provenance = node.graph.metadata.nodes.get(&node.id)?;
                let definition = node
                    .graph
                    .metadata
                    .definitions
                    .get(&provenance.definition)?;
                let template = definition
                    .nodes
                    .iter()
                    .find(|n| n.id == provenance.local_node)?;
                return template.composition_origin.as_ref();
            }
            Self::Module(module) => {
                let instance = module
                    .graph
                    .metadata
                    .instances
                    .get(module.lookup.instance_metadata)?;
                (module.graph, instance.declaration_id?)
            }
        };
        graph.composition_origins.get(&declaration)
    }
}
