//! P2-2a packaged project isolation and revision test using real native SQLite.
use phenix_advanced_contracts::{
    compare_source, EvidenceError, EvidenceProblem, EvidenceScope, EvidenceScopeKind, ProjectEdit,
    ProjectMember, ProjectMemberChange, ProjectMemberKey, ProjectMembershipError,
    ProjectMembershipProblem, ProjectRecord, ProjectRecordKey, ProjectStorageError,
    ProjectStorageProblem, ProjectTransaction, SourceMatch, SourceObservation, SourceRef,
};
use phenix_contract::{Authority, Portable, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection},
    Outcome,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path};

const INTERFACE: &str = "phenix.advanced.project-storage@1";
const MEMBER_INTERFACE: &str = "phenix.advanced.project-membership@1";
const PROVIDER: &str = "phenix.advanced.project-storage-sqlite";

struct Fixture {
    package: String,
    definitions: Vec<String>,
}
impl Fixture {
    fn composition(
        &self,
        resource: String,
        interface: &str,
        allowed: &[&str],
        operation: &str,
    ) -> Composition {
        let grants = allowed
            .iter()
            .map(|operation| ScopedGrant {
                caller: "root".into(),
                interface: interface.into(),
                operation: (*operation).into(),
                resources: BTreeSet::from([resource.clone()]),
            })
            .collect::<Vec<_>>();
        Composition {
            format: "phenix.composition.v3".into(),
            artifacts: vec![self.package.clone()],
            definitions: self.definitions.clone(),
            entry: Selection {
                interface: interface.into(),
                operation: operation.into(),
                provider: PROVIDER.into(),
            },
            slots: vec![],
            authority: Authority {
                operations: grants.iter().map(ScopedGrant::key).collect(),
                resources: BTreeSet::from([resource.clone()]),
            },
            grants,
            excluded: BTreeSet::new(),
            substitutions: vec![],
            resource: Some(resource),
        }
    }

    fn invoke_selected(
        &self,
        resource: String,
        interface: &str,
        allowed: &[&str],
        operation: &str,
        input: Value,
    ) -> Result<Outcome, phenix_contract::Failure> {
        let policy = self.composition(resource, interface, allowed, operation);
        let kernel = Prepared::prepare(policy)?.load()?;
        let root = kernel.admit_default_root()?;
        root.invoke(&input)
    }

    fn invoke(
        &self,
        project: &str,
        operation: &str,
        input: Value,
    ) -> Result<Outcome, phenix_contract::Failure> {
        self.invoke_selected(
            format!("phenix.project.{project}"),
            INTERFACE,
            &["read", "transact"],
            operation,
            input,
        )
    }

    fn session_project_read(&self, session: &str, project: &str, name: &str) -> Outcome {
        self.invoke_selected(
            format!("phenix.session.{session}"),
            INTERFACE,
            &["read"],
            "read",
            key(project, name),
        )
        .unwrap()
    }

    fn source_call(&self, session: &str, reference: SourceRef) -> Outcome {
        self.invoke_selected(
            format!("phenix.session.{session}"),
            "phenix.advanced.source@1",
            &["observe"],
            "observe",
            reference.to_value(),
        )
        .unwrap()
    }

    fn member_call(&self, resource: &str, operation: &str, input: Value) -> Outcome {
        self.invoke_selected(
            resource.into(),
            MEMBER_INTERFACE,
            &["set", "get"],
            operation,
            input,
        )
        .unwrap()
    }

    fn member_ok(&self, resource: &str, operation: &str, input: Value) -> ProjectMember {
        let result = self.member_call(resource, operation, input);
        let Outcome::Success(value) = result else {
            panic!("expected membership success, got {result:?}");
        };
        ProjectMember::from_value(value).unwrap()
    }

    fn member_deny(
        &self,
        resource: &str,
        operation: &str,
        input: Value,
        reason: ProjectMembershipProblem,
    ) {
        let result = self.member_call(resource, operation, input);
        let Outcome::Domain(value) = result else {
            panic!("expected membership domain denial, got {result:?}");
        };
        assert_eq!(
            ProjectMembershipError::from_value(value).unwrap().problem,
            reason
        );
    }

    fn ok<T: Portable>(&self, root_project: &str, operation: &str, request: Value) -> T {
        let result = self.invoke(root_project, operation, request).unwrap();
        let Outcome::Success(value) = result else {
            panic!("expected {operation} success, got {result:?}");
        };
        T::from_value(value).unwrap()
    }

    fn expect(
        &self,
        root_project: &str,
        operation: &str,
        request: Value,
        code: ProjectStorageProblem,
    ) {
        let result = self.invoke(root_project, operation, request).unwrap();
        let Outcome::Domain(value) = result else {
            panic!("expected {operation} domain denial, got {result:?}");
        };
        let actual = ProjectStorageError::from_value(value).unwrap();
        assert_eq!(actual.problem, code, "wrong project storage failure");
    }

