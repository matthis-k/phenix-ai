#![forbid(unsafe_code)]

mod runtime_config;

use phenix_core::{
    ComponentId, ComponentProcessArgument, InterfaceId, Key, LayerPolicy, LocalPersistence,
    PhenixValue, PluginExecution, PluginId, PluginManifest, ServiceId,
};
use phenix_harness::{
    PhenixRuntime, PhenixRuntimeBuilder, application::serve_configured_application,
    default_suite_authority, invocation_defaults_manifest,
};
use phenix_plugin_catalog::{
    OptionStartupPrecedence, adapter_acp_manifest, advanced_agent_configuration_manifest,
    agent_loop_manifest, artifact_manifest, basic_agent_configuration_manifest,
    basic_context_manifest, basic_model_manifest, basic_product_configuration_manifest,
    basic_skills_manifest, basic_tools_manifest, benchmark_outcome_manifest, cli_manifest,
    common_provider_definitions, context_manifest, debug_manifest, efficiency_evaluation_manifest,
    execution_manifest, expand_profile_defaults, frontend_manifest,
    full_product_configuration_manifest, hook_manifest,
    job_manifest, language_manifest, local_environment_manifest, memory_manifest,
    model_routing_manifest, openai_codex_manifest, options_manifest, planning_manifest,
    providers_manifest, repository_worker_manifest, sdk_manifest, session_manifest,
    session_tree_manifest, step_runner_manifest, workspace_manifest,
};
use phenix_runtime::serve_jsonl;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

/// Frontend protocol selected for this harness process.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Mode {
    Acp,
    #[default]
    Jsonl,
}

impl Mode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "acp" => Ok(Self::Acp),
            "jsonl" => Ok(Self::Jsonl),
            _ => Err(format!("unknown mode: {value}; expected acp or jsonl")),
        }
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
struct Cli {
    help: bool,
    list_services: bool,
    mode: Mode,
    profile: Option<String>,
    enable_plugins: BTreeSet<String>,
    disable_plugins: BTreeSet<String>,
    config_file: Option<PathBuf>,
    provider_policy_file: Option<PathBuf>,
    provider_bindings: Vec<(InterfaceId, ComponentId)>,
    disabled_providers: Vec<(InterfaceId, ComponentId)>,
    plugin_arguments: Vec<String>,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("phenix-harness: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    let cli = parse_cli(env::args().skip(1))?;
    if cli.help {
        print_help();
        return Ok(());
    }
    let config_file = cli
        .config_file
        .clone()
        .or_else(|| env::var_os("PHENIX_CONFIG_FILE").map(PathBuf::from));
    let composition = load_portable_configuration(config_file.as_deref())?;

    let state = state_path()?;
    if let Some(parent) = state.parent() {
        fs::create_dir_all(parent)?;
    }
    let persistence = LocalPersistence::open(&state)?;
    let excluded_defaults = effective_disabled_plugins(&cli, &composition);
    let mut builder = match configured_first_party_plugins(&cli, &composition)? {
        Some(enabled) => PhenixRuntimeBuilder::with_selected_suite_excluding(&enabled, &excluded_defaults)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?,
        None => PhenixRuntimeBuilder::with_default_suite()?,
    };
    for package in configured_plugin_packages()? {
        add_packaged_plugin(&mut builder, &package)?;
    }
    apply_configured_layer_policy(&mut builder, &composition)?;
    apply_configured_provider_policy(&mut builder, &cli, &composition)?;
    let mut harness = builder.build_with_persistence(persistence)?;
    let process_arguments = resolve_process_arguments(
        &cli.plugin_arguments,
        harness.resolved_generation().process_arguments(),
    )
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    harness.activate()?;
    if let Some(path) = env::var_os("PHENIX_DEFAULT_CONFIG_DIR") {
        runtime_config::apply_default_config_directory(&mut harness, Path::new(&path))?;
    }
    let config_directory = env::var_os("PHENIX_CONFIG_DIR").map(PathBuf::from);
    let nix_settings = env::var_os("PHENIX_NIX_SETTINGS").map(PathBuf::from);
    if config_directory.is_some() || nix_settings.is_some() {
        let precedence = match env::var("PHENIX_SETTINGS_PRECEDENCE") {
            Ok(value) if value == "file" => OptionStartupPrecedence::File,
            Ok(value) if value == "nix" => OptionStartupPrecedence::Nix,
            Err(env::VarError::NotPresent) => OptionStartupPrecedence::Nix,
            Ok(value) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid PHENIX_SETTINGS_PRECEDENCE: {value}"),
                )
                .into());
            }
            Err(error) => return Err(error.into()),
        };
        runtime_config::apply_startup_settings(
            &mut harness,
            config_directory.as_deref(),
            nix_settings.as_deref(),
            precedence,
        )?;
    }
    apply_process_arguments(&mut harness, &process_arguments)?;

    if cli.list_services {
        let plugins = harness
            .kernel()
            .config()
            .manifests()
            .map(|manifest| manifest.id.as_str().to_owned())
            .collect::<Vec<_>>();
        let mut services = harness
            .kernel()
            .config()
            .manifests()
            .flat_map(|manifest| manifest.services.iter())
            .map(|contribution| contribution.service.as_str().to_owned())
            .collect::<Vec<_>>();
        services.sort();
        services.dedup();
        println!(
            "{}",
            serde_json::to_string(&json!({ "plugins": plugins, "services": services }))?
        );
        return Ok(());
    }

    match cli.mode {
        Mode::Acp => serve_configured_application(harness).await?,
        Mode::Jsonl => {
            let stdin = io::stdin();
            let stdout = io::stdout();
            let mut stdout = io::BufWriter::new(stdout.lock());
            serve_jsonl(
                harness.kernel_mut(),
                &default_suite_authority(),
                stdin.lock(),
                &mut stdout,
            )?;
        }
    }
    Ok(())
}

