//! Completed conversation turns using ordinary granted Storage datasets.
//! One explicitly bound dataset per conversation. No graph storage engine,
//! credentials, implicit filesystem path, or process-global session identity.

use crate::{
    AgentEvent, CONTRACTS_PLUGIN, FEEDBACK_CONTRACT, LOOP_PLUGIN, ModelInput, ToolCall,
    ToolOutcome, ToolValue, feedback_contract,
    frontend::run_turn_observed,
    harness::Extension,
    model::context::{ContextSource, ModelContext},
};
use phenix_graph::{
    ContractValue, Failure, OutputKey,
    compose::{NetworkPatch, PatchBuilder, PatchComponent},
};
use phenix_kernel::{CallbackError, Contribution, ImportSpec, Key, host::Generation};
use phenix_plugin::{PluginInstance, Registrar};
use phenix_storage::{Address, Codec, Error, Schema, Session};
use std::cell::Cell;

/// An ordered exchange at a UTF-8 byte boundary in the assistant text.
/// The failure is absent on success. No live invocation or provider handle
/// escapes into durable history.
#[derive(Clone, Debug, PartialEq, Eq, ContractValue)]
#[contract(schema = "agent.conversation.tool-exchange")]
pub struct ToolExchange {
    pub position: u64,
    pub call: ToolCall,
    pub output: ToolValue,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, ContractValue)]
#[contract(schema = "agent.conversation.turn")]
pub struct TurnRecord {
    pub prompt: String,
    pub assistant: String,
    pub tools: Vec<ToolExchange>,
}

const OBSERVER: &str = "conversation-feedback";
const CAPTURE: &str = "conversation-feedback";
const IMPORT: ImportSpec = ImportSpec::new("feedback", CONTRACTS_PLUGIN, FEEDBACK_CONTRACT);

struct FeedbackCapture;

impl PluginInstance for FeedbackCapture {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), phenix_plugin::Failure> {
        Contribution::dynamic("main", |context| {
            let feedback = IMPORT.bind(context)?;
            Ok(capture_patch(feedback.path())?)
        })
        .require(&IMPORT)
        .register(registrar)
    }
}

fn capture_patch(path: &str) -> Result<NetworkPatch, phenix_graph::NetworkError> {
    let feedback = PatchComponent::at(path, feedback_contract().required().clone());
    let mut patch = PatchBuilder::new(crate::capacity());
    patch.expose_output(
        CAPTURE,
        &feedback.output(OutputKey::<ModelInput>::new("output"))?,
    );
    Ok(patch.finish())
}

/// Select this ordinary extension when the frontend wants durable tool
/// exchanges. It observes the existing checked feedback seam.
pub fn extension() -> Extension {
    Extension::new(OBSERVER, FeedbackCapture, |options, _| {
        let import = Key::new(OBSERVER, "feedback");
        let offer = Key::new(LOOP_PLUGIN, "feedback");
        if let Some(allow) = &mut options.imports.allow {
            allow.insert((import.clone(), offer.clone()));
        }
        options.imports.selections.insert(import, offer);
        options
            .composition
            .entry(Key::new(OBSERVER, "main"))
            .or_default()
            .grants
            .extend(capture_patch(crate::TOOL_FEEDBACK)?.footprints());
        Ok(())
    })
}

pub struct TurnCodec;

impl Codec for TurnCodec {
    type Value = TurnRecord;

    fn schema(&self) -> Schema {
        TurnRecord::schema()
    }

    fn encode(&self, value: &TurnRecord) -> phenix_storage::Result<Vec<u8>> {
        Ok(value.encode())
    }

    fn decode(&self, bytes: &[u8]) -> phenix_storage::Result<TurnRecord> {
        TurnRecord::decode(bytes).map_err(|error| Error::CodecCause {
            summary: "decode conversation turn",
            cause: Box::new(error),
        })
    }
}

/// Bounded append-only view over a host-granted dataset. The host must not
/// delete or overwrite its records. Competing appends at the same sequence
/// conflict atomically rather than overwriting a committed turn.
#[derive(Clone)]
pub struct Conversation {
    session: Session,
    address: Address,
    max_turns: usize,
    max_turn_bytes: usize,
}

impl Conversation {
    pub fn new(
        session: Session,
        address: Address,
        max_turns: usize,
        max_turn_bytes: usize,
    ) -> Self {
        Self {
            session,
            address,
            max_turns,
            max_turn_bytes,
        }
    }

    pub fn load(&self) -> phenix_storage::Result<Vec<TurnRecord>> {
        let mut turns = Vec::new();
        for index in 0..self.max_turns {
            let Some(turn) = self
                .session
                .get(&self.address, &index.to_string(), &TurnCodec)?
            else {
                break;
            };
            self.validate(&turn)?;
            turns.push(turn);
        }
        Ok(turns)
    }