    fn read(&self, root_project: &str, request_project: &str, name: &str) -> ProjectRecord {
        self.ok(
            root_project,
            "read",
            ProjectRecordKey {
                project: request_project.into(),
                key: name.into(),
            }
            .to_value(),
        )
    }

    fn edit(
        &self,
        root_project: &str,
        request_project: &str,
        updates: Vec<ProjectEdit>,
    ) -> Vec<ProjectRecord> {
        self.ok(
            root_project,
            "transact",
            ProjectTransaction {
                project: request_project.into(),
                edits: updates,
            }
            .to_value(),
        )
    }
}

fn edit(key: &str, expected: u64, bytes: &[u8]) -> ProjectEdit {
    ProjectEdit {
        key: key.into(),
        expected,
        data: phenix_contract::Blob(bytes.to_vec()),
    }
}
fn key(project: &str, name: &str) -> Value {
    ProjectRecordKey {
        project: project.into(),
        key: name.into(),
    }
    .to_value()
}
fn txn(project: &str, updates: Vec<ProjectEdit>) -> Value {
    ProjectTransaction {
        project: project.into(),
        edits: updates,
    }
    .to_value()
}
fn write(fixture: &Fixture) -> serde_json::Value {
    fixture.expect(
        "alpha",
        "read",
        key("alpha", "theme"),
        ProjectStorageProblem::NotFound,
    );

    let created = fixture.edit("alpha", "alpha", vec![edit("theme", 0, b"violet")]);
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].revision, 1);
    fixture.expect(
        "alpha",
        "transact",
        txn("alpha", vec![edit("theme", 0, b"stale")]),
        ProjectStorageProblem::Conflict,
    );
    fixture.expect(
        "alpha",
        "transact",
        txn(
            "alpha",
            vec![edit("scratch", 0, b"leak"), edit("theme", 0, b"stale")],
        ),
        ProjectStorageProblem::Conflict,
    );
    fixture.expect(
        "alpha",
        "read",
        key("alpha", "scratch"),
        ProjectStorageProblem::NotFound,
    );

    fixture.expect(
        "alpha",
        "read",
        key("beta", "theme"),
        ProjectStorageProblem::Denied,
    );
    fixture.expect(
        "alpha",
        "transact",
        txn("beta", vec![edit("theme", 0, b"forged")]),
        ProjectStorageProblem::Denied,
    );
    fixture.expect(
        "alpha",
        "read",
        key("alpha", "../escape"),
        ProjectStorageProblem::InvalidInput,
    );

    let changed = fixture.edit("alpha", "alpha", vec![edit("theme", 1, b"blue")]);
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].revision, 2);
    assert_eq!(fixture.read("alpha", "alpha", "theme").data.0, b"blue");
    serde_json::json!({
        "accepted": true,
        "initial_missing": "not_found",
        "created_revision": created[0].revision,
        "updated_revision": changed[0].revision,
        "stale_cas": "conflict",
        "atomic_rollback": true,
        "foreign_project_read": "denied",
        "foreign_project_write": "denied",
        "invalid_key": "rejected",
    })
}

fn restart(fixture: &Fixture) -> serde_json::Value {
    let shared = fixture.read("alpha", "alpha", "theme");
    assert_eq!(shared.revision, 2);
    assert_eq!(shared.data.0, b"blue");
    fixture.expect(
        "beta",
        "read",
        key("beta", "theme"),
        ProjectStorageProblem::NotFound,
    );
    fixture.expect(
        "alpha",
        "read",
        key("beta", "theme"),
        ProjectStorageProblem::Denied,
    );
    serde_json::json!({
        "accepted": true,
        "shared_project_revision": shared.revision,
        "shared_project_data": String::from_utf8(shared.data.0).unwrap(),
        "other_project_missing": "not_found",
        "foreign_project_read": "denied",
    })
}