fn print_help() {
    println!(
        "phenix [OPTIONS]\n\nRuns the packaged Phenix composition. The default mode is jsonl.\n\nOptions:\n  --mode MODE           Frontend mode: jsonl or acp\n  --list-services       List active plugins and services as JSON\n  --profile ID          Select a Phenix product/profile ID\n  --enable-plugin ID    Enable a bundled plugin for this process\n  --disable-plugin ID   Disable a bundled plugin for this process\n  --config FILE         Load portable Phenix composition JSON\n  --provider-policy FILE    Load portable provider policy JSON\n  --bind-provider A=B       Bind interface A to provider component B\n  --disable-provider A=B    Exclude provider component B for interface A\n  -h, --help            Print help\n\nLoaded plugins may declare additional long options."
    );
}

fn parse_cli(args: impl IntoIterator<Item = String>) -> Result<Cli, String> {
    let mut cli = Cli::default();
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--help" | "-h" => cli.help = true,
            "--list-services" => cli.list_services = true,
            "--mode" => {
                let mode = args
                    .next()
                    .ok_or_else(|| "--mode requires acp or jsonl".to_owned())?;
                cli.mode = Mode::parse(&mode)?;
            }
            _ if argument.starts_with("--mode=") => {
                let mode = argument.strip_prefix("--mode=").expect("prefix checked");
                if mode.is_empty() {
                    return Err("--mode requires acp or jsonl".into());
                }
                cli.mode = Mode::parse(mode)?;
            }
            "--profile" => {
                let id = args
                    .next()
                    .ok_or_else(|| "--profile requires a plugin/profile id".to_owned())?;
                if id.is_empty() {
                    return Err("--profile requires a plugin/profile id".into());
                }
                cli.profile = Some(id);
            }
            _ if argument.starts_with("--profile=") => {
                let id = argument.strip_prefix("--profile=").expect("prefix checked");
                if id.is_empty() {
                    return Err("--profile requires a plugin/profile id".into());
                }
                cli.profile = Some(id.to_owned());
            }
            "--config" => {
                let path = args
                    .next()
                    .ok_or_else(|| "--config requires a file path".to_owned())?;
                if path.is_empty() {
                    return Err("--config requires a file path".into());
                }
                cli.config_file = Some(PathBuf::from(path));
            }
            _ if argument.starts_with("--config=") => {
                let path = argument.strip_prefix("--config=").expect("prefix checked");
                if path.is_empty() {
                    return Err("--config requires a file path".into());
                }
                cli.config_file = Some(PathBuf::from(path));
            }
            "--provider-policy" => {
                let path = args
                    .next()
                    .ok_or_else(|| "--provider-policy requires a file path".to_owned())?;
                if path.is_empty() {
                    return Err("--provider-policy requires a file path".into());
                }
                cli.provider_policy_file = Some(PathBuf::from(path));
            }
            _ if argument.starts_with("--provider-policy=") => {
                let path = argument
                    .strip_prefix("--provider-policy=")
                    .expect("prefix checked");
                if path.is_empty() {
                    return Err("--provider-policy requires a file path".into());
                }
                cli.provider_policy_file = Some(PathBuf::from(path));
            }
            "--bind-provider" => {
                let binding = args
                    .next()
                    .ok_or_else(|| "--bind-provider requires INTERFACE=COMPONENT".to_owned())?;
                cli.provider_bindings.push(parse_provider_pair(&binding)?);
            }
            _ if argument.starts_with("--bind-provider=") => {
                let binding = argument
                    .strip_prefix("--bind-provider=")
                    .expect("prefix checked");
                cli.provider_bindings.push(parse_provider_pair(binding)?);
            }
            "--disable-provider" => {
                let binding = args
                    .next()
                    .ok_or_else(|| "--disable-provider requires INTERFACE=COMPONENT".to_owned())?;
                cli.disabled_providers.push(parse_provider_pair(&binding)?);
            }
            _ if argument.starts_with("--disable-provider=") => {
                let binding = argument
                    .strip_prefix("--disable-provider=")
                    .expect("prefix checked");
                cli.disabled_providers.push(parse_provider_pair(binding)?);
            }
            "--enable-plugin" => {
                let plugin = args
                    .next()
                    .ok_or_else(|| "--enable-plugin requires a plugin id".to_owned())?;
                if plugin.is_empty() {
                    return Err("--enable-plugin requires a plugin id".into());
                }
                cli.enable_plugins.insert(plugin);
            }
            "--disable-plugin" => {
                let plugin = args
                    .next()
                    .ok_or_else(|| "--disable-plugin requires a plugin id".to_owned())?;
                if plugin.is_empty() {
                    return Err("--disable-plugin requires a plugin id".into());
                }
                cli.disable_plugins.insert(plugin);
            }
            _ if argument.starts_with("--enable-plugin=") => {
                let plugin = argument
                    .strip_prefix("--enable-plugin=")
                    .expect("prefix checked");
                if plugin.is_empty() {
                    return Err("--enable-plugin requires a plugin id".into());
                }
                cli.enable_plugins.insert(plugin.to_owned());
            }
            _ if argument.starts_with("--disable-plugin=") => {
                let plugin = argument
                    .strip_prefix("--disable-plugin=")
                    .expect("prefix checked");
                if plugin.is_empty() {
                    return Err("--disable-plugin requires a plugin id".into());
                }
                cli.disable_plugins.insert(plugin.to_owned());
            }
            _ => cli.plugin_arguments.push(argument),
        }
    }
    Ok(cli)
}

