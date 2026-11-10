use std::{cell::RefCell, rc::Rc};

use phenix_graph::{
    AssemblyChange, GraphAssembly, GraphBuilder, GraphOrigin, NodeTemplate, PatchBuilder,
    PatchError, PatchPlanner, PatchSpec, PatchTarget,
};

#[test]
fn same_package_replacement_requires_an_instance_write() {
    check_replacement_footprint(true);
}

#[test]
fn same_factory_cloned_template_replacement_requires_a_node_write() {
    check_replacement_footprint(false);
}

fn check_replacement_footprint(is_module: bool) {
    for declared in [false, true] {
        let mut child = GraphBuilder::new("child");
        let inner = child.node("inner", |x: i64| x).unwrap();
        child.expose_input("request", inner.inlet()).unwrap();
        child.expose_output("result", inner.output()).unwrap();
        let package = Rc::new(child.module());
        let template = NodeTemplate::local_function(|x: i64| x);
        let original_origin = GraphOrigin::default();
        let replacement_origin = GraphOrigin::default();
        let target = if is_module {
            PatchTarget::Instance("target".into())
        } else {
            PatchTarget::Node("target".into())
        };

        let handles = Rc::new(RefCell::new(None));
        let saved = Rc::clone(&handles);
        let original_package = package.clone();
        let original_template = template.clone();
        let origin = original_origin.clone();
        let mut base = PatchPlanner::new();
        base.add_patch(PatchBuilder::custom(
            PatchSpec::new("base").writes(target.clone()),
            move |g| {
                g.with_origin(&origin, |g| {
                    if is_module {
                        g.instance("target", &original_package)?;
                    } else {
                        g.add("target", original_template)
                            .map_err(|e| e.to_string())?;
                    }
                    let input = g
                        .discover("target")?
                        .inlets::<(), phenix_graph::LocalValue>()
                        .port(phenix_graph::InletPort::<(), i64>::new(if is_module {
                            "request"
                        } else {
                            "run"
                        }))
                        .field(phenix_graph::SlotField::<i64, i64>::new("value"))
                        .target()?;
                    let output = g
                        .discover("target")?
                        .output::<i64, phenix_graph::LocalValue>()?;
                    *saved.borrow_mut() = Some((input, output));
                    Ok(())
                })
            },
        ))
        .unwrap();
        let mut assembly = GraphAssembly::new("patch_lineage");
        base.apply(&mut assembly).unwrap();
        let revisions = assembly.revisions().to_vec();

        let mut planner = PatchPlanner::new();
        // This earlier contribution must also roll back if replacement is rejected.
        planner
            .add_patch(PatchBuilder::custom(
                PatchSpec::new("earlier").writes(PatchTarget::Node("extra".into())),
                |g| {
                    g.node("extra", |x: i64| x).map_err(|e| e.to_string())?;
                    Ok(())
                },
            ))
            .unwrap();
        let spec = PatchSpec::new("replace").after("earlier");
        let spec = if declared {
            spec.writes(target.clone())
        } else {
            spec
        };
        let origin = replacement_origin.clone();
        planner
            .add_patch(PatchBuilder::custom(spec, move |g| {
                g.with_origin(&origin, |g| {
                    if is_module {
                        g.replace("target", package.as_ref())
                    } else {
                        g.replace("target", template)
                    }
                })
            }))
            .unwrap();
        let result = planner.apply(&mut assembly);
        if declared {
            result.unwrap();
            assert_eq!(
                assembly.revisions().last().unwrap().changes,
                [if is_module {
                    AssemblyChange::InstanceRebound("target".into())
                } else {
                    AssemblyChange::NodeReplaced("target".into())
                }]
            );
        } else {
            assert_eq!(
                result,
                Err(PatchError::UndeclaredWrite {
                    patch: "replace".into(),
                    target: Box::new(target),
                })
            );
            assert_eq!(assembly.revisions(), revisions);
        }

        let (input, output) = handles.borrow_mut().take().unwrap();
        let mut probe = PatchPlanner::new();
        probe
            .add_patch(PatchBuilder::custom(
                PatchSpec::new("probe").writes_all([
                    PatchTarget::PublicInput("cached_input".into()),
                    PatchTarget::PublicOutput("cached_output".into()),
                ]),
                move |g| {
                    if declared {
                        assert!(g.expose_input("cached_input", input).is_err());
                        assert!(g.expose_output("cached_output", output).is_err());
                    } else {
                        g.expose_input("cached_input", input)?;
                        g.expose_output("cached_output", output)?;
                    }
                    Ok(())
                },
            ))
            .unwrap();
        probe.apply(&mut assembly).unwrap();
        let compiled = assembly.finish().unwrap();
        assert_eq!(compiled.graph.find("extra").is_some(), declared);
        assert_eq!(
            compiled.graph.find("target").unwrap().origin(),
            Some(if declared {
                &replacement_origin
            } else {
                &original_origin
            })
        );
    }
}
