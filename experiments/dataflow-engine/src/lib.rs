//! Experimental typed fixture. Boundary policy is explicit, never a scheduler.
pub mod dfir_engine;
pub mod timely_engine;

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

pub use dataflow_fixture::*;
/// Plugin-owned state and controlled physical work; no readiness or progress counters.
#[derive(Default)]
pub struct Plugin {
    pub revision: u64,
    pub bias: String,
    pub effects: BTreeMap<Identity, String>,
    pub remote: Option<std::path::PathBuf>,
    pending: BTreeMap<Identity, oneshot::Sender<()>>,
    cancelled: BTreeMap<u64, CancellationToken>,
}
pub type SharedPlugin = Rc<RefCell<Plugin>>;
impl Plugin {
    pub fn commit(&mut self, expected: u64, bias: &str) -> Result<(), &'static str> {
        if self.revision != expected {
            return Err("conflict");
        }
        self.bias = bias.into();
        self.revision += 1;
        Ok(())
    }
    pub fn release(&mut self, id: Identity) {
        if let Some(tx) = self.pending.remove(&id) {
            let _ = tx.send(());
        }
    }
    pub fn disappear(&mut self, id: Identity) {
        self.pending.remove(&id);
    }
    pub fn cancel(&mut self, root: u64) {
        self.cancelled.entry(root).or_default().cancel();
    }
    pub fn physical_pending(&self) -> usize {
        self.pending.len()
    }
}
pub fn service(
    p: Prepared,
    state: SharedPlugin,
    t: Trace,
) -> impl std::future::Future<Output = Option<ResultValue>> {
    // Starting physical work happens when framework schedules this node.
    let m = p.0;
    trace(&t, m.id, if m.tool { "Tool" } else { "Model" });
    let (rx, token, bias, remote) = {
        let mut s = state.borrow_mut();
        let token = s.cancelled.entry(m.id.root).or_default().clone();
        let rx = if m.pending {
            let (tx, rx) = oneshot::channel();
            s.pending.insert(m.id, tx);
            Some(rx)
        } else {
            None
        };
        (rx, token, s.bias.clone(), s.remote.clone())
    };
    async move {
        if let Some(rx) = rx {
            if rx.await.is_err() {
                trace(&t, m.id, "WorkerLost");
                return None;
            }
        }
        if token.is_cancelled() {
            trace(&t, m.id, "PhysicalDrained");
            trace(&t, m.id, "Cancelled");
            return None;
        }
        let text = if let Some(path) = remote {
            use tokio::io::AsyncWriteExt;
            let mut child = tokio::process::Command::new(path)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let mut stdin = child.stdin.take().unwrap();
            stdin
                .write_all(&serde_json::to_vec(&m).unwrap())
                .await
                .unwrap();
            drop(stdin);
            let status =
                tokio::time::timeout(std::time::Duration::from_secs(2), child.wait()).await;
            let Ok(Ok(status)) = status else {
                let _ = child.start_kill();
                let _ = child.wait().await;
                trace(&t, m.id, "PhysicalDrained");
                trace(&t, m.id, "TransportLost");
                return None;
            };
            trace(&t, m.id, "PhysicalDrained");
            if !status.success() {
                trace(&t, m.id, "TransportLost");
                return None;
            }
            use tokio::io::AsyncReadExt;
            let mut out = Vec::new();
            child
                .stdout
                .take()
                .unwrap()
                .read_to_end(&mut out)
                .await
                .unwrap();
            let Ok((id, text)) = serde_json::from_slice::<(Identity, String)>(&out) else {
                trace(&t, m.id, "MalformedRemote");
                return None;
            };
            if id != m.id {
                trace(&t, m.id, "IdentityMismatch");
                return None;
            }
            if token.is_cancelled() {
                trace(&t, m.id, "Cancelled");
                return None;
            }
            text
        } else {
            trace(&t, m.id, "PhysicalDrained");
            response(&m, &bias)
        };
        // Explicit owner-level idempotency policy; frameworks don't supply this.
        let text = state
            .borrow_mut()
            .effects
            .entry(m.id)
            .or_insert(text)
            .clone();
        Some(ResultValue { id: m.id, text })
    }
}

/// Trusted ingress checks. Serialization does not grant authority.
#[derive(Default)]
pub struct Ingress {
    seen: BTreeSet<Identity>,
}
impl Ingress {
    pub fn admit(
        &mut self,
        port: &str,
        batch: &[Message],
        authorized: bool,
    ) -> Result<Vec<Message>, &'static str> {
        if !authorized {
            return Err("denied");
        }
        if port != "Prepare.in" {
            return Err("unknown port");
        }
        let mut ids = BTreeSet::new();
        if batch
            .iter()
            .any(|m| self.seen.contains(&m.id) || !ids.insert(m.id))
        {
            return Err("duplicate occurrence");
        }
        self.seen.extend(ids);
        Ok(batch.to_vec())
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod frontier_tests;
