//! Independent, session-scoped native ToolCatalog and ToolExecute provider.
//! Admission and access control remain kernel-owned; this plugin owns tool semantics.
mod event_registry;
use phenix_basic_contracts::{
    error, BasicError, CatalogCursor, ErrorCode, ToolCall, ToolInfo, ToolPage, ToolQuery,
};
use phenix_basic_contracts::{ToolsPopulate, ToolsReady};
use phenix_sdk::{StaticGraphContribution, StaticGraphNode};
use std::collections::BTreeMap;

/// Tools owns the extension phase; other plugins may subscribe but cannot
/// emit tools.populate on its behalf. No alternative graph scheduler.
pub const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "phenix.basic.tools",
    events: &[ToolsPopulate::EVENT, ToolsReady::EVENT],
    actions: &[],
    nodes: &[
        StaticGraphNode::on(
            "on-begin",
            &[phenix_sdk::KERNEL_BEGIN],
            "phenix.tools.startup@1/begin",
        ),
        StaticGraphNode::on(
            "on-populated",
            &["settled:phenix.tools.populate"],
            "phenix.tools.settlement@1/complete",
        ),
    ],
};

const REVISION: u64 = 1;

/// Content-address the validated catalog for cursor invalidation. A legacy
/// built-in-only catalog preserves the existing revision for compatibility.
fn revision(tools: &[ToolInfo], contributors: usize) -> u64 {
    if contributors == 0 {
        return REVISION;
    }
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
}

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

