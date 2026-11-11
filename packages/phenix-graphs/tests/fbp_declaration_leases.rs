use phenix_graph::{
    Value,
    contract::Contract,
    interface::{Interface, PortDef},
    network::{CompiledNetwork, NetworkBuilder},
    process::{Process, ProcessContext, ProcessEmission, ProcessFactory, Step},
    runtime::SessionOptions,
};
use std::sync::{Arc, Mutex};

type Events = Arc<Mutex<Vec<&'static str>>>;

struct Probe {
    events: Events,
    name: &'static str,
}
impl Drop for Probe {
    fn drop(&mut self) {
        let mut events = self.events.lock().unwrap();
        if self.name != "lease" {
            assert!(!events.contains(&"lease"), "{} outlived code", self.name);
        }
        events.push(self.name);
    }
}
struct Factory(Probe);
impl ProcessFactory for Factory {
    fn spawn(&self) -> Box<dyn Process> {
        assert!(!self.0.events.lock().unwrap().contains(&"lease"));
        Box::new(Source {
            probe: Probe {
                events: self.0.events.clone(),
                name: "process",
            },
            emitted: false,
        })
    }
}
struct Source {
    probe: Probe,
    emitted: bool,
}
impl Process for Source {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        assert!(!self.probe.events.lock().unwrap().contains(&"lease"));
        if self.emitted {
            return Ok(Step::Finished);
        }
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new(
            "out",
            Value::local(Probe {
                events: self.probe.events.clone(),
                name: "payload",
            }),
        )))
    }
}
fn declaration(events: &Events) -> CompiledNetwork {
    let mut builder = NetworkBuilder::new();
    let source = builder
        .node(
            "source",
            Arc::new(Interface::new([], [PortDef::local::<Probe>("out")]).unwrap()),
            Arc::new(Factory(Probe {
                events: events.clone(),
                name: "factory",
            })),
        )
        .unwrap();
    builder.start(&source).unwrap();
    builder
        .expose_output("out", source.output::<Probe>("out").unwrap())
        .unwrap();
    let contract = Contract::new(
        "test/source",
        Arc::new(Interface::new([], [PortDef::local::<Probe>("out")]).unwrap()),
    )
    .unwrap();
    builder.claim(&contract).unwrap();
    builder.build().unwrap().compile().unwrap()
}
fn attach(compiled: &CompiledNetwork, events: &Events) -> CompiledNetwork {
    compiled.with_lease(Arc::new(Probe {
        events: events.clone(),
        name: "lease",
    }))
}

#[test]
fn detached_compiled_clone_preserves_identity_and_factory_drop_order() {
    let events = Events::default();
    let original = declaration(&events);
    let snapshot = original.inspect("source").unwrap();
    let contract = Contract::new("test/source", original.interface().clone()).unwrap();
    let bound = original
        .bind("", &contract)
        .unwrap()
        .output::<Probe>("out")
        .unwrap();
    let leased = attach(&original, &events);
    assert!(bound.resolve(&leased).is_ok());
    assert_eq!(
        leased.inspect("source").unwrap().definition_key,
        snapshot.definition_key
    );
    let detached = leased.clone();
    drop(original);
    drop(leased);
    assert!(events.lock().unwrap().is_empty());
    drop(detached);
    assert_eq!(*events.lock().unwrap(), ["factory", "lease"]);
    // Metadata-only snapshots neither retain nor execute plugin code.
    assert_eq!(snapshot.path, "source");
}

#[test]
fn detached_factory_retains_code_after_declarations_drop() {
    let events = Events::default();
    let compiled = attach(&declaration(&events), &events);
    let factory = compiled.processes()[0].factory().clone();
    drop(compiled);
    assert!(events.lock().unwrap().is_empty());
    drop(factory);
    assert_eq!(*events.lock().unwrap(), ["factory", "lease"]);
}

#[test]
fn directly_spawned_process_and_its_emission_retain_code() {
    let events = Events::default();
    let compiled = attach(&declaration(&events), &events);
    let factory = compiled.processes()[0].factory().clone();
    let mut process = factory.spawn();
    drop(compiled);
    drop(factory);
    let Step::Emit(emission) = process.step(&mut ProcessContext::default()).unwrap() else {
        panic!("expected emission");
    };
    drop(process);
    assert_eq!(*events.lock().unwrap(), ["factory", "process"]);
    drop(emission);
    assert_eq!(
        *events.lock().unwrap(),
        ["factory", "process", "payload", "lease"]
    );
}

#[test]
fn detached_start_propagates_leases_to_escaped_value_and_typed_payload() {
    for caller_lease in [false, true] {
        let events = Events::default();
        let compiled = attach(&declaration(&events), &events).clone();
        let caller = Arc::new(());
        let weak = Arc::downgrade(&caller);
        let mut session = if caller_lease {
            compiled.start_with_options(SessionOptions {
                lease: Some(caller.clone()),
                ..SessionOptions::default()
            })
        } else {
            compiled.start()
        };
        drop(caller);
        drop(compiled);
        session.run_until_idle(100);
        let packet = session.take_output("out").unwrap().unwrap();
        let value = packet.value().clone();
        let payload = value.leased_shared::<Probe>().unwrap();
        let cloned_payload = payload.clone();
        drop(payload);
        drop(session);
        drop(packet);
        assert!(!events.lock().unwrap().contains(&"lease"));
        drop(value);
        assert!(!events.lock().unwrap().contains(&"lease"));
        assert_eq!(weak.upgrade().is_some(), caller_lease);
        drop(cloned_payload);
        assert_eq!(
            *events.lock().unwrap(),
            ["process", "factory", "payload", "lease"]
        );
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn detached_job_survives_session_cancellation_and_drop() {
    let events = Events::default();
    let compiled = attach(&declaration(&events), &events);
    let mut session = compiled.start();
    drop(compiled);
    let (_, job) = session.next_job().unwrap();
    session.cancel();
    drop(session);
    assert_eq!(*events.lock().unwrap(), ["factory"]);
    let completion = job.run();
    assert!(!events.lock().unwrap().contains(&"lease"));
    drop(completion);
    assert_eq!(
        *events.lock().unwrap(),
        ["factory", "payload", "process", "lease"]
    );
}
