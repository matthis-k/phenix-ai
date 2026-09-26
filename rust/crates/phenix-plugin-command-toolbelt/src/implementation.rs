use phenix_core::{
    Authority, CapabilityId, ComponentInterface, PluginContext, PluginExecution, PluginHost,
    PluginId, PluginInstance, PluginManifest, SdkClient, ServiceContribution, ServiceId,
};
use phenix_sdk::{WorkspaceCommand, WorkspaceInterface, WorkspaceResponse};
use phenix_sdk_macros::PhenixValue;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fmt::{self, Display, Formatter},
};

pub const CLI_DISCOVER_SERVICE: &str = "phenix.command-toolbelt.discover@1";
pub const CLI_VERSION_SERVICE: &str = "phenix.command-toolbelt.version@1";
pub const CLI_AUTH_STATE_SERVICE: &str = "phenix.command-toolbelt.auth-state@1";
const CLI_PLUGIN: &str = "phenix.command-toolbelt";
const WORKSPACE_PLUGIN: &str = "phenix.workspace";
const WORKSPACE_SHELL: &str = "workspace.shell";
const SUPPORTED: &[&str] = &["git", "gh", "jj", "rg", "fd", "jq", "nix", "cargo"];

struct CliSdk<'host, 'runtime> {
    workspace: SdkClient<'host, 'runtime, WorkspaceInterface>,
}

type CliContext<'host, 'runtime> = PluginContext<'host, 'runtime, CliSdk<'host, 'runtime>>;

