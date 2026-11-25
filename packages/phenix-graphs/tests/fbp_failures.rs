use phenix_graph::{
    CallbackPhase, Failure, FailureContext, FailureKind, TypeIdentity, Value, WrongFailureValue,
};
use std::{
    any::{TypeId, type_name},
    error::Error,
    fmt,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Debug)]
struct Leaf;
impl fmt::Display for Leaf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("leaf")
    }
}
impl Error for Leaf {}

// Deliberately not Clone, including its nested source.
#[derive(Debug)]
struct Original {
    nested: Leaf,
}
impl fmt::Display for Original {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("original")
    }
}
impl Error for Original {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.nested)
    }
}

#[test]
fn non_clone_cause_and_nested_source_keep_concrete_identity_across_cow() {
    fn assert_traits<T: Clone + Send + Sync>() {}
    assert_traits::<Failure>();
    let original = Failure::external("host summary", Original { nested: Leaf });
    let pointer = original
        .source()
        .unwrap()
        .downcast_ref::<Original>()
        .unwrap();
    let nested = pointer.source().unwrap().downcast_ref::<Leaf>().unwrap();
    let enriched = original
        .clone()
        .with_context(FailureContext {
            process: Some(7),
            path: Some("worker/input".into()),
            phase: Some(CallbackPhase::Step),
            ..Default::default()
        })
        .with_kind(FailureKind::Framework)
        .with_lease(Arc::new(()))
        .with_cleanup(Failure::application_message("cleanup"));
    for failure in [&original, &enriched, &enriched.clone()] {
        let cause = failure
            .source()
            .unwrap()
            .downcast_ref::<Original>()
            .unwrap();
        assert!(std::ptr::eq(pointer, cause));
        assert!(std::ptr::eq(
            nested,
            cause.source().unwrap().downcast_ref::<Leaf>().unwrap()
        ));
    }
    assert_eq!(original.kind(), FailureKind::External);
    assert_eq!(original.context(), &FailureContext::default());
    assert!(original.cleanup().is_empty());
    assert_eq!(enriched.kind(), FailureKind::Framework);
    assert_eq!(enriched.context().process, Some(7));
    assert_eq!(enriched.context().request, None);
    assert_eq!(enriched.context().ticket, None);
    assert_eq!(enriched.cleanup()[0].summary(), "cleanup");
    let replaced = enriched.clone().with_context(FailureContext::default());
    assert_eq!(replaced.context().process, None);
    assert_eq!(enriched.context().process, Some(7));
}

struct HostileCallbacks(Arc<AtomicUsize>);
impl HostileCallbacks {
    fn called(&self) -> ! {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("external callback must not run")
    }
}
impl fmt::Display for HostileCallbacks {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.called()
    }
}
impl fmt::Debug for HostileCallbacks {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.called()
    }
}
impl Error for HostileCallbacks {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.called()
    }
}

#[test]
fn all_host_operations_avoid_external_display_debug_and_source() {
    let calls = Arc::new(AtomicUsize::new(0));
    let failure = Failure::external("cached", HostileCallbacks(calls.clone()));
    let failure = failure
        .clone()
        .with_context(FailureContext {
            field: Some("input".into()),
            ..Default::default()
        })
        .with_cleanup(Failure::external(
            "secondary",
            HostileCallbacks(calls.clone()),
        ));
    assert_eq!(failure.summary(), "cached");
    assert_eq!(format!("{failure}"), "cached");
    assert!(format!("{failure:?}").contains("cached"));
    assert!(failure.source().unwrap().is::<HostileCallbacks>());
    let value = Value::local(failure.clone()).with_lease(Arc::new(()));
    let copied = Failure::from_value(&value).unwrap();
    assert_eq!(format!("{copied}"), "cached");
    assert!(format!("{copied:?}").contains("cached"));
    drop((copied, value, failure));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn application_message_is_a_real_string_leaf() {
    let leaf = Failure::application_message(String::from("application message"));
    assert_eq!(leaf.kind(), FailureKind::Application);
    assert_eq!(leaf.summary(), "application message");
    assert!(leaf.source().is_none());
    assert!(leaf.cleanup().is_empty());
    assert!(!leaf.dispose_catching());
}

#[test]
fn origin_survives_cloning_and_cow_but_not_independent_construction() {
    let original = Failure::application_message("same text");
    let cloned = original.clone();
    let enriched = cloned
        .clone()
        .with_context(FailureContext {
            phase: Some(CallbackPhase::Cleanup),
            ..Default::default()
        })
        .with_lease(Arc::new(()))
        .with_cleanup(Failure::application_message("secondary"));
    assert!(original.same_origin(&cloned));
    assert!(original.same_origin(&enriched));
    assert!(enriched.same_origin(&original));
    assert!(!original.same_origin(&Failure::application_message("same text")));
    assert!(original.cleanup().is_empty());
    assert_eq!(original.context(), &FailureContext::default());
}

type Events = Arc<Mutex<Vec<&'static str>>>;

struct Lease {
    name: &'static str,
    events: Events,
    panic: bool,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.events.lock().unwrap().push(self.name);
        assert!(!self.panic, "hostile lease");
    }
}

