use phenix_graph::contract::Contract;
use phenix_graph::network::{NetworkBuilder, NetworkError};
use phenix_graph::process::{Process, ProcessContext, Step};
use phenix_graph::{ComponentDef, GraphOrigin, Interface, PortDef};
use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Noop;
impl Process for Noop {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        Ok(Step::Finished)
    }
}
fn capacity() -> NonZeroUsize {
    NonZeroUsize::new(2).unwrap()
}
fn interface() -> Arc<Interface> {
    Arc::new(
        Interface::new(
            [PortDef::local::<i64>("in")],
            [PortDef::local::<i64>("out")],
        )
        .unwrap(),
    )
}
fn contract() -> Contract {
    Contract::new("test/transaction", interface()).unwrap()
}
fn definition() -> Arc<ComponentDef> {
    Arc::new(
        ComponentDef::primitive(interface(), Arc::new(|| Noop))
            .claim(&contract())
            .unwrap(),
    )
}

#[test]
fn errors_and_panics_restore_all_topology_and_revoke_escaped_handles() {
    for panic in [false, true] {
        let origin = GraphOrigin::default();
        let mut builder = NetworkBuilder::new().with_origin(origin.clone());
        let obsolete = builder.add("existing", definition()).unwrap();
        let existing = builder.replace(&obsolete, definition()).unwrap();
        let input = existing.input::<i64>("in").unwrap();
        let output = existing.output::<i64>("out").unwrap();
        builder
            .expose_input("in", input.clone(), capacity())
            .unwrap();
        builder.expose_output("out", output.clone()).unwrap();
        builder.require_contract("existing", &contract()).unwrap();
        let source = builder
            .node("source", Arc::new(Interface::default()), Arc::new(|| Noop))
            .unwrap();
        let mut escaped = None;
        let mut escaped_input = None;
        let external_effects = AtomicUsize::new(0);
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            builder.additions(|builder| {
                let added = builder.add("added", definition()).unwrap();
                escaped_input = Some(added.input::<i64>("in").unwrap());
                escaped = Some(added.clone());
                builder
                    .connect(
                        output.clone(),
                        added.input::<i64>("in").unwrap(),
                        capacity(),
                    )
                    .unwrap();
                builder
                    .expose_input("in", added.input::<i64>("in").unwrap(), capacity())
                    .unwrap();
                builder
                    .expose_input("new_in", input.clone(), capacity())
                    .unwrap();
                builder.expose_output("new_out", output.clone()).unwrap();
                builder.start(&source).unwrap();
                let expanded = Contract::new(
                    "test/expanded",
                    Arc::new(
                        Interface::new(
                            [PortDef::local::<i64>("in"), PortDef::local::<i64>("new_in")],
                            [
                                PortDef::local::<i64>("out"),
                                PortDef::local::<i64>("new_out"),
                            ],
                        )
                        .unwrap(),
                    ),
                )
                .unwrap();
                builder.claim(&expanded).unwrap();
                builder.require_contract("added", &contract()).unwrap();
                external_effects.fetch_add(1, Ordering::Relaxed);
                if panic {
                    panic_any(123_u32);
                }
                Err::<(), _>("abort")
            })
        }));
        if panic {
            assert_eq!(*outcome.unwrap_err().downcast::<u32>().unwrap(), 123);
        } else {
            assert_eq!(outcome.unwrap(), Err("abort"));
        }
        assert_eq!(external_effects.load(Ordering::Relaxed), 1);
        assert!(matches!(
            obsolete.input::<i64>("in"),
            Err(NetworkError::StaleHandle)
        ));
        let escaped = escaped.unwrap();
        assert!(matches!(
            escaped.input::<i64>("in"),
            Err(NetworkError::StaleHandle)
        ));
        assert!(matches!(
            builder.inspect(&escaped),
            Err(NetworkError::StaleHandle)
        ));
        builder.inspect(&existing).unwrap();
        let retry = builder.add("added", definition()).unwrap();
        assert!(matches!(
            builder.connect(output.clone(), escaped_input.unwrap(), capacity()),
            Err(NetworkError::StaleHandle)
        ));
        builder.inspect(&retry).unwrap();
        // The original endpoints and revision lineage survive rollback.
        let replaced = builder.replace(&existing, definition()).unwrap();
        assert!(matches!(
            builder.inspect(&existing),
            Err(NetworkError::StaleHandle)
        ));
        builder.inspect(&replaced).unwrap();
        let network = builder.build().unwrap();
        assert_eq!(network.origin(), Some(&origin));
        assert_eq!(network.instances().len(), 3);
        assert!(network.connections().is_empty());
        assert_eq!(network.input_projections().len(), 1);
        assert_eq!(
            network.input_projections()["in"].targets,
            [input.path().clone()]
        );
        assert_eq!(network.output_projections().len(), 1);
        assert_eq!(network.output_projections()["out"], *output.path());
        assert!(network.source_starts().is_empty());
        assert!(network.claims().is_empty());
        assert_eq!(
            network
                .requirements()
                .iter()
                .filter(|(p, _)| p == "existing")
                .count(),
            4
        );
        // Only the final added declaration contributes an added requirement.
        assert_eq!(
            network
                .requirements()
                .iter()
                .filter(|(p, _)| p == "added")
                .count(),
            1
        );
        network.compile().unwrap();
    }
}

