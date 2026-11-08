//! Authorized versioned batches. Absence has no history token: do not memoize
//! missing entries without revalidation. No snapshot emits invalidations.
use crate::*;

/// Backend batch membership; duplicates must produce duplicate ordered results.
pub struct ReadKey<'a> {
    pub dataset: &'a str,
    pub key: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_preflights_each_mount_after_all_schemas_without_reading_values() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        struct Probe {
            probes: Arc<AtomicUsize>,
            atomic: Arc<AtomicBool>,
        }
        impl Backend for Probe {
            fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
                self.probes.fetch_add(1, Ordering::SeqCst);
                Some(self)
            }
        }
        impl TypedKv for Probe {
            fn get(&mut self, _: &str, _: &str) -> Result<Option<Vec<u8>>> {
                panic!("preflight must not read values")
            }
            fn put(&mut self, _: &str, _: &str, _: Vec<u8>) -> Result<()> {
                panic!("preflight must not write values")
            }
            fn delete(&mut self, _: &str, _: &str) -> Result<bool> {
                panic!("preflight must not delete values")
            }
            fn supports_atomic_read_many(&self) -> bool {
                self.atomic.load(Ordering::SeqCst)
            }
            fn get_revisioned(&mut self, _: &str, _: &str) -> Result<Option<Revisioned<Vec<u8>>>> {
                panic!("preflight must not read revisioned values")
            }
            fn read_many(&mut self, _: &[ReadKey<'_>]) -> Result<Vec<Option<Revisioned<Vec<u8>>>>> {
                panic!("preflight must not read batches")
            }
        }
        let registry = Registry::new();
        let probes = Arc::new(AtomicUsize::new(0));
        let atomic = Arc::new(AtomicBool::new(false));
        registry
            .mount(
                "main",
                Probe {
                    probes: probes.clone(),
                    atomic: atomic.clone(),
                },
            )
            .unwrap();
        let address = Address::parse("store://main/values").unwrap();
        let schema = Schema {
            name: "bytes".into(),
            revision: 1,
            structure: "bytes".into(),
            codec: "raw".into(),
        };
        registry.register(&address, schema.clone()).unwrap();
        let session = registry.session([Grant::new(
            Scope::Dataset(address.clone()),
            [Operation::Read],
        )]);
        let source = ReadSource {
            session,
            address,
            schema,
            key: "value".into(),
        };
        let mut denied = source.clone();
        denied.session = registry.session([]);
        assert!(matches!(
            SnapshotReader::new(vec![source.clone(), denied], ReadConsistency::BestEffort),
            Err(Error::Denied)
        ));
        let mut wrong = source.clone();
        wrong.schema.revision += 1;
        assert!(matches!(
            SnapshotReader::new(vec![source.clone(), wrong], ReadConsistency::BestEffort),
            Err(Error::SchemaMismatch)
        ));
        assert_eq!(probes.load(Ordering::SeqCst), 0);
        assert!(matches!(
            SnapshotReader::new(vec![source.clone()], ReadConsistency::AtomicWithinMount),
            Err(Error::UnsupportedCapability {
                capability: Capability::AtomicReadMany
            })
        ));
        probes.store(0, Ordering::SeqCst);
        SnapshotReader::new(
            vec![source.clone(), source.clone()],
            ReadConsistency::BestEffort,
        )
        .unwrap();
        assert_eq!(probes.load(Ordering::SeqCst), 1);
        atomic.store(true, Ordering::SeqCst);
        let reader = SnapshotReader::new(vec![source], ReadConsistency::AtomicWithinMount).unwrap();
        atomic.store(false, Ordering::SeqCst);
        assert!(matches!(
            reader.read(),
            Err(Error::UnsupportedCapability {
                capability: Capability::AtomicReadMany
            })
        ));
        SnapshotReader::new(vec![], ReadConsistency::AtomicWithinMount)
            .unwrap()
            .read()
            .unwrap();
    }

    #[test]
    fn construction_rejects_sql_only_sources_in_both_consistency_modes() {
        struct SqlOnly(Connection);
        impl Backend for SqlOnly {
            fn sql(&mut self) -> Option<&mut dyn SqlBackend> {
                Some(self)
            }
        }
        impl SqlBackend for SqlOnly {
            fn connection(&mut self) -> &mut Connection {
                &mut self.0
            }
        }
        let registry = Registry::new();
        registry
            .mount("main", SqlOnly(Connection::open_in_memory().unwrap()))
            .unwrap();
        let address = Address::parse("store://main/values").unwrap();
        let schema = Schema {
            name: "bytes".into(),
            revision: 1,
            structure: "bytes".into(),
            codec: "raw".into(),
        };
        registry.register(&address, schema.clone()).unwrap();
        let session = registry.session([Grant::new(
            Scope::Dataset(address.clone()),
            [Operation::Read],
        )]);
        let source = ReadSource {
            session,
            address,
            schema,
            key: "value".into(),
        };
        for consistency in [
            ReadConsistency::AtomicWithinMount,
            ReadConsistency::BestEffort,
        ] {
            assert!(matches!(
                SnapshotReader::new(vec![source.clone()], consistency),
                Err(Error::UnsupportedCapability {
                    capability: Capability::TypedKv
                })
            ));
        }
    }

    #[test]
    fn sqlite_commit_between_batch_selects_keeps_original_snapshot() {
        let path = std::env::temp_dir().join(format!(
            "phenix-snapshot-interleave-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut connection = Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .unwrap();
        connection.execute_batch("CREATE TABLE __phenix_kv(dataset TEXT, key TEXT, value BLOB, revision INTEGER); INSERT INTO __phenix_kv VALUES ('values', 'a', x'00', 1), ('values', 'b', x'00', 2)").unwrap();
        let (start, started) = std::sync::mpsc::channel();
        let (finish, finished) = std::sync::mpsc::channel();
        let writer_path = path.clone();
        let writer = std::thread::spawn(move || {
            started
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            let mut writer = Connection::open(writer_path).unwrap();
            let tx = writer.transaction().unwrap();
            tx.execute_batch("UPDATE __phenix_kv SET value = x'01', revision = revision + 2")
                .unwrap();
            tx.commit().unwrap();
            finish.send(()).unwrap();
        });
        let mut calls = 0;
        // Stop after several SELECTs have established the read snapshot, then
        // require an independent WAL writer to commit before continuing.
        connection
            .progress_handler(
                1,
                Some(move || {
                    calls += 1;
                    if calls == 100 {
                        start.send(()).unwrap();
                        finished
                            .recv_timeout(std::time::Duration::from_secs(5))
                            .unwrap();
                    }
                    false
                }),
            )
            .unwrap();
        let mut backend = SqliteBackend(connection);
        let keys: Vec<_> = (0..100)
            .flat_map(|_| {
                [
                    ReadKey {
                        dataset: "values",
                        key: "a",
                    },
                    ReadKey {
                        dataset: "values",
                        key: "b",
                    },
                ]
            })
            .collect();
        let values = backend.read_many(&keys).unwrap();
        assert!(values.iter().all(|v| v.as_ref().unwrap().value == [0]));
        writer.join().unwrap();
        backend.0.progress_handler(0, None::<fn() -> bool>).unwrap();
        assert!(
            backend
                .read_many(&keys)
                .unwrap()
                .iter()
                .all(|v| v.as_ref().unwrap().value == [1])
        );
        drop(backend);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn memory_batch_holds_mount_lock_across_internal_atomic_mutations() {
        let registry = Registry::new();
        registry.mount_memory("main").unwrap();
        let address = Address::parse("store://main/values").unwrap();
        let schema = Schema {
            name: "bytes".into(),
            revision: 1,
            structure: "bytes".into(),
            codec: "raw".into(),
        };
        registry.register(&address, schema.clone()).unwrap();
        let session = registry.session([Grant::new(
            Scope::Dataset(address.clone()),
            [Operation::Read],
        )]);
        let mount = session.mount(&address).unwrap();
        {
            let mut guard = mount.lock().unwrap();
            let kv = guard.backend.typed_kv().unwrap();
            for key in ["a", "b"] {
                kv.put("values", key, vec![0]).unwrap();
            }
        }
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let start = barrier.clone();
        let writer = std::thread::spawn(move || {
            start.wait();
            for value in 1..=200 {
                let mut guard = mount.lock().unwrap();
                let kv = guard.backend.typed_kv().unwrap();
                kv.put("values", "a", vec![value]).unwrap();
                std::thread::yield_now();
                kv.put("values", "b", vec![value]).unwrap();
            }
        });
        barrier.wait();
        for _ in 0..200 {
            let snapshot = session
                .read_many(
                    &[
                        ReadRequest {
                            address: &address,
                            key: "a",
                            schema: &schema,
                        },
                        ReadRequest {
                            address: &address,
                            key: "b",
                            schema: &schema,
                        },
                    ],
                    ReadConsistency::AtomicWithinMount,
                )
                .unwrap();
            assert_eq!(
                snapshot.entries()[0].value().unwrap().value,
                snapshot.entries()[1].value().unwrap().value
            );
        }
        writer.join().unwrap();
    }
}

pub struct ReadRequest<'a> {
    pub address: &'a Address,
    pub key: &'a str,
    pub schema: &'a Schema,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadConsistency {
    AtomicWithinMount,
    BestEffort,
}

/// Erased read authority, retaining the original immutable session grants.
/// Does not expose the registry or allow issuing or combining grants.
#[derive(Clone)]
pub struct ReadSource {
    session: Session,
    address: Address,
    schema: Schema,
    key: String,
}
impl ReadSource {
    pub fn address(&self) -> &Address {
        &self.address
    }
    pub fn key(&self) -> &str {
        &self.key
    }
    pub fn schema(&self) -> &Schema {
        &self.schema
    }
    pub fn changed(&self, capacity: usize) -> Result<Subscription> {
        self.session
            .subscribe(Scope::Dataset(self.address.clone()), capacity)
    }
    pub(crate) fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.session.inner, &other.session.inner)
            && self.address == other.address
            && self.key == other.key
            && self.schema == other.schema
    }
}

