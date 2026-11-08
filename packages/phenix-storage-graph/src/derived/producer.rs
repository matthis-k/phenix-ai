use super::*;
use crate::StorageBinding;
use phenix_graph::Value;
use phenix_kernel::{Notice, Producer, WakeHandle};

/// Deferred observation request. Put `route()` in Contribution::routes and call
/// `activate()` in its activation factory, never during graph composition.
/// All subscriptions are acquired before queuing one bootstrap full rescan.
/// Concurrent registry-local commits remain queued for subsequent rescans. There
/// is no cross-process notification guarantee or durable change cursor. Each
/// StorageBinding retains one bounded drain per source, separately from Storage's
/// bounded queue; polling rotates across sources. Kernel owns producer fairness.
#[derive(Clone)]
pub struct Observation {
    root: Arc<Root>,
    capacity: usize,
}
impl Observation {
    pub(crate) fn new(root: Arc<Root>, capacity: usize) -> Self {
        Self { root, capacity }
    }
    pub fn route(&self) -> &str {
        &self.root.trigger
    }
    pub fn activate(&self) -> Result<Box<dyn Producer>, Error> {
        let bindings = self
            .root
            .sources
            .iter()
            .map(|source| {
                Ok(StorageBinding::mapped(
                    source.changed(self.capacity)?,
                    self.route(),
                    |notice| {
                        Value::local(match notice {
                            phenix_storage::Notice::Overflow { .. } => RefreshCause::Resynchronize,
                            notice => RefreshCause::Invalidated(notice),
                        })
                    },
                ))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Box::new(Observer {
            bindings,
            next: 0,
            bootstrap: true,
            route: self.route().into(),
        }))
    }
}
struct Observer {
    bindings: Vec<StorageBinding>,
    next: usize,
    bootstrap: bool,
    route: String,
}
impl Producer for Observer {
    fn poll(&mut self) -> Result<Option<Notice>, String> {
        if self.bootstrap {
            self.bootstrap = false;
            return Ok(Some(Notice {
                trigger: self.route.clone(),
                values: vec![Value::local(RefreshCause::Resynchronize)],
            }));
        }
        for _ in 0..self.bindings.len() {
            let i = self.next;
            self.next = (i + 1) % self.bindings.len();
            if let Some(notice) = self.bindings[i].poll()? {
                return Ok(Some(notice));
            }
        }
        Ok(None)
    }
    fn set_waker(&mut self, wake: WakeHandle) -> bool {
        if self.bootstrap {
            wake.wake();
        }
        let mut supported = true;
        for binding in &mut self.bindings {
            supported = binding.set_waker(wake.clone()) && supported;
        }
        supported
    }
}
