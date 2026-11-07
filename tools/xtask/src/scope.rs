//! Repository hygiene and one-way cutover tooling. No graph execution or
//! production graph semantics live here. Tests are ordinary Rust tests.
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PR_HEAD: &str = "d8b26b43614c5f6b4b7b53cfd7d4480998e7c6e2";

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn children(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = fs::read_dir(path).map_err(|err| format!("{}: {err}", path.display()))?
        .map(|entry| entry.map(|e| e.path()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    Ok(paths)
}

fn rust_files(path: &Path, result: &mut Vec<PathBuf>) -> Result<(), String> {
    if !path.exists() { return Ok(()); }
    for child in children(path)? {
        if child.is_symlink() { continue; }
        if child.is_dir() {
            if child.file_name().is_some_and(|name| name == "target") { continue; }
            rust_files(&child, result)?;
        } else if child.extension().is_some_and(|ext| ext == "rs") {
            result.push(child);
        }
    }
    Ok(())
}

/// Scan only this workspace's structural boundaries, not source-code style.
/// Rust/Cargo compilation and behavioral testing are separate acceptance gates.
pub fn audit(root: &Path) -> Vec<String> {
    let mut errors = Vec::new();
    let manifest = root.join("Cargo.toml");
    let contents = match read(&manifest) {
        Ok(s) => s,
        Err(e) => return vec![e],
    };
    let expected: BTreeSet<&str> = [
        "crates/phenix-graph",
        "crates/phenix-graph/port-args-derive",
        "tools/xtask",
    ].into_iter().collect();
    // The repository uses exactly one plain workspace members array. We
    // intentionally do not implement a partial TOML interpreter here.
    let actual: BTreeSet<&str> = contents.split("members").nth(1)
        .and_then(|body| body.split_once('['))
        .and_then(|(_, body)| body.split_once(']'))
        .map(|(members, _)| members.split('"').enumerate()
            .filter_map(|(index, element)| (index % 2 == 1).then_some(element))
            .collect()).unwrap_or_default();
    if actual != expected {
        errors.push(format!("workspace members mismatch: {actual:?}, expected {expected:?}"));
    }
    let allowed = [
        "Cargo.toml", "Cargo.lock", "README.md", "AGENTS.md", ".gitignore",
        "LICENSE", "LICENSE.md", "LICENSE-MIT", "LICENSE-APACHE", "NOTICE",
        ".github", ".git", "crates", "docs", "tools", "target",
    ];
    match children(root) {
        Ok(paths) => for path in paths {
            let name = path.file_name().unwrap().to_string_lossy();
            if !allowed.contains(&name.as_ref()) {
                errors.push(format!("out-of-scope root path: {name}"));
            }
        },
        Err(error) => errors.push(error),
    }
    if let Ok(paths) = children(&root.join("crates")) {
        for path in paths {
            if path.file_name().is_some_and(|name| name != "phenix-graph") {
                errors.push(format!("out-of-scope crate: {}", path.display()));
            }
        }
    } else {
        errors.push("missing crates directory".to_owned());
    }
    if let Ok(paths) = children(&root.join("tools")) {
        for path in paths {
            if path.file_name().is_some_and(|name| name != "xtask") {
                errors.push(format!("out-of-scope tooling: {}", path.display()));
            }
        }
    } else {
        errors.push("missing tools/xtask directory".to_owned());
    }
    let graph = root.join("crates/phenix-graph");
    for required in ["src/lib.rs", "src/authoring/patching.rs", "src/deployment.rs", "src/testing.rs"] {
        if !graph.join(required).is_file() {
            errors.push(format!("missing prototype component {required}"));
        }
    }
    let mut files = Vec::new();
    if let Err(error) = rust_files(&graph, &mut files) { errors.push(error); }
    for path in files {
        let code = match read(&path) { Ok(code) => code, Err(error) => {
            errors.push(error);
            continue;
        }};
        let retired = [
            "NativeGraphRun", "LegacyEdgeAdapter", "EdgeTransport", "with_edge_transport",
            "GraphAuthor", "pub fn connect_path(", "pub fn disconnect_path(",
            "pub fn trigger_paths(", "pub fn expose_input_path(",
            "pub fn expose_output_path(", "pub fn register_splice(",
            "pub fn register<F>", ".fire_trigger(", "Deployment::local(",
            // `Option`/`Result` are data by default. Interpret them through
            // Output<T>, not through another action adapter / node category.
            "Node::optional(", "Node::fallible(",
            "pub struct OptionalNode", "pub struct FallibleNode",
            "pub fn local_optional", "pub fn local_fallible",
            // Shared ownership is part of the Rust argument type, not a node kind.
            "pub struct SharedNode", "pub fn shared<F>(",
            "SharedFunctionNodeAction", "pub fn shared_function<",
            // Output dispatch is solely the returned Output<T> selection.
            // An output-port or patch-level selector would be a second routing API.
            "pub trait OutputSelector", "pub enum OutputRouting",
            "pub fn route<", "pub fn route(",
            "OutputRoutingChanged", "PatchTarget::OutputRouting",
            // The one-node / one-return direction must not reintroduce
            // specialized node categories or duplicated output adapters.
            "pub struct BranchNode", "pub enum Branch<", "pub fn branch(",
            "pub fn branch_named(", "pub fn local_branch<",
            "pub fn function_optional<", "pub fn function_fallible<",
            "pub fn register_optional<", "pub fn register_fallible<",
            "pub fn local_directed<", "LocalDirectedFunctionNodeAction",
            "LocalOwnedFunctionNodeAction", "pub fn for_args<",
        ];
        for marker in retired {
            if code.contains(marker) {
                errors.push(format!("obsolete API {marker} in {}", path.display()));
            }
        }
        if path != graph.join("src/deployment.rs") && code.contains("Engine::new(") {
            errors.push(format!("direct engine construction in {}", path.display()));
        }
    }
    let engine = read(&graph.join("src/lib.rs")).unwrap_or_default();
    if !engine.contains("pub(crate) fn new(graph: Arc<CompiledGraph>") {
        errors.push("Engine::new must be internal; Deployment is the public constructor".into());
    }
    let workflow = root.join(".github/workflows/graph-only.yml");
    if !workflow.is_file() { errors.push("missing graph-only GitHub Actions".into()); }
    if let Ok(paths) = children(&root.join(".github/workflows")) {
        for path in paths {
            if path.file_name().is_some_and(|name| name != "graph-only.yml") {
                errors.push(format!("obsolete CI workflow: {}", path.display()));
            }
        }
    }
    errors
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git").args(args).current_dir(root)
        .output().map_err(|error| format!("git {:?}: {error}", args))?;
    if !output.status.success() {
        return Err(format!("git {:?} failed: {}", args, String::from_utf8_lossy(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn erase(path: &Path) -> Result<(), String> {
    if path.is_dir() && !path.is_symlink() {
        fs::remove_dir_all(path).map_err(|error| format!("{}: {error}", path.display()))
    } else {
        fs::remove_file(path).map_err(|error| format!("{}: {error}", path.display()))
    }
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in children(source)? {
        let name = entry.file_name().unwrap();
        if name == "target" || name == ".git" { continue; }
        let dest = destination.join(name);
        if entry.is_symlink() { return Err(format!("refusing symlink in source: {}", entry.display())); }
        if entry.is_dir() { copy_tree(&entry, &dest)?; }
        else { fs::copy(&entry, &dest).map_err(|error| format!("{}: {error}", entry.display()))?; }
    }
    Ok(())
}

/// Destructive branch-only cutover. Graph scope only: historical source stays
/// available in Git, never as a fallback code path in the working tree.
pub fn cutover(repo: &Path, source: &Path, expected_head: &str) -> Result<(), String> {
    let root = fs::canonicalize(repo).map_err(|error| error.to_string())?;
    let source = fs::canonicalize(source).map_err(|error| error.to_string())?;
    if root == source || source.starts_with(&root) || root.starts_with(&source) {
        return Err("repository and prototype workspace must be separate directories".into());
    }
    for relative in [
        "Cargo.toml", "crates/phenix-graph/Cargo.toml",
        "crates/phenix-kernel/src/graph.rs", "crates/phenix-contract/src/lib.rs",
    ] {
        if !root.join(relative).is_file() { return Err(format!("not the expected source tree: {relative}")); }
    }
    for relative in [
        "Cargo.toml", "crates/phenix-graph/src/lib.rs",
        "tools/xtask/src/scope.rs", ".github/workflows/graph-only.yml",
    ] {
        if !source.join(relative).is_file() { return Err(format!("missing prototype file {relative}")); }
    }
    if !read(&root.join("Cargo.toml"))?.contains("members = [\"crates/*\"]") {
        return Err("not the expected PR #1241 workspace".into());
    }
    if PathBuf::from(git(&root, &["rev-parse", "--show-toplevel"])?).canonicalize()
        .map_err(|error| error.to_string())? != root {
        return Err("cutover must run at the Git repository root".into());
    }
    let branch = git(&root, &["branch", "--show-current"])?;
    if branch.is_empty() || branch == "main" || branch == "master" {
        return Err("refusing default branch or detached HEAD".into());
    }
    if !git(&root, &["status", "--porcelain"])?.is_empty() {
        return Err("refusing dirty worktree".into());
    }
    let head = git(&root, &["rev-parse", "HEAD"])?;
    if head != expected_head { return Err(format!("expected head {expected_head}, found {head}")); }
    // Validate the replacement completely before modifying a single file.
    let problems = audit(&source);
    if !problems.is_empty() { return Err(format!("invalid replacement: {}", problems.join("; "))); }
    for entry in children(&root)? {
        let name = entry.file_name().unwrap();
        if name == ".git" || [".gitignore", "LICENSE", "LICENSE.md", "LICENSE-MIT", "LICENSE-APACHE", "NOTICE"].contains(&name.to_str().unwrap_or("")) {
            continue;
        }
        erase(&entry)?;
    }
    copy_tree(&source, &root)?;
    let doc = root.join("docs/REINTEGRATION.md");
    if doc.is_file() {
        let content = read(&doc)?;
        fs::write(&doc, content.replace(PR_HEAD, &head)).map_err(|e| e.to_string())?;
    }
    let problems = audit(&root);
    if !problems.is_empty() { return Err(format!("post-cutover scope violation: {}", problems.join("; "))); }
    println!("Prototype-only workspace installed; source revision retained in Git: {head}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);

    struct Temporary(PathBuf);
    impl Temporary {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "phenix-xtask-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn join(&self, name: &str) -> PathBuf { self.0.join(name) }
    }
    impl Drop for Temporary {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }
    fn source() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
    }
    fn old_repo(dir: &Path) -> String {
        for (file, content) in [
            ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
            ("crates/phenix-graph/Cargo.toml", "[package]\nname = \"phenix-graph\"\n"),
            ("crates/phenix-graph/src/lib.rs", "pub struct OldGraphSession;"),
            ("crates/phenix-kernel/src/graph.rs", "pub struct NativeGraphRun;"),
            ("crates/phenix-contract/src/lib.rs", "pub mod graph;"),
            ("crates/phenix-artifact/src/lib.rs", "old artifact"),
            ("crates/phenix-basic-runtime/src/main.rs", "old runtime"),
            (".github/workflows/old.yml", "old CI"),
        ] {
            let path = dir.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        git(dir, &["init", "-q"]).unwrap();
        git(dir, &["add", "."]).unwrap();
        git(dir, &["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "baseline"]).unwrap();
        git(dir, &["switch", "-qc", "prototype-clean"]).unwrap();
        git(dir, &["rev-parse", "HEAD"]).unwrap()
    }
    #[test]
    fn workspace_scope_is_a_rust_test() {
        let errors = audit(&source());
        assert!(errors.is_empty(), "{errors:#?}");
    }
    #[test]
    fn cutover_removes_all_old_graph_call_sites_and_verifies_rust_workspace() {
        let fixture = Temporary::new();
        let repo = fixture.join("repo");
        fs::create_dir_all(&repo).unwrap();
        let sha = old_repo(&repo);
        cutover(&repo, &source(), &sha).unwrap();
        assert!(!repo.join("crates/phenix-kernel").exists());
        assert!(!repo.join("crates/phenix-contract").exists());
        assert!(!repo.join("crates/phenix-artifact").exists());
        assert!(!repo.join("crates/phenix-basic-runtime").exists());
        assert!(!repo.join(".github/workflows/old.yml").exists());
        assert!(!repo.join("crates/phenix-graph/src/session.rs").exists());
        assert!(repo.join("tools/xtask/src/main.rs").is_file());
        assert!(repo.join("crates/phenix-graph/src/authoring/patching.rs").is_file());
        assert!(audit(&repo).is_empty());
        assert!(repo.join("docs/REINTEGRATION.md").is_file());
    }
    #[test]
    fn cutover_rejects_wrong_head_and_dirty_worktree() {
        let fixture = Temporary::new();
        let repo = fixture.join("repo");
        fs::create_dir_all(&repo).unwrap();
        let sha = old_repo(&repo);
        assert!(cutover(&repo, &source(), "wrong-sha").unwrap_err().contains("expected head"));
        fs::write(repo.join("Cargo.toml"), "modified").unwrap();
        assert!(cutover(&repo, &source(), &sha).unwrap_err().contains("dirty worktree"));
    }
    #[test]
    fn scope_audit_rejects_retired_crates_and_legacy_engine_apis() {
        let fixture = Temporary::new();
        let repo = fixture.join("repo");
        copy_tree(&source(), &repo).unwrap();
        let old = repo.join("crates/phenix-kernel/src/lib.rs");
        fs::create_dir_all(old.parent().unwrap()).unwrap();
        fs::write(&old, "old kernel").unwrap();
        assert!(audit(&repo).iter().any(|e| e.contains("out-of-scope crate")));
        fs::remove_dir_all(repo.join("crates/phenix-kernel")).unwrap();
        let engine = repo.join("crates/phenix-graph/src/lib.rs");
        fs::write(&engine, format!("{}\npub struct NativeGraphRun;\n", read(&engine).unwrap())).unwrap();
        assert!(audit(&repo).iter().any(|e| e.contains("obsolete API")));
    }
    #[test]
    fn cutover_refuses_the_default_branch_and_detached_head() {
        let fixture = Temporary::new();
        let repo = fixture.join("repo");
        fs::create_dir_all(&repo).unwrap();
        let head = old_repo(&repo);
        git(&repo, &["switch", "-qc", "main"]).unwrap();
        assert!(cutover(&repo, &source(), &head).unwrap_err().contains("default branch"));
        git(&repo, &["checkout", "--detach", "-q", "HEAD"]).unwrap();
        assert!(cutover(&repo, &source(), &head).unwrap_err().contains("detached HEAD"));
        assert!(repo.join("crates/phenix-kernel/src/graph.rs").exists());
    }

    #[test]
    fn audit_rejects_reintroduced_specialized_optional_or_fallible_nodes() {
        let fixture = Temporary::new();
        let repo = fixture.join("repo");
        copy_tree(&source(), &repo).unwrap();
        let file = repo.join("crates/phenix-graph/src/authoring.rs");
        let original = read(&file).unwrap();
        fs::write(&file, format!("{original}\npub struct OptionalNode<T>(T);\n")).unwrap();
        assert!(audit(&repo).iter().any(|e| e.contains("OptionalNode")));
        fs::write(&file, format!("{original}\npub struct FallibleNode<T>(T);\n")).unwrap();
        assert!(audit(&repo).iter().any(|e| e.contains("FallibleNode")));
    }

    #[test]
    fn audit_detects_parallel_public_authoring_paths() {
        let fixture = Temporary::new();
        let repo = fixture.join("repo");
        copy_tree(&source(), &repo).unwrap();
        let file = repo.join("crates/phenix-graph/src/authoring.rs");
        let original = read(&file).unwrap();
        fs::write(&file, format!("{original}\npub struct GraphAuthor;\n")).unwrap();
        assert!(audit(&repo).iter().any(|error| error.contains("GraphAuthor")));
    }
    #[test]
    fn audit_rejects_parallel_output_policy_registration() {
        let fixture = Temporary::new();
        let repo = fixture.join("repo");
        copy_tree(&source(), &repo).unwrap();
        let file = repo.join("crates/phenix-graph/src/authoring.rs");
        let original = read(&file).unwrap();
        fs::write(&file, format!("{original}\npub fn route<T>(_: T) {{}}\n")).unwrap();
        assert!(audit(&repo).iter().any(|e| e.contains("obsolete API pub fn route<")));
    }

}
