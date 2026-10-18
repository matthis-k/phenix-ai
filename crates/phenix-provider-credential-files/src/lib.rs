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
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_SECRET_BYTES: usize = 64 * 1024;
const MAX_PROFILE_COMPONENT: usize = 64;
#[cfg(test)]
static NONCE: AtomicU64 = AtomicU64::new(0);
// Reserved publish slot, accessed only while exclusively locking the pinned
// directory. No other candidate names are scanned or removed.
const PENDING: &str = ".credential-pending";
// Explicit 0600 sidecar marker for a pending publication. Unlike inode
// extended attributes, this works with O_TMPFILE on Linux filesystems where
// user xattrs are unavailable (including some Nix sandboxes). Both entries
// live under the same pinned, 0700 directory and exclusive directory flock.
// It proves our recovery protocol, not adversarial identity against the UID.
const PENDING_MARKER_NAME: &str = ".credential-pending.marker";
const PENDING_MARKER: &[u8] = b"phenix-pending-v1";

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
    pub fn new(mut data: Vec<u8>) -> Result<Self, CredentialError> {
        if data.is_empty() || data.len() > MAX_SECRET_BYTES || data.contains(&0) {
            data.fill(0);
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
        let backend = Self {
            #[cfg(test)]
            root: path,
            directory,
        };
        let _lock = backend.lock_and_recover()?;
        Ok(backend)
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
        let destination =
            CString::new(destination).map_err(|_| CredentialError::InvalidIdentity)?;
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
        self.store_with_hook(id, secret, |_| {})
    }

    fn mark_pending(&self) -> Result<(), CredentialError> {
        // Publish the marker and fsync the directory before linking the
        // anonymous credential inode. Recovery then knows whether the reserved
        // pending slot belongs to an interrupted writer.
        let mut marker = self
            .open_relative(
                PENDING_MARKER_NAME,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                0o600,
            )
            .map_err(|_| CredentialError::InsecureFile)?;
        let result = (|| -> Result<(), CredentialError> {
            marker
                .write_all(PENDING_MARKER)
                .map_err(|_| CredentialError::Io)?;
            marker.sync_all().map_err(|_| CredentialError::Io)?;
            self.directory.sync_all().map_err(|_| CredentialError::Io)?;
            Ok(())
        })();
        if result.is_err() {
            // This name was created exclusively by this operation.
            let _ = self.unlink_relative(PENDING_MARKER_NAME);
        }
        result
    }

    fn recover_pending(&self) -> Result<(), CredentialError> {
        let mut marker =
            match self.open_relative(PENDING_MARKER_NAME, libc::O_RDONLY | libc::O_NONBLOCK, 0) {
                Ok(file) => Some(file),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(_) => return Err(CredentialError::InsecureFile),
            };
        if let Some(file) = marker.as_mut() {
            let metadata = file.metadata().map_err(|_| CredentialError::InsecureFile)?;
            if !metadata.is_file()
                || metadata.mode() & 0o777 != 0o600
                || metadata.uid() != unsafe { libc::geteuid() }
                || metadata.nlink() != 1
                || metadata.len() != PENDING_MARKER.len() as u64
            {
                return Err(CredentialError::InsecureFile);
            }
            let mut bytes = [0u8; PENDING_MARKER.len()];
            file.read_exact(&mut bytes)
                .map_err(|_| CredentialError::InsecureFile)?;
            if bytes.as_slice() != PENDING_MARKER {
                return Err(CredentialError::InsecureFile);
            }
        }
        let pending = match self.open_relative(PENDING, libc::O_RDONLY | libc::O_NONBLOCK, 0) {
            Ok(file) => Some(file),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(_) => return Err(CredentialError::InsecureFile),
        };
        let had_pending = pending.is_some();
        if let Some(pending) = pending {
            // Never unlink an unknown reserved-name entry. The marker must
            // precede the pending link and the pending inode must be a private
            // regular file, not a symlink, directory or multi-linked inode.
            if marker.is_none() {
                return Err(CredentialError::InsecureFile);
            }
            let metadata = pending
                .metadata()
                .map_err(|_| CredentialError::InsecureFile)?;
            if !metadata.is_file()
                || metadata.mode() & 0o777 != 0o600
                || metadata.uid() != unsafe { libc::geteuid() }
                || metadata.nlink() != 1
                || metadata.len() > MAX_SECRET_BYTES as u64
            {
                return Err(CredentialError::InsecureFile);
            }
            self.unlink_relative(PENDING)?;
        }
        if marker.is_some() {
            self.unlink_relative(PENDING_MARKER_NAME)?;
        }
        if marker.is_some() || had_pending {
            self.directory.sync_all().map_err(|_| CredentialError::Io)?;
        }
        Ok(())
    }

    fn lock_and_recover(&self) -> Result<File, CredentialError> {
        // A fresh open description is essential: dup/flock on the shared fd
        // would not serialize threads using the same backend instance.
        let lock = self
            .open_relative(".", libc::O_RDONLY | libc::O_DIRECTORY, 0)
            .map_err(|_| CredentialError::Io)?;
        loop {
            if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } == 0 {
                break;
            }
            if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                return Err(CredentialError::Io);
            }
        }
        validate_directory(&self.directory)?;
        self.recover_pending()?;
        Ok(lock)
    }

    fn store_with_hook(
        &self,
        id: &CredentialId,
        secret: &Secret,
        checkpoint: impl Fn(&str),
    ) -> Result<(), CredentialError> {
        let destination = id.file_name()?;
        let _lock = self.lock_and_recover()?;
        // No named plaintext candidate exists while writing/syncing. Unsupported
        // filesystems fail closed, rather than falling back to orphanable files.
        let mut output = self
            .open_relative(".", libc::O_RDWR | libc::O_TMPFILE, 0o600)
            .map_err(|_| CredentialError::Io)?;
        output
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| CredentialError::Io)?;
        output
            .write_all(secret.expose_for_authorized_provider())
            .map_err(|_| CredentialError::Io)?;
        output.sync_all().map_err(|_| CredentialError::Io)?;
        self.mark_pending()?;
        checkpoint("synced");
        // /proc/self/fd permits linking our anonymous inode without requiring
        // CAP_DAC_READ_SEARCH (unlike linkat with AT_EMPTY_PATH).
        let source = CString::new(format!("/proc/self/fd/{}", output.as_raw_fd())).unwrap();
        let pending = CString::new(PENDING).unwrap();
        if unsafe {
            libc::linkat(
                libc::AT_FDCWD,
                source.as_ptr(),
                self.directory.as_raw_fd(),
                pending.as_ptr(),
                libc::AT_SYMLINK_FOLLOW,
            )
        } != 0
        {
            // Pending inode was not linked by us; preserve any unexpected entry.
            // The marker was created exclusively by this operation.
            let _ = self.unlink_relative(PENDING_MARKER_NAME);
            return Err(CredentialError::Io);
        }
        let result = (|| -> Result<(), CredentialError> {
            checkpoint("linked");
            self.rename_relative(PENDING, &destination)?;
            self.directory.sync_all().map_err(|_| CredentialError::Io)?;
            self.unlink_relative(PENDING_MARKER_NAME)?;
            self.directory.sync_all().map_err(|_| CredentialError::Io)?;
            Ok(())
        })();
        if result.is_err() {
            // Recovery uses our validated marker to clean a still-linked
            // candidate; never remove another publisher's entry.
            let _ = self.recover_pending();
        }
        result
    }

    pub fn read(&self, id: &CredentialId) -> Result<Secret, CredentialError> {
        let _lock = self.lock_and_recover()?;
        let input = self
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
        let _lock = self.lock_and_recover()?;
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
        let fixture = Fixture::new();
        let root = CredentialFiles::open(&fixture.0, true).unwrap();
        let boundary = CredentialId {
            provider: "a".repeat(64),
            connection: "b".repeat(48),
            profile: "c".into(),
        };
        assert_eq!(boundary.file_name().unwrap().len(), 239);
        root.store(
            &boundary,
            &Secret::new(b"boundary-secret".to_vec()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            root.read(&boundary)
                .unwrap()
                .expose_for_authorized_provider(),
            b"boundary-secret"
        );
        root.revoke(&boundary).unwrap();
        let over = CredentialId {
            profile: "cc".into(),
            ..boundary
        };
        assert_eq!(over.file_name(), Err(CredentialError::InvalidIdentity));
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
    fn same_instance_rechecks_directory_and_file_permissions() {
        let fixture = Fixture::new();
        let root = CredentialFiles::open(&fixture.0, true).unwrap();
        let secret = Secret::new(b"fixture-secret".to_vec()).unwrap();
        root.store(&id("work"), &secret).unwrap();
        let path = root.path(&id("work")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            root.read(&id("work")).unwrap_err(),
            CredentialError::InsecureFile
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            root.read(&id("work")).unwrap_err(),
            CredentialError::InsecureDirectory
        );
        assert_eq!(
            root.store(&id("work"), &secret),
            Err(CredentialError::InsecureDirectory)
        );
        assert_eq!(
            root.revoke(&id("work")),
            Err(CredentialError::InsecureDirectory)
        );
        assert_eq!(fs::read(path).unwrap(), b"fixture-secret");
    }

    #[test]
    fn interrupted_writer_child() {
        let Ok(path) = std::env::var("PHENIX_TEST_INTERRUPTED_VAULT") else {
            return;
        };
        let stage = std::env::var("PHENIX_TEST_INTERRUPTED_STAGE").unwrap();
        let root = CredentialFiles::open(path, true).unwrap();
        root.store_with_hook(
            &id("work"),
            &Secret::new(b"interrupted-secret".to_vec()).unwrap(),
            |at| {
                if at == stage {
                    std::process::exit(73);
                }
            },
        )
        .unwrap();
        panic!("checkpoint not reached");
    }

    #[test]
    fn interrupted_import_and_rotation_recover_without_plaintext_candidates() {
        for stage in ["synced", "linked"] {
            for rotating in [false, true] {
                let fixture = Fixture::new();
                let root = CredentialFiles::open(&fixture.0, true).unwrap();
                if rotating {
                    root.store(&id("work"), &Secret::new(b"old-secret".to_vec()).unwrap())
                        .unwrap();
                }
                let status = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "tests::interrupted_writer_child"])
                    .env("PHENIX_TEST_INTERRUPTED_VAULT", &fixture.0)
                    .env("PHENIX_TEST_INTERRUPTED_STAGE", stage)
                    .status()
                    .unwrap();
                assert_eq!(status.code(), Some(73));
                assert_eq!(fixture.0.join(PENDING).exists(), stage == "linked");
                let reopened = CredentialFiles::open(&fixture.0, true).unwrap();
                assert!(!fixture.0.join(PENDING).exists());
                if rotating {
                    assert_eq!(
                        reopened
                            .read(&id("work"))
                            .unwrap()
                            .expose_for_authorized_provider(),
                        b"old-secret"
                    );
                    reopened.revoke(&id("work")).unwrap();
                } else {
                    assert_eq!(
                        reopened.read(&id("work")).unwrap_err(),
                        CredentialError::NotFound
                    );
                }
                assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
                reopened
                    .store(&id("work"), &Secret::new(b"new-secret".to_vec()).unwrap())
                    .unwrap();
                drop(reopened);
                let restarted = CredentialFiles::open(&fixture.0, true).unwrap();
                assert_eq!(
                    restarted
                        .read(&id("work"))
                        .unwrap()
                        .expose_for_authorized_provider(),
                    b"new-secret"
                );
                restarted.revoke(&id("work")).unwrap();
                assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
            }
        }
    }

    #[test]
    fn recovery_waits_for_live_writer_and_uses_pinned_directory() {
        let fixture = Fixture::new();
        let writer = CredentialFiles::open(&fixture.0, true).unwrap();
        let reader = CredentialFiles::open(&fixture.0, true).unwrap();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let writing = std::thread::spawn(move || {
            writer
                .store_with_hook(
                    &id("work"),
                    &Secret::new(b"live-secret".to_vec()).unwrap(),
                    |stage| {
                        if stage == "linked" {
                            ready_tx.send(()).unwrap();
                            resume_rx.recv().unwrap();
                        }
                    },
                )
                .unwrap();
        });
        ready_rx.recv().unwrap();
        let pinned = fixture.0.with_extension("pinned");
        fs::rename(&fixture.0, &pinned).unwrap();
        fs::create_dir(&fixture.0).unwrap();
        fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(fixture.0.join(PENDING), b"replacement-witness").unwrap();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let reading = std::thread::spawn(move || {
            let secret = reader.read(&id("work")).unwrap();
            assert_eq!(secret.expose_for_authorized_provider(), b"live-secret");
            done_tx.send(()).unwrap();
        });
        assert!(done_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_err());
        assert!(pinned.join(PENDING).exists());
        resume_tx.send(()).unwrap();
        writing.join().unwrap();
        reading.join().unwrap();
        assert!(!pinned.join(PENDING).exists());
        assert_eq!(
            fs::read(fixture.0.join(PENDING)).unwrap(),
            b"replacement-witness"
        );
        fs::remove_dir_all(pinned).unwrap();
    }

    #[test]
    fn recovery_rejects_unmarked_or_symlinked_pending_entries_without_deleting_them() {
        for symlinked in [false, true] {
            let fixture = Fixture::new();
            let reserved = fixture.0.join(PENDING);
            let witness = fixture.0.join("witness");
            fs::write(&witness, b"unowned-witness").unwrap();
            if symlinked {
                symlink(&witness, &reserved).unwrap();
            } else {
                fs::write(&reserved, b"unowned-pending-witness").unwrap();
                fs::set_permissions(&reserved, fs::Permissions::from_mode(0o600)).unwrap();
            }
            assert!(matches!(
                CredentialFiles::open(&fixture.0, true),
                Err(CredentialError::InsecureFile)
            ));
            assert!(reserved.symlink_metadata().is_ok());
            assert_eq!(fs::read(&witness).unwrap(), b"unowned-witness");
            if !symlinked {
                assert_eq!(fs::read(&reserved).unwrap(), b"unowned-pending-witness");
            }
        }
    }

    #[test]
    fn successful_publication_marks_only_owned_recoverable_candidates() {
        let fixture = Fixture::new();
        let root = CredentialFiles::open(&fixture.0, true).unwrap();
        root.store_with_hook(
            &id("work"),
            &Secret::new(b"marked-secret".to_vec()).unwrap(),
            |stage| {
                if stage == "linked" {
                    let pending = root.open_relative(PENDING, libc::O_RDONLY, 0).unwrap();
                    assert!(pending.metadata().unwrap().is_file());
                    let mut marker = root
                        .open_relative(PENDING_MARKER_NAME, libc::O_RDONLY, 0)
                        .unwrap();
                    let mut bytes = [0u8; PENDING_MARKER.len()];
                    marker.read_exact(&mut bytes).unwrap();
                    assert_eq!(bytes, PENDING_MARKER);
                }
            },
        )
        .unwrap();
        assert_eq!(
            root.read(&id("work"))
                .unwrap()
                .expose_for_authorized_provider(),
            b"marked-secret"
        );
    }

    #[test]
    fn candidate_collision_preserves_unowned_file() {
        let fixture = Fixture::new();
        let root = CredentialFiles::open(&fixture.0, true).unwrap();
        let candidate = fixture.0.join(format!(
            ".credential-{}-{}",
            std::process::id(),
            NONCE.load(Ordering::Relaxed)
        ));
        fs::write(&candidate, b"unowned-witness").unwrap();
        let _ = root.store(
            &id("work"),
            &Secret::new(b"fixture-secret".to_vec()).unwrap(),
        );
        assert_eq!(fs::read(candidate).unwrap(), b"unowned-witness");
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
