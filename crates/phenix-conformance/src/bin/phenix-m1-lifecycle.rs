use phenix_contract::{Authority, Code, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection},
    Outcome,
};
use std::{collections::BTreeSet, fs, path::Path};

const PROVIDER: &str = "example.lifecycle.z-provider";
const CONSUMER: &str = "example.lifecycle.a-consumer";
const RUN: &str = "example.lifecycle@1/run";
const SERVICE: &str = "example.lifecycle-service@1/echo";
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
fn events(trace: &Path) -> Vec<String> {
    fs::read_to_string(trace)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}
fn reset(trace: &Path, faults: &str) {
    let _ = fs::remove_file(trace);
    std::env::set_var("PHENIX_LIFECYCLE_TRACE", trace);
    std::env::set_var("PHENIX_LIFECYCLE_FAULTS", faults);
}
fn assert_events(trace: &Path, expected: &[&str]) -> Vec<String> {
    let observed = events(trace);
    assert_eq!(observed, expected);
    observed
}
fn fd_count() -> usize {
    fs::read_dir("/proc/self/fd").unwrap().count()
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 4, "PROVIDER CONSUMER OUTPUT");
    let output = Path::new(&args[3]);
    fs::create_dir_all(output).unwrap();
    let trace = output.join("trace");
    let s = selection(SERVICE, PROVIDER);
    let policy = Composition {
        format: "phenix.composition.v3".into(),
        definitions: vec![],
        artifacts: args[1..3].to_vec(),
        entry: selection(RUN, CONSUMER),
        slots: vec![phenix_conformance::binding(
            CONSUMER,
            &format!("{}/{}", s.interface, s.operation),
            &(s.provider),
        )],
        grants: vec![grant("root", RUN), grant(CONSUMER, SERVICE)],
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: [RUN.into(), SERVICE.into()].into(),
            resources: BTreeSet::new(),
        },
        resource: None,
    };
    let baseline_fds = fd_count();
    let full = [
        "provider:start",
        "consumer:start",
        "consumer:stop",
        "consumer:drop",
        "provider:stop",
        "provider:drop",
    ];
    reset(&trace, "");
    let prepared = Prepared::prepare(policy.clone()).unwrap();
    assert!(
        !trace.exists(),
        "offline validation executed lifecycle code"
    );
    assert_eq!(
        prepared.inspect()["lifecycle"]["startup_order"],
        serde_json::json!([PROVIDER, CONSUMER])
    );
    let mut reverse = policy.clone();
    reverse.artifacts.reverse();
    assert_eq!(
        Prepared::prepare(reverse).unwrap().inspect(),
        prepared.inspect()
    );
    let kernel = prepared.load().unwrap();
    assert_eq!(
        fd_count(),
        baseline_fds + 2,
        "fixture did not acquire two real resources"
    );
    assert_events(&trace, &full[..2]);
    let root = kernel.admit_default_root().unwrap();
    assert_eq!(
        kernel.shutdown().unwrap_err().code,
        Code::ReentrancyRejected
    );
    assert_events(&trace, &full[..2]);
    let request = Value::map([("text", Value::String("lifecycle".into()))]);
    assert_eq!(
        root.invoke(&request).unwrap(),
        Outcome::Success(Value::map([("text", Value::String("LIFECYCLE".into()))]))
    );
    drop(root);
    kernel.shutdown().unwrap();
    assert!(kernel.inspect()["lifecycle"]["stopped"].as_bool().unwrap());
    assert!(kernel.inspect()["lifecycle"]["stop_failures"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        kernel.admit_default_root().err().unwrap().code,
        Code::PluginFailed
    );
    let explicit = events(&trace);
    assert_eq!(
        explicit,
        [
            "provider:start",
            "consumer:start",
            "provider:invoke",
            "consumer:stop",
            "consumer:drop",
            "provider:stop",
            "provider:drop"
        ]
    );
    kernel.shutdown().unwrap();
    drop(kernel);
    assert_eq!(
        events(&trace),
        explicit,
        "stop repeated during shutdown/drop"
    );

    assert_eq!(
        fd_count(),
        baseline_fds,
        "explicit shutdown leaked resources"
    );
    reset(&trace, "");
    let kernel = Prepared::prepare(policy.clone()).unwrap().load().unwrap();
    drop(kernel);
    let dropped = assert_events(&trace, &full);
    assert_eq!(fd_count(), baseline_fds, "drop shutdown leaked resources");
    let mut failures = Vec::new();
    for faults in [
        "consumer:start",
        "consumer:start,consumer:stop",
        "consumer:start,consumer:stop-panic",
        "consumer:start,consumer:drop-panic",
        "consumer:start-panic",
        "provider:start",
        "provider:start-panic",
    ] {
        reset(&trace, faults);
        let error = Prepared::prepare(policy.clone())
            .unwrap()
            .load()
            .err()
            .unwrap();
        assert_eq!(error.code, Code::PluginFailed);
        let first = faults.split(',').next().unwrap();
        let expected = if first.ends_with("-panic") {
            "caught lifecycle panic"
        } else {
            first
        };
        assert_eq!(
            error.message, expected,
            "cleanup replaced the startup failure"
        );
        let observed = if faults.starts_with("provider") {
            assert_events(
                &trace,
                &["provider:start", "provider:stop", "provider:drop"],
            )
        } else {
            assert_events(&trace, &full)
        };
        assert_eq!(
            fd_count(),
            baseline_fds,
            "startup rollback leaked resources"
        );
        failures.push(serde_json::json!({"faults":faults,"failure":error,"events":observed}));
    }
    let mut stops = Vec::new();
    for faults in [
        "consumer:stop",
        "consumer:stop,provider:stop",
        "consumer:stop-panic",
        "provider:stop-panic",
        "consumer:drop-panic",
        "consumer:stop,consumer:drop-panic",
    ] {
        reset(&trace, faults);
        let kernel = Prepared::prepare(policy.clone()).unwrap().load().unwrap();
        let error = kernel.shutdown().unwrap_err();
        assert_eq!(error.code, Code::PluginFailed);
        let first = faults.split(',').next().unwrap();
        assert_eq!(
            error.message,
            if first.ends_with("-panic") {
                "caught lifecycle panic"
            } else {
                first
            }
        );
        let observed = assert_events(&trace, &full);
        let inspection = kernel.inspect();
        let reports = inspection["lifecycle"]["stop_failures"].as_array().unwrap();
        assert_eq!(
            reports.len(),
            faults
                .split(',')
                .map(|f| f.split_once(':').unwrap().0)
                .collect::<BTreeSet<_>>()
                .len()
        );
        assert_eq!(
            reports[0]["provider"],
            if faults.starts_with("consumer") {
                CONSUMER
            } else {
                PROVIDER
            }
        );
        if reports.len() == 2 {
            assert_eq!(reports[1]["provider"], PROVIDER);
        }
        assert_eq!(inspection["last_failure"]["phase"], "lifecycle");
        kernel.shutdown().unwrap();
        drop(kernel);
        assert_eq!(events(&trace), observed);
        assert_eq!(fd_count(), baseline_fds, "failed shutdown leaked resources");
        stops.push(serde_json::json!({"faults":faults,"returned":error,"inspection":inspection,"events":observed}));
    }
    fs::write(output.join("lifecycle.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "format":"phenix.m02-lifecycle-evidence.v1","sdk_version":"0.1.0","contract_version":"0.1.0",
        "full_m02_accepted":false,"fd_baseline":baseline_fds,"acquired_resource_count":2,"explicit_shutdown":explicit,"drop_shutdown":dropped,
        "startup_failures":failures,"stop_failures":stops,
        "cases":["offline-no-execution","dependency-start-order","artifact-order-determinism","native-typed-call","active-root-shutdown-rejected","reverse-explicit-stop","reverse-drop-stop","partial-start-cleanup","startup-panic-cleanup","rollback-preserves-first-failure","stop-failure-destroys-state","stop-panic-destroys-state","all-stops-attempted","shutdown-idempotent","post-shutdown-admission-rejected","recoverable-native-resource-leaks"]
    })).unwrap()).unwrap();
    fs::write(
        output.join("lifecycle-policy.json"),
        serde_json::to_vec(&policy).unwrap(),
    )
    .unwrap();
    println!("M1 synchronous lifecycle slice passed; asynchronous gate remains open");
}
