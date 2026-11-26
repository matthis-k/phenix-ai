//! Integration of plugin revision leases with graph-owned contracts.
use phenix_graph::{GraphOrigin, contract::Contract, network::CompiledNetwork};
use phenix_plugin::{Code, Contribution, ContributionHandle, Failure, PluginSnapshot};

/// Framework binding failures, without exposing graph-origin authority tokens.
#[derive(Debug, PartialEq, Eq)]
pub enum BindingFault {
    MissingComponentLocator {
        plugin: String,
        revision: u64,
        kind: &'static str,
        contribution: String,
    },
    GenerationOriginMismatch {
        plugin: String,
        revision: u64,
        contribution: String,
        path: String,
        has_origin: bool,
    },
}
impl std::fmt::Display for BindingFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for BindingFault {}

/// Resolve a contribution and check its actual component in a compiled graph.
/// This checks revision ancestry, not exact producer ownership. Kernel composition
/// separately enforces exact producer ownership for registered offers.
pub fn bind_component<C: Contribution>(
    snapshot: &PluginSnapshot,
    id: &str,
    locator: impl FnOnce(&C) -> Option<&str>,
    requested: &Contract,
    graph: &CompiledNetwork,
) -> Result<ContributionHandle<C>, Failure> {
    snapshot.bind::<C>(id, |handle| {
        let path = locator(handle).ok_or_else(|| {
            Failure::with_source(
                Code::MissingBinding,
                "contribution has no component locator",
                Box::new(BindingFault::MissingComponentLocator {
                    plugin: snapshot.name().into(),
                    revision: snapshot.revision(),
                    kind: C::KIND,
                    contribution: id.into(),
                }),
            )
        })?;
        let unit = graph.inspect(path).map_err(|cause| {
            Failure::with_source(
                Code::MissingBinding,
                format!(
                    "compiled graph unit not found: {path}, contract {}",
                    requested.id()
                ),
                Box::new(cause),
            )
        })?;
        if !unit
            .origin
            .as_ref()
            .is_some_and(|origin| {
                origin.belongs_to(&GraphOrigin::from_root(snapshot.revision_identity().clone()))
            })
        {
            return Err(Failure::with_source(
                Code::Denied,
                format!("compiled unit {path}: contribution generation origin mismatch"),
                Box::new(BindingFault::GenerationOriginMismatch {
                    plugin: snapshot.name().into(),
                    revision: snapshot.revision(),
                    contribution: id.into(),
                    path: path.into(),
                    has_origin: unit.origin.is_some(),
                }),
            ));
        }
        graph
            .check_implementation(path, requested)
            .map_err(|error| {
                Failure::with_source(
                    Code::SchemaRejected,
                    format!(
                        "compiled unit {path}, contribution {id}, contract {}",
                        requested.id()
                    ),
                    Box::new(error),
                )
            })?;
        Ok(())
    })
}
