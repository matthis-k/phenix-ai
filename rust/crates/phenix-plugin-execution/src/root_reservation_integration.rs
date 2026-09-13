use crate::{execution_factory, execution_manifest, execution_resource_service};
use phenix_core::{Authority, Kernel, KernelConfig, LocalPersistence, PhenixValue, Project};
use phenix_sdk::{
    BudgetActual, BudgetReservation, BudgetReservationPurpose, BudgetReservationRequest,
    ExecutionResourceCommand, ExecutionResourceResponse, RootBudgetLedger, RootBudgetLimits,
};
use std::{collections::BTreeMap, fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

fn temp_db() -> PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("phenix-root-reservation-{}-{nonce}.sqlite", std::process::id()))
}

fn authority() -> Authority {
    execution_manifest(Authority::default()).maximum_authority
}

fn kernel(path: &PathBuf) -> Kernel {
    let manifest = execution_manifest(Authority::default());
    let plugin = manifest.id.clone();
    let persistence = LocalPersistence::open(path).unwrap();
    let mut kernel = Kernel::with_persistence(KernelConfig::new([manifest]).unwrap(), persistence);
    kernel.register_embedded_factory(plugin, execution_factory).unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn invoke(kernel: &mut Kernel, command: ExecutionResourceCommand) -> ExecutionResourceResponse {
    let output = kernel.invoke(
        &execution_resource_service(),
        &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
        &authority(),
        None,
    ).unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    ExecutionResourceResponse::try_from(Project(&output)).unwrap()
}

#[test]
fn reserve_then_settle_releases_only_unused_root_step_capacity() {
    let path = temp_db();
    let mut kernel = kernel(&path);
    invoke(&mut kernel, ExecutionResourceCommand::RegisterRootBudget {
        ledger: RootBudgetLedger {
            root_execution_id: "root".into(),
            limits: RootBudgetLimits {
                fresh_input_tokens: 10_000,
                output_tokens: 2_000,
                cost_microunits: Some(10_000),
                attempts: 4,
            },
            reservations: BTreeMap::new(),
        },
    });
    invoke(&mut kernel, ExecutionResourceCommand::Reserve {
        root_execution_id: "root".into(),
        reservation: BudgetReservationRequest {
            reservation_id: "attempt-1".into(),
            parent_reservation_id: None,
            policy_revision: "policy-1".into(),
            purpose: BudgetReservationPurpose::RootStep,
            budget: BudgetReservation {
                input_tokens: 3_000,
                output_tokens: 500,
                cost_microunits: Some(2_000),
            },
            attempts: 1,
        },
    });
    let reserved = invoke(&mut kernel, ExecutionResourceCommand::Remaining {
        root_execution_id: "root".into(),
    });
    let ExecutionResourceResponse::Remaining { budget } = reserved else { panic!("expected remaining") };
    assert_eq!(budget.fresh_input_tokens, 7_000);
    assert_eq!(budget.cost_microunits, Some(8_000));

    invoke(&mut kernel, ExecutionResourceCommand::SettleReservation {
        root_execution_id: "root".into(),
        reservation_id: "attempt-1".into(),
        actual: BudgetActual {
            fresh_input_tokens: 1_200,
            output_tokens: 200,
            cost_microunits: Some(700),
            attempts: 1,
        },
    });
    let settled = invoke(&mut kernel, ExecutionResourceCommand::Remaining {
        root_execution_id: "root".into(),
    });
    let ExecutionResourceResponse::Remaining { budget } = settled else { panic!("expected remaining") };
    assert_eq!(budget.fresh_input_tokens, 8_800);
    assert_eq!(budget.output_tokens, 1_800);
    assert_eq!(budget.cost_microunits, Some(9_300));
    assert_eq!(budget.attempts, 3);
    let _ = fs::remove_file(path);
}
