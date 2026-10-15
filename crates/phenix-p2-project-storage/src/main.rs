//! P2-2a packaged project isolation and revision test using real native SQLite.
use phenix_advanced_contracts::{
    ProjectEdit, ProjectMember, ProjectMemberChange, ProjectMemberKey, ProjectMembershipError,
    ProjectMembershipProblem, ProjectRecord, ProjectRecordKey, ProjectStorageError,
    ProjectStorageProblem, ProjectTransaction,
};
use phenix_contract::{Authority, Portable, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection},
    Outcome,
};
use std::{collections::BTreeSet, fs, path::Path};

const INTERFACE: &str = "phenix.advanced.project-storage@1";
const MEMBER_INTERFACE: &str = "phenix.advanced.project-membership@1";
const PROVIDER: &str = "phenix.advanced.project-storage-sqlite";

struct Fixture {
    package: String,
    definitions: Vec<String>,
}
impl Fixture {
    fn invoke_selected(
        &self,
        resource: String,
        interface: &str,
        allowed: &[&str],
        operation: &str,
        input: Value,
    ) -> Result<Outcome, phenix_contract::Failure> {
        let grants = allowed
            .iter()
            .map(|operation| ScopedGrant {
                caller: "root".into(),
                interface: interface.into(),
                operation: (*operation).into(),
                resources: BTreeSet::from([resource.clone()]),
            })
            .collect::<Vec<_>>();
        let policy = Composition {
            format: "phenix.composition.v2".into(),
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
        };
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
    }
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
