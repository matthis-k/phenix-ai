#![allow(unexpected_cfgs)]

#[cfg(loom)]
use loom::sync::atomic::{AtomicUsize, Ordering};
#[cfg(loom)]
use loom::sync::{Condvar, Mutex, MutexGuard};
use std::ops::{Deref, DerefMut};
use std::sync::TryLockError;
#[cfg(not(loom))]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(not(loom))]
use std::sync::{Condvar, Mutex, MutexGuard};

/// Exclusive ownership transfer for mutable state accessed by callbacks.
pub(crate) struct ExclusiveSlot<T> {
    value: Mutex<Option<T>>,
    available: Condvar,
    waiters: AtomicUsize,
}

impl<T> ExclusiveSlot<T> {
    pub(crate) fn new(value: T) -> Self {
        Self {
            value: Mutex::new(Some(value)),
            available: Condvar::new(),
            waiters: AtomicUsize::new(0),
        }
    }

    pub(crate) fn take(&self) -> ExclusiveLease<'_, T> {
        let mut slot = self.lock();
        while slot.is_none() {
            self.waiters.fetch_add(1, Ordering::Relaxed);
            slot = self
                .available
                .wait(slot)
                .unwrap_or_else(|error| error.into_inner());
            self.waiters.fetch_sub(1, Ordering::Relaxed);
        }
        ExclusiveLease {
            owner: self,
            value: slot.take(),
        }
    }

    pub(crate) fn try_take(&self) -> Option<ExclusiveLease<'_, T>> {
        let mut slot = match self.value.try_lock() {
            Ok(slot) => slot,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return None,
        };
        Some(ExclusiveLease {
            owner: self,
            value: Some(slot.take()?),
        })
    }

    #[cfg(all(test, not(loom)))]
    pub(crate) fn lock_is_available(&self) -> bool {
        !matches!(self.value.try_lock(), Err(TryLockError::WouldBlock))
    }

    fn lock(&self) -> MutexGuard<'_, Option<T>> {
        self.value.lock().unwrap_or_else(|error| error.into_inner())
    }
}

pub(crate) struct ExclusiveLease<'a, T> {
    owner: &'a ExclusiveSlot<T>,
    value: Option<T>,
}

impl<T> ExclusiveLease<'_, T> {
    pub(crate) fn release(&mut self) {
        let Some(value) = self.value.take() else {
            return;
        };
        let mut slot = self.owner.lock();
        *slot = Some(value);
        if self.owner.waiters.load(Ordering::Relaxed) != 0 {
            self.owner.available.notify_one();
        }
    }
}

impl<T> Deref for ExclusiveLease<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value.as_ref().expect("exclusive lease is active")
    }
}

impl<T> DerefMut for ExclusiveLease<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value.as_mut().expect("exclusive lease is active")
    }
}

impl<T> Drop for ExclusiveLease<'_, T> {
    fn drop(&mut self) {
        self.release();
    }
}
