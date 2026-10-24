//! Opt-in external tool contributor proving that the stock Basic tool manager
//! can offer and execute another plugin without editing its builtin table.
use phenix_basic_contracts::{error, BasicError, ErrorCode, ToolInfo, ToolRequest, ToolResult};
use phenix_sdk::{CallError, CallResult, Host};

const ID: &str = "fixture.reverse";

#[phenix_sdk::plugin(
    id = "phenix.tool.fixture-reverse",
    version = "1.0.0",
    provide(phenix_basic_contracts::ToolRegistrations, slot = "Registrations")
)]
mod plugin {
    use super::*;
    use phenix_basic_contracts::ToolRegistrations;

    #[provide(ToolRegistrations::list)]
    fn list() -> CallResult<Vec<ToolInfo>, BasicError> {
        Ok(vec![ToolInfo {
            id: ID.into(),
            description: "Reverse Unicode scalar values".into(),
            arguments_schema: r#"{"type":"string"}"#.into(),
            idempotent: true,
        }])
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
        assert_eq!(metadata.provides.len(), 1);
        assert_eq!(metadata.requires.len(), 0);
    }
}
