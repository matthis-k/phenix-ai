//! Explicit opt-in file-backed credential fallback for headless Linux.
//!
//! This is an ordinary library. A runtime-authorized credential provider owns
//! which caller may import, resolve or revoke a named profile. No frontend,
//! model request, journal or Nix expression may receive its secret bytes.
//!
//! A 0600 plaintext file is weaker than an OS secret service. Callers must
//! obtain explicit consent before configuring this backend.

#![cfg(target_os = "linux")]

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_SECRET_BYTES: usize = 64 * 1024;
const MAX_PROFILE_COMPONENT: usize = 64;
static NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialError {
    InvalidIdentity,
    ConsentRequired,
    InsecureDirectory,
    InsecureFile,
    NotFound,
    Io,
}

pub struct Secret {
    data: Vec<u8>,
}

impl Secret {
    pub fn new(data: Vec<u8>) -> Result<Self, CredentialError> {
        if data.is_empty() || data.len() > MAX_SECRET_BYTES || data.contains(&0) {
            return Err(CredentialError::InvalidIdentity);
        }
        Ok(Self { data })
    }

    pub fn expose_for_authorized_provider(&self) -> &[u8] {
        &self.data
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt.write_str("Secret([REDACTED])")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.data.fill(0);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CredentialId {
    pub provider: String,
    pub connection: String,
    pub profile: String,
}

fn component(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_PROFILE_COMPONENT
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

impl CredentialId {
    fn file_name(&self) -> Result<String, CredentialError> {
        if !component(&self.provider) || !component(&self.connection) || !component(&self.profile) {
            return Err(CredentialError::InvalidIdentity);
        }
        // Hex-encode components before joining. Plain "__" separators
        // collide when user-chosen IDs contain underscores.
        fn encode(value: &str) -> String {
            value
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        }
        let file = format!(
            "{}-{}-{}.credential",
            encode(&self.provider),
            encode(&self.connection),
            encode(&self.profile)
        );
        // Linux filesystems commonly cap one filename at 255 bytes. Reject
        // oversized identities explicitly rather than leaking an I/O error.
        if file.len() > 240 {
            return Err(CredentialError::InvalidIdentity);
        }
        Ok(file)
    }
}

/// File fallback only; the default credential-store plugin should prefer a
/// supported OS secret store. `consent` must originate from runtime policy.
pub struct CredentialFiles {
    root: PathBuf,
}

impl CredentialFiles {
    pub fn open(root: impl AsRef<Path>, consent: bool) -> Result<Self, CredentialError> {
        if !consent {
            return Err(CredentialError::ConsentRequired);
        }
        let path = root.as_ref().to_path_buf();
        validate_directory(&path)?;
        Ok(Self { root: path })
    }

    fn path(&self, id: &CredentialId) -> Result<PathBuf, CredentialError> {
        Ok(self.root.join(id.file_name()?))
    }

    pub fn store(&self, id: &CredentialId, secret: &Secret) -> Result<(), CredentialError> {
        validate_directory(&self.root)?;
        let destination = self.path(id)?;
        let candidate = self.root.join(format!(
            ".credential-{}-{}",
            std::process::id(),
            NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> Result<(), CredentialError> {
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&candidate)
                .map_err(|_| CredentialError::Io)?;
            output
                .write_all(secret.expose_for_authorized_provider())
                .map_err(|_| CredentialError::Io)?;
            output.sync_all().map_err(|_| CredentialError::Io)?;
            fs::rename(&candidate, destination).map_err(|_| CredentialError::Io)?;
            File::open(&self.root)
                .and_then(|dir| dir.sync_all())
                .map_err(|_| CredentialError::Io)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(candidate);
        }
        result
    }

    pub fn read(&self, id: &CredentialId) -> Result<Secret, CredentialError> {
        validate_directory(&self.root)?;
        let path = self.path(id)?;
        // Linux open(2): O_NOFOLLOW protects against a final-component symlink.
        let mut input = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|err| {
                if err.kind() == io::ErrorKind::NotFound {
                    CredentialError::NotFound
                } else {
                    CredentialError::Io
                }
            })?;
        let metadata = input.metadata().map_err(|_| CredentialError::Io)?;
        if !metadata.is_file()
            || metadata.mode() & 0o777 != 0o600
            || metadata.nlink() != 1
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.len() > MAX_SECRET_BYTES as u64
        {
            return Err(CredentialError::InsecureFile);
        }
        let mut bytes = Vec::new();
        input
            .read_to_end(&mut bytes)
            .map_err(|_| CredentialError::Io)?;
        Secret::new(bytes)
    }

    pub fn revoke(&self, id: &CredentialId) -> Result<(), CredentialError> {
        validate_directory(&self.root)?;
        fs::remove_file(self.path(id)?).map_err(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                CredentialError::NotFound
            } else {
                CredentialError::Io
            }
        })?;
        File::open(&self.root)
            .and_then(|dir| dir.sync_all())
            .map_err(|_| CredentialError::Io)
    }
}

fn validate_directory(path: &Path) -> Result<(), CredentialError> {
    // symlink_metadata does not follow a final-component symlink.
    let metadata = fs::symlink_metadata(path).map_err(|_| CredentialError::InsecureDirectory)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.mode() & 0o777 != 0o700
        || metadata.uid() != unsafe { libc::geteuid() }
    {
        return Err(CredentialError::InsecureDirectory);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "phenix-credential-files-{}-{}",
                std::process::id(),
                NONCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&directory).unwrap();
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
            Self(directory)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn id(profile: &str) -> CredentialId {
        CredentialId {
            provider: "openai".into(),
            connection: "api".into(),
            profile: profile.into(),
        }
    }

    #[test]
    fn requires_consent_and_private_owned_directory() {
        let fixture = Fixture::new();
        assert!(matches!(
            CredentialFiles::open(&fixture.0, false),
            Err(CredentialError::ConsentRequired)
        ));
        let root = CredentialFiles::open(&fixture.0, true).unwrap();
        assert!(root.path(&id("work")).is_ok());
        fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            CredentialFiles::open(&fixture.0, true),
            Err(CredentialError::InsecureDirectory)
        ));
    }

    #[test]
    fn distinct_named_profiles_survive_restart_and_revoke_independently() {
        let fixture = Fixture::new();
        let root = CredentialFiles::open(&fixture.0, true).unwrap();
        root.store(&id("personal"), &Secret::new(b"alpha".to_vec()).unwrap())
            .unwrap();
        root.store(&id("work"), &Secret::new(b"beta".to_vec()).unwrap())
            .unwrap();
        assert_eq!(
            fs::metadata(root.path(&id("personal")).unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let reopened = CredentialFiles::open(&fixture.0, true).unwrap();
        assert_eq!(
            reopened
                .read(&id("personal"))
                .unwrap()
                .expose_for_authorized_provider(),
            b"alpha"
        );
        assert_eq!(
            reopened
                .read(&id("work"))
                .unwrap()
                .expose_for_authorized_provider(),
            b"beta"
        );
        reopened.revoke(&id("personal")).unwrap();
        assert_eq!(
            reopened.read(&id("personal")).unwrap_err(),
            CredentialError::NotFound
        );
        assert_eq!(
            reopened
                .read(&id("work"))
                .unwrap()
                .expose_for_authorized_provider(),
            b"beta"
        );
    }

    #[test]
    fn distinct_component_tuples_never_alias_credential_files() {
        let first = CredentialId {
            provider: "a__b".into(),
            connection: "c".into(),
            profile: "d".into(),
        };
        let second = CredentialId {
            provider: "a".into(),
            connection: "b__c".into(),
            profile: "d".into(),
        };
        assert_ne!(first.file_name().unwrap(), second.file_name().unwrap());
    }

    #[test]
    fn a_long_combined_identity_cannot_exceed_filename_limits() {
        let too_long = CredentialId {
            provider: "a".repeat(64),
            connection: "b".repeat(64),
            profile: "c".repeat(64),
        };
        assert_eq!(
            too_long.file_name(),
            Err(CredentialError::InvalidIdentity)
        );
    }

    #[test]
    fn path_traversal_and_symlink_secret_reads_are_denied() {
        let fixture = Fixture::new();
        let root = CredentialFiles::open(&fixture.0, true).unwrap();
        assert_eq!(
            root.path(&id("../secret")),
            Err(CredentialError::InvalidIdentity)
        );
        let outside = fixture.0.join("outside");
        fs::write(&outside, b"unsafe").unwrap();
        let destination = root.path(&id("linked")).unwrap();
        symlink(outside, destination).unwrap();
        assert!(root.read(&id("linked")).is_err());
        let linked_dir = fixture.0.with_extension("alias");
        symlink(&fixture.0, &linked_dir).unwrap();
        assert!(matches!(
            CredentialFiles::open(&linked_dir, true),
            Err(CredentialError::InsecureDirectory)
        ));
        fs::remove_file(linked_dir).unwrap();
    }

    #[test]
    fn cannot_log_secret_data_via_debug_or_error() {
        let secret = Secret::new(b"do-not-log-me".to_vec()).unwrap();
        assert!(!format!("{secret:?}").contains("do-not-log-me"));
        assert!(matches!(
            Secret::new(Vec::new()),
            Err(CredentialError::InvalidIdentity)
        ));
    }
}
