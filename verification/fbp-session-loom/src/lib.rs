//! Model-check the actual private ownership primitives, substituting only
//! Loom synchronization types. This is not a model of the whole Kernel.
//! Build ONLY with RUSTFLAGS="--cfg loom" inside this independent crate.
#![allow(unexpected_cfgs)]

#[cfg(not(loom))]
compile_error!("This verifier requires RUSTFLAGS='--cfg loom'");

#[path = "../../../packages/phenix-kernel/src/exclusive.rs"]
mod exclusive;
#[path = "../../../packages/phenix-kernel/src/producer_ownership.rs"]
mod producer_ownership;

#[cfg(test)]
mod tests {
    use crate::exclusive::ExclusiveSlot;
    use crate::producer_ownership::{
        DRIVER_OWNER, DirectDrainScope, DriverAdmissionError, DriverScope,
    };
    use loom::sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use loom::thread;
    use std::sync::atomic::{AtomicUsize as ScheduleCount, Ordering as ScheduleOrdering};

    const PREEMPTION_BOUND: usize = 2;
    const MAX_BRANCHES: usize = 1_000;
    const MAX_PERMUTATIONS: usize = 20_000;

    struct ModelReport {
        name: &'static str,
        schedules: usize,
        max_threads: usize,
        inconclusive: bool,
    }

    impl ModelReport {
        fn print(&self) {
            println!(
                "loom report property={:?} schedules={} max_threads={} preemption_bound={} max_branches={} max_permutations={} max_duration=none inconclusive={}",
                self.name,
                self.schedules,
                self.max_threads,
                PREEMPTION_BOUND,
                MAX_BRANCHES,
                MAX_PERMUTATIONS,
                self.inconclusive,
            );
        }
    }

    fn bounded_model(
        name: &'static str,
        max_threads: usize,
        model: impl Fn() + Send + Sync + 'static,
    ) {
        let schedules = std::sync::Arc::new(ScheduleCount::new(0));
        let progress = schedules.clone();
        let mut builder = loom::model::Builder::new();
        builder.max_threads = max_threads;
        builder.max_branches = MAX_BRANCHES;
        builder.preemption_bound = Some(PREEMPTION_BOUND);
        builder.max_permutations = Some(MAX_PERMUTATIONS);
        // Loom checks its permutation cap every checkpoint_interval. Checking
        // every iteration makes cap accounting precise; no duration cap can
        // silently turn this into time-sampled evidence.
        builder.checkpoint_interval = 1;
        builder.max_duration = None;
        builder.check(move || {
            model();
            progress.fetch_add(1, ScheduleOrdering::Relaxed);
        });
        let explored = schedules.load(ScheduleOrdering::Relaxed);
        // Loom 0.7.2 Builder::check has no completion-event result. It checks
        // `i >= max_permutations` before each iteration, so MAX-1 completed
        // schedules is the first potentially cap-truncated count. Treat an
        // otherwise indistinguishable natural count conservatively.
        let inconclusive = explored >= MAX_PERMUTATIONS - 1;
        let report = ModelReport {
            name,
            schedules: explored,
            max_threads,
            inconclusive,
        };
        report.print();
        if max_threads == 1 {
            assert_eq!(explored, 1, "{name}: sequential model must run once");
        } else {
            assert!(explored > 1, "{name}: no meaningful schedule exploration");
        }
        assert!(
            !inconclusive,
            "{name}: permutation cap reached; exploration is inconclusive"
        );
    }

    // Independent union oracle. The expected live-guard census is supplied by
    // test operations, never inferred from the production ownership word.
    fn assert_union(owner: &AtomicUsize, direct_live: usize, driver_live: usize) {
        let actual = owner.load(Ordering::Acquire);
        match (direct_live, driver_live) {
            (0, 0) => assert_eq!(actual, 0, "no guards must mean free"),
            (0, 1) => assert_eq!(actual, DRIVER_OWNER, "one live driver"),
            (direct, 0) if direct > 0 && direct < DRIVER_OWNER => {
                assert_eq!(actual, direct, "live direct-guard count")
            }
            _ => panic!("invalid direct/driver union: {direct_live}/{driver_live}"),
        }
    }