fn parse_provider_pair(value: &str) -> Result<(InterfaceId, ComponentId), String> {
    let (interface, component) = value
        .split_once('=')
        .ok_or_else(|| "provider binding requires INTERFACE=COMPONENT".to_owned())?;
    let interface = InterfaceId::parse(interface)
        .map_err(|error| format!("invalid provider interface {interface}: {error}"))?;
    let component = ComponentId::parse(component)
        .map_err(|error| format!("invalid provider component {component}: {error}"))?;
    Ok((interface, component))
}

#[derive(Clone, Debug)]
struct ProcessArgumentInvocation {
    argument: ComponentProcessArgument,
    value: Option<String>,
}

fn resolve_process_arguments(
    raw: &[String],
    arguments: &[ComponentProcessArgument],
) -> Result<Vec<ProcessArgumentInvocation>, String> {
    const CORE_ARGUMENTS: &[&str] = &[
        "--help",
        "--list-services",
        "--mode",
        "--enable-plugin",
        "--disable-plugin",
        "--config",
        "--profile",
        "--provider-policy",
        "--bind-provider",
        "--disable-provider",
    ];

    let mut declared = BTreeMap::new();
    for argument in arguments {
        if CORE_ARGUMENTS.contains(&argument.name.as_str()) {
            return Err(format!(
                "plugin process argument {} conflicts with a core argument",
                argument.name
            ));
        }
        declared.insert(argument.name.as_str(), (argument, argument.takes_value));
    }

    let mut resolved = Vec::new();
    let mut index = 0;
    while index < raw.len() {
        let argument = &raw[index];
        let (name, inline_value) = match argument.split_once('=') {
            Some((name, value)) => (name, Some(value.to_owned())),
            None => (argument.as_str(), None),
        };
        let Some((process_argument, takes_value)) = declared.get(name).copied() else {
            return Err(format!("unknown argument: {argument}"));
        };

        let value = match (takes_value, inline_value) {
            (false, Some(_)) => {
                return Err(format!(
                    "plugin process argument {name} does not take a value"
                ));
            }
            (false, None) => None,
            (true, Some(value)) => Some(value),
            (true, None) => {
                index += 1;
                let value = raw
                    .get(index)
                    .ok_or_else(|| format!("plugin process argument {name} requires a value"))?;
                Some(value.clone())
            }
        };

        resolved.push(ProcessArgumentInvocation {
            argument: process_argument.clone(),
            value,
        });
        index += 1;
    }

    Ok(resolved)
}

fn apply_process_arguments(
    harness: &mut PhenixRuntime,
    arguments: &[ProcessArgumentInvocation],
) -> Result<(), Box<dyn Error>> {
    for argument in arguments {
        let component = harness
            .resolved_generation()
            .components()
            .iter()
            .find(|component| component.id == argument.argument.component)
            .ok_or_else(|| {
                format!(
                    "plugin process argument targets missing component {}",
                    argument.argument.component
                )
            })?;
        let owner = component.owner.clone();
        let service = ServiceId::parse(argument.argument.interface.as_str())?;
        let input = PhenixValue::Table(BTreeMap::from([
            (
                Key::parse("name").expect("static process argument field is valid"),
                PhenixValue::String(argument.argument.name.clone()),
            ),
            (
                Key::parse("value").expect("static process argument field is valid"),
                PhenixValue::Option(
                    argument
                        .value
                        .clone()
                        .map(PhenixValue::String)
                        .map(Box::new),
                ),
            ),
        ]));
        let encoded = serde_json::to_vec(&input)?;
        harness.kernel_mut().invoke_component(
            &argument.argument.component,
            &service,
            &encoded,
            &argument.argument.required_authority,
            &owner,
        )?;
    }
    Ok(())
}

fn first_party_plugins() -> Vec<(PluginManifest, bool)> {
    let authority = default_suite_authority();
    let mut plugins = vec![
        (advanced_agent_configuration_manifest(), false),
        (basic_agent_configuration_manifest(), false),
        (basic_product_configuration_manifest(), false),
        (full_product_configuration_manifest(), false),
        (providers_manifest(), false),
        (openai_codex_manifest(), true),
        (adapter_acp_manifest(), false),
        (repository_worker_manifest(), true),
        (session_manifest(), true),
        (session_tree_manifest(), true),
        (artifact_manifest(), true),
        (cli_manifest(authority.clone()), true),
        (context_manifest(), true),
        (execution_manifest(authority.clone()), true),
        (efficiency_evaluation_manifest(), true),
        (benchmark_outcome_manifest(), false),
        (agent_loop_manifest(authority.clone()), true),
        (language_manifest(), true),
        (memory_manifest(), true),
        (planning_manifest(), true),
        (local_environment_manifest(), true),
        (workspace_manifest(), true),
        (model_routing_manifest(authority.clone()), true),
        (step_runner_manifest(authority.clone()), true),
        (job_manifest(), true),
        (frontend_manifest(authority.clone()), true),
        (hook_manifest(authority.clone()), true),
        (debug_manifest(authority.clone()), true),
        (options_manifest(), true),
        (invocation_defaults_manifest(authority.clone()), true),
        (sdk_manifest(authority), true),
        (basic_model_manifest(), false),
        (basic_tools_manifest(), false),
        (basic_skills_manifest(), false),
        (basic_context_manifest(), false),
    ];
    plugins.extend(
        common_provider_definitions()
            .into_iter()
            .map(|provider| (provider.manifest(), true)),
    );
    plugins
}

