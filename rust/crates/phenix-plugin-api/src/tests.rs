use super::*;
use phenix_core::{
    Kernel, KernelConfig, PermissionId, ResolvedGeneration, ResolvedGenerationActivation,
};
use phenix_plugin_context::{context_component_manifest, context_factory, context_manifest};
use phenix_plugin_execution::{
    execution_component_manifest, execution_factory, execution_manifest,
};
use phenix_plugin_options::{
    options_component_manifest, options_durable_schema_registrations, options_factory,
    options_manifest, options_service,
};
use phenix_plugin_sessions::{session_component_manifest, session_factory, session_manifest};
use phenix_sdk::OptionScope;
use std::sync::atomic::{AtomicU64, Ordering};

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            env::temp_dir().join(format!("phenix-sdk-{name}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn abi<T>(value: &T) -> Vec<u8>
where
    for<'value> phenix_core::PhenixValue: From<&'value T>,
{
    serde_json::to_vec(&phenix_core::PhenixValue::from(value)).unwrap()
}

fn projected<T>(bytes: &[u8]) -> T
where
    for<'value> T: TryFrom<
        phenix_core::Project<&'value phenix_core::PhenixValue>,
        Error = phenix_core::ValueError,
    >,
{
    serde_json::from_slice::<phenix_core::PhenixValue>(bytes)
        .unwrap()
        .project()
        .unwrap()
}

fn authority() -> Authority {
    Authority::new([
        PermissionId::parse("kernel.persistence.schema").unwrap(),
        PermissionId::parse("kernel.persistence.read").unwrap(),
        PermissionId::parse("kernel.persistence.write").unwrap(),
    ])
}

#[test]
fn runtime_identity_is_api() {
    assert_eq!(SDK_PLUGIN, "phenix.api");
    assert_eq!(SDK_COMPONENT, "phenix.api");
    assert_eq!(SDK_SESSION_SERVICE, "phenix.api.sessions@1");
    assert_eq!(SDK_TOOLS_SERVICE, "phenix.api.tools@1");
    assert_eq!(SDK_SKILLS_SERVICE, "phenix.api.skills@1");
    assert_eq!(SDK_CONFIG_SERVICE, "phenix.api.config@1");
}

#[test]
fn default_sdk_contribution_exposes_standard_modules() {
    let contribution = sdk_contribution();
    assert_eq!(contribution.namespace.as_str(), "phenix");
    for interface in [
        SdkSessionInterface::interface_id(),
        ModelRoutingInterface::interface_id(),
        SdkToolsInterface::interface_id(),
        SdkSkillsInterface::interface_id(),
        ContextInterface::interface_id(),
        OptionsInterface::interface_id(),
    ] {
        assert!(contribution.interfaces.contains(&interface));
    }
}

#[test]
fn config_paths_parse_before_dispatch() {
    assert_eq!(
        serde_json::from_str::<SdkConfigCommand>(
            r#"{"operation":"read","path":"nested/settings.json"}"#,
        )
        .unwrap(),
        SdkConfigCommand::Read {
            path: SdkConfigPath::parse("nested/settings.json").unwrap(),
        }
    );
    for path in ["", ".", "../settings.json", "/settings.json"] {
        let input = format!(r#"{{"operation":"read","path":{path:?}}}"#);
        assert!(serde_json::from_str::<SdkConfigCommand>(&input).is_err());
    }
}

#[test]
fn config_reads_stay_under_the_config_root() {
    let directory = TempDirectory::new("config-read");
    fs::write(directory.path().join("settings.json"), b"settings").unwrap();

    assert_eq!(
        config_command(
            directory.path(),
            SdkConfigCommand::Read {
                path: SdkConfigPath::parse("settings.json").unwrap(),
            },
        )
        .unwrap(),
        SdkConfigResponse::File {
            content: b"settings".to_vec().into(),
        }
    );
}

#[cfg(unix)]
#[test]
fn config_reads_reject_symlink_escapes() {
    use std::os::unix::fs::symlink;

    let directory = TempDirectory::new("config-symlink");
    let config = directory.path().join("config");
    let outside = directory.path().join("outside");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.json"), b"secret").unwrap();
    symlink(outside.join("secret.json"), config.join("settings.json")).unwrap();

    let error = config_command(
        &config,
        SdkConfigCommand::Read {
            path: SdkConfigPath::parse("settings.json").unwrap(),
        },
    )
    .unwrap_err();
    assert!(error.contains("escapes config root"));
}

#[test]
fn session_open_uses_scoped_options() {
    let authority = authority();
    let session_manifest = session_manifest();
    let options_manifest = options_manifest();
    let sdk_manifest = sdk_manifest(authority.clone());
    let manifests = vec![
        session_manifest.clone(),
        options_manifest.clone(),
        sdk_manifest.clone(),
    ];
    let resolved = ResolvedGeneration::resolve_with_durable_schemas(
        manifests.clone(),
        [
            session_component_manifest(),
            options_component_manifest(),
            sdk_component_manifest(authority.clone()),
        ],
        options_durable_schema_registrations(),
        [],
        &authority,
    )
    .unwrap();
    let mut kernel = Kernel::new(KernelConfig::new(manifests).unwrap());
    kernel
        .register_embedded_factory(session_manifest.id, session_factory)
        .unwrap();
    kernel
        .register_embedded_factory(options_manifest.id, options_factory)
        .unwrap();
    kernel
        .register_embedded_factory(sdk_manifest.id, sdk_factory)
        .unwrap();
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel.activate_all().unwrap();

    let open = |kernel: &mut Kernel| {
        let output = kernel
            .invoke(
                &sdk_session_service(),
                &abi(&SdkSessionCommand::Open {
                    id: "root".into(),
                    agent: Some("planner".into()),
                }),
                &authority,
                None,
            )
            .unwrap();
        projected::<SdkSessionResponse>(&output)
    };

    assert!(matches!(
        open(&mut kernel),
        SdkSessionResponse::Opened { created: true, .. }
    ));
    assert!(matches!(
        open(&mut kernel),
        SdkSessionResponse::Opened { created: false, .. }
    ));

    let options_component = options_component_manifest();
    kernel
        .invoke_component(
            &options_component.id,
            &options_service(),
            &abi(&OptionCommand::Set {
                key: OptionKey::parse("session.reuse_existing").unwrap(),
                scope: OptionScope::Agent(OptionSubjectId::parse("planner").unwrap()),
                value: OptionValue::Bool(false),
            }),
            &authority,
            &options_component.owner,
        )
        .unwrap();

    assert!(kernel
        .invoke(
            &sdk_session_service(),
            &abi(&SdkSessionCommand::Open {
                id: "root".into(),
                agent: Some("planner".into()),
            }),
            &authority,
            None,
        )
        .is_err());
}

#[test]
fn sdk_tools_wrap_execution_callables() {
    let authority = authority();
    let execution_manifest = execution_manifest(authority.clone());
    let sdk_manifest = sdk_manifest(authority.clone());
    let manifests = vec![execution_manifest.clone(), sdk_manifest.clone()];
    let resolved = ResolvedGeneration::resolve(
        manifests.clone(),
        [
            execution_component_manifest(authority.clone()),
            sdk_component_manifest(authority.clone()),
        ],
        [],
        &authority,
    )
    .unwrap();
    let mut kernel = Kernel::new(KernelConfig::new(manifests).unwrap());
    kernel
        .register_embedded_factory(execution_manifest.id, execution_factory)
        .unwrap();
    kernel
        .register_embedded_factory(sdk_manifest.id, sdk_factory)
        .unwrap();
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel.activate_all().unwrap();

    let output = kernel
        .invoke(
            &sdk_tools_service(),
            &abi(&SdkToolCommand::Register {
                id: "read".into(),
                service: "fixture.read@1".into(),
                required_capabilities: BTreeSet::new(),
            }),
            &authority,
            None,
        )
        .unwrap();
    assert!(matches!(
        projected::<SdkToolResponse>(&output),
        SdkToolResponse::Tool { tool } if tool.id == "read" && tool.service == "fixture.read@1"
    ));
}

#[test]
fn sdk_skills_wrap_context_resources() {
    let authority = authority();
    let execution_manifest = execution_manifest(authority.clone());
    let context_manifest = context_manifest();
    let sdk_manifest = sdk_manifest(authority.clone());
    let manifests = vec![
        execution_manifest.clone(),
        context_manifest.clone(),
        sdk_manifest.clone(),
    ];
    let resolved = ResolvedGeneration::resolve(
        manifests.clone(),
        [
            execution_component_manifest(authority.clone()),
            context_component_manifest(),
            sdk_component_manifest(authority.clone()),
        ],
        [],
        &authority,
    )
    .unwrap();
    let mut kernel = Kernel::new(KernelConfig::new(manifests).unwrap());
    kernel
        .register_embedded_factory(execution_manifest.id, execution_factory)
        .unwrap();
    kernel
        .register_embedded_factory(context_manifest.id, context_factory)
        .unwrap();
    kernel
        .register_embedded_factory(sdk_manifest.id, sdk_factory)
        .unwrap();
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel.activate_all().unwrap();

    let register = kernel
        .invoke(
            &sdk_skills_service(),
            &abi(&SdkSkillCommand::Register {
                id: "review".into(),
                content: b"review carefully".to_vec().into(),
            }),
            &authority,
            None,
        )
        .unwrap();
    assert!(matches!(
        projected::<SdkSkillResponse>(&register),
        SdkSkillResponse::Skill { skill: Some(skill) }
            if skill.id == "review" && skill.content.as_ref() == b"review carefully"
    ));

    let list = kernel
        .invoke(
            &sdk_skills_service(),
            &abi(&SdkSkillCommand::List),
            &authority,
            None,
        )
        .unwrap();
    assert!(matches!(
        projected::<SdkSkillResponse>(&list),
        SdkSkillResponse::Skills { skills } if skills.len() == 1 && skills[0].id == "review"
    ));
}
