from pathlib import Path

path = Path("rust/crates/phenix-core/Cargo.toml")
text = path.read_text()
old = '''[[test]]
name = "observable_allocations"
path = "tests/observable_allocations.rs"
'''
new = old + '''
[[test]]
name = "persistence_backend_conformance"
path = "tests/persistence_backend_conformance.rs"
'''
if text.count(old) != 1:
    raise SystemExit("phenix-core/Cargo.toml: unexpected explicit test topology")
path.write_text(text.replace(old, new, 1))