    #[test]
    fn competing_session_callers_mutate_one_exclusive_owner() {
        bounded_model("competing session callers", 3, || {
            let session = Arc::new(ExclusiveSlot::new(0_usize));
            let active = Arc::new(AtomicUsize::new(0));
            let successes = Arc::new(AtomicUsize::new(0));
            let mut workers = Vec::new();
            for _ in 0..2 {
                let session = session.clone();
                let active = active.clone();
                let successes = successes.clone();
                workers.push(thread::spawn(move || {
                    if let Some(mut lease) = session.try_take() {
                        assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                        *lease += 1;
                        successes.fetch_add(1, Ordering::SeqCst);
                        active.fetch_sub(1, Ordering::SeqCst);
                    }
                }));
            }
            for worker in workers {
                worker.join().unwrap();
            }
            assert_eq!(
                *session.try_take().unwrap(),
                successes.load(Ordering::SeqCst)
            );
            assert!(successes.load(Ordering::SeqCst) > 0);
        });
    }

    #[test]
    fn busy_session_caller_cannot_steal_ownership_and_can_retry_after_release() {
        bounded_model("busy session caller retry", 2, || {
            let session = Arc::new(ExclusiveSlot::new(0_usize));
            let mut owner = session.try_take().unwrap();
            let contender = session.clone();
            let contender = thread::spawn(move || assert!(contender.try_take().is_none()));
            // This independent rejected attempt gives the two callers real
            // atomic-order alternatives without changing the lease contract.
            assert!(session.try_take().is_none());
            contender.join().unwrap();
            *owner = 3;
            drop(owner);
            assert_eq!(*session.try_take().unwrap(), 3);
        });
    }

    #[test]
    fn early_lease_release_publishes_mutated_state_to_the_next_owner() {
        bounded_model("early exclusive-lease release", 1, || {
            let session = ExclusiveSlot::new(1_usize);
            {
                let mut lease = session.try_take().unwrap();
                *lease = 9;
                lease.release();
                lease.release();
            }
            assert_eq!(*session.try_take().unwrap(), 9);
        });
    }

    #[test]
    fn two_direct_scopes_remain_admitted_until_the_last_release() {
        let overlaps = std::sync::Arc::new(ScheduleCount::new(0));
        let observed_overlaps = overlaps.clone();
        bounded_model("two live direct scopes", 3, move || {
            let owner = Arc::new(AtomicUsize::new(0));
            let active_direct = Arc::new(AtomicUsize::new(0));
            let mut workers = Vec::new();
            for _ in 0..2 {
                let owner = owner.clone();
                let active_direct = active_direct.clone();
                let observed_overlaps = observed_overlaps.clone();
                workers.push(thread::spawn(move || {
                    let scope = DirectDrainScope::enter(&owner)
                        .expect("direct scope must coexist with another direct scope");
                    let previous = active_direct.fetch_add(1, Ordering::SeqCst);
                    assert!(previous < 2);
                    if previous == 1 {
                        observed_overlaps.fetch_add(1, ScheduleOrdering::Relaxed);
                    }
                    // This actor's own guard proves at least one direct owner;
                    // the independent two-actor census caps the legal union.
                    assert!((1..=2).contains(&owner.load(Ordering::Acquire)));
                    thread::yield_now();
                    assert!(active_direct.load(Ordering::SeqCst) >= 1);
                    assert!((1..=2).contains(&owner.load(Ordering::Acquire)));
                    active_direct.fetch_sub(1, Ordering::SeqCst);
                    drop(scope);
                }));
            }
            for worker in workers {
                worker.join().unwrap();
            }
            assert_eq!(active_direct.load(Ordering::SeqCst), 0);
            assert_union(&owner, 0, 0);
        });
        assert!(
            overlaps.load(ScheduleOrdering::Relaxed) > 0,
            "no schedule retained two simultaneous direct guards"
        );
    }

