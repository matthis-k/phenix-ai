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

/// Page a pinned catalog: the cursor is bound to session, query and the
/// immutable contributor snapshot revision.
fn catalog_from(
    request: &ToolQuery,
    tools: &[ToolInfo],
    revision: u64,
) -> Result<ToolPage, BasicError> {
    if request.limit == 0 {
        return Err(error(ErrorCode::InvalidInput, "page size must be positive"));
    }
    let offset = if let Some(cursor) = &request.cursor {
        if cursor.session != request.session
            || cursor.query != request.query
            || cursor.revision != revision
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
    let matches = tools
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
            .map(|tool| (*tool).clone())
            .collect(),
        revision,
        next: (end < matches.len()).then(|| CatalogCursor {
            session: request.session.clone(),
            query: request.query.clone(),
            revision,
            offset: end as u64,
        }),
    })
}

/// The default composition remains usable without external contributors.
pub fn catalog(request: &ToolQuery) -> Result<ToolPage, BasicError> {
    let tools = TOOLS.iter().map(tool).collect::<Vec<_>>();
    catalog_from(request, &tools, REVISION)
}

/// Validate all arguments before effects. A deliberately failing fixture is
/// still an executed tool, and therefore gets counted by the native handler.
pub fn execute(call: &ToolCall) -> Result<String, BasicError> {
    if call.id.is_empty() {
        return Err(error(ErrorCode::InvalidInput, "missing tool call id"));
    }
    let input: String = serde_json::from_str(&call.arguments)
        .map_err(|_| error(ErrorCode::InvalidInput, "expected JSON string argument"))?;
    let definition =
        selected(&call.tool).ok_or_else(|| error(ErrorCode::NotFound, "tool not found"))?;
    (definition.execute)(&input)
}

#[cfg(feature = "graph-v2")]
#[used]
#[no_mangle]
#[link_section = ".phenix.graph"]
pub static PHENIX_GRAPH: [u8; phenix_sdk::NATIVE_SECTION_BYTES] =
    *include_bytes!(concat!(env!("OUT_DIR"), "/graph-v2.bin"));

