use phenix_sdk::live::Operation;
use phenix_sdk::{abi::*, contract, live, *};

#[contract(id = "test.stateless@1")]
trait Text {
    fn uppercase(&self, text: String) -> Result<String, String>;
    fn checked(&self, text: String) -> Result<String, String>;
    fn kernel_error(&self, text: String) -> Result<String, String>;
    fn constant(&self) -> Result<u64, String>;
}

#[plugin(id = "test.stateless", define(Text), provide(Text, slot = "text"))]
mod plugin {
    use super::*;
    #[provide(Text::uppercase)]
    fn uppercase(text: String) -> String {
        text.to_uppercase()
    }
    type Checked = Result<String, String>;
    #[provide(Text::checked)]
    fn checked(text: String) -> Checked {
        if text.is_empty() {
            Err("empty".into())
        } else {
            Ok(text)
        }
    }
    #[provide(Text::kernel_error)]
    fn kernel_error(_: String) -> CallResult<String, String> {
        Err(Failure::new(Code::Denied, "handler denial").into())
    }
    #[provide(Text::constant)]
    fn constant() -> u64 {
        42
    }
}

unsafe extern "C" fn unexpected_state(_: u64, _: u32, _: Bytes) -> Reply {
    panic!("stateless plugin must not create a cell")
}
unsafe extern "C" fn unexpected_import(_: u64, _: Bytes, _: Bytes, _: Bytes) -> Reply {
    panic!("stateless plugin has no import")
}
unsafe extern "C" fn wake(_: u64, _: u64) -> u32 {
    0
}

#[test]
fn stateless_handlers_have_typed_results_and_generated_v3_lifecycle() {
    let metadata = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
    assert_eq!(metadata.version, "0.1.0");
    assert!(metadata.properties.is_empty());
    assert!(metadata.requires.is_empty());
    let table = unsafe { StatePluginTable::negotiate(plugin::phenix_native_v3()) }.unwrap();
    let host = StateHostApi {
        header: Header::state(std::mem::size_of::<StateHostApi>()),
        scope: 7,
        ticket: 0,
        permit: 0,
        invoke: Some(unexpected_import),
        state: Some(unexpected_state),
        wake: Some(wake),
    };
    let mut instance = std::ptr::null_mut();
    assert_eq!(
        unsafe { table.create.unwrap()(&mut instance).consume_state() }.unwrap(),
        (0, Value::Null)
    );
    assert_eq!(
        unsafe { table.initialize.unwrap()(instance, &host).consume_state() }.unwrap(),
        (0, Value::Null)
    );
    let invoke = |operation: &str, value: Value| {
        let bytes = value.encode_profile(WireProfile::V2).unwrap();
        unsafe {
            table.invoke.unwrap()(
                instance,
                &host,
                Bytes::new(operation.as_bytes()),
                Bytes::new(&bytes),
            )
            .consume_state()
        }
        .unwrap()
    };
    assert_eq!(
        invoke(Text::uppercase::KEY, Value::String("phenix".into())),
        (0, Value::String("PHENIX".into()))
    );
    assert_eq!(
        invoke(Text::checked::KEY, Value::String(String::new())),
        (1, Value::String("empty".into()))
    );
    assert_eq!(
        invoke(Text::checked::KEY, Value::String("ok".into())),
        (0, Value::String("ok".into()))
    );
    let (status, error) = invoke(Text::kernel_error::KEY, Value::String(String::new()));
    assert_eq!(status, 2);
    assert_eq!(Failure::from_value(error).unwrap().code, Code::Denied);
    assert_eq!(
        invoke(Text::constant::KEY, <() as live::Portable>::to_value(&())),
        (0, Value::U64(42))
    );
    assert_eq!(
        unsafe { table.stop.unwrap()(instance).consume_state() }.unwrap(),
        (0, Value::Null)
    );
}