#[test]
fn success_returns_live_handles_and_commits_metadata_without_spawning() {
    let spawns = Arc::new(AtomicUsize::new(0));
    let counter = spawns.clone();
    let definition = Arc::new(ComponentDef::primitive(
        Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap()),
        Arc::new(move || {
            counter.fetch_add(1, Ordering::Relaxed);
            Noop
        }),
    ));
    let mut builder = NetworkBuilder::new();
    let (source, sink) = builder
        .additions(|builder| {
            let source = builder.add("source", definition.clone())?;
            let sink = builder.add("sink", crate::definition())?;
            builder.connect(
                source.output::<i64>("out")?,
                sink.input::<i64>("in")?,
                capacity(),
            )?;
            builder.expose_input("in", sink.input::<i64>("in")?, capacity())?;
            builder.expose_output("out", sink.output::<i64>("out")?)?;
            builder.start(&source)?;
            builder.claim(&contract())?;
            builder.require_contract("sink", &contract())?;
            Ok::<_, NetworkError>((source, sink))
        })
        .unwrap();
    builder.inspect(&source).unwrap();
    builder.inspect(&sink).unwrap();
    let network = builder.build().unwrap();
    let frozen = network.clone();
    let mut sibling = NetworkBuilder::from_def(&frozen);
    assert!(matches!(
        sibling.inspect(&source),
        Err(NetworkError::ForeignScope)
    ));
    let sibling_source = sibling.component("source").unwrap();
    sibling.replace(&sibling_source, definition).unwrap();
    assert!(source.output::<i64>("out").is_ok());
    assert_eq!(network.connections().len(), 1);
    assert_eq!(network.source_starts(), ["source"]);
    assert_eq!(network.claims(), [contract()]);
    assert_eq!(
        network
            .requirements()
            .iter()
            .filter(|(p, _)| p == "sink")
            .count(),
        2
    );
    network.compile().unwrap();
    frozen.compile().unwrap();
    assert_eq!(spawns.load(Ordering::Relaxed), 0);
}

#[test]
fn replacement_is_rejected_even_if_error_is_caught_and_transaction_commits() {
    let mut builder = NetworkBuilder::new();
    let existing = builder.add("existing", definition()).unwrap();
    builder
        .additions(|builder| {
            let added = builder.add("added", definition()).unwrap();
            for current in [&existing, &added] {
                assert!(matches!(
                    builder.replace(current, definition()),
                    Err(NetworkError::ReplacementDuringAdditions)
                ));
            }
            Ok::<_, ()>(())
        })
        .unwrap();
    builder.inspect(&existing).unwrap();
    builder.replace(&existing, definition()).unwrap();
}

