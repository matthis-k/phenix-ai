#![forbid(unsafe_code)]

use phenix_application_interface::types::SessionProjectionState;
use phenix_core::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, ContextResourceId, HasPhenixSchema, InterfaceId, PluginContext,
    PluginExecution, PluginHost, PluginId, PluginInstance, PluginManifest, SdkClient,
    SdkContribution, SdkNamespace, SdkObservableResource, SdkResourceId, ServiceContribution,
    ServiceId, ServiceRole, ValueId, ValuePath,
};
use phenix_sdk::{
    ContextCommand, ContextDescriptor, ContextInterface, ContextResourceKind,
    ContextResourceRevision, ContextResponse, ContextScope, ExecutionAuthority, ExecutionCommand,
    ExecutionInterface, ExecutionResponse, ModelRoutingInterface, OptionCommand, OptionContext,
    OptionKey, OptionResponse, OptionSubjectId, OptionValue, OptionsInterface,
};
pub use phenix_sdk::{
    SdkConfigCommand, SdkConfigInterface, SdkConfigPath, SdkConfigResponse, SdkSessionCommand,
    SdkSessionInterface, SdkSessionResponse, SdkSkill, SdkSkillCommand, SdkSkillResponse,
    SdkSkillSummary, SdkSkillsInterface, SdkTool, SdkToolCommand, SdkToolResponse,
    SdkToolsInterface, SessionCommand, SessionId, SessionInterface, SessionResponse,
    SDK_CONFIG_SERVICE, SDK_SESSION_SERVICE, SDK_SKILLS_SERVICE, SDK_TOOLS_SERVICE,
};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

pub const SDK_PLUGIN: &str = "phenix.api";
pub const SDK_COMPONENT: &str = "phenix.api";

struct SdkDependencies<'host, 'runtime> {
    sessions: SdkClient<'host, 'runtime, SessionInterface>,
    options: SdkClient<'host, 'runtime, OptionsInterface>,
    execution: SdkClient<'host, 'runtime, ExecutionInterface>,
    context: SdkClient<'host, 'runtime, ContextInterface>,
}

type SdkRuntimeContext<'host, 'runtime, 'plugin> =
    PluginContext<'host, 'runtime, SdkDependencies<'host, 'runtime>, &'plugin Option<PathBuf>>;

fn plugin_context<'host, 'runtime, 'plugin>(
    host: &'host PluginHost<'runtime>,
    config_root: &'plugin Option<PathBuf>,
) -> SdkRuntimeContext<'host, 'runtime, 'plugin> {
    let component = sdk_component_id();
    PluginContext::new(
        host,
        SdkDependencies {
            sessions: SdkClient::new(host, component.clone()),
            options: SdkClient::new(host, component.clone()),
            execution: SdkClient::new(host, component.clone()),
            context: SdkClient::new(host, component),
        },
        config_root,
        (),
    )
}

#[must_use]
pub fn sdk_config_service() -> ServiceId {
    ServiceId::parse(SDK_CONFIG_SERVICE).expect("static SDK config service id is valid")
}

#[must_use]
pub fn sdk_session_service() -> ServiceId {
    ServiceId::parse(SDK_SESSION_SERVICE).expect("static SDK session service id is valid")
}

#[must_use]
pub fn sdk_tools_service() -> ServiceId {
    ServiceId::parse(SDK_TOOLS_SERVICE).expect("static SDK tools service id is valid")
}

#[must_use]
pub fn sdk_skills_service() -> ServiceId {
    ServiceId::parse(SDK_SKILLS_SERVICE).expect("static SDK skills service id is valid")
}

#[must_use]
pub fn sdk_component_id() -> ComponentId {
    ComponentId::parse(SDK_COMPONENT).expect("static SDK component id is valid")
}

#[must_use]
pub fn sdk_manifest(maximum_authority: Authority) -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(SDK_PLUGIN).expect("static SDK plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: [
            sdk_session_service(),
            sdk_tools_service(),
            sdk_skills_service(),
            sdk_config_service(),
        ]
        .into_iter()
        .map(|service| ServiceContribution {
            role: ServiceRole::Terminal,
            service,
            priority: 100,
            required_authority: Authority::default(),
        })
        .collect(),
        resource_namespaces: Vec::new(),
        maximum_authority,
    }
}

