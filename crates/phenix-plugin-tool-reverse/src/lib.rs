//! One tool definition and one handler. No tool-manager, registry or graph scheduler.
use phenix_basic_contracts::{error, BasicError, ErrorCode, ToolInfo, ToolRequest, ToolResult};
use phenix_sdk::{CallError, CallResult, Host};

const ID: &str = "text.reverse";

pub fn reverse(input: &str) -> String {
    input.chars().rev().collect()
}

#[phenix_sdk::plugin(
    id = "phenix.tool.reverse",
    version = "1.0.0",
    provide(phenix_basic_contracts::ToolRegistrations, slot = "Tools")
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
                "missing run or call ID",
            )));
        }
        if request.call.tool != ID {
            return Err(CallError::Domain(error(ErrorCode::NotFound, "unknown tool")));
        }
        let input: String = serde_json::from_str(&request.call.arguments).map_err(|_| {
            CallError::Domain(error(ErrorCode::InvalidInput, "expected JSON string"))
        })?;
        Ok(ToolResult { text: reverse(&input) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_sdk::ContractMetadata;

    #[test]
    fn unicode_scalar_order_and_metadata() {
        assert_eq!(reverse("a🙂é"), "é🙂a");
        let meta = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert_eq!(meta.provides.len(), 1);
        assert!(meta.requires.is_empty());
    }
}
