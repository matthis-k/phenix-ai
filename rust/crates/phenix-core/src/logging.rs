use crate::{ContentReference, ContentReferenceStore, FileContentReferenceStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    env,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

pub const PHENIX_LOG_ENV: &str = "PHENIX_LOG";
pub const PHENIX_LOG_DEPTH_ENV: &str = "PHENIX_LOG_DEPTH";
pub const PHENIX_LOG_STORE_ENV: &str = "PHENIX_LOG_STORE";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LogSink {
    Stderr,
    Stdout,
    Directory(PathBuf),
    AppendFile(PathBuf),
    TruncateFile(PathBuf),
}

impl LogSink {
    #[must_use]
    pub fn directory(path: impl Into<PathBuf>) -> Self {
        Self::Directory(path.into())
    }

    #[must_use]
    pub fn append_file(path: impl Into<PathBuf>) -> Self {
        Self::AppendFile(path.into())
    }

    #[must_use]
    pub fn truncate_file(path: impl Into<PathBuf>) -> Self {
        Self::TruncateFile(path.into())
    }

    pub fn parse(spec: &str) -> Result<Self, String> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err("log sink must not be empty".into());
        }
        match spec {
            "stderr" | "console" => Ok(Self::Stderr),
            "stdout" => Ok(Self::Stdout),
            _ => {
                if let Some(path) = spec
                    .strip_prefix("directory:")
                    .or_else(|| spec.strip_prefix("dir:"))
                {
                    return non_empty_path(path, Self::Directory);
                }
                if let Some(path) = spec.strip_prefix("append:") {
                    return non_empty_path(path, Self::AppendFile);
                }
                if let Some(path) = spec.strip_prefix("file:") {
                    return non_empty_path(path, Self::AppendFile);
                }
                if let Some(path) = spec.strip_prefix("truncate:") {
                    return non_empty_path(path, Self::TruncateFile);
                }
                Ok(Self::AppendFile(PathBuf::from(spec)))
            }
        }
    }

    pub fn from_env() -> Result<Option<Self>, String> {
        let Some(value) = env::var_os(PHENIX_LOG_ENV) else {
            return Ok(None);
        };
        let value = value.to_string_lossy();
        if value.trim().is_empty() {
            return Ok(None);
        }
        Self::parse(&value).map(Some)
    }

    #[must_use]
    pub fn description(&self) -> String {
        match self {
            Self::Stderr => "stderr".into(),
            Self::Stdout => "stdout".into(),
            Self::Directory(path) => format!("dir:{}", path.display()),
            Self::AppendFile(path) => format!("append:{}", path.display()),
            Self::TruncateFile(path) => format!("truncate:{}", path.display()),
        }
    }

    fn inferred_store_root(&self) -> Option<PathBuf> {
        match self {
            Self::Directory(root) => Some(root.join("objects")),
            Self::AppendFile(path) | Self::TruncateFile(path) => {
                let mut root = OsString::from(path.as_os_str());
                root.push(".d");
                Some(PathBuf::from(root).join("objects"))
            }
            Self::Stderr | Self::Stdout => None,
        }
    }
}

fn non_empty_path(
    path: &str,
    constructor: impl FnOnce(PathBuf) -> LogSink,
) -> Result<LogSink, String> {
    if path.is_empty() {
        return Err("log sink requires a non-empty path".into());
    }
    Ok(constructor(PathBuf::from(path)))
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogDetailMode {
    Summary,
    Reference,
    #[default]
    Inline,
}

impl LogDetailMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "summary" | "compact" => Ok(Self::Summary),
            "reference" | "ref" => Ok(Self::Reference),
            "inline" | "full" => Ok(Self::Inline),
            other => Err(format!(
                "unsupported log depth {other:?}; expected summary, reference, or inline"
            )),
        }
    }

    pub fn from_env() -> Result<Option<Self>, String> {
        let Some(value) = env::var_os(PHENIX_LOG_DEPTH_ENV) else {
            return Ok(None);
        };
        let value = value.to_string_lossy();
        if value.trim().is_empty() {
            return Ok(None);
        }
        Self::parse(&value).map(Some)
    }
}

