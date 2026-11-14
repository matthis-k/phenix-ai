use phenix_storage::{Capability, Error as StorageError, scoped};
use phenix_storage_plugin::Error;
use std::error::Error as _;

#[test]
fn storage_and_scoped_errors_preserve_the_typed_original_cause() {
    let error = Error::from(scoped::Error::from(StorageError::UnsupportedCapability {
        capability: Capability::AtomicReadMany,
    }));
    let scoped = error
        .source()
        .unwrap()
        .downcast_ref::<scoped::Error>()
        .unwrap();
    let original = scoped
        .source()
        .unwrap()
        .downcast_ref::<StorageError>()
        .unwrap();
    assert!(matches!(
        original,
        StorageError::UnsupportedCapability {
            capability: Capability::AtomicReadMany,
        }
    ));
    let direct = Error::from(StorageError::Denied);
    assert!(matches!(
        direct.source().unwrap().downcast_ref::<StorageError>(),
        Some(StorageError::Denied)
    ));
    assert!(Error::Denied.source().is_none());
}

#[test]
fn filesystem_errors_retain_their_kind_instead_of_only_display_text() {
    let error = Error::from(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "host placement denied",
    ));
    let cause = error
        .source()
        .unwrap()
        .downcast_ref::<std::io::Error>()
        .unwrap();
    assert_eq!(cause.kind(), std::io::ErrorKind::PermissionDenied);
}
