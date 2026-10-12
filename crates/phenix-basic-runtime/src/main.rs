//! Terminal and JSONL Basic product host. Plugins perform domain work;
//! only this frontend executable reads stdin and submits graph actions.
use phenix_basic_contracts::{
    BasicError, ClientInput, EventPage, JournalQuery, RenderRequest, Submission,
};
use phenix_contract::{Portable, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, Selection},
    Kernel, Outcome,
};
use std::{
    env,
    io::{self, BufRead, Write},
    path::Path,
};

fn invoke<T: Portable>(
    kernel: &Kernel,
    provider: &str,
    operation: &str,
    request: impl Portable,
) -> Result<Result<T, BasicError>, String> {
    let (interface, name) = operation
        .split_once('/')
        .ok_or_else(|| format!("invalid operation {operation}"))?;
    let selection = Selection {
        interface: interface.into(),
        operation: name.into(),
        provider: provider.into(),
    };
    let root = kernel.admit_root(selection).map_err(|e| format!("{e:?}"))?;
    match root
        .invoke(&request.to_value())
        .map_err(|e| format!("{e:?}"))?
    {
        Outcome::Success(value) => Ok(Ok(T::from_value(value).map_err(|e| format!("{e:?}"))?)),
        Outcome::Domain(value) => Ok(Err(
            BasicError::from_value(value).map_err(|e| format!("{e:?}"))?
        )),
    }
}

fn completed<T>(result: Result<Result<T, BasicError>, String>) -> Result<T, String> {
    result?.map_err(|e| format!("{:?}: {}", e.code, e.message))
}

fn render_new(
    kernel: &Kernel,
    frontend: &str,
    session: &str,
    after: &mut u64,
    mut sink: impl Write,
) -> Result<(), String> {
    loop {
        let page: EventPage = completed(invoke(
            kernel,
            "phenix.basic.agent",
            "phenix.basic.output@1/read",
            JournalQuery {
                session: session.into(),
                after: *after,
                limit: 128,
            },
        ))?;
        if page.events.is_empty() {
            break;
        }
        if page.next <= *after {
            return Err("journal cursor did not advance".into());
        }
        let lines: Vec<String> = completed(invoke(
            kernel,
            frontend,
            "phenix.basic.frontend@1/render",
            RenderRequest {
                session: session.into(),
                events: page.events,
            },
        ))?;
        for line in lines {
            writeln!(sink, "{line}").map_err(|e| e.to_string())?;
        }
        *after = page.next;
    }
    sink.flush().map_err(|e| e.to_string())
}

fn submit(kernel: &Kernel, input: Submission) -> Result<(), String> {
    if input.cancel {
        // The synchronous B5 slice has no pending root to cancel. F1 only
        // produces the typed packet; G4's host pump owns active cancellation.
        return Err("no active pending graph execution to cancel".into());
    }
    if input.action != "phenix.basic.submit" {
        return Err("frontend selected an unknown Basic action".into());
    }
    let value = Value::decode(&input.payload.0).map_err(|e| format!("{e:?}"))?;
    let steps = kernel
        .submit_action(&input.action, &value)
        .map_err(|e| format!("{e:?}"))?;
    if steps.is_empty() {
        return Err("prompt action did not activate a graph node".into());
    }
    for step in steps {
        if let Outcome::Domain(value) = step.outcome {
            let cause = BasicError::from_value(value).map_err(|e| format!("{e:?}"))?;
            return Err(format!("{:?}: {}", cause.code, cause.message));
        }
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 || !["terminal", "jsonl"].contains(&args[1].as_str()) {
        return Err("usage: phenix-basic-runtime terminal|jsonl COMPOSITION.json".into());
    }
    let path = Path::new(&args[2]);
    let config = std::fs::read(path).map_err(|e| e.to_string())?;
    let composition: Composition = serde_json::from_slice(&config).map_err(|e| e.to_string())?;
    let kernel = Prepared::prepare(composition)
        .map_err(|e| format!("{e:?}"))?
        .load()
        .map_err(|e| format!("{e:?}"))?;
    let frontend = match args[1].as_str() {
        "terminal" => "phenix.basic.frontend-terminal",
        "jsonl" => "phenix.basic.frontend-jsonl",
        _ => unreachable!(),
    };
    // The first product fixture binds one authenticated session. Later
    // frontends will ask the controller to construct a scope per session.
    let session = "s1";
    let mut after = 0_u64;
    let mut stdout = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let decoded = completed(invoke::<Submission>(
            &kernel,
            frontend,
            "phenix.basic.frontend@1/decode",
            ClientInput {
                session: session.into(),
                line,
            },
        ))?;
        // A domain failure is returned to the caller rather than forging
        // a successful answer. Persisted failure events can still be replayed.
        let result = submit(&kernel, decoded);
        render_new(&kernel, frontend, session, &mut after, &mut stdout)?;
        result?;
    }
    kernel.shutdown().map_err(|e| format!("{e:?}"))
}
fn main() {
    if let Err(err) = run() {
        eprintln!("phenix-basic: {err}");
        std::process::exit(1);
    }
}
