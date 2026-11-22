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
    let e = builder.add("e", primitive.clone()).unwrap();
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