fn member_key(project: &str, session: &str) -> Value {
    ProjectMemberKey {
        project: project.into(),
        session: session.into(),
    }
    .to_value()
}
fn member_change(project: &str, session: &str, expected: u64, active: bool) -> Value {
    ProjectMemberChange {
        project: project.into(),
        session: session.into(),
        expected,
        active,
    }
    .to_value()
}
fn member_grant(fixture: &Fixture) -> serde_json::Value {
    for session in ["s1", "s2"] {
        let granted = fixture.member_ok(
            "phenix.project.alpha",
            "set",
            member_change("alpha", session, 0, true),
        );
        assert_eq!(granted.revision, 1);
        assert!(granted.active);
    }
    fixture.member_deny(
        "phenix.session.s3",
        "set",
        member_change("alpha", "s3", 0, true),
        ProjectMembershipProblem::Denied,
    );
    fixture.member_deny(
        "phenix.project.beta",
        "set",
        member_change("alpha", "s3", 0, true),
        ProjectMembershipProblem::Denied,
    );
    fixture.member_deny(
        "phenix.project.alpha",
        "set",
        member_change("alpha", "s1", 0, true),
        ProjectMembershipProblem::Conflict,
    );
    let records = fixture.edit("alpha", "alpha", vec![edit("theme", 0, b"blue")]);
    assert_eq!(records[0].revision, 1);
    serde_json::json!({"membership_created": true, "grant_revision": 1, "forged_admin_denied": true, "stale_cas": "conflict"})
}
fn member_check(fixture: &Fixture) -> serde_json::Value {
    for session in ["s1", "s2"] {
        let member = fixture.member_ok(
            &format!("phenix.session.{session}"),
            "get",
            member_key("alpha", session),
        );
        assert!(member.active);
        assert_eq!(member.revision, 1);
        let result = fixture.session_project_read(session, "alpha", "theme");
        let Outcome::Success(value) = result else {
            panic!("authorized session must read shared project record: {result:?}");
        };
        assert_eq!(ProjectRecord::from_value(value).unwrap().data.0, b"blue");
    }
    let denied = fixture.session_project_read("s3", "alpha", "theme");
    assert!(
        matches!(denied, Outcome::Domain(_)),
        "nonmember read: {denied:?}"
    );
    let denied = fixture.session_project_read("s1", "beta", "theme");
    assert!(
        matches!(denied, Outcome::Domain(_)),
        "foreign project read: {denied:?}"
    );
    fixture.member_deny(
        "phenix.session.s3",
        "get",
        member_key("alpha", "s1"),
        ProjectMembershipProblem::Denied,
    );
    fixture.member_deny(
        "phenix.session.s3",
        "get",
        member_key("alpha", "s3"),
        ProjectMembershipProblem::Denied,
    );
    fixture.member_deny(
        "phenix.session.s1",
        "get",
        member_key("beta", "s1"),
        ProjectMembershipProblem::Denied,
    );
    serde_json::json!({"two_sessions_authorized": true, "nonmember_denied": true, "impersonation_denied": true, "foreign_project_denied": true})
}
fn member_revoke(fixture: &Fixture) -> serde_json::Value {
    let revoked = fixture.member_ok(
        "phenix.project.alpha",
        "set",
        member_change("alpha", "s2", 1, false),
    );
    assert_eq!(revoked.revision, 2);
    assert!(!revoked.active);
    fixture.member_deny(
        "phenix.project.alpha",
        "set",
        member_change("alpha", "s2", 1, true),
        ProjectMembershipProblem::Conflict,
    );
    serde_json::json!({"revoked": true, "revision": 2, "stale_regrant_denied": true})
}
fn member_after_revoke(fixture: &Fixture) -> serde_json::Value {
    let retained = fixture.member_ok("phenix.session.s1", "get", member_key("alpha", "s1"));
    assert_eq!(retained.revision, 1);
    assert!(matches!(
        fixture.session_project_read("s1", "alpha", "theme"),
        Outcome::Success(_)
    ));
    let denied = fixture.session_project_read("s2", "alpha", "theme");
    assert!(
        matches!(denied, Outcome::Domain(_)),
        "revoked member read: {denied:?}"
    );
    fixture.member_deny(
        "phenix.session.s2",
        "get",
        member_key("alpha", "s2"),
        ProjectMembershipProblem::Denied,
    );
    fixture.member_deny(
        "phenix.session.s3",
        "get",
        member_key("alpha", "s2"),
        ProjectMembershipProblem::Denied,
    );
    serde_json::json!({"retained_member": "s1", "revoked_member_denied": "s2"})
}
/// Keep a session root admitted while a separate project-authorized root
/// revokes its membership. Its first later effect must consult live membership.
fn member_live_revoke(fixture: &Fixture) -> serde_json::Value {
    let before = fixture.session_project_read("s1", "alpha", "theme");
    let Outcome::Success(value) = before else {
        panic!("member must read project record before revocation: {before:?}");
    };
    assert_eq!(ProjectRecord::from_value(value).unwrap().data.0, b"blue");

    let policy = fixture.composition("phenix.session.s1".into(), INTERFACE, &["read"], "read");
    let kernel = Prepared::prepare(policy).unwrap().load().unwrap();
    let admitted = kernel.admit_default_root().unwrap();
    let revoked = fixture.member_ok(
        "phenix.project.alpha",
        "set",
        member_change("alpha", "s1", 1, false),
    );
    assert_eq!(revoked.revision, 2);
    assert!(!revoked.active);

    let after = admitted.invoke(&key("alpha", "theme")).unwrap();
    let Outcome::Domain(value) = after else {
        panic!("admitted root must revalidate live membership: {after:?}");
    };
    let problem = ProjectStorageError::from_value(value).unwrap().problem;
    assert_eq!(problem, ProjectStorageProblem::Denied);
    serde_json::json!({
        "pre_revocation_read": true,
        "admitted_root_denied": true,
        "new_membership_revision": revoked.revision,
    })
}

