use phenix_core::{
    ArtifactRevision, Authority, CapabilityId, ComponentExport, ComponentId, ComponentInterface,
    ComponentManifest, DurableSchema, PhenixValue, PluginContext, PluginExecution, PluginHost,
    PluginId, PluginInstance, PluginManifest, ResourceNamespace, ServiceContribution, ServiceId,
    TransactionOp, ValueCodec,
};
use phenix_sdk::{
    efficiency_outcome_evidence_service, EfficiencyOutcomeEvidence,
    EfficiencyOutcomeEvidenceCommand, EfficiencyOutcomeEvidenceInterface,
    EfficiencyOutcomeEvidenceRequest, EfficiencyOutcomeEvidenceResponse, EvaluationOutcome,
};
use std::collections::BTreeMap;

pub const BENCHMARK_OUTCOME_PLUGIN: &str = "phenix.benchmark-outcomes";
pub const BENCHMARK_OUTCOME_COMPONENT: &str = "phenix.benchmark-outcomes";
pub const BENCHMARK_OUTCOME_SERVICE: &str = "phenix.benchmark-outcomes@1";

const BENCHMARK_OUTCOME_NAMESPACE: &str = "phenix.benchmark-outcomes.state";
const STATE_KEY: &str = "outcomes";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct BenchmarkOutcomeRecord {
    pub task_fixture_revision: String,
    pub root_execution_id: String,
    pub evaluator_identity: String,
    pub evidence_revision: String,
    pub outcome: EvaluationOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum BenchmarkOutcomeCommand {
    Publish {
        record: BenchmarkOutcomeRecord,
    },
    Get {
        task_fixture_revision: String,
        root_execution_id: String,
        evaluator_identity: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum BenchmarkOutcomeResponse {
    Published {
        evidence: EfficiencyOutcomeEvidence,
    },
    Lookup {
        record: Option<BenchmarkOutcomeRecord>,
    },
}

pub struct BenchmarkOutcomeInterface;

impl ComponentInterface for BenchmarkOutcomeInterface {
    fn interface_id() -> phenix_core::InterfaceId {
        phenix_core::InterfaceId::parse(BENCHMARK_OUTCOME_SERVICE)
            .expect("static benchmark outcome interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<BenchmarkOutcomeCommand, BenchmarkOutcomeResponse>()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, phenix_sdk_macros::PhenixValue)]
struct BenchmarkOutcomeProjection {
    records: BTreeMap<String, BenchmarkOutcomeRecord>,
}

#[must_use]
pub fn benchmark_outcome_service() -> ServiceId {
    ServiceId::parse(BENCHMARK_OUTCOME_SERVICE)
        .expect("static benchmark outcome service id is valid")
}

#[must_use]
pub fn benchmark_outcome_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(BENCHMARK_OUTCOME_PLUGIN)
            .expect("static benchmark outcome plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![
            ServiceContribution {
                role: phenix_core::ServiceRole::Terminal,
                service: benchmark_outcome_service(),
                priority: 100,
                required_authority: persistence_authority(),
            },
            ServiceContribution {
                role: phenix_core::ServiceRole::Terminal,
                service: efficiency_outcome_evidence_service(),
                priority: 100,
                required_authority: persistence_read_authority(),
            },
        ],
        resource_namespaces: vec![benchmark_outcome_namespace()],
        maximum_authority: persistence_authority(),
    }
}

#[must_use]
pub fn benchmark_outcome_component_id() -> ComponentId {
    ComponentId::parse(BENCHMARK_OUTCOME_COMPONENT)
        .expect("static benchmark outcome component id is valid")
}

#[must_use]
pub fn benchmark_outcome_component_manifest() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: benchmark_outcome_component_id(),
        owner: PluginId::parse(BENCHMARK_OUTCOME_PLUGIN)
            .expect("static benchmark outcome plugin id is valid"),
        imports: Vec::new(),
        exports: vec![
            ComponentExport {
                interface: BenchmarkOutcomeInterface::interface_id(),
                schema: BenchmarkOutcomeInterface::schema(),
                priority: 100,
                required_authority: persistence_authority(),
            },
            ComponentExport {
                interface: EfficiencyOutcomeEvidenceInterface::interface_id(),
                schema: EfficiencyOutcomeEvidenceInterface::schema(),
                priority: 100,
                required_authority: persistence_read_authority(),
            },
        ],
        maximum_authority: persistence_authority(),
    }
}

#[must_use]
pub fn benchmark_outcome_factory() -> Box<dyn PluginInstance> {
    Box::new(BenchmarkOutcomePlugin)
}

fn persistence_authority() -> Authority {
    Authority::new([
        CapabilityId::parse(PERSISTENCE_SCHEMA).expect("static persistence capability is valid"),
        CapabilityId::parse(PERSISTENCE_READ).expect("static persistence capability is valid"),
        CapabilityId::parse(PERSISTENCE_WRITE).expect("static persistence capability is valid"),
    ])
}

fn persistence_read_authority() -> Authority {
    Authority::new([
        CapabilityId::parse(PERSISTENCE_READ).expect("static persistence capability is valid"),
    ])
}

fn benchmark_outcome_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(BENCHMARK_OUTCOME_NAMESPACE)
        .expect("static benchmark outcome namespace is valid")
}

type BenchmarkOutcomeContext<'host, 'runtime> = PluginContext<'host, 'runtime, ()>;

fn context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> BenchmarkOutcomeContext<'host, 'runtime> {
    PluginContext::new(host, (), (), ())
}

struct BenchmarkOutcomePlugin;

impl PluginInstance for BenchmarkOutcomePlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        context(host)
            .kernel
            .register_durable_schema(&DurableSchema::new(benchmark_outcome_namespace(), 1))
            .map_err(|error| error.to_string())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let context = context(host);
        if service == &benchmark_outcome_service() {
            let command = context
                .kernel
                .decode_projected::<BenchmarkOutcomeCommand>(
                    &BenchmarkOutcomeInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let response = handle_benchmark(&context, command)?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service == &efficiency_outcome_evidence_service() {
            let command = context
                .kernel
                .decode_projected::<EfficiencyOutcomeEvidenceCommand>(
                    &EfficiencyOutcomeEvidenceInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let response = handle_evidence(&context, command)?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        Err(format!("unsupported benchmark outcome service: {service}"))
    }
}

fn handle_benchmark(
    context: &BenchmarkOutcomeContext<'_, '_>,
    command: BenchmarkOutcomeCommand,
) -> Result<BenchmarkOutcomeResponse, String> {
    match command {
        BenchmarkOutcomeCommand::Publish { record } => {
            validate_record(&record)?;
            let key = record_key(
                &record.task_fixture_revision,
                &record.root_execution_id,
                &record.evaluator_identity,
            );
            let (old, mut state) = read_state(context)?;
            if let Some(existing) = state.records.get(&key) {
                if existing != &record {
                    return Err(
                        "benchmark outcome identity already has different terminal evidence".into(),
                    );
                }
                return Ok(BenchmarkOutcomeResponse::Published {
                    evidence: evidence(existing),
                });
            }
            let published = evidence(&record);
            state.records.insert(key, record);
            persist_state(context, old, &state)?;
            Ok(BenchmarkOutcomeResponse::Published {
                evidence: published,
            })
        }
        BenchmarkOutcomeCommand::Get {
            task_fixture_revision,
            root_execution_id,
            evaluator_identity,
        } => {
            validate_identity("task fixture revision", &task_fixture_revision)?;
            validate_identity("root execution id", &root_execution_id)?;
            validate_identity("evaluator identity", &evaluator_identity)?;
            let (_, state) = read_state(context)?;
            Ok(BenchmarkOutcomeResponse::Lookup {
                record: state
                    .records
                    .get(&record_key(
                        &task_fixture_revision,
                        &root_execution_id,
                        &evaluator_identity,
                    ))
                    .cloned(),
            })
        }
    }
}

fn handle_evidence(
    context: &BenchmarkOutcomeContext<'_, '_>,
    command: EfficiencyOutcomeEvidenceCommand,
) -> Result<EfficiencyOutcomeEvidenceResponse, String> {
    match command {
        EfficiencyOutcomeEvidenceCommand::Resolve { request } => {
            validate_evidence_request(&request)?;
            let (_, state) = read_state(context)?;
            let evidence = state
                .records
                .get(&record_key(
                    &request.task_fixture_revision,
                    &request.root_execution_id,
                    &request.evaluator_identity,
                ))
                .map(evidence);
            Ok(EfficiencyOutcomeEvidenceResponse::Evidence { evidence })
        }
    }
}

fn validate_record(record: &BenchmarkOutcomeRecord) -> Result<(), String> {
    validate_identity("task fixture revision", &record.task_fixture_revision)?;
    validate_identity("root execution id", &record.root_execution_id)?;
    validate_identity("evaluator identity", &record.evaluator_identity)?;
    validate_identity("evidence revision", &record.evidence_revision)?;
    if record.outcome == EvaluationOutcome::Unresolved {
        return Err("benchmark outcome provider accepts terminal outcomes only".into());
    }
    Ok(())
}

fn validate_evidence_request(request: &EfficiencyOutcomeEvidenceRequest) -> Result<(), String> {
    validate_identity("task fixture revision", &request.task_fixture_revision)?;
    validate_identity("root execution id", &request.root_execution_id)?;
    validate_identity("evaluator identity", &request.evaluator_identity)
}

fn validate_identity(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(())
    }
}

fn evidence(record: &BenchmarkOutcomeRecord) -> EfficiencyOutcomeEvidence {
    let key = record_key(
        &record.task_fixture_revision,
        &record.root_execution_id,
        &record.evaluator_identity,
    );
    EfficiencyOutcomeEvidence {
        source_identity: format!("benchmark-outcome:{key}"),
        evaluator_identity: record.evaluator_identity.clone(),
        evidence_revision: record.evidence_revision.clone(),
        outcome: record.outcome,
    }
}

fn record_key(
    task_fixture_revision: &str,
    root_execution_id: &str,
    evaluator_identity: &str,
) -> String {
    let mut bytes = Vec::new();
    for part in [
        task_fixture_revision.as_bytes(),
        root_execution_id.as_bytes(),
        evaluator_identity.as_bytes(),
    ] {
        bytes.extend_from_slice(&u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes());
        bytes.extend_from_slice(part);
    }
    ArtifactRevision::from_content(&bytes).to_string()
}

fn read_state(
    context: &BenchmarkOutcomeContext<'_, '_>,
) -> Result<(Option<Vec<u8>>, BenchmarkOutcomeProjection), String> {
    let old = context
        .kernel
        .read_durable(&benchmark_outcome_namespace(), STATE_KEY)
        .map_err(|error| error.to_string())?;
    let state = old
        .as_deref()
        .map(|bytes| {
            let value: PhenixValue =
                serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
            BenchmarkOutcomeProjection::from_value(&value).map_err(|error| error.to_string())
        })
        .transpose()?
        .unwrap_or_default();
    Ok((old, state))
}

fn persist_state(
    context: &BenchmarkOutcomeContext<'_, '_>,
    old: Option<Vec<u8>>,
    state: &BenchmarkOutcomeProjection,
) -> Result<(), String> {
    context
        .kernel
        .transact_durable(
            &benchmark_outcome_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: STATE_KEY.into(),
                    expected: old,
                },
                TransactionOp::Put {
                    key: STATE_KEY.into(),
                    value: serde_json::to_vec(&state.to_value())
                        .map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{Kernel, KernelConfig, LocalPersistence, Project};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_db(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "phenix-benchmark-outcomes-{name}-{}-{nonce}.sqlite",
            std::process::id()
        ))
    }

    fn kernel(path: &PathBuf) -> Kernel {
        let manifest = benchmark_outcome_manifest();
        let plugin = manifest.id.clone();
        let persistence = LocalPersistence::open(path).unwrap();
        let mut kernel =
            Kernel::with_persistence(KernelConfig::new([manifest]).unwrap(), persistence);
        kernel
            .register_embedded_factory(plugin, benchmark_outcome_factory)
            .unwrap();
        kernel.activate_all().unwrap();
        kernel
    }

    fn publish(
        kernel: &mut Kernel,
        record: BenchmarkOutcomeRecord,
    ) -> Result<BenchmarkOutcomeResponse, String> {
        let command = BenchmarkOutcomeCommand::Publish { record };
        let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
        let output = kernel
            .invoke(
                &benchmark_outcome_service(),
                &input,
                &persistence_authority(),
                None,
            )
            .map_err(|error| error.to_string())?;
        let value: PhenixValue = serde_json::from_slice(&output).unwrap();
        BenchmarkOutcomeResponse::try_from(Project(&value)).map_err(|error| error.to_string())
    }

    fn resolve(
        kernel: &mut Kernel,
        task_fixture_revision: &str,
        root_execution_id: &str,
        evaluator_identity: &str,
    ) -> EfficiencyOutcomeEvidenceResponse {
        let command = EfficiencyOutcomeEvidenceCommand::Resolve {
            request: EfficiencyOutcomeEvidenceRequest {
                task_fixture_revision: task_fixture_revision.into(),
                root_execution_id: root_execution_id.into(),
                evaluator_identity: evaluator_identity.into(),
            },
        };
        let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
        let output = kernel
            .invoke(
                &efficiency_outcome_evidence_service(),
                &input,
                &persistence_read_authority(),
                None,
            )
            .unwrap();
        let value: PhenixValue = serde_json::from_slice(&output).unwrap();
        EfficiencyOutcomeEvidenceResponse::try_from(Project(&value)).unwrap()
    }

    fn record(outcome: EvaluationOutcome) -> BenchmarkOutcomeRecord {
        BenchmarkOutcomeRecord {
            task_fixture_revision: "fixture-revision-1".into(),
            root_execution_id: "root-1".into(),
            evaluator_identity: "tests-v1".into(),
            evidence_revision: "test-result-revision-1".into(),
            outcome,
        }
    }

    #[test]
    fn published_terminal_result_resolves_as_typed_outcome_evidence_after_restart() {
        let path = temp_db("restart");
        let expected = {
            let mut kernel = kernel(&path);
            let BenchmarkOutcomeResponse::Published { evidence } =
                publish(&mut kernel, record(EvaluationOutcome::Succeeded)).unwrap()
            else {
                panic!("expected published evidence");
            };
            assert_eq!(evidence.evaluator_identity, "tests-v1");
            assert_eq!(evidence.evidence_revision, "test-result-revision-1");
            assert_eq!(evidence.outcome, EvaluationOutcome::Succeeded);
            evidence
        };

        let mut restored = kernel(&path);
        assert_eq!(
            resolve(&mut restored, "fixture-revision-1", "root-1", "tests-v1"),
            EfficiencyOutcomeEvidenceResponse::Evidence {
                evidence: Some(expected)
            }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn terminal_evidence_is_idempotent_but_cannot_be_rewritten() {
        let path = temp_db("immutable");
        let mut kernel = kernel(&path);
        let terminal = record(EvaluationOutcome::Failed);
        let first = publish(&mut kernel, terminal.clone()).unwrap();
        let replay = publish(&mut kernel, terminal).unwrap();
        assert_eq!(replay, first);

        let conflict = publish(&mut kernel, record(EvaluationOutcome::Succeeded)).unwrap_err();
        assert!(conflict.contains("different terminal evidence"));
        let unresolved = publish(&mut kernel, record(EvaluationOutcome::Unresolved)).unwrap_err();
        assert!(unresolved.contains("terminal outcomes only"));

        assert_eq!(
            resolve(
                &mut kernel,
                "fixture-revision-1",
                "unknown-root",
                "tests-v1"
            ),
            EfficiencyOutcomeEvidenceResponse::Evidence { evidence: None }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn component_exports_publish_and_shared_evidence_interfaces() {
        let component = benchmark_outcome_component_manifest();
        assert_eq!(component.imports.len(), 0);
        assert_eq!(component.exports.len(), 2);
        assert_eq!(
            component.exports[0].interface,
            BenchmarkOutcomeInterface::interface_id()
        );
        assert_eq!(
            component.exports[1].interface,
            EfficiencyOutcomeEvidenceInterface::interface_id()
        );
        assert_eq!(
            component.exports[0].required_authority,
            persistence_authority()
        );
        assert_eq!(
            component.exports[1].required_authority,
            persistence_read_authority()
        );
    }
}
