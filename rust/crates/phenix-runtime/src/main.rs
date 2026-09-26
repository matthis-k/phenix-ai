use phenix_runtime::Runtime;
use phenix_core::Authority;
use serde_json::json;
use std::io;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = Runtime::default();
    runtime.activate_all()?;

    if std::env::args().any(|argument| argument == "--list-services") {
        let plugins = runtime
            .kernel()
            .config()
            .manifests()
            .map(|manifest| manifest.id.as_str().to_owned())
            .collect::<Vec<_>>();
        let mut services = runtime
            .kernel()
            .config()
            .manifests()
            .flat_map(|manifest| manifest.services.iter())
            .map(|contribution| contribution.service.as_str().to_owned())
            .collect::<Vec<_>>();
        services.sort();
        services.dedup();
        println!(
            "{}",
            serde_json::to_string(&json!({ "plugins": plugins, "services": services }))?
        );
        return Ok(());
    }

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = io::BufWriter::new(stdout.lock());
    runtime.serve_jsonl(&Authority::default(), stdin.lock(), &mut stdout)?;
    Ok(())
}
