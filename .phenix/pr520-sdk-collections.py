from pathlib import Path


def replace_once(path, old, new):
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one match, found {count}: {old[:120]!r}")
    file.write_text(text.replace(old, new, 1))


durable = r'''use phenix_core::{
    DurableKeyRange, DurableRecord, KernelAccess, KernelError, PluginId, ResourceNamespace,
    ScanDirection, TransactionOp,
};
use serde::{de::DeserializeOwned, Serialize};
use std::{marker::PhantomData, ops::Bound};

const KEY_SEPARATOR: char = '/';
const METADATA_SEPARATOR: char = '@';

#[derive(Debug, thiserror::Error)]
pub enum DurableCollectionError {
    #[error("kernel durable storage failed: {0}")]
    Kernel(KernelError),
    #[error("durable collection conflict for {plugin} at {namespace}/{key}")]
    Conflict {
        plugin: PluginId,
        namespace: ResourceNamespace,
        key: String,
    },
    #[error("durable key codec failed: {message}")]
    KeyCodec { message: String },
    #[error("failed to encode durable value: {0}")]
    EncodeValue(String),
    #[error("failed to decode durable value: {0}")]
    DecodeValue(String),
    #[error("malformed durable record key: {0}")]
    MalformedRecordKey(String),
    #[error("durable log sequence overflow")]
    SequenceOverflow,
    #[error("malformed durable log tail: {0}")]
    MalformedTail(String),
    #[error("malformed durable log entry key: {0}")]
    MalformedEntryKey(String),
}

impl From<KernelError> for DurableCollectionError {
    fn from(error: KernelError) -> Self {
        match error {
            KernelError::PersistenceConflict {
                plugin,
                namespace,
                key,
            } => Self::Conflict {
                plugin,
                namespace,
                key,
            },
            error => Self::Kernel(error),
        }
    }
}

/// Stable storage-key codec for one durable map key type.
///
/// Encoded keys must be deterministic and may not contain `/` or `@`, which are
/// reserved by the collection layout. Storage ordering is ordering of the
/// encoded keys, not automatically the domain ordering of `K`.
pub trait DurableKeyCodec<K> {
    fn encode_key(&self, key: &K) -> Result<String, DurableCollectionError>;
    fn decode_key(&self, encoded: &str) -> Result<K, DurableCollectionError>;

    fn encode_prefix(&self, prefix: &K) -> Result<String, DurableCollectionError> {
        self.encode_key(prefix)
    }
}

/// Deterministic UTF-8 string codec using lowercase hexadecimal bytes.
///
/// The representation contains only ASCII hex digits, so collection separators
/// are escaped automatically. Lexical encoded order matches UTF-8 byte order.
#[derive(Clone, Copy, Debug, Default)]
pub struct Utf8HexKeyCodec;

impl DurableKeyCodec<String> for Utf8HexKeyCodec {
    fn encode_key(&self, key: &String) -> Result<String, DurableCollectionError> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = String::with_capacity(key.len() * 2);
        for byte in key.as_bytes() {
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
        Ok(encoded)
    }

    fn decode_key(&self, encoded: &str) -> Result<String, DurableCollectionError> {
        if encoded.len() % 2 != 0 {
            return Err(DurableCollectionError::KeyCodec {
                message: format!("hex key has odd length: {}", encoded.len()),
            });
        }
        let bytes = encoded.as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len() / 2);
        for pair in bytes.chunks_exact(2) {
            let high = decode_hex(pair[0])?;
            let low = decode_hex(pair[1])?;
            decoded.push((high << 4) | low);
        }
        String::from_utf8(decoded).map_err(|error| DurableCollectionError::KeyCodec {
            message: format!("encoded key is not valid UTF-8: {error}"),
        })
    }
}

fn decode_hex(byte: u8) -> Result<u8, DurableCollectionError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(DurableCollectionError::KeyCodec {
            message: format!("invalid hex digit: {}", char::from(byte)),
        }),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableMapRange<K> {
    lower: Bound<K>,
    upper: Bound<K>,
}

impl<K> DurableMapRange<K> {
    #[must_use]
    pub fn new(lower: Bound<K>, upper: Bound<K>) -> Self {
        Self { lower, upper }
    }

    #[must_use]
    pub fn all() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn lower(&self) -> &Bound<K> {
        &self.lower
    }

    #[must_use]
    pub fn upper(&self) -> &Bound<K> {
        &self.upper
    }
}

impl<K> Default for DurableMapRange<K> {
    fn default() -> Self {
        Self {
            lower: Bound::Unbounded,
            upper: Bound::Unbounded,
        }
    }
}

pub struct DurableMap<K, V, C = Utf8HexKeyCodec> {
    namespace: ResourceNamespace,
    collection: String,
    codec: C,
    marker: PhantomData<fn() -> (K, V)>,
}

impl<V> DurableMap<String, V, Utf8HexKeyCodec> {
    #[must_use]
    pub fn new(namespace: ResourceNamespace, collection: impl Into<String>) -> Self {
        Self::with_codec(namespace, collection, Utf8HexKeyCodec)
    }
}

impl<K, V, C> DurableMap<K, V, C> {
    #[must_use]
    pub fn with_codec(
        namespace: ResourceNamespace,
        collection: impl Into<String>,
        codec: C,
    ) -> Self {
        Self {
            namespace,
            collection: collection.into(),
            codec,
            marker: PhantomData,
        }
    }

    #[must_use]
    pub fn namespace(&self) -> &ResourceNamespace {
        &self.namespace
    }

    #[must_use]
    pub fn collection(&self) -> &str {
        &self.collection
    }
}

impl<K, V, C> DurableMap<K, V, C>
where
    C: DurableKeyCodec<K>,
    V: DeserializeOwned + Serialize,
{
    pub fn get(
        &self,
        kernel: &KernelAccess<'_, '_>,
        key: &K,
    ) -> Result<Option<V>, DurableCollectionError> {
        self.get_with(kernel, key)
    }

    pub fn put(
        &self,
        kernel: &KernelAccess<'_, '_>,
        key: &K,
        value: &V,
    ) -> Result<(), DurableCollectionError> {
        self.put_with(kernel, key, value)
    }

    pub fn delete(
        &self,
        kernel: &KernelAccess<'_, '_>,
        key: &K,
    ) -> Result<(), DurableCollectionError> {
        self.delete_with(kernel, key)
    }

    pub fn scan(
        &self,
        kernel: &KernelAccess<'_, '_>,
        range: &DurableMapRange<K>,
        direction: ScanDirection,
        limit: Option<usize>,
    ) -> Result<Vec<(K, V)>, DurableCollectionError> {
        self.scan_with(kernel, range, direction, limit)
    }

    pub fn scan_prefix(
        &self,
        kernel: &KernelAccess<'_, '_>,
        prefix: &K,
        direction: ScanDirection,
        limit: Option<usize>,
    ) -> Result<Vec<(K, V)>, DurableCollectionError> {
        self.scan_prefix_with(kernel, prefix, direction, limit)
    }

    fn get_with<A: DurableAccess>(
        &self,
        access: &A,
        key: &K,
    ) -> Result<Option<V>, DurableCollectionError> {
        let key = self.record_key(key)?;
        access
            .read(&self.namespace, &key)?
            .map(|value| decode_value(&value))
            .transpose()
    }

    fn put_with<A: DurableAccess>(
        &self,
        access: &A,
        key: &K,
        value: &V,
    ) -> Result<(), DurableCollectionError> {
        let key = self.record_key(key)?;
        let value = encode_value(value)?;
        access
            .transact(&self.namespace, &[TransactionOp::Put { key, value }])
            .map_err(Into::into)
    }

    fn delete_with<A: DurableAccess>(
        &self,
        access: &A,
        key: &K,
    ) -> Result<(), DurableCollectionError> {
        let key = self.record_key(key)?;
        access
            .transact(&self.namespace, &[TransactionOp::Delete { key }])
            .map_err(Into::into)
    }

    fn scan_with<A: DurableAccess>(
        &self,
        access: &A,
        range: &DurableMapRange<K>,
        direction: ScanDirection,
        limit: Option<usize>,
    ) -> Result<Vec<(K, V)>, DurableCollectionError> {
        let range = self.record_range(range)?;
        let records = access.scan(&self.namespace, &range, direction, limit)?;
        self.decode_records(records)
    }

    fn scan_prefix_with<A: DurableAccess>(
        &self,
        access: &A,
        prefix: &K,
        direction: ScanDirection,
        limit: Option<usize>,
    ) -> Result<Vec<(K, V)>, DurableCollectionError> {
        let encoded = self.codec.encode_prefix(prefix)?;
        validate_key_component(&encoded)?;
        let range = DurableKeyRange::prefix(format!("{}{}", self.entry_prefix(), encoded));
        let records = access.scan(&self.namespace, &range, direction, limit)?;
        self.decode_records(records)
    }

    fn decode_records(
        &self,
        records: Vec<DurableRecord>,
    ) -> Result<Vec<(K, V)>, DurableCollectionError> {
        let prefix = self.entry_prefix();
        records
            .into_iter()
            .map(|record| {
                let encoded = record.key.strip_prefix(&prefix).ok_or_else(|| {
                    DurableCollectionError::MalformedRecordKey(record.key.clone())
                })?;
                let key = self.codec.decode_key(encoded)?;
                let value = decode_value(&record.value)?;
                Ok((key, value))
            })
            .collect()
    }

    fn record_key(&self, key: &K) -> Result<String, DurableCollectionError> {
        let encoded = self.codec.encode_key(key)?;
        validate_key_component(&encoded)?;
        Ok(format!("{}{}", self.entry_prefix(), encoded))
    }

    fn record_range(
        &self,
        range: &DurableMapRange<K>,
    ) -> Result<DurableKeyRange, DurableCollectionError> {
        let collection = DurableKeyRange::prefix(self.entry_prefix());
        let lower = match range.lower() {
            Bound::Included(key) => Bound::Included(self.record_key(key)?),
            Bound::Excluded(key) => Bound::Excluded(self.record_key(key)?),
            Bound::Unbounded => collection.lower().clone(),
        };
        let upper = match range.upper() {
            Bound::Included(key) => Bound::Included(self.record_key(key)?),
            Bound::Excluded(key) => Bound::Excluded(self.record_key(key)?),
            Bound::Unbounded => collection.upper().clone(),
        };
        Ok(DurableKeyRange::new(lower, upper))
    }

    fn entry_prefix(&self) -> String {
        format!("{}/map/", self.collection)
    }
}

fn validate_key_component(encoded: &str) -> Result<(), DurableCollectionError> {
    if encoded.contains(KEY_SEPARATOR) || encoded.contains(METADATA_SEPARATOR) {
        return Err(DurableCollectionError::KeyCodec {
            message: "encoded key contains reserved collection separator".into(),
        });
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableLogEntry<T> {
    pub sequence: u64,
    pub value: T,
}

pub struct DurableLog<T> {
    namespace: ResourceNamespace,
    collection: String,
    marker: PhantomData<fn() -> T>,
}

impl<T> DurableLog<T> {
    #[must_use]
    pub fn new(namespace: ResourceNamespace, collection: impl Into<String>) -> Self {
        Self {
            namespace,
            collection: collection.into(),
            marker: PhantomData,
        }
    }

    #[must_use]
    pub fn namespace(&self) -> &ResourceNamespace {
        &self.namespace
    }

    #[must_use]
    pub fn collection(&self) -> &str {
        &self.collection
    }
}

impl<T> DurableLog<T>
where
    T: DeserializeOwned + Serialize,
{
    pub fn append(
        &self,
        kernel: &KernelAccess<'_, '_>,
        value: &T,
    ) -> Result<u64, DurableCollectionError> {
        self.append_with(kernel, value)
    }

    pub fn entries(
        &self,
        kernel: &KernelAccess<'_, '_>,
        direction: ScanDirection,
        limit: Option<usize>,
    ) -> Result<Vec<DurableLogEntry<T>>, DurableCollectionError> {
        self.entries_with(kernel, direction, limit)
    }

    pub fn latest(
        &self,
        kernel: &KernelAccess<'_, '_>,
    ) -> Result<Option<DurableLogEntry<T>>, DurableCollectionError> {
        self.latest_with(kernel)
    }

    fn append_with<A: DurableAccess>(
        &self,
        access: &A,
        value: &T,
    ) -> Result<u64, DurableCollectionError> {
        let tail_key = self.tail_key();
        let expected = access.read(&self.namespace, &tail_key)?;
        let current = expected.as_deref().map(decode_tail).transpose()?;
        let next = match current {
            Some(sequence) => sequence
                .checked_add(1)
                .ok_or(DurableCollectionError::SequenceOverflow)?,
            None => 0,
        };
        let entry_key = self.entry_key(next);
        let value = encode_value(value)?;
        let tail = encode_value(&next)?;

        access
            .transact(
                &self.namespace,
                &[
                    TransactionOp::AssertValue {
                        key: tail_key.clone(),
                        expected,
                    },
                    TransactionOp::Put {
                        key: entry_key,
                        value,
                    },
                    TransactionOp::Put {
                        key: tail_key,
                        value: tail,
                    },
                ],
            )
            .map_err(DurableCollectionError::from)?;
        Ok(next)
    }

    fn entries_with<A: DurableAccess>(
        &self,
        access: &A,
        direction: ScanDirection,
        limit: Option<usize>,
    ) -> Result<Vec<DurableLogEntry<T>>, DurableCollectionError> {
        let prefix = self.entry_prefix();
        let records = access.scan(
            &self.namespace,
            &DurableKeyRange::prefix(prefix.clone()),
            direction,
            limit,
        )?;
        records
            .into_iter()
            .map(|record| {
                let encoded = record.key.strip_prefix(&prefix).ok_or_else(|| {
                    DurableCollectionError::MalformedEntryKey(record.key.clone())
                })?;
                let sequence = decode_sequence(encoded)?;
                let value = decode_value(&record.value)?;
                Ok(DurableLogEntry { sequence, value })
            })
            .collect()
    }

    fn latest_with<A: DurableAccess>(
        &self,
        access: &A,
    ) -> Result<Option<DurableLogEntry<T>>, DurableCollectionError> {
        Ok(self
            .entries_with(access, ScanDirection::Reverse, Some(1))?
            .into_iter()
            .next())
    }

    fn tail_key(&self) -> String {
        format!("{}/log/@tail", self.collection)
    }

    fn entry_prefix(&self) -> String {
        format!("{}/log/entry/", self.collection)
    }

    fn entry_key(&self, sequence: u64) -> String {
        format!("{}{:020}", self.entry_prefix(), sequence)
    }
}

fn decode_tail(value: &[u8]) -> Result<u64, DurableCollectionError> {
    serde_json::from_slice(value)
        .map_err(|error| DurableCollectionError::MalformedTail(error.to_string()))
}

fn decode_sequence(value: &str) -> Result<u64, DurableCollectionError> {
    if value.len() != 20 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(DurableCollectionError::MalformedEntryKey(value.to_owned()));
    }
    value
        .parse()
        .map_err(|error| DurableCollectionError::MalformedEntryKey(format!("{value}: {error}")))
}

fn encode_value<T: Serialize>(value: &T) -> Result<Vec<u8>, DurableCollectionError> {
    serde_json::to_vec(value).map_err(|error| DurableCollectionError::EncodeValue(error.to_string()))
}

fn decode_value<T: DeserializeOwned>(value: &[u8]) -> Result<T, DurableCollectionError> {
    serde_json::from_slice(value)
        .map_err(|error| DurableCollectionError::DecodeValue(error.to_string()))
}

trait DurableAccess {
    fn read(
        &self,
        namespace: &ResourceNamespace,
        key: &str,
    ) -> Result<Option<Vec<u8>>, KernelError>;

    fn scan(
        &self,
        namespace: &ResourceNamespace,
        range: &DurableKeyRange,
        direction: ScanDirection,
        limit: Option<usize>,
    ) -> Result<Vec<DurableRecord>, KernelError>;

    fn transact(
        &self,
        namespace: &ResourceNamespace,
        operations: &[TransactionOp],
    ) -> Result<(), KernelError>;
}

impl DurableAccess for KernelAccess<'_, '_> {
    fn read(
        &self,
        namespace: &ResourceNamespace,
        key: &str,
    ) -> Result<Option<Vec<u8>>, KernelError> {
        self.read_durable(namespace, key)
    }

    fn scan(
        &self,
        namespace: &ResourceNamespace,
        range: &DurableKeyRange,
        direction: ScanDirection,
        limit: Option<usize>,
    ) -> Result<Vec<DurableRecord>, KernelError> {
        self.scan_durable(namespace, range, direction, limit)
    }

    fn transact(
        &self,
        namespace: &ResourceNamespace,
        operations: &[TransactionOp],
    ) -> Result<(), KernelError> {
        self.transact_durable(namespace, operations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
    };

    #[derive(Default)]
    struct MemoryAccess {
        records: RefCell<BTreeMap<(ResourceNamespace, String), Vec<u8>>>,
        conflict_next_assertion: Cell<bool>,
        transactions: Cell<usize>,
    }

    impl MemoryAccess {
        fn put_raw(&self, namespace: &ResourceNamespace, key: impl Into<String>, value: Vec<u8>) {
            self.records
                .borrow_mut()
                .insert((namespace.clone(), key.into()), value);
        }

        fn raw(&self, namespace: &ResourceNamespace, key: &str) -> Option<Vec<u8>> {
            self.records
                .borrow()
                .get(&(namespace.clone(), key.to_owned()))
                .cloned()
        }
    }

    impl DurableAccess for MemoryAccess {
        fn read(
            &self,
            namespace: &ResourceNamespace,
            key: &str,
        ) -> Result<Option<Vec<u8>>, KernelError> {
            Ok(self.raw(namespace, key))
        }

        fn scan(
            &self,
            namespace: &ResourceNamespace,
            range: &DurableKeyRange,
            direction: ScanDirection,
            limit: Option<usize>,
        ) -> Result<Vec<DurableRecord>, KernelError> {
            let records = self.records.borrow();
            let mut matches = records
                .iter()
                .filter(|((record_namespace, key), _)| {
                    record_namespace == namespace && in_range(key, range)
                })
                .map(|((_, key), value)| DurableRecord {
                    key: key.clone(),
                    value: value.clone(),
                })
                .collect::<Vec<_>>();
            matches.sort_by(|left, right| left.key.cmp(&right.key));
            if direction == ScanDirection::Reverse {
                matches.reverse();
            }
            if let Some(limit) = limit {
                matches.truncate(limit);
            }
            Ok(matches)
        }

        fn transact(
            &self,
            namespace: &ResourceNamespace,
            operations: &[TransactionOp],
        ) -> Result<(), KernelError> {
            self.transactions.set(self.transactions.get() + 1);
            let assertion_key = operations.iter().find_map(|operation| match operation {
                TransactionOp::AssertValue { key, .. } => Some(key.clone()),
                _ => None,
            });
            if self.conflict_next_assertion.replace(false) {
                if let Some(key) = assertion_key {
                    return Err(conflict_error(namespace, key));
                }
            }

            {
                let records = self.records.borrow();
                for operation in operations {
                    if let TransactionOp::AssertValue { key, expected } = operation {
                        let actual = records.get(&(namespace.clone(), key.clone())).cloned();
                        if &actual != expected {
                            return Err(conflict_error(namespace, key.clone()));
                        }
                    }
                }
            }

            let mut records = self.records.borrow_mut();
            for operation in operations {
                match operation {
                    TransactionOp::Put { key, value } => {
                        records.insert((namespace.clone(), key.clone()), value.clone());
                    }
                    TransactionOp::Delete { key } => {
                        records.remove(&(namespace.clone(), key.clone()));
                    }
                    TransactionOp::AssertValue { .. } => {}
                }
            }
            Ok(())
        }
    }

    fn namespace() -> ResourceNamespace {
        ResourceNamespace::parse("fixture.collections").unwrap()
    }

    fn conflict_error(namespace: &ResourceNamespace, key: String) -> KernelError {
        KernelError::PersistenceConflict {
            plugin: PluginId::parse("fixture.collections").unwrap(),
            namespace: namespace.clone(),
            key,
        }
    }

    fn in_range(key: &str, range: &DurableKeyRange) -> bool {
        let lower = match range.lower() {
            Bound::Included(value) => key >= value.as_str(),
            Bound::Excluded(value) => key > value.as_str(),
            Bound::Unbounded => true,
        };
        let upper = match range.upper() {
            Bound::Included(value) => key <= value.as_str(),
            Bound::Excluded(value) => key < value.as_str(),
            Bound::Unbounded => true,
        };
        lower && upper
    }

    #[test]
    fn utf8_hex_key_codec_is_stable_and_escapes_layout_separators() {
        let codec = Utf8HexKeyCodec;
        let key = "a/@\0é".to_owned();
        let encoded = codec.encode_key(&key).unwrap();

        assert_eq!(encoded, "612f4000c3a9");
        assert!(!encoded.contains(KEY_SEPARATOR));
        assert!(!encoded.contains(METADATA_SEPARATOR));
        assert_eq!(codec.decode_key(&encoded).unwrap(), key);
    }

    #[test]
    fn durable_map_crud_bounded_scan_and_prefix_scan_are_collection_local() {
        let namespace = namespace();
        let access = MemoryAccess::default();
        let map = DurableMap::<String, String>::new(namespace.clone(), "users");
        let other = DurableMap::<String, String>::new(namespace.clone(), "other");

        for (key, value) in [
            ("b", "B"),
            ("a/@", "escaped"),
            ("alpha", "A"),
            ("alphabet", "AB"),
        ] {
            map.put_with(&access, &key.to_owned(), &value.to_owned())
                .unwrap();
        }
        other
            .put_with(&access, &"alpha".to_owned(), &"other".to_owned())
            .unwrap();

        assert_eq!(
            map.get_with(&access, &"a/@".to_owned()).unwrap(),
            Some("escaped".to_owned())
        );

        let all = map
            .scan_with(
                &access,
                &DurableMapRange::all(),
                ScanDirection::Forward,
                None,
            )
            .unwrap();
        assert_eq!(
            all.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>(),
            vec!["a/@", "alpha", "alphabet", "b"]
        );

        let bounded = map
            .scan_with(
                &access,
                &DurableMapRange::new(
                    Bound::Excluded("alpha".to_owned()),
                    Bound::Included("b".to_owned()),
                ),
                ScanDirection::Forward,
                None,
            )
            .unwrap();
        assert_eq!(
            bounded
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
            vec!["alphabet", "b"]
        );

        let prefixed = map
            .scan_prefix_with(
                &access,
                &"alpha".to_owned(),
                ScanDirection::Reverse,
                None,
            )
            .unwrap();
        assert_eq!(
            prefixed
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
            vec!["alphabet", "alpha"]
        );

        map.delete_with(&access, &"b".to_owned()).unwrap();
        assert_eq!(map.get_with(&access, &"b".to_owned()).unwrap(), None);
        assert_eq!(
            other.get_with(&access, &"alpha".to_owned()).unwrap(),
            Some("other".to_owned())
        );
    }

    #[test]
    fn durable_map_rejects_malformed_keys_and_values() {
        let namespace = namespace();
        let access = MemoryAccess::default();
        let map = DurableMap::<String, String>::new(namespace.clone(), "broken");

        access.put_raw(
            &namespace,
            format!("{}zz", map.entry_prefix()),
            serde_json::to_vec("value").unwrap(),
        );
        assert!(matches!(
            map.scan_with(
                &access,
                &DurableMapRange::all(),
                ScanDirection::Forward,
                None
            ),
            Err(DurableCollectionError::KeyCodec { .. })
        ));

        let clean = MemoryAccess::default();
        let key = map.record_key(&"valid".to_owned()).unwrap();
        clean.put_raw(&namespace, key, b"{".to_vec());
        assert!(matches!(
            map.get_with(&clean, &"valid".to_owned()),
            Err(DurableCollectionError::DecodeValue(_))
        ));
    }

    #[test]
    fn durable_log_uses_fixed_width_ordered_entries_and_reverse_latest_read() {
        let namespace = namespace();
        let access = MemoryAccess::default();
        let log = DurableLog::<String>::new(namespace.clone(), "events");

        assert_eq!(log.append_with(&access, &"first".to_owned()).unwrap(), 0);
        assert_eq!(log.append_with(&access, &"second".to_owned()).unwrap(), 1);
        assert_eq!(log.append_with(&access, &"third".to_owned()).unwrap(), 2);

        let entries = log
            .entries_with(&access, ScanDirection::Forward, None)
            .unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.sequence, entry.value.as_str()))
                .collect::<Vec<_>>(),
            vec![(0, "first"), (1, "second"), (2, "third")]
        );
        assert_eq!(
            log.latest_with(&access).unwrap(),
            Some(DurableLogEntry {
                sequence: 2,
                value: "third".to_owned(),
            })
        );

        assert!(access
            .raw(&namespace, &format!("{}{:020}", log.entry_prefix(), 0))
            .is_some());
        assert_eq!(
            access.raw(&namespace, &log.tail_key()).unwrap(),
            serde_json::to_vec(&2_u64).unwrap()
        );

        let reopened = DurableLog::<String>::new(namespace, "events");
        assert_eq!(
            reopened.latest_with(&access).unwrap(),
            Some(DurableLogEntry {
                sequence: 2,
                value: "third".to_owned(),
            })
        );
    }

    #[test]
    fn durable_log_surfaces_conflict_without_hidden_retry() {
        let namespace = namespace();
        let access = MemoryAccess::default();
        let log = DurableLog::<String>::new(namespace, "events");

        log.append_with(&access, &"first".to_owned()).unwrap();
        let before = access.transactions.get();
        access.conflict_next_assertion.set(true);

        assert!(matches!(
            log.append_with(&access, &"second".to_owned()),
            Err(DurableCollectionError::Conflict { .. })
        ));
        assert_eq!(access.transactions.get(), before + 1);
        assert_eq!(
            log.entries_with(&access, ScanDirection::Forward, None)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn durable_log_rejects_overflow_and_malformed_layout() {
        let namespace = namespace();
        let overflow = MemoryAccess::default();
        let log = DurableLog::<String>::new(namespace.clone(), "overflow");
        overflow.put_raw(
            &namespace,
            log.tail_key(),
            serde_json::to_vec(&u64::MAX).unwrap(),
        );
        assert!(matches!(
            log.append_with(&overflow, &"next".to_owned()),
            Err(DurableCollectionError::SequenceOverflow)
        ));

        let malformed_tail = MemoryAccess::default();
        malformed_tail.put_raw(&namespace, log.tail_key(), b"not-json".to_vec());
        assert!(matches!(
            log.append_with(&malformed_tail, &"next".to_owned()),
            Err(DurableCollectionError::MalformedTail(_))
        ));

        let malformed_entry = MemoryAccess::default();
        malformed_entry.put_raw(
            &namespace,
            format!("{}not-a-sequence", log.entry_prefix()),
            serde_json::to_vec("value").unwrap(),
        );
        assert!(matches!(
            log.entries_with(&malformed_entry, ScanDirection::Forward, None),
            Err(DurableCollectionError::MalformedEntryKey(_))
        ));

        let malformed_value = MemoryAccess::default();
        malformed_value.put_raw(
            &namespace,
            format!("{}{:020}", log.entry_prefix(), 0),
            b"{".to_vec(),
        );
        assert!(matches!(
            log.entries_with(&malformed_value, ScanDirection::Forward, None),
            Err(DurableCollectionError::DecodeValue(_))
        ));
    }
}
'''
Path("rust/crates/phenix-sdk/src/durable.rs").write_text(durable)

