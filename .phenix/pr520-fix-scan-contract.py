from pathlib import Path
import re


def replace_exact(path, old, new, expected=1):
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != expected:
        raise SystemExit(f"{path}: expected {expected} matches, found {count}: {old!r}")
    file.write_text(text.replace(old, new))


persistence = "rust/crates/phenix-core/src/persistence.rs"
replace_exact(
    persistence,
    'assert_eq!(max.upper(), &Bound::Excluded("itemt".into()));',
    'assert_eq!(max.upper(), &Bound::Excluded("items0".into()));',
)
replace_exact(
    persistence,
    "        limit: usize,",
    "        limit: Option<usize>,",
    expected=2,
)
replace_exact(
    persistence,
    "        if limit == 0 {\n            return Ok(Vec::new());\n        }",
    "        if matches!(limit, Some(0)) {\n            return Ok(Vec::new());\n        }",
)
replace_exact(
    persistence,
    '''        let limit_index = values.len() + 1;
        sql.push_str(&format!(" LIMIT ?{limit_index}"));
        values.push(Value::Integer(i64::try_from(limit).unwrap_or(i64::MAX)));
''',
    '''        if let Some(limit) = limit {
            let limit_index = values.len() + 1;
            sql.push_str(&format!(" LIMIT ?{limit_index}"));
            values.push(Value::Integer(i64::try_from(limit).unwrap_or(i64::MAX)));
        }
''',
)

replace_exact(
    "rust/crates/phenix-core/src/runtime/host.rs",
    "        limit: usize,",
    "        limit: Option<usize>,",
)
replace_exact(
    "rust/crates/phenix-core/src/plugin_context.rs",
    "        limit: usize,",
    "        limit: Option<usize>,",
)
replace_exact(
    "rust/crates/phenix-core/tests/persistence_backend_conformance.rs",
    "        limit: usize,",
    "        limit: Option<usize>,",
)
replace_exact(
    "rust/crates/phenix-core/tests/persistence_backend_conformance.rs",
    "        records.truncate(limit);",
    "        if let Some(limit) = limit {\n            records.truncate(limit);\n        }",
)
replace_exact(
    "rust/crates/phenix-core/src/runtime/persistence_bootstrap.rs",
    "            _limit: usize,",
    "            _limit: Option<usize>,",
)
replace_exact(
    "rust/crates/phenix-plugin-session-tree/src/session_tree_atomicity_regression.rs",
    "        limit: usize,",
    "        limit: Option<usize>,",
)
replace_exact(
    "rust/crates/phenix-core/src/persistence_provider/tests.rs",
    "        _limit: usize,",
    "        _limit: Option<usize>,",
)

conformance = Path("rust/crates/phenix-core/tests/persistence_backend_conformance.rs")
text = conformance.read_text()
text, count = re.subn(
    r"(ScanDirection::(?:Forward|Reverse),\n\s+)(\d+)(,)",
    lambda match: f"{match.group(1)}Some({match.group(2)}){match.group(3)}",
    text,
)
if count != 6:
    raise SystemExit(f"persistence conformance: expected 6 scan limits, found {count}")
needle = '''    assert_eq!(
        bounded.iter().map(|record| record.key.as_str()).collect::<Vec<_>>(),
        vec!["alpha", "beta", "beta-2"]
    );
'''
insert = needle + '''
    let unbounded = backend
        .scan(
            &first_owner,
            &first_namespace,
            &DurableKeyRange::all(),
            ScanDirection::Forward,
            None,
        )
        .unwrap();
    assert_eq!(
        unbounded
            .iter()
            .map(|record| record.key.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "beta", "beta-2", "gamma", "record"]
    );
'''
if text.count(needle) != 1:
    raise SystemExit("persistence conformance: bounded assertion anchor changed")
conformance.write_text(text.replace(needle, insert, 1))
