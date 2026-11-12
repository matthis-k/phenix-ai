use phenix_graph::network::{ComponentRef, NetworkBuilder, NetworkDef, NetworkError};
use phenix_graph::process::{Process, ProcessContext, ProcessFactory, Step};
use phenix_graph::{Body, CompileLimits, ComponentDef, GraphOrigin, Interface, PortDef};
use std::cell::Cell;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Noop;
impl Process for Noop {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        Ok(Step::Finished)
    }
}
fn factory() -> Arc<dyn ProcessFactory> {
    Arc::new(|| Noop)
}
fn cap(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).unwrap()
}
fn io() -> Arc<Interface> {
    Arc::new(
        Interface::new(
            [PortDef::local::<i64>("in")],
            [PortDef::local::<i64>("out")],
        )
        .unwrap(),
    )
}
fn primitive() -> Arc<ComponentDef> {
    Arc::new(ComponentDef::primitive(io(), factory()))
}
fn module() -> NetworkDef {
    let mut builder = NetworkBuilder::new();
    let leaf = builder.node("leaf", io(), factory()).unwrap();
    builder
        .expose_input("in", leaf.input::<i64>("in").unwrap(), cap(2))
        .unwrap();
    builder
        .expose_output("out", leaf.output::<i64>("out").unwrap())
        .unwrap();
    builder.build().unwrap()
}
fn source_interface() -> Arc<Interface> {
    Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap())
}
fn paths(network: &phenix_graph::CompiledNetwork) -> Vec<&str> {
    network
        .processes()
        .iter()
        .map(|process| process.path())
        .collect()
}

#[test]
fn node_and_module_constructors_return_the_same_component_handle() {
    fn same_handle(a: ComponentRef, b: ComponentRef) -> [ComponentRef; 2] {
        [a, b]
    }
    let mut builder = NetworkBuilder::new();
    let node = builder.node("node", io(), factory()).unwrap();
    let child = builder.module("module", module()).unwrap();
    let [node, child] = same_handle(node, child);
    builder
        .connect(
            node.output::<i64>("out").unwrap(),
            child.input::<i64>("in").unwrap(),
            cap(3),
        )
        .unwrap();
    builder
        .connect(
            child.output::<i64>("out").unwrap(),
            node.input::<i64>("in").unwrap(),
            cap(4),
        )
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    assert_eq!(paths(&compiled), ["module/leaf", "node"]);
    assert_eq!(compiled.connections().len(), 2);
    assert_eq!(compiled.connections()[0].from(), (0, "out"));
    assert_eq!(compiled.connections()[0].to(), (1, "in"));
}

#[test]
fn zero_input_zero_output_sources_sinks_and_multioutput_are_native() {
    let mut builder = NetworkBuilder::new();
    let empty = builder
        .node("empty", Arc::new(Interface::default()), factory())
        .unwrap();
    let source = builder
        .node(
            "source",
            Arc::new(
                Interface::new(
                    [],
                    [
                        PortDef::local::<i64>("left"),
                        PortDef::local::<String>("right"),
                    ],
                )
                .unwrap(),
            ),
            factory(),
        )
        .unwrap();
    let sink = builder
        .node(
            "sink",
            Arc::new(
                Interface::new(
                    [
                        PortDef::local::<i64>("numbers"),
                        PortDef::local::<String>("text"),
                    ],
                    [],
                )
                .unwrap(),
            ),
            factory(),
        )
        .unwrap();
    builder
        .connect(
            source.output::<i64>("left").unwrap(),
            sink.input::<i64>("numbers").unwrap(),
            cap(1),
        )
        .unwrap();
    builder
        .connect(
            source.output::<String>("right").unwrap(),
            sink.input::<String>("text").unwrap(),
            cap(2),
        )
        .unwrap();
    builder.start(&source).unwrap();
    builder.start(&empty).unwrap();
    assert!(matches!(
        builder.start(&sink),
        Err(NetworkError::InvalidSource(_))
    ));
    assert!(empty.input::<()>("in").is_err());
    assert!(empty.output::<()>("out").is_err());
    let compiled = builder.build().unwrap().compile().unwrap();
    assert!(compiled.interface().inputs().is_empty());
    assert!(compiled.interface().outputs().is_empty());
    assert_eq!(paths(&compiled), ["empty", "sink", "source"]);
    assert_eq!(compiled.source_starts(), [0, 2]);
    assert_eq!(compiled.connections().len(), 2);
}