fn configured_first_party_plugins(
    cli: &Cli,
    config: &PortableCompositionConfig,
) -> Result<Option<BTreeSet<String>>, Box<dyn Error>> {
    let configured = env::var_os("PHENIX_ENABLED_PLUGINS");
    let configured = configured
        .map(OsString::into_string)
        .transpose()
        .map_err(|_| "PHENIX_ENABLED_PLUGINS must be valid UTF-8")?;
    resolve_configured_first_party_plugins(cli, config, configured.as_deref()).map_err(Into::into)
}

fn effective_disabled_plugins(
    cli: &Cli,
    config: &PortableCompositionConfig,
) -> BTreeSet<String> {
    let mut disabled = config.plugins.disable.clone();
    for id in &cli.enable_plugins {
        disabled.remove(id);
    }
    disabled.extend(cli.disable_plugins.iter().cloned());
    disabled
}

fn resolve_configured_first_party_plugins(
    cli: &Cli,
    config: &PortableCompositionConfig,
    environment_selection: Option<&str>,
) -> Result<Option<BTreeSet<String>>, String> {
    // Deployment environment supplies defaults; portable configuration may
    // choose a profile. Explicit command-line plugin options win over both.
    let base = cli
        .profile
        .as_deref()
        .or(config.profile.as_deref())
        .or(environment_selection);
    let mut selection = Cli::default();
    selection.enable_plugins = config.plugins.enable.clone();
    selection.disable_plugins = config.plugins.disable.clone();
    for id in &cli.enable_plugins {
        selection.disable_plugins.remove(id);
        selection.enable_plugins.insert(id.clone());
    }
    for id in &cli.disable_plugins {
        selection.enable_plugins.remove(id);
        selection.disable_plugins.insert(id.clone());
    }
    resolve_first_party_plugins(&selection, base)
}

