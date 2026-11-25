//! Model-check the *actual* production wake.rs, substituting only Loom's
//! instrumented Mutex, Condvar, Arc and AtomicBool for their std equivalents.
//! Build ONLY with RUSTFLAGS="--cfg loom" inside this independent crate.
#![allow(unexpected_cfgs)]

#[cfg(not(loom))]
compile_error!("This verifier requires RUSTFLAGS='--cfg loom'");

#[cfg(loom)]
#[allow(dead_code, unexpected_cfgs)]
mod wake {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../packages/phenix-graphs/src/runtime/wake.rs"
    ));

    #[cfg(test)]
    mod loom_checks {
        use super::*;
        use loom::thread;
        use std::sync::atomic::{AtomicUsize, Ordering as CountOrdering};

        // Explicitly bounded exploration rather than a false claim of
        // exhaustiveness. The test logs how many schedules were explored.
        fn check(name: &'static str, case: impl Fn() + Send + Sync + 'static) {
            let schedules = std::sync::Arc::new(AtomicUsize::new(0));
            let progress = schedules.clone();
            let mut model = loom::model::Builder::new();
            model.max_threads = 3;
            model.max_branches = 1_000;
            model.preemption_bound = Some(2);
            model.max_permutations = Some(20_000);
            model.check(move || {
                case();
                progress.fetch_add(1, CountOrdering::Relaxed);
            });
            let explored = schedules.load(CountOrdering::Relaxed);
            assert!(explored > 1, "{name}: no meaningful schedule exploration");
            println!("{name}: explored {explored} Loom schedules (max 20000, preemptions <= 2)");
        }

        #[test]
        fn old_wake_and_retirement_cannot_leave_a_stale_target() {
            check("old notify vs retirement", || {
                const PROCESS: usize = 7;
                let signal = WakeSignal::new();
                let old = Arc::new(WakeSlot::default());

                let notifying = {
                    let signal = signal.clone();
                    let old = old.clone();
                    thread::spawn(move || signal.notify_target(&old, PROCESS, true))
                };
                let retiring = {
                    let signal = signal.clone();
                    let old = old.clone();
                    thread::spawn(move || signal.retire_target(PROCESS, &old))
                };

                let notified = notifying.join().unwrap();
                retiring.join().unwrap();
                assert!(!old.active.load(Ordering::Acquire));
                assert!(!old.pending.load(Ordering::Acquire));
                assert!(!signal.notify_target(&old, PROCESS, false));
                let (global, pending) = signal.take_scheduler_wakes();
                assert_eq!(global, 0);
                assert!(pending.is_empty());
                let state = signal.state.lock().unwrap();
                assert_eq!(state.epoch, u64::from(notified));
                assert!(state.active);
            });
        }

        #[test]
        fn fresh_wake_does_not_share_old_slot_authority_at_same_index() {
            check("retired owner vs replacement", || {
                const PROCESS: usize = 9;
                let signal = WakeSignal::new();
                let old = Arc::new(WakeSlot::default());
                let fresh = Arc::new(WakeSlot::default());

                let old_sender = {
                    let signal = signal.clone();
                    let old = old.clone();
                    thread::spawn(move || {
                        let _ = signal.notify_target(&old, PROCESS, false);
                        signal.notify_target(&old, PROCESS, true)
                    })
                };
                let replacing = {
                    let signal = signal.clone();
                    let old = old.clone();
                    let fresh = fresh.clone();
                    thread::spawn(move || {
                        signal.retire_target(PROCESS, &old);
                        assert!(signal.notify_target(&fresh, PROCESS, true));
                    })
                };

                let _last_old_wake = old_sender.join().unwrap();
                replacing.join().unwrap();
                assert!(!old.active.load(Ordering::Acquire));
                assert!(!old.pending.load(Ordering::Acquire));
                assert!(!signal.notify_target(&old, PROCESS, true));
                assert!(fresh.pending.load(Ordering::Acquire));
                let (_, targets) = signal.take_scheduler_wakes();
                assert_eq!(targets, BTreeSet::from([PROCESS]));
                let (_, again) = signal.take_scheduler_wakes();
                assert!(again.is_empty());
            });
        }

        #[test]
        fn terminal_invalidation_rejects_late_targeted_and_global_wakes() {
            check("notifications vs invalidation", || {
                const PROCESS: usize = 11;
                let signal = WakeSignal::new();
                let slot = Arc::new(WakeSlot::default());
                let notifying = {
                    let signal = signal.clone();
                    let slot = slot.clone();
                    thread::spawn(move || {
                        let _ = signal.notify_target(&slot, PROCESS, true);
                        let _ = signal.notify();
                    })
                };
                let invalidating = {
                    let signal = signal.clone();
                    thread::spawn(move || signal.invalidate())
                };
                notifying.join().unwrap();
                invalidating.join().unwrap();

                assert!(!signal.notify());
                assert!(!signal.notify_target(&slot, PROCESS, true));
                let (_, targets) = signal.take_scheduler_wakes();
                assert!(targets.is_empty());
                let state = signal.state.lock().unwrap();
                assert!(!state.active);
                assert!(state.epoch <= 2);
                assert!(state.global_epoch <= 1);
            });
        }
    }
}