    fn validate(&self, turn: &TurnRecord) -> phenix_storage::Result<()> {
        if turn.encode().len() > self.max_turn_bytes {
            return Err(Error::Codec("conversation turn exceeds byte limit".into()));
        }
        let mut previous = 0;
        for exchange in &turn.tools {
            let position = usize::try_from(exchange.position)
                .map_err(|_| Error::Codec("invalid tool position".into()))?;
            if position < previous || !turn.assistant.is_char_boundary(position) {
                return Err(Error::Codec("invalid tool position".into()));
            }
            previous = position;
        }
        Ok(())
    }

    pub fn append(&self, index: usize, turn: &TurnRecord) -> phenix_storage::Result<()> {
        self.validate(turn)?;
        if index >= self.max_turns {
            return Err(Error::Codec("conversation capacity exhausted".into()));
        }
        if index > 0
            && self
                .session
                .get(&self.address, &(index - 1).to_string(), &TurnCodec)?
                .is_none()
        {
            return Err(Error::Codec(
                "conversation append would create a gap".into(),
            ));
        }
        self.session
            .compare_exchange(&self.address, &index.to_string(), None, turn, &TurnCodec)?;
        Ok(())
    }

    /// Load prior committed turns at the next prompt, not at plugin startup.
    /// The context adapter freezes the returned view for that feedback cycle.
    pub fn context(&self, base: ModelContext) -> ContextSource {
        let history = self.clone();
        ContextSource::new(move || {
            let mut context = base.clone();
            context.history = history
                .load()
                .map_err(|error| Failure::application_message(error.to_string()))?;
            Ok(context)
        })
    }

    /// Persist only after the native request has settled successfully.
    /// Streaming events may already be visible when persistence fails;
    /// the returned error must not be interpreted as external-effect rollback.
    pub fn run_turn(
        &self,
        generation: &Generation,
        prompt: String,
        budget: usize,
        mut emit: impl FnMut(&AgentEvent),
    ) -> Result<(), CallbackError> {
        let index = self.load()?.len();
        if index >= self.max_turns || prompt.len() > self.max_turn_bytes {
            return Err(std::io::Error::other("conversation capacity exhausted").into());
        }
        let capture = generation
            .compiled()
            .bind_capture(CAPTURE, OutputKey::<ModelInput>::new(CAPTURE))?;
        let mut assistant = String::new();
        let mut requested = Vec::new();
        let mut completed = Vec::new();
        let bytes = Cell::new(prompt.len());
        let overflow = Cell::new(false);
        let account = |amount: usize| {
            bytes.set(bytes.get().saturating_add(amount));
            if bytes.get() > self.max_turn_bytes {
                overflow.set(true);
            }
            !overflow.get()
        };
        run_turn_observed(
            generation,
            prompt.clone(),
            budget,
            vec![capture],
            |event| {
                match event {
                    AgentEvent::Delta(text) => {
                        if account(text.len()) {
                            assistant.push_str(text);
                        }
                    }
                    AgentEvent::ToolRequested(call) => {
                        if account(call.encode().len()) {
                            requested.push((assistant.len() as u64, call.clone()));
                        }
                    }
                    _ => {}
                }
                emit(event);
            },
            |_, packet| {
                let Some(ModelInput::ToolResult(result)) = packet.value().borrow::<ModelInput>()
                else {
                    return Err(std::io::Error::other("expected correlated tool result").into());
                };
                let (output, failure) = match result.outcome() {
                    ToolOutcome::Succeeded { output } => (output, None),
                    ToolOutcome::Failed { output, reason } => (output, Some(reason)),
                };
                if account(
                    output.encode().len().saturating_add(failure.map_or(0, String::len)),
                ) {
                    completed.push((
                        result.invocation().call().clone(),
                        output.clone(),
                        failure.cloned(),
                    ));
                }
                Ok(())
            },
        )?;
        if overflow.get() || requested.len() != completed.len() {
            return Err(std::io::Error::other("incomplete or oversized conversation turn").into());
        }
        let mut tools = Vec::new();
        for ((position, call), (actual, output, failure)) in requested.into_iter().zip(completed) {
            if call != actual {
                return Err(std::io::Error::other("conversation tool feedback mismatch").into());
            }
            tools.push(ToolExchange {
                position,
                call,
                output,
                failure,
            });
        }
        let record = TurnRecord {
            prompt,
            assistant,
            tools,
        };
        self.append(index, &record)?;
        Ok(())
    }
}