#[test]
fn nested_commits_are_revoked_by_outer_failure_and_inner_failure_is_local() {
    let mut builder = NetworkBuilder::new();
    let mut escaped = None;
    assert_eq!(
        builder.additions(|builder| {
            let outer = builder.add("outer", definition()).unwrap();
            assert_eq!(
                builder.additions(|builder| {
                    escaped = Some(builder.add("inner", definition()).unwrap());
                    Err::<(), _>(1)
                }),
                Err(1)
            );
            assert!(matches!(
                escaped.as_ref().unwrap().output::<i64>("out"),
                Err(NetworkError::StaleHandle)
            ));
            builder.inspect(&outer).unwrap();
            escaped = Some(
                builder
                    .additions(|builder| builder.add("inner", definition()))
                    .unwrap(),
            );
            Err::<(), _>(2)
        }),
        Err(2)
    );
    assert!(matches!(
        escaped.unwrap().output::<i64>("out"),
        Err(NetworkError::StaleHandle)
    ));
    let outer = builder.add("outer", definition()).unwrap();
    builder.replace(&outer, definition()).unwrap();
    assert!(builder.component("inner").is_err());
}

#[test]
fn failed_derived_like_export_can_retry_same_name_without_partial_edits() {
    let mut builder = NetworkBuilder::new();
    let existing = builder.add("existing", definition()).unwrap();
    builder
        .expose_output("result", existing.output::<i64>("out").unwrap())
        .unwrap();
    let mut escaped = None;
    let result = builder.additions(|builder| {
        let derived = builder.add("derived", definition())?;
        escaped = Some(derived.clone());
        builder.connect(
            existing.output::<i64>("out")?,
            derived.input::<i64>("in")?,
            capacity(),
        )?;
        builder.expose_output("result", derived.output::<i64>("out")?)
    });
    assert!(matches!(result, Err(NetworkError::InvalidProjection(_))));
    let derived = builder
        .additions(|builder| {
            let derived = builder.add("derived", definition())?;
            builder.connect(
                existing.output::<i64>("out")?,
                derived.input::<i64>("in")?,
                capacity(),
            )?;
            builder.expose_output("derived_result", derived.output::<i64>("out")?)?;
            Ok::<_, NetworkError>(derived)
        })
        .unwrap();
    assert!(matches!(
        builder.inspect(&escaped.unwrap()),
        Err(NetworkError::StaleHandle)
    ));
    builder.inspect(&derived).unwrap();
    let network = builder.build().unwrap();
    assert_eq!(network.connections().len(), 1);
    assert_eq!(network.output_projections().len(), 2);
}

#[test]
fn foreign_handles_cannot_author_edges_or_exports_in_a_transaction() {
    let mut foreign = NetworkBuilder::new();
    let foreign = foreign.add("same", definition()).unwrap();
    let mut builder = NetworkBuilder::new();
    let local = builder.add("same", definition()).unwrap();
    builder
        .additions(|builder| {
            assert!(matches!(
                builder.connect(
                    foreign.output::<i64>("out").unwrap(),
                    local.input::<i64>("in").unwrap(),
                    capacity()
                ),
                Err(NetworkError::ForeignScope)
            ));
            assert!(matches!(
                builder.expose_output("foreign", foreign.output::<i64>("out").unwrap()),
                Err(NetworkError::ForeignScope)
            ));
            Ok::<_, ()>(())
        })
        .unwrap();
    let network = builder.build().unwrap();
    assert!(network.connections().is_empty());
    assert!(network.output_projections().is_empty());
}

