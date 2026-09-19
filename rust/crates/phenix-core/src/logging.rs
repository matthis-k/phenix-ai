use serde::Serialize;
use serde_json::Value;
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

pub const PHENIX_LOG_ENV: &str = "PHENIX_LOG";

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

enum LogWriter {
    Stderr,
    Stdout,
    File(File),
}

pub struct StructuredLogger {
    sink: LogSink,
    writer: Mutex<LogWriter>,
}

impl StructuredLogger {
    pub fn new(sink: LogSink) -> Result<Self, String> {
        let writer = match &sink {
            LogSink::Stderr => LogWriter::Stderr,
            LogSink::Stdout => LogWriter::Stdout,
            LogSink::AppendFile(path) => LogWriter::File(open_file(path, false)?),
            LogSink::TruncateFile(path) => LogWriter::File(open_file(path, true)?),
        };
        Ok(Self {
            sink,
            writer: Mutex::new(writer),
        })
    }

    #[must_use]
    pub fn sink(&self) -> &LogSink {
        &self.sink
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

fn open_file(path: &Path, truncate: bool) -> Result<File, String> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    if truncate {
        options.truncate(true);
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
            "phenix-core-log-{}-{nonce}-{name}.jsonl",
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
    fn append_file_preserves_existing_records_across_logger_instances() {
        let path = unique_path("append");
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
        let path = unique_path("truncate");
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
    fn record_accepts_arbitrary_serializable_payloads() {
        let path = unique_path("payload");
        let logger = StructuredLogger::new(LogSink::append_file(&path)).unwrap();
        let payload = Value::String("metadata-only".into());
        logger.record("fixture", payload).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("metadata-only"));
        let _ = fs::remove_file(path);
    }
}