#[test]
fn empty_network_and_empty_instanced_composite_need_no_dummy_output() {
    let empty = NetworkBuilder::new().build().unwrap();
    assert!(empty.compile().unwrap().processes().is_empty());
    let mut builder = NetworkBuilder::new();
    let child = builder.module("empty", empty).unwrap();
    builder.start(&child).unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    assert!(compiled.processes().is_empty());
    assert!(compiled.connections().is_empty());
    assert!(compiled.outputs().is_empty());
    assert!(compiled.source_starts().is_empty());
}

#[test]
fn nested_repeated_modules_flatten_to_only_their_physical_processes() {
    let definition = module();
    let mut middle = NetworkBuilder::new();
    let a = middle.module("a", definition.clone()).unwrap();
    let b = middle.module("b", definition).unwrap();
    middle
        .connect(
            a.output::<i64>("out").unwrap(),
            b.input::<i64>("in").unwrap(),
            cap(5),
        )
        .unwrap();
    middle
        .expose_input("in", a.input::<i64>("in").unwrap(), cap(6))
        .unwrap();
    middle
        .expose_output("out", b.output::<i64>("out").unwrap())
        .unwrap();
    let middle = middle.build().unwrap();
    let mut root = NetworkBuilder::new();
    let first = root.module("first", middle.clone()).unwrap();
    let second = root.module("second", middle).unwrap();
    root.connect(
        first.output::<i64>("out").unwrap(),
        second.input::<i64>("in").unwrap(),
        cap(7),
    )
    .unwrap();
    root.expose_input("request", first.input::<i64>("in").unwrap(), cap(8))
        .unwrap();
    root.expose_output("result", second.output::<i64>("out").unwrap())
        .unwrap();
    let compiled = root.build().unwrap().compile().unwrap();
    assert_eq!(
        paths(&compiled),
        [
            "first/a/leaf",
            "first/b/leaf",
            "second/a/leaf",
            "second/b/leaf"
        ]
    );
    assert_eq!(compiled.connections().len(), 3);
    assert_eq!(compiled.inputs()["request"], [(0, "in".into())]);
    assert_eq!(compiled.outputs()["result"], (3, "out".into()));
    assert_eq!(compiled.ingress_capacities()["request"], cap(8));
    assert_eq!(
        compiled
            .connections()
            .iter()
            .map(|edge| edge.capacity().get())
            .collect::<Vec<_>>(),
        [5, 7, 5]
    );
}

#[test]
fn replacement_in_both_body_directions_preserves_edges_and_exports_and_revokes_handles() {
    for composite_first in [false, true] {
        let composite = Arc::new(ComponentDef::composite(module()));
        let initial = if composite_first {
            composite.clone()
        } else {
            primitive()
        };
        let replacement = if composite_first {
            primitive()
        } else {
            composite
        };
        let mut builder = NetworkBuilder::new();
        let before = builder
            .node("before", source_interface(), factory())
            .unwrap();
        let target = builder.add("target", initial).unwrap();
        let after = builder.node("after", io(), factory()).unwrap();
        builder
            .connect(
                before.output::<i64>("out").unwrap(),
                target.input::<i64>("in").unwrap(),
                cap(3),
            )
            .unwrap();
        builder
            .connect(
                target.output::<i64>("out").unwrap(),
                after.input::<i64>("in").unwrap(),
                cap(4),
            )
            .unwrap();
        builder
            .expose_input("request", target.input::<i64>("in").unwrap(), cap(2))
            .unwrap();
        builder
            .expose_output("result", target.output::<i64>("out").unwrap())
            .unwrap();
        let old_input = target.input::<i64>("in").unwrap();
        let old_output = target.output::<i64>("out").unwrap();
        let new = builder.replace(&target, replacement).unwrap();
        assert!(matches!(
            target.input::<i64>("in"),
            Err(NetworkError::StaleHandle)
        ));
        assert!(matches!(
            target.output::<i64>("out"),
            Err(NetworkError::StaleHandle)
        ));
        assert!(matches!(
            builder.expose_input("bad", old_input, cap(2)),
            Err(NetworkError::StaleHandle)
        ));
        assert!(matches!(
            builder.expose_output("bad", old_output),
            Err(NetworkError::StaleHandle)
        ));
        assert!(matches!(
            builder.replace(&target, primitive()),
            Err(NetworkError::StaleHandle)
        ));
        assert!(new.input::<i64>("in").is_ok());
        assert!(new.output::<i64>("out").is_ok());
        assert!(before.output::<i64>("out").is_ok());
        assert!(after.input::<i64>("in").is_ok());
        let network = builder.build().unwrap();
        assert_eq!(network.connections()[0].to.process, "target");
        assert_eq!(network.connections()[1].from.process, "target");
        assert_eq!(
            network.input_projections()["request"].targets[0].process,
            "target"
        );
        assert_eq!(network.output_projections()["result"].process, "target");
        let compiled = network.compile().unwrap();
        assert_eq!(compiled.connections().len(), 2);
        let target_path = if composite_first {
            "target"
        } else {
            "target/leaf"
        };
        assert_eq!(
            compiled.processes()[compiled.outputs()["result"].0].path(),
            target_path
        );
    }
}