struct DropCause {
    name: &'static str,
    leases: Vec<Weak<Lease>>,
    events: Events,
    panic: bool,
}
impl fmt::Debug for DropCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DropCause")
    }
}
impl fmt::Display for DropCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("drop cause")
    }
}
impl Error for DropCause {}
impl Drop for DropCause {
    fn drop(&mut self) {
        for lease in &self.leases {
            assert!(lease.strong_count() > 0, "cause lost a lease");
        }
        self.events.lock().unwrap().push(self.name);
        if self.panic {
            std::panic::panic_any(PanicPayload {
                leases: self.leases.clone(),
                events: self.events.clone(),
            });
        }
    }
}

struct PanicPayload {
    leases: Vec<Weak<Lease>>,
    events: Events,
}
impl Drop for PanicPayload {
    fn drop(&mut self) {
        for lease in &self.leases {
            assert!(lease.strong_count() > 0, "panic payload lost a lease");
        }
        self.events.lock().unwrap().push("panic-payload");
        panic!("hostile panic payload destructor");
    }
}

fn leases(events: &Events, panic: bool) -> Vec<Arc<Lease>> {
    ["lease-first", "lease-middle", "lease-last"]
        .map(|name| {
            Arc::new(Lease {
                name,
                events: events.clone(),
                panic,
            })
        })
        .into()
}

fn cause(name: &'static str, leases: &[Arc<Lease>], events: &Events, panic: bool) -> Failure {
    Failure::external(
        "cached drop cause",
        DropCause {
            name,
            leases: leases.iter().map(Arc::downgrade).collect(),
            events: events.clone(),
            panic,
        },
    )
}

fn hostile_failure(events: &Events, cause_panic: bool, lease_panic: bool) -> Failure {
    let leases = leases(events, lease_panic);
    // These secondary handles do not escape. The parent leases must pin both.
    let mut failure = cause("primary", &leases, events, cause_panic)
        .with_cleanup(cause("secondary-first", &leases, events, cause_panic))
        .with_cleanup(cause("secondary-last", &leases, events, cause_panic));
    for lease in leases {
        failure = failure.with_lease(lease);
    }
    failure
}

fn expected_events(cause_panic: bool) -> Vec<&'static str> {
    let mut events = Vec::new();
    for name in ["primary", "secondary-first", "secondary-last"] {
        events.push(name);
        if cause_panic {
            events.push("panic-payload");
        }
    }
    events.extend(["lease-last", "lease-middle", "lease-first"]);
    events
}

#[test]
fn ordinary_drop_pins_all_leases_through_causes_secondaries_and_panicking_payloads() {
    for cause_panic in [false, true] {
        for lease_panic in [false, true] {
            let events = Events::default();
            drop(hostile_failure(&events, cause_panic, lease_panic));
            assert_eq!(*events.lock().unwrap(), expected_events(cause_panic));
        }
    }
}

#[test]
fn managed_disposal_reports_real_internal_panics_not_a_successful_outer_catch() {
    for cause_panic in [false, true] {
        for lease_panic in [false, true] {
            let events = Events::default();
            let failure = hostile_failure(&events, cause_panic, lease_panic);
            let result = failure.dispose_catching();
            assert_eq!(result, cause_panic || lease_panic);
            assert_eq!(*events.lock().unwrap(), expected_events(cause_panic));
        }
    }
    let events = Events::default();
    let failure =
        Failure::application_message("parent").with_cleanup(hostile_failure(&events, true, false));
    assert!(failure.dispose_catching());
    assert_eq!(*events.lock().unwrap(), expected_events(true));
}

