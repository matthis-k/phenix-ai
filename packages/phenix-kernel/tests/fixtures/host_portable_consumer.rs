use phenix_graph::{
    ContractValue, Interface, PortDef, Schema,
    component::ComponentDef,
    compose::{Edit, NetworkPatch},
    network::{ConnectionDef, PortPath},
    process::{Process, ProcessContext, Step},
};
use phenix_kernel::{CandidateArtifacts, ContractRequirement, Contribution, Key};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::sync::{Arc, Mutex};

pub struct Request(pub i64);
pub struct Response(pub i64);

macro_rules! wire_value {
    ($ty:ident, $schema:literal) => {
        impl ContractValue for $ty {
            fn schema() -> Schema {
                Schema {
                    name: $schema.into(),
                    revision: 1,
                    structure: "record{value:i64}".into(),
                    codec: "i64-le-v1".into(),
                }
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

struct Sink(Arc<Mutex<Vec<i64>>>);
impl Process for Sink {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
        let Some(value) = context.take("value") else {
            return Ok(Step::Finished);
        };
        self.0
            .lock()
            .unwrap()
            .push(Response::decode(value.bytes())?.0);
        Ok(Step::Complete)
    }
}

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
            patch: NetworkPatch::new([]),
            prepare: Some(Box::new(move |context| {
                let selected = context.import("transform")?;
                let bound = selected.bind(context.compiled)?;
                // Only contract-visible names and C's own Rust types are used.
                assert!(bound.wire_input::<Request>("extra").is_err());
                bound.wire_input::<Request>("left")?;
                bound.wire_input::<Request>("right")?;
                bound.wire_output::<Response>("value")?;
                let capacity = std::num::NonZeroUsize::new(1).unwrap();
                let port = |process: &str, name: &str| PortPath {
                    process: process.into(),
                    port: name.into(),
                };
                let observed = observed.clone();
                let sink = ComponentDef::primitive(
                    Arc::new(Interface::new([PortDef::wire::<Response>("value")?], [])?),
                    Arc::new(move || Sink(observed.clone())),
                );
                let mut edits = vec![
                    Edit::Add {
                        path: "consumer-sink".into(),
                        definition: Arc::new(sink),
                    },
                    Edit::Connect(ConnectionDef {
                        from: port(bound.path(), "value"),
                        to: port("consumer-sink", "value"),
                        capacity,
                    }),
                ];
                for name in ["left", "right"] {
                    edits.push(Edit::ExposeInput {
                        name: name.into(),
                        targets: vec![port(bound.path(), name)],
                        capacity,
                    });
                }
                Ok(CandidateArtifacts {
                    patch: Some(NetworkPatch::new(edits)),
                    ..Default::default()
                })
            })),
        })
    }
}