    #[test]
    fn two_driver_contenders_cover_rejection_then_one_and_two_sequential_successes() {
        bounded_model("two driver contenders: zero, one, two successes", 3, || {
            let owner = Arc::new(AtomicUsize::new(0));
            let active_driver = Arc::new(AtomicUsize::new(0));
            let successes = Arc::new(AtomicUsize::new(0));
            let rejected = Arc::new(AtomicUsize::new(0));

            let first = DriverScope::enter(&owner).expect("initial driver admission");
            active_driver.fetch_add(1, Ordering::SeqCst);
            assert_union(&owner, 0, 1);
            let mut workers = Vec::new();
            for _ in 0..2 {
                let owner = owner.clone();
                let rejected = rejected.clone();
                workers.push(thread::spawn(move || {
                    assert!(matches!(
                        DriverScope::enter(&owner),
                        Err(DriverAdmissionError::AlreadyRunning)
                    ));
                    rejected.fetch_add(1, Ordering::SeqCst);
                    assert_union(&owner, 0, 1);
                }));
            }
            for worker in workers {
                worker.join().unwrap();
            }
            assert_eq!(rejected.load(Ordering::SeqCst), 2);
            assert_eq!(successes.load(Ordering::SeqCst), 0);
            assert_union(&owner, 0, 1);
            active_driver.fetch_sub(1, Ordering::SeqCst);
            drop(first);
            assert_union(&owner, 0, 0);

            // Reuse the main actor for bounded sequential retries rather than
            // manufacturing extra Loom threads beyond the declared two-worker
            // model. Both one- and two-success outcomes follow the zero-success
            // contention frame above.
            for expected_successes in 1..=2 {
                successes.store(0, Ordering::SeqCst);
                for _ in 0..expected_successes {
                    let driver = DriverScope::enter(&owner).expect("sequential driver retry");
                    assert_eq!(active_driver.fetch_add(1, Ordering::SeqCst), 0);
                    assert_union(&owner, 0, 1);
                    successes.fetch_add(1, Ordering::SeqCst);
                    active_driver.fetch_sub(1, Ordering::SeqCst);
                    drop(driver);
                    assert_eq!(active_driver.load(Ordering::SeqCst), 0);
                    assert_union(&owner, 0, 0);
                }
                assert_eq!(successes.load(Ordering::SeqCst), expected_successes);
            }
        });
    }

    #[test]
    fn direct_and_driver_active_modes_are_mutually_exclusive() {
        bounded_model("direct and driver active-mode gauges", 3, || {
            let owner = Arc::new(AtomicUsize::new(0));
            let active_direct = Arc::new(AtomicUsize::new(0));
            let active_driver = Arc::new(AtomicUsize::new(0));

            let direct = {
                let owner = owner.clone();
                let active_direct = active_direct.clone();
                let active_driver = active_driver.clone();
                thread::spawn(move || {
                    let Some(scope) = DirectDrainScope::enter(&owner) else {
                        return false;
                    };
                    assert_eq!(active_driver.load(Ordering::SeqCst), 0);
                    assert_eq!(active_direct.fetch_add(1, Ordering::SeqCst), 0);
                    assert_union(&owner, 1, 0);
                    thread::yield_now();
                    assert_eq!(active_driver.load(Ordering::SeqCst), 0);
                    assert_union(&owner, 1, 0);
                    active_direct.fetch_sub(1, Ordering::SeqCst);
                    drop(scope);
                    true
                })
            };
            let driver = {
                let owner = owner.clone();
                let active_direct = active_direct.clone();
                let active_driver = active_driver.clone();
                thread::spawn(move || {
                    let scope = match DriverScope::enter(&owner) {
                        Ok(scope) => scope,
                        Err(DriverAdmissionError::DrainInProgress) => return false,
                        Err(DriverAdmissionError::AlreadyRunning) => {
                            panic!("the sole driver actor cannot conflict with another driver")
                        }
                    };
                    assert_eq!(active_direct.load(Ordering::SeqCst), 0);
                    assert_eq!(active_driver.fetch_add(1, Ordering::SeqCst), 0);
                    assert_union(&owner, 0, 1);
                    thread::yield_now();
                    assert_eq!(active_direct.load(Ordering::SeqCst), 0);
                    assert_union(&owner, 0, 1);
                    active_driver.fetch_sub(1, Ordering::SeqCst);
                    drop(scope);
                    true
                })
            };

            let direct_succeeded = direct.join().unwrap();
            let driver_succeeded = driver.join().unwrap();
            assert!(
                direct_succeeded || driver_succeeded,
                "one of two contenders from free must enter"
            );
            assert_eq!(active_direct.load(Ordering::SeqCst), 0);
            assert_eq!(active_driver.load(Ordering::SeqCst), 0);
            assert_union(&owner, 0, 0);
        });
    }