#[test]
fn ordinary_drop_also_contains_cleanup_while_already_unwinding() {
    let events = Events::default();
    let result = std::panic::catch_unwind({
        let events = events.clone();
        move || {
            let _failure = hostile_failure(&events, true, true);
            panic!("outer panic");
        }
    });
    assert!(result.is_err());
    assert_eq!(*events.lock().unwrap(), expected_events(true));
}

#[test]
fn leased_cow_clones_keep_their_leases_until_the_final_cause_drop_in_both_orders() {
    for reverse in [false, true] {
        let events = Events::default();
        let leases = leases(&events, false);
        let mut first = cause("cause", &leases, &events, false);
        for lease in leases {
            first = first.with_lease(lease);
        }
        let second = first.clone().with_context(FailureContext {
            output: Some("out".into()),
            ..Default::default()
        });
        let (first, last) = if reverse {
            (second, first)
        } else {
            (first, second)
        };
        assert!(!first.dispose_catching());
        assert!(events.lock().unwrap().is_empty());
        assert!(!last.dispose_catching());
        assert_eq!(
            *events.lock().unwrap(),
            ["cause", "lease-last", "lease-middle", "lease-first"]
        );
    }
}

#[test]
fn managed_shared_record_reports_only_the_operation_that_runs_cleanup() {
    let events = Events::default();
    let failure = hostile_failure(&events, true, false);
    let clone = failure.clone();
    assert!(!failure.dispose_catching());
    assert!(events.lock().unwrap().is_empty());
    assert!(clone.dispose_catching());
    assert_eq!(*events.lock().unwrap(), expected_events(true));
}

struct ObserveLease {
    lease: Weak<Lease>,
    observations: Arc<Mutex<Vec<bool>>>,
}
impl fmt::Debug for ObserveLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ObserveLease")
    }
}
impl fmt::Display for ObserveLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("observe lease")
    }
}
impl Error for ObserveLease {}
impl Drop for ObserveLease {
    fn drop(&mut self) {
        self.observations
            .lock()
            .unwrap()
            .push(self.lease.strong_count() > 0);
    }
}

#[test]
fn lease_enrichment_is_per_handle_and_nonretroactive_in_both_drop_orders() {
    for unleased_first in [false, true] {
        let events = Events::default();
        let lease = Arc::new(Lease {
            name: "lease",
            events,
            panic: false,
        });
        let weak = Arc::downgrade(&lease);
        let observations = Arc::new(Mutex::new(Vec::new()));
        // Host-only observation, no unloaded code or unsafe operation is involved.
        let unleased = Failure::external(
            "observation",
            ObserveLease {
                lease: weak.clone(),
                observations: observations.clone(),
            },
        );
        let leased = unleased.clone().with_lease(lease);
        if unleased_first {
            drop(unleased);
            assert!(observations.lock().unwrap().is_empty());
            assert!(weak.strong_count() > 0);
            drop(leased);
        } else {
            drop(leased);
            assert!(observations.lock().unwrap().is_empty());
            assert_eq!(weak.strong_count(), 0);
            drop(unleased);
        }
        assert_eq!(*observations.lock().unwrap(), [unleased_first]);
        assert_eq!(weak.strong_count(), 0);
    }
}

#[test]
fn from_value_copies_every_lease_without_retaining_payload_or_envelope() {
    let events = Events::default();
    let leases = leases(&events, false);
    let value = Value::local(cause("cause", &leases, &events, true));
    let payload = value.shared::<Failure>().unwrap();
    let weak_payload = Arc::downgrade(&payload);
    let source = payload
        .source()
        .unwrap()
        .downcast_ref::<DropCause>()
        .unwrap() as *const _;
    drop(payload);
    let mut value = value;
    for lease in leases {
        value = value.with_lease(lease);
    }
    let failure = Failure::from_value(&value).unwrap();
    assert_eq!(
        source,
        failure
            .source()
            .unwrap()
            .downcast_ref::<DropCause>()
            .unwrap() as *const _
    );
    let copy = failure.clone().with_kind(FailureKind::Cleanup);
    drop(value);
    assert!(weak_payload.upgrade().is_none(), "retained the payload Arc");
    assert!(events.lock().unwrap().is_empty());
    drop(failure);
    assert!(events.lock().unwrap().is_empty());
    assert!(copy.dispose_catching());
    assert_eq!(
        *events.lock().unwrap(),
        [
            "cause",
            "panic-payload",
            "lease-last",
            "lease-middle",
            "lease-first"
        ]
    );
}

