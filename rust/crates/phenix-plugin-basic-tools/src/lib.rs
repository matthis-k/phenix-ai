use phenix_core::{
    Authority, CallableId, CapabilityId, ComponentId, ComponentInterface, ComponentManifest,
    ArtifactRevision, InterfaceId, PluginContext, PluginInstance, PluginManifest, ResourceNamespace,
    ToolCatalogCursor, ToolCatalogDescriptor, ToolCommand, ToolDefinition, ToolResponse,
    TransactionOp, TOOL_SERVICE,
};
use phenix_sdk::{StaticPluginComponentDispatch, StaticPluginDefinition};

pub const BASIC_TOOLS_PLUGIN: &str = "phenix.basic-tools";
pub const BASIC_TOOLS_COMPONENT: &str = "phenix.basic-tools";
const BASIC_TOOLS_NAMESPACE: &str = "phenix.basic-tools.state";
const INDEX_KEY: &str = "tools/@all";

type BasicToolsContext<'host, 'runtime> = PluginContext<'host, 'runtime, ()>;

pub struct BasicToolsInterface;

impl ComponentInterface for BasicToolsInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(TOOL_SERVICE).expect("static tool interface id is valid")
    }
}

struct ToolStore;

#[phenix_sdk::resource(schema = 1)]
impl ToolStore {}

#[phenix_sdk::component]
struct Api;

#[phenix_sdk::component]
impl Api {
    #[phenix(export("phenix.tools@1"), terminal, priority = 10)]
    fn handle(
        &self,
        context: &phenix_sdk::PluginContext<'_, '_, ()>,
        command: ToolCommand,
    ) -> Result<ToolResponse, String> {
        handle(context, command)
    }
}

#[phenix_sdk::plugin(id = "phenix.basic-tools", authority = persistence_authority())]
pub struct Plugin {
    #[phenix(component, id = "phenix.basic-tools")]
    api: Api,

    #[phenix(resource, id = "phenix.basic-tools.state")]
    _state: phenix_sdk::Durable<ToolStore>,
}

#[must_use]
pub fn basic_tools_manifest() -> PluginManifest {
    Plugin::manifest()
}

#[must_use]
pub fn basic_tools_component_manifest() -> ComponentManifest {
    Plugin::component_manifests()
        .into_iter()
        .next()
        .expect("basic tools plugin has one generated component")
}

#[must_use]
pub fn basic_tools_factory() -> Box<dyn PluginInstance> {
    StaticPluginComponentDispatch::into_plugin_instance(Plugin {
        api: Api,
        _state: phenix_sdk::Durable::new(),
    })
}

#[must_use]
pub fn basic_tools_component_id() -> ComponentId {
    basic_tools_component_manifest().id
}

fn handle(
    context: &BasicToolsContext<'_, '_>,
    command: ToolCommand,
) -> Result<ToolResponse, String> {
    match command {
        ToolCommand::Register { tool } => {
            write_tool(context, &tool)?;
            Ok(ToolResponse::Tool { tool: Some(tool) })
        }
        ToolCommand::Get { id } => Ok(ToolResponse::Tool {
            tool: read_tool(context, &id)?,
        }),
        ToolCommand::List => Ok(ToolResponse::Tools {
            tools: read_ids(context)?
                .into_iter()
                .map(|id| {
                    read_tool(context, &id)?.ok_or_else(|| format!("missing durable tool: {id}"))
                })
                .collect::<Result<Vec<_>, _>>()?,
        }),
        ToolCommand::Search { query, cursor, limit } => search_catalog(context, query, cursor, limit),
        ToolCommand::LoadSchemas {
            ids,
            catalog_revision,
        } => load_schemas(context, ids, catalog_revision),
        ToolCommand::Invoke { id, input } => {
            let tool = read_tool(context, &id)?.ok_or_else(|| format!("unknown tool: {id}"))?;
            let mut output = tool.output_prefix.into_vec();
            output.extend_from_slice(input.as_ref());
            Ok(ToolResponse::Output {
                output: output.into(),
            })
        }
    }
}

const MAX_CATALOG_PAGE: u32 = 100;

fn catalog_snapshot(
    context: &BasicToolsContext<'_, '_>,
) -> Result<(ArtifactRevision, Vec<ToolDefinition>), String> {
    let tools = read_ids(context)?
        .into_iter()
        .map(|id| read_tool(context, &id)?.ok_or_else(|| format!("missing durable tool: {id}")))
        .collect::<Result<Vec<_>, _>>()?;
    let encoded = serde_json::to_vec(&tools).map_err(|error| error.to_string())?;
    Ok((ArtifactRevision::from_content(&encoded), tools))
}

fn schema_identity(schema: &phenix_core::PhenixSchema) -> Result<ArtifactRevision, String> {
    serde_json::to_vec(schema)
        .map(|encoded| ArtifactRevision::from_content(&encoded))
        .map_err(|error| error.to_string())
}

