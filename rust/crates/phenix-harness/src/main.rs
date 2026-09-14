use phenix_conductor::serve_jsonl;
use phenix_core::{
    LayerPolicy, LocalPersistence, PluginExecution, PluginId, PluginManifest, ServiceId,
};
use phenix_harness::runtime_config;
use phenix_harness::{default_suite_authority, HarnessBuilder};
use phenix_plugin_catalog::{
    adapter_acp_manifest, artifact_manifest, basic_context_manifest, basic_model_manifest,
    basic_skills_manifest, basic_tools_manifest, cli_manifest, context_manifest, debug_manifest,
    execution_manifest, frontend_manifest, hook_manifest, job_manifest, language_manifest,
    memory_manifest, model_routing_manifest, options_manifest, planning_manifest,
    repository_worker_manifest, sdk_manifest, session_manifest, session_tree_manifest,
    workspace_manifest, OptionStartupPrecedence,
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Default, Eq, PartialEq)]
struct Cli {
    help: bool,
    list_services: bool,
    enable_plugins: BTreeSet<String>,
    disable_plugins: BTreeSet<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("phenix-harness: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
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
        Some(enabled) => HarnessBuilder::with_selected_suite(&enabled)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?,
        None => HarnessBuilder::with_default_suite()?,
    };
    for package in configured_plugin_packages()? {
        add_packaged_plugin(&mut builder, &package)?;
    }
    apply_configured_layer_policy(&mut builder)?;
    let mut harness = builder.build_with_persistence(persistence)?;
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
                .into())
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

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = io::BufWriter::new(stdout.lock());
    serve_jsonl(
        harness.kernel_mut(),
        &default_suite_authority(),
        stdin.lock(),
        &mut stdout,
    )?;
    Ok(())
}

fn print_help() {
    println!(
        "phenix-harness [OPTIONS]\n\nWithout arguments, reads JSONL service requests from stdin and writes JSONL responses.\n\nOptions:\n  --list-services       List active plugins and services as JSON\n  --enable-plugin ID    Enable a bundled plugin for this process\n  --disable-plugin ID   Disable a bundled plugin for this process\n  -h, --help            Print help"
    );
}

fn parse_cli(args: impl IntoIterator<Item = String>) -> Result<Cli, String> {
    let mut cli = Cli::default();
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--help" | "-h" => cli.help = true,
            "--list-services" => cli.list_services = true,
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
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(cli)
}

fn first_party_plugins() -> Vec<(PluginManifest, bool)> {
    let authority = default_suite_authority();
    vec![
        (adapter_acp_manifest(), false),
        (repository_worker_manifest(), true),
        (session_manifest(), true),
        (session_tree_manifest(), true),
        (artifact_manifest(), true),
        (cli_manifest(authority.clone()), true),
        (context_manifest(), true),
        (execution_manifest(authority.clone()), true),
        (language_manifest(), true),
        (memory_manifest(), true),
        (planning_manifest(), true),
        (workspace_manifest(), true),
        (model_routing_manifest(authority.clone()), true),
        (job_manifest(), true),
        (frontend_manifest(authority.clone()), true),
        (hook_manifest(authority.clone()), true),
        (debug_manifest(authority.clone()), true),
        (options_manifest(), true),
        (sdk_manifest(authority), true),
        (basic_model_manifest(), false),
        (basic_tools_manifest(), false),
        (basic_skills_manifest(), false),
        (basic_context_manifest(), false),
    ]
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

fn apply_configured_layer_policy(builder: &mut HarnessBuilder) -> Result<(), Box<dyn Error>> {
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

fn add_packaged_plugin(builder: &mut HarnessBuilder, package: &Path) -> Result<(), Box<dyn Error>> {
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
