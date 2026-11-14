#[path = "../../../packages/phenix-kernel/src/exclusive.rs"]
mod exclusive;

#[cfg(test)]
mod tests {
    use crate::exclusive::ExclusiveSlot;
    use loom::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use loom::thread;

    fn bounded_model(model: impl Fn() + Send + Sync + 'static) {
        let mut builder = loom::model::Builder::new();
        builder.preemption_bound = Some(2);
        builder.max_permutations = Some(20_000);
        builder.max_duration = Some(std::time::Duration::from_secs(20));
        builder.check(model);
    }

    #[test]
    fn competing_session_callers_mutate_one_exclusive_owner() {
        bounded_model(|| {
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
        bounded_model(|| {
            let session = Arc::new(ExclusiveSlot::new(0_usize));
            let mut owner = session.try_take().unwrap();
            let contender = session.clone();
            thread::spawn(move || assert!(contender.try_take().is_none()))
                .join()
                .unwrap();
            *owner = 3;
            drop(owner);
            assert_eq!(*session.try_take().unwrap(), 3);
        });
    }

    #[test]
    fn early_lease_release_publishes_mutated_state_to_the_next_owner() {
        bounded_model(|| {
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
}
