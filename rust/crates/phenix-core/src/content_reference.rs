use crate::ArtifactRevision;
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "snake_case")]
pub enum ContentLocator {
    File { path: String },
    Service { service: String, resource: String },
}

#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContentReference {
    pub digest: ArtifactRevision,
    pub media_type: String,
    pub bytes: usize,
    pub locator: ContentLocator,
}

impl ContentReference {
    #[must_use]
    pub fn new(content: &[u8], media_type: impl Into<String>, locator: ContentLocator) -> Self {
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
        return Err(format!(
            "content reference path is not a safe relative path: {path:?}"
        ));
    }
    Ok(path)
}
