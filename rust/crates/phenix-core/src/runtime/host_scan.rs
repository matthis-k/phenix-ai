impl<'a> PluginHost<'a> {
    pub fn scan_durable(
        &self,
        namespace: &ResourceNamespace,
        range: &DurableKeyRange,
        direction: ScanDirection,
        limit: Option<usize>,
    ) -> Result<Vec<DurableRecord>, KernelError> {
        self.require_persistence_operation(PERSISTENCE_READ, namespace)?;
        self.persistence
            .lock()
            .scan(self.plugin, namespace, range, direction, limit)
            .map_err(|error| self.persistence_error(error.to_string()))
    }
}