#[test]
fn rollback_preserves_nonempty_frozen_metadata_and_definition_identity() {
    let origin = GraphOrigin::default();
    let mut builder = NetworkBuilder::new().with_origin(origin);
    let left = builder.add("left", definition()).unwrap();
    let right = builder.add("right", definition()).unwrap();
    let source = builder
        .node("source", Arc::new(Interface::default()), Arc::new(|| Noop))
        .unwrap();
    builder.start(&source).unwrap();
    builder
        .connect(
            left.output::<i64>("out").unwrap(),
            right.input::<i64>("in").unwrap(),
            capacity(),
        )
        .unwrap();
    builder
        .expose_input("in", left.input::<i64>("in").unwrap(), capacity())
        .unwrap();
    builder
        .expose_output("out", right.output::<i64>("out").unwrap())
        .unwrap();
    builder.claim(&contract()).unwrap();
    builder.require_contract("left", &contract()).unwrap();
    let frozen = builder.build().unwrap();
    let mut builder = NetworkBuilder::from_def(&frozen);
    let left = builder.component("left").unwrap();
    assert_eq!(
        builder.additions(|builder| {
            builder.add("aborted", definition()).unwrap();
            builder.require_contract("missing", &contract()).unwrap();
            Err::<(), _>(())
        }),
        Err(())
    );
    builder.inspect(&left).unwrap();
    let restored = builder.build().unwrap();
    assert!(Arc::ptr_eq(
        &restored.instances()["left"],
        &frozen.instances()["left"]
    ));
    assert_eq!(restored.connections(), frozen.connections());
    assert_eq!(restored.input_projections(), frozen.input_projections());
    assert_eq!(restored.output_projections(), frozen.output_projections());
    assert_eq!(restored.source_starts(), frozen.source_starts());
    assert_eq!(restored.claims(), frozen.claims());
    assert_eq!(restored.origin(), frozen.origin());
    assert!(restored.requirements().iter().all(|(p, _)| p != "missing"));
    restored.compile().unwrap();
    frozen.compile().unwrap();
}

#[test]
fn caught_inner_panic_restores_guard_and_allows_outer_commit() {
    let mut builder = NetworkBuilder::new();
    let mut escaped = None;
    let outer = builder
        .additions(|builder| {
            let outer = builder.add("outer", definition()).unwrap();
            let panic = catch_unwind(AssertUnwindSafe(|| {
                builder.additions::<(), ()>(|builder| {
                    escaped = Some(builder.add("inner", definition()).unwrap());
                    panic_any("inner panic");
                })
            }));
            assert!(panic.is_err());
            assert!(matches!(
                builder.replace(&outer, definition()),
                Err(NetworkError::ReplacementDuringAdditions)
            ));
            Ok::<_, ()>(outer)
        })
        .unwrap();
    assert!(matches!(
        escaped.unwrap().input::<i64>("in"),
        Err(NetworkError::StaleHandle)
    ));
    builder.inspect(&outer).unwrap();
    builder.replace(&outer, definition()).unwrap();
}

#[test]
fn moved_transaction_builders_are_revoked_and_original_scope_is_restored() {
    for swap in [false, true] {
        for outcome in [0, 1, 2] {
            let mut builder = NetworkBuilder::new();
            let existing = builder.add("existing", definition()).unwrap();
            let old_input = existing.input::<i64>("in").unwrap();
            let mut moved = NetworkBuilder::new();
            let foreign = moved.add("foreign", definition()).unwrap();
            let mut escaped = None;
            let mut reissued = None;
            let mut bound = None;
            let mut endpoint = None;
            let result = catch_unwind(AssertUnwindSafe(|| {
                builder.additions(|builder| {
                    let added = builder.add("added", definition()).unwrap();
                    endpoint = Some(added.input::<i64>("in").unwrap());
                    escaped = Some(added);
                    reissued = Some(builder.component("existing").unwrap());
                    bound = Some(builder.bind(&existing, &contract()).unwrap());
                    if swap {
                        std::mem::swap(builder, &mut moved);
                    } else {
                        moved = std::mem::take(builder);
                    }
                    match outcome {
                        0 => Ok(()),
                        1 => Err("abort"),
                        _ => panic_any(123_u32),
                    }
                })
            }));
            let payload = result.unwrap_err();
            if outcome == 2 {
                assert_eq!(*payload.downcast::<u32>().unwrap(), 123);
            }
            for handle in [
                escaped.as_ref().unwrap(),
                reissued.as_ref().unwrap(),
                bound.as_ref().unwrap(),
            ] {
                assert!(matches!(
                    handle.input::<i64>("in"),
                    Err(NetworkError::StaleHandle)
                ));
                assert!(matches!(
                    moved.inspect(handle),
                    Err(NetworkError::StaleHandle)
                ));
            }
            assert!(matches!(
                moved.component("existing"),
                Err(NetworkError::StaleHandle)
            ));
            assert!(matches!(
                moved.replace(&existing, definition()),
                Err(NetworkError::StaleHandle)
            ));
            assert!(matches!(
                moved.add("late", definition()),
                Err(NetworkError::StaleHandle)
            ));
            assert!(matches!(
                moved.start(&existing),
                Err(NetworkError::StaleHandle)
            ));
            assert!(matches!(
                moved.claim(&contract()),
                Err(NetworkError::StaleHandle)
            ));
            assert!(matches!(
                moved.require_contract("existing", &contract()),
                Err(NetworkError::StaleHandle)
            ));
            assert!(
                catch_unwind(AssertUnwindSafe(|| moved.additions(|_| Ok::<_, ()>(())))).is_err()
            );
            assert!(matches!(moved.build(), Err(NetworkError::StaleHandle)));
            builder.inspect(&existing).unwrap();
            assert!(matches!(
                builder.inspect(&foreign),
                Err(NetworkError::ForeignScope)
            ));
            let retry = builder.add("added", definition()).unwrap();
            builder.inspect(&retry).unwrap();
            builder.expose_input("in", old_input, capacity()).unwrap();
            assert!(matches!(
                builder.expose_input("stale", endpoint.unwrap(), capacity()),
                Err(NetworkError::StaleHandle)
            ));
            builder.replace(&existing, definition()).unwrap();
            assert!(matches!(
                existing.input::<i64>("in"),
                Err(NetworkError::StaleHandle)
            ));
            let network = builder.build().unwrap();
            assert_eq!(network.instances().len(), 2);
            network.compile().unwrap();
        }
    }
}

