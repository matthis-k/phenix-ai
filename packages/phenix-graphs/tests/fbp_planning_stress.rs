#[path = "../benches/support/composition_cases.rs"]
mod cases;

#[test]
fn default_success_envelope_real_writes_recovery_splices_and_hierarchy() {
    for n in [16, 32] {
        cases::dense(n, false, |_| {});
        cases::dense(n, true, |_| {});
        cases::splices(n, |_| {});
        cases::hierarchical(n, |_| {});
    }
    cases::defeat(4, |_| {});
}
