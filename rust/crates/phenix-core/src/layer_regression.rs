use crate::{
    Authority, Kernel, KernelConfig, KernelError, LayerPolicy, LayerResult, PluginExecution,
    PluginHost, PluginId, PluginInstance, PluginManifest, ServiceContribution, ServiceId,
    ServiceRole,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn service() -> ServiceId {
    ServiceId::parse("test.layer-regression@1").unwrap()
}

fn manifest(id: &str, role: ServiceRole, priority: i32) -> PluginManifest {
    PluginManifest {
        id: plugin(id),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            service: service(),
            role,
            priority,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

struct Terminal {
    called: Arc<AtomicBool>,
}

impl PluginInstance for Terminal {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        self.called.store(true, Ordering::SeqCst);
        let mut output = b"terminal:".to_vec();
        output.extend_from_slice(input);
        Ok(output)
    }
}

struct FailingLayer;

impl PluginInstance for FailingLayer {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke_layer(
        &mut self,
        _service: &ServiceId,
        _input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<LayerResult, String> {
        Err("layer failed".into())
    }
}

#[test]
fn no_layer_dispatch_matches_direct_terminal_dispatch() {
    let called = Arc::new(AtomicBool::new(false));
    let terminal_manifest = manifest("terminal", ServiceRole::Terminal, 1);
    let terminal_id = terminal_manifest.id.clone();
    let terminal_called = Arc::clone(&called);
    let mut kernel = Kernel::new(KernelConfig::new([terminal_manifest]).unwrap());
    kernel
        .register_embedded_factory(terminal_id, move || {
            Box::new(Terminal {
                called: Arc::clone(&terminal_called),
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();

    assert_eq!(
        kernel
            .invoke(&service(), b"input", &Authority::default(), None)
            .unwrap(),
        b"terminal:input"
    );
    assert!(called.load(Ordering::SeqCst));
}

#[test]
fn layer_failure_stops_before_terminal_dispatch() {
    let called = Arc::new(AtomicBool::new(false));
    let layer_manifest = manifest("layer", ServiceRole::Layer, 100);
    let terminal_manifest = manifest("terminal", ServiceRole::Terminal, 1);
    let layer_id = layer_manifest.id.clone();
    let terminal_id = terminal_manifest.id.clone();
    let terminal_called = Arc::clone(&called);
    let config = KernelConfig::new([layer_manifest, terminal_manifest])
        .unwrap()
        .with_layer_policy(
            service(),
            vec![LayerPolicy {
                plugin: layer_id.clone(),
                priority: 100,
                required: false,
                enabled: true,
            }],
        )
        .unwrap();
    let mut kernel = Kernel::new(config);
    kernel
        .register_embedded_factory(layer_id, || Box::new(FailingLayer))
        .unwrap();
    kernel
        .register_embedded_factory(terminal_id, move || {
            Box::new(Terminal {
                called: Arc::clone(&terminal_called),
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();

    assert!(matches!(
        kernel.invoke(&service(), b"input", &Authority::default(), None),
        Err(KernelError::ServiceInvoke { .. })
    ));
    assert!(!called.load(Ordering::SeqCst));
}
