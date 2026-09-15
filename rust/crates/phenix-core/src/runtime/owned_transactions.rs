use super::{PluginHost, PERSISTENCE_WRITE};
use crate::{KernelError, NamespaceTransaction, ResourceNamespace, TransactionOp};

impl PluginHost<'_> {
    /// Atomically commit multiple durable namespace transactions owned by the current plugin.
    ///
    /// This is the same-owner counterpart to the owner-prepared cross-plugin protocol. Core
    /// derives every participant owner from the scoped host, so callers cannot use this path to
    /// write another plugin's namespace.
    pub fn transact_owned_durable_many(
        &self,
        participants: &[(&ResourceNamespace, &[TransactionOp])],
    ) -> Result<(), KernelError> {
        self.require_capability(PERSISTENCE_WRITE)?;
        self.require_not_cancelled("durable multi-namespace transaction")?;
        if participants.is_empty() {
            return Err(KernelError::HostOperationDenied {
                plugin: self.plugin.clone(),
                operation: "durable multi-namespace transaction without participants".into(),
            });
        }

        let mut transactions = Vec::with_capacity(participants.len());
        for (namespace, operations) in participants {
            self.require_persistence_operation(PERSISTENCE_WRITE, namespace)?;
            transactions.push(NamespaceTransaction {
                owner: self.plugin.clone(),
                namespace: (*namespace).clone(),
                operations: operations.to_vec(),
            });
        }

        self.persistence
            .lock()
            .expect("kernel persistence mutex poisoned")
            .transact_many(&transactions)
            .map_err(|error| self.persistence_error(error.to_string()))
    }
}