fn project_source(revision: u64, bytes: &[u8]) -> SourceRef {
    let digest = Sha256::digest(bytes);
    SourceRef {
        owner: "phenix.advanced.project-storage-sqlite".into(),
        resource: "phenix.project.alpha".into(),
        revision: format!("theme@{revision}"),
        digest_sha256: format!("{digest:x}"),
        range_start: 0,
        range_end: 0,
        scope: EvidenceScope {
            kind: EvidenceScopeKind::Project,
            id: "alpha".into(),
        },
    }
}

fn source_observed(fixture: &Fixture, session: &str, reference: SourceRef) -> SourceObservation {
    let result = fixture.source_call(session, reference);
    let Outcome::Success(value) = result else {
        panic!("expected source observation success, got {result:?}");
    };
    SourceObservation::from_value(value).unwrap()
}
fn source_denied(fixture: &Fixture, session: &str, reference: SourceRef, problem: EvidenceProblem) {
    let result = fixture.source_call(session, reference);
    let Outcome::Domain(value) = result else {
        panic!("expected source observation denial, got {result:?}");
    };
    assert_eq!(EvidenceError::from_value(value).unwrap().problem, problem);
}
fn source_current(fixture: &Fixture) -> serde_json::Value {
    let original = project_source(1, b"blue");
    for session in ["s1", "s2"] {
        let observed = source_observed(fixture, session, original.clone());
        assert_eq!(compare_source(&original, &observed), SourceMatch::Current);
    }
    source_denied(
        fixture,
        "s3",
        original.clone(),
        EvidenceProblem::ScopeDenied,
    );
    let mut malformed = original.clone();
    malformed.scope.kind = EvidenceScopeKind::Session;
    source_denied(fixture, "s1", malformed, EvidenceProblem::InvalidIdentity);
    let mut forged = original.clone();
    forged.digest_sha256 = "f".repeat(64);
    assert_eq!(
        compare_source(&forged, &source_observed(fixture, "s1", forged.clone())),
        SourceMatch::Changed
    );
    serde_json::json!({
        "both_members_current": true,
        "unapproved_session_denied": true,
        "malformed_source_denied": true,
        "forged_digest_changed": true
    })
}
fn source_changed(fixture: &Fixture) -> serde_json::Value {
    let before = project_source(1, b"blue");
    let updated = fixture.edit("alpha", "alpha", vec![edit("theme", 1, b"green")]);
    assert_eq!(updated[0].revision, 2);
    assert_eq!(
        compare_source(&before, &source_observed(fixture, "s2", before.clone())),
        SourceMatch::Changed
    );
    let current = project_source(2, b"green");
    assert_eq!(
        compare_source(&current, &source_observed(fixture, "s1", current.clone())),
        SourceMatch::Current
    );
    serde_json::json!({"stale_revision_changed": true, "new_revision_current": true})
}
fn source_after_revoke(fixture: &Fixture) -> serde_json::Value {
    let current = project_source(2, b"green");
    source_denied(fixture, "s2", current.clone(), EvidenceProblem::ScopeDenied);
    assert_eq!(
        compare_source(&current, &source_observed(fixture, "s1", current.clone())),
        SourceMatch::Current
    );
    serde_json::json!({"revoked_source_denied": true, "remaining_member_current": true})
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    assert_eq!(
        args.len(),
        6,
        "usage: phenix-p2-project-storage PLUGIN BASIC-DEFS ADV-DEFS ACTION OUTPUT"
    );
    let fixture = Fixture {
        package: args[1].clone(),
        definitions: vec![args[2].clone(), args[3].clone()],
    };
    let result = match args[4].as_str() {
        "write" => write(&fixture),
        "restart" => restart(&fixture),
        "member-grant" => member_grant(&fixture),
        "member-check" => member_check(&fixture),
        "member-revoke" => member_revoke(&fixture),
        "member-after-revoke" => member_after_revoke(&fixture),
        "member-live-revoke" => member_live_revoke(&fixture),
        "source-current" => source_current(&fixture),
        "source-changed" => source_changed(&fixture),
        "source-after-revoke" => source_after_revoke(&fixture),
        other => panic!("unknown action: {other}"),
    };
    let out = Path::new(&args[5]);
    fs::create_dir_all(out).unwrap();
    fs::write(
        out.join("project-storage.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
}
