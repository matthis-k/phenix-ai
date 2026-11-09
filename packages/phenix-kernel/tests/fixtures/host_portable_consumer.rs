use phenix_graph::{ContractValue, Node, Schema};
use phenix_kernel::{ContractRequirement, Contribution, Key};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::sync::{Arc, Mutex};

pub struct Request(pub i64);
pub struct Response(pub i64);

macro_rules! wire_value {
    ($ty:ident, $schema:literal) => {
        impl ContractValue for $ty {
            const SCHEMA: &'static str = $schema;
            fn schema_descriptor() -> Option<Schema> {
                Some(Schema {
                    name: $schema.into(),
                    revision: 1,
                    structure: "record{value:i64}".into(),
                    codec: "i64-le-v1".into(),
                })
            }
            fn encode(&self) -> Vec<u8> {
                self.0.to_le_bytes().to_vec()
            }
            fn decode(bytes: &[u8]) -> Result<Self, String> {
                Ok(Self(i64::from_le_bytes(
                    bytes.try_into().map_err(|_| "consumer: invalid i64")?,
                )))
            }
        }
    };
}
wire_value!(Request, "host-portable/request");
wire_value!(Response, "host-portable/response");

pub struct Plugin(pub Arc<Mutex<Vec<i64>>>);

impl PluginInstance for Plugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        into.add(ContractRequirement {
            id: "transform".into(),
            consumer: Key::new(into.owner(), "main"),
            contract: Key::new("definition", "transform"),
        })?;
        let observed = self.0.clone();
        into.add(Contribution {
            id: "main".into(),
            requires: vec![],
            routes: vec![],
            compose: Box::new(move |context| {
                let selected = context.import("transform")?;
                let graph = &mut *context.graph;
                let bound = selected.bind(graph).map_err(|e| e.to_string())?;
                // Only contract-visible names and C's own Rust types are used.
                assert!(bound.wire_output::<Response>("diagnostic").is_err());
                assert!(bound.wire_input::<Request>("run", "extra").is_err());
                let left = bound
                    .wire_input::<Request>("run", "left")
                    .map_err(|e| e.to_string())?;
                let right = bound
                    .wire_input::<Request>("run", "right")
                    .map_err(|e| e.to_string())?;
                let output = bound
                    .wire_output::<Response>("value")
                    .map_err(|e| e.to_string())?;
                let sink = graph
                    .node(
                        "consumer-sink",
                        Node::wire({
                            let observed = observed.clone();
                            move |response: Response| {
                                observed.lock().unwrap().push(response.0);
                                response
                            }
                        }),
                    )
                    .map_err(|e| e.to_string())?;
                graph
                    .connect_wire(output, sink.inlet())
                    .map_err(|e| e.to_string())?;
                let mut inputs = Vec::new();
                for (name, target) in [("left", left), ("right", right)] {
                    let source = graph
                        .node(format!("consumer-{name}"), Node::wire(|v: Request| v))
                        .map_err(|e| e.to_string())?;
                    graph
                        .connect_wire(source.output(), target)
                        .map_err(|e| e.to_string())?;
                    inputs.push(graph.expose_input(name, source.inlet())?);
                }
                graph.trigger("run", inputs)?;
                Ok(())
            }),
        })
    }
}
