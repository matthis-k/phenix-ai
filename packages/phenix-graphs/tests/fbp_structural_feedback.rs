use phenix_graph::{ComponentDef, network::NetworkBuilder};
use std::{collections::BTreeSet, num::NonZeroUsize, sync::Arc};

fn capacity() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

#[test]
fn compiled_feedback_cycles_include_self_loops_but_not_acyclic_branches() {
    let mut builder = NetworkBuilder::new();
    let primitive = Arc::new(ComponentDef::map(|n: i64| n));
    let a = builder.add("a", primitive.clone()).unwrap();
    let b = builder.add("b", primitive.clone()).unwrap();
    let c = builder.add("c", primitive.clone()).unwrap();
    let d = builder.add("d", primitive.clone()).unwrap();
    builder.add("e", primitive.clone()).unwrap();
    builder
        .connect(
            a.output::<i64>("output").unwrap(),
            b.input::<i64>("input").unwrap(),
            capacity(),
        )
        .unwrap();
    builder
        .connect(
            b.output::<i64>("output").unwrap(),
            a.input::<i64>("input").unwrap(),
            capacity(),
        )
        .unwrap();
    builder
        .connect(
            b.output::<i64>("output").unwrap(),
            c.input::<i64>("input").unwrap(),
            capacity(),
        )
        .unwrap();
    builder
        .connect(
            d.output::<i64>("output").unwrap(),
            d.input::<i64>("input").unwrap(),
            capacity(),
        )
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    let found: BTreeSet<Vec<&str>> = compiled.structural_feedback_cycles().into_iter().collect();
    assert_eq!(found, BTreeSet::from([vec!["a", "b"], vec!["d"]]));
    assert_eq!(compiled.processes().len(), 5);
    assert!(compiled.processes().iter().any(|p| p.path() == "c"));
    assert!(compiled.processes().iter().any(|p| p.path() == "e"));
}

#[test]
fn compiled_dag_has_no_static_feedback_warning() {
    let mut builder = NetworkBuilder::new();
    let primitive = Arc::new(ComponentDef::map(|n: i64| n));
    let a = builder.add("a", primitive.clone()).unwrap();
    let b = builder.add("b", primitive).unwrap();
    builder
        .connect(
            a.output::<i64>("output").unwrap(),
            b.input::<i64>("input").unwrap(),
            capacity(),
        )
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    assert!(compiled.structural_feedback_cycles().is_empty());
}

#[test]
fn first_packet_progress_distinguishes_seeded_and_unseeded_feedback() {
    use phenix_graph::{
        Interface, PortDef, Process, ProcessFactory, network::StructuralPacketProgress as Progress,
        runtime::Readiness,
    };
    use std::collections::BTreeMap;

    struct NeverCalled;
    impl ProcessFactory for NeverCalled {
        fn spawn(&self) -> Box<dyn Process> {
            panic!("structural inspection must not spawn a process")
        }

        fn readiness(&self) -> Readiness {
            panic!("structural inspection must not invoke a policy factory")
        }
    }

    let mut builder = NetworkBuilder::new();
    let join = builder
        .node(
            "join",
            Arc::new(
                Interface::new(
                    ["seed", "feedback"].map(PortDef::local::<i64>),
                    [PortDef::local::<i64>("out")],
                )
                .unwrap(),
            ),
            Arc::new(NeverCalled),
        )
        .unwrap();
    let relay = builder
        .add("relay", Arc::new(ComponentDef::map(|n: i64| n)))
        .unwrap();
    builder
        .expose_input("seed", join.input::<i64>("seed").unwrap(), capacity())
        .unwrap();
    builder
        .connect(
            join.output::<i64>("out").unwrap(),
            relay.input::<i64>("input").unwrap(),
            capacity(),
        )
        .unwrap();
    builder
        .connect(
            relay.output::<i64>("output").unwrap(),
            join.input::<i64>("feedback").unwrap(),
            capacity(),
        )
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    let mut policies = BTreeMap::from([
        ("join".into(), Readiness::ALL),
        ("relay".into(), Readiness::ANY),
    ]);
    // A topological ingress path exists, but All requires feedback before
    // either process can produce its first packet.
    assert_eq!(
        compiled.structural_packet_progress(&policies),
        BTreeMap::from([("join", Progress::Blocked), ("relay", Progress::Blocked)])
    );
    policies.insert("join".into(), Readiness::ANY);
    assert_eq!(
        compiled.structural_packet_progress(&policies),
        BTreeMap::from([
            ("join", Progress::Reachable),
            ("relay", Progress::Reachable),
        ])
    );
    policies.remove("relay");
    policies.insert("join".into(), Readiness::ALL);
    assert_eq!(
        compiled.structural_packet_progress(&policies),
        BTreeMap::from([("join", Progress::Unknown), ("relay", Progress::Unknown)])
    );
}

