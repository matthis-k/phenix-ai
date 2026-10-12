use phenix_contract::{Authority, Code, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection},
    Call, Generations, Outcome, Poll, Retirement, Root,
};
use std::{collections::BTreeSet, fs, path::Path};
fn selection() -> Selection {
    Selection {
        interface: "example.revision@1".into(),
        operation: "run".into(),
        provider: "example.revision".into(),
    }
}
fn policy(path: &str) -> Composition {
    Composition {
        format: "phenix.composition.v2".into(),
        definitions:vec![],
        artifacts: vec![path.into()],
        entry: selection(),
        slots: vec![],
        grants: vec![ScopedGrant {
            caller: "root".into(),
            interface: selection().interface,
            operation: "run".into(),
            resources: BTreeSet::new(),
        }],
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: [selection().key()].into(),
            resources: BTreeSet::new(),
        },
        resource: None,
    }
}
fn request(text: &str) -> Value {
    Value::map([("text", Value::String(text.into()))])
}
fn ticket(call: Call) -> phenix_kernel::Ticket {
    match call {
        Call::Pending(t) => t,
        other => panic!("{other:?}"),
    }
}
fn complete(root: &Root<'_>, text: &str, expected: &str) {
    let t = ticket(root.begin(&request(text)).unwrap());
    match root.poll(&t).unwrap() {
        Poll::Ready(Ok(Outcome::Success(v))) => assert_eq!(v, request(expected)),
        other => panic!("{other:?}"),
    };
    root.release(&t).unwrap();
}
fn fds() -> usize {
    fs::read_dir("/proc/self/fd").unwrap().count()
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 5, "REVISION_A REVISION_B IMPORTS OUTPUT");
    let output = Path::new(&args[4]);
    fs::create_dir_all(output).unwrap();
    let a_policy = policy(&args[1]);
    let b_policy = policy(&args[2]);
    let baseline = fds();
    let trace = output.join("offline-trace");
    let _ = fs::remove_file(&trace);
    std::env::set_var("PHENIX_REVISION_LOAD_TRACE", &trace);
    let mut owner = Generations::default();
    let a = owner
        .install_candidate(
            Prepared::prepare(a_policy.clone()).unwrap(),
            "local.test",
        )
        .unwrap();
    let b = owner
        .install_candidate(
            Prepared::prepare(b_policy.clone()).unwrap(),
            "local.test",
        )
        .unwrap();
    assert_ne!(a, b);
    assert!(!trace.exists(), "candidate preparation loaded code");
    assert!(owner.admit_default_root().is_err());
    assert!(owner.admit_root_on(&a, selection()).is_err());
    let ai = owner.inspect_candidate(&a).unwrap();
    let bi = owner.inspect_candidate(&b).unwrap();
    assert_eq!(
        ai["canonical"]["artifacts"][0][2], bi["canonical"]["artifacts"][0][2],
        "revision descriptors differ"
    );
    assert_ne!(
        ai["canonical"]["artifacts"][0][1], bi["canonical"]["artifacts"][0][1],
        "revision executable digests equal"
    );
    assert_eq!(
        a,
        Prepared::prepare(a_policy.clone())
            .unwrap()
            .generation_identity("local.test")
            .unwrap()
    );
    assert_ne!(
        a,
        Prepared::prepare(a_policy.clone())
            .unwrap()
            .generation_identity("other.environment")
            .unwrap()
    );
    assert!(Prepared::prepare(a_policy.clone())
        .unwrap()
        .generation_identity("invalid identity")
        .is_err());
    let mut reordered = a_policy.clone();
    reordered.artifacts.reverse();
    reordered.grants.reverse();
    assert_eq!(
        a,
        Prepared::prepare(reordered)
            .unwrap()
            .generation_identity("local.test")
            .unwrap()
    );
    owner.promote(&a).unwrap();
    let old = owner.admit_default_root().unwrap();
    assert_eq!(old.generation_identity(), Some(a.as_str()));
    let pending = ticket(old.begin(&request("pinned")).unwrap());
    assert_eq!(fds(), baseline + 1);
    owner.trial(&b).unwrap();
    assert_eq!(owner.default_identity(), Some(&a));
    let trial = owner.admit_root_on(&b, selection()).unwrap();
    complete(&trial, "trial", "B:trial");
    drop(trial);
    owner.promote(&b).unwrap();
    assert_eq!(owner.default_identity(), Some(&b));
    assert!(matches!(
        owner.retire(&a).unwrap(),
        Retirement::Blocked { .. }
    ));
    let current = owner.admit_default_root().unwrap();
    complete(&current, "new", "B:new");
    drop(current);
    assert!(
        matches!(old.poll(&pending).unwrap(),Poll::Ready(Ok(Outcome::Success(v))) if v==request("A:pinned"))
    );
    old.release(&pending).unwrap();
    complete(&old, "still-old", "A:still-old");
    assert!(
        matches!(owner.retire(&a).unwrap(), Retirement::Blocked { .. }),
        "live root lost its pin after completing a call"
    );
    let concurrent = owner.inspect();
    fs::write(
        output.join("concurrent.json"),
        serde_json::to_vec_pretty(&concurrent).unwrap(),
    )
    .unwrap();
    drop(old);
    owner.promote(&a).unwrap();
    let rollback = owner.admit_default_root().unwrap();
    complete(&rollback, "rollback", "A:rollback");
    drop(rollback);
    let failed = owner
        .install_candidate(
            Prepared::prepare(b_policy.clone()).unwrap(),
            "local.failed-start",
        )
        .unwrap();
    std::env::set_var("PHENIX_REVISION_START_FAIL", "B");
    assert_eq!(owner.promote(&failed).unwrap_err().code, Code::PluginFailed);
    std::env::remove_var("PHENIX_REVISION_START_FAIL");
    assert_eq!(owner.default_identity(), Some(&a));
    assert_eq!(owner.inspect_candidate(&failed).unwrap()["state"], "failed");
    assert!(owner.promote(&failed).is_err());
    let surviving = owner.admit_default_root().unwrap();
    complete(&surviving, "survives", "A:survives");
    drop(surviving);
    assert!(matches!(
        owner.retire(&failed).unwrap(),
        Retirement::Retired
    ));
    let mut invalid = b_policy.clone();
    invalid.grants.clear();
    assert_eq!(
        Prepared::prepare(invalid).err().unwrap().code,
        Code::Denied
    );
    assert_eq!(owner.default_identity(), Some(&a));
    owner.promote(&b).unwrap();
    let draining = owner.admit_root_on(&a, selection()).unwrap();
    let _t = ticket(draining.begin(&request("stubborn")).unwrap());
    drop(draining);
    assert!(matches!(
        owner.retire(&a).unwrap(),
        Retirement::Blocked { .. }
    ));
    assert_eq!(fds(), baseline + 1);
    assert_eq!(owner.drain_cancelled(&a).unwrap(), 1);
    assert!(matches!(
        owner.retire(&a).unwrap(),
        Retirement::Blocked { .. }
    ));
    assert_eq!(owner.drain_cancelled(&a).unwrap(), 0);
    assert_eq!(fds(), baseline);
    let late = owner.admit_root_on(&a, selection()).unwrap();
    complete(&late, "late", "late-rejected");
    drop(late);
    assert!(matches!(owner.retire(&a).unwrap(), Retirement::Retired));
    assert!(owner.admit_root_on(&a, selection()).is_err());
    assert!(matches!(
        owner.retire(&b).unwrap(),
        Retirement::Blocked { .. }
    ));
    let tree_policy: Composition =
        serde_json::from_slice(&fs::read(Path::new(&args[3]).join("composition.json")).unwrap())
            .unwrap();
    let tree_entry = tree_policy.entry.clone();
    let tree = owner
        .install_candidate(
            Prepared::prepare(tree_policy).unwrap(),
            "local.pending-tree",
        )
        .unwrap();
    owner.trial(&tree).unwrap();
    let parent = owner.admit_root_on(&tree, tree_entry).unwrap();
    let _ticket = ticket(parent.begin(&request("cancel-tree")).unwrap());
    drop(parent);
    let blocked = owner.inspect_candidate(&tree).unwrap();
    assert_eq!(blocked["pins"]["physical_leases"], 2);
    assert!(matches!(
        owner.retire(&tree).unwrap(),
        Retirement::Blocked { .. }
    ));
    assert_eq!(owner.drain_cancelled(&tree).unwrap(), 0);
    assert!(matches!(owner.retire(&tree).unwrap(), Retirement::Retired));
    assert_eq!(fds(), baseline);
    let report = owner.inspect();
    drop(owner);
    let mut dropped = Generations::default();
    let id = dropped
        .install_candidate(
            Prepared::prepare(a_policy).unwrap(),
            "local.owner-drop",
        )
        .unwrap();
    dropped.promote(&id).unwrap();
    let root = dropped.admit_default_root().unwrap();
    let t = ticket(root.begin(&request("owner-drop")).unwrap());
    drop(dropped);
    assert!(
        matches!(root.poll(&t).unwrap(),Poll::Ready(Ok(Outcome::Success(v))) if v==request("A:owner-drop"))
    );
    root.release(&t).unwrap();
    drop(root);
    assert_eq!(fds(), baseline);
    fs::write(output.join("evidence.json"),serde_json::to_vec_pretty(&serde_json::json!({"format":"phenix.m03.generations.v1","revision_a":ai,"revision_b":bi,"cases":["offline candidate","equal descriptors different executable identities","environment identity","explicit trial","concurrent revisions","old root across promotion","new root uses promoted revision","rollback","failed promotion preserves default","invalid policy","active root retirement blocker","abandoned physical lease blocker","stale wake rejection","real resource release","root survives owner drop","generation retains imported child tree"],"inspection":report})).unwrap()).unwrap();
}