fn search_catalog(
    context: &BasicToolsContext<'_, '_>,
    query: String,
    cursor: Option<ToolCatalogCursor>,
    limit: u32,
) -> Result<ToolResponse, String> {
    if limit == 0 {
        return Err("tool catalog page limit must be greater than zero".into());
    }
    let (catalog_revision, tools) = catalog_snapshot(context)?;
    let query_identity = ArtifactRevision::from_content(query.as_bytes());
    let offset = match cursor {
        Some(cursor) => {
            if cursor.catalog_revision != catalog_revision {
                return Err("stale tool catalog cursor revision".into());
            }
            if cursor.query_identity != query_identity {
                return Err("tool catalog cursor belongs to a different query".into());
            }
            usize::try_from(cursor.offset)
                .map_err(|_| "tool catalog cursor offset cannot be represented".to_owned())?
        }
        None => 0,
    };

    let needle = query.to_lowercase();
    let matched = tools
        .into_iter()
        .filter(|tool| {
            query.is_empty()
                || tool.id.as_str().to_lowercase().contains(&needle)
                || tool.description.to_lowercase().contains(&needle)
        })
        .collect::<Vec<_>>();
    if offset > matched.len() {
        return Err("tool catalog cursor offset exceeds result set".into());
    }
    let page_len = usize::try_from(limit.min(MAX_CATALOG_PAGE))
        .expect("bounded u32 catalog page size fits usize");
    let end = offset.saturating_add(page_len).min(matched.len());
    let descriptors = matched[offset..end]
        .iter()
        .map(|tool| {
            Ok(ToolCatalogDescriptor {
                id: tool.id.clone(),
                description: tool.description.clone(),
                input_type_identity: schema_identity(&tool.input_schema)?,
                output_type_identity: schema_identity(&tool.output_schema)?,
                catalog_revision: catalog_revision.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let next_cursor = (end < matched.len()).then(|| ToolCatalogCursor {
        catalog_revision: catalog_revision.clone(),
        query_identity,
        offset: u32::try_from(end).unwrap_or(u32::MAX),
    });
    Ok(ToolResponse::Catalog {
        descriptors,
        next_cursor,
        catalog_revision,
    })
}

fn load_schemas(
    context: &BasicToolsContext<'_, '_>,
    ids: Vec<CallableId>,
    expected_revision: ArtifactRevision,
) -> Result<ToolResponse, String> {
    let (catalog_revision, tools) = catalog_snapshot(context)?;
    if expected_revision != catalog_revision {
        return Err("stale tool catalog revision".into());
    }
    let requested = ids.into_iter().collect::<std::collections::BTreeSet<_>>();
    let available = tools
        .into_iter()
        .map(|tool| (tool.id.clone(), tool))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut selected = Vec::with_capacity(requested.len());
    for id in requested {
        let tool = available
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("unknown tool schema requested: {id}"))?;
        selected.push(tool);
    }
    Ok(ToolResponse::Schemas {
        tools: selected,
        catalog_revision,
    })
}

fn write_tool(context: &BasicToolsContext<'_, '_>, tool: &ToolDefinition) -> Result<(), String> {
    let mut ids = read_ids(context)?;
    if !ids.contains(&tool.id) {
        ids.push(tool.id.clone());
        ids.sort();
    }
    context
        .kernel
        .transact_durable(
            &namespace(),
            &[
                TransactionOp::Put {
                    key: format!("tool/{}", tool.id),
                    value: serde_json::to_vec(tool).map_err(|error| error.to_string())?,
                },
                TransactionOp::Put {
                    key: INDEX_KEY.into(),
                    value: serde_json::to_vec(&ids).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())
}

fn read_tool(
    context: &BasicToolsContext<'_, '_>,
    id: &CallableId,
) -> Result<Option<ToolDefinition>, String> {
    context
        .kernel
        .read_durable(&namespace(), &format!("tool/{id}"))
        .map_err(|error| error.to_string())?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn read_ids(context: &BasicToolsContext<'_, '_>) -> Result<Vec<CallableId>, String> {
    context
        .kernel
        .read_durable(&namespace(), INDEX_KEY)
        .map_err(|error| error.to_string())?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn namespace() -> ResourceNamespace {
    ResourceNamespace::parse(BASIC_TOOLS_NAMESPACE).expect("static namespace is valid")
}

fn persistence_authority() -> Authority {
    Authority::new([
        capability("kernel.persistence.schema"),
        capability("kernel.persistence.read"),
        capability("kernel.persistence.write"),
    ])
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).expect("static capability is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_authoring_preserves_stable_identity() {
        let manifest = basic_tools_manifest();
        assert_eq!(manifest.id.as_str(), BASIC_TOOLS_PLUGIN);
        assert!(manifest.services.is_empty());
        assert_eq!(manifest.resource_namespaces, vec![namespace()]);

        let component = basic_tools_component_manifest();
        assert_eq!(component.id.as_str(), BASIC_TOOLS_COMPONENT);
        assert_eq!(component.exports.len(), 1);
        assert_eq!(
            component.exports[0].interface,
            BasicToolsInterface::interface_id()
        );
    }
}
