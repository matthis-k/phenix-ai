use phenix_core::{
    Authority, Kernel, KernelConfig, PluginExecution, PluginHost, PluginId, PluginInstance,
    PluginManifest, ResolvedGeneration, ResolvedGenerationActivation, ServiceContribution, ServiceId,
    ServiceRole,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
    time::Duration,
};

const SPAWN_SERVICE: &str = "fixture.concurrency.spawn@1";

#[derive(Default)]
struct GateState {
    started: bool,
    release: bool,
    finished: bool,
}

struct BlockingTaskPlugin {
    gate: Arc<(Mutex<GateState>, Condvar)>,
}

impl PluginInstance for BlockingTaskPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        _input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service.as_str() != SPAWN_SERVICE {
            return Err(format!("unsupported service: {service}"));
        }
        let gate = Arc::clone(&self.gate);
        host.task_scope()
            .ok_or_else(|| "task scope unavailable".to_owned())?
            .spawn(&Authority::default(), move |_cancellation| {
                let (state, changed) = &*gate;
                let mut state = state.lock().unwrap();
                state.started = true;
                changed.notify_all();
                while !state.release {
                    state = changed.wait(state).unwrap();
                }
                state.finished = true;
                changed.notify_all();
            });
        Ok(b"spawned".to_vec())
    }
}

struct StopProbe {
    stopped: Arc<AtomicBool>,
}

impl PluginInstance for StopProbe {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn stop(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        self.stopped.store(true, Ordering::Release);
        Ok(())
    }
}

fn manifest(id: &str, service: Option<ServiceId>) -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(id).unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: service
            .map(|service| ServiceContribution {
                service,
                role: ServiceRole::Terminal,
                priority: 0,
                required_authority: Authority::default(),
            })
            .into_iter()
            .collect(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

#[test]
fn blocked_plugin_task_does_not_stall_unrelated_kernel_transition() {
    let blocking_id = PluginId::parse("fixture.concurrency.blocking").unwrap();
    let probe_id = PluginId::parse("fixture.concurrency.probe").unwrap();
    let service = ServiceId::parse(SPAWN_SERVICE).unwrap();
    let manifests = [
        manifest(blocking_id.as_str(), Some(service.clone())),
        manifest(probe_id.as_str(), None),
    ];
    let resolved = ResolvedGeneration::resolve(manifests.clone(), [], [], &Authority::default())
        .unwrap();
    let mut kernel = Kernel::new(KernelConfig::new(manifests).unwrap());
    let gate = Arc::new((Mutex::new(GateState::default()), Condvar::new()));
    let factory_gate = Arc::clone(&gate);
    kernel
        .register_embedded_factory(blocking_id, move || {
            Box::new(BlockingTaskPlugin {
                gate: Arc::clone(&factory_gate),
            })
        })
        .unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let factory_stopped = Arc::clone(&stopped);
    kernel
        .register_embedded_factory(probe_id.clone(), move || {
            Box::new(StopProbe {
                stopped: Arc::clone(&factory_stopped),
            })
        })
        .unwrap();
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel.activate_all().unwrap();

    assert_eq!(
        kernel
            .invoke(&service, &[], &Authority::default(), None)
            .unwrap(),
        b"spawned"
    );
    let (state, changed) = &*gate;
    let state = state.lock().unwrap();
    let (state, timeout) = changed
        .wait_timeout_while(state, Duration::from_secs(1), |state| !state.started)
        .unwrap();
    assert!(!timeout.timed_out(), "blocking task did not start");
    drop(state);

    let (done, completed) = std::sync::mpsc::sync_channel(1);
    let transition_completed_before_release = thread::scope(|scope| {
        let transition = scope.spawn(|| {
            let result = kernel.stop(&probe_id);
            let _ = done.send(result.as_ref().map(|_| ()).map_err(ToString::to_string));
            result
        });
        let completed_before_release = completed.recv_timeout(Duration::from_secs(1)).is_ok();

        let mut state = state.lock().unwrap();
        state.release = true;
        changed.notify_all();
        while !state.finished {
            state = changed.wait(state).unwrap();
        }
        drop(state);

        transition.join().unwrap().unwrap();
        completed_before_release
    });

    assert!(
        transition_completed_before_release,
        "unrelated kernel stop waited for a blocked plugin task"
    );
    assert!(stopped.load(Ordering::Acquire));
}
