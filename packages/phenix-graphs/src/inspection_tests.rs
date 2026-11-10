use super::*;
use crate::*;
use std::sync::atomic::AtomicUsize;

struct Noop(Arc<AtomicUsize>);
impl NodeAction for Noop {
    fn execute(&self, _: InvocationCtx) -> Result<Vec<Emission>, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(vec![])
    }
}
fn ports() -> Vec<Port> {
    vec![Port::all(0, vec![InputSlot::local::<u64>(0).named("value")]).named("run")]
}

#[test]
fn action_lookup_distinguishes_absent_unique_and_shared_allocations_without_execution() {
    let executions = Arc::new(AtomicUsize::new(0));
    let shared: Arc<dyn NodeAction> = Arc::new(Noop(executions.clone()));
    let unique: Arc<dyn NodeAction> = Arc::new(Noop(executions.clone()));
    let absent: Arc<dyn NodeAction> = Arc::new(Noop(executions.clone()));
    let mut graph = GraphBuilder::new("identity");
    for (name, action) in [("first", &shared), ("second", &shared), ("unique", &unique)] {
        graph
            .add(
                name,
                NodeTemplate::with_dyn_action(
                    0,
                    ports(),
                    OutputSlot::local::<()>(0),
                    action.clone(),
                ),
            )
            .unwrap();
    }
    let compiled = graph.compile().unwrap();
    let node_id = |name| match compiled.find(name).unwrap() {
        FindResult::Node(node) => node.id(),
        _ => panic!("expected node"),
    };
    let engine = Deployment::new(compiled.clone()).build().unwrap();
    assert!(engine.node_ids_for_action(&absent).is_empty());
    assert_eq!(engine.node_ids_for_action(&unique), vec![node_id("unique")]);
    assert_eq!(
        engine.node_ids_for_action(&shared),
        vec![node_id("first"), node_id("second")]
    );
    assert_eq!(executions.load(Ordering::SeqCst), 0);
}

#[test]
fn factory_action_lookup_matches_only_the_actual_instance_in_its_own_engine() {
    let factories = Arc::new(AtomicUsize::new(0));
    let executions = Arc::new(AtomicUsize::new(0));
    let (calls, runs) = (factories.clone(), executions.clone());
    let mut graph = GraphBuilder::new("factory");
    graph
        .add(
            "node",
            NodeTemplate::with_action_factory(0, ports(), OutputSlot::local::<()>(0), move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Noop(runs.clone())
            }),
        )
        .unwrap();
    let compiled = graph.compile().unwrap();
    assert_eq!(factories.load(Ordering::SeqCst), 0);
    let first = Deployment::new(compiled.clone()).build().unwrap();
    let second = Deployment::new(compiled).build().unwrap();
    let (&id, action) = first.instances.iter().next().unwrap();
    let other = &second.instances[&id];
    assert_eq!(first.node_ids_for_action(action), vec![id.get()]);
    assert!(second.node_ids_for_action(action).is_empty());
    assert_eq!(second.node_ids_for_action(other), vec![id.get()]);
    assert!(first.node_ids_for_action(other).is_empty());
    assert_eq!(factories.load(Ordering::SeqCst), 2);
    assert_eq!(executions.load(Ordering::SeqCst), 0);
}

#[test]
fn trigger_inspection_preserves_value_arity_and_physical_fanout_without_admission() {
    let executions = Arc::new(AtomicUsize::new(0));
    let mut root = ModuleDefBuilder::new("routes");
    let addresses: Vec<_> = [1, 2]
        .into_iter()
        .map(|id| {
            root.node(NodeTemplate::with_dyn_action(
                id,
                ports(),
                OutputSlot::local::<()>(0),
                Arc::new(Noop(executions.clone())),
            ));
            InputAddr {
                node: NodeId(id),
                port: PortId(0),
                slot: SlotId(0),
            }
        })
        .collect();
    for (id, targets) in [vec![addresses[0]], vec![addresses[1]], addresses]
        .into_iter()
        .enumerate()
    {
        root.input_with_identity(
            id as u16,
            format!("input{id}"),
            type_name::<u64>().into(),
            Some(TypeId::of::<u64>()),
            targets.into_iter().map(ModuleInputTarget::Node).collect(),
        );
    }
    root.trigger_named(1, "single", vec![ModuleTriggerTarget::PublicInput(0)]);
    root.trigger_named(
        2,
        "multi",
        vec![
            ModuleTriggerTarget::PublicInput(0),
            ModuleTriggerTarget::PublicInput(1),
        ],
    );
    root.trigger_named(3, "fanout", vec![ModuleTriggerTarget::PublicInput(2)]);
    let mut catalog = ModuleCatalog::new();
    catalog.add(root.build()).unwrap();
    let engine = Deployment::new(catalog.compile("routes").unwrap())
        .build()
        .unwrap();
    assert!(engine.trigger_targets("missing").is_none());
    let single = engine.trigger_targets("single").unwrap();
    let multi = engine.trigger_targets("multi").unwrap();
    let fanout = engine.trigger_targets("fanout").unwrap();
    assert_eq!(single.len(), 1);
    assert_eq!(single[0].len(), 1);
    assert_eq!(multi.len(), 2);
    assert!(multi.iter().all(|targets| targets.len() == 1));
    assert_eq!(single[0], multi[0]);
    assert_eq!(fanout.len(), 1);
    assert_eq!(fanout[0], multi.concat());
    assert_eq!(executions.load(Ordering::SeqCst), 0);
    let report = engine.fire("single", vec![Value::local(1u64)]).unwrap();
    assert!(report.success());
    assert_eq!(report.wave_id(), 1);
    assert_eq!(executions.load(Ordering::SeqCst), 1);
}
