//! Opt-in external tool contributor proving that the stock Basic tool manager
//! can offer and execute another plugin without editing its builtin table.
use phenix_basic_contracts::{error, BasicError, ErrorCode, ToolInfo, ToolRequest, ToolResult};
use phenix_sdk::{CallError, CallResult, Host, StaticGraphContribution};

/// Event registration is only a hook: the tool manager declares the event
/// and owns the catalog; this plugin owns only the reverse tool's execution.
pub const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "phenix.tool.fixture-reverse",
    events: &[],
    actions: &[],
    nodes: &[phenix_sdk::on_event!(
        "populate",
        phenix_basic_contracts::ToolsPopulate,
        phenix_basic_contracts::ToolPopulate::register,
    )],
};

fn definition() -> ToolInfo {
    ToolInfo {
        id: ID.into(),
        description: "Reverse Unicode scalar values".into(),
        arguments_schema: r#"{"type":"string"}"#.into(),
        idempotent: true,
    }
}

const ID: &str = "fixture.reverse";

#[phenix_sdk::plugin(
    id = "phenix.tool.fixture-reverse",
    version = "1.0.0",
    provide(phenix_basic_contracts::ToolRegistrations, slot = "Registrations"),
    provide(phenix_basic_contracts::ToolPopulate, slot = "Populate"),
    require(phenix_basic_contracts::ToolRegistry, slot = "Registry"),
    graph(super::GRAPH)
)]
mod plugin {
    use super::*;
    use phenix_basic_contracts::{ToolPopulate, ToolRegistrations, Tools, ToolsPopulate};

    #[provide(ToolRegistrations::list)]
    fn list() -> CallResult<Vec<ToolInfo>, BasicError> {
        Ok(vec![definition()])
    }

    #[provide(ToolPopulate::register)]
    fn register(host: &Host<'_>, _: ToolsPopulate) -> CallResult<(), BasicError> {
        Tools::new(host, "Registry").register(definition())
    }

    #[provide(ToolRegistrations::execute)]
    fn execute(host: &Host<'_>, request: ToolRequest) -> CallResult<ToolResult, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        if request.run.is_empty() || request.call.id.is_empty() {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidInput,
                "missing authorized tool run or call ID",
            )));
        }
        if request.call.tool != ID {
            return Err(CallError::Domain(error(
                ErrorCode::NotFound,
                "unknown tool",
            )));
        }
        let input: String = serde_json::from_str(&request.call.arguments).map_err(|_| {
            CallError::Domain(error(ErrorCode::InvalidInput, "expected JSON string"))
        })?;
        Ok(ToolResult {
            text: input.chars().rev().collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_sdk::ContractMetadata;

    #[test]
    fn contributor_has_only_one_typed_registration_contract() {
        let metadata = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert_eq!(metadata.provides.len(), 2);
        assert_eq!(metadata.requires.len(), 1);
        assert_eq!(metadata.requires[0].contract, "phenix.tools.registry@1");
        assert_eq!(GRAPH.nodes[0].triggers, ["phenix.tools.populate"]);
        assert!(GRAPH.events.is_empty());
        assert!(GRAPH.actions.is_empty());
    }
}
