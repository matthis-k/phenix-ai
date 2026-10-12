use phenix_contract::{Authority, Code, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection},
    Call, Outcome, Poll,
};
use std::{collections::BTreeSet, fs, path::Path};
const RUN: &str = "example.pending@1/run";
const COUNT: &str = "example.transform.count@1/count";
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
fn fds() -> usize {
    fs::read_dir("/proc/self/fd").unwrap().count()
}
fn ticket(call: Call) -> phenix_kernel::Ticket {
    match call {
        Call::Pending(ticket) => ticket,
        other => panic!("expected pending: {other:?}"),
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.len() == 4 || args.len() == 5,
        "PENDING PROVIDER OUTPUT [drop-live]"
    );
    let output = Path::new(&args[3]);
    fs::create_dir_all(output).unwrap();
    let s = selection(SERVICE, "example.provider");
    let policy = Composition {
        format: "phenix.composition.v2".into(),
        definitions: vec![],
        artifacts: args[1..3].to_vec(),
        entry: selection(RUN, "example.pending"),
        slots: vec![phenix_conformance::binding(
            "example.pending",
            &format!("{}/{}", s.interface, s.operation),
            &(s.provider),
        )],
        grants: vec![
            grant("root", RUN),
            grant("root", COUNT),
            grant("example.pending", SERVICE),
        ],
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: [RUN.into(), SERVICE.into(), COUNT.into()].into(),
            resources: BTreeSet::new(),
        },
        resource: None,
    };
    let trace = output.join("start-trace");
    let _ = fs::remove_file(&trace);
    std::env::set_var("PHENIX_PENDING_TRACE", &trace);
    let prepared = Prepared::prepare(policy.clone()).unwrap();
    assert!(
        !trace.exists(),
        "offline preparation executed pending plugin"
    );
    fs::write(
        output.join("composition.json"),
        serde_json::to_vec_pretty(&policy).unwrap(),
    )
    .unwrap();
    if args.len() == 5 {
        let kernel = prepared.load().unwrap();
        let root = kernel.admit_default_root().unwrap();
        let _ticket = ticket(root.begin(&request("stubborn")).unwrap());
        drop(root);
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
        let before = fs::read_to_string(&trace).unwrap();
        let acquired = fds();
        drop(kernel);
        assert_eq!(
            fs::read_to_string(&trace).unwrap(),
            before,
            "drop stopped a live instance"
        );
        assert_eq!(
            fds(),
            acquired,
            "drop released the worker's actual resource"
        );
        let artifact = Path::new(&args[1]).join("lib/plugin.so");
        assert!(
            fs::read_to_string("/proc/self/maps")
                .unwrap()
                .contains(artifact.to_str().unwrap()),
            "drop unloaded live worker code"
        );
        fs::write(
            output.join("retained-on-drop.json"),
            b"{\"live_image_retained\":true}",
        )
        .unwrap();
        return;
    }
    let baseline = fds();
    let kernel = prepared.load().unwrap();
    let root = kernel.admit_default_root().unwrap();
    // The synchronous API rejects pending providers before worker acquisition.
    assert_eq!(
        root.invoke(&request("sync")).unwrap_err().code,
        Code::AbiIncompatible
    );
    assert_eq!(fds(), baseline);
    let call = ticket(root.begin(&request("ready")).unwrap());
    assert_eq!(fds(), baseline + 1);
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
    assert_eq!(
        kernel.inspect()["pending"]["tickets"][0]["wake_requests"],
        1
    );
    assert_eq!(
        root.begin(&request("second")).unwrap_err().code,
        Code::ReentrancyRejected
    );
    // A different selected kernel/root cannot poll another kernel's handle.
    let other = Prepared::prepare(policy.clone()).unwrap().load().unwrap();
    let other_root = other.admit_default_root().unwrap();
    assert_eq!(
        other_root.poll(&call).unwrap_err().code,
        Code::ScopeRejected
    );
    drop(other_root);
    other.shutdown().unwrap();
    match root.poll(&call).unwrap() {
        Poll::Ready(Ok(Outcome::Success(value))) => assert_eq!(value, request("READY")),
        other => panic!("{other:?}"),
    }
    assert_eq!(fds(), baseline);
    assert_eq!(root.poll(&call).unwrap_err().code, Code::ScopeRejected);
    assert_eq!(root.cancel(&call).unwrap_err().code, Code::ScopeRejected);
    root.release(&call).unwrap();
    assert_eq!(root.poll(&call).unwrap_err().code, Code::ScopeRejected);
    let late = ticket(root.begin(&request("late")).unwrap());
    assert!(matches!(
        root.poll(&late).unwrap(),
        Poll::Ready(Ok(Outcome::Success(_)))
    ));
    root.release(&late).unwrap();
    let domain = ticket(root.begin(&request("")).unwrap());
    assert!(
        matches!(root.poll(&domain).unwrap(), Poll::Ready(Ok(Outcome::Domain(Value::String(ref error)))) if error == "Empty")
    );
    root.release(&domain).unwrap();
    let cancelled = ticket(root.begin(&request("cancel")).unwrap());
    assert!(root.cancel(&cancelled).unwrap());
    assert_eq!(
        root.cancel(&cancelled).unwrap_err().code,
        Code::ScopeRejected
    );
    assert!(matches!(
        root.poll(&cancelled).unwrap(),
        Poll::Cancelled { quiescent: true }
    ));
    root.release(&cancelled).unwrap();
    assert_eq!(fds(), baseline);
    let error = ticket(root.begin(&request("error")).unwrap());
    match root.poll(&error).unwrap() {
        Poll::Ready(Err(failure)) => assert_eq!(failure.message, "explicit worker error"),
        other => panic!("{other:?}"),
    }
    root.release(&error).unwrap();
    let stubborn = ticket(root.begin(&request("stubborn")).unwrap());
    assert!(!root.cancel(&stubborn).unwrap());
    assert_eq!(
        root.release(&stubborn).unwrap_err().code,
        Code::ScopeRejected
    );
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
    assert!(matches!(
        root.poll(&stubborn).unwrap(),
        Poll::Cancelled { quiescent: false }
    ));
    assert_eq!(fds(), baseline + 1);
    assert!(matches!(
        root.poll(&stubborn).unwrap(),
        Poll::Cancelled { quiescent: true }
    ));
    root.release(&stubborn).unwrap();
    assert_eq!(fds(), baseline);
    // Panic selects Failed once but cannot claim that its worker stopped.
    let panicked = ticket(root.begin(&request("panic")).unwrap());
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
    assert!(root.cancel(&panicked).unwrap());
    assert!(matches!(root.poll(&panicked).unwrap(), Poll::Ready(Err(_))));
    root.release(&panicked).unwrap();
    drop(root);
    let counter = kernel
        .admit_root(selection(COUNT, "example.provider"))
        .unwrap();
    assert_eq!(
        counter
            .invoke(&Value::map([("count", Value::U64(0))]))
            .unwrap(),
        Outcome::Success(Value::map([("count", Value::U64(1))]))
    );
    drop(counter);
    kernel.shutdown().unwrap();
    // Drop initiates cancellation. Physical work pins the root until draining.
    let draining = Prepared::prepare(policy.clone()).unwrap().load().unwrap();
    let root = draining.admit_default_root().unwrap();
    let _ticket = ticket(root.begin(&request("stubborn")).unwrap());
    drop(root);
    assert_eq!(draining.inspect()["pending"]["physical_leases"], 1);
    assert!(draining.shutdown().is_err());
    assert!(draining.admit_default_root().is_err());
    assert_eq!(draining.drain_cancelled().unwrap(), 1);
    assert_eq!(draining.drain_cancelled().unwrap(), 0);
    assert_eq!(fds(), baseline);
    draining.shutdown().unwrap();
    let mut denied_policy = policy.clone();
    denied_policy
        .grants
        .retain(|g| g.caller != "example.pending");
    let denied = Prepared::prepare(denied_policy).unwrap().load().unwrap();
    let root = denied.admit_default_root().unwrap();
    let call = ticket(root.begin(&request("denied")).unwrap());
    assert!(matches!(
        root.poll(&call).unwrap(),
        Poll::Ready(Err(phenix_contract::Failure {
            code: Code::Denied,
            ..
        }))
    ));
    root.release(&call).unwrap();
    drop(root);
    denied.shutdown().unwrap();
    let child = std::process::Command::new(&args[0])
        .args(&args[1..4])
        .arg("drop-live")
        .status()
        .unwrap();
    assert!(child.success());
    let cases = [
        "sync-rejects-before-effect",
        "worker-wake",
        "native-typed-import",
        "busy-instance",
        "wrong-owner",
        "once-only-result",
        "release-rejects-poll",
        "late-wake-rejected",
        "cooperative-cancel",
        "repeat-cancel-rejected",
        "typed-worker-error",
        "declared-domain-error",
        "denied-effects-counter",
        "cancel-before-effect",
        "noncooperative-lease",
        "drain-after-cancel",
        "panic-retains-lease",
        "caller-drop-pins-root",
        "drain-releases-resources",
        "poll-authority-denied",
        "kernel-drop-retains-image",
    ];
    fs::write(output.join("evidence.json"), serde_json::to_vec_pretty(&serde_json::json!({"format":"phenix.m1.pending.evidence.v1","passed":cases,"baseline_fds":baseline,"inspection":draining.inspect(),"full_m02_accepted":false})).unwrap()).unwrap();
    println!("M1 pending native conformance passed");
}
