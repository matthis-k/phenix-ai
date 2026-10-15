//! P2-2b membership contract acceptance. Written before implementing the provider.
use phenix_advanced_contracts::{
    ProjectMember, ProjectMemberChange, ProjectMemberKey, ProjectMembershipProblem,
};
use phenix_contract::Portable;

#[test]
fn membership_revision_and_stable_project_identity_round_trip() {
    let change = ProjectMemberChange {
        project: "project-uuid-7".into(),
        session: "s1".into(),
        expected: 0,
        active: true,
    };
    let decoded = ProjectMemberChange::from_value(change.to_value()).unwrap();
    assert_eq!(decoded, change);
    let member = ProjectMember {
        project: change.project.clone(),
        session: change.session.clone(),
        revision: 1,
        active: true,
    };
    assert_eq!(
        ProjectMember::from_value(member.to_value()).unwrap(),
        member
    );
    assert_ne!(change.project, change.session);
    let key = ProjectMemberKey {
        project: change.project,
        session: "s2".into(),
    };
    assert_eq!(ProjectMemberKey::from_value(key.to_value()).unwrap(), key);
    assert_ne!(
        ProjectMembershipProblem::Conflict,
        ProjectMembershipProblem::Denied
    );
}
