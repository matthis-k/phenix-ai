use crate::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, InterfaceId, Kernel, KernelError, PhenixValue, PluginExecution, PluginHost,
    PluginId, PluginInstance, PluginManifest, ResolvedGeneration, ResolvedGenerationActivation,
    ServiceId, SharedPluginInvocation,
};
use std::sync::Arc;

struct Entry;

impl ComponentInterface for Entry {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse("fixture.component-reentrancy.entry@1").unwrap()
    }
}

struct Middle;

impl ComponentInterface for Middle {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse("fixture.component-reentrancy.middle@1").unwrap()
    }
}

struct Terminal;

impl ComponentInterface for Terminal {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse("fixture.component-reentrancy.terminal@1").unwrap()
    }
}

fn plugin() -> PluginId {
    PluginId::parse("fixture.component-reentrancy").unwrap()
}

fn component(name: &str) -> ComponentId {
    ComponentId::parse(format!("fixture.component-reentrancy.{name}")).unwrap()
}

fn manifest() -> PluginManifest {
    PluginManifest {
        id: plugin(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn export<I: ComponentInterface>() -> ComponentExport {
    ComponentExport {
        interface: I::interface_id(),
        schema: I::schema(),
        priority: 0,
        required_authority: Authority::default(),
    }
}

fn import<I: ComponentInterface>() -> ComponentImport {
    ComponentImport {
        interface: I::interface_id(),
        schema: I::schema(),
        required: true,
        authority: Authority::default(),
    }
}

fn component_manifest(
    name: &str,
    imports: Vec<ComponentImport>,
    exports: Vec<ComponentExport>,
) -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: component(name),
        owner: plugin(),
        imports,
        exports,
        maximum_authority: Authority::default(),
    }
}

struct ReentrantPlugin {
    invocation: Arc<ReentrantInvocation>,
}

impl ReentrantPlugin {
    fn new(recurse_to_entry: bool) -> Self {
        Self {
            invocation: Arc::new(ReentrantInvocation { recurse_to_entry }),
        }
    }
}

impl PluginInstance for ReentrantPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn shared_invocation(&self) -> Option<Arc<dyn SharedPluginInvocation>> {
        Some(self.invocation.clone())
    }
}

struct LegacyReentrantPlugin;

impl PluginInstance for LegacyReentrantPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke_component(
        &mut self,
        target: &ComponentId,
        _service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if target == &component("entry") {
            let request: PhenixValue =
                serde_json::from_slice(input).map_err(|error| error.to_string())?;
            let response = host
                .invoke_import::<Middle>(&component("entry"), &request)
                .map_err(|error| error.to_string())?;
            serde_json::to_vec(&response).map_err(|error| error.to_string())
        } else {
            Ok(input.to_vec())
        }
    }
}

struct ReentrantInvocation {
    recurse_to_entry: bool,
}

impl SharedPluginInvocation for ReentrantInvocation {
    fn invoke_component(
        &self,
        target: &ComponentId,
        _service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let request: PhenixValue =
            serde_json::from_slice(input).map_err(|error| error.to_string())?;
        let response = if target == &component("entry") {
            host.invoke_import::<Middle>(&component("entry"), &request)
                .map_err(|error| error.to_string())?
        } else if target == &component("middle") && self.recurse_to_entry {
            host.invoke_import::<Entry>(&component("bridge"), &request)
                .map_err(|error| error.to_string())?
        } else if target == &component("middle") {
            host.invoke_import::<Terminal>(&component("middle"), &request)
                .map_err(|error| error.to_string())?
        } else if target == &component("terminal") {
            request
        } else {
            return Err(format!("unexpected component target: {target}"));
        };
        serde_json::to_vec(&response).map_err(|error| error.to_string())
    }
}

fn kernel(recurse_to_entry: bool) -> Kernel {
    let mut components = vec![
        component_manifest("entry", vec![import::<Middle>()], vec![export::<Entry>()]),
        component_manifest(
            "middle",
            if recurse_to_entry {
                Vec::new()
            } else {
                vec![import::<Terminal>()]
            },
            vec![export::<Middle>()],
        ),
    ];
    if recurse_to_entry {
        components.push(component_manifest(
            "bridge",
            vec![import::<Entry>()],
            Vec::new(),
        ));
    } else {
        components.push(component_manifest(
            "terminal",
            Vec::new(),
            vec![export::<Terminal>()],
        ));
    }
    let resolved =
        ResolvedGeneration::resolve([manifest()], components, [], &Authority::default()).unwrap();
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel
        .register_embedded_factory(plugin(), move || {
            Box::new(ReentrantPlugin::new(recurse_to_entry))
        })
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn legacy_kernel() -> Kernel {
    let components = vec![
        component_manifest("entry", vec![import::<Middle>()], vec![export::<Entry>()]),
        component_manifest("middle", Vec::new(), vec![export::<Middle>()]),
    ];
    let resolved =
        ResolvedGeneration::resolve([manifest()], components, [], &Authority::default()).unwrap();
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel
        .register_embedded_factory(plugin(), || Box::new(LegacyReentrantPlugin))
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

#[test]
fn legacy_plugin_reentry_is_rejected_before_relocking_its_instance() {
    let mut kernel = legacy_kernel();
    let input = serde_json::to_vec(&PhenixValue::String("legacy".into())).unwrap();

    let error = kernel
        .invoke_component(
            &component("entry"),
            &ServiceId::parse(Entry::interface_id().as_str().to_owned()).unwrap(),
            &input,
            &Authority::default(),
            &plugin(),
        )
        .unwrap_err();

    assert!(matches!(error, KernelError::ServiceInvoke { .. }));
    assert!(error.to_string().contains("causal plugin re-entry"));
}

#[test]
fn typed_imports_can_reenter_a_shared_plugin_at_distinct_component_endpoints() {
    let mut kernel = kernel(false);
    let input = serde_json::to_vec(&PhenixValue::String("ok".into())).unwrap();

    let output = kernel
        .invoke_component(
            &component("entry"),
            &ServiceId::parse(Entry::interface_id().as_str().to_owned()).unwrap(),
            &input,
            &Authority::default(),
            &plugin(),
        )
        .unwrap();

    assert_eq!(
        serde_json::from_slice::<PhenixValue>(&output).unwrap(),
        PhenixValue::String("ok".into())
    );
}

#[test]
fn typed_imports_reject_a_true_component_cycle_in_a_shared_plugin() {
    let mut kernel = kernel(true);
    let input = serde_json::to_vec(&PhenixValue::String("loop".into())).unwrap();

    let error = kernel
        .invoke_component(
            &component("entry"),
            &ServiceId::parse(Entry::interface_id().as_str().to_owned()).unwrap(),
            &input,
            &Authority::default(),
            &plugin(),
        )
        .unwrap_err();

    assert!(matches!(error, KernelError::ServiceInvoke { .. }));
    assert!(error.to_string().contains("causal same-service re-entry"));
}
