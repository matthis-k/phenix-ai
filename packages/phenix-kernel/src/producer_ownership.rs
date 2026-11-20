#![allow(unexpected_cfgs)]

#[cfg(loom)]
use loom::sync::atomic::{AtomicUsize, Ordering};
#[cfg(not(loom))]
use std::sync::atomic::{AtomicUsize, Ordering};

// One linearizable producer ownership word: 0 means free, MAX is a
// whole-lifetime blocking driver, all other values count in-flight or
// gate-queued manual drains. The producer_drain mutex still serializes
// actual callbacks; the owner word prevents direct/driver mode overlap.
pub(crate) const DRIVER_OWNER: usize = usize::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DriverAdmissionError {
    AlreadyRunning,
    DrainInProgress,
}

pub(crate) struct DriverScope<'a>(&'a AtomicUsize);
impl<'a> DriverScope<'a> {
    pub(crate) fn enter(owner: &'a AtomicUsize) -> Result<Self, DriverAdmissionError> {
        match owner.compare_exchange(0, DRIVER_OWNER, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => Ok(Self(owner)),
            Err(DRIVER_OWNER) => Err(DriverAdmissionError::AlreadyRunning),
            Err(_) => Err(DriverAdmissionError::DrainInProgress),
        }
    }
}
impl Drop for DriverScope<'_> {
    fn drop(&mut self) {
        self.0.store(0, Ordering::Release);
    }
}

pub(crate) struct DirectDrainScope<'a>(&'a AtomicUsize);
impl<'a> DirectDrainScope<'a> {
    pub(crate) fn enter(owner: &'a AtomicUsize) -> Option<Self> {
        owner
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < DRIVER_OWNER - 1).then(|| count + 1)
            })
            .ok()
            .map(|_| Self(owner))
    }
}
impl Drop for DirectDrainScope<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}
