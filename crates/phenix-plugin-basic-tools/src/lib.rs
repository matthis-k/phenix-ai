//! Independent, session-scoped native ToolCatalog and ToolExecute provider.
//! Admission and access control remain kernel-owned; this plugin owns tool semantics.
use phenix_basic_contracts::{
    error, BasicError, CatalogCursor, ErrorCode, ToolCall, ToolInfo, ToolPage, ToolQuery,
};
use std::collections::BTreeMap;

const REVISION: u64 = 1;

/// One source of truth for the shipped tool metadata and execution handler.
/// Runtime routing and JSON admission use this table, not repeated name matches.
struct ToolDefinition {
    id: &'static str,
    description: &'static str,
    arguments_schema: &'static str,
    idempotent: bool,
    execute: fn(&str) -> Result<String, BasicError>,
}

fn uppercase(input: &str) -> Result<String, BasicError> {
    Ok(input.to_uppercase())
}

fn length(input: &str) -> Result<String, BasicError> {
    Ok(input.chars().count().to_string())
}

fn fail(_: &str) -> Result<String, BasicError> {
    Err(error(ErrorCode::ToolFailed, "scripted failure"))
}

const TOOLS: &[ToolDefinition] = &[
    ToolDefinition {
        id: "fail",
        description: "Return a typed tool failure",
        arguments_schema: r#"{"type":"string"}"#,
        idempotent: true,
        execute: fail,
    },
    ToolDefinition {
        id: "length",
        description: "Count Unicode characters",
        arguments_schema: r#"{"type":"string"}"#,
        idempotent: true,
        execute: length,
    },
    ToolDefinition {
        id: "uppercase",
        description: "Uppercase a string",
        arguments_schema: r#"{"type":"string"}"#,
        idempotent: true,
        execute: uppercase,
    },
];

fn selected(id: &str) -> Option<&'static ToolDefinition> {
    TOOLS.iter().find(|definition| definition.id == id)
}

fn tool(definition: &ToolDefinition) -> ToolInfo {
    ToolInfo {
        id: definition.id.into(),
        description: definition.description.into(),
        arguments_schema: definition.arguments_schema.into(),
        idempotent: definition.idempotent,
    }
}

/// Cursor semantics are independent of the native host. Only the caller's
/// session-scoped catalog handler can reach this method.
pub fn catalog(request: &ToolQuery) -> Result<ToolPage, BasicError> {
    if request.limit == 0 {
        return Err(error(ErrorCode::InvalidInput, "page size must be positive"));
    }
    let offset = if let Some(cursor) = &request.cursor {
        if cursor.session != request.session
            || cursor.query != request.query
            || cursor.revision != REVISION
        {
            return Err(error(
                ErrorCode::InvalidCursor,
                "stale or foreign query cursor",
            ));
        }
        usize::try_from(cursor.offset)
            .map_err(|_| error(ErrorCode::InvalidCursor, "invalid cursor offset"))?
    } else {
        0
    };
    let matches = TOOLS
        .iter()
        .filter(|definition| definition.id.contains(&request.query))
        .collect::<Vec<_>>();
    if offset > matches.len() {
        return Err(error(ErrorCode::InvalidCursor, "cursor beyond result set"));
    }
    let take = usize::try_from(request.limit).unwrap_or(usize::MAX);
    let end = offset.saturating_add(take).min(matches.len());
    Ok(ToolPage {
        tools: matches[offset..end]
            .iter()
            .map(|definition| tool(definition))
            .collect(),
        revision: REVISION,
        next: (end < matches.len()).then(|| CatalogCursor {
            session: request.session.clone(),
            query: request.query.clone(),
            revision: REVISION,
            offset: end as u64,
        }),
    })
}

/// Validate all arguments before effects. A deliberately failing fixture is
/// still an executed tool, and therefore gets counted by the native handler.
pub fn execute(call: &ToolCall) -> Result<String, BasicError> {
    if call.id.is_empty() {
        return Err(error(ErrorCode::InvalidInput, "missing tool call id"));
    }
    let input: String = serde_json::from_str(&call.arguments)
        .map_err(|_| error(ErrorCode::InvalidInput, "expected JSON string argument"))?;
    let definition = selected(&call.tool)
        .ok_or_else(|| error(ErrorCode::NotFound, "tool not found"))?;
    (definition.execute)(&input)
}