enum LogWriter {
    Stderr,
    Stdout,
    File(File),
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructuredLogPage {
    pub records: Vec<Value>,
    pub next_cursor: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct StructuredLogReader {
    path: PathBuf,
    reference_store: Option<FileContentReferenceStore>,
}

impl StructuredLogReader {
    pub fn configured(sink: LogSink) -> Result<Self, String> {
        let path = match &sink {
            LogSink::Directory(root) => root.join("phenix.log"),
            LogSink::AppendFile(path) | LogSink::TruncateFile(path) => path.clone(),
            LogSink::Stderr | LogSink::Stdout => {
                return Err("configured log sink is not readable as a local file".into())
            }
        };
        let mut reference_store = sink.inferred_store_root().map(FileContentReferenceStore::new);
        if let Some(root) =
            env::var_os(PHENIX_LOG_STORE_ENV).filter(|root| !root.as_os_str().is_empty())
        {
            reference_store = Some(FileContentReferenceStore::new(root));
        }
        Ok(Self {
            path,
            reference_store,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read_page(&self, cursor: u64, limit: usize) -> Result<StructuredLogPage, String> {
        if limit == 0 {
            return Ok(StructuredLogPage {
                records: Vec::new(),
                next_cursor: Some(cursor),
            });
        }
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(StructuredLogPage {
                    records: Vec::new(),
                    next_cursor: None,
                })
            }
            Err(error) => return Err(format!("{}: {error}", self.path.display())),
        };
        let mut records = Vec::new();
        let mut scanned = cursor;
        let mut has_more = false;
        for line in BufReader::new(file).lines().skip(cursor as usize) {
            let line = line.map_err(|error| format!("{}: {error}", self.path.display()))?;
            if records.len() == limit {
                has_more = true;
                break;
            }
            scanned += 1;
            let value = serde_json::from_str::<Value>(&line).map_err(|error| {
                format!(
                    "{} record {} is invalid JSON: {error}",
                    self.path.display(),
                    scanned
                )
            })?;
            records.push(value);
        }
        Ok(StructuredLogPage {
            records,
            next_cursor: has_more.then_some(scanned),
        })
    }

    pub fn read_reference(&self, reference: &ContentReference) -> Result<Option<Vec<u8>>, String> {
        let store = self
            .reference_store
            .as_ref()
            .ok_or_else(|| "configured log sink has no reference store".to_owned())?;
        store.get(reference)
    }
}

pub struct StructuredLogger {
    sink: LogSink,
    writer: Mutex<LogWriter>,
    detail_mode: LogDetailMode,
    reference_store: Option<FileContentReferenceStore>,
}

impl StructuredLogger {
    pub fn new(sink: LogSink) -> Result<Self, String> {
        let writer = match &sink {
            LogSink::Stderr => LogWriter::Stderr,
            LogSink::Stdout => LogWriter::Stdout,
            LogSink::Directory(root) => {
                LogWriter::File(open_file(&root.join("phenix.log"), false)?)
            }
            LogSink::AppendFile(path) => LogWriter::File(open_file(path, false)?),
            LogSink::TruncateFile(path) => LogWriter::File(open_file(path, true)?),
        };
        let reference_store = sink
            .inferred_store_root()
            .map(FileContentReferenceStore::new);
        Ok(Self {
            sink,
            writer: Mutex::new(writer),
            detail_mode: LogDetailMode::Inline,
            reference_store,
        })
    }

    pub fn configured(sink: LogSink) -> Result<Self, String> {
        let mut logger = Self::new(sink)?;
        if let Some(root) =
            env::var_os(PHENIX_LOG_STORE_ENV).filter(|root| !root.as_os_str().is_empty())
        {
            logger.reference_store = Some(FileContentReferenceStore::new(root));
        }
        logger.detail_mode = LogDetailMode::from_env()?.unwrap_or_else(|| {
            if logger.reference_store.is_some() {
                LogDetailMode::Reference
            } else {
                LogDetailMode::Inline
            }
        });
        Ok(logger)
    }

    #[must_use]
    pub fn with_detail_mode(mut self, detail_mode: LogDetailMode) -> Self {
        self.detail_mode = detail_mode;
        self
    }

    #[must_use]
    pub fn with_reference_store(mut self, store: FileContentReferenceStore) -> Self {
        self.reference_store = Some(store);
        self
    }

    #[must_use]
    pub fn sink(&self) -> &LogSink {
        &self.sink
    }

    #[must_use]
    pub fn detail_mode(&self) -> LogDetailMode {
        self.detail_mode
    }

    #[must_use]
    pub fn reference_store(&self) -> Option<&FileContentReferenceStore> {
        self.reference_store.as_ref()
    }

    pub fn record<T: Serialize>(&self, kind: &str, payload: T) -> Result<(), String> {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_millis();
        let record = serde_json::json!({
            "timestamp_ms": timestamp_ms,
            "pid": process::id(),
            "kind": kind,
            "payload": payload,
        });
        self.write_json_line(&record)
    }

    pub fn record_detail<Summary, Detail>(
        &self,
        kind: &str,
        summary: &Summary,
        detail: &Detail,
    ) -> Result<(), String>
    where
        Summary: Serialize,
        Detail: Serialize,
    {
        self.record_detail_with_store(kind, summary, detail, None)
    }

    pub fn record_detail_with_store<Summary, Detail>(
        &self,
        kind: &str,
        summary: &Summary,
        detail: &Detail,
        store: Option<&dyn ContentReferenceStore>,
    ) -> Result<(), String>
    where
        Summary: Serialize,
        Detail: Serialize,
    {
        match self.detail_mode {
            LogDetailMode::Summary => self.record(
                kind,
                serde_json::json!({
                    "summary": summary,
                }),
            ),
            LogDetailMode::Inline => self.record(
                kind,
                serde_json::json!({
                    "summary": summary,
                    "detail": {
                        "kind": "inline",
                        "value": detail,
                    },
                }),
            ),
            LogDetailMode::Reference => {
                let reference = self.store_json_with_optional_store(detail, store)?;
                self.record(
                    kind,
                    serde_json::json!({
                        "summary": summary,
                        "detail": {
                            "kind": "reference",
                            "reference": reference,
                        },
                    }),
                )
            }
        }
    }

    pub fn store_json<T: Serialize>(&self, value: &T) -> Result<ContentReference, String> {
        self.store_json_with_optional_store(value, None)
    }

    pub fn store_json_with<T: Serialize>(
        &self,
        value: &T,
        store: &dyn ContentReferenceStore,
    ) -> Result<ContentReference, String> {
        self.store_json_with_optional_store(value, Some(store))
    }

    fn store_json_with_optional_store<T: Serialize>(
        &self,
        value: &T,
        store: Option<&dyn ContentReferenceStore>,
    ) -> Result<ContentReference, String> {
        let bytes = canonical_json_bytes(value)?;
        let store = store
            .or_else(|| {
                self.reference_store
                    .as_ref()
                    .map(|store| store as &dyn ContentReferenceStore)
            })
            .ok_or_else(|| {
                format!(
                    "reference-depth logging requires {} for console sinks",
                    PHENIX_LOG_STORE_ENV
                )
            })?;
        store.put("application/json", &bytes)
    }

    pub fn write_json_line<T: Serialize>(&self, value: &T) -> Result<(), String> {
        let mut line = serde_json::to_vec(value).map_err(|error| error.to_string())?;
        line.push(b'\n');
        let mut writer = self
            .writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &mut *writer {
            LogWriter::Stderr => {
                let mut stderr = io::stderr().lock();
                stderr.write_all(&line).map_err(|error| error.to_string())?;
                stderr.flush().map_err(|error| error.to_string())
            }
            LogWriter::Stdout => {
                let mut stdout = io::stdout().lock();
                stdout.write_all(&line).map_err(|error| error.to_string())?;
                stdout.flush().map_err(|error| error.to_string())
            }
            LogWriter::File(file) => {
                file.write_all(&line).map_err(|error| error.to_string())?;
                file.flush().map_err(|error| error.to_string())
            }
        }
    }
}

fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let mut value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    canonicalize_json(&mut value);
    serde_json::to_vec(&value).map_err(|error| error.to_string())
}

fn canonicalize_json(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                canonicalize_json(value);
            }
        }
        Value::Object(values) => {
            let mut entries = std::mem::take(values).into_iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            for (key, mut value) in entries {
                canonicalize_json(&mut value);
                values.insert(key, value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn open_file(path: &Path, truncate: bool) -> Result<File, String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let mut options = OpenOptions::new();
    options.create(true);
    if truncate {
        options.write(true).truncate(true);
    } else {
        options.append(true);
    }
    options
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("phenix-core-log-{}-{nonce}-{name}", process::id()))
    }

    #[test]
    fn sink_parser_supports_console_and_file_modes() {
        assert_eq!(LogSink::parse("stderr").unwrap(), LogSink::Stderr);
        assert_eq!(LogSink::parse("console").unwrap(), LogSink::Stderr);
        assert_eq!(LogSink::parse("stdout").unwrap(), LogSink::Stdout);
        assert_eq!(
            LogSink::parse("dir:/tmp/phenix").unwrap(),
            LogSink::Directory(PathBuf::from("/tmp/phenix"))
        );
        assert_eq!(
            LogSink::parse("directory:/tmp/phenix").unwrap(),
            LogSink::Directory(PathBuf::from("/tmp/phenix"))
        );
        assert_eq!(
            LogSink::parse("append:/tmp/phenix.log").unwrap(),
            LogSink::AppendFile(PathBuf::from("/tmp/phenix.log"))
        );
        assert_eq!(
            LogSink::parse("truncate:/tmp/phenix.log").unwrap(),
            LogSink::TruncateFile(PathBuf::from("/tmp/phenix.log"))
        );
        assert_eq!(
            LogSink::parse("/tmp/phenix.log").unwrap(),
            LogSink::AppendFile(PathBuf::from("/tmp/phenix.log"))
        );
    }

    #[test]
    fn log_depth_parser_is_explicit() {
        assert_eq!(
            LogDetailMode::parse("summary").unwrap(),
            LogDetailMode::Summary
        );
        assert_eq!(
            LogDetailMode::parse("reference").unwrap(),
            LogDetailMode::Reference
        );
        assert_eq!(
            LogDetailMode::parse("inline").unwrap(),
            LogDetailMode::Inline
        );
        assert!(LogDetailMode::parse("everything").is_err());
    }

    #[test]
    fn append_file_preserves_existing_records_across_logger_instances() {
        let path = unique_path("append.jsonl");
        fs::write(&path, b"seed\n").unwrap();

        StructuredLogger::new(LogSink::append_file(&path))
            .unwrap()
            .record("first", serde_json::json!({"value": 1}))
            .unwrap();
        StructuredLogger::new(LogSink::append_file(&path))
            .unwrap()
            .record("second", serde_json::json!({"value": 2}))
            .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("seed\n"));
        assert!(content.contains("\"kind\":\"first\""));
        assert!(content.contains("\"kind\":\"second\""));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn truncate_file_replaces_previous_run_then_keeps_appending() {
        let path = unique_path("truncate.jsonl");
        fs::write(&path, b"seed\n").unwrap();

        let logger = StructuredLogger::new(LogSink::truncate_file(&path)).unwrap();
        logger
            .record("first", serde_json::json!({"value": 1}))
            .unwrap();
        logger
            .record("second", serde_json::json!({"value": 2}))
            .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.contains("seed"));
        assert!(content.contains("\"kind\":\"first\""));
        assert!(content.contains("\"kind\":\"second\""));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn directory_sink_uses_phenix_log_as_root_and_shared_object_store() {
        let root = unique_path("directory");
        let logger = StructuredLogger::new(LogSink::directory(&root))
            .unwrap()
            .with_detail_mode(LogDetailMode::Reference);
        let child = logger
            .store_json(&serde_json::json!({"body": "child"}))
            .unwrap();
        logger
            .record_detail(
                "response",
                &serde_json::json!({"status": "ok"}),
                &serde_json::json!({"child": child}),
            )
            .unwrap();

        let main_path = root.join("phenix.log");
        let main = fs::read_to_string(&main_path).unwrap();
        assert!(main.contains("\"kind\":\"reference\""));
        assert_eq!(
            logger.reference_store().unwrap().root(),
            root.join("objects").as_path()
        );

        let record: Value = serde_json::from_str(main.lines().next().unwrap()).unwrap();
        let detail: ContentReference =
            serde_json::from_value(record["payload"]["detail"]["reference"].clone()).unwrap();
        let detail_bytes = logger
            .reference_store()
            .unwrap()
            .get(&detail)
            .unwrap()
            .unwrap();
        let detail_value: Value = serde_json::from_slice(&detail_bytes).unwrap();
        assert_eq!(
            detail_value["child"]["digest"],
            serde_json::to_value(child.digest).unwrap()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reference_depth_keeps_detail_out_of_main_log_and_verifies_cas() {
        let path = unique_path("reference.jsonl");
        let logger = StructuredLogger::new(LogSink::append_file(&path))
            .unwrap()
            .with_detail_mode(LogDetailMode::Reference);
        let marker = "DETAIL-MARKER-ONLY-IN-CAS";
        logger
            .record_detail(
                "response",
                &serde_json::json!({"status": "ok"}),
                &serde_json::json!({"body": marker, "nested": {"value": 7}}),
            )
            .unwrap();

        let main = fs::read_to_string(&path).unwrap();
        assert!(main.contains("\"kind\":\"reference\""));
        assert!(!main.contains(marker));

        let record: Value = serde_json::from_str(main.lines().next().unwrap()).unwrap();
        let reference: ContentReference =
            serde_json::from_value(record["payload"]["detail"]["reference"].clone()).unwrap();
        let stored = logger
            .reference_store()
            .unwrap()
            .get(&reference)
            .unwrap()
            .unwrap();
        let stored: Value = serde_json::from_slice(&stored).unwrap();
        assert_eq!(stored["body"], marker);
        assert_eq!(
            reference.digest,
            crate::ArtifactRevision::from_content(&canonical_json_bytes(&stored).unwrap())
        );
    }

    #[test]
    fn content_references_can_form_deduplicated_dags() {
        let root = unique_path("objects");
        let store = FileContentReferenceStore::new(&root);
        let logger = StructuredLogger::new(LogSink::Stderr)
            .unwrap()
            .with_reference_store(store.clone());

        let child = logger
            .store_json(&serde_json::json!({"response": "large-body"}))
            .unwrap();
        let child_again = logger
            .store_json(&serde_json::json!({"response": "large-body"}))
            .unwrap();
        assert_eq!(child, child_again);

        let parent = logger
            .store_json(&serde_json::json!({
                "received": true,
                "body": child,
            }))
            .unwrap();
        let parent_bytes = store.get(&parent).unwrap().unwrap();
        let parent_value: Value = serde_json::from_slice(&parent_bytes).unwrap();
        assert_eq!(
            parent_value["body"]["digest"],
            serde_json::to_value(child_again.digest).unwrap()
        );
    }

    #[test]
    fn structured_log_reader_pages_root_records_and_reads_references() {
        let root = unique_path("reader");
        let logger = StructuredLogger::new(LogSink::directory(&root))
            .unwrap()
            .with_detail_mode(LogDetailMode::Reference);
        logger.record("first", serde_json::json!({"value": 1})).unwrap();
        let reference = logger
            .store_json(&serde_json::json!({"detail": "payload"}))
            .unwrap();
        logger
            .record("second", serde_json::json!({"reference": reference.clone()}))
            .unwrap();

        let reader = StructuredLogReader::configured(LogSink::directory(&root)).unwrap();
        let first = reader.read_page(0, 1).unwrap();
        assert_eq!(first.records.len(), 1);
        assert_eq!(first.next_cursor, Some(1));
        assert_eq!(first.records[0]["kind"], "first");

        let second = reader.read_page(first.next_cursor.unwrap(), 8).unwrap();
        assert_eq!(second.records.len(), 1);
        assert_eq!(second.next_cursor, None);
        assert_eq!(second.records[0]["kind"], "second");

        let bytes = reader.read_reference(&reference).unwrap().unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            serde_json::json!({"detail": "payload"})
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn file_reference_store_rejects_path_traversal() {
        let store = FileContentReferenceStore::new(unique_path("safe"));
        let reference = ContentReference {
            digest: crate::ArtifactRevision::from_content(b"content"),
            media_type: "application/octet-stream".into(),
            bytes: 7,
            locator: crate::ContentLocator::File {
                path: "../outside".into(),
            },
        };
        assert!(store.get(&reference).is_err());
    }
}