fn resolve_first_party_plugins(
    cli: &Cli,
    configured: Option<&str>,
) -> Result<Option<BTreeSet<String>>, String> {
    let selection_requested =
        configured.is_some() || !cli.enable_plugins.is_empty() || !cli.disable_plugins.is_empty();
    if !selection_requested {
        return Ok(None);
    }

    let plugins = first_party_plugins();
    let available = plugins
        .iter()
        .map(|(manifest, _)| (manifest.id.as_str().to_owned(), manifest))
        .collect::<BTreeMap<_, _>>();
    let mut enabled = match configured {
        Some(value) => value
            .split(',')
            .filter(|entry| !entry.is_empty())
            .map(str::to_owned)
            .collect::<BTreeSet<_>>(),
        None => plugins
            .iter()
            .filter(|(_, enabled)| *enabled)
            .map(|(manifest, _)| manifest.id.as_str().to_owned())
            .collect(),
    };

    enabled.extend(cli.enable_plugins.iter().cloned());
    for plugin in &cli.disable_plugins {
        enabled.remove(plugin);
    }
    // Profile inheritance contributes replaceable defaults, not hard manifest
    // dependencies. This is Phenix-native configuration, not Nix expansion.
    enabled = expand_profile_defaults(&enabled, &cli.disable_plugins);

    let unknown = enabled
        .iter()
        .chain(cli.disable_plugins.iter())
        .filter(|id| !available.contains_key(*id))
        .cloned()
        .collect::<BTreeSet<_>>();
    if !unknown.is_empty() {
        return Err(format!(
            "unknown bundled plugin id(s): {}",
            unknown.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }

    let mut pending = enabled.iter().cloned().collect::<Vec<_>>();
    while let Some(plugin) = pending.pop() {
        let manifest = available
            .get(&plugin)
            .expect("validated enabled plugin exists in bundled catalog");
        for dependency in &manifest.dependencies {
            let dependency = dependency.as_str().to_owned();
            if cli.disable_plugins.contains(&dependency) {
                return Err(format!(
                    "bundled plugin {plugin} requires {dependency}, but {dependency} is explicitly disabled"
                ));
            }
            if !available.contains_key(&dependency) {
                return Err(format!(
                    "bundled plugin {plugin} depends on unavailable bundled plugin {dependency}"
                ));
            }
            if enabled.insert(dependency.clone()) {
                pending.push(dependency);
            }
        }
    }

    Ok(Some(enabled))
}

/// Deployment-independent composition input. This frontend only lowers
/// selections into the canonical Phenix runtime builder and resolver.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PortableCompositionConfig {
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    plugins: PortablePluginSelection,
    #[serde(default)]
    providers: PortableProviderPolicy,
    #[serde(default)]
    layers: Vec<ConfiguredLayerPolicy>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PortablePluginSelection {
    #[serde(default)]
    enable: BTreeSet<String>,
    #[serde(default)]
    disable: BTreeSet<String>,
}

fn load_portable_configuration(
    path: Option<&Path>,
) -> Result<PortableCompositionConfig, Box<dyn Error>> {
    match path {
        Some(path) => Ok(serde_json::from_str(&fs::read_to_string(path)?)?),
        None => Ok(PortableCompositionConfig::default()),
    }
}

/// One portable, intentionally minimal provider-selection document.
///
/// This is not an alternative graph resolver: it only lowers user decisions
/// into the existing Phenix provider composition policy.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PortableProviderPolicy {
    #[serde(default)]
    bind: BTreeMap<InterfaceId, ComponentId>,
    #[serde(default)]
    disable: BTreeMap<InterfaceId, BTreeSet<ComponentId>>,
}

fn apply_configured_provider_policy(
    builder: &mut PhenixRuntimeBuilder,
    cli: &Cli,
    config: &PortableCompositionConfig,
) -> Result<(), Box<dyn Error>> {
    for (interface, provider) in &config.providers.bind {
        builder.bind_provider(interface.clone(), provider.clone());
    }
    for (interface, providers) in &config.providers.disable {
        for provider in providers {
            builder.disable_provider(interface.clone(), provider.clone());
        }
    }
    if let Some(path) = &cli.provider_policy_file {
        let source = fs::read_to_string(path)?;
        let configured: PortableProviderPolicy = serde_json::from_str(&source)?;
        for (interface, provider) in configured.bind {
            builder.bind_provider(interface, provider);
        }
        for (interface, providers) in configured.disable {
            for provider in providers {
                builder.disable_provider(interface.clone(), provider);
            }
        }
    }
    // Explicit CLI arguments override bindings in the supplied file.
    for (interface, provider) in &cli.provider_bindings {
        builder.bind_provider(interface.clone(), provider.clone());
    }
    for (interface, provider) in &cli.disabled_providers {
        builder.disable_provider(interface.clone(), provider.clone());
    }
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfiguredLayerPolicy {
    service: String,
    plugin: String,
    priority: i32,
    #[serde(default)]
    required: bool,
    #[serde(default = "default_layer_enabled")]
    enabled: bool,
}

fn default_layer_enabled() -> bool {
    true
}

fn apply_configured_layer_policy(
    builder: &mut PhenixRuntimeBuilder,
    config: &PortableCompositionConfig,
) -> Result<(), Box<dyn Error>> {
    let mut policies = BTreeMap::<ServiceId, Vec<LayerPolicy>>::new();
    if let Some(value) = env::var_os("PHENIX_LAYER_POLICY") {
        let value = value
            .into_string()
            .map_err(|_| "PHENIX_LAYER_POLICY must be valid UTF-8")?;
        let configured: Vec<ConfiguredLayerPolicy> = serde_json::from_str(&value)?;
        for layer in configured {
            add_layer_policy(&mut policies, layer)?;
        }
    }

    // For a service explicitly configured in the portable file, its Layer
    // list replaces the deployment-supplied default Layer list.
    let mut file_policies = BTreeMap::<ServiceId, Vec<LayerPolicy>>::new();
    for layer in &config.layers {
        add_layer_policy_values(&mut file_policies, layer)?;
    }
    policies.extend(file_policies);
    for (service, layers) in policies {
        builder.set_layer_policy(service, layers);
    }
    Ok(())
}

fn add_layer_policy(
    policies: &mut BTreeMap<ServiceId, Vec<LayerPolicy>>,
    layer: ConfiguredLayerPolicy,
) -> Result<(), Box<dyn Error>> {
    add_layer_policy_values(policies, &layer)
}

fn add_layer_policy_values(
    policies: &mut BTreeMap<ServiceId, Vec<LayerPolicy>>,
    layer: &ConfiguredLayerPolicy,
) -> Result<(), Box<dyn Error>> {
    let service = ServiceId::parse(layer.service.clone())?;
    policies.entry(service).or_default().push(LayerPolicy {
        plugin: PluginId::parse(layer.plugin.clone())?,
        priority: layer.priority,
        required: layer.required,
        enabled: layer.enabled,
    });
    Ok(())
}

fn configured_plugin_packages() -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let Some(value) = env::var_os("PHENIX_PLUGIN_PACKAGES") else {
        return Ok(Vec::new());
    };
    let value = value
        .into_string()
        .map_err(|_| "PHENIX_PLUGIN_PACKAGES must be valid UTF-8")?;
    if value.is_empty() {
        return Ok(Vec::new());
    }
    Ok(value.split(':').map(PathBuf::from).collect())
}

fn add_packaged_plugin(
    builder: &mut PhenixRuntimeBuilder,
    package: &Path,
) -> Result<(), Box<dyn Error>> {
    let manifest_path = package.join("share/phenix-plugin/manifest.json");
    let manifest: PluginManifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    if matches!(manifest.execution, PluginExecution::Embedded) {
        return Err("packaged embedded plugins must be linked through Harness policy".into());
    }
    builder.add_manifest(manifest);
    Ok(())
}

fn state_path() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = env::var_os("PHENIX_STATE_DB") {
        return Ok(PathBuf::from(path));
    }
    if let Some(state_home) = env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(state_home).join("phenix/harness.sqlite"));
    }
    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".local/state/phenix/harness.sqlite"));
    }
    Err("cannot determine durable state path; set PHENIX_STATE_DB or XDG_STATE_HOME".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_layer_policy_groups_layers_by_service() {
        let configured = serde_json::to_string(&vec![json!({
            "service": "demo@1",
            "plugin": "layer",
            "priority": 7,
            "required": true
        })])
        .unwrap();
        let parsed: Vec<ConfiguredLayerPolicy> = serde_json::from_str(&configured).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].service, "demo@1");
        assert_eq!(parsed[0].plugin, "layer");
        assert_eq!(parsed[0].priority, 7);
        assert!(parsed[0].required);
        assert!(parsed[0].enabled);
    }

    #[test]
    fn cli_plugin_flags_are_parsed_without_reaching_plugins() {
        let cli = parse_cli([
            "--enable-plugin".into(),
            "phenix.adapter.acp".into(),
            "--disable-plugin=phenix.debug".into(),
        ])
        .unwrap();
        assert_eq!(
            cli.enable_plugins,
            BTreeSet::from(["phenix.adapter.acp".to_owned()])
        );
        assert_eq!(
            cli.disable_plugins,
            BTreeSet::from(["phenix.debug".to_owned()])
        );
    }

    fn process_argument(name: &str, takes_value: bool) -> ComponentProcessArgument {
        ComponentProcessArgument {
            component: phenix_core::ComponentId::parse("fixture.cli").unwrap(),
            interface: phenix_core::InterfaceId::parse("fixture.cli@1").unwrap(),
            name: name.to_owned(),
            takes_value,
            description: "fixture".into(),
            required_authority: phenix_core::Authority::default(),
        }
    }

    #[test]
    fn plugin_arguments_are_deferred_until_the_resolved_graph_is_known() {
        let cli = parse_cli([
            "--plugin-handled-value".into(),
            "7".into(),
            "--plugin-switch".into(),
        ])
        .unwrap();
        assert_eq!(
            cli.plugin_arguments,
            vec![
                "--plugin-handled-value".to_owned(),
                "7".to_owned(),
                "--plugin-switch".to_owned()
            ]
        );

        let triggers = [
            process_argument("--plugin-handled-value", true),
            process_argument("--plugin-switch", false),
        ];
        let resolved = resolve_process_arguments(&cli.plugin_arguments, &triggers).unwrap();
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].value.as_deref(), Some("7"));
        assert_eq!(resolved[1].value, None);
    }

    #[test]
    fn declared_plugin_argument_is_dispatched_to_its_component() {
        use phenix_core::{
            Authority, ComponentExport, ComponentId, ComponentManifest, InterfaceId,
            InterfaceSchema, PhenixSchema, PluginHost, PluginInstance,
        };
        use std::sync::{Arc, Mutex};

        struct CaptureProcessArgument(Arc<Mutex<Vec<PhenixValue>>>);

        impl PluginInstance for CaptureProcessArgument {
            fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
                Ok(())
            }

            fn invoke_component(
                &mut self,
                _component: &ComponentId,
                _service: &ServiceId,
                input: &[u8],
                _host: &PluginHost<'_>,
            ) -> Result<Vec<u8>, String> {
                let value: PhenixValue =
                    serde_json::from_slice(input).map_err(|error| error.to_string())?;
                self.0.lock().unwrap().push(value);
                serde_json::to_vec(&PhenixValue::Unit).map_err(|error| error.to_string())
            }
        }

        let plugin_id = PluginId::parse("fixture.process-argument").unwrap();
        let component_id = ComponentId::parse("fixture.process-argument").unwrap();
        let interface = InterfaceId::parse("fixture.process-argument@1").unwrap();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let factory_observed = Arc::clone(&observed);

        let mut builder = PhenixRuntimeBuilder::new();
        builder
            .add_embedded(
                PluginManifest {
                    id: plugin_id.clone(),
                    version: 1,
                    execution: PluginExecution::Embedded,
                    dependencies: Vec::new(),
                    services: Vec::new(),
                    resource_namespaces: Vec::new(),
                    maximum_authority: Authority::default(),
                },
                move || Box::new(CaptureProcessArgument(Arc::clone(&factory_observed))),
            )
            .unwrap();
        builder.add_component(ComponentManifest {
            id: component_id.clone(),
            owner: plugin_id,
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: interface.clone(),
                schema: InterfaceSchema::new(PhenixSchema::Any, PhenixSchema::Any),
                priority: 100,
                required_authority: Authority::default(),
            }],
            listeners: Vec::new(),
            maximum_authority: Authority::default(),
        });
        builder.add_process_argument(ComponentProcessArgument {
            component: component_id,
            interface,
            name: "--plugin-handled-value".into(),
            takes_value: true,
            description: "fixture".into(),
            required_authority: Authority::default(),
        });

        let mut harness = builder.build().unwrap();
        let arguments = resolve_process_arguments(
            &["--plugin-handled-value".into(), "7".into()],
            harness.resolved_generation().process_arguments(),
        )
        .unwrap();
        harness.activate().unwrap();
        apply_process_arguments(&mut harness, &arguments).unwrap();

        let values = observed.lock().unwrap();
        assert_eq!(values.len(), 1);
        let PhenixValue::Table(fields) = &values[0] else {
            panic!("process argument input is not a table");
        };
        assert_eq!(
            fields
                .get(&Key::parse("name").unwrap())
                .and_then(|value| match value {
                    PhenixValue::String(value) => Some(value.as_str()),
                    _ => None,
                }),
            Some("--plugin-handled-value")
        );
        assert_eq!(
            fields.get(&Key::parse("value").unwrap()),
            Some(&PhenixValue::Option(Some(Box::new(PhenixValue::String(
                "7".into()
            )))))
        );
    }

    #[test]
    fn undeclared_plugin_arguments_are_rejected_after_graph_resolution() {
        let cli = parse_cli(["--not-provided".into()]).unwrap();
        let error = resolve_process_arguments(&cli.plugin_arguments, &[]).unwrap_err();
        assert!(error.contains("--not-provided"));
    }

    #[test]
    fn plugin_argument_inline_values_are_supported() {
        let cli = parse_cli(["--plugin-handled-value=7".into()]).unwrap();
        let triggers = [process_argument("--plugin-handled-value", true)];
        let resolved = resolve_process_arguments(&cli.plugin_arguments, &triggers).unwrap();
        assert_eq!(resolved[0].value.as_deref(), Some("7"));
    }

    #[test]
    fn plugin_argument_value_is_required_when_declared() {
        let triggers = [process_argument("--plugin-handled-value", true)];
        let error =
            resolve_process_arguments(&["--plugin-handled-value".into()], &triggers).unwrap_err();
        assert!(error.contains("requires a value"));
    }

    #[test]
    fn plugin_argument_without_value_rejects_inline_value() {
        let triggers = [process_argument("--plugin-switch", false)];
        let error =
            resolve_process_arguments(&["--plugin-switch=7".into()], &triggers).unwrap_err();
        assert!(error.contains("does not take a value"));
    }

    #[test]
    fn plugin_arguments_cannot_shadow_core_options() {
        let error =
            resolve_process_arguments(&[], &[process_argument("--mode", true)]).unwrap_err();
        assert!(error.contains("conflicts with a core argument"));
    }

    #[test]
    fn mode_is_an_explicit_frontend_choice() {
        assert_eq!(
            parse_cli(["--mode".into(), "acp".into()]).unwrap().mode,
            Mode::Acp
        );
        assert_eq!(
            parse_cli(["--mode=jsonl".into()]).unwrap().mode,
            Mode::Jsonl
        );
        assert!(parse_cli(["--mode=unknown".into()]).is_err());
    }

    #[test]
    fn no_selection_preserves_default_suite() {
        assert_eq!(
            resolve_first_party_plugins(&Cli::default(), None).unwrap(),
            None
        );
    }

    #[test]
    fn configured_plugins_are_the_base_for_cli_overrides() {
        let configured = basic_model_manifest().id.as_str().to_owned();
        let added = adapter_acp_manifest().id.as_str().to_owned();
        let cli = parse_cli([
            "--enable-plugin".into(),
            added.clone(),
            "--disable-plugin".into(),
            configured.clone(),
        ])
        .unwrap();
        let enabled = resolve_first_party_plugins(&cli, Some(&configured))
            .unwrap()
            .unwrap();
        assert!(!enabled.contains(&configured));
        assert!(enabled.contains(&added));
    }

    #[test]
    fn configured_advanced_agent_closes_through_basic_configuration() {
        let advanced = advanced_agent_configuration_manifest()
            .id
            .as_str()
            .to_owned();
        let basic = basic_agent_configuration_manifest().id.as_str().to_owned();
        let enabled = resolve_first_party_plugins(&Cli::default(), Some(&advanced))
            .unwrap()
            .unwrap();

        assert!(enabled.contains(&advanced));
        assert!(enabled.contains(&basic));
        assert!(enabled.contains("phenix.agent-loop"));
        assert!(enabled.contains("phenix.options"));
        assert!(enabled.contains("phenix.memory"));
        assert!(enabled.contains("phenix.planning"));
    }

    #[test]
    fn configured_full_product_closes_over_model_providers() {
        let full = full_product_configuration_manifest().id.as_str().to_owned();
        let enabled = resolve_first_party_plugins(&Cli::default(), Some(&full))
            .unwrap()
            .unwrap();

        for required in [
            "phenix.product.full",
            "phenix.agent.advanced",
            "phenix.providers",
            "openai-api",
            "openai-codex",
            "phenix.workspace",
        ] {
            assert!(enabled.contains(required), "full product missed {required}");
        }
    }

    #[test]
    fn cli_override_keeps_default_model_providers() {
        let cli = parse_cli(["--disable-plugin=phenix.debug".into()]).unwrap();
        let enabled = resolve_first_party_plugins(&cli, None).unwrap().unwrap();
        assert!(enabled.contains("openai-api"));
        assert!(enabled.contains("openai-codex"));
        assert!(!enabled.contains("phenix.debug"));
    }

    #[test]
    fn configured_local_environment_is_a_bundled_plugin() {
        let plugin = local_environment_manifest().id.as_str().to_owned();
        let enabled = resolve_first_party_plugins(&Cli::default(), Some(&plugin))
            .unwrap()
            .unwrap();
        assert!(enabled.contains(&plugin));
    }

    #[test]
    fn explicit_enable_selects_packaged_plugin() {
        let plugin = adapter_acp_manifest().id.as_str().to_owned();
        let cli = parse_cli(["--enable-plugin".into(), plugin.clone()]).unwrap();
        let enabled = resolve_first_party_plugins(&cli, None).unwrap().unwrap();
        assert!(enabled.contains(&plugin));
    }

    #[test]
    fn explicit_disable_wins_over_explicit_enable() {
        let plugin = adapter_acp_manifest().id.as_str().to_owned();
        let cli = parse_cli([
            "--enable-plugin".into(),
            plugin.clone(),
            "--disable-plugin".into(),
            plugin.clone(),
        ])
        .unwrap();
        let enabled = resolve_first_party_plugins(&cli, None).unwrap().unwrap();
        assert!(!enabled.contains(&plugin));
    }

    #[test]
    fn empty_plugin_id_is_rejected() {
        assert!(parse_cli(["--enable-plugin".into(), String::new()]).is_err());
        assert!(parse_cli(["--disable-plugin=".into()]).is_err());
    }

    #[test]
    fn explicit_disable_blocks_required_dependency() {
        let execution = execution_manifest(default_suite_authority())
            .id
            .as_str()
            .to_owned();
        let cli = parse_cli(["--disable-plugin".into(), execution.clone()]).unwrap();
        let error = resolve_first_party_plugins(&cli, None).unwrap_err();
        assert!(error.contains(&execution));
        assert!(error.contains("requires"));
    }

    #[test]
    fn provider_overrides_have_a_native_cli_independent_of_nix() {
        let cli = parse_cli([
            "--provider-policy".into(),
            "providers.json".into(),
            "--bind-provider=fixture.memory@1=fixture.external".into(),
            "--disable-provider".into(),
            "fixture.context@1=fixture.basic".into(),
        ])
        .unwrap();

        assert_eq!(
            cli.provider_policy_file.as_deref(),
            Some(Path::new("providers.json"))
        );
        assert_eq!(
            cli.provider_bindings,
            vec![(
                InterfaceId::parse("fixture.memory@1").unwrap(),
                ComponentId::parse("fixture.external").unwrap(),
            )]
        );
        assert_eq!(
            cli.disabled_providers,
            vec![(
                InterfaceId::parse("fixture.context@1").unwrap(),
                ComponentId::parse("fixture.basic").unwrap(),
            )]
        );

        let inline = parse_cli(["--provider-policy=providers.json".into()]).unwrap();
        assert_eq!(cli.provider_policy_file, inline.provider_policy_file);
    }

    #[test]
    fn portable_provider_policy_json_is_typed_and_rejects_unknown_fields() {
        let json = r#"{
            "bind": {"fixture.memory@1": "fixture.external"},
            "disable": {"fixture.context@1": ["fixture.basic"]}
        }"#;
        let config: PortableProviderPolicy = serde_json::from_str(json).unwrap();
        assert_eq!(
            config
                .bind
                .get(&InterfaceId::parse("fixture.memory@1").unwrap()),
            Some(&ComponentId::parse("fixture.external").unwrap())
        );
        assert!(
            config
                .disable
                .get(&InterfaceId::parse("fixture.context@1").unwrap())
                .unwrap()
                .contains(&ComponentId::parse("fixture.basic").unwrap())
        );
        assert!(serde_json::from_str::<PortableProviderPolicy>(r#"{"typo":{}}"#).is_err());
        assert!(
            serde_json::from_str::<PortableProviderPolicy>(
                r#"{"bind":{"invalid-interface":"fixture.external"}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn malformed_provider_options_fail_before_plugin_argument_dispatch() {
        assert!(parse_cli(["--provider-policy=".into()]).is_err());
        assert!(parse_cli(["--bind-provider".into()]).is_err());
        assert!(parse_cli(["--bind-provider=fixture.memory@1".into()]).is_err());
        assert!(parse_cli(["--disable-provider=fixture.memory@1=".into()]).is_err());
        assert!(parse_cli(["--bind-provider=invalid=fixture.external".into()]).is_err());
    }

    #[test]
    fn portable_composition_includes_profile_plugins_providers_and_layers() {
        let source = r#"{
            "profile": "phenix.product.basic",
            "plugins": {
                "enable": ["phenix.debug"],
                "disable": ["phenix.planning"]
            },
            "providers": {
                "bind": {"fixture.memory@1": "fixture.external"},
                "disable": {"fixture.context@1": ["fixture.basic"]}
            },
            "layers": [{
                "service": "fixture.memory@1",
                "plugin": "fixture.observer",
                "priority": 15,
                "required": false
            }]
        }"#;
        let config: PortableCompositionConfig = serde_json::from_str(source).unwrap();
        assert_eq!(config.profile.as_deref(), Some("phenix.product.basic"));
        assert!(config.plugins.enable.contains("phenix.debug"));
        assert!(config.plugins.disable.contains("phenix.planning"));
        assert_eq!(
            config
                .providers
                .bind
                .get(&InterfaceId::parse("fixture.memory@1").unwrap()),
            Some(&ComponentId::parse("fixture.external").unwrap())
        );
        assert_eq!(config.layers.len(), 1);
        assert_eq!(config.layers[0].priority, 15);
        assert!(config.layers[0].enabled);
        assert!(serde_json::from_str::<PortableCompositionConfig>(r#"{"typo":42}"#).is_err());
        assert!(
            serde_json::from_str::<PortableCompositionConfig>(
                r#"{"plugins":{"enable":["phenix.debug"],"typo":true}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn portable_profile_selection_is_phx_owned_and_cli_options_override_file() {
        let config: PortableCompositionConfig = serde_json::from_str(
            r#"{
                "profile": "phenix.product.basic",
                "plugins": {
                    "enable": ["phenix.debug"],
                    "disable": ["phenix.options"]
                }
            }"#,
        )
        .unwrap();
        let mut cli = Cli::default();
        // CLI selection overrides a file exclusion, but cannot make a required
        // dependency disappear from the selected graph.
        cli.enable_plugins.insert("phenix.options".into());
        cli.disable_plugins.insert("phenix.debug".into());
        let selected =
            resolve_configured_first_party_plugins(&cli, &config, Some("phenix.product.full"))
                .unwrap()
                .unwrap();
        assert!(selected.contains("phenix.product.basic"));
        assert!(!selected.contains("phenix.product.full"));
        assert!(selected.contains("phenix.options"));
        assert!(!selected.contains("phenix.debug"));
    }

    #[test]
    fn native_config_file_flag_supports_both_forms() {
        let positional = parse_cli(["--config".into(), "composition.json".into()]).unwrap();
        let inline = parse_cli(["--config=composition.json".into()]).unwrap();
        assert_eq!(positional.config_file, inline.config_file);
        assert_eq!(
            positional.config_file.as_deref(),
            Some(Path::new("composition.json"))
        );
        assert!(parse_cli(["--config".into()]).is_err());
        assert!(parse_cli(["--config=".into()]).is_err());
    }

    #[test]
    fn cli_profile_overrides_file_profile_and_environment() {
        let config: PortableCompositionConfig =
            serde_json::from_str(r#"{"profile":"phenix.product.basic"}"#).unwrap();
        let cli = parse_cli(["--profile=phenix.product.full".into()]).unwrap();
        let selected =
            resolve_configured_first_party_plugins(&cli, &config, Some("phenix.agent.basic"))
                .unwrap()
                .unwrap();
        assert!(selected.contains("phenix.product.full"));
        assert!(selected.contains("phenix.agent.advanced"));
        assert!(parse_cli(["--profile".into()]).is_err());
        assert!(parse_cli(["--profile=".into()]).is_err());
    }
}