#[test]
fn failed_replacement_is_atomic_and_does_not_revoke_current_handles() {
    let mut builder = NetworkBuilder::new();
    let original = builder.add("worker", primitive()).unwrap();
    let incompatible = Arc::new(ComponentDef::primitive(source_interface(), factory()));
    assert!(matches!(
        builder.replace(&original, incompatible),
        Err(NetworkError::InterfaceMismatch(_))
    ));
    assert!(original.input::<i64>("in").is_ok());
    assert!(original.output::<i64>("out").is_ok());
    builder
        .expose_output("result", original.output::<i64>("out").unwrap())
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    assert_eq!(paths(&compiled), ["worker"]);
}

#[test]
fn builder_scope_is_checked_for_connect_export_start_and_replacement() {
    let mut a = NetworkBuilder::new();
    let mut b = NetworkBuilder::new();
    let a_node = a.node("same", io(), factory()).unwrap();
    let b_node = b.node("same", io(), factory()).unwrap();
    assert!(matches!(
        b.connect(
            a_node.output::<i64>("out").unwrap(),
            b_node.input::<i64>("in").unwrap(),
            cap(1)
        ),
        Err(NetworkError::ForeignScope)
    ));
    assert!(matches!(
        b.expose_input("request", a_node.input::<i64>("in").unwrap(), cap(1)),
        Err(NetworkError::ForeignScope)
    ));
    assert!(matches!(
        b.expose_output("result", a_node.output::<i64>("out").unwrap()),
        Err(NetworkError::ForeignScope)
    ));
    assert!(matches!(b.start(&a_node), Err(NetworkError::ForeignScope)));
    assert!(matches!(
        b.replace(&a_node, primitive()),
        Err(NetworkError::ForeignScope)
    ));
    let b = b.build().unwrap();
    assert!(b.connections().is_empty());
    assert!(b.interface().inputs().is_empty());
    assert!(b.interface().outputs().is_empty());
}

#[test]
fn immutable_definition_branches_have_independent_authoring_revisions() {
    let mut initial = NetworkBuilder::new();
    let old = initial.add("worker", primitive()).unwrap();
    let definition = initial.build().unwrap();
    let mut a = NetworkBuilder::from_def(&definition);
    let mut b = NetworkBuilder::from_def(&definition);
    let a_old = a.component("worker").unwrap();
    let b_old = b.component("worker").unwrap();
    assert!(matches!(
        a.replace(&old, primitive()),
        Err(NetworkError::ForeignScope)
    ));
    a.replace(&a_old, Arc::new(ComponentDef::composite(module())))
        .unwrap();
    assert!(a_old.output::<i64>("out").is_err());
    assert!(b_old.output::<i64>("out").is_ok());
    b.replace(&b_old, primitive()).unwrap();
    assert!(matches!(
        definition.instances()["worker"].body(),
        Body::Primitive(_)
    ));
    assert_eq!(
        paths(&a.build().unwrap().compile().unwrap()),
        ["worker/leaf"]
    );
    assert_eq!(paths(&b.build().unwrap().compile().unwrap()), ["worker"]);
}

