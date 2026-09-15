use phenix_application_interface::{application_descriptor, generate, ApplicationDescriptor};
use std::{
    env,
    error::Error,
    fs,
    io::{self, Write},
};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.as_slice() {
        [] => io::stdout().write_all(application_descriptor().canonical_json()?.as_bytes())?,
        [flag, path] if flag == "--check" => {
            let canonical = application_descriptor().canonical_json()?;
            if fs::read_to_string(path)? != canonical {
                eprintln!("{canonical}");
                return Err(format!("application descriptor is stale: {path}").into());
            }
        }
        [flag, path] if flag == "--rust" => {
            let descriptor: ApplicationDescriptor =
                serde_json::from_str(&fs::read_to_string(path)?)?;
            io::stdout().write_all(generate::rust(&descriptor)?.as_bytes())?;
        }
        _ => {
            return Err(
                "usage: phenix-application-descriptor [--check snapshot | --rust descriptor]"
                    .into(),
            )
        }
    }
    Ok(())
}
