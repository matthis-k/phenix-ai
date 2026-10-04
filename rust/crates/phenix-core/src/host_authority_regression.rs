use phenix_core::{
    Authority, PermissionId, Kernel, KernelConfig, PluginExecution, PluginHost, PluginId,
    PluginInstance, PluginManifest, ServiceContribution, ServiceId,
};

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn service(value: &str) -> ServiceId {
    ServiceId::parse(value).unwrap()
}

fn capability(value: &str) -> PermissionId {
    PermissionId::parse(value).unwrap()
}

struct Downstream;

impl PluginInstance for Downstream {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        _input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        Ok(b"unexpected".to_vec())
    }
}

struct Delegator;

impl PluginInstance for Delegator {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        _input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let requested = Authority::new([capability("fs.write")]);
        for _ in 0..2 {
            if host
                .invoke_service_abi(&service("downstream@1"), b"", &requested, None)
                .is_ok()
            {
                return Err("delegated call regained denied authority".into());
            }
        }
        Ok(b"denied-twice".to_vec())
    }
}

#[test]
fn plugin_delegation_and_retry_cannot_regain_denied_authority() {
    let read = capability("fs.read");
    let write = capability("fs.write");
    let delegator = PluginManifest {
        id: plugin("delegator"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: crate::ServiceRole::Terminal,
            service: service("root@1"),
            priority: 1,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::new([read.clone(), write]),
    };
    let downstream = PluginManifest {
        id: plugin("downstream"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: crate::ServiceRole::Terminal,
            service: service("downstream@1"),
            priority: 1,
            required_authority: Authority::new([capability("fs.write")]),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::new([capability("fs.write")]),
    };

    let mut kernel = Kernel::new(KernelConfig::new([delegator, downstream]).unwrap());
    kernel
        .register_embedded_factory(plugin("delegator"), || Box::new(Delegator))
        .unwrap();
    kernel
        .register_embedded_factory(plugin("downstream"), || Box::new(Downstream))
        .unwrap();
    kernel.activate_all().unwrap();

    assert_eq!(
        kernel
            .invoke(&service("root@1"), b"", &Authority::new([read]), None,)
            .unwrap(),
        b"denied-twice"
    );
}
