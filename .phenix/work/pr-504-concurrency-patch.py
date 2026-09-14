from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"expected exactly one match in {path}, found {count}: {old[:120]!r}")
    file.write_text(text.replace(old, new, 1))


replace_once(
    "rust/crates/phenix-core/src/runtime.rs",
    "pub trait SharedPluginInvocation: Send + Sync {\n    fn invoke(\n",
    "pub trait SharedPluginInvocation: Send + Sync {\n    /// Return whether this immutable endpoint can serve one service.\n    ///\n    /// Composite plugins may expose shared dispatch only for long-running or\n    /// reentrant-safe services while retaining mutable dispatch for the rest.\n    fn supports(&self, _service: &ServiceId) -> bool {\n        true\n    }\n\n    fn invoke(\n",
)
replace_once(
    "rust/crates/phenix-core/src/runtime.rs",
    "    prepared_embedded_instances: BTreeMap<PluginId, Box<dyn PluginInstance>>,\n",
    "    prepared_embedded_instances: Mutex<BTreeMap<PluginId, Box<dyn PluginInstance>>>,\n",
)

replace_once(
    "rust/crates/phenix-core/src/runtime/kernel.rs",
    "            prepared_embedded_instances: BTreeMap::new(),\n",
    "            prepared_embedded_instances: Mutex::new(BTreeMap::new()),\n",
)
replace_once(
    "rust/crates/phenix-core/src/runtime/kernel.rs",
    "        self.prepared_embedded_instances.insert(plugin, instance);\n",
    "        self.prepared_embedded_instances\n            .lock()\n            .insert(plugin, instance);\n",
)
replace_once(
    "rust/crates/phenix-core/src/runtime/kernel.rs",
    "        if let Some(instance) = self.prepared_embedded_instances.remove(plugin) {\n",
    "        if let Some(instance) = self.prepared_embedded_instances.lock().remove(plugin) {\n",
)
replace_once(
    "rust/crates/phenix-core/src/runtime/kernel.rs",
    "    pub fn invoke_component(\n        &mut self,\n",
    "    pub fn invoke_component(\n        &self,\n",
)
replace_once(
    "rust/crates/phenix-core/src/runtime/kernel.rs",
    "    pub fn invoke(\n        &mut self,\n",
    "    pub fn invoke(\n        &self,\n",
)

replace_once(
    "rust/crates/phenix-core/src/runtime/dispatch.rs",
    "    let shared_invocation = if guards.call_stack.contains(&provider.plugin) {\n        instance\n            .try_lock()\n            .and_then(|instance| instance.shared_invocation())\n    } else {\n        instance.lock().shared_invocation()\n    };\n",
    "    let shared_invocation = if guards.call_stack.contains(&provider.plugin) {\n        instance\n            .try_lock()\n            .and_then(|instance| instance.shared_invocation())\n    } else {\n        instance.lock().shared_invocation()\n    }\n    .filter(|invocation| invocation.supports(&chain.service));\n",
)

replace_once(
    "rust/crates/phenix-plugin-execution/src/agent_loop.rs",
    "    fn invoke(\n        &mut self,\n        service: &ServiceId,\n        input: &[u8],\n        host: &PluginHost<'_>,\n    ) -> Result<Vec<u8>, String> {\n        if service != &agent_loop_service() {\n            return Err(format!(\"unsupported agent loop service: {service}\"));\n        }\n        let context = context(host);\n        let interface = AgentLoopInterface::interface_id();\n        let command = context\n            .kernel\n            .decode_projected::<AgentLoopCommand>(&interface, input)\n            .map_err(|error| error.to_string())?;\n        let response = handle(&context, command)?;\n        context\n            .kernel\n            .encode_value(&response)\n            .map_err(|error| error.to_string())\n    }\n}\n",
    "    fn invoke(\n        &mut self,\n        service: &ServiceId,\n        input: &[u8],\n        host: &PluginHost<'_>,\n    ) -> Result<Vec<u8>, String> {\n        invoke_agent_loop(service, input, host)\n    }\n}\n\npub(crate) fn invoke_agent_loop(\n    service: &ServiceId,\n    input: &[u8],\n    host: &PluginHost<'_>,\n) -> Result<Vec<u8>, String> {\n    if service != &agent_loop_service() {\n        return Err(format!(\"unsupported agent loop service: {service}\"));\n    }\n    let context = context(host);\n    let interface = AgentLoopInterface::interface_id();\n    let command = context\n        .kernel\n        .decode_projected::<AgentLoopCommand>(&interface, input)\n        .map_err(|error| error.to_string())?;\n    let response = handle(&context, command)?;\n    context\n        .kernel\n        .encode_value(&response)\n        .map_err(|error| error.to_string())\n}\n",
)

replace_once(
    "rust/crates/phenix-plugin-execution/src/lib.rs",
    "use phenix_core::{\n    Authority, PluginHost, PluginInstance, PluginManifest, ServiceContribution, ServiceId,\n};\n",
    "use phenix_core::{\n    Authority, PluginHost, PluginInstance, PluginManifest, ServiceContribution, ServiceId,\n    SharedPluginInvocation,\n};\nuse std::sync::Arc;\n",
)
replace_once(
    "rust/crates/phenix-plugin-execution/src/lib.rs",
    "impl PluginInstance for ExecutionPackagePlugin {\n    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {\n",
    "struct ExecutionPackageSharedInvocation;\n\nimpl SharedPluginInvocation for ExecutionPackageSharedInvocation {\n    fn supports(&self, service: &ServiceId) -> bool {\n        service == &agent_loop::agent_loop_service()\n    }\n\n    fn invoke(\n        &self,\n        service: &ServiceId,\n        input: &[u8],\n        host: &PluginHost<'_>,\n    ) -> Result<Vec<u8>, String> {\n        agent_loop::invoke_agent_loop(service, input, host)\n    }\n}\n\nimpl PluginInstance for ExecutionPackagePlugin {\n    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {\n",
)
replace_once(
    "rust/crates/phenix-plugin-execution/src/lib.rs",
    "    fn invoke(\n        &mut self,\n",
    "    fn shared_invocation(&self) -> Option<Arc<dyn SharedPluginInvocation>> {\n        Some(Arc::new(ExecutionPackageSharedInvocation))\n    }\n\n    fn invoke(\n        &mut self,\n",
)

replace_once(
    "rust/crates/phenix-harness/src/lib.rs",
    "    pub fn invoke(\n        &mut self,\n",
    "    pub fn invoke(\n        &self,\n",
)

Path("rust/crates/phenix-core/tests/kernel_concurrent_invocation.rs").write_text(
    "use phenix_core::Kernel;\n\n"
    "fn assert_send_sync<T: Send + Sync>() {}\n\n"
    "#[test]\n"
    "fn kernel_supports_shared_immutable_invocation() {\n"
    "    assert_send_sync::<Kernel>();\n"
    "}\n"
)

workflow = Path(".github/workflows/sync-maintenance.yml")
workflow_text = workflow.read_text()
patch_line = "          python .phenix/work/pr-504-concurrency-patch.py\n"
if workflow_text.count(patch_line) != 1:
    raise SystemExit("maintenance one-shot patch line is missing or duplicated")
workflow.write_text(workflow_text.replace(patch_line, "", 1))
Path(__file__).unlink()
