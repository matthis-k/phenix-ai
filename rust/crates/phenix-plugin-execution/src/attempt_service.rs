use phenix_core::{
    ComponentInterface, DurableSchema, PluginContext, PluginHost, PluginInstance,
    ResourceNamespace, ServiceId, TransactionOp,
};
use phenix_sdk::{
    step_attempt_service, AttemptOutcome, StepAttemptCommand, StepAttemptInterface,
    StepAttemptPhase, StepAttemptRecord, StepAttemptResponse, StepPlan, UsageAttemptKind,
    UsageAttribution,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const ATTEMPT_NAMESPACE: &str = "phenix.execution.attempts.state";
pub(crate) const ATTEMPT_STATE_KEY: &str = "state";
const MAX_ATTEMPT_STATE_BYTES: usize = 16 * 1024 * 1024;

type AttemptContext<'host, 'runtime> = PluginContext<'host, 'runtime, ()>;

fn context<'host, 'runtime>(host: &'host PluginHost<'runtime>) -> AttemptContext<'host, 'runtime> {
    PluginContext::new(host, (), (), ())
}

pub(crate) fn attempt_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(ATTEMPT_NAMESPACE).expect("static namespace is valid")
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct AttemptLedger {
    attempts: BTreeMap<String, StepAttemptRecord>,
}

impl AttemptLedger {
    fn create(
        &mut self,
        attribution: UsageAttribution,
        plan: StepPlan,
    ) -> Result<StepAttemptRecord, String> {
        if self.attempts.contains_key(&attribution.attempt_id) {
            return Err(format!(
                "step attempt already exists: {}",
                attribution.attempt_id
            ));
        }
        if attribution.parent_attempt_id.as_deref() == Some(attribution.attempt_id.as_str()) {
            return Err("step attempt cannot parent itself".into());
        }
        match attribution.kind {
            UsageAttemptKind::Root if attribution.parent_attempt_id.is_some() => {
                return Err("root step attempt cannot have a parent attempt".into())
            }
            UsageAttemptKind::Retry if attribution.parent_attempt_id.is_none() => {
                return Err("retry step attempt requires a parent attempt".into())
            }
            _ => {}
        }
        if let Some(parent_id) = &attribution.parent_attempt_id {
            let parent = self
                .attempts
                .get(parent_id)
                .ok_or_else(|| format!("unknown parent step attempt: {parent_id}"))?;
            if parent.attribution.root_execution_id != attribution.root_execution_id {
                return Err("step attempt parent belongs to a different root execution".into());
            }
            if attribution.kind == UsageAttemptKind::Retry {
                if parent.phase != StepAttemptPhase::Settled {
                    return Err("retry parent step attempt is not settled".into());
                }
                if parent.outcome == Some(AttemptOutcome::Succeeded) {
                    return Err("successful step attempt cannot be retried".into());
                }
            }
        }
        let record = StepAttemptRecord::new(attribution, plan)
            .map_err(|error| format!("step attempt creation failed: {error:?}"))?;
        self.attempts
            .insert(record.attribution.attempt_id.clone(), record.clone());
        Ok(record)
    }

    fn get(&self, attempt_id: &str) -> Option<&StepAttemptRecord> {
        self.attempts.get(attempt_id)
    }

    fn list_root(&self, root_execution_id: &str) -> Vec<StepAttemptRecord> {
        self.attempts
            .values()
            .filter(|record| record.attribution.root_execution_id == root_execution_id)
            .cloned()
            .collect()
    }

    fn mutate<F>(&mut self, attempt_id: &str, mutation: F) -> Result<StepAttemptRecord, String>
    where
        F: FnOnce(&mut StepAttemptRecord) -> Result<(), String>,
    {
        let record = self
            .attempts
            .get_mut(attempt_id)
            .ok_or_else(|| format!("unknown step attempt: {attempt_id}"))?;
        mutation(record)?;
        Ok(record.clone())
    }

    pub(crate) fn settle(
        &mut self,
        attempt_id: &str,
        outcome: AttemptOutcome,
    ) -> Result<StepAttemptRecord, String> {
        self.mutate(attempt_id, |attempt| {
            attempt
                .settle(outcome)
                .map_err(|error| format!("attempt settlement failed: {error:?}"))
        })
    }
}

#[must_use]
pub(crate) fn attempt_factory() -> Box<dyn PluginInstance> {
    Box::new(AttemptPlugin)
}

struct AttemptPlugin;

impl PluginInstance for AttemptPlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        let context = context(host);
        context
            .kernel
            .register_durable_schema(&DurableSchema::new(attempt_namespace(), 1))
            .map_err(|error| error.to_string())?;
        let snapshot = context
            .kernel
            .read_durable(&attempt_namespace(), ATTEMPT_STATE_KEY)
            .map_err(|error| error.to_string())?;
        restore(snapshot.as_deref()).map(|_| ())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &step_attempt_service() {
            return Err(format!("unsupported step attempt service: {service}"));
        }
        let context = context(host);
        let command = context
            .kernel
            .decode_projected::<StepAttemptCommand>(&StepAttemptInterface::interface_id(), input)
            .map_err(|error| error.to_string())?;
        let old = context
            .kernel
            .read_durable(&attempt_namespace(), ATTEMPT_STATE_KEY)
            .map_err(|error| error.to_string())?;
        let ledger = restore(old.as_deref())?;
        let response = if matches!(
            &command,
            StepAttemptCommand::Get { .. } | StepAttemptCommand::ListRoot { .. }
        ) {
            read(&ledger, command)?
        } else {
            mutate(&context, old, ledger, command)?
        };
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn read(
    ledger: &AttemptLedger,
    command: StepAttemptCommand,
) -> Result<StepAttemptResponse, String> {
    match command {
        StepAttemptCommand::Get { attempt_id } => Ok(StepAttemptResponse::AttemptLookup {
            attempt: ledger.get(&attempt_id).cloned(),
        }),
        StepAttemptCommand::ListRoot { root_execution_id } => Ok(StepAttemptResponse::Attempts {
            attempts: ledger.list_root(&root_execution_id),
        }),
        _ => Err("mutating step attempt command reached read path".into()),
    }
}

fn mutate(
    context: &AttemptContext<'_, '_>,
    old: Option<Vec<u8>>,
    mut next: AttemptLedger,
    command: StepAttemptCommand,
) -> Result<StepAttemptResponse, String> {
    let attempt = match command {
        StepAttemptCommand::Create { attribution, plan } => next.create(attribution, plan)?,
        StepAttemptCommand::BindReservation {
            attempt_id,
            reservation_id,
        } => next.mutate(&attempt_id, |attempt| {
            attempt
                .bind_reservation(reservation_id)
                .map_err(|error| format!("reservation binding failed: {error:?}"))
        })?,
        StepAttemptCommand::BindRoute {
            attempt_id,
            decision,
        } => next.mutate(&attempt_id, |attempt| {
            attempt
                .bind_route(decision)
                .map_err(|error| format!("route binding failed: {error:?}"))
        })?,
        StepAttemptCommand::BindProjection {
            attempt_id,
            projection,
        } => next.mutate(&attempt_id, |attempt| {
            attempt
                .bind_projection(projection)
                .map_err(|error| format!("projection binding failed: {error:?}"))
        })?,
        StepAttemptCommand::MarkDispatched {
            attempt_id,
            dispatch_id,
        } => next.mutate(&attempt_id, |attempt| {
            attempt
                .mark_dispatched(dispatch_id)
                .map_err(|error| format!("dispatch binding failed: {error:?}"))
        })?,
        StepAttemptCommand::Settle {
            attempt_id,
            outcome,
        } => next.settle(&attempt_id, outcome)?,
        StepAttemptCommand::Get { .. } | StepAttemptCommand::ListRoot { .. } => {
            return Err("read-only step attempt command reached mutation path".into())
        }
    };
    let encoded = encode_ledger(&next)?;
    context
        .kernel
        .transact_durable(
            &attempt_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: ATTEMPT_STATE_KEY.into(),
                    expected: old,
                },
                TransactionOp::Put {
                    key: ATTEMPT_STATE_KEY.into(),
                    value: encoded,
                },
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(StepAttemptResponse::Attempt { attempt })
}

pub(crate) fn encode_ledger(ledger: &AttemptLedger) -> Result<Vec<u8>, String> {
    let encoded = serde_json::to_vec(ledger).map_err(|error| error.to_string())?;
    if encoded.len() > MAX_ATTEMPT_STATE_BYTES {
        return Err(format!(
            "step attempt state exceeds {MAX_ATTEMPT_STATE_BYTES} bytes"
        ));
    }
    Ok(encoded)
}

pub(crate) fn restore(snapshot: Option<&[u8]>) -> Result<AttemptLedger, String> {
    let Some(bytes) = snapshot else {
        return Ok(AttemptLedger::default());
    };
    if bytes.len() > MAX_ATTEMPT_STATE_BYTES {
        return Err(format!(
            "step attempt state exceeds {MAX_ATTEMPT_STATE_BYTES} bytes"
        ));
    }
    serde_json::from_slice(bytes).map_err(|error| error.to_string())
}