#[test]
fn public_input_fanout_and_projection_aliases_preserve_capacities() {
    let mut child = NetworkBuilder::new();
    let a = child.add("a", primitive()).unwrap();
    let b = child.add("b", primitive()).unwrap();
    child
        .expose_input("both", a.input::<i64>("in").unwrap(), cap(2))
        .unwrap();
    child
        .expose_input("both", b.input::<i64>("in").unwrap(), cap(2))
        .unwrap();
    child
        .expose_input("alias", a.input::<i64>("in").unwrap(), cap(4))
        .unwrap();
    child
        .expose_output("one", a.output::<i64>("out").unwrap())
        .unwrap();
    child
        .expose_output("alias", a.output::<i64>("out").unwrap())
        .unwrap();
    let mut root = NetworkBuilder::new();
    let source = root.node("source", source_interface(), factory()).unwrap();
    let child = root.module("child", child.build().unwrap()).unwrap();
    root.connect(
        source.output::<i64>("out").unwrap(),
        child.input::<i64>("both").unwrap(),
        cap(3),
    )
    .unwrap();
    root.expose_input("request", child.input::<i64>("both").unwrap(), cap(7))
        .unwrap();
    root.expose_input("single", child.input::<i64>("alias").unwrap(), cap(9))
        .unwrap();
    root.expose_output("result", child.output::<i64>("one").unwrap())
        .unwrap();
    root.expose_output("alias", child.output::<i64>("alias").unwrap())
        .unwrap();
    let compiled = root.build().unwrap().compile().unwrap();
    assert_eq!(
        compiled.inputs()["request"],
        [(0, "in".into()), (1, "in".into())]
    );
    assert_eq!(compiled.inputs()["single"], [(0, "in".into())]);
    assert_eq!(compiled.ingress_capacities()["request"], cap(7));
    assert_eq!(compiled.ingress_capacities()["single"], cap(9));
    assert_eq!(compiled.outputs()["result"], compiled.outputs()["alias"]);
    assert_eq!(compiled.connections().len(), 2);
    assert!(
        compiled
            .connections()
            .iter()
            .all(|edge| edge.capacity() == cap(3))
    );
}

#[test]
fn conflicting_projections_reject_without_overwriting_the_first_declaration() {
    let mut builder = NetworkBuilder::new();
    let a = builder.add("a", primitive()).unwrap();
    let b = builder.add("b", primitive()).unwrap();
    builder
        .expose_input("request", a.input::<i64>("in").unwrap(), cap(1))
        .unwrap();
    assert!(
        builder
            .expose_input("request", a.input::<i64>("in").unwrap(), cap(1))
            .is_err()
    );
    assert!(
        builder
            .expose_input("request", b.input::<i64>("in").unwrap(), cap(2))
            .is_err()
    );
    builder
        .expose_output("result", a.output::<i64>("out").unwrap())
        .unwrap();
    assert!(
        builder
            .expose_output("result", b.output::<i64>("out").unwrap())
            .is_err()
    );
    let definition = builder.build().unwrap();
    assert_eq!(definition.input_projections()["request"].targets.len(), 1);
    assert_eq!(definition.output_projections()["result"].process, "a");
}

#[test]
fn physical_alias_collisions_are_rejected_after_projection_expansion() {
    for connection in [false, true] {
        let mut child = NetworkBuilder::new();
        let leaf = child.add("leaf", primitive()).unwrap();
        for name in ["a", "b"] {
            child
                .expose_input(name, leaf.input::<i64>("in").unwrap(), cap(1))
                .unwrap();
        }
        let mut root = NetworkBuilder::new();
        let child = root.module("child", child.build().unwrap()).unwrap();
        if connection {
            let source = root.node("source", source_interface(), factory()).unwrap();
            for port in ["a", "b"] {
                root.connect(
                    source.output::<i64>("out").unwrap(),
                    child.input::<i64>(port).unwrap(),
                    cap(1),
                )
                .unwrap();
            }
        } else {
            for port in ["a", "b"] {
                root.expose_input("both", child.input::<i64>(port).unwrap(), cap(1))
                    .unwrap();
            }
        }
        let result = root.build().unwrap().compile();
        if connection {
            assert!(matches!(
                result,
                Err(NetworkError::DuplicateConnection { .. })
            ));
        } else {
            assert!(matches!(result, Err(NetworkError::InvalidProjection(_))));
        }
    }
}