    #[test]
    fn gate_queued_direct_scope_has_no_free_window() {
        bounded_model("gate-queued direct scope retains the union", 2, || {
            let owner = Arc::new(AtomicUsize::new(0));
            let active_direct = Arc::new(AtomicUsize::new(0));
            let gate = Arc::new(Mutex::new(()));
            let admitted = Arc::new((Mutex::new(false), Condvar::new()));

            let first = DirectDrainScope::enter(&owner).expect("first direct admission");
            active_direct.fetch_add(1, Ordering::SeqCst);
            let first_gate = gate.lock().unwrap();
            let queued = {
                let owner = owner.clone();
                let active_direct = active_direct.clone();
                let gate = gate.clone();
                let admitted = admitted.clone();
                thread::spawn(move || {
                    let second = DirectDrainScope::enter(&owner)
                        .expect("second direct scope must be admitted before the gate");
                    active_direct.fetch_add(1, Ordering::SeqCst);
                    let (lock, changed) = &*admitted;
                    let mut is_admitted = lock.lock().unwrap();
                    *is_admitted = true;
                    changed.notify_one();
                    drop(is_admitted);

                    let _gate = gate.lock().unwrap();
                    assert_eq!(active_direct.load(Ordering::SeqCst), 1);
                    assert_union(&owner, 1, 0);
                    active_direct.fetch_sub(1, Ordering::SeqCst);
                    drop(second);
                })
            };

            let (lock, changed) = &*admitted;
            let mut is_admitted = lock.lock().unwrap();
            while !*is_admitted {
                is_admitted = changed.wait(is_admitted).unwrap();
            }
            drop(is_admitted);
            assert_eq!(active_direct.load(Ordering::SeqCst), 2);
            assert_union(&owner, 2, 0);
            assert!(matches!(
                DriverScope::enter(&owner),
                Err(DriverAdmissionError::DrainInProgress)
            ));
            assert_union(&owner, 2, 0);

            // The first active callback ends, but its queued successor still
            // owns the direct union while waiting for the callback gate.
            active_direct.fetch_sub(1, Ordering::SeqCst);
            drop(first);
            assert_union(&owner, 1, 0);
            assert!(matches!(
                DriverScope::enter(&owner),
                Err(DriverAdmissionError::DrainInProgress)
            ));
            assert_union(&owner, 1, 0);
            drop(first_gate);
            queued.join().unwrap();
            assert_eq!(active_direct.load(Ordering::SeqCst), 0);
            assert_union(&owner, 0, 0);
        });
    }

