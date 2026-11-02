#[phenix_sdk::plugin(
    id = "experiment.fixture",
    version = "1.0.0",
    define(dataflow_native_contracts::Prepare),
    define(dataflow_native_contracts::Service),
    define(dataflow_native_contracts::Complete),
    provide(dataflow_native_contracts::Prepare, slot = "Prepare"),
    provide(dataflow_native_contracts::Service, slot = "Service"),
    provide(dataflow_native_contracts::Complete, slot = "Complete")
)]
mod plugin {
    use dataflow_native_contracts::{Branch, Complete, Error, Prepare, Service, Text};
    use phenix_sdk::{CallResult, Host};
    #[derive(Default)]
    pub struct State;
    #[provide(Prepare::run)]
    fn prepare(_: &mut State, _: &Host<'_>, request: Text) -> CallResult<Branch, Error> {
        let m: dataflow_fixture::Message = serde_json::from_str(&request.text).unwrap();
        let p = dataflow_fixture::prepare(m, &Default::default());
        let text = Text {
            text: serde_json::to_string(&p.0).unwrap(),
        };
        Ok(if p.0.tool {
            Branch::Tool { text: text.text }
        } else {
            Branch::Model { text: text.text }
        })
    }
    #[provide(Service::run)]
    fn service(_: &mut State, _: &Host<'_>, request: Text) -> CallResult<Text, Error> {
        let m: dataflow_fixture::Message = serde_json::from_str(&request.text).unwrap();
        assert!(
            !m.pending,
            "native pending covered separately by existing acceptance"
        );
        let r = dataflow_fixture::ResultValue {
            id: m.id,
            text: dataflow_fixture::response(&m, ""),
        };
        // ResultValue is serialized by host-independent fixture wire representation below.
        Ok(Text {
            text: serde_json::to_string(&(r.id, r.text)).unwrap(),
        })
    }
    #[provide(Complete::run)]
    fn complete(_: &mut State, _: &Host<'_>, request: Text) -> CallResult<Text, Error> {
        Ok(request)
    }
}
#[used]
#[no_mangle]
#[link_section = ".phenix.graph"]
pub static PHENIX_GRAPH_V2: [u8; phenix_sdk::NATIVE_SECTION_BYTES] =
    *include_bytes!(concat!(env!("OUT_DIR"), "/graph.bin"));