#[phenix_sdk::plugin(
    id = "phenix.basic.tools",
    version = "1.0.0",
    provide(phenix_basic_contracts::ToolCatalog, slot = "Catalog"),
    provide(phenix_basic_contracts::ToolExecute, slot = "Tools"),
    provide(phenix_basic_contracts::ToolGraph, slot = "GraphTools"),
    provide(phenix_basic_contracts::ToolGraphExecutor, slot = "GraphBuiltin"),
    require(phenix_basic_contracts::ToolReceipts, slot = "Receipts", optional),
    require(phenix_basic_contracts::Storage, slot = "StorageAuthority", optional),
    require(
        phenix_basic_contracts::ToolGraphExecutor,
        slot = "GraphContributors",
        many(min = 0)
    ),
    require(
        phenix_basic_contracts::ToolRegistrations,
        slot = "Contributors",
        many(min = 0)
    )
)]
mod plugin {
    use super::*;
    use phenix_basic_contracts::{
        BasicError, GraphToolBatch, GraphToolCollection, GraphToolFailure, GraphToolInvocation,
        GraphToolSuccess, RoutedToolInvocation, Scope, ToolCatalog, ToolExecute, ToolGraph,
        ToolGraphExecutor, ToolPage, ToolQuery, ToolRegistrations, ToolRequest, ToolResult,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    /// Stable, preauthorized execution route, not a model-supplied provider ID.
    struct ToolSnapshot {
        tools: Vec<ToolInfo>,
        routes: BTreeMap<String, Option<usize>>,
        revision: u64,
    }

    #[derive(Default)]
    pub struct State {
        counts: BTreeMap<String, u64>,
        snapshot: Option<ToolSnapshot>,
        planned: BTreeMap<String, (GraphToolBatch, bool)>,
    }

    fn selected_snapshot<'a>(
        state: &'a mut State,
        host: &Host<'_>,
    ) -> CallResult<&'a ToolSnapshot, BasicError> {
        if state.snapshot.is_none() {
            let count = if cfg!(feature = "graph-v2") {
                imports::GraphContributors::count(host)?
            } else {
                imports::Contributors::count(host)?
            };
            if count > 64 {
                return Err(CallError::Domain(error(
                    ErrorCode::InvalidInput,
                    "too many tool contributors",
                )));
            }
            let mut routes = BTreeMap::new();
            let mut tools = Vec::new();
            for definition in TOOLS {
                routes.insert(definition.id.to_owned(), None);
                tools.push(tool(definition));
            }
            for index in 0..count {
                let contributions = if cfg!(feature = "graph-v2") {
                    imports::GraphContributors::invoke_at::<ToolGraphExecutor::list>(
                        host,
                        index,
                        &(),
                    )?
                } else {
                    imports::Contributors::invoke_at::<ToolRegistrations::list>(host, index, &())?
                };
                if contributions.len() > 256 {
                    return Err(CallError::Domain(error(
                        ErrorCode::InvalidInput,
                        "too many registered tools",
                    )));
                }
                for offered in contributions {
                    if offered.id.is_empty()
                        || offered.id.len() > 128
                        || !offered.id.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric()
                                || byte == b'.'
                                || byte == b'-'
                                || byte == b'_'
                        })
                        || offered.description.is_empty()
                        || offered.description.len() > 4096
                        || offered.arguments_schema.len() > 16384
                        || !serde_json::from_str::<serde_json::Value>(&offered.arguments_schema)
                            .is_ok_and(|schema| schema.is_object())
                        || tools.len() >= 256
                        || routes.insert(offered.id.clone(), Some(index)).is_some()
                    {
                        return Err(CallError::Domain(error(
                            ErrorCode::InvalidInput,
                            "invalid or conflicting tool registration",
                        )));
                    }
                    tools.push(offered);
                }
            }
            tools.sort_by(|a, b| a.id.cmp(&b.id));
            let revision = if count == 0 {
                REVISION
            } else {
                tools
                    .iter()
                    .fold(14695981039346656037_u64, |mut hash, entry| {
                        for byte in entry
                            .id
                            .bytes()
                            .chain([0])
                            .chain(entry.description.bytes())
                            .chain([0])
                            .chain(entry.arguments_schema.bytes())
                            .chain([u8::from(entry.idempotent), 0])
                        {
                            hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
                        }
                        hash
                    })
                    .max(1)
            };
            state.snapshot = Some(ToolSnapshot {
                tools,
                routes,
                revision,
            });
        }
        Ok(state.snapshot.as_ref().expect("initialized tool snapshot"))
    }

    #[provide(ToolCatalog::list)]
    fn list(
        state: &mut State,
        host: &Host<'_>,
        request: ToolQuery,
    ) -> CallResult<ToolPage, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        let snapshot = selected_snapshot(state, host)?;
        super::catalog_from(&request, &snapshot.tools, snapshot.revision).map_err(CallError::Domain)
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
        // Resolve only against the sealed, kernel-selected contributor table.
        // Invalid names/JSON cannot increment counters or invoke effects.
        let route = selected_snapshot(state, host)?
            .routes
            .get(&request.call.tool)
            .copied()
            .ok_or_else(|| CallError::Domain(error(ErrorCode::NotFound, "tool not found")))?;
        if cfg!(feature = "graph-v2") && route.is_some() {
            return Err(CallError::Domain(error(
                ErrorCode::Denied,
                "V2 contributors require graph admission",
            )));
        }
        if request.call.id.is_empty() {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidInput,
                "missing call id",
            )));
        }
        let parsed: serde_json::Value = serde_json::from_str(&request.call.arguments)
            .map_err(|_| CallError::Domain(error(ErrorCode::InvalidInput, "invalid tool JSON")))?;
        if route.is_none() && !parsed.is_string() {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidInput,
                "expected JSON string argument",
            )));
        }
        let counter = state.counts.entry(request.session.clone()).or_default();
        *counter = counter.checked_add(1).ok_or_else(|| {
            CallError::Domain(error(ErrorCode::ToolFailed, "call count overflow"))
        })?;
        match route {
            Some(index) => imports::Contributors::invoke_at::<ToolRegistrations::execute>(
                host, index, &request,
            ),
            None => super::execute(&request.call)
                .map(|text| ToolResult { text })
                .map_err(CallError::Domain),
        }
    }

    #[provide(ToolExecute::count)]
    fn count(state: &mut State, host: &Host<'_>, request: Scope) -> CallResult<u64, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        Ok(*state.counts.get(&request.session).unwrap_or(&0))
    }

    #[provide(ToolGraphExecutor::list)]
    fn builtin_list() -> CallResult<Vec<ToolInfo>, BasicError> {
        Ok(TOOLS.iter().map(tool).collect())
    }

    #[provide(ToolGraphExecutor::validate)]
    fn validate_builtin(
        host: &Host<'_>,
        invocation: GraphToolInvocation,
    ) -> CallResult<(), BasicError> {
        phenix_basic_contracts::check_session(host, &invocation.session)?;
        if invocation.call.id.is_empty()
            || selected(&invocation.call.tool).is_none()
            || serde_json::from_str::<String>(&invocation.call.arguments).is_err()
        {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidInput,
                "invalid builtin invocation",
            )));
        }
        Ok(())
    }

    #[provide(ToolGraphExecutor::invoke)]
    fn builtin_invoke(
        host: &Host<'_>,
        invocation: GraphToolInvocation,
    ) -> CallResult<GraphToolSuccess, GraphToolFailure> {
        phenix_basic_contracts::check_session(host, &invocation.session).map_err(|failure| {
            match failure {
                CallError::Kernel(failure) => CallError::Kernel(failure),
                CallError::Domain(error) => CallError::Domain(GraphToolFailure {
                    invocation: Box::new(invocation.clone()),
                    error,
                }),
            }
        })?;
        if invocation.run.is_empty() || invocation.batch == 0 {
            return Err(CallError::Domain(GraphToolFailure {
                invocation: Box::new(invocation),
                error: error(ErrorCode::InvalidInput, "missing tool batch identity"),
            }));
        }
        let outcome = match super::execute(&invocation.call) {
            Ok(text) => phenix_basic_contracts::GraphToolOutcome::Ok { invocation, text },
            Err(error) => phenix_basic_contracts::GraphToolOutcome::Err { invocation, error },
        };
        phenix_basic_contracts::complete_graph_tool(host, outcome)
    }

    fn prepare_routes(
        state: &mut State,
        host: &Host<'_>,
        batch: GraphToolBatch,
    ) -> CallResult<Vec<RoutedToolInvocation>, BasicError> {
        if !cfg!(feature = "graph-v2") {
            return Err(CallError::Domain(error(
                ErrorCode::Denied,
                "graph tool planning requires V2",
            )));
        }
        phenix_basic_contracts::check_session(host, &batch.session)?;
        if batch.run.is_empty() || batch.batch == 0 || batch.calls.len() > 256 {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidInput,
                "invalid graph tool batch",
            )));
        }
        if state
            .planned
            .get(&batch.session)
            .is_some_and(|(previous, _)| previous.run == batch.run && previous.batch >= batch.batch)
        {
            return Err(CallError::Domain(error(
                ErrorCode::Conflict,
                "tool batch already planned",
            )));
        }
        let self_route = imports::GraphContributors::count(host)? as u64;
        let snapshot = selected_snapshot(state, host)?;
        let mut ids = std::collections::BTreeSet::new();
        let mut invocations = Vec::new();
        for input in batch.invocations() {
            if input.call.id.is_empty() || !ids.insert(input.call.id.clone()) {
                return Err(CallError::Domain(error(
                    ErrorCode::InvalidReply,
                    "duplicate or empty tool call ID",
                )));
            }
            let route = snapshot
                .routes
                .get(&input.call.tool)
                .ok_or_else(|| CallError::Domain(error(ErrorCode::NotFound, "tool not found")))?;
            let parsed: serde_json::Value =
                serde_json::from_str(&input.call.arguments).map_err(|_| {
                    CallError::Domain(error(ErrorCode::InvalidInput, "invalid tool JSON"))
                })?;
            if route.is_none() && !parsed.is_string() {
                return Err(CallError::Domain(error(
                    ErrorCode::InvalidInput,
                    "expected JSON string argument",
                )));
            }
            if let Some(index) = route {
                imports::GraphContributors::invoke_at::<ToolGraphExecutor::validate>(
                    host, *index, &input,
                )?;
            }
            invocations.push(RoutedToolInvocation {
                route: route.map_or(self_route, |index| index as u64),
                input,
            });
        }
        Ok(invocations)
    }

    #[provide(ToolGraph::validate)]
    fn validate(
        state: &mut State,
        host: &Host<'_>,
        batch: GraphToolBatch,
    ) -> CallResult<(), BasicError> {
        prepare_routes(state, host, batch).map(|_| ())
    }

    #[provide(ToolGraph::plan)]
    fn plan(
        state: &mut State,
        host: &Host<'_>,
        batch: GraphToolBatch,
    ) -> CallResult<Vec<RoutedToolInvocation>, BasicError> {
        let invocations = prepare_routes(state, host, batch.clone())?;
        state.planned.insert(batch.session.clone(), (batch, false));
        Ok(invocations)
    }

    #[provide(ToolGraph::collect)]
    fn collect(
        state: &mut State,
        host: &Host<'_>,
        collection: GraphToolCollection,
    ) -> CallResult<GraphToolCollection, BasicError> {
        phenix_basic_contracts::check_session(host, &collection.batch.session)?;
        collection.validate().map_err(CallError::Domain)?;
        let admitted = state
            .planned
            .get_mut(&collection.batch.session)
            .filter(|(batch, consumed)| batch == &collection.batch && !*consumed)
            .ok_or_else(|| {
                CallError::Domain(error(ErrorCode::Denied, "not the planned tool batch"))
            })?;
        for outcome in &collection.outcomes {
            if host
                .invoke::<phenix_basic_contracts::ToolReceipts::read>(
                    "Receipts",
                    outcome.invocation(),
                )?
                .as_ref()
                != Some(outcome)
            {
                return Err(CallError::Domain(error(
                    ErrorCode::Denied,
                    "tool collection has no exact durable receipt",
                )));
            }
        }
        let counter = state
            .counts
            .entry(collection.batch.session.clone())
            .or_default();
        let count = counter
            .checked_add(collection.outcomes.len() as u64)
            .ok_or_else(|| {
                CallError::Domain(error(ErrorCode::ToolFailed, "call count overflow"))
            })?;
        admitted.1 = true;
        *counter = count;
        Ok(collection)
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
