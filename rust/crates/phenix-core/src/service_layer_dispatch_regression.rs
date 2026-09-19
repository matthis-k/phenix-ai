use crate::{
    runtime_trace_event_type, Authority, CapabilityId, EventEnvelope, EventFailurePolicy,
    EventSubscription, Kernel, KernelConfig, KernelError, LayerPolicy, LayerResult,
    PluginExecution, PluginHost, PluginId, PluginInstance, PluginManifest, RuntimeTraceEvent,
    ServiceContribution, ServiceId, ServiceRole, SubscriptionId, SubscriptionSpec,
    RUNTIME_TRACE_EVENT_VERSION,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn service() -> ServiceId {
    ServiceId::parse("demo.layered@1").unwrap()
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).unwrap()
}

fn manifest(id: &str, role: ServiceRole, priority: i32, maximum: Authority) -> PluginManifest {
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
        maximum_authority: maximum,
    }
}

struct Terminal {
    called: Arc<AtomicBool>,
    fail: bool,
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
        if self.fail {
            Err("terminal failed".into())
        } else {
            let mut output = b"terminal:".to_vec();
            output.extend_from_slice(input);
            Ok(output)
        }
    }
}

#[derive(Clone, Copy)]
enum Behavior {
    Delegate,
    Handle,
    Deny,
    DoubleDelegate,
    Reenter,
    RequestMoreAuthority,
}

struct Layer {
    behavior: Behavior,
}

impl PluginInstance for Layer {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke_layer(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<LayerResult, String> {
        match self.behavior {
            Behavior::Delegate => {
                let output = host
                    .continue_service(input, host.authority())
                    .map_err(|error| error.to_string())?;
                let mut wrapped = b"layer:".to_vec();
                wrapped.extend_from_slice(&output);
                Ok(LayerResult::Handled(wrapped))
            }
            Behavior::Handle => Ok(LayerResult::Handled(b"handled".to_vec())),
            Behavior::Deny => Ok(LayerResult::Denied("policy denied".into())),
            Behavior::DoubleDelegate => {
                host.continue_service(input, host.authority())
                    .map_err(|error| error.to_string())?;
                let error = host.continue_service(input, host.authority()).unwrap_err();
                Ok(LayerResult::Handled(error.to_string().into_bytes()))
            }
            Behavior::Reenter => {
                let error = host
                    .invoke_service_abi(service, input, host.authority(), None)
                    .unwrap_err();
                Ok(LayerResult::Handled(error.to_string().into_bytes()))
            }
            Behavior::RequestMoreAuthority => {
                let requested = Authority::new([capability("demo.read"), capability("demo.write")]);
                host.continue_service(input, &requested)
                    .map(LayerResult::Handled)
                    .map_err(|error| error.to_string())
            }
        }
    }
}

struct AuthorityTerminal;

impl PluginInstance for AuthorityTerminal {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        _input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        Ok(if host.authority().permits(&capability("demo.write")) {
            b"write".to_vec()
        } else {
            b"read-only".to_vec()
        })
    }
}