#[test]
fn explicit_sources_start_only_selected_roots_even_in_sink_only_composites() {
    let mut child = NetworkBuilder::new();
    let selected = child
        .node("selected", Arc::new(Interface::default()), factory())
        .unwrap();
    child
        .node("disconnected", Arc::new(Interface::default()), factory())
        .unwrap();
    child.start(&selected).unwrap();
    let mut root = NetworkBuilder::new();
    let child = root.module("child", child.build().unwrap()).unwrap();
    root.node(
        "also_disconnected",
        Arc::new(Interface::default()),
        factory(),
    )
    .unwrap();
    let unstarted = root.build().unwrap();
    assert!(unstarted.compile().unwrap().source_starts().is_empty());
    let mut root = NetworkBuilder::from_def(&unstarted);
    assert!(matches!(
        root.start(&child),
        Err(NetworkError::ForeignScope)
    ));
    root.start(&root.component("child").unwrap()).unwrap();
    let compiled = root.build().unwrap().compile().unwrap();
    assert_eq!(
        paths(&compiled),
        ["also_disconnected", "child/disconnected", "child/selected"]
    );
    assert_eq!(compiled.source_starts(), [2]);
    assert!(compiled.interface().outputs().is_empty());
}

#[test]
fn physical_data_cycles_and_self_feedback_are_not_recursive_definitions() {
    let mut builder = NetworkBuilder::new();
    let a = builder.add("a", primitive()).unwrap();
    let b = builder.add("b", primitive()).unwrap();
    builder
        .connect(
            a.output::<i64>("out").unwrap(),
            a.input::<i64>("in").unwrap(),
            cap(1),
        )
        .unwrap();
    builder
        .connect(
            a.output::<i64>("out").unwrap(),
            b.input::<i64>("in").unwrap(),
            cap(2),
        )
        .unwrap();
    builder
        .connect(
            b.output::<i64>("out").unwrap(),
            a.input::<i64>("in").unwrap(),
            cap(3),
        )
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    assert_eq!(compiled.processes().len(), 2);
    assert_eq!(compiled.connections().len(), 3);
    assert!(compiled.source_starts().is_empty());
}

#[test]
fn process_ids_connections_and_exports_are_deterministic_by_full_path() {
    let mut results = Vec::new();
    for reversed in [false, true] {
        let mut builder = NetworkBuilder::new();
        let mut nested = NetworkBuilder::new();
        let x = nested.add("x", primitive()).unwrap();
        nested
            .expose_input("in", x.input::<i64>("in").unwrap(), cap(1))
            .unwrap();
        nested
            .expose_output("out", x.output::<i64>("out").unwrap())
            .unwrap();
        let nested = Arc::new(ComponentDef::composite(nested.build().unwrap()));
        let names = if reversed {
            ["z", "a-", "a"]
        } else {
            ["a", "a-", "z"]
        };
        for name in names {
            builder
                .add(
                    name,
                    if name == "a" {
                        nested.clone()
                    } else {
                        primitive()
                    },
                )
                .unwrap();
        }
        let a = builder.component("a").unwrap();
        let b = builder.component("a-").unwrap();
        let z = builder.component("z").unwrap();
        let edges = if reversed {
            [(&b, &z), (&a, &b)]
        } else {
            [(&a, &b), (&b, &z)]
        };
        for (from, to) in edges {
            builder
                .connect(
                    from.output::<i64>("out").unwrap(),
                    to.input::<i64>("in").unwrap(),
                    cap(2),
                )
                .unwrap();
        }
        builder
            .expose_input("request", a.input::<i64>("in").unwrap(), cap(4))
            .unwrap();
        builder
            .expose_output("result", z.output::<i64>("out").unwrap())
            .unwrap();
        results.push(builder.build().unwrap().compile().unwrap());
    }
    assert_eq!(paths(&results[0]), ["a-", "a/x", "z"]);
    assert_eq!(paths(&results[0]), paths(&results[1]));
    assert_eq!(results[0].connections(), results[1].connections());
    assert_eq!(results[0].inputs(), results[1].inputs());
    assert_eq!(results[0].outputs(), results[1].outputs());
}