#[test]
fn nested_move_failure_is_local_but_outer_rollback_revokes_inner_commits() {
    let mut builder = NetworkBuilder::new();
    let existing = builder.add("existing", definition()).unwrap();
    let mut moved = None;
    let mut escaped = None;
    let mut committed = None;
    assert_eq!(
        builder.additions(|builder| {
            let outer = builder.add("outer", definition()).unwrap();
            assert!(
                catch_unwind(AssertUnwindSafe(|| {
                    builder.additions(|builder| {
                        escaped = Some(builder.add("inner", definition()).unwrap());
                        moved = Some(std::mem::take(builder));
                        Ok::<_, ()>(())
                    })
                }))
                .is_err()
            );
            builder.inspect(&outer).unwrap();
            builder.inspect(&existing).unwrap();
            committed = Some(
                builder
                    .additions(|builder| builder.add("inner", definition()))
                    .unwrap(),
            );
            Err::<(), _>("outer abort")
        }),
        Err("outer abort")
    );
    assert!(matches!(
        moved.unwrap().build(),
        Err(NetworkError::StaleHandle)
    ));
    for handle in [escaped.unwrap(), committed.unwrap()] {
        assert!(matches!(
            handle.output::<i64>("out"),
            Err(NetworkError::StaleHandle)
        ));
    }
    builder.inspect(&existing).unwrap();
    builder.add("inner", definition()).unwrap();
    builder.replace(&existing, definition()).unwrap();
}

#[test]
fn active_transaction_cannot_freeze_an_executable_partial_candidate() {
    let mut builder = NetworkBuilder::new();
    let existing = builder.add("existing", definition()).unwrap();
    let mut escaped = None;
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            builder.additions(|builder| {
                escaped = Some(builder.add("partial", definition()).unwrap());
                let candidate = std::mem::take(builder).build();
                assert!(matches!(candidate, Err(NetworkError::BuildDuringAdditions)));
                Err::<(), _>(())
            })
        }))
        .is_err()
    );
    assert!(matches!(
        escaped.unwrap().input::<i64>("in"),
        Err(NetworkError::StaleHandle)
    ));
    builder.inspect(&existing).unwrap();
    assert_eq!(builder.build().unwrap().instances().len(), 1);
}

#[test]
fn temporarily_moving_the_same_scope_back_preserves_transaction_semantics() {
    let mut builder = NetworkBuilder::new();
    let added = builder
        .additions(|builder| {
            let mut moved = std::mem::take(builder);
            let added = moved.add("added", definition())?;
            std::mem::swap(builder, &mut moved);
            Ok::<_, NetworkError>(added)
        })
        .unwrap();
    builder.inspect(&added).unwrap();
    builder.replace(&added, definition()).unwrap();
}
