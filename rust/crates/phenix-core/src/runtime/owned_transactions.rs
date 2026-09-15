use super::{PluginHost, PERSISTENCE_WRITE};
use crate::{
    CallCancellationToken, CapabilityId, KernelError, NamespaceTransaction, ResourceNamespace,
    TransactionOp,
};

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
        let write = CapabilityId::parse(PERSISTENCE_WRITE)
            .expect("kernel persistence write capability is valid");
        if !self.authority.permits(&write) {
            return Err(KernelError::HostOperationDenied {
                plugin: self.plugin.clone(),
                operation: PERSISTENCE_WRITE.into(),
            });
        }
        if self
            .call_cancellation
            .as_ref()
            .is_some_and(CallCancellationToken::is_cancelled)
        {
            self.prepared_mutations.clear();
            return Err(KernelError::HostOperationDenied {
                plugin: self.plugin.clone(),
                operation: "durable multi-namespace transaction after call cancellation".into(),
            });
        }
        if participants.is_empty() {
            return Err(KernelError::HostOperationDenied {
                plugin: self.plugin.clone(),
                operation: "durable multi-namespace transaction without participants".into(),
            });
        }

        let mut transactions = Vec::with_capacity(participants.len());
        for (namespace, operations) in participants {
            if self.config.resource_owner(namespace) != Some(self.plugin) {
                return Err(KernelError::HostOperationDenied {
                    plugin: self.plugin.clone(),
                    operation: format!("{PERSISTENCE_WRITE}:{}", namespace.as_str()),
                });
            }
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
            .map_err(|error| KernelError::Persistence {
                plugin: self.plugin.clone(),
                message: error.to_string(),
            })
    }
}