fn context<'host, 'runtime>(host: &'host PluginHost<'runtime>) -> CliContext<'host, 'runtime> {
    PluginContext::new(
        host,
        CliSdk {
            workspace: SdkClient::new(host, crate::cli_component_id()),
        },
        (),
        (),
    )
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct CliName(String);

impl CliName {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        value.into().try_into()
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CliName {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if SUPPORTED.contains(&value.as_str()) {
            Ok(Self(value))
        } else {
            Err(format!("unsupported CLI probe target: {value}"))
        }
    }
}

impl Display for CliName {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl phenix_core::ValueCodec for CliName {
    fn phenix_type() -> phenix_core::Type {
        phenix_core::Type::String
    }

    fn to_value(&self) -> phenix_core::PhenixValue {
        phenix_core::PhenixValue::String(self.0.clone())
    }

    fn from_value(value: &phenix_core::PhenixValue) -> Result<Self, phenix_core::ValueError> {
        let value = String::try_from(phenix_core::Exact(value))?;
        Self::try_from(value).map_err(phenix_core::ValueError::InvalidValue)
    }

    fn project_from_value(
        value: &phenix_core::PhenixValue,
    ) -> Result<Self, phenix_core::ValueError> {
        let value = String::try_from(phenix_core::Project(value))?;
        Self::try_from(value).map_err(phenix_core::ValueError::InvalidValue)
    }
}

impl From<&CliName> for phenix_core::PhenixValue {
    fn from(value: &CliName) -> Self {
        <CliName as phenix_core::ValueCodec>::to_value(value)
    }
}

impl<'value> TryFrom<phenix_core::Exact<&'value phenix_core::PhenixValue>> for CliName {
    type Error = phenix_core::ValueError;

    fn try_from(
        value: phenix_core::Exact<&'value phenix_core::PhenixValue>,
    ) -> Result<Self, Self::Error> {
        <Self as phenix_core::ValueCodec>::from_value(value.0)
    }
}

impl<'value> TryFrom<phenix_core::Project<&'value phenix_core::PhenixValue>> for CliName {
    type Error = phenix_core::ValueError;

    fn try_from(
        value: phenix_core::Project<&'value phenix_core::PhenixValue>,
    ) -> Result<Self, Self::Error> {
        <Self as phenix_core::ValueCodec>::project_from_value(value.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct CliProbeRequest {
    pub name: CliName,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(rename_all = "snake_case")]
pub enum CliAvailability {
    Available,
    Unavailable,
    Limited,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(rename_all = "snake_case")]
pub enum CliAuthState {
    Unsupported,
    Unknown,
    Authenticated,
    Unauthenticated,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct CliDescriptor {
    pub name: CliName,
    pub availability: CliAvailability,
    pub executable_identity: Option<String>,
    pub version: Option<String>,
    pub auth_state: Option<CliAuthState>,
    pub supported_probe_capabilities: BTreeSet<String>,
    pub observation_provenance: String,
}

#[must_use]
pub fn cli_manifest(maximum_authority: Authority) -> PluginManifest {
    let shell = Authority::new([capability(WORKSPACE_SHELL)]);
    PluginManifest {
        id: plugin(CLI_PLUGIN),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: vec![plugin(WORKSPACE_PLUGIN)],
        services: vec![
            contribution(cli_discover_service()),
            contribution(cli_version_service()),
            contribution(cli_auth_state_service()),
        ],
        resource_namespaces: Vec::new(),
        maximum_authority: maximum_authority.attenuate(&shell),
    }
}

#[must_use]
pub fn cli_factory() -> Box<dyn PluginInstance> {
    Box::new(CliPlugin)
}

#[must_use]
pub fn cli_discover_service() -> ServiceId {
    service(CLI_DISCOVER_SERVICE)
}

#[must_use]
pub fn cli_version_service() -> ServiceId {
    service(CLI_VERSION_SERVICE)
}

#[must_use]
pub fn cli_auth_state_service() -> ServiceId {
    service(CLI_AUTH_STATE_SERVICE)
}

fn contribution(service: ServiceId) -> ServiceContribution {
    ServiceContribution {
        role: phenix_core::ServiceRole::Terminal,
        service,
        priority: 100,
        required_authority: Authority::default(),
    }
}

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).expect("static plugin id is valid")
}

fn service(value: &str) -> ServiceId {
    ServiceId::parse(value).expect("static service id is valid")
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).expect("static capability is valid")
}

fn probe_capabilities(name: &CliName) -> BTreeSet<String> {
    let mut capabilities = BTreeSet::from(["discover".to_owned(), "version".to_owned()]);
    if name.as_str() == "gh" {
        capabilities.insert("auth_state".to_owned());
    }
    capabilities
}

fn descriptor(name: &CliName, availability: CliAvailability) -> CliDescriptor {
    CliDescriptor {
        name: name.clone(),
        availability,
        executable_identity: None,
        version: None,
        auth_state: None,
        supported_probe_capabilities: probe_capabilities(name),
        observation_provenance: "workspace-shell-probe".to_owned(),
    }
}

fn shell(context: &CliContext<'_, '_>, command: String) -> Result<WorkspaceResponse, String> {
    context
        .sdk
        .workspace
        .invoke_projected::<WorkspaceCommand, WorkspaceResponse>(&WorkspaceCommand::Shell {
            command,
        })
        .map_err(|error| error.to_string())
}

fn discover(context: &CliContext<'_, '_>, name: &CliName) -> Result<CliDescriptor, String> {
    let response = match shell(context, format!("command -v -- {name}")) {
        Ok(response) => response,
        Err(error) if error.contains(WORKSPACE_SHELL) || error.contains("authority") => {
            return Ok(descriptor(name, CliAvailability::Limited));
        }
        Err(error) => return Err(error),
    };
    let WorkspaceResponse::Process {
        exit_code, stdout, ..
    } = response
    else {
        return Err("workspace shell returned a non-process response".into());
    };
    if exit_code != 0 {
        return Ok(descriptor(name, CliAvailability::Unavailable));
    }
    let mut result = descriptor(name, CliAvailability::Available);
    result.executable_identity = stdout
        .lines()
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    Ok(result)
}

fn version(context: &CliContext<'_, '_>, name: &CliName) -> Result<CliDescriptor, String> {
    let mut result = discover(context, name)?;
    if result.availability != CliAvailability::Available {
        return Ok(result);
    }
    let response = shell(context, format!("{name} --version"))?;
    let WorkspaceResponse::Process {
        exit_code,
        stdout,
        stderr,
    } = response
    else {
        return Err("workspace shell returned a non-process response".into());
    };
    if exit_code == 0 {
        result.version = stdout
            .lines()
            .chain(stderr.lines())
            .find(|line| !line.trim().is_empty())
            .map(|line| line.trim().to_owned());
    }
    Ok(result)
}

fn auth_state(context: &CliContext<'_, '_>, name: &CliName) -> Result<CliDescriptor, String> {
    let mut result = discover(context, name)?;
    if name.as_str() != "gh" {
        result.auth_state = Some(CliAuthState::Unsupported);
        return Ok(result);
    }
    if result.availability != CliAvailability::Available {
        result.auth_state = Some(match result.availability {
            CliAvailability::Limited => CliAuthState::Unknown,
            CliAvailability::Unavailable => CliAuthState::Unauthenticated,
            CliAvailability::Available => unreachable!(),
        });
        return Ok(result);
    }
    let response = shell(context, "gh auth status >/dev/null 2>&1".to_owned())?;
    let WorkspaceResponse::Process { exit_code, .. } = response else {
        return Err("workspace shell returned a non-process response".into());
    };
    result.auth_state = Some(if exit_code == 0 {
        CliAuthState::Authenticated
    } else {
        CliAuthState::Unauthenticated
    });
    Ok(result)
}

struct CliPlugin;

impl PluginInstance for CliPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let context = context(host);
        let interface = if service == &cli_discover_service() {
            crate::CliDiscoverInterface::interface_id()
        } else if service == &cli_version_service() {
            crate::CliVersionInterface::interface_id()
        } else if service == &cli_auth_state_service() {
            crate::CliAuthStateInterface::interface_id()
        } else {
            return Err(format!("unsupported CLI service: {service}"));
        };
        let request = context
            .kernel
            .decode_projected::<CliProbeRequest>(&interface, input)
            .map_err(|error| error.to_string())?;
        let result = if service == &cli_discover_service() {
            discover(&context, &request.name)?
        } else if service == &cli_version_service() {
            version(&context, &request.name)?
        } else {
            auth_state(&context, &request.name)?
        };
        context
            .kernel
            .encode_value(&result)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        Kernel, KernelConfig, PhenixValue, Project, ResolvedHarness, ResolvedHarnessActivation,
    };
    use phenix_plugin_environment_local::{
        local_environment_component_manifest, local_environment_factory_for,
        local_environment_manifest,
    };
    use phenix_plugin_workspace::{
        workspace_component_manifest, workspace_factory_for, workspace_manifest,
    };
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_workspace() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("phenix-cli-plugin-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn kernel(authority: Authority) -> Kernel {
        let root = temp_workspace();
        let workspace = workspace_manifest();
        let workspace_id = workspace.id.clone();
        let environment = local_environment_manifest();
        let environment_id = environment.id.clone();
        let cli = cli_manifest(authority.clone());
        let cli_id = cli.id.clone();
        let resolved = ResolvedHarness::resolve(
            [environment.clone(), workspace.clone(), cli.clone()],
            [
                local_environment_component_manifest(),
                workspace_component_manifest(),
                crate::cli_component_manifest(authority.clone()),
            ],
            [],
            &authority,
        )
        .unwrap();
        let mut kernel = Kernel::new(KernelConfig::new([environment, workspace, cli]).unwrap());
        kernel.activate_resolved_harness(&resolved).unwrap();
        let environment_root = root.clone();
        kernel
            .register_embedded_factory(environment_id, move || {
                local_environment_factory_for(environment_root.clone())
            })
            .unwrap();
        kernel
            .register_embedded_factory(workspace_id, move || workspace_factory_for(root.clone()))
            .unwrap();
        kernel
            .register_embedded_factory(cli_id, cli_factory)
            .unwrap();
        kernel.activate_all().unwrap();
        kernel
    }

    fn invoke(
        kernel: &mut Kernel,
        service: ServiceId,
        name: CliName,
        authority: Authority,
    ) -> Result<CliDescriptor, String> {
        let request = CliProbeRequest { name };
        let input = serde_json::to_vec(&PhenixValue::from(&request)).unwrap();
        let output = kernel
            .invoke(&service, &input, &authority, None)
            .map_err(|error| error.to_string())?;
        {
            let output: PhenixValue =
                serde_json::from_slice(&output).map_err(|error| error.to_string())?;
            CliDescriptor::try_from(Project(&output)).map_err(|error| error.to_string())
        }
    }

    #[test]
    fn manifest_depends_on_workspace_and_cannot_self_grant_shell() {
        let denied = cli_manifest(Authority::default());
        assert_eq!(denied.dependencies, vec![plugin(WORKSPACE_PLUGIN)]);
        assert!(!denied
            .maximum_authority
            .permits(&capability(WORKSPACE_SHELL)));

        let granted = cli_manifest(Authority::new([capability(WORKSPACE_SHELL)]));
        assert!(granted
            .maximum_authority
            .permits(&capability(WORKSPACE_SHELL)));
    }

    #[test]
    fn unsupported_target_is_rejected_while_parsing() {
        let value = PhenixValue::String("git;touch-pwned".into());
        let error = CliName::try_from(Project(&value)).unwrap_err();
        assert!(error.to_string().contains("unsupported CLI probe target"));
    }

    #[test]
    fn missing_shell_authority_returns_a_typed_limited_result() {
        let mut kernel = kernel(Authority::default());
        let result = invoke(
            &mut kernel,
            cli_discover_service(),
            CliName::parse("git").unwrap(),
            Authority::default(),
        )
        .unwrap();
        assert_eq!(result.availability, CliAvailability::Limited);
        assert_eq!(result.executable_identity, None);
    }

    #[test]
    fn non_gh_auth_probe_is_typed_unsupported() {
        let shell = Authority::new([capability(WORKSPACE_SHELL)]);
        let mut kernel = kernel(shell.clone());
        let result = invoke(
            &mut kernel,
            cli_auth_state_service(),
            CliName::parse("git").unwrap(),
            shell,
        )
        .unwrap();
        assert_eq!(result.auth_state, Some(CliAuthState::Unsupported));
    }
}
