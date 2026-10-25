extern crate self as phenix_sdk;
pub mod live;
pub use live::{Host, ImportCall, ImportPoll, PendingImport, PendingResult};
pub use phenix_contract::*;
pub use phenix_macros::{action, contract, event, plugin, property, Portable};

/// An event type whose stable identity and payload schema come from the
/// #[phenix_sdk::event] declaration. Host::emit_event keeps callers typed.
pub trait GraphEventPayload: live::Portable {
    const ID: &'static str;
}

#[doc(hidden)]
pub fn lifecycle(hook: impl FnOnce() -> Result<(), Failure>) -> Result<(), Failure> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(hook))
        .unwrap_or_else(|_| Err(Failure::new(Code::PluginFailed, "caught lifecycle panic")))
}
/// A copied worker permit authorizes wake only, never ordinary host imports.
#[derive(Clone, Copy)]
pub struct WakePermit {
    ticket: u64,
    permit: u64,
    wake: unsafe extern "C" fn(u64, u64) -> u32,
}
impl WakePermit {
    pub fn wake(&self) -> bool {
        unsafe { (self.wake)(self.permit, self.ticket) == 0 }
    }
}

#[cfg(test)]
mod graph_declaration_tests {
    use super::*;

    #[event(id = "example.prompt")]
    #[derive(Portable)]
    pub struct Prompt {
        pub text: String,
    }

    #[action(
        id = "example.submit",
        event = "example.prompt",
        description = "Submit a prompt",
        requires = "example.input@1/submit"
    )]
    pub struct Submit;

    #[test]
    fn macros_emit_const_sdk_declarations() {
        const GRAPH: StaticGraphContribution = StaticGraphContribution {
            plugin: "example",
            events: &[Prompt::EVENT],
            actions: &[Submit::ACTION],
            nodes: &[],
        };
        const GRAPH_BYTES: MetadataBytes = descriptor::static_metadata::graph(GRAPH);
        let metadata = GRAPH.owned().unwrap();
        assert_eq!(<Prompt as GraphEventPayload>::ID, "example.prompt");
        assert_eq!(GRAPH_BYTES.bytes(), metadata.encode().unwrap());
        assert_eq!(metadata.events[0].id, "example.prompt");
        assert_eq!(
            metadata.events[0].payload,
            ValueSchema::Struct(std::collections::BTreeMap::from([(
                "text".into(),
                ValueSchema::String
            )]))
        );
        assert_eq!(metadata.actions[0].requires, vec!["example.input@1/submit"]);
        assert_eq!(
            GraphContribution::decode(&metadata.encode().unwrap()).unwrap(),
            metadata
        );
    }
}
