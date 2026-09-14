from pathlib import Path

path = Path("rust/crates/phenix-core/src/persistence_provider/tests.rs")
text = path.read_text()
old_import = "use crate::{DurableSchema, NamespaceTransaction, ResourceNamespace};"
new_import = "use crate::{\n    DurableKeyRange, DurableRecord, DurableSchema, NamespaceTransaction, ResourceNamespace,\n    ScanDirection,\n};"
if text.count(old_import) != 1:
    raise SystemExit("persistence_provider/tests.rs: unexpected imports")
text = text.replace(old_import, new_import, 1)
old = '''    fn transact_many(
        &mut self,
        _transactions: &[NamespaceTransaction],
    ) -> Result<(), PersistenceError> {
        Ok(())
    }
'''
new = '''    fn scan(
        &self,
        _caller: &PluginId,
        _namespace: &ResourceNamespace,
        _range: &DurableKeyRange,
        _direction: ScanDirection,
        _limit: usize,
    ) -> Result<Vec<DurableRecord>, PersistenceError> {
        Ok(Vec::new())
    }

''' + old
if text.count(old) != 1:
    raise SystemExit("persistence_provider/tests.rs: unexpected backend implementation")
path.write_text(text.replace(old, new, 1))
