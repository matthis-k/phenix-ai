//! Explicit opt-in file-backed credential fallback for headless Linux.
//!
//! This is an ordinary library. A runtime-authorized credential provider owns
//! which caller may import, resolve or revoke a named profile. No frontend,
//! model request, journal or Nix expression may receive its secret bytes.
//!
//! A 0600 plaintext file is weaker than an OS secret service. Callers must
//! obtain explicit consent before configuring this backend.

#![cfg(target_os = "linux")]

use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
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
    // Kept for display and test inspection only. All security-sensitive file
    // operations are relative to the pinned directory descriptor below.
    #[cfg(test)]
    root: PathBuf,
    directory: File,
}

impl CredentialFiles {
    pub fn open(root: impl AsRef<Path>, consent: bool) -> Result<Self, CredentialError> {
        if !consent {
            return Err(CredentialError::ConsentRequired);
        }
        let path = root.as_ref().to_path_buf();
        // Do not resolve a final symlink. A subsequent rename or replacement
        // of `path` cannot silently redirect operations away from this fd.
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .map_err(|_| CredentialError::InsecureDirectory)?;
        validate_directory(&directory)?;
        Ok(Self {
            #[cfg(test)]
            root: path,
            directory,
        })
    }

    #[cfg(test)]
    fn path(&self, id: &CredentialId) -> Result<PathBuf, CredentialError> {
        Ok(self.root.join(id.file_name()?))
    }

    /// Resolve a validated leaf name relative to the originally approved
    /// directory, rather than re-resolving a potentially replaced path.
    fn open_relative(&self, name: &str, flags: i32, mode: libc::mode_t) -> io::Result<File> {
        let name = CString::new(name)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid leaf name"))?;
        // SAFETY: the directory fd and CString are valid for the duration of
        // openat. On success, ownership of the returned fd transfers to File.
        let fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                mode,
            )
        };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            // SAFETY: openat returned a new, owned, nonnegative descriptor.
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }

    fn rename_relative(&self, source: &str, destination: &str) -> Result<(), CredentialError> {
        let source = CString::new(source).map_err(|_| CredentialError::InvalidIdentity)?;
        let destination = CString::new(destination).map_err(|_| CredentialError::InvalidIdentity)?;
        // SAFETY: both names are validated leaf components, and the pinned
        // directory descriptor is live. No symlink traversal of parent paths.
        let status = unsafe {
            libc::renameat(
                self.directory.as_raw_fd(),
                source.as_ptr(),
                self.directory.as_raw_fd(),
                destination.as_ptr(),
            )
        };
        if status != 0 {
            return Err(CredentialError::Io);
        }
        Ok(())
    }

    fn unlink_relative(&self, name: &str) -> Result<(), CredentialError> {
        let name = CString::new(name).map_err(|_| CredentialError::InvalidIdentity)?;
        // SAFETY: only a single leaf path is resolved under the pinned fd.
        let status = unsafe { libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0) };
        if status != 0 {
            let err = io::Error::last_os_error();
            return Err(if err.kind() == io::ErrorKind::NotFound {
                CredentialError::NotFound
            } else {
                CredentialError::Io
            });
        }
        Ok(())
    }

    pub fn store(&self, id: &CredentialId, secret: &Secret) -> Result<(), CredentialError> {
        validate_directory(&self.directory)?;
        let destination = id.file_name()?;
        let candidate = format!(
            ".credential-{}-{}",
            std::process::id(),
            NONCE.fetch_add(1, Ordering::Relaxed)
        );
        let result = (|| -> Result<(), CredentialError> {
            let mut output = self
                .open_relative(
                    &candidate,
                    libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                    0o600,
                )
                .map_err(|_| CredentialError::Io)?;
            // A restrictive umask must not produce an unreadable stored secret.
            output
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|_| CredentialError::Io)?;
            output
                .write_all(secret.expose_for_authorized_provider())
                .map_err(|_| CredentialError::Io)?;
            output.sync_all().map_err(|_| CredentialError::Io)?;
            self.rename_relative(&candidate, &destination)?;
            self.directory.sync_all().map_err(|_| CredentialError::Io)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = self.unlink_relative(&candidate);
        }
        result
    }

    pub fn read(&self, id: &CredentialId) -> Result<Secret, CredentialError> {
        validate_directory(&self.directory)?;
        let mut input = self
            .open_relative(&id.file_name()?, libc::O_RDONLY | libc::O_NONBLOCK, 0)
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
        // A concurrent writer may enlarge a file after fstat: never allow
        // unbounded secret allocation even if the initial length was valid.
        input
            .take(MAX_SECRET_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| CredentialError::Io)?;
        if bytes.len() > MAX_SECRET_BYTES {
            bytes.fill(0);
            return Err(CredentialError::InsecureFile);
        }
        Secret::new(bytes)
    }

    pub fn revoke(&self, id: &CredentialId) -> Result<(), CredentialError> {
        validate_directory(&self.directory)?;
        self.unlink_relative(&id.file_name()?)?;
        self.directory.sync_all().map_err(|_| CredentialError::Io)
    }
}

fn validate_directory(directory: &File) -> Result<(), CredentialError> {
    let metadata = directory
        .metadata()
        .map_err(|_| CredentialError::InsecureDirectory)?;
    if !metadata.is_dir()
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
        assert_eq!(too_long.file_name(), Err(CredentialError::InvalidIdentity));
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
    fn replacing_the_configured_path_cannot_redirect_open_vault_operations() {
        let fixture = Fixture::new();
        let root = CredentialFiles::open(&fixture.0, true).unwrap();
        let original = Secret::new(b"original-secret".to_vec()).unwrap();
        root.store(&id("work"), &original).unwrap();

        // A validated path is not a stable authority boundary: the parent
        // can replace it after the vault opens, without using a symlink.
        let pinned = fixture.0.with_extension("original");
        fs::rename(&fixture.0, &pinned).unwrap();
        fs::create_dir(&fixture.0).unwrap();
        fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o700)).unwrap();
        let filename = id("work").file_name().unwrap();
        let impostor = fixture.0.join(&filename);
        fs::write(&impostor, b"impostor-secret").unwrap();
        fs::set_permissions(&impostor, fs::Permissions::from_mode(0o600)).unwrap();

        assert_eq!(
            root.read(&id("work"))
                .unwrap()
                .expose_for_authorized_provider(),
            b"original-secret"
        );
        root.store(
            &id("work"),
            &Secret::new(b"updated-secret".to_vec()).unwrap(),
        )
        .unwrap();
        assert_eq!(fs::read(pinned.join(&filename)).unwrap(), b"updated-secret");
        assert_eq!(fs::read(&impostor).unwrap(), b"impostor-secret");
        root.revoke(&id("work")).unwrap();
        assert!(!pinned.join(&filename).exists());
        assert_eq!(fs::read(&impostor).unwrap(), b"impostor-secret");

        fs::remove_dir_all(&pinned).unwrap();
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