#[phenix_sdk::plugin(
    id = "phenix.basic.tools",
    version = "1.0.0",
    provide(phenix_basic_contracts::ToolCatalog, slot = "Catalog"),
    provide(phenix_basic_contracts::ToolExecute, slot = "Tools")
)]
mod plugin {
    use super::*;
    use phenix_basic_contracts::{
        BasicError, Scope, ToolCatalog, ToolExecute, ToolPage, ToolQuery, ToolRequest, ToolResult,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State {
        counts: BTreeMap<String, u64>,
    }

    #[provide(ToolCatalog::list)]
    fn list(
        _: &mut State,
        host: &Host<'_>,
        request: ToolQuery,
    ) -> CallResult<ToolPage, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        super::catalog(&request).map_err(CallError::Domain)
    }

    #[provide(ToolExecute::execute)]
    fn execute(
        state: &mut State,
        host: &Host<'_>,
        request: ToolRequest,
    ) -> CallResult<ToolResult, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        if request.run.is_empty() {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidInput,
                "missing run",
            )));
        }
        // Unknown tools and invalid JSON fail without consuming a tool effect.
        if selected(&request.call.tool).is_none() {
            return Err(CallError::Domain(error(
                ErrorCode::NotFound,
                "tool not found",
            )));
        }
        let _: String = serde_json::from_str(&request.call.arguments)
            .map_err(|_| CallError::Domain(error(ErrorCode::InvalidInput, "invalid tool JSON")))?;
        if request.call.id.is_empty() {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidInput,
                "missing call id",
            )));
        }
        let counter = state.counts.entry(request.session.clone()).or_default();
        *counter = counter.checked_add(1).ok_or_else(|| {
            CallError::Domain(error(ErrorCode::ToolFailed, "call count overflow"))
        })?;
        super::execute(&request.call)
            .map(|text| ToolResult { text })
            .map_err(CallError::Domain)
    }

    #[provide(ToolExecute::count)]
    fn count(state: &mut State, host: &Host<'_>, request: Scope) -> CallResult<u64, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        Ok(*state.counts.get(&request.session).unwrap_or(&0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(query: &str, offset: Option<CatalogCursor>, limit: u64) -> ToolQuery {
        ToolQuery {
            session: "one".into(),
            query: query.into(),
            cursor: offset,
            limit,
        }
    }

    #[test]
    fn catalog_cursor_is_bound_to_query_and_revision() {
        let first = catalog(&page("", None, 1)).unwrap();
        assert_eq!(first.tools[0].id, "fail");
        assert_eq!(first.revision, 1);
        assert_eq!(first.next.as_ref().unwrap().session, "one");
        let next = first.next.unwrap();
        assert_eq!(
            catalog(&page("", Some(next.clone()), 1)).unwrap().tools[0].id,
            "length"
        );
        assert_eq!(
            catalog(&page("up", Some(next.clone()), 1))
                .unwrap_err()
                .code,
            ErrorCode::InvalidCursor
        );
        // A cursor is a continuation of one session's catalog query, not
        // a transferable offset for another session with the same query.
        let mut foreign = page("", Some(next.clone()), 1);
        foreign.session = "another".into();
        assert_eq!(
            catalog(&foreign).unwrap_err().code,
            ErrorCode::InvalidCursor
        );
        let mut stale = next;
        stale.revision = 0;
        assert_eq!(
            catalog(&page("", Some(stale), 1)).unwrap_err().code,
            ErrorCode::InvalidCursor
        );
        assert_eq!(
            catalog(&page("", None, 0)).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }

    #[test]
    fn declarations_and_dispatch_use_the_same_unique_ordered_registry() {
        let catalog = catalog(&page("", None, 10)).unwrap();
        assert_eq!(catalog.tools.len(), TOOLS.len());
        assert!(TOOLS.windows(2).all(|pair| pair[0].id < pair[1].id));
        for definition in TOOLS {
            let entry = catalog
                .tools
                .iter()
                .find(|item| item.id == definition.id)
                .unwrap();
            assert_eq!(entry.description, definition.description);
            assert_eq!(entry.arguments_schema, definition.arguments_schema);
            let call = ToolCall {
                id: "registered".into(),
                tool: definition.id.into(),
                arguments: r#""example""#.into(),
            };
            let outcome = execute(&call);
            assert!(outcome.is_ok() || outcome.unwrap_err().code == ErrorCode::ToolFailed);
        }
    }

    #[test]
    fn typed_tool_results_and_errors() {
        let mut call = ToolCall {
            id: "c1".into(),
            tool: "uppercase".into(),
            arguments: r#""aß""#.into(),
        };
        assert_eq!(execute(&call).unwrap(), "ASS");
        call.tool = "length".into();
        assert_eq!(execute(&call).unwrap(), "2");
        call.tool = "fail".into();
        assert_eq!(execute(&call).unwrap_err().code, ErrorCode::ToolFailed);
        call.arguments = "not-json".into();
        assert_eq!(execute(&call).unwrap_err().code, ErrorCode::InvalidInput);
    }
}