#[test]
fn compile_limits_bound_depth_instances_including_empty_modules_and_work() {
    let mut builder = NetworkBuilder::new();
    builder.module("nested", module()).unwrap();
    let definition = builder.build().unwrap();
    assert!(matches!(
        definition.compile_with_limits(CompileLimits {
            max_depth: 1,
            ..CompileLimits::default()
        }),
        Err(NetworkError::LimitExceeded {
            limit: "max_depth",
            ..
        })
    ));
    assert!(
        definition
            .compile_with_limits(CompileLimits {
                max_depth: 2,
                ..CompileLimits::default()
            })
            .is_ok()
    );
    assert!(matches!(
        definition.compile_with_limits(CompileLimits {
            max_instances: 1,
            ..CompileLimits::default()
        }),
        Err(NetworkError::LimitExceeded {
            limit: "max_instances",
            ..
        })
    ));
    assert!(matches!(
        definition.compile_with_limits(CompileLimits {
            max_work: 0,
            ..CompileLimits::default()
        }),
        Err(NetworkError::LimitExceeded {
            limit: "max_work",
            ..
        })
    ));
    let mut builder = NetworkBuilder::new();
    for name in ["a", "b", "c"] {
        builder
            .module(name, NetworkBuilder::new().build().unwrap())
            .unwrap();
    }
    assert!(matches!(
        builder.build().unwrap().compile_with_limits(CompileLimits {
            max_instances: 2,
            ..CompileLimits::default()
        }),
        Err(NetworkError::LimitExceeded {
            limit: "max_instances",
            ..
        })
    ));
}

struct Counter(Cell<usize>);
impl Process for Counter {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        self.0.set(self.0.get() + 1);
        if self.0.get() == 1 {
            Ok(Step::Yield)
        } else {
            Ok(Step::Finished)
        }
    }
}

#[test]
fn compilation_never_spawns_and_reused_factories_produce_independent_mutable_processes() {
    let spawns = Arc::new(AtomicUsize::new(0));
    let calls = spawns.clone();
    let factory: Arc<dyn ProcessFactory> = Arc::new(move || {
        calls.fetch_add(1, Ordering::SeqCst);
        Counter(Cell::new(0))
    });
    let definition = Arc::new(ComponentDef::primitive(
        Arc::new(Interface::default()),
        factory,
    ));
    let mut builder = NetworkBuilder::new();
    builder.add("a", definition.clone()).unwrap();
    builder.add("b", definition).unwrap();
    let network = builder.build().unwrap();
    let compiled = network.compile().unwrap();
    network.compile().unwrap();
    assert_eq!(spawns.load(Ordering::SeqCst), 0);
    let mut processes: Vec<_> = (0..2)
        .flat_map(|_| {
            compiled
                .processes()
                .iter()
                .map(|process| process.factory().spawn())
        })
        .collect();
    assert_eq!(spawns.load(Ordering::SeqCst), 4);
    let mut context = ProcessContext::default();
    for process in &mut processes {
        assert!(matches!(process.step(&mut context).unwrap(), Step::Yield));
    }
    for process in &mut processes {
        assert!(matches!(
            process.step(&mut context).unwrap(),
            Step::Finished
        ));
    }
}

#[test]
fn origin_is_preserved_with_explicit_child_override_and_is_not_a_claim() {
    let parent_origin = GraphOrigin::default();
    let leaf_origin = GraphOrigin::default();
    let mut child = NetworkBuilder::new();
    child.add("inherited", primitive()).unwrap();
    child
        .add(
            "explicit",
            Arc::new(ComponentDef::primitive(io(), factory()).with_origin(leaf_origin.clone())),
        )
        .unwrap();
    let definition =
        ComponentDef::composite(child.build().unwrap()).with_origin(parent_origin.clone());
    assert!(definition.claims().is_empty());
    let mut root = NetworkBuilder::new();
    root.add("child", Arc::new(definition)).unwrap();
    let compiled = root.build().unwrap().compile().unwrap();
    assert_eq!(compiled.processes()[0].origin(), Some(&leaf_origin));
    assert_eq!(compiled.processes()[1].origin(), Some(&parent_origin));
    assert_ne!(parent_origin, leaf_origin);
}