#[test]
fn public_lease_and_cleanup_attachment_recursively_pin_each_escaping_handle_in_either_order() {
    for lease_first in [false, true] {
        for reverse in [false, true] {
            let events = Events::default();
            let leases = leases(&events, true);
            let weak_leases: Vec<_> = leases.iter().map(Arc::downgrade).collect();
            let primary = cause("primary", &leases, &events, true).with_context(FailureContext {
                field: Some("primary".into()),
                ..Default::default()
            });
            let secondary = cause("secondary-first", &leases, &events, true)
                .with_context(FailureContext {
                    phase: Some(CallbackPhase::Cleanup),
                    ..Default::default()
                })
                .with_cleanup(cause("secondary-last", &leases, &events, true));
            let original = primary.clone().with_cleanup(secondary.clone());
            let mut enriched = primary;
            if !lease_first {
                enriched = enriched.with_cleanup(secondary.clone());
            }
            for lease in &leases {
                enriched = enriched.with_lease(lease.clone()).with_lease(lease.clone());
            }
            if lease_first {
                enriched = enriched.with_cleanup(secondary);
            } else {
                drop(secondary);
            }
            for lease in &leases {
                enriched = enriched.with_lease(lease.clone());
                // One external owner plus one lease per enriched record.
                assert_eq!(Arc::strong_count(lease), 4);
            }
            let mut before = &original;
            let mut after = &enriched;
            for depth in 0..3 {
                assert!(before.same_origin(after));
                assert!(std::ptr::eq(
                    before.source().unwrap(),
                    after.source().unwrap()
                ));
                assert_eq!(before.context(), after.context());
                assert_eq!(before.summary(), after.summary());
                assert_eq!(before.kind(), after.kind());
                if depth < 2 {
                    before = &before.cleanup()[0];
                    after = &after.cleanup()[0];
                }
            }
            let mut escaped = vec![
                enriched.cleanup()[0].clone(),
                enriched.cleanup()[0].cleanup()[0].clone(),
            ];
            drop(original);
            drop(enriched);
            drop(leases);
            assert!(weak_leases.iter().all(|lease| lease.upgrade().is_some()));
            if reverse {
                escaped.reverse();
            }
            drop(escaped.pop());
            assert!(weak_leases.iter().all(|lease| lease.upgrade().is_some()));
            assert!(escaped.pop().unwrap().dispose_catching());
            assert!(weak_leases.iter().all(|lease| lease.upgrade().is_none()));
            assert_eq!(*events.lock().unwrap(), expected_events(true));
        }
    }
}

#[test]
fn public_recursive_lease_inheritance_does_not_enrich_earlier_nested_cleanup_clones() {
    for lease_first in [false, true] {
        for earlier_first in [false, true] {
            let lease = Arc::new(Lease {
                name: "lease",
                events: Events::default(),
                panic: false,
            });
            let weak = Arc::downgrade(&lease);
            let observations = Arc::new(Mutex::new(Vec::new()));
            let secondary =
                Failure::application_message("secondary").with_cleanup(Failure::external(
                    "nested observation",
                    ObserveLease {
                        lease: weak.clone(),
                        observations: observations.clone(),
                    },
                ));
            let earlier = secondary.cleanup()[0].clone();
            let parent = Failure::application_message("parent");
            let enriched = if lease_first {
                parent.with_lease(lease).with_cleanup(secondary)
            } else {
                parent.with_cleanup(secondary).with_lease(lease)
            };
            let later = enriched.cleanup()[0].cleanup()[0].clone();
            assert!(earlier.same_origin(&later));
            assert!(std::ptr::eq(
                earlier.source().unwrap(),
                later.source().unwrap()
            ));
            drop(enriched);
            if earlier_first {
                drop(earlier);
                assert!(weak.upgrade().is_some());
                assert!(observations.lock().unwrap().is_empty());
                drop(later);
            } else {
                drop(later);
                assert!(weak.upgrade().is_none());
                assert!(observations.lock().unwrap().is_empty());
                drop(earlier);
            }
            assert_eq!(*observations.lock().unwrap(), [earlier_first]);
            assert!(weak.upgrade().is_none());
        }
    }
}

