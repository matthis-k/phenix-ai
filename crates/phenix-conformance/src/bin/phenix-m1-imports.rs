use phenix_contract::{Authority, Code, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection},
    Call, Outcome, Poll,
};
use std::{collections::BTreeSet, fs, path::Path};
const PARENT: &str = "example.pending-consumer@1/run";
const CHILD: &str = "example.pending@1/run";
const SERVICE: &str = "example.transform.apply@1/apply";
fn selection(key: &str, provider: &str) -> Selection {
    let (interface, operation) = key.split_once('/').unwrap();
    Selection {
        interface: interface.into(),
        operation: operation.into(),
        provider: provider.into(),
    }
}
fn grant(caller: &str, key: &str) -> ScopedGrant {
    let s = selection(key, "");
    ScopedGrant {
        caller: caller.into(),
        interface: s.interface,
        operation: s.operation,
        resources: BTreeSet::new(),
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
fn fds() -> usize {
    fs::read_dir("/proc/self/fd").unwrap().count()
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        7,
        "CONSUMER WORKER PROVIDER IMMEDIATE SECURITY OUTPUT"
    );
    let output = Path::new(&args[6]);
    fs::create_dir_all(output).unwrap();
    let policy = Composition {
        format: "phenix.composition.v2".into(),
        definitions: vec![],
        artifacts: args[1..4].to_vec(),
        entry: selection(PARENT, "example.pending-consumer"),
        slots: [
            ("example.pending-consumer", CHILD, "example.pending"),
            ("example.pending", SERVICE, "example.provider"),
            ("example.pending-consumer", SERVICE, "example.provider"),
        ]
        .into_iter()
        .map(|(caller, key, provider)| {
            let s = selection(key, provider);
            phenix_conformance::binding(
                caller,
                &format!("{}/{}", s.interface, s.operation),
                &(s.provider),
            )
        })
        .collect(),
        grants: vec![
            grant("root", PARENT),
            grant("example.pending-consumer", CHILD),
            grant("example.pending", SERVICE),
        ],
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: [PARENT.into(), CHILD.into(), SERVICE.into()].into(),
            resources: BTreeSet::new(),
        },
        resource: None,
    };
    fs::write(
        output.join("composition.json"),
        serde_json::to_vec_pretty(&policy).unwrap(),
    )
    .unwrap();
    let baseline = fds();
    let kernel = Prepared::prepare(policy.clone()).unwrap().load().unwrap();
    let root = kernel.admit_default_root().unwrap();
    for (mode, expected) in [
        ("hello", Some("HELLO")),
        ("", None),
        ("error", None),
        ("attenuated", None),
    ] {
        let t = ticket(root.begin(&request(mode)).unwrap());
        assert_eq!(fds(), baseline + 1);
        let inspection = kernel.inspect();
        assert_eq!(inspection["pending"]["physical_leases"], 2);
        assert_eq!(
            inspection["pending"]["tickets"][0]["wake_requests"], 1,
            "child wake did not reach parent"
        );
        match root.poll(&t).unwrap() {
            Poll::Ready(Ok(Outcome::Success(v))) => assert_eq!(v, request(expected.unwrap())),
            Poll::Ready(Ok(Outcome::Domain(_))) if mode.is_empty() => (),
            Poll::Ready(Err(f)) if mode == "attenuated" => assert_eq!(f.code, Code::Denied),
            Poll::Ready(Err(f)) if mode == "error" => {
                assert_eq!(f.message, "explicit worker error")
            }
            other => panic!("{mode}: {other:?}"),
        }
        root.release(&t).unwrap();
        assert_eq!(fds(), baseline);
    }
    let t = ticket(root.begin(&request("store")).unwrap());
    assert!(matches!(root.poll(&t).unwrap(), Poll::Ready(Ok(_))));
    root.release(&t).unwrap();
    let t = ticket(root.begin(&request("wrong-owner")).unwrap());
    assert!(matches!(root.poll(&t).unwrap(), Poll::Ready(Ok(_))));
    root.release(&t).unwrap();
    let t = ticket(root.begin(&request("early")).unwrap());
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 2);
    assert!(matches!(root.poll(&t).unwrap(), Poll::Ready(Err(_))));
    assert_eq!(root.release(&t).unwrap_err().code, Code::ScopeRejected);
    assert!(matches!(root.poll(&t).unwrap(), Poll::Draining));
    root.release(&t).unwrap();
    assert_eq!(fds(), baseline);
    let t = ticket(root.begin(&request("cancel-tree")).unwrap());
    assert!(!root.cancel(&t).unwrap());
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 2);
    assert!(matches!(
        root.poll(&t).unwrap(),
        Poll::Cancelled { quiescent: true }
    ));
    root.release(&t).unwrap();
    drop(root);
    kernel.shutdown().unwrap();
    let abandoned = Prepared::prepare(policy.clone()).unwrap().load().unwrap();
    let root = abandoned.admit_default_root().unwrap();
    let _t = ticket(root.begin(&request("cancel-tree")).unwrap());
    drop(root);
    assert!(abandoned.shutdown().is_err());
    assert!(abandoned.admit_default_root().is_err());
    assert_eq!(abandoned.drain_cancelled().unwrap(), 0);
    assert_eq!(fds(), baseline);
    abandoned.shutdown().unwrap();
    let mut immediate_policy = policy.clone();
    immediate_policy.artifacts[1] = args[4].clone();
    let immediate = Prepared::prepare(immediate_policy).unwrap().load().unwrap();
    let root = immediate.admit_default_root().unwrap();
    let t = ticket(root.begin(&request("immediate")).unwrap());
    assert!(
        matches!(root.poll(&t).unwrap(),Poll::Ready(Ok(Outcome::Success(v))) if v == request("IMMEDIATE"))
    );
    root.release(&t).unwrap();
    drop(root);
    immediate.shutdown().unwrap();
    let mut denied_policy = policy.clone();
    denied_policy
        .grants
        .retain(|g| g.caller != "example.pending-consumer");
    let denied = Prepared::prepare(denied_policy).unwrap().load().unwrap();
    let root = denied.admit_default_root().unwrap();
    let t = ticket(root.begin(&request("denied")).unwrap());
    assert_eq!(fds(), baseline);
    assert!(matches!(root.poll(&t).unwrap(),Poll::Ready(Err(f)) if f.code == Code::Denied));
    root.release(&t).unwrap();
    drop(root);
    denied.shutdown().unwrap();
    let mut security_policy = policy.clone();
    security_policy.artifacts[0] = args[5].clone();
    security_policy.entry.provider = "example.pending-security".into();
    for grant in &mut security_policy.grants {
        if grant.caller == "example.pending-consumer" {
            grant.caller = "example.pending-security".into();
        }
    }
    for binding in &mut security_policy.slots {
        if binding.caller == "example.pending-consumer" {
            binding.caller = "example.pending-security".into();
            if binding.slot == "TransitiveEffect" {
                binding.slot = "Service".into();
            }
        }
    }
    let mut sibling = security_policy
        .slots
        .iter()
        .find(|slot| slot.caller == "example.pending-security" && slot.slot == "Worker")
        .unwrap()
        .clone();
    sibling.slot = "Sibling".into();
    security_policy.slots.push(sibling);
    for resource in [None, Some("red")] {
        let mut security_policy = security_policy.clone();
        security_policy.resource = resource.map(str::to_owned);
        if resource.is_some() {
            security_policy.authority.resources = ["red".into(), "blue".into()].into();
            for grant in &mut security_policy.grants {
                grant.resources = ["red".into(), "blue".into()].into();
            }
        }
        let security = Prepared::prepare(security_policy).unwrap().load().unwrap();
        let root = security.admit_default_root().unwrap();
        let t = ticket(
            root.begin(&request(resource.unwrap_or("security")))
                .unwrap(),
        );
        assert_eq!(fds(), baseline + 1, "forgeries must retain the live worker");
        assert_eq!(security.inspect()["pending"]["physical_leases"], 2);
        let inspection = security.inspect();
        for child in inspection["pending"]["tickets"].as_array().unwrap() {
            assert_eq!(child["cancellation_requested"], false);
            assert_eq!(child["delivered"], false);
            assert_eq!(child["native_quiescent"], false);
        }
        assert!(
            matches!(root.poll(&t).unwrap(), Poll::Ready(Ok(Outcome::Success(v))) if v == request("HELLO"))
        );
        root.release(&t).unwrap();
        assert_eq!(fds(), baseline);
        drop(root);
        security.shutdown().unwrap();
    }
    fs::write(output.join("evidence.json"),serde_json::to_vec_pretty(&serde_json::json!({"format":"phenix.m1.imports.v1","cases":["typed completion","immediate provider","denial before worker acquisition","domain error","child failure","frozen attenuation","ancestor wake","double settlement","wrong parent without handle consumption","live-child completion rejected","native-quiescent parent not reentered","recursive cancellation","caller-drop tree draining","actual resource release","raw-ABI malformed begin zero acquisition","forged slot","forged Many index","forged operation","selected-provider mismatch","parent ticket substitution","unknown ticket","forged cancel zero effects","valid child survives forgeries","scope checked before invalid borrowed bytes","cross-thread scope rejection","read-only inherited resource in begin and poll","scoped resource switch rejected"],"inspection":kernel.inspect()})).unwrap()).unwrap();
}
