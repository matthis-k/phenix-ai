//! Independent immutable per-call receipts. No executor or Session callback cycle.
use phenix_basic_contracts::*;
use phenix_sdk::{CallError, CallResult, Host, Portable};
use sha2::{Digest, Sha256};

const BATCH: &str = "graph-tool-batch.v1";

#[derive(Debug, Clone, PartialEq, Eq, Portable)]
struct BatchRecord {
    batch: GraphToolBatch,
    sealed: bool,
}

fn receipt_key(invocation: &GraphToolInvocation) -> Result<String, phenix_sdk::Failure> {
    let digest = Sha256::digest(invocation.to_value().encode()?);
    Ok(format!(
        "graph-tool-receipt.v1.{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

fn load<T: Portable>(
    host: &Host<'_>,
    session: &str,
    key: &str,
) -> CallResult<Option<(u64, T)>, BasicError> {
    match host.invoke::<Storage::read>(
        "Storage",
        &RecordKey {
            session: session.into(),
            key: key.into(),
        },
    ) {
        Ok(record) if record.revision == 0 => Err(CallError::Domain(error(
            ErrorCode::StorageFailed,
            "zero revision tool receipt record",
        ))),
        Ok(record) => Ok(Some((
            record.revision,
            decode(&record.data).map_err(|_| {
                CallError::Domain(error(
                    ErrorCode::StorageFailed,
                    "invalid tool receipt record",
                ))
            })?,
        ))),
        Err(CallError::Domain(problem)) if problem.code == ErrorCode::NotFound => Ok(None),
        Err(failure) => Err(failure),
    }
}

fn manifest(host: &Host<'_>, session: &str) -> CallResult<(u64, BatchRecord), BasicError> {
    let (revision, record): (u64, BatchRecord) = load(host, session, BATCH)?.ok_or_else(|| {
        CallError::Domain(error(ErrorCode::NotFound, "no admitted receipt batch"))
    })?;
    if record.batch.session != session || record.batch.calls.len() > 256 {
        return Err(CallError::Domain(error(
            ErrorCode::StorageFailed,
            "foreign or oversized receipt manifest",
        )));
    }
    GraphToolRecoveryState {
        batch: record.batch.clone(),
        outcomes: vec![None; record.batch.calls.len()],
        status: RunStatus::Failed,
    }
    .validate()
    .map_err(CallError::Domain)?;
    Ok((revision, record))
}

fn read_receipt(
    host: &Host<'_>,
    invocation: &GraphToolInvocation,
) -> CallResult<Option<GraphToolOutcome>, BasicError> {
    let saved: Option<(u64, GraphToolOutcome)> =
        load(host, &invocation.session, &receipt_key(invocation)?)?;
    match saved {
        Some((_, outcome)) if outcome.invocation() != invocation => Err(CallError::Domain(error(
            ErrorCode::StorageFailed,
            "receipt identity mismatch",
        ))),
        Some((_, outcome)) => Ok(Some(outcome)),
        None => Ok(None),
    }
}

fn write_receipt(
    host: &Host<'_>,
    outcome: GraphToolOutcome,
    resolving: bool,
) -> CallResult<GraphToolOutcome, BasicError> {
    let invocation = outcome.invocation();
    phenix_basic_contracts::check_session(host, &invocation.session)?;
    let (revision, record) = manifest(host, &invocation.session)?;
    if record.sealed != resolving || !record.batch.invocations().contains(invocation) {
        return Err(CallError::Domain(error(
            ErrorCode::Denied,
            "tool receipt batch is fenced or foreign",
        )));
    }
    if let Some(saved) = read_receipt(host, invocation)? {
        return if saved == outcome {
            Ok(saved)
        } else {
            Err(CallError::Domain(error(
                ErrorCode::Conflict,
                "terminal tool receipt is immutable",
            )))
        };
    }
    // Guard the fence and immutable receipt in one existing atomic Storage CAS.
    let transaction = StorageTransaction {
        session: invocation.session.clone(),
        edits: vec![
            StorageEdit {
                key: BATCH.into(),
                expected: revision,
                data: encode(&record)?,
            },
            StorageEdit {
                key: receipt_key(invocation)?,
                expected: 0,
                data: encode(&outcome)?,
            },
        ],
    };
    let records = host.invoke::<Storage::transact>("Storage", &transaction)?;
    if records.len() != transaction.edits.len()
        || records
            .iter()
            .zip(&transaction.edits)
            .any(|(saved, edit)| !saved.matches_commit(edit.expected, &edit.data))
    {
        return Err(CallError::Kernel(phenix_sdk::Failure::new(
            phenix_sdk::Code::PluginFailed,
            "invalid atomic receipt/fence acknowledgement",
        )));
    }
    Ok(outcome)
}

fn seal_batch(
    host: &Host<'_>,
    batch: &GraphToolBatch,
) -> CallResult<Vec<Option<GraphToolOutcome>>, BasicError> {
    let (revision, mut record) = manifest(host, &batch.session)?;
    if &record.batch != batch {
        return Err(CallError::Domain(error(
            ErrorCode::Denied,
            "not the admitted receipt batch",
        )));
    }
    if !record.sealed {
        record.sealed = true;
        transact_one(
            host,
            "Storage",
            &batch.session,
            BATCH,
            revision,
            encode(&record)?,
        )?;
    }
    batch
        .invocations()
        .iter()
        .map(|invocation| read_receipt(host, invocation))
        .collect()
}

#[phenix_sdk::plugin(
    id = "phenix.basic.tool-receipts",
    version = "1.0.0",
    provide(phenix_basic_contracts::ToolReceipts, slot = "Receipts"),
    require(phenix_basic_contracts::Storage, slot = "Storage"),
    require(phenix_basic_contracts::Session, slot = "Session"),
    require(phenix_basic_contracts::GraphToolSession, slot = "Recovery")
)]
mod plugin {
    use super::*;

    #[provide(ToolReceipts::admit)]
    fn admit(host: &Host<'_>, batch: GraphToolBatch) -> CallResult<(), BasicError> {
        phenix_basic_contracts::check_session(host, &batch.session)?;
        if batch.calls.len() > 256 {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidInput,
                "receipt batch membership limit",
            )));
        }
        GraphToolRecoveryState {
            outcomes: vec![None; batch.calls.len()],
            batch: batch.clone(),
            status: RunStatus::Failed,
        }
        .validate()
        .map_err(CallError::Domain)?;
        let session = host.invoke::<Session::open>(
            "Session",
            &Scope {
                session: batch.session.clone(),
            },
        )?;
        if session.run != batch.run
            || session.status != RunStatus::Running
            || !session.pending.is_empty()
        {
            return Err(CallError::Domain(error(
                ErrorCode::Conflict,
                "receipt admission requires the current idle tool boundary",
            )));
        }
        let previous: Option<(u64, BatchRecord)> = load(host, &batch.session, BATCH)?;
        if let Some((_, previous)) = &previous {
            if previous.batch.run == batch.run
                && (!previous.sealed || previous.batch.batch >= batch.batch)
            {
                return Err(CallError::Domain(error(
                    ErrorCode::Conflict,
                    "receipt batch already admitted",
                )));
            }
        }
        let revision = previous.map_or(0, |(revision, _)| revision);
        let session = batch.session.clone();
        transact_one(
            host,
            "Storage",
            &session,
            BATCH,
            revision,
            encode(&BatchRecord {
                batch,
                sealed: false,
            })?,
        )?;
        Ok(())
    }

    #[provide(ToolReceipts::record)]
    fn record(
        host: &Host<'_>,
        outcome: GraphToolOutcome,
    ) -> CallResult<GraphToolOutcome, BasicError> {
        write_receipt(host, outcome, false)
    }

    #[provide(ToolReceipts::resolve)]
    fn resolve(
        host: &Host<'_>,
        outcome: GraphToolOutcome,
    ) -> CallResult<GraphToolOutcome, BasicError> {
        write_receipt(host, outcome, true)
    }

    #[provide(ToolReceipts::read)]
    fn read(
        host: &Host<'_>,
        invocation: GraphToolInvocation,
    ) -> CallResult<Option<GraphToolOutcome>, BasicError> {
        phenix_basic_contracts::check_session(host, &invocation.session)?;
        read_receipt(host, &invocation)
    }

    #[provide(ToolReceipts::seal)]
    fn seal(
        host: &Host<'_>,
        batch: GraphToolBatch,
    ) -> CallResult<Vec<Option<GraphToolOutcome>>, BasicError> {
        phenix_basic_contracts::check_session(host, &batch.session)?;
        seal_batch(host, &batch)
    }

    #[provide(ToolReceipts::recover)]
    fn recover(
        host: &Host<'_>,
        request: GraphToolRecovery,
    ) -> CallResult<SessionSnapshot, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        let (_, record) = manifest(host, &request.session)?;
        if record.batch.run != request.run
            || !matches!(request.status, RunStatus::Cancelled | RunStatus::Failed)
        {
            return Err(CallError::Domain(error(
                ErrorCode::Denied,
                "foreign receipt recovery",
            )));
        }
        let session = host.invoke::<Session::open>(
            "Session",
            &Scope {
                session: request.session.clone(),
            },
        )?;
        if session.run != request.run {
            return Err(CallError::Domain(error(
                ErrorCode::Conflict,
                "receipt recovery run changed",
            )));
        }
        let outcomes = seal_batch(host, &record.batch)?;
        host.invoke::<GraphToolSession::recover>(
            "Recovery",
            &GraphToolRecoveryState {
                batch: record.batch,
                outcomes,
                status: request.status,
            },
        )
    }
}
