#![forbid(unsafe_code)]

mod runtime_config;

use phenix_core::{
    ComponentProcessArgument, Key, LayerPolicy, LocalPersistence, PhenixValue, PluginExecution,
    PluginId, PluginManifest, ServiceId,
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
    execution_manifest, frontend_manifest, full_product_configuration_manifest, hook_manifest,
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
    enable_plugins: BTreeSet<String>,
    disable_plugins: BTreeSet<String>,
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

    let state = state_path()?;
    if let Some(parent) = state.parent() {
        fs::create_dir_all(parent)?;
    }
    let persistence = LocalPersistence::open(&state)?;
    let mut builder = match configured_first_party_plugins(&cli)? {
        Some(enabled) => PhenixRuntimeBuilder::with_selected_suite(&enabled)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?,
        None => PhenixRuntimeBuilder::with_default_suite()?,
    };
    for package in configured_plugin_packages()? {
        add_packaged_plugin(&mut builder, &package)?;
    }
    apply_configured_layer_policy(&mut builder)?;
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
        "phenix [OPTIONS]\n\nRuns the packaged Phenix composition. The default mode is jsonl.\n\nOptions:\n  --mode MODE           Frontend mode: jsonl or acp\n  --list-services       List active plugins and services as JSON\n  --enable-plugin ID    Enable a bundled plugin for this process\n  --disable-plugin ID   Disable a bundled plugin for this process\n  -h, --help            Print help\n\nLoaded plugins may declare additional long options."
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

fn configured_first_party_plugins(cli: &Cli) -> Result<Option<BTreeSet<String>>, Box<dyn Error>> {
    let configured = env::var_os("PHENIX_ENABLED_PLUGINS");
    let configured = configured
        .map(OsString::into_string)
        .transpose()
        .map_err(|_| "PHENIX_ENABLED_PLUGINS must be valid UTF-8")?;
    resolve_first_party_plugins(cli, configured.as_deref()).map_err(Into::into)
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

#[derive(serde::Deserialize)]
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

fn apply_configured_layer_policy(builder: &mut PhenixRuntimeBuilder) -> Result<(), Box<dyn Error>> {
    let Some(value) = env::var_os("PHENIX_LAYER_POLICY") else {
        return Ok(());
    };
    let value = value
        .into_string()
        .map_err(|_| "PHENIX_LAYER_POLICY must be valid UTF-8")?;
    let configured: Vec<ConfiguredLayerPolicy> = serde_json::from_str(&value)?;
    let mut policies = BTreeMap::<ServiceId, Vec<LayerPolicy>>::new();
    for layer in configured {
        let service = ServiceId::parse(layer.service)?;
        policies.entry(service).or_default().push(LayerPolicy {
            plugin: PluginId::parse(layer.plugin)?,
            priority: layer.priority,
            required: layer.required,
            enabled: layer.enabled,
        });
    }
    for (service, layers) in policies {
        builder.set_layer_policy(service, layers);
    }
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
}
