use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

mod scope;

fn usage() -> &'static str {
    "Usage:\n  cargo run -p xtask -- audit [WORKSPACE]\n  cargo run -p xtask -- verify\n  cargo run -p xtask -- cutover --repo PATH --source WORKSPACE [--expected-head SHA]"
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
        Some("audit") => {
            let workspace = arguments
                .next()
                .map(PathBuf::from)
                .unwrap_or(env::current_dir().map_err(|err| err.to_string())?);
            if arguments.next().is_some() {
                return Err(usage().into());
            }
            let violations = scope::audit(&workspace);
            for problem in &violations {
                eprintln!("scope: {problem}");
            }
            if !violations.is_empty() {
                return Err(format!("{} graph scope violation(s)", violations.len()));
            }
            println!("PASS: one Rust graph engine and no retired source in the workspace");
            Ok(())
        }
        Some("cutover") => {
            let mut repo = None;
            let mut source = None;
            let mut head = scope::PR_HEAD.to_owned();
            while let Some(option) = arguments.next() {
                let value = arguments
                    .next()
                    .ok_or_else(|| format!("missing value for {option}"))?;
                match option.as_str() {
                    "--repo" => repo = Some(PathBuf::from(value)),
                    "--source" => source = Some(PathBuf::from(value)),
                    "--expected-head" => head = value,
                    _ => return Err(format!("unknown argument {option}\n{}", usage())),
                }
            }
            let repo = repo.ok_or_else(|| format!("missing --repo\n{}", usage()))?;
            let source = source.ok_or_else(|| format!("missing --source\n{}", usage()))?;
            scope::cutover(&repo, &source, &head)
        }
        Some("verify") => {
            if arguments.next().is_some() {
                return Err(usage().into());
            }
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
            let problems = scope::audit(&root);
            if !problems.is_empty() {
                return Err(format!("scope audit failed:\n{}", problems.join("\n")));
            }
            for (command, args) in [
                ("cargo", vec!["fmt", "--all", "--", "--check"]),
                ("cargo", vec!["check", "--workspace", "--all-targets"]),
                (
                    "cargo",
                    vec!["test", "--workspace", "--all-targets", "--all-features"],
                ),
                (
                    "cargo",
                    vec!["test", "-p", "phenix-graph", "--no-default-features"],
                ),
            ] {
                println!("Running {command} {}", args.join(" "));
                let result = std::process::Command::new(command)
                    .args(args)
                    .current_dir(&root)
                    .status()
                    .map_err(|error| error.to_string())?;
                if !result.success() {
                    return Err(format!("{command} exited with {result}"));
                }
            }
            Ok(())
        }
        _ => Err(usage().into()),
    }
}
