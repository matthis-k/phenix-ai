use phenix_core::{
    InitialObservation, ObservableRegistration, ObservableStore, ObservationDelivery,
    ObservationMode, ObservationScope, ObservationSpec, PhenixValue, PluginId, SnapshotPolicy,
    Type, ValueAddress, ValueId, ValuePath,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct CountingAllocator;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        System.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn current_only_scalar_commit_with_local_diff_listener_allocates_nothing() {
    let store = ObservableStore::default();
    let value = ValueId::parse("allocation.scalar@1").unwrap();
    store
        .register(ObservableRegistration {
            id: value.clone(),
            owner: PluginId::parse("allocation-test").unwrap(),
            schema: Type::U64,
            snapshot_policy: SnapshotPolicy::CurrentOnly,
            initial: PhenixValue::U64(0),
        })
        .unwrap();
    store
        .subscribe(
            ObservationSpec {
                address: ValueAddress {
                    value: value.clone(),
                    path: ValuePath::root(),
                },
                scope: ObservationScope::Exact,
                mode: ObservationMode::Diff,
                initial: InitialObservation::None,
            },
            Arc::new(|delivery: ObservationDelivery<'_>| {
                assert!(delivery.version.get() > 0);
                assert_eq!(delivery.changes.len(), 1);
            }),
        )
        .unwrap();

    // Warm all lazy runtime paths before measuring the transaction itself.
    store
        .transaction([&value], |tx| {
            tx.replace(&value, ValuePath::root(), PhenixValue::U64(1))
        })
        .unwrap();
    store
        .transaction([&value], |tx| {
            tx.replace(&value, ValuePath::root(), PhenixValue::U64(0))
        })
        .unwrap();

    ALLOCATIONS.store(0, Ordering::SeqCst);
    store
        .transaction([&value], |tx| {
            tx.replace(&value, ValuePath::root(), PhenixValue::U64(1))
        })
        .unwrap();
    assert_eq!(ALLOCATIONS.load(Ordering::SeqCst), 0);
}