#[must_use]
pub fn sdk_component_manifest(maximum_authority: Authority) -> ComponentManifest {
    let optional_import =
        |interface: InterfaceId, schema: phenix_core::InterfaceSchema| ComponentImport {
            interface,
            schema,
            required: false,
            authority: maximum_authority.clone(),
        };
    ComponentManifest {
        listeners: Vec::new(),
        id: sdk_component_id(),
        owner: PluginId::parse(SDK_PLUGIN).expect("static SDK plugin id is valid"),
        imports: vec![
            optional_import(SessionInterface::interface_id(), SessionInterface::schema()),
            optional_import(OptionsInterface::interface_id(), OptionsInterface::schema()),
            optional_import(
                ExecutionInterface::interface_id(),
                ExecutionInterface::schema(),
            ),
            optional_import(ContextInterface::interface_id(), ContextInterface::schema()),
        ],
        exports: vec![
            ComponentExport {
                interface: SdkSessionInterface::interface_id(),
                schema: SdkSessionInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: SdkToolsInterface::interface_id(),
                schema: SdkToolsInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: SdkSkillsInterface::interface_id(),
                schema: SdkSkillsInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: SdkConfigInterface::interface_id(),
                schema: SdkConfigInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
        ],
        maximum_authority,
    }
}

#[must_use]
pub fn sdk_factory() -> Box<dyn PluginInstance> {
    Box::new(SdkPlugin {
        config_root: env::var_os("PHENIX_CONFIG_DIR").map(PathBuf::from),
    })
}

#[must_use]
pub fn sdk_contribution() -> SdkContribution {
    let mut contribution = SdkContribution::new(
        PluginId::parse(SDK_PLUGIN).expect("static SDK plugin id is valid"),
        SdkNamespace::parse("phenix").expect("static SDK namespace is valid"),
    );
    contribution.interfaces = BTreeSet::from([
        SdkSessionInterface::interface_id(),
        ModelRoutingInterface::interface_id(),
        SdkToolsInterface::interface_id(),
        SdkSkillsInterface::interface_id(),
        SdkConfigInterface::interface_id(),
        ContextInterface::interface_id(),
        OptionsInterface::interface_id(),
    ]);
    contribution.resources = BTreeSet::from([
        SdkResourceId::parse("sdk/phenix/sessions").expect("static SDK resource id is valid"),
        SdkResourceId::parse("sdk/phenix/models").expect("static SDK resource id is valid"),
        SdkResourceId::parse("sdk/phenix/tools").expect("static SDK resource id is valid"),
        SdkResourceId::parse("sdk/phenix/skills").expect("static SDK resource id is valid"),
        SdkResourceId::parse("sdk/phenix/context").expect("static SDK resource id is valid"),
        SdkResourceId::parse("sdk/phenix/options").expect("static SDK resource id is valid"),
        SdkResourceId::parse("sdk/phenix/config").expect("static SDK resource id is valid"),
    ]);
    contribution.insert_observable(SdkObservableResource::new(
        SdkResourceId::parse("sdk/phenix/session-state")
            .expect("static session state SDK resource id is valid"),
        ["sessions", "state"],
        ValueId::parse("phenix.application.sessions@1")
            .expect("static session projection value id is valid"),
        ValuePath::root(),
        SessionProjectionState::phenix_schema(),
    ));
    contribution
}

struct SdkPlugin {
    config_root: Option<PathBuf>,
}

impl PluginInstance for SdkPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let context = plugin_context(host, &self.config_root);
        if service == &sdk_session_service() {
            let interface = SdkSessionInterface::interface_id();
            let command = context
                .kernel
                .decode::<SdkSessionCommand>(&interface, input)
                .map_err(|error| error.to_string())?;
            let response = session_command(&context, command)?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service == &sdk_tools_service() {
            let interface = SdkToolsInterface::interface_id();
            let command = context
                .kernel
                .decode::<SdkToolCommand>(&interface, input)
                .map_err(|error| error.to_string())?;
            let response = tool_command(&context, command)?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service == &sdk_skills_service() {
            let interface = SdkSkillsInterface::interface_id();
            let command = context
                .kernel
                .decode::<SdkSkillCommand>(&interface, input)
                .map_err(|error| error.to_string())?;
            let response = skill_command(&context, command)?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service == &sdk_config_service() {
            let interface = SdkConfigInterface::interface_id();
            let command = context
                .kernel
                .decode::<SdkConfigCommand>(&interface, input)
                .map_err(|error| error.to_string())?;
            let root = context
                .plugin
                .settings
                .as_deref()
                .ok_or("PHENIX_CONFIG_DIR is not configured")?;
            let response = config_command(root, command)?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        Err(format!("unsupported SDK service: {service}"))
    }
}

fn session_command(
    context: &SdkRuntimeContext<'_, '_, '_>,
    command: SdkSessionCommand,
) -> Result<SdkSessionResponse, String> {
    match command {
        SdkSessionCommand::Open { id, agent } => open_session(context, id, agent),
    }
}

fn config_command(root: &Path, command: SdkConfigCommand) -> Result<SdkConfigResponse, String> {
    match command {
        SdkConfigCommand::Read { path } => {
            let path = config_path(root, &path)?;
            let content = fs::read(&path).map_err(|error| {
                format!("failed to read config file {}: {error}", path.display())
            })?;
            Ok(SdkConfigResponse::File {
                content: content.into(),
            })
        }
    }
}

fn config_path(root: &Path, relative: &SdkConfigPath) -> Result<PathBuf, String> {
    let root = fs::canonicalize(root)
        .map_err(|error| format!("failed to resolve config root {}: {error}", root.display()))?;
    if !root.is_dir() {
        return Err(format!(
            "config root is not a directory: {}",
            root.display()
        ));
    }
    let path = root.join(relative.as_path());
    let path = fs::canonicalize(&path)
        .map_err(|error| format!("failed to resolve config file {}: {error}", path.display()))?;
    if !path.starts_with(&root) {
        return Err(format!(
            "config path escapes config root: {}",
            relative.as_path().display()
        ));
    }
    Ok(path)
}

fn open_session(
    context: &SdkRuntimeContext<'_, '_, '_>,
    id: String,
    agent: Option<String>,
) -> Result<SdkSessionResponse, String> {
    let id = SessionId::parse(id).map_err(str::to_owned)?;
    let option_context = option_context(id.as_str(), agent)?;
    let existing = match context
        .sdk
        .sessions
        .invoke(&SessionCommand::Get { id: id.clone() })
        .map_err(|error| error.to_string())?
    {
        SessionResponse::Session { session } => session,
        response => return Err(format!("unexpected session lookup response: {response:?}")),
    };

    if let Some(session) = existing {
        if resolve_bool(context, "session.reuse_existing", &option_context)? {
            return Ok(SdkSessionResponse::Opened {
                session,
                created: false,
            });
        }
        return Err(format!(
            "session already exists and reuse is disabled: {id}"
        ));
    }

    if !resolve_bool(context, "session.auto_create", &option_context)? {
        return Err(format!(
            "session does not exist and auto-create is disabled: {id}"
        ));
    }

    match context
        .sdk
        .sessions
        .invoke(&SessionCommand::Create {
            session: phenix_sdk::SessionRecord::new(id),
        })
        .map_err(|error| error.to_string())?
    {
        SessionResponse::Created { session } => Ok(SdkSessionResponse::Opened {
            session,
            created: true,
        }),
        response => Err(format!("unexpected session create response: {response:?}")),
    }
}

fn tool_command(
    context: &SdkRuntimeContext<'_, '_, '_>,
    command: SdkToolCommand,
) -> Result<SdkToolResponse, String> {
    match command {
        SdkToolCommand::Register {
            id,
            service,
            required_capabilities,
        } => {
            let response = context
                .sdk
                .execution
                .invoke::<ExecutionCommand, ExecutionResponse>(
                    &ExecutionCommand::RegisterCallable {
                        id,
                        service,
                        required_authority: ExecutionAuthority::new(required_capabilities),
                    },
                )
                .map_err(|error| error.to_string())?;
            let ExecutionResponse::Callable { callable } = response else {
                return Err("unexpected execution response while registering SDK tool".into());
            };
            Ok(SdkToolResponse::Tool {
                tool: SdkTool {
                    id: callable.id,
                    service: callable.service,
                    required_capabilities: callable.required_authority.capabilities,
                },
            })
        }
        SdkToolCommand::Invoke {
            execution_id,
            id,
            input,
        } => {
            let response = context
                .sdk
                .execution
                .invoke::<ExecutionCommand, ExecutionResponse>(&ExecutionCommand::InvokeCallable {
                    execution_id,
                    callable_id: id,
                    input,
                })
                .map_err(|error| error.to_string())?;
            let ExecutionResponse::Invocation { output } = response else {
                return Err("unexpected execution response while invoking SDK tool".into());
            };
            Ok(SdkToolResponse::Output { output })
        }
    }
}

fn skill_command(
    context: &SdkRuntimeContext<'_, '_, '_>,
    command: SdkSkillCommand,
) -> Result<SdkSkillResponse, String> {
    match command {
        SdkSkillCommand::Register { id, content } => {
            require_non_empty("skill id", &id)?;
            let resource_id = skill_resource_id(&id)?;
            let response = invoke_context(
                context,
                ContextCommand::Register {
                    resource_id,
                    kind: ContextResourceKind::Skill,
                    source: format!("sdk:{id}"),
                    scope: ContextScope::Workspace,
                    content,
                },
            )?;
            let ContextResponse::Registered { resource } = response else {
                return Err("unexpected context response while registering SDK skill".into());
            };
            Ok(SdkSkillResponse::Skill {
                skill: Some(skill_from_revision(resource)),
            })
        }
        SdkSkillCommand::Get { id } => {
            require_non_empty("skill id", &id)?;
            let Some(descriptor) = find_skill_descriptor(context, &id)? else {
                return Ok(SdkSkillResponse::Skill { skill: None });
            };
            let response = invoke_context(
                context,
                ContextCommand::Get {
                    resource_id: descriptor.resource_id,
                    revision: descriptor.revision,
                },
            )?;
            let ContextResponse::Resource { resource } = response else {
                return Err("unexpected context response while reading SDK skill".into());
            };
            Ok(SdkSkillResponse::Skill {
                skill: resource.map(skill_from_revision),
            })
        }
        SdkSkillCommand::List => {
            let response = invoke_context(context, ContextCommand::List)?;
            let ContextResponse::Resources { descriptors } = response else {
                return Err("unexpected context response while listing SDK skills".into());
            };
            Ok(SdkSkillResponse::Skills {
                skills: descriptors
                    .into_iter()
                    .filter(|descriptor| descriptor.kind == ContextResourceKind::Skill)
                    .map(skill_summary)
                    .collect(),
            })
        }
    }
}

fn invoke_context(
    context: &SdkRuntimeContext<'_, '_, '_>,
    command: ContextCommand,
) -> Result<ContextResponse, String> {
    context
        .sdk
        .context
        .invoke_projected(&command)
        .map_err(|error| error.to_string())
}

fn find_skill_descriptor(
    context: &SdkRuntimeContext<'_, '_, '_>,
    id: &str,
) -> Result<Option<ContextDescriptor>, String> {
    let response = invoke_context(context, ContextCommand::List)?;
    let ContextResponse::Resources { descriptors } = response else {
        return Err("unexpected context response while locating SDK skill".into());
    };
    Ok(descriptors.into_iter().find(|descriptor| {
        descriptor.kind == ContextResourceKind::Skill && skill_id(&descriptor.resource_id) == id
    }))
}

fn skill_from_revision(resource: ContextResourceRevision) -> SdkSkill {
    SdkSkill {
        id: skill_id(&resource.descriptor.resource_id).to_owned(),
        content: resource.content,
    }
}

fn skill_summary(descriptor: ContextDescriptor) -> SdkSkillSummary {
    SdkSkillSummary {
        id: skill_id(&descriptor.resource_id).to_owned(),
        revision: descriptor.revision.to_string(),
        source: descriptor.source,
    }
}

fn skill_resource_id(id: &str) -> Result<ContextResourceId, String> {
    ContextResourceId::parse(format!("skill:{id}")).map_err(str::to_owned)
}

fn skill_id(resource_id: &ContextResourceId) -> &str {
    resource_id
        .as_str()
        .strip_prefix("skill:")
        .unwrap_or(resource_id.as_str())
}

fn require_non_empty(name: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{name} must not be empty"));
    }
    Ok(())
}

fn option_context(id: &str, agent: Option<String>) -> Result<OptionContext, String> {
    Ok(OptionContext {
        session: Some(OptionSubjectId::parse(id).map_err(|error| error.to_owned())?),
        agent: agent
            .map(OptionSubjectId::parse)
            .transpose()
            .map_err(|error| error.to_owned())?,
    })
}

fn resolve_bool(
    context: &SdkRuntimeContext<'_, '_, '_>,
    key: &str,
    option_context: &OptionContext,
) -> Result<bool, String> {
    let key = OptionKey::parse(key).expect("static SDK option key is valid");
    let response = context
        .sdk
        .options
        .invoke_fallible_projected::<OptionCommand, OptionResponse, String>(
            &OptionCommand::Resolve {
                key: key.clone(),
                context: option_context.clone(),
            },
        )
        .map_err(|error| error.to_string())?;
    let OptionResponse::Value { option } = response else {
        return Err(format!("unexpected option resolution response for {key}"));
    };
    match option.value {
        OptionValue::Bool(value) => Ok(value),
        value => Err(format!("option {key} must resolve to bool, got {value:?}")),
    }
}

#[cfg(test)]
mod tests;