fn kernel_with_layer(
    behavior: Behavior,
    terminal_called: Arc<AtomicBool>,
    maximum: Authority,
) -> Kernel {
    let layer_manifest = manifest("layer", ServiceRole::Layer, 100, maximum.clone());
    let terminal_manifest = manifest("terminal", ServiceRole::Terminal, 1, maximum);
    let layer_id = layer_manifest.id.clone();
    let terminal_id = terminal_manifest.id.clone();
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
        .register_embedded_factory(layer_id, move || Box::new(Layer { behavior }))
        .unwrap();
    kernel
        .register_embedded_factory(terminal_id, move || {
            Box::new(Terminal {
                called: Arc::clone(&terminal_called),
                fail: false,
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

#[test]
fn layer_delegates_and_transforms_response() {
    let called = Arc::new(AtomicBool::new(false));
    let mut kernel = kernel_with_layer(
        Behavior::Delegate,
        Arc::clone(&called),
        Authority::default(),
    );
    assert_eq!(
        kernel
            .invoke(&service(), b"x", &Authority::default(), None)
            .unwrap(),
        b"layer:terminal:x"
    );
    assert!(called.load(Ordering::SeqCst));
}

#[test]
fn layer_can_handle_without_invoking_terminal() {
    let called = Arc::new(AtomicBool::new(false));
    let mut kernel = kernel_with_layer(Behavior::Handle, Arc::clone(&called), Authority::default());
    assert_eq!(
        kernel
            .invoke(&service(), b"x", &Authority::default(), None)
            .unwrap(),
        b"handled"
    );
    assert!(!called.load(Ordering::SeqCst));
}

#[test]
fn layer_denial_is_typed_and_terminal_is_not_invoked() {
    let called = Arc::new(AtomicBool::new(false));
    let mut kernel = kernel_with_layer(Behavior::Deny, Arc::clone(&called), Authority::default());
    assert!(matches!(
        kernel.invoke(&service(), b"x", &Authority::default(), None),
        Err(KernelError::ServiceDenied { .. })
    ));
    assert!(!called.load(Ordering::SeqCst));
}

#[test]
fn continuation_is_one_shot() {
    let mut kernel = kernel_with_layer(
        Behavior::DoubleDelegate,
        Arc::new(AtomicBool::new(false)),
        Authority::default(),
    );
    let output = kernel
        .invoke(&service(), b"x", &Authority::default(), None)
        .unwrap();
    assert!(String::from_utf8(output)
        .unwrap()
        .contains("already consumed"));
}

#[test]
fn same_service_recursive_invocation_is_rejected() {
    let mut kernel = kernel_with_layer(
        Behavior::Reenter,
        Arc::new(AtomicBool::new(false)),
        Authority::default(),
    );
    let output = kernel
        .invoke(&service(), b"x", &Authority::default(), None)
        .unwrap();
    assert!(String::from_utf8(output)
        .unwrap()
        .contains("same-service re-entry"));
}

#[test]
fn continuation_cannot_expand_authority() {
    let maximum = Authority::new([capability("demo.read"), capability("demo.write")]);
    let layer_manifest = manifest("layer", ServiceRole::Layer, 100, maximum.clone());
    let terminal_manifest = manifest("terminal", ServiceRole::Terminal, 1, maximum);
    let layer_id = layer_manifest.id.clone();
    let terminal_id = terminal_manifest.id.clone();
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
        .register_embedded_factory(layer_id, || {
            Box::new(Layer {
                behavior: Behavior::RequestMoreAuthority,
            })
        })
        .unwrap();
    kernel
        .register_embedded_factory(terminal_id, || Box::new(AuthorityTerminal))
        .unwrap();
    kernel.activate_all().unwrap();
    let caller = Authority::new([capability("demo.read")]);
    assert_eq!(
        kernel.invoke(&service(), b"x", &caller, None).unwrap(),
        b"read-only"
    );
}

#[test]
fn terminal_failure_does_not_fallback_to_lower_priority_provider() {
    let high_called = Arc::new(AtomicBool::new(false));
    let low_called = Arc::new(AtomicBool::new(false));
    let high = manifest("high", ServiceRole::Terminal, 100, Authority::default());
    let low = manifest("low", ServiceRole::Terminal, 1, Authority::default());
    let high_id = high.id.clone();
    let low_id = low.id.clone();
    let high_flag = Arc::clone(&high_called);
    let low_flag = Arc::clone(&low_called);
    let mut kernel = Kernel::new(KernelConfig::new([high, low]).unwrap());
    kernel
        .register_embedded_factory(high_id, move || {
            Box::new(Terminal {
                called: Arc::clone(&high_flag),
                fail: true,
            })
        })
        .unwrap();
    kernel
        .register_embedded_factory(low_id, move || {
            Box::new(Terminal {
                called: Arc::clone(&low_flag),
                fail: false,
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();
    assert!(matches!(
        kernel.invoke(&service(), b"x", &Authority::default(), None),
        Err(KernelError::ServiceInvoke { .. })
    ));
    assert!(high_called.load(Ordering::SeqCst));
    assert!(!low_called.load(Ordering::SeqCst));
}

#[test]
fn provenance_records_planned_chain_delegation_terminal_and_authority() {
    let called = Arc::new(AtomicBool::new(false));
    let read = capability("demo.read");
    let mut kernel = kernel_with_layer(
        Behavior::Delegate,
        Arc::clone(&called),
        Authority::new([read.clone()]),
    );
    let caller = Authority::new([read.clone(), capability("demo.write")]);
    kernel.invoke(&service(), b"x", &caller, None).unwrap();

    let records = kernel.service_invocation_provenance();
    let record = records.last().expect("invocation provenance recorded");
    assert_eq!(record.service, service());
    assert_eq!(record.policy_identity, record.planned_chain.policy_identity);
    assert_eq!(record.planned_chain.layers.len(), 1);
    assert_eq!(record.planned_chain.layers[0].plugin, plugin("layer"));
    assert_eq!(record.planned_chain.terminal.plugin, plugin("terminal"));
    assert_eq!(record.caller_authority, caller);
    assert!(record.terminal_reached);
    assert_eq!(record.participants.len(), 2);
    assert_eq!(record.participants[0].plugin, plugin("layer"));
    assert_eq!(record.participants[0].role, ServiceRole::Layer);
    assert_eq!(
        record.participants[0].outcome,
        crate::ServiceParticipantOutcome::Delegated
    );
    assert_eq!(
        record.participants[0].effective_authority,
        Authority::new([read.clone()])
    );
    assert_eq!(record.participants[1].plugin, plugin("terminal"));
    assert_eq!(record.participants[1].role, ServiceRole::Terminal);
    assert_eq!(
        record.participants[1].outcome,
        crate::ServiceParticipantOutcome::Succeeded
    );
    assert_eq!(
        record.participants[1].effective_authority,
        Authority::new([read])
    );
}

#[test]
fn runtime_trace_reports_service_chain_without_request_payload() {
    let called = Arc::new(AtomicBool::new(false));
    let mut kernel = kernel_with_layer(
        Behavior::Delegate,
        Arc::clone(&called),
        Authority::default(),
    );
    let (sender, receiver) = mpsc::channel();
    let handler = Arc::new(
        move |event: &EventEnvelope, _authority: &Authority| -> Result<(), String> {
            sender
                .send(event.clone())
                .map_err(|error| error.to_string())
        },
    );
    kernel
        .events()
        .install_subscriptions([EventSubscription {
            spec: SubscriptionSpec {
                id: SubscriptionId::parse("test/runtime-trace").unwrap(),
                owner: plugin("trace-listener"),
                event_type: runtime_trace_event_type(),
                event_version: RUNTIME_TRACE_EVENT_VERSION,
                dependencies: Vec::new(),
                failure_policy: EventFailurePolicy::FailDelivery,
                required_authority: Authority::default(),
                maximum_authority: Authority::default(),
                kernel_policy_revision: 0,
            },
            handler,
        }])
        .unwrap();

    let input = b"secret-marker";
    assert_eq!(
        kernel
            .invoke(&service(), input, &Authority::default(), None)
            .unwrap(),
        b"layer:terminal:secret-marker"
    );

    let event = receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("runtime trace should be delivered");
    let encoded = String::from_utf8(event.payload.clone()).unwrap();
    assert!(!encoded.contains("secret-marker"));
    let trace: RuntimeTraceEvent = serde_json::from_slice(&event.payload).unwrap();
    let RuntimeTraceEvent::ServiceInvocation {
        service: traced_service,
        input_bytes,
        output_bytes,
        success,
        error,
        terminal_reached,
        participants,
    } = trace;
    assert_eq!(traced_service, service().as_str());
    assert_eq!(input_bytes, input.len());
    assert_eq!(output_bytes, Some(b"layer:terminal:secret-marker".len()));
    assert!(success);
    assert_eq!(error, None);
    assert!(terminal_reached);
    assert_eq!(participants.len(), 2);
    assert_eq!(participants[0].plugin, "layer");
    assert_eq!(participants[0].role, "layer");
    assert_eq!(participants[0].outcome, "delegated");
    assert_eq!(participants[1].plugin, "terminal");
    assert_eq!(participants[1].role, "terminal");
    assert_eq!(participants[1].outcome, "succeeded");
}

#[test]
fn provenance_distinguishes_handle_and_denial_without_terminal_entry() {
    for (behavior, expected) in [
        (Behavior::Handle, crate::ServiceParticipantOutcome::Handled),
        (Behavior::Deny, crate::ServiceParticipantOutcome::Denied),
    ] {
        let called = Arc::new(AtomicBool::new(false));
        let mut kernel = kernel_with_layer(behavior, Arc::clone(&called), Authority::default());
        let _ = kernel.invoke(&service(), b"x", &Authority::default(), None);
        let records = kernel.service_invocation_provenance();
        let record = records.last().expect("invocation provenance recorded");
        assert!(!record.terminal_reached);
        assert_eq!(record.participants.len(), 1);
        assert_eq!(record.participants[0].outcome, expected);
        assert!(!called.load(Ordering::SeqCst));
    }
}

#[test]
fn provenance_records_terminal_failure_without_provider_fallback() {
    let high_called = Arc::new(AtomicBool::new(false));
    let low_called = Arc::new(AtomicBool::new(false));
    let high = manifest("high", ServiceRole::Terminal, 100, Authority::default());
    let low = manifest("low", ServiceRole::Terminal, 1, Authority::default());
    let high_id = high.id.clone();
    let low_id = low.id.clone();
    let high_flag = Arc::clone(&high_called);
    let low_flag = Arc::clone(&low_called);
    let mut kernel = Kernel::new(KernelConfig::new([high, low]).unwrap());
    kernel
        .register_embedded_factory(high_id, move || {
            Box::new(Terminal {
                called: Arc::clone(&high_flag),
                fail: true,
            })
        })
        .unwrap();
    kernel
        .register_embedded_factory(low_id, move || {
            Box::new(Terminal {
                called: Arc::clone(&low_flag),
                fail: false,
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();
    assert!(kernel
        .invoke(&service(), b"x", &Authority::default(), None)
        .is_err());

    let record = kernel
        .service_invocation_provenance()
        .pop()
        .expect("terminal failure provenance recorded");
    assert!(record.terminal_reached);
    assert_eq!(record.participants.len(), 1);
    assert_eq!(record.participants[0].plugin, plugin("high"));
    assert_eq!(
        record.participants[0].outcome,
        crate::ServiceParticipantOutcome::Failed
    );
    assert!(high_called.load(Ordering::SeqCst));
    assert!(!low_called.load(Ordering::SeqCst));
}

#[test]
fn layer_policy_identity_is_pinned_to_the_resolved_configuration_snapshot() {
    let terminal = manifest("terminal", ServiceRole::Terminal, 10, Authority::default());
    let layer = manifest("layer", ServiceRole::Layer, 10, Authority::default());
    let base = KernelConfig::new([terminal.clone(), layer.clone()]).unwrap();
    let base_identity = base.policy_identity();
    let configured = base
        .clone()
        .with_layer_policy(
            service(),
            vec![LayerPolicy {
                plugin: plugin("layer"),
                priority: 20,
                required: false,
                enabled: true,
            }],
        )
        .unwrap();
    assert_eq!(base.policy_identity(), base_identity);
    assert_ne!(configured.policy_identity(), base_identity);

    let chain = configured
        .resolve_chain(&service(), &Authority::default(), None)
        .unwrap();
    assert_eq!(chain.policy_identity, configured.policy_identity());
}

#[test]
fn inactive_optional_layer_is_removed_from_planned_chain() {
    let called = Arc::new(AtomicBool::new(false));
    let layer_manifest = manifest("layer", ServiceRole::Layer, 100, Authority::default());
    let terminal_manifest = manifest("terminal", ServiceRole::Terminal, 1, Authority::default());
    let layer_id = layer_manifest.id.clone();
    let terminal_id = terminal_manifest.id.clone();
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
        .register_embedded_factory(layer_id.clone(), || {
            Box::new(Layer {
                behavior: Behavior::Delegate,
            })
        })
        .unwrap();
    let terminal_called = Arc::clone(&called);
    kernel
        .register_embedded_factory(terminal_id, move || {
            Box::new(Terminal {
                called: Arc::clone(&terminal_called),
                fail: false,
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();
    kernel.stop(&layer_id).unwrap();

    assert_eq!(
        kernel
            .invoke(&service(), b"x", &Authority::default(), None)
            .unwrap(),
        b"terminal:x"
    );
    assert!(called.load(Ordering::SeqCst));
    let record = kernel
        .service_invocation_provenance()
        .pop()
        .expect("eligible chain provenance recorded");
    assert!(record.planned_chain.layers.is_empty());
    assert_eq!(record.participants.len(), 1);
    assert_eq!(record.participants[0].plugin, plugin("terminal"));
}

#[test]
fn inactive_required_layer_fails_before_dispatch() {
    let called = Arc::new(AtomicBool::new(false));
    let layer_manifest = manifest("layer", ServiceRole::Layer, 100, Authority::default());
    let terminal_manifest = manifest("terminal", ServiceRole::Terminal, 1, Authority::default());
    let layer_id = layer_manifest.id.clone();
    let terminal_id = terminal_manifest.id.clone();
    let config = KernelConfig::new([layer_manifest, terminal_manifest])
        .unwrap()
        .with_layer_policy(
            service(),
            vec![LayerPolicy {
                plugin: layer_id.clone(),
                priority: 100,
                required: true,
                enabled: true,
            }],
        )
        .unwrap();
    let mut kernel = Kernel::new(config);
    kernel
        .register_embedded_factory(layer_id.clone(), || {
            Box::new(Layer {
                behavior: Behavior::Delegate,
            })
        })
        .unwrap();
    let terminal_called = Arc::clone(&called);
    kernel
        .register_embedded_factory(terminal_id, move || {
            Box::new(Terminal {
                called: Arc::clone(&terminal_called),
                fail: false,
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();
    kernel.stop(&layer_id).unwrap();

    assert!(matches!(
        kernel.invoke(&service(), b"x", &Authority::default(), None),
        Err(KernelError::RequiredLayerUnavailable { plugin: id, .. }) if id == layer_id
    ));
    assert!(!called.load(Ordering::SeqCst));
    assert!(kernel.service_invocation_provenance().is_empty());
}
