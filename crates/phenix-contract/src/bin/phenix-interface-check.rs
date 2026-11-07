//! Offline graph-interface check using the existing canonical Value/StaticSchema.
use phenix_contract::{
    interface::{InterfaceSurface, PortDirection},
    StaticField, StaticSchema,
};
use std::{error::Error, fs};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn read(path: &str) -> Result<InterfaceSurface> {
    Ok(InterfaceSurface::decode(&fs::read(path)?)?)
}

fn self_test() -> Result<()> {
    const REQUEST: StaticSchema = StaticSchema::Struct(&[
        StaticField {
            name: "prompt",
            schema: StaticSchema::String,
        },
        StaticField {
            name: "tokens",
            schema: StaticSchema::U64,
        },
    ]);
    let good = InterfaceSurface::new("model.generate@1")?
        .port("request", PortDirection::Input, REQUEST)?
        .output::<String>("reply")?;
    let independently_declared = InterfaceSurface::new("model.generate@1")?
        .port("request", PortDirection::Input, REQUEST)?
        .output::<String>("reply")?;
    good.check_implementation(&InterfaceSurface::decode(
        &independently_declared.encode()?,
    )?)?;
    let wrong_type = InterfaceSurface::new("model.generate@1")?
        .input::<u64>("request")?
        .output::<String>("reply")?;
    assert!(good.check_implementation(&wrong_type).is_err());
    let wrong_id = InterfaceSurface::new("other.generate@1")?
        .port("request", PortDirection::Input, REQUEST)?
        .output::<String>("reply")?;
    assert!(good.check_implementation(&wrong_id).is_err());
    let missing_output = InterfaceSurface::new("model.generate@1")?.port(
        "request",
        PortDirection::Input,
        REQUEST,
    )?;
    assert!(good.check_implementation(&missing_output).is_err());
    Ok(())
}

fn check() -> Result<()> {
    let mut consumer = None;
    let mut provider = None;
    let mut contract = None;
    let mut args = std::env::args().skip(1);
    while let Some(option) = args.next() {
        if option == "--self-test" {
            if args.next().is_some() {
                return Err("--self-test takes no additional arguments".into());
            }
            return self_test();
        }
        let value = args.next().ok_or("missing path after option")?;
        match option.as_str() {
            "--consumer" => consumer = Some(value),
            "--provider" => provider = Some(value),
            "--contract" => contract = Some(value),
            _ => return Err(format!("unknown option {option}").into()),
        }
    }
    let consumer = read(consumer.as_deref().ok_or("missing --consumer")?)?;
    let provider = read(provider.as_deref().ok_or("missing --provider")?)?;
    if let Some(path) = contract {
        let authority = read(&path)?;
        authority.check_implementation(&consumer)?;
        authority.check_implementation(&provider)?;
    } else {
        consumer.check_implementation(&provider)?;
    }
    Ok(())
}

fn main() {
    if let Err(error) = check() {
        eprintln!("phenix-interface-check: {error}");
        std::process::exit(1);
    }
}
