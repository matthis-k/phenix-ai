//! Completed text-turn history using ordinary granted Storage datasets.
//! One explicitly bound dataset per conversation. No graph storage engine,
//! credentials, implicit filesystem path, or process-global session identity.

use crate::{
    AgentEvent,
    frontend::run_turn,
    model::context::{ContextSource, ModelContext},
};
use phenix_graph::{ContractValue, Failure};
use phenix_kernel::{CallbackError, host::Generation};
use phenix_storage::{Address, Codec, Error, Schema, Session};

/// Initial durable text projection. Tool activity is marked but structured
/// tool arguments/results are not restored by this representation.
#[derive(Clone, Debug, PartialEq, Eq, ContractValue)]
#[contract(schema = "agent.conversation.text-turn")]
pub struct TurnRecord {
    pub prompt: String,
    pub assistant: String,
    pub used_tools: bool,
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
    max_text_bytes: usize,
}

impl Conversation {
    pub fn new(session: Session, address: Address, max_turns: usize, max_text_bytes: usize) -> Self {
        Self {
            session,
            address,
            max_turns,
            max_text_bytes,
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
        if turn.prompt.len().saturating_add(turn.assistant.len()) > self.max_text_bytes {
            return Err(Error::Codec("conversation turn exceeds text limit".into()));
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
            return Err(Error::Codec("conversation append would create a gap".into()));
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
        if index >= self.max_turns || prompt.len() > self.max_text_bytes {
            return Err(std::io::Error::other("conversation capacity exhausted").into());
        }
        let mut record = TurnRecord {
            prompt: prompt.clone(),
            assistant: String::new(),
            used_tools: false,
        };
        let mut overflow = false;
        run_turn(generation, prompt, budget, |event| {
            match event {
                AgentEvent::Delta(text) => {
                    if record
                        .prompt
                        .len()
                        .saturating_add(record.assistant.len())
                        .saturating_add(text.len())
                        > self.max_text_bytes
                    {
                        overflow = true;
                    }
                    if !overflow {
                        record.assistant.push_str(text);
                    }
                }
                AgentEvent::ToolRequested(_) => record.used_tools = true,
                _ => {}
            }
            emit(event);
        })?;
        if overflow {
            return Err(std::io::Error::other("conversation turn exceeds text limit").into());
        }
        self.append(index, &record)?;
        Ok(())
    }
}
