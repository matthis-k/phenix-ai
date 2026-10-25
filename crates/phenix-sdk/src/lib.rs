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

/// Declare a graph subscription using the event and operation types.
///
/// Both identities are taken from existing #[event] and #[contract] metadata;
/// no manually synchronized event ID or operation string is necessary.
/// The result remains an ordinary M1 graph node and edge: declaration order
/// has no effect on scheduling, and the kernel checks authorization offline.
///
/// Example:
/// ```ignore
/// phenix_sdk::on_event!("populate", ProvidersPopulate, ProviderPopulate::register)
/// ```
///
/// A literal operation string is still supported for compatibility with
/// dynamically selected operations, but typed markers should be preferred.
#[macro_export]
macro_rules! on_event {
    ($id:expr, $event:ty, $operation:literal $(,)?) => {
        $crate::StaticGraphNode::on(
            $id,
            &[<$event as $crate::GraphEventPayload>::ID],
            $operation,
        )
    };
    ($id:expr, $event:ty, $operation:ty $(,)?) => {
        $crate::StaticGraphNode::on(
            $id,
            &[<$event as $crate::GraphEventPayload>::ID],
            <$operation as $crate::Operation>::KEY,
        )
    };
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
    fn arbitrary_graph_declaration_order_has_identical_native_metadata() {
        // These graphs describe the same event-to-node triggers and ordering
        // edges, but deliberately permute every authored collection.
        const ALPHA: StaticEventDefinition = StaticEventDefinition {
            id: "example.alpha",
            payload: StaticSchema::String,
        };
        const ZETA: StaticEventDefinition = StaticEventDefinition {
            id: "example.zeta",
            payload: StaticSchema::String,
        };
        const A: StaticActionDefinition = StaticActionDefinition {
            id: "example.action-a",
            event: "example.alpha",
            description: "Alpha",
            requires: &["example.task@1/zeta", "example.task@1/alpha"],
        };
        const B: StaticActionDefinition = StaticActionDefinition {
            id: "example.action-b",
            event: "example.zeta",
            description: "Zeta",
            requires: &[],
        };
        const FIRST: StaticGraphContribution = StaticGraphContribution {
            plugin: "example",
            events: &[ZETA, ALPHA],
            actions: &[B, A],
            nodes: &[
                StaticGraphNode::on(
                    "finish",
                    &["example.zeta", "example.alpha"],
                    "example.task@1/finish",
                )
                .after(&["example/zeta", "example/alpha"]),
                StaticGraphNode::on("zeta", &["example.zeta"], "example.task@1/zeta"),
                StaticGraphNode::on("alpha", &["example.alpha"], "example.task@1/alpha"),
            ],
        };
        const SECOND: StaticGraphContribution = StaticGraphContribution {
            plugin: "example",
            events: &[ALPHA, ZETA],
            actions: &[A, B],
            nodes: &[
                StaticGraphNode::on("alpha", &["example.alpha"], "example.task@1/alpha"),
                StaticGraphNode::on(
                    "finish",
                    &["example.alpha", "example.zeta"],
                    "example.task@1/finish",
                )
                .after(&["example/alpha", "example/zeta"]),
                StaticGraphNode::on("zeta", &["example.zeta"], "example.task@1/zeta"),
            ],
        };
        const FIRST_BYTES: MetadataBytes = descriptor::static_metadata::graph(FIRST);
        const SECOND_BYTES: MetadataBytes = descriptor::static_metadata::graph(SECOND);
        let canonical = FIRST.owned().unwrap();
        assert_eq!(FIRST_BYTES.bytes(), SECOND_BYTES.bytes());
        assert_eq!(FIRST_BYTES.bytes(), canonical.encode().unwrap());
        assert_eq!(
            SECOND_BYTES.bytes(),
            SECOND.owned().unwrap().encode().unwrap()
        );
        assert_eq!(
            GraphContribution::decode(FIRST_BYTES.bytes()).unwrap(),
            canonical
        );
        assert_eq!(
            canonical
                .nodes
                .iter()
                .map(|node| node.id.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "finish", "zeta"]
        );
        assert_eq!(canonical.nodes[1].after, ["example/alpha", "example/zeta"]);
    }

    struct ExampleSubmit;
    impl Operation for ExampleSubmit {
        type Request = String;
        type Response = String;
        type Error = String;
        const KEY: &'static str = "example.input@1/submit";
    }

    #[test]
    fn typed_subscription_derives_both_ids_and_matches_literal_encoding() {
        const TYPED: StaticGraphNode = on_event!("receive", Prompt, ExampleSubmit);
        const LITERAL: StaticGraphNode =
            on_event!("receive", Prompt, "example.input@1/submit");
        assert_eq!(TYPED.id, LITERAL.id);
        assert_eq!(TYPED.triggers, LITERAL.triggers);
        assert_eq!(TYPED.operation, LITERAL.operation);
        const TYPED_GRAPH: StaticGraphContribution = StaticGraphContribution {
            plugin: "example",
            events: &[Prompt::EVENT],
            actions: &[],
            nodes: &[TYPED],
        };
        const LITERAL_GRAPH: StaticGraphContribution = StaticGraphContribution {
            plugin: "example",
            events: &[Prompt::EVENT],
            actions: &[],
            nodes: &[LITERAL],
        };
        const TYPED_BYTES: MetadataBytes = descriptor::static_metadata::graph(TYPED_GRAPH);
        const LITERAL_BYTES: MetadataBytes = descriptor::static_metadata::graph(LITERAL_GRAPH);
        assert_eq!(TYPED_BYTES.bytes(), LITERAL_BYTES.bytes());
    }

    #[test]
    fn typed_subscription_uses_event_contract_not_repeated_string() {
        const HOOK: StaticGraphNode = on_event!("receive", Prompt, "example.input@1/submit");
        assert_eq!(HOOK.id, "receive");
        assert_eq!(HOOK.triggers, ["example.prompt"]);
        assert_eq!(HOOK.operation, "example.input@1/submit");
        assert_eq!(HOOK.failure, FailurePolicy::FailRoot);
        const GRAPH: StaticGraphContribution = StaticGraphContribution {
            plugin: "example",
            events: &[Prompt::EVENT],
            actions: &[],
            nodes: &[HOOK],
        };
        const META: MetadataBytes = descriptor::static_metadata::graph(GRAPH);
        assert_eq!(META.bytes(), GRAPH.owned().unwrap().encode().unwrap());
    }

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
