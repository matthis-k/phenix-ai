//! Common-API whole composition fixtures, shared by correctness and paired runs.
use phenix_graph::{
    ComponentDef, GraphOrigin, NetworkBuilder, Value,
    compose::{ComposeError, Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration},
    network::PortPath,
};
use std::{collections::BTreeSet, num::NonZeroUsize, sync::Arc};

fn port(process: &str, port: &str) -> PortPath {
    PortPath {
        process: process.into(),
        port: port.into(),
    }
}
fn edge(from: &str, to: &str) -> ConnectionKey {
    ConnectionKey::new(port(from, "output"), port(to, "input"))
}
fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}
fn reg(id: String, dependencies: Vec<String>, priority: i64, patch: NetworkPatch) -> Registration {
    Registration::new(
        id,
        dependencies,
        priority,
        GraphOrigin::default(),
        patch.footprints(),
        patch,
    )
}
fn output(composer: &Composer, expected: i64) {
    for compiled in [
        composer.current().compiled().clone(),
        composer.current().network().compile().unwrap(),
    ] {
        let mut session = compiled.start();
        session.inject("in", Value::local(1_i64)).unwrap();
        session.run_until_idle(10_000);
        assert_eq!(
            session
                .take_output("out")
                .unwrap()
                .unwrap()
                .value()
                .borrow::<i64>(),
            Some(&expected)
        );
    }
}

pub fn dense(n: usize, reverse_priority: bool, configure: fn(&mut Composer)) {
    let mut builder = NetworkBuilder::new();
    let node = builder
        .add("worker", Arc::new(ComponentDef::map(|n: i64| n)))
        .unwrap();
    builder
        .expose_input("in", node.input::<i64>("input").unwrap(), one())
        .unwrap();
    builder
        .expose_output("out", node.output::<i64>("output").unwrap())
        .unwrap();
    let mut composer = Composer::new(builder.build().unwrap()).unwrap();
    configure(&mut composer);
    let id = |i| format!("r{i:04}");
    composer
        .register((0..n).rev().map(|i| {
            let priority = if reverse_priority { n - i } else { i + 1 } as i64;
            reg(
                id(i),
                vec![],
                priority,
                NetworkPatch::new([Edit::Replace {
                    path: "worker".into(),
                    definition: Arc::new(ComponentDef::map(move |n: i64| n + priority)),
                }]),
            )
        }))
        .unwrap();
    let winner = if reverse_priority { 0 } else { n - 1 };
    assert_eq!(composer.current().active(), &BTreeSet::from([id(winner)]));
    assert_eq!(
        composer.current().footprints()[&id(winner)],
        BTreeSet::from([Footprint::Component("worker".into())])
    );
    output(&composer, n as i64 + 1);
    assert_eq!(
        composer.revoke(&id(winner)).unwrap(),
        BTreeSet::from([id(winner)])
    );
    output(&composer, n as i64);
}

pub fn defeat(groups: usize, configure: fn(&mut Composer)) {
    let mut builder = NetworkBuilder::new();
    builder
        .add("worker", Arc::new(ComponentDef::map(|n: i64| n)))
        .unwrap();
    let mut composer = Composer::new(builder.build().unwrap()).unwrap();
    configure(&mut composer);
    let patch = |names: Vec<String>| {
        NetworkPatch::new(names.into_iter().map(|name| Edit::ExposeOutput {
            name,
            from: port("worker", "output"),
        }))
    };
    let mut expected = BTreeSet::new();
    for i in 0..groups {
        let (p, q, upper_p, upper_q, r) = (
            format!("p{i}"),
            format!("q{i}"),
            format!("P{i}"),
            format!("Q{i}"),
            format!("R{i}"),
        );
        let (a, b, x) = (format!("a{i}"), format!("b{i}"), format!("x{i}"));
        composer
            .register([
                reg(p.clone(), vec![], 0, patch(vec![a.clone()])),
                reg(q.clone(), vec![], 0, patch(vec![b.clone()])),
            ])
            .unwrap();
        composer
            .register([
                reg(upper_q.clone(), vec![q.clone()], 5, patch(vec![a])),
                reg(upper_p.clone(), vec![p.clone()], 10, patch(vec![x])),
            ])
            .unwrap();
        let mut suppressed = expected.clone();
        suppressed.extend([q, upper_q]);
        assert_eq!(composer.current().active(), &suppressed);
        let r_reg = reg(r.clone(), vec![], 3, patch(vec![b]));
        composer.register([r_reg.clone()]).unwrap();
        expected.extend([p, upper_p, r.clone()]);
        assert_eq!(composer.current().active(), &expected);
        assert_eq!(
            composer
                .current()
                .network()
                .compile()
                .unwrap()
                .processes()
                .len(),
            1
        );
        composer.revoke(&r).unwrap();
        assert_eq!(composer.current().active(), &suppressed);
        composer.register([r_reg]).unwrap();
        assert_eq!(composer.current().active(), &expected);
    }
}

