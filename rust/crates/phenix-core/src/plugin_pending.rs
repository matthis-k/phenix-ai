//! Single-settlement pending plugin invocation boundary.
//!
//! This is the Rust-native provider-side boundary, not the portable C ABI:
//! foreign plugins can adapt their correlated begin/poll/wakeup methods to a
//! `PluginPendingCall` after crossing the separately versioned native ABI.
//! Core never detaches a callback merely because its caller requested cancel.
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

/// Starting a shared plugin invocation may yield an immediate result or one
/// pending settlement. No model/tool-specific semantics enter the kernel.
pub enum PluginCallStart {
    Immediate(Result<Vec<u8>, String>),
    Pending(PluginPendingCall),
}

/// An outstanding native plugin response. The kernel's generation-pinned
/// worker owns this receiver for the entire callback lifetime.
pub struct PluginPendingCall {
    receiver: Receiver<Result<Vec<u8>, String>>,
}

/// A one-shot terminal outcome of a pending plugin call.
pub struct PluginCallCompletion {
    sender: Option<Sender<Result<Vec<u8>, String>>>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum PluginPendingPoll {
    Pending,
    Ready(Result<Vec<u8>, String>),
    Abandoned,
}

impl PluginPendingCall {
    /// Create one response channel; the producer owns its completion capability.
    /// The channel has no implicit timeout, retry, or detached child worker.
    #[must_use]
    pub fn channel() -> (Self, PluginCallCompletion) {
        let (sender, receiver) = mpsc::channel();
        (
            Self { receiver },
            PluginCallCompletion {
                sender: Some(sender),
            },
        )
    }

    /// Nonblocking callback/poll equivalence for native runtime adapters.
    pub fn poll(&self) -> PluginPendingPoll {
        match self.receiver.try_recv() {
            Ok(result) => PluginPendingPoll::Ready(result),
            Err(TryRecvError::Empty) => PluginPendingPoll::Pending,
            Err(TryRecvError::Disconnected) => PluginPendingPoll::Abandoned,
        }
    }

    /// Await physical callback settlement inside a root-owned native worker.
    /// Cancellation is delivered via PluginHost's inherited token, not by
    /// silently dropping the receiver or ending its generation lease.
    pub fn wait(self) -> Result<Vec<u8>, String> {
        self.receiver
            .recv()
            .map_err(|_| "pending plugin callback abandoned without settlement".to_owned())?
    }
}

impl PluginCallCompletion {
    /// Publish exactly one terminal result. The return value reports whether
    /// the kernel still receives callbacks; it never grants an extra dispatch.
    pub fn complete(mut self, result: Result<Vec<u8>, String>) -> Result<(), String> {
        self.sender
            .take()
            .expect("completion capability is single-use")
            .send(result)
            .map_err(|_| "pending plugin result receiver no longer exists".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_call_polls_and_completes_once() {
        let (pending, completion) = PluginPendingCall::channel();
        assert_eq!(pending.poll(), PluginPendingPoll::Pending);
        completion.complete(Ok(vec![1, 2, 3])).unwrap();
        assert_eq!(
            pending.poll(),
            PluginPendingPoll::Ready(Ok(vec![1, 2, 3]))
        );
        assert_eq!(pending.poll(), PluginPendingPoll::Abandoned);
    }

    #[test]
    fn abandoned_plugin_completion_is_not_normal_success() {
        let (pending, completion) = PluginPendingCall::channel();
        drop(completion);
        assert!(pending.wait().unwrap_err().contains("abandoned"));
    }

    #[test]
    fn deferred_plugin_failure_preserves_error() {
        let (pending, completion) = PluginPendingCall::channel();
        completion.complete(Err("provider rejected request".into())).unwrap();
        assert_eq!(pending.wait(), Err("provider rejected request".into()));
    }
}