    #[test]
    fn rejected_entries_never_release_the_live_owner() {
        bounded_model("rejected entries preserve live guards", 1, || {
            let owner = AtomicUsize::new(0);
            let driver = DriverScope::enter(&owner).expect("initial driver");
            for _ in 0..2 {
                assert!(matches!(
                    DriverScope::enter(&owner),
                    Err(DriverAdmissionError::AlreadyRunning)
                ));
                assert!(DirectDrainScope::enter(&owner).is_none());
                assert_union(&owner, 0, 1);
            }
            drop(driver);
            assert_union(&owner, 0, 0);

            let first = DirectDrainScope::enter(&owner).expect("first direct scope");
            assert!(matches!(
                DriverScope::enter(&owner),
                Err(DriverAdmissionError::DrainInProgress)
            ));
            assert_union(&owner, 1, 0);
            let second = DirectDrainScope::enter(&owner).expect("second direct scope");
            assert!(matches!(
                DriverScope::enter(&owner),
                Err(DriverAdmissionError::DrainInProgress)
            ));
            assert_union(&owner, 2, 0);
            drop(second);
            assert_union(&owner, 1, 0);
            drop(first);
            assert_union(&owner, 0, 0);

            let retry = DriverScope::enter(&owner).expect("driver retry after direct scopes");
            assert_union(&owner, 0, 1);
            drop(retry);
            assert_union(&owner, 0, 0);
        });
    }

    #[test]
    fn caught_unwind_restores_direct_and_driver_ownership() {
        bounded_model("caught unwind restores owner RAII", 1, || {
            let owner = AtomicUsize::new(0);
            let direct_panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _first = DirectDrainScope::enter(&owner).expect("first direct scope");
                let _second = DirectDrainScope::enter(&owner).expect("second direct scope");
                assert_union(&owner, 2, 0);
                panic!("drop two direct scopes through unwind");
            }));
            assert!(direct_panic.is_err());
            assert_union(&owner, 0, 0);

            let driver_panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _driver = DriverScope::enter(&owner).expect("driver after direct unwind");
                assert_union(&owner, 0, 1);
                panic!("drop driver scope through unwind");
            }));
            assert!(driver_panic.is_err());
            assert_union(&owner, 0, 0);

            drop(DirectDrainScope::enter(&owner).expect("direct retry"));
            drop(DriverScope::enter(&owner).expect("driver retry"));
            assert_union(&owner, 0, 0);
        });
    }

    #[test]
    fn direct_count_boundary_never_aliases_or_overflows_the_driver_sentinel() {
        bounded_model("defensive direct-count numeric boundary", 1, || {
            // Deliberate test-only initialization: AtomicUsize::new is valid in
            // both std and Loom. No unsafe code or production setter is needed.
            let at_limit = AtomicUsize::new(DRIVER_OWNER - 1);
            assert!(DirectDrainScope::enter(&at_limit).is_none());
            assert_eq!(at_limit.load(Ordering::Acquire), DRIVER_OWNER - 1);
            assert!(matches!(
                DriverScope::enter(&at_limit),
                Err(DriverAdmissionError::DrainInProgress)
            ));
            assert_eq!(at_limit.load(Ordering::Acquire), DRIVER_OWNER - 1);

            let driver = AtomicUsize::new(DRIVER_OWNER);
            assert!(DirectDrainScope::enter(&driver).is_none());
            assert_eq!(driver.load(Ordering::Acquire), DRIVER_OWNER);
            assert!(matches!(
                DriverScope::enter(&driver),
                Err(DriverAdmissionError::AlreadyRunning)
            ));
            assert_eq!(driver.load(Ordering::Acquire), DRIVER_OWNER);

            let next_to_limit = AtomicUsize::new(DRIVER_OWNER - 2);
            let scope = DirectDrainScope::enter(&next_to_limit)
                .expect("the last representable direct count is admissible");
            assert_eq!(next_to_limit.load(Ordering::Acquire), DRIVER_OWNER - 1);
            assert!(DirectDrainScope::enter(&next_to_limit).is_none());
            assert!(matches!(
                DriverScope::enter(&next_to_limit),
                Err(DriverAdmissionError::DrainInProgress)
            ));
            drop(scope);
            assert_eq!(next_to_limit.load(Ordering::Acquire), DRIVER_OWNER - 2);
        });
    }
}
