use crate::{execution_factory, execution_manifest};
use phenix_core::{
    Authority, ConfigContribution, ConfigContributionSource, ConfigNamespace,
    ConfigurationFrontendId, Kernel, KernelConfig, LocalPersistence, PermissionId, PhenixValue,
    Project, ResolvedGeneration, ResolvedGenerationActivation,
};
use phenix_sdk::{
    execution_service, ExecutionAuthority, ExecutionCommand, ExecutionRecord, ExecutionResponse,
    WorkerTaskRecord,
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn capability(value: &str) -> PermissionId {
    PermissionId::parse(value).expect("static capability is valid")
}

fn caller_authority() -> Authority {
    Authority::new([
        capability("kernel.persistence.schema"),
        capability("kernel.persistence.read"),
        capability("kernel.persistence.write"),
        capability("fs.read"),
    ])
}

fn generation_contribution(mode: &str) -> ConfigContribution {
    ConfigContribution {
        source: ConfigContributionSource {
            frontend: ConfigurationFrontendId::parse("generation-test").unwrap(),
            source_identity: "fixture:generation".into(),
            source_revision: format!("rev-{mode}"),
        },
        namespace: ConfigNamespace::parse("fixture.generation@1").unwrap(),
        contract_version: 1,
        precedence: 10,
        value: serde_json::json!({"mode": mode}).into(),
        requested_authority: Authority::default(),
    }
}

fn kernel_with(path: &PathBuf, mode: &str) -> Kernel {
    let manifest = execution_manifest(caller_authority());
    let plugin = manifest.id.clone();
    let resolved = ResolvedGeneration::resolve(
        [manifest.clone()],
        [],
        [generation_contribution(mode)],
        &caller_authority(),
    )
    .unwrap();
    let persistence = LocalPersistence::open(path).unwrap();
    let mut kernel = Kernel::with_persistence(KernelConfig::new([manifest]).unwrap(), persistence);
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel
        .register_embedded_factory(plugin, execution_factory)
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn invoke(kernel: &mut Kernel, command: &ExecutionCommand) -> ExecutionResponse {
    let output = kernel
        .invoke(
            &execution_service(),
            &serde_json::to_vec(&PhenixValue::from(command)).unwrap(),
            &caller_authority(),
            None,
        )
        .unwrap();
    {
        let value: PhenixValue = serde_json::from_slice(&output).unwrap();
        ExecutionResponse::try_from(Project(&value)).unwrap()
    }
}

fn execution(response: ExecutionResponse) -> ExecutionRecord {
    match response {
        ExecutionResponse::Execution { execution } => execution,
        other => panic!("unexpected execution response: {other:?}"),
    }
}

fn task(response: ExecutionResponse) -> WorkerTaskRecord {
    match response {
        ExecutionResponse::Task { task } => task,
        other => panic!("unexpected task response: {other:?}"),
    }
}

fn temp_db() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-execution-generation-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

#[test]
fn restored_execution_lineage_stays_pinned_when_the_generation_topology_changes() {
    let path = temp_db();
    let old_generation;
    {
        let mut kernel = kernel_with(&path, "old");
        let root = execution(invoke(
            &mut kernel,
            &ExecutionCommand::CreateExecution {
                id: "root".into(),
                requested_authority: ExecutionAuthority::new(["fs.read"]),
            },
        ));
        old_generation = root.graph_generation.clone();
        invoke(
            &mut kernel,
            &ExecutionCommand::RegisterCallable {
                id: "fixture.callable".into(),
                service: "fixture.echo@1".into(),
                required_authority: ExecutionAuthority::new(["fs.read"]),
            },
        );
        let initial_task = task(invoke(
            &mut kernel,
            &ExecutionCommand::CreateTask {
                id: "before-reload".into(),
                parent_execution: "root".into(),
                description: "before reload".into(),
                depends_on: Default::default(),
                requested_authority: ExecutionAuthority::new(["fs.read"]),
            },
        ));
        assert_eq!(initial_task.graph_generation, old_generation);
    }

    let mut kernel = kernel_with(&path, "new");
    let active_generation = kernel.graph_generation().unwrap().as_str().to_owned();
    assert_ne!(active_generation, old_generation);

    let restored = match invoke(
        &mut kernel,
        &ExecutionCommand::GetExecution { id: "root".into() },
    ) {
        ExecutionResponse::ExecutionLookup {
            execution: Some(execution),
        } => execution,
        other => panic!("unexpected restored execution response: {other:?}"),
    };
    assert_eq!(restored.graph_generation, old_generation);

    let error = kernel
        .invoke(
            &execution_service(),
            &serde_json::to_vec(&PhenixValue::from(&ExecutionCommand::InvokeCallable {
                execution_id: "root".into(),
                callable_id: "fixture.callable".into(),
                input: Vec::new(),
            }))
            .unwrap(),
            &caller_authority(),
            None,
        )
        .unwrap_err();
    assert!(error.to_string().contains(&format!(
        "execution root is pinned to graph generation {old_generation}, but active generation is {active_generation}"
    )));

    let delegated = execution(invoke(
        &mut kernel,
        &ExecutionCommand::DelegateExecution {
            parent_execution: "root".into(),
            id: "child".into(),
            requested_authority: ExecutionAuthority::new(["fs.read"]),
        },
    ));
    assert_eq!(delegated.graph_generation, old_generation);

    let inherited_task = task(invoke(
        &mut kernel,
        &ExecutionCommand::CreateTask {
            id: "after-reload".into(),
            parent_execution: "root".into(),
            description: "after reload".into(),
            depends_on: Default::default(),
            requested_authority: ExecutionAuthority::new(["fs.read"]),
        },
    ));
    assert_eq!(inherited_task.graph_generation, old_generation);

    let fresh_root = execution(invoke(
        &mut kernel,
        &ExecutionCommand::CreateExecution {
            id: "fresh-root".into(),
            requested_authority: ExecutionAuthority::new(["fs.read"]),
        },
    ));
    assert_eq!(fresh_root.graph_generation, active_generation);

    let _ = fs::remove_file(path);
}
