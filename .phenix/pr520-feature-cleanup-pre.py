from pathlib import Path

path = Path("rust/crates/phenix-sdk-macros/src/plugin_attr_core.rs")
text = path.read_text()
old = 'assert_eq!(features, ["Transactions", "Migrations"]);'
new = 'assert_eq!(features, ["Migrations"]);'
if text.count(old) != 1:
    raise SystemExit(f"expected one stale feature expectation, found {text.count(old)}")
path.write_text(text.replace(old, new, 1))