#[phenix_sdk::plugin(
    id = "phenix.basic.tools",
    version = "1.0.0",
    provide(phenix_basic_contracts::ToolCatalog, slot = "Catalog"),
    provide(phenix_basic_contracts::ToolExecute, slot = "Tools"),
    provide(phenix_basic_contracts::ToolRegistry, slot = "Registry"),
    provide(phenix_basic_contracts::ToolStartup, slot = "Startup"),
    provide(phenix_basic_contracts::ToolSettlement, slot = "Settlement"),
    require(
        phenix_basic_contracts::ToolRegistrations,
        slot = "Contributors",
        many(min = 0)
    ),
    graph(super::GRAPH)
)]
mod plugin {
    use super::*;
    use phenix_basic_contracts::{
        BasicError, Scope, ToolCatalog, ToolExecute, ToolInfo, ToolKernelBegin, ToolPage,
        ToolQuery, ToolRegistrations, ToolRegistry, ToolRequest, ToolResult, ToolSettlement,
        ToolStartup, ToolsPopulate, ToolsPopulated, ToolsReady,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State {
        counts: BTreeMap<String, u64>,
        /// The manager-owned registry is the sole authoritative catalog.
        /// Execution routes are resolver-selected; publication is immutable.
        registry: super::event_registry::Registry,
        /// Attested selected contributor catalogs captured BEFORE event
        /// fan-out, so event handlers cannot recursively enter themselves.
        attested_catalogs: Option<Vec<Vec<ToolInfo>>>,
    }

    /// Kernel.begin stages the manager-owned intrinsic tools and emits the
    /// plugin-owned populate event. Subscriber plugins register through the
    /// ordinary selected ToolRegistry contract, not event payload routing.
    #[provide(ToolStartup::begin)]
    fn on_begin(
        state: &mut State,
        host: &Host<'_>,
        _: ToolKernelBegin,
    ) -> CallResult<(), BasicError> {
        if state.registry.selected().is_some() {
            return Err(CallError::Domain(error(
                ErrorCode::Conflict,
                "tool catalog already selected",
            )));
        }
        let count = imports::Contributors::count(host)?;
        if count > 64 {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidInput,
                "too many tool contributors",
            )));
        }
        let mut catalogs = Vec::with_capacity(count);
        for index in 0..count {
            catalogs.push(imports::Contributors::invoke_at::<ToolRegistrations::list>(
                host,
                index,
                &(),
            )?);
        }
        state
            .registry
            .begin(TOOLS.iter().map(super::tool))
            .map_err(CallError::Domain)?;
        state.attested_catalogs = Some(catalogs);
        if let Err(failure) = host.emit_event(&ToolsPopulate {}) {
            let _ = state.registry.abort();
            return Err(CallError::Kernel(failure));
        }
        Ok(())
    }

    /// Verify every contribution against a preselected M1 import. The actual
    /// imported executor index comes from the resolver, never untrusted data.
    #[provide(ToolRegistry::register)]
    fn register(state: &mut State, offered: ToolInfo) -> CallResult<(), BasicError> {
        if state.registry.selected().is_some() {
            return Err(CallError::Domain(error(
                ErrorCode::Conflict,
                "tool catalog is already published",
            )));
        }
        let catalogs = state.attested_catalogs.as_deref().ok_or_else(|| {
            CallError::Domain(error(
                ErrorCode::Conflict,
                "tool registration requires an attested population wave",
            ))
        })?;
        state
            .registry
            .register(offered, catalogs)
            .map_err(CallError::Domain)
    }

    /// Finalize only after the entire tools.populate wave has settled.
    /// Legacy pull-only contributors are staged through the SAME registry,
    /// preserving their authenticated execution route during migration.
    #[provide(ToolSettlement::complete)]
    fn on_populated(
        state: &mut State,
        host: &Host<'_>,
        _: ToolsPopulated,
    ) -> CallResult<(), BasicError> {
        if state.registry.selected().is_some() {
            return Err(CallError::Domain(error(
                ErrorCode::Conflict,
                "tool catalog is already published",
            )));
        }
        let catalogs = state.attested_catalogs.as_deref().ok_or_else(|| {
            CallError::Domain(error(
                ErrorCode::Conflict,
                "tool settlement requires an attested population wave",
            ))
        })?;
        let count = catalogs.len();
        for (index, catalog) in catalogs.iter().enumerate() {
            for offered in catalog {
                if let Some(existing) = state.registry.find(&offered.id) {
                    if existing.route != Some(index) || existing.info != *offered {
                        return Err(CallError::Domain(error(
                            ErrorCode::InvalidInput,
                            "duplicate or mismatched tool contributor",
                        )));
                    }
                    continue;
                }
                state
                    .registry
                    .register(offered.clone(), catalogs)
                    .map_err(CallError::Domain)?;
            }
        }
        // A full queue must not commit a catalog without delivering ready.
        // The ready notification itself executes only in a later graph wave.
        let preview = state.registry.preview().map_err(CallError::Domain)?;
        let tools = preview
            .iter()
            .map(|entry| entry.info.clone())
            .collect::<Vec<_>>();
        let revision = super::revision(&tools, count);
        host.emit_event(&ToolsReady { revision })
            .map_err(CallError::Kernel)?;
        state
            .registry
            .publish(revision)
            .map_err(CallError::Domain)?;
        // Once the selected execution routes are sealed in the registry,
        // attestation pages are no longer needed. The snapshot is the single
        // authority for both catalog queries and routing.
        state.attested_catalogs = None;
        Ok(())
    }

    fn selected_snapshot<'a>(
        state: &'a mut State,
        host: &Host<'_>,
    ) -> CallResult<&'a super::event_registry::Catalog, BasicError> {
        if state.registry.selected().is_none() {
            if state.registry.blocks_pull_catalog() {
                return Err(CallError::Domain(error(
                    ErrorCode::Conflict,
                    "tool catalog population has not settled",
                )));
            }
            let count = imports::Contributors::count(host)?;
            if count > 64 {
                return Err(CallError::Domain(error(
                    ErrorCode::InvalidInput,
                    "too many tool contributors",
                )));
            }
            // This temporary direct compatibility adapter uses the same
            // authoritative Registry as the graph path, never a second
            // snapshot or a caller-provided execution route.
            state
                .registry
                .begin(TOOLS.iter().map(super::tool))
                .map_err(CallError::Domain)?;
            let mut selected_catalogs = Vec::with_capacity(count);
            for index in 0..count {
                selected_catalogs.push(
                    imports::Contributors::invoke_at::<ToolRegistrations::list>(host, index, &())?,
                );
            }
            for catalog in &selected_catalogs {
                for offered in catalog {
                    state
                        .registry
                        .register(offered.clone(), &selected_catalogs)
                        .map_err(CallError::Domain)?;
                }
            }
            let tools = state
                .registry
                .preview()
                .map_err(CallError::Domain)?
                .into_iter()
                .map(|entry| entry.info)
                .collect::<Vec<_>>();
            let revision = super::revision(&tools, count);
            state
                .registry
                .publish(revision)
                .map_err(CallError::Domain)?;
        }
        state.registry.selected().ok_or_else(|| {
            CallError::Domain(error(
                ErrorCode::Conflict,
                "published tool catalog is unavailable",
            ))
        })
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