#[test]
fn from_value_recursively_leases_escaping_cleanup_without_changing_causes_or_payload_ownership() {
    for depth in 0..3 {
        for value_first in [false, true] {
            let events = Events::default();
            let leases = leases(&events, false);
            let weak_leases: Vec<_> = leases.iter().map(Arc::downgrade).collect();
            let failure = cause("primary", &leases, &events, true).with_cleanup(
                cause("secondary-first", &leases, &events, true).with_cleanup(cause(
                    "secondary-last",
                    &leases,
                    &events,
                    true,
                )),
            );
            let value = Value::local(failure);
            let payload = value.shared::<Failure>().unwrap();
            let weak_payload = Arc::downgrade(&payload);
            drop(payload);
            let mut value = value;
            for lease in leases {
                value = value.with_lease(lease);
            }
            let extracted = Failure::from_value(&value).unwrap();
            let mut original = value.borrow::<Failure>().unwrap();
            let mut enriched = &extracted;
            for level in 0..3 {
                assert!(original.same_origin(enriched));
                assert!(std::ptr::eq(
                    original
                        .source()
                        .unwrap()
                        .downcast_ref::<DropCause>()
                        .unwrap(),
                    enriched
                        .source()
                        .unwrap()
                        .downcast_ref::<DropCause>()
                        .unwrap(),
                ));
                if level < 2 {
                    original = &original.cleanup()[0];
                    enriched = &enriched.cleanup()[0];
                }
            }
            let mut escaped = &extracted;
            for _ in 0..depth {
                escaped = &escaped.cleanup()[0];
            }
            let escaped = escaped.clone();
            let cow = escaped.clone().with_kind(FailureKind::Cleanup);
            if value_first {
                drop(value);
                drop(extracted);
            } else {
                drop(extracted);
                drop(value);
            }
            assert!(weak_payload.upgrade().is_none());
            assert!(weak_leases.iter().all(|lease| lease.upgrade().is_some()));
            assert!(!escaped.dispose_catching());
            assert!(weak_leases.iter().all(|lease| lease.upgrade().is_some()));
            assert!(cow.dispose_catching());
            assert!(weak_leases.iter().all(|lease| lease.upgrade().is_none()));
            assert_eq!(*events.lock().unwrap(), expected_events(true));
        }
    }
}

struct WrongPayload(Arc<AtomicUsize>);
impl Drop for WrongPayload {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn wrong_payload_is_a_typed_fault_with_all_leases_but_no_payload_retention() {
    let events = Events::default();
    let drops = Arc::new(AtomicUsize::new(0));
    let value = Value::local(WrongPayload(drops.clone()));
    let payload = value.shared::<WrongPayload>().unwrap();
    let weak_payload = Arc::downgrade(&payload);
    drop(payload);
    let leases = leases(&events, false);
    let weak_leases: Vec<_> = leases.iter().map(Arc::downgrade).collect();
    let mut value = value;
    for lease in leases {
        value = value.with_lease(lease);
    }
    let failure = Failure::from_value(&value).unwrap_err();
    assert_eq!(failure.kind(), FailureKind::Framework);
    let typed = failure
        .source()
        .unwrap()
        .downcast_ref::<WrongFailureValue>()
        .unwrap();
    assert_eq!(
        typed.actual(),
        &TypeIdentity::Local(TypeId::of::<WrongPayload>())
    );
    assert!(typed.source().is_none());
    drop(value);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(weak_payload.upgrade().is_none());
    assert!(weak_leases.iter().all(|lease| lease.strong_count() > 0));
    assert!(events.lock().unwrap().is_empty());
    drop(failure);
    assert!(weak_leases.iter().all(|lease| lease.strong_count() == 0));
    assert_eq!(
        *events.lock().unwrap(),
        ["lease-last", "lease-middle", "lease-first"]
    );
}

#[test]
fn from_value_rejects_wire_schema_spoofs_and_other_failure_wrappers() {
    let wire = Value::from_wire_parts(type_name::<Failure>().into(), Vec::new());
    let wrong = Failure::from_value(&wire).unwrap_err();
    assert_eq!(
        wrong
            .source()
            .unwrap()
            .downcast_ref::<WrongFailureValue>()
            .unwrap()
            .actual(),
        &TypeIdentity::Wire(type_name::<Failure>().into())
    );
    let wrapped = Value::local(Arc::new(Failure::application_message("wrapped")));
    assert!(Failure::from_value(&wrapped).is_err());
    let unleased = Value::local(Failure::application_message("local"));
    let copied = Failure::from_value(&unleased).unwrap();
    assert_eq!(copied.summary(), "local");
    assert!(copied.source().is_none());
}