lib = "rust/crates/phenix-sdk/src/lib.rs"
replace_once(
    lib,
    "mod authoring;\npub mod contracts;",
    "mod authoring;\nmod durable;\npub mod contracts;",
)
replace_once(
    lib,
    "pub use contracts::*;\npub use phenix_core::{",
    "pub use contracts::*;\npub use durable::*;\npub use phenix_core::{",
)
replace_once(
    lib,
    "Contract, ContractId, ContractValue, DurableSchema, Exact, HasPhenixSchema, Key, LayerResult,",
    "Contract, ContractId, ContractValue, DurableKeyRange, DurableRecord, DurableSchema, Exact,\n    HasPhenixSchema, KernelError, Key, LayerResult,",
)
replace_once(
    lib,
    "PluginId, Project, ReferenceId, RuntimeId, Type, TypeKind, ValueError,",
    "PluginId, Project, ReferenceId, ResourceNamespace, RuntimeId, ScanDirection, TransactionOp, Type,\n    TypeKind, ValueError,",
)

runtime_tests = "rust/crates/phenix-core/src/runtime/tests.rs"
replace_once(
    runtime_tests,
    '''            b"write" => {
                host.transact_durable(
                    &self.namespace,
                    &[TransactionOp::Put {
                        key: "changed".into(),
                        value: b"yes".to_vec(),
                    }],
                )
                .map_err(|error| error.to_string())?;
                Ok(b"written".to_vec())
            }
            _ => Err("unsupported input".into()),''',
    '''            b"write" => {
                host.transact_durable(
                    &self.namespace,
                    &[TransactionOp::Put {
                        key: "changed".into(),
                        value: b"yes".to_vec(),
                    }],
                )
                .map_err(|error| error.to_string())?;
                Ok(b"written".to_vec())
            }
            b"scan" => host
                .scan_durable(
                    &self.namespace,
                    &DurableKeyRange::all(),
                    ScanDirection::Forward,
                    None,
                )
                .map(|records| {
                    records
                        .into_iter()
                        .map(|record| record.key)
                        .collect::<Vec<_>>()
                        .join(",")
                        .into_bytes()
                })
                .map_err(|error| error.to_string()),
            _ => Err("unsupported input".into()),''',
)
replace_once(
    runtime_tests,
    '''    let error = kernel
        .invoke(
            &service("storage@1"),
            b"write",
            &Authority::new([read, write]),
            None,
        )
        .unwrap();''',
    '''    assert_eq!(
        kernel
            .invoke(
                &service("storage@1"),
                b"scan",
                &Authority::new([read.clone()]),
                None,
            )
            .unwrap(),
        b"seed"
    );
    let denied_scan = kernel
        .invoke(
            &service("storage@1"),
            b"scan",
            &Authority::default(),
            None,
        )
        .unwrap_err();
    assert!(denied_scan.to_string().contains(PERSISTENCE_READ));

    let error = kernel
        .invoke(
            &service("storage@1"),
            b"write",
            &Authority::new([read, write]),
            None,
        )
        .unwrap();''',
)
replace_once(
    runtime_tests,
    '''        maximum_authority: Authority::new([capability(PERSISTENCE_SCHEMA)]),
    };
    let kernel = Kernel::new(KernelConfig::new([owner]).unwrap());
    let authority = Authority::new([capability(PERSISTENCE_SCHEMA)]);''',
    '''        maximum_authority: Authority::new([
            capability(PERSISTENCE_SCHEMA),
            capability(PERSISTENCE_READ),
        ]),
    };
    let kernel = Kernel::new(KernelConfig::new([owner]).unwrap());
    let authority = Authority::new([
        capability(PERSISTENCE_SCHEMA),
        capability(PERSISTENCE_READ),
    ]);''',
)
replace_once(
    runtime_tests,
    '''    assert!(matches!(
        host.register_durable_schema(&DurableSchema::new(other_namespace, 1)),
        Err(KernelError::HostOperationDenied { .. })
    ));
}''',
    '''    assert!(matches!(
        host.register_durable_schema(&DurableSchema::new(other_namespace.clone(), 1)),
        Err(KernelError::HostOperationDenied { .. })
    ));
    assert!(matches!(
        host.scan_durable(
            &other_namespace,
            &DurableKeyRange::all(),
            ScanDirection::Forward,
            None,
        ),
        Err(KernelError::HostOperationDenied { .. })
    ));
}''',
)
