//! Plugin lifecycle errors. Not graph execution errors.
use std::{fmt, sync::Arc};

/// An owned callback cause, retaining its concrete type for source inspection.
pub type CallbackError = Box<dyn std::error::Error + Send + Sync + 'static>;

struct LeasedCallback {
    source: Option<CallbackError>,
    _lease: Arc<dyn Send + Sync>,
}
impl fmt::Debug for LeasedCallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LeasedCallback")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}
impl fmt::Display for LeasedCallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.as_ref().expect("live callback cause").fmt(f)
    }
}
impl std::error::Error for LeasedCallback {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_deref().map(|source| source as _)
    }
}
impl Drop for LeasedCallback {
    fn drop(&mut self) {
        // Keep the exact lease outside both unwind boundaries. Field order alone
        // releases it before a caller could dispose of a destructor panic payload.
        let source = self.source.take();
        if let Err(payload) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(source)))
            && let Err(second) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload)))
        {
            // Bounded cleanup: do not recursively destroy a second panic payload.
            std::mem::forget(second);
        }
    }
}

/// Retain native code/resources through every method and destructor of an escaping
/// callback cause. The original concrete error remains in the source chain.
/// Source and first panic-payload destruction are contained while the lease is
/// held; a second panic payload is deliberately forgotten, not recursively dropped.
pub fn lease_callback_error(source: CallbackError, lease: Arc<dyn Send + Sync>) -> CallbackError {
    Box::new(LeasedCallback {
        source: Some(source),
        _lease: lease,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackPhase {
    Construct,
    Contribute,
    Bind,
}

#[derive(Debug)]
pub struct CallbackContext {
    pub plugin: String,
    pub phase: CallbackPhase,
    pub contribution: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    DescriptorInvalid,
    SchemaRejected,
    Denied,
    MissingBinding,
    InfrastructureFailure,
    PluginFailed,
}
#[derive(Debug)]
pub struct Failure {
    pub code: Code,
    pub message: String,
    pub context: Option<CallbackContext>,
    source: Option<CallbackError>,
}
impl Failure {
    /// An application-provided message leaf. Framework failures attach typed
    /// causes with [`Self::with_source`] instead.
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            context: None,
            source: None,
        }
    }

    pub fn with_source(code: Code, message: impl Into<String>, source: CallbackError) -> Self {
        Self {
            source: Some(source),
            ..Self::new(code, message)
        }
    }

    pub(crate) fn at(
        self,
        plugin: &str,
        phase: CallbackPhase,
        lease: Arc<dyn Send + Sync>,
    ) -> Self {
        let mut error = Self::with_source(
            self.code,
            self.message.clone(),
            lease_callback_error(Box::new(self), lease),
        );
        error.context = Some(CallbackContext {
            plugin: plugin.into(),
            phase,
            contribution: None,
        });
        error
    }
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(context) = &self.context {
            write!(f, "plugin {:?} {:?}: ", context.plugin, context.phase)?;
        }
        write!(f, "{:?}: {}", self.code, self.message)
    }
}
impl std::error::Error for Failure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_deref().map(|source| source as _)
    }
}
pub(crate) fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Weak,
        atomic::{AtomicUsize, Ordering::SeqCst},
    };

    #[derive(Debug)]
    struct Lease(Arc<AtomicUsize>);
    impl Drop for Lease {
        fn drop(&mut self) {
            self.0.fetch_add(1, SeqCst);
        }
    }
    #[derive(Debug)]
    struct Cause {
        lease: Weak<Lease>,
        drops: Arc<AtomicUsize>,
        payload_drops: Arc<AtomicUsize>,
        second_drops: Arc<AtomicUsize>,
        panic: u8,
    }
    impl fmt::Display for Cause {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            assert!(self.lease.upgrade().is_some());
            f.write_str("concrete callback cause")
        }
    }
    impl std::error::Error for Cause {}
    impl Drop for Cause {
        fn drop(&mut self) {
            assert!(self.lease.upgrade().is_some());
            self.drops.fetch_add(1, SeqCst);
            if self.panic != 0 {
                std::panic::panic_any(Payload {
                    lease: self.lease.clone(),
                    drops: self.payload_drops.clone(),
                    second_drops: self.second_drops.clone(),
                    panic: self.panic == 2,
                });
            }
        }
    }
    struct Payload {
        lease: Weak<Lease>,
        drops: Arc<AtomicUsize>,
        second_drops: Arc<AtomicUsize>,
        panic: bool,
    }
    impl Drop for Payload {
        fn drop(&mut self) {
            assert!(self.lease.upgrade().is_some());
            self.drops.fetch_add(1, SeqCst);
            if self.panic {
                std::panic::panic_any(Second(self.second_drops.clone()));
            }
        }
    }
    struct Second(Arc<AtomicUsize>);
    impl Drop for Second {
        fn drop(&mut self) {
            self.0.fetch_add(1, SeqCst);
        }
    }

    #[test]
    fn callback_cleanup_keeps_exact_lease_through_source_and_bounded_payload_drops() {
        for panic in 0..=2 {
            for unwinding in [false, true] {
                let lease_drops = Arc::new(AtomicUsize::new(0));
                let lease = Arc::new(Lease(lease_drops.clone()));
                let drops = Arc::new(AtomicUsize::new(0));
                let payload_drops = Arc::new(AtomicUsize::new(0));
                let second_drops = Arc::new(AtomicUsize::new(0));
                let cause = lease_callback_error(
                    Box::new(Cause {
                        lease: Arc::downgrade(&lease),
                        drops: drops.clone(),
                        payload_drops: payload_drops.clone(),
                        second_drops: second_drops.clone(),
                        panic,
                    }),
                    lease,
                );
                assert!(cause.source().unwrap().is::<Cause>());
                assert_eq!(cause.to_string(), "concrete callback cause");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _cause = cause;
                    if unwinding {
                        panic!("outer unwind");
                    }
                }));
                assert_eq!(result.is_err(), unwinding);
                assert_eq!(drops.load(SeqCst), 1);
                assert_eq!(payload_drops.load(SeqCst), usize::from(panic != 0));
                assert_eq!(second_drops.load(SeqCst), 0);
                assert_eq!(lease_drops.load(SeqCst), 1);
            }
        }
    }
}
