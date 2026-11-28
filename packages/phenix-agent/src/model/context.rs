//! Provider-facing context, assembled by the trusted host from selected
//! contributions. It contains descriptions and instructions, not graph handles,
//! credentials, filesystem capabilities or unselected plugin contributions.

use crate::tools::ToolDescriptor;
use phenix_kernel::Key;
use phenix_plugin::Contribution;
use std::collections::BTreeMap;

pub type ToolCatalog = BTreeMap<String, BTreeMap<String, ToolDescriptor>>;

/// A model-visible skill declaration. Loading a plugin does not automatically
/// expose its instructions: the host chooses the contribution by identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub id: String,
    pub description: String,
    pub instructions: String,
}

impl Contribution for Skill {
    const KIND: &'static str = "agent.skill";

    fn id(&self) -> &str {
        &self.id
    }
}

/// A generation-local view. Empty environment entries are meaningful.
/// Future conversation/history assembly belongs at the model boundary,
/// not in the graph or plugin registries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelContext {
    pub tools: ToolCatalog,
    pub skills: BTreeMap<Key, Skill>,
}
