use crate::ArtifactRevision;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    env,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
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
    AppendFile(PathBuf),
    TruncateFile(PathBuf),
}

impl LogSink {
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
            Self::AppendFile(path) => format!("append:{}", path.display()),
            Self::TruncateFile(path) => format!("truncate:{}", path.display()),
        }
    }

    fn inferred_store_root(&self) -> Option<PathBuf> {
        let path = match self {
            Self::AppendFile(path) | Self::TruncateFile(path) => path,
            Self::Stderr | Self::Stdout => return None,
        };
        let mut root = OsString::from(path.as_os_str());
        root.push(".d");
        Some(PathBuf::from(root).join("objects"))
    }
}

fn non_empty_path(
    path: &str,
    constructor: impl FnOnce(PathBuf) -> LogSink,
) -> Result<LogSink, String> {
    if path.is_empty() {
        return Err("file log sink requires a path".into());
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "snake_case")]
pub enum ContentLocator {
    File { path: String },
    Service { service: String, resource: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContentReference {
    pub digest: ArtifactRevision,
    pub media_type: String,
    pub bytes: usize,
    pub locator: ContentLocator,
}

impl ContentReference {
    #[must_use]
    pub fn new(
        content: &[u8],
        media_type: impl Into<String>,
        locator: ContentLocator,
    ) -> Self {
        Self {
            digest: ArtifactRevision::from_content(content),
            media_type: media_type.into(),
            bytes: content.len(),
            locator,
        }
    }
}

pub trait ContentReferenceStore {
    fn put(&self, media_type: &str, content: &[u8]) -> Result<ContentReference, String>;
    fn get(&self, reference: &ContentReference) -> Result<Option<Vec<u8>>, String>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileContentReferenceStore {
    root: PathBuf,
}

impl FileContentReferenceStore {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl ContentReferenceStore for FileContentReferenceStore {
    fn put(&self, media_type: &str, content: &[u8]) -> Result<ContentReference, String> {
        let digest = ArtifactRevision::from_content(content);
        let hex = digest
            .as_ref()
            .strip_prefix("sha256:")
            .expect("artifact revisions use canonical sha256 identities");
        let relative = PathBuf::from("sha256").join(&hex[..2]).join(hex);
        let path = self.root.join(&relative);
        persist_content_addressed(&path, &digest, content)?;
        Ok(ContentReference {
            digest,
            media_type: media_type.into(),
            bytes: content.len(),
            locator: ContentLocator::File {
                path: portable_relative_path(&relative)?,
            },
        })
    }

    fn get(&self, reference: &ContentReference) -> Result<Option<Vec<u8>>, String> {
        let ContentLocator::File { path } = &reference.locator else {
            return Ok(None);
        };
        let relative = safe_relative_path(path)?;
        let path = self.root.join(relative);
        let Some(content) = fs::read(&path)
            .map(Some)
            .or_else(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    Ok(None)
                } else {
                    Err(error)
                }
            })
            .map_err(|error| format!("{}: {error}", path.display()))?
        else {
            return Ok(None);
        };
        if content.len() != reference.bytes {
            return Err(format!(
                "referenced content length mismatch for {}: expected {}, got {}",
                path.display(),
                reference.bytes,
                content.len()
            ));
        }
        let actual = ArtifactRevision::from_content(&content);
        if actual != reference.digest {
            return Err(format!(
                "referenced content digest mismatch for {}: expected {}, got {}",
                path.display(),
                reference.digest,
                actual
            ));
        }
        Ok(Some(content))
    }
}

enum LogWriter {
    Stderr,
    Stdout,
    File(File),
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
        logger.detail_mode = LogDetailMode::from_env()?.unwrap_or(LogDetailMode::Inline);
        if let Some(root) = env::var_os(PHENIX_LOG_STORE_ENV)
            .filter(|root| !root.as_os_str().is_empty())
        {
            logger.reference_store = Some(FileContentReferenceStore::new(root));
        }
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

fn persist_content_addressed(
    path: &Path,
    expected: &ArtifactRevision,
    content: &[u8],
) -> Result<(), String> {
    if path.exists() {
        return verify_content(path, expected);
    }
    let parent = path
        .parent()
        .ok_or_else(|| format!("content-addressed path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let mut temporary = OsString::from(path.as_os_str());
    temporary.push(format!(".tmp-{}-{nonce}", process::id()));
    let temporary = PathBuf::from(temporary);

    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("{}: {error}", temporary.display()))?;
        file.write_all(content)
            .map_err(|error| format!("{}: {error}", temporary.display()))?;
        file.sync_all()
            .map_err(|error| format!("{}: {error}", temporary.display()))?;
        fs::rename(&temporary, path)
            .map_err(|error| format!("{} -> {}: {error}", temporary.display(), path.display()))?;
        verify_content(path, expected)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn verify_content(path: &Path, expected: &ArtifactRevision) -> Result<(), String> {
    let content = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let actual = ArtifactRevision::from_content(&content);
    if &actual != expected {
        return Err(format!(
            "content-addressed object mismatch at {}: expected {}, got {}",
            path.display(),
            expected,
            actual
        ));
    }
    Ok(())
}

fn portable_relative_path(path: &Path) -> Result<String, String> {
    let path = path
        .to_str()
        .ok_or_else(|| "content reference path must be UTF-8".to_owned())?;
    Ok(path.replace('\\', "/"))
}

fn safe_relative_path(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("content reference path is not a safe relative path: {path:?}"));
    }
    Ok(path)
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
        env::temp_dir().join(format!(
            "phenix-core-log-{}-{nonce}-{name}",
            process::id()
        ))
    }

    #[test]
    fn sink_parser_supports_console_and_file_modes() {
        assert_eq!(LogSink::parse("stderr").unwrap(), LogSink::Stderr);
        assert_eq!(LogSink::parse("console").unwrap(), LogSink::Stderr);
        assert_eq!(LogSink::parse("stdout").unwrap(), LogSink::Stdout);
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
        assert_eq!(LogDetailMode::parse("summary").unwrap(), LogDetailMode::Summary);
        assert_eq!(
            LogDetailMode::parse("reference").unwrap(),
            LogDetailMode::Reference
        );
        assert_eq!(LogDetailMode::parse("inline").unwrap(), LogDetailMode::Inline);
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
        let reference: ContentReference = serde_json::from_value(
            record["payload"]["detail"]["reference"].clone(),
        )
        .unwrap();
        let stored = logger
            .reference_store()
            .unwrap()
            .get(&reference)
            .unwrap()
            .unwrap();
        let stored: Value = serde_json::from_slice(&stored).unwrap();
        assert_eq!(stored["body"], marker);
        assert_eq!(reference.digest, ArtifactRevision::from_content(
            &canonical_json_bytes(&stored).unwrap()
        ));
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
    fn file_reference_store_rejects_path_traversal() {
        let store = FileContentReferenceStore::new(unique_path("safe"));
        let reference = ContentReference {
            digest: ArtifactRevision::from_content(b"content"),
            media_type: "application/octet-stream".into(),
            bytes: 7,
            locator: ContentLocator::File {
                path: "../outside".into(),
            },
        };
        assert!(store.get(&reference).is_err());
    }
}
