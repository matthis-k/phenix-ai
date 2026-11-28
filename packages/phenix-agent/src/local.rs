//! Real bounded local text-file reads. The host opens explicitly chosen files;
//! tool arguments are aliases, never filesystem paths. No directory traversal
//! or shell execution is exposed. This is not a sandbox for arbitrary Rust code.

use crate::tools::Tool;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    sync::Mutex,
};

pub const READ_TEXT: &str = "read-text";
pub const FILES: &str = "readable-files";

/// Read-only handles opened and granted by the trusted host. The capability
/// exposes no path-opening method to tool code.
pub struct ReadableFiles {
    files: BTreeMap<String, Mutex<File>>,
    max_bytes: usize,
}

impl ReadableFiles {
    pub fn new(files: BTreeMap<String, File>, max_bytes: usize) -> Self {
        Self {
            files: files
                .into_iter()
                .map(|(alias, file)| (alias, Mutex::new(file)))
                .collect(),
            max_bytes,
        }
    }

    pub fn read_text(&self, alias: &str) -> Result<String, String> {
        let file = self.files.get(alias).ok_or("file alias not granted")?;
        let mut file = file.lock().map_err(|_| "file handle unavailable")?;
        file.seek(SeekFrom::Start(0))
            .map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        let limit = u64::try_from(self.max_bytes)
            .map_err(|_| "file limit too large")?
            .saturating_add(1);
        file.by_ref()
            .take(limit)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > self.max_bytes {
            return Err("file exceeds read limit".into());
        }
        String::from_utf8(bytes).map_err(|_| "file is not UTF-8".into())
    }
}

/// Same tool definition can be placed in any selected execution environment.
/// The environment's host grant supplies its file set.
pub fn read_text() -> Tool {
    Tool::with_resource(READ_TEXT, FILES, |files: &ReadableFiles, alias: String| {
        files.read_text(&alias)
    })
    .description("Read UTF-8 text from a host-granted file alias")
}