pub fn splices(n: usize, configure: fn(&mut Composer)) {
    let mut builder = NetworkBuilder::new();
    let a = builder
        .add("a", Arc::new(ComponentDef::map(|n: i64| n)))
        .unwrap();
    let z = builder
        .add("z", Arc::new(ComponentDef::map(|n: i64| n)))
        .unwrap();
    builder
        .connect(
            a.output::<i64>("output").unwrap(),
            z.input::<i64>("input").unwrap(),
            one(),
        )
        .unwrap();
    builder
        .expose_input("in", a.input::<i64>("input").unwrap(), one())
        .unwrap();
    builder
        .expose_output("out", z.output::<i64>("output").unwrap())
        .unwrap();
    let node = |i| format!("m{i:04}");
    let id = |i| format!("r{:04}", n - i);
    for i in 0..n {
        builder
            .add(node(i), Arc::new(ComponentDef::map(|n: i64| n + 1)))
            .unwrap();
    }
    let mut composer = Composer::new(builder.build().unwrap()).unwrap();
    configure(&mut composer);
    let registration = |i: usize, ordered: bool| {
        let patch = NetworkPatch::new([Edit::Splice {
            anchor: edge("a", "z"),
            input: port(&node(i), "input"),
            output: port(&node(i), "output"),
        }]);
        let from = if i == 0 || !ordered {
            "a".into()
        } else {
            node(i - 1)
        };
        let mut grants = patch.footprints();
        grants.extend([
            Footprint::Connection(edge(&from, "z")),
            Footprint::Connection(edge(&from, &node(i))),
        ]);
        Registration::new(
            id(i),
            if i == 0 || !ordered {
                vec![]
            } else {
                vec![id(i - 1)]
            },
            i as i64,
            GraphOrigin::default(),
            grants,
            patch,
        )
    };
    composer
        .register((0..n).rev().map(|i| registration(i, true)))
        .unwrap();
    assert_eq!(composer.current().active(), &(0..n).map(id).collect());
    let chain: Vec<_> = std::iter::once("a".to_owned())
        .chain((0..n).map(node))
        .chain(std::iter::once("z".into()))
        .collect();
    let actual: BTreeSet<_> = composer
        .current()
        .network()
        .connections()
        .iter()
        .map(|e| ConnectionKey::new(e.from.clone(), e.to.clone()))
        .collect();
    assert_eq!(
        actual,
        chain.windows(2).map(|p| edge(&p[0], &p[1])).collect()
    );
    output(&composer, n as i64 + 1);
    assert_eq!(composer.revoke(&id(0)).unwrap(), (0..n).map(id).collect());
    output(&composer, 1);
    assert!(matches!(
        composer.register([registration(0, false), registration(1, false)]),
        Err(ComposeError::Conflict { .. })
    ));
    assert!(composer.current().active().is_empty());
}

pub fn hierarchical(n: usize, configure: fn(&mut Composer)) {
    let mut inner = NetworkBuilder::new();
    let definition = Arc::new(ComponentDef::map(|n: i64| n));
    for i in 0..n {
        let child = inner
            .add(format!("leaf{i:04}"), definition.clone())
            .unwrap();
        inner.allow_replace(&child).unwrap();
    }
    let definition = Arc::new(ComponentDef::composite(inner.build().unwrap()));
    let mut builder = NetworkBuilder::new();
    builder.add("m", definition.clone()).unwrap();
    let mut composer = Composer::new(builder.build().unwrap()).unwrap();
    configure(&mut composer);
    let parent = reg(
        "parent".into(),
        vec![],
        0,
        NetworkPatch::new([Edit::Replace {
            path: "m".into(),
            definition,
        }]),
    );
    let children: Vec<_> = (0..n)
        .map(|i| {
            reg(
                format!("r{i:04}"),
                vec!["parent".into()],
                0,
                NetworkPatch::new([Edit::Replace {
                    path: format!("m/leaf{i:04}"),
                    definition: Arc::new(ComponentDef::map(|n: i64| n + 1)),
                }]),
            )
        })
        .collect();
    composer
        .register(std::iter::once(parent.clone()).chain(children.clone()))
        .unwrap();
    assert_eq!(composer.current().compiled().processes().len(), n);
    let fresh = composer.current().network().compile().unwrap();
    for i in 0..n {
        assert_eq!(
            composer
                .current()
                .compiled()
                .inspect(&format!("m/leaf{i:04}"))
                .unwrap()
                .physical_index,
            fresh
                .inspect(&format!("m/leaf{i:04}"))
                .unwrap()
                .physical_index
        );
    }
    assert_eq!(composer.revoke("parent").unwrap().len(), n + 1);
    let independent = reg("child".into(), vec![], 0, children[0].patch().clone());
    assert!(matches!(
        composer.register([parent, independent]),
        Err(ComposeError::Conflict { .. })
    ));
    assert!(composer.current().active().is_empty());
}
