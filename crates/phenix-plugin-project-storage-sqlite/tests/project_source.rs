//! Project-source RED acceptance. A shared record has a verifiable revision,
//! not just an untrusted project ID.
use phenix_advanced_contracts::{
    compare_source, EvidenceScopeKind, ProjectRecord, SourceMatch, SourceObservation,
};
use phenix_plugin_project_storage_sqlite::project_record_reference;
use phenix_sdk::Blob;

#[test]
fn project_source_is_exact_and_revision_sensitive() {
    let record = ProjectRecord {
        revision: 1,
        data: Blob(b"violet".to_vec()),
    };
    let reference = project_record_reference("alpha", "theme", &record).unwrap();
    assert_eq!(reference.scope.kind, EvidenceScopeKind::Project);
    assert_eq!(reference.scope.id, "alpha");
    assert_eq!(reference.resource, "phenix.project.alpha");
    let actual = SourceObservation {
        owner: reference.owner.clone(),
        resource: reference.resource.clone(),
        revision: reference.revision.clone(),
        digest_sha256: reference.digest_sha256.clone(),
    };
    assert_eq!(compare_source(&reference, &actual), SourceMatch::Current);
    let changed = ProjectRecord {
        revision: 2,
        data: Blob(b"blue".to_vec()),
    };
    let updated = project_record_reference("alpha", "theme", &changed).unwrap();
    assert_ne!(reference.revision, updated.revision);
    assert_ne!(reference.digest_sha256, updated.digest_sha256);
    assert_eq!(
        compare_source(
            &reference,
            &SourceObservation {
                owner: updated.owner,
                resource: updated.resource,
                revision: updated.revision,
                digest_sha256: updated.digest_sha256,
            }
        ),
        SourceMatch::Changed
    );
}

#[test]
fn invalid_keys_cannot_be_encoded_as_project_sources() {
    let record = ProjectRecord {
        revision: 1,
        data: Blob(vec![1]),
    };
    assert!(project_record_reference("../escape", "theme", &record).is_err());
    assert!(project_record_reference("alpha", "../escape", &record).is_err());
    assert!(project_record_reference(
        "alpha",
        "theme",
        &ProjectRecord {
            revision: 0,
            data: record.data
        }
    )
    .is_err());
}
