//! Provider-facing context, assembled by the trusted host from selected
//! contributions. It contains descriptions and instructions, not graph handles,
//! credentials, filesystem capabilities or unselected plugin contributions.

use crate::{conversation::TurnRecord, tools::ToolDescriptor};
use phenix_graph::Failure;
use phenix_kernel::Key;
use phenix_plugin::Contribution;
use std::{collections::BTreeMap, sync::Arc};

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

/// A model-visible turn snapshot. Empty environment entries are meaningful.
/// Selected metadata and prior history belong at the model boundary,
/// not in the graph or plugin registries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelContext {
    pub tools: ToolCatalog,
    pub skills: BTreeMap<Key, Skill>,
    pub history: Vec<TurnRecord>,
}

/// A model-visible snapshot factory, not an authority to select new tools.
/// Evaluated once per prompt by the ordinary model adapter.
#[derive(Clone)]
pub struct ContextSource(Arc<dyn Fn() -> Result<ModelContext, Failure> + Send + Sync>);

impl ContextSource {
    pub fn new(load: impl Fn() -> Result<ModelContext, Failure> + Send + Sync + 'static) -> Self {
        Self(Arc::new(load))
    }

    pub fn load(&self) -> Result<ModelContext, Failure> {
        (self.0)()
    }
}

impl From<ModelContext> for ContextSource {
    fn from(context: ModelContext) -> Self {
        Self::new(move || Ok(context.clone()))
    }
}