#[test]
fn definitions_are_send_sync_and_ports_cannot_be_mutated_through_public_views() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<ComponentDef>();
    send_sync::<NetworkDef>();
    send_sync::<phenix_graph::CompiledNetwork>();
    let definition = primitive();
    let mut detached = definition.interface().inputs()["in"].clone();
    detached.name = "changed".into();
    assert!(definition.interface().input("in").is_some());
    assert!(definition.interface().input("changed").is_none());
}

#[test]
fn duplicate_instances_invalid_process_names_and_duplicate_edges_fail_without_mutation() {
    let mut builder = NetworkBuilder::new();
    for name in ["", ".", "..", "nested/name", "bad\nname"] {
        assert!(builder.add(name, primitive()).is_err());
    }
    let node = builder.add("node", primitive()).unwrap();
    assert!(matches!(
        builder.add("node", primitive()),
        Err(NetworkError::DuplicateInstance(_))
    ));
    builder
        .connect(
            node.output::<i64>("out").unwrap(),
            node.input::<i64>("in").unwrap(),
            cap(1),
        )
        .unwrap();
    assert!(matches!(
        builder.connect(
            node.output::<i64>("out").unwrap(),
            node.input::<i64>("in").unwrap(),
            cap(2)
        ),
        Err(NetworkError::DuplicateConnection { .. })
    ));
    let network = builder.build().unwrap();
    assert_eq!(network.instances().len(), 1);
    assert_eq!(network.connections().len(), 1);
}

#[test]
fn zero_output_source_replacement_preserves_explicit_start_and_uses_new_origin() {
    for composite_first in [false, true] {
        let mut inner = NetworkBuilder::new();
        let selected = inner
            .node("selected", Arc::new(Interface::default()), factory())
            .unwrap();
        inner
            .node("unselected", Arc::new(Interface::default()), factory())
            .unwrap();
        inner.start(&selected).unwrap();
        let composite = ComponentDef::composite(inner.build().unwrap());
        let primitive = ComponentDef::primitive(Arc::new(Interface::default()), factory());
        let (initial, replacement) = if composite_first {
            (composite, primitive)
        } else {
            (primitive, composite)
        };
        let old_origin = GraphOrigin::default();
        let new_origin = GraphOrigin::default();
        let mut builder = NetworkBuilder::new();
        let old = builder
            .add("source", Arc::new(initial.with_origin(old_origin.clone())))
            .unwrap();
        builder.start(&old).unwrap();
        builder
            .replace(&old, Arc::new(replacement.with_origin(new_origin.clone())))
            .unwrap();
        assert!(matches!(
            builder.start(&old),
            Err(NetworkError::StaleHandle)
        ));
        let network = builder.build().unwrap();
        assert_eq!(network.source_starts(), ["source"]);
        assert_eq!(network.instances()["source"].origin(), Some(&new_origin));
        let compiled = network.compile().unwrap();
        assert_eq!(compiled.source_starts(), [0]);
        assert_eq!(
            compiled.processes()[0].path(),
            if composite_first {
                "source"
            } else {
                "source/selected"
            }
        );
        assert_eq!(compiled.processes()[0].origin(), Some(&new_origin));
        assert_ne!(compiled.processes()[0].origin(), Some(&old_origin));
        assert!(compiled.outputs().is_empty());
    }
}

#[test]
fn compile_work_limit_accounts_for_expanded_connection_fanout() {
    let mut child = NetworkBuilder::new();
    for index in 0..32 {
        let leaf = child.add(format!("leaf_{index:02}"), primitive()).unwrap();
        child
            .expose_input("both", leaf.input::<i64>("in").unwrap(), cap(1))
            .unwrap();
    }
    let mut root = NetworkBuilder::new();
    let fanout = root.module("fanout", child.build().unwrap()).unwrap();
    let source = root.node("source", source_interface(), factory()).unwrap();
    root.connect(
        source.output::<i64>("out").unwrap(),
        fanout.input::<i64>("both").unwrap(),
        cap(3),
    )
    .unwrap();
    let network = root.build().unwrap();
    let compiled = network.compile().unwrap();
    assert_eq!(compiled.processes().len(), 33);
    assert_eq!(compiled.connections().len(), 32);
    assert!(matches!(network.compile_with_limits(CompileLimits {
        max_work: 180, ..CompileLimits::default()
    }), Err(NetworkError::LimitExceeded { limit: "max_work", path }) if path.is_empty()));
    assert_eq!(network.compile().unwrap().connections().len(), 32);
}
