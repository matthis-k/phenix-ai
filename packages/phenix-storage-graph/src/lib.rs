//! Optional storage-to-kernel integration. Storage and kernel do not depend on this crate.
use phenix_graph::Value;
use phenix_kernel::{Notice, Producer, WakeHandle};
use phenix_storage::Subscription;
use std::{collections::VecDeque, sync::Arc, task::Wake};

pub mod derived;

/// Routes each storage notice, including overflow, to one kernel wave as a local value.
/// Owns its subscription; dropping the binding cancels observation. No storage authority
/// is added: create the subscription through an appropriately authorized session.
pub struct StorageBinding {
    subscription: Subscription,
    trigger: String,
    pending: VecDeque<phenix_storage::Notice>,
    map: fn(phenix_storage::Notice) -> Value,
}

impl StorageBinding {
    pub fn new(subscription: Subscription, trigger: impl Into<String>) -> Self {
        Self {
            subscription,
            trigger: trigger.into(),
            pending: VecDeque::new(),
            map: Value::local,
        }
    }

    /// Preserve subscription polling and wakes while changing the route payload.
    pub fn mapped(
        subscription: Subscription,
        trigger: impl Into<String>,
        map: fn(phenix_storage::Notice) -> Value,
    ) -> Self {
        Self {
            map,
            ..Self::new(subscription, trigger)
        }
    }
}

struct KernelWake(WakeHandle);
impl Wake for KernelWake {
    fn wake(self: Arc<Self>) {
        self.0.wake();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.wake();
    }
}

impl Producer for StorageBinding {
    fn poll(&mut self) -> Result<Option<Notice>, phenix_kernel::CallbackError> {
        if self.pending.is_empty() {
            self.pending.extend(self.subscription.drain()?);
        }
        Ok(self.pending.pop_front().map(|notice| Notice {
            trigger: self.trigger.clone(),
            values: vec![(self.map)(notice)],
        }))
    }

    fn set_waker(&mut self, waker: WakeHandle) -> bool {
        if !self.pending.is_empty() {
            waker.wake();
        }
        self.subscription
            .set_waker(Arc::new(KernelWake(waker)).into())
            .is_ok()
    }
}
