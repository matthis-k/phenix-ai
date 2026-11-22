use phenix_graph::{ComponentDef, network::NetworkBuilder};
use std::{collections::BTreeSet, num::NonZeroUsize, sync::Arc};

fn cap() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

#[test]
fn feedback_cycles_are_classified_by_declared_roots_without_progress_claims() {
    let mut builder = NetworkBuilder::new();
    let primitive = Arc::new(ComponentDef::map(|n: i64| n));
    let source = builder
        .add("source", Arc::new(ComponentDef::source(|| std::iter::once(7_i64))))
        .unwrap();
    builder.start(&source).unwrap();
    let a = builder.add("a", primitive.clone()).unwrap();
    let b = builder.add("b", primitive.clone()).unwrap();
    let c = builder.add("c", primitive.clone()).unwrap();
    let d = builder.add("d", primitive.clone()).unwrap();
    let x = builder.add("x", primitive.clone()).unwrap();
    let y = builder.add("y", primitive).unwrap();

    for (from, to) in [
        (&a, &b),
        (&b, &a),
        (&c, &d),
        (&d, &c),
        (&x, &y),
        (&y, &x),
        (&source, &a),
    ] {
        builder
            .connect(
                from.output::<i64>("output").unwrap(),
                to.input::<i64>("input").unwrap(),
                cap(),
            )
            .unwrap();
    }
    builder
        .expose_input("seed", c.input::<i64>("input").unwrap(), cap())
        .unwrap();

    let compiled = builder.build().unwrap().compile().unwrap();
    let feedback: BTreeSet<Vec<&str>> =
        compiled.structural_feedback_cycles().into_iter().collect();
    assert_eq!(
        feedback,
        BTreeSet::from([vec!["a", "b"], vec!["c", "d"], vec!["x", "y"]])
    );
    let reachability = compiled.structural_reachability();
    assert_eq!(
        reachability.from_declared_starts,
        BTreeSet::from(["a".into(), "b".into(), "source".into()])
    );
    assert_eq!(
        reachability.from_public_ingress,
        BTreeSet::from(["c".into(), "d".into()])
    );
    // x/y have no path from either declared root, but the result is still
    // purely structural and is not proof of runtime deadlock.
    assert!(!reachability.from_declared_starts.contains("x"));
    assert!(!reachability.from_public_ingress.contains("y"));
    assert_eq!(compiled.processes().len(), 7);
}

#[test]
fn unregistered_zero_input_component_is_not_a_declared_start() {
    let mut builder = NetworkBuilder::new();
    builder
        .add("implicit", Arc::new(ComponentDef::source(|| std::iter::once(1_i64))))
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    assert!(compiled.structural_reachability().from_declared_starts.is_empty());
    assert!(compiled.structural_reachability().from_public_ingress.is_empty());
    assert!(compiled.structural_feedback_cycles().is_empty());
}
