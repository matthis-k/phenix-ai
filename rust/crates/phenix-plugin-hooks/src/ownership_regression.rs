use crate::hook_service;
use phenix_core::{
    Authority, EventBus, Kernel, KernelConfig, KernelError, KernelEvent, PluginExecution,
    PluginHost, PluginId, PluginInstance, PluginManifest, ServiceContribution, ServiceId,
    ServiceRole,
};

struct ReplacementHooks;

impl PluginInstance for ReplacementHooks {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &hook_service() {
            return Err(format!("unsupported replacement hook service: {service}"));
        }
        let mut output = b"replacement:".to_vec();
        output.extend_from_slice(input);
        Ok(output)
    }
}

fn replacement_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse("replacement-hooks").unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            service: hook_service(),
            role: ServiceRole::Terminal,
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

#[test]
fn hook_behavior_is_absent_when_omitted_and_replaceable_through_public_core_api() {
    let omitted = Kernel::new(KernelConfig::empty());
    assert_eq!(
        omitted
            .invoke(&hook_service(), b"ignored", &Authority::default(), None,)
            .unwrap_err(),
        KernelError::NoEligibleProvider(hook_service())
    );

    let manifest = replacement_manifest();
    let plugin = manifest.id.clone();
    let mut replacement = Kernel::new(KernelConfig::new([manifest]).unwrap());
    replacement
        .register_embedded_factory(plugin, || Box::new(ReplacementHooks))
        .unwrap();
    replacement.activate_all().unwrap();

    assert_eq!(
        replacement
            .invoke(&hook_service(), b"event", &Authority::default(), None,)
            .unwrap(),
        b"replacement:event"
    );
}

#[test]
fn generic_kernel_events_remain_available_when_configurable_hooks_are_omitted() {
    let events = EventBus::default();
    let receiver = events.subscribe();
    let plugin = PluginId::parse("fixture.plugin").unwrap();

    events.publish(KernelEvent::PluginActivated(plugin.clone()));

    assert_eq!(
        receiver.recv().unwrap(),
        KernelEvent::PluginActivated(plugin)
    );
}