#[derive(Clone)]
pub struct SnapshotEntry {
    source: ReadSource,
    value: Option<Revisioned<Vec<u8>>>,
}
impl SnapshotEntry {
    pub fn address(&self) -> &Address {
        self.source.address()
    }
    pub fn key(&self) -> &str {
        self.source.key()
    }
    pub fn value(&self) -> Option<&Revisioned<Vec<u8>>> {
        self.value.as_ref()
    }
    pub(crate) fn source(&self) -> &ReadSource {
        &self.source
    }
}
pub struct Snapshot {
    entries: Vec<SnapshotEntry>,
    consistency: ReadConsistency,
}
impl Snapshot {
    pub fn entries(&self) -> &[SnapshotEntry] {
        &self.entries
    }
    pub fn consistency(&self) -> ReadConsistency {
        self.consistency
    }
}

impl Session {
    pub fn read_many(
        &self,
        requests: &[ReadRequest<'_>],
        consistency: ReadConsistency,
    ) -> Result<Snapshot> {
        SnapshotReader::new(
            requests
                .iter()
                .map(|r| ReadSource {
                    session: self.clone(),
                    address: r.address.clone(),
                    key: r.key.into(),
                    schema: r.schema.clone(),
                })
                .collect(),
            consistency,
        )?
        .read()
    }
    pub(crate) fn read_source(
        &self,
        address: Address,
        key: String,
        schema: Schema,
    ) -> Result<ReadSource> {
        let source = ReadSource {
            session: self.clone(),
            address,
            key,
            schema,
        };
        SnapshotReader::new(vec![source.clone()], ReadConsistency::BestEffort)?;
        Ok(source)
    }
}

/// Empty groups succeed without lookup or IO. Duplicates are retained in order.
/// Construction validates all grants/schemas before probing mount capabilities,
/// without reading values. Read repeats these checks. BestEffort permits mixed registries.
pub struct SnapshotReader {
    sources: Vec<ReadSource>,
    consistency: ReadConsistency,
}
impl SnapshotReader {
    pub fn new(sources: Vec<ReadSource>, consistency: ReadConsistency) -> Result<Self> {
        let reader = Self {
            sources,
            consistency,
        };
        reader.validate()?;
        Ok(reader)
    }
    fn validate(&self) -> Result<()> {
        // All authority checks precede even the first registry/dataset lookup.
        for source in &self.sources {
            source.session.authorize(&source.address, Operation::Read)?;
        }
        if self.consistency == ReadConsistency::AtomicWithinMount
            && let Some(first) = self.sources.first()
            && self.sources.iter().any(|s| {
                !Arc::ptr_eq(&s.session.inner, &first.session.inner)
                    || s.address.mount() != first.address.mount()
            })
        {
            return Err(Error::MixedSnapshotSources);
        }
        for source in &self.sources {
            let mount = source.session.mount(&source.address)?;
            let _active = Active::enter(&mount)?;
            let mount = mount.lock().map_err(|_| Error::Poisoned)?;
            if mount.datasets[source.address.dataset()] != source.schema {
                return Err(Error::SchemaMismatch);
            }
        }
        for group in self.groups() {
            let first = &self.sources[group[0]];
            let mount = first.session.mount(&first.address)?;
            let _active = Active::enter(&mount)?;
            let mut mount = mount.lock().map_err(|_| Error::Poisoned)?;
            let kv = mount
                .backend
                .typed_kv()
                .ok_or(Error::UnsupportedCapability {
                    capability: Capability::TypedKv,
                })?;
            if self.consistency == ReadConsistency::AtomicWithinMount
                && !kv.supports_atomic_read_many()
            {
                return Err(Error::UnsupportedCapability {
                    capability: Capability::AtomicReadMany,
                });
            }
        }
        Ok(())
    }
    fn groups(&self) -> Vec<Vec<usize>> {
        // Group by physical mount without ever combining sessions' grants.
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for (index, source) in self.sources.iter().enumerate() {
            if let Some(group) = groups.iter_mut().find(|g| {
                Arc::ptr_eq(&self.sources[g[0]].session.inner, &source.session.inner)
                    && self.sources[g[0]].address.mount() == source.address.mount()
            }) {
                group.push(index);
            } else {
                groups.push(vec![index]);
            }
        }
        groups
    }
    pub fn read(&self) -> Result<Snapshot> {
        self.validate()?;
        let mut entries: Vec<_> = self
            .sources
            .iter()
            .map(|source| SnapshotEntry {
                source: source.clone(),
                value: None,
            })
            .collect();
        for group in self.groups() {
            let first = &self.sources[group[0]];
            let mount = first.session.mount(&first.address)?;
            let _active = Active::enter(&mount)?;
            let mut mount = mount.lock().map_err(|_| Error::Poisoned)?;
            let kv = mount
                .backend
                .typed_kv()
                .ok_or(Error::UnsupportedCapability {
                    capability: Capability::TypedKv,
                })?;
            let keys: Vec<_> = group
                .iter()
                .map(|&index| ReadKey {
                    dataset: self.sources[index].address.dataset(),
                    key: &self.sources[index].key,
                })
                .collect();
            let batch = if kv.supports_atomic_read_many() {
                kv.read_many(&keys)?
            } else if self.consistency == ReadConsistency::BestEffort {
                keys.iter()
                    .map(|k| kv.get_revisioned(k.dataset, k.key))
                    .collect::<Result<Vec<_>>>()?
            } else {
                return Err(Error::UnsupportedCapability {
                    capability: Capability::AtomicReadMany,
                });
            };
            if batch.len() != group.len() {
                return Err(Error::SnapshotEntryMismatch);
            }
            for (index, value) in group.into_iter().zip(batch) {
                entries[index].value = value;
            }
        }
        Ok(Snapshot {
            entries,
            consistency: self.consistency,
        })
    }
}