#[test]
fn first_packet_progress_counts_distinct_input_ports_not_producers() {
    use phenix_graph::{
        Interface, PortDef, Process, ProcessContext, Step,
        network::StructuralPacketProgress as Progress, runtime::Readiness,
    };
    use std::collections::BTreeMap;

    struct Sink;
    impl Process for Sink {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
            panic!("analysis must not run a process")
        }
    }

    let mut builder = NetworkBuilder::new();
    let join = builder
        .node(
            "join",
            Arc::new(Interface::new(["a", "b"].map(PortDef::local::<i64>), []).unwrap()),
            Arc::new(|| Sink),
        )
        .unwrap();
    for name in ["first", "second"] {
        let source = builder
            .add(
                name,
                Arc::new(ComponentDef::source(|| std::iter::once(1_i64))),
            )
            .unwrap();
        builder.start(&source).unwrap();
        builder
            .connect(
                source.output::<i64>("output").unwrap(),
                join.input::<i64>("a").unwrap(),
                capacity(),
            )
            .unwrap();
    }
    let compiled = builder.build().unwrap().compile().unwrap();
    let policies = BTreeMap::from([(
        "join".into(),
        Readiness::at_least(NonZeroUsize::new(2).unwrap()),
    )]);
    let result = compiled.structural_packet_progress(&policies);
    assert_eq!(result["first"], Progress::Reachable);
    assert_eq!(result["second"], Progress::Reachable);
    assert_eq!(result["join"], Progress::Blocked);
}

#[test]
fn structural_packet_progress_matches_exhaustive_activation_orders() {
    use phenix_graph::{
        Interface, PortDef, Process, ProcessContext, Step,
        network::StructuralPacketProgress as Progress, runtime::Readiness,
    };
    use std::collections::BTreeMap;

    struct Idle;
    impl Process for Idle {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
            unreachable!("structural analysis is passive")
        }
    }

    // Each edge supplies the target's feedback port; external ingress
    // supplies its distinct seed port. Enumerate legal first activations
    // independently of the production worklist and fixed-point algorithm.
    fn outcomes(edges: usize, seeds: usize, policies: [usize; 2], unknown: bool) -> usize {
        fn explore(
            active: usize,
            edges: usize,
            seeds: usize,
            policies: [usize; 2],
            unknown: bool,
        ) -> usize {
            let mut result = active;
            for (node, policy) in policies.into_iter().enumerate() {
                if active & (1 << node) != 0 {
                    continue;
                }
                let feedback = (0..2).any(|source| {
                    active & (1 << source) != 0 && edges & (1 << (source * 2 + node)) != 0
                });
                let count = usize::from(seeds & (1 << node) != 0) + usize::from(feedback);
                if (policy == 0 && unknown) || (policy != 0 && count >= policy) {
                    result |= explore(active | (1 << node), edges, seeds, policies, unknown);
                }
            }
            result
        }
        explore(0, edges, seeds, policies, unknown)
    }

    let names = ["a", "b"];
    let interface = Arc::new(
        Interface::new(
            ["seed", "feedback"].map(PortDef::local::<i64>),
            [PortDef::local::<i64>("out")],
        )
        .unwrap(),
    );
    for edges in 0..16 {
        for seeds in 0..4 {
            let mut builder = NetworkBuilder::new();
            let nodes = names.map(|name| {
                builder
                    .node(name, interface.clone(), Arc::new(|| Idle))
                    .unwrap()
            });
            for (target, node) in nodes.iter().enumerate() {
                if seeds & (1 << target) != 0 {
                    builder
                        .expose_input(
                            names[target],
                            node.input::<i64>("seed").unwrap(),
                            capacity(),
                        )
                        .unwrap();
                }
                for (source, producer) in nodes.iter().enumerate() {
                    if edges & (1 << (source * 2 + target)) != 0 {
                        builder
                            .connect(
                                producer.output::<i64>("out").unwrap(),
                                node.input::<i64>("feedback").unwrap(),
                                capacity(),
                            )
                            .unwrap();
                    }
                }
            }
            let compiled = builder.build().unwrap().compile().unwrap();
            for a in 0..3 {
                for b in 0..3 {
                    let policies = [a, b];
                    let declarations: BTreeMap<_, _> = names
                        .into_iter()
                        .zip(policies)
                        .filter_map(|(name, policy)| {
                            let readiness = match policy {
                                1 => Readiness::ANY,
                                2 => Readiness::ALL,
                                _ => return None,
                            };
                            Some((name.to_owned(), readiness))
                        })
                        .collect();
                    let actual = compiled.structural_packet_progress(&declarations);
                    let lower = outcomes(edges, seeds, policies, false);
                    let upper = outcomes(edges, seeds, policies, true);
                    for (node, name) in names.into_iter().enumerate() {
                        let expected = if lower & (1 << node) != 0 {
                            Progress::Reachable
                        } else if upper & (1 << node) != 0 {
                            Progress::Unknown
                        } else {
                            Progress::Blocked
                        };
                        assert_eq!(
                            actual[name], expected,
                            "edges={edges:04b}, seeds={seeds:02b}, policies={policies:?}, node={name}"
                        );
                    }
                }
            }
        }
    }
}
