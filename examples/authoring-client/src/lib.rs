use example_authoring_api::{Count, Counter, Notes, Observed, Relay};
use phenix_sdk::{live, *};

#[plugin(
    id = "test.authored-client",
    define(Relay),
    define(Count),
    provide(Relay, slot = "relay"),
    require(Counter, slot = "counter"),
    require(Counter, slot = "fallback", optional),
    require(Counter, slot = "all", many(min = 0)),
    require(Count, slot = "count"),
    require(Observed, slot = "observed"),
    require(Notes, slot = "notes")
)]
mod client {
    use super::*;
    fn property_error(error: CallError<live::PropertyError>) -> CallError<String> {
        match error {
            CallError::Kernel(failure) => CallError::Kernel(failure),
            CallError::Domain(error) => CallError::Domain(format!("{error:?}")),
        }
    }
    #[provide(Relay::run)]
    fn run(input: String, host: &live::Host<'_>) -> CallResult<u64, String> {
        match input.as_str() {
            "locate-field" => {
                let location = Notes::get::locate(host, "notes")?;
                return Ok(host
                    .invoke::<Notes::get>("notes", &location)
                    .map_err(property_error)?
                    .len() as u64);
            }
            "foreign-publish" => {
                let reference = Observed::get::locate(host, "observed")?;
                let before = reference.read(host).map_err(|error| {
                    Failure::new(Code::PluginFailed, format!("foreign read: {error}"))
                })?;
                if !matches!(
                    host.publish(reference, before.revision, &999_u64),
                    Err(live::AccessError::State(ValueAccessError::Denied))
                        | Err(live::AccessError::Kernel(Failure {
                            code: Code::Denied,
                            ..
                        }))
                ) {
                    return Err(Failure::new(Code::PluginFailed, "foreign publication was not denied").into());
                }
                let after = reference.read(host).map_err(|error| {
                    Failure::new(Code::PluginFailed, format!("foreign read after denial: {error}"))
                })?;
                return Ok(*after.value);
            }
            "locate-count" => {
                let location = Count::get::locate(host, "count")?;
                assert_eq!(
                    location.location(),
                    Count::set::locate(host, "count")?.location()
                );
                assert_eq!(
                    location.location(),
                    Count::watch::locate(host, "count")?.location()
                );
                return host
                    .invoke::<Count::get>("count", &location)
                    .map_err(property_error);
            }
            "lookup-native" => {
                return host
                    .locate_property::<u64>("counter", Counter::run::KEY)
                    .map(|_| 0)
                    .map_err(Into::into);
            }
            "lookup-missing" => {
                return Count::get::locate(host, "absent")
                    .map(|_| 0)
                    .map_err(Into::into);
            }
            "lookup-mismatch" => {
                return host
                    .locate_property::<String>("count", Count::get::KEY)
                    .map(|_| 0)
                    .map_err(Into::into);
            }
            _ => (),
        }
        assert_eq!(imports::fallback::count(host)?, 0);
        assert_eq!(imports::all::count(host)?, 1);
        let first = imports::all::invoke::<Counter::reference>(host, &())?;
        let indexed = imports::all::invoke_at::<Counter::reference>(host, 0, &())?;
        assert_eq!(first.location(), indexed.location());
        assert!(matches!(
            imports::all::invoke_at::<Counter::run>(host, 1, &input),
            Err(CallError::Kernel(Failure {
                code: Code::MissingBinding,
                ..
            }))
        ));
        imports::counter::invoke::<Counter::run>(host, &input)
    }
}
