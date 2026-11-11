//! Small, independent composition oracle. No expected result is composed or compiled.
use phenix_graph::compose::{
    ComposeError, Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration,
};
use phenix_graph::network::{ConnectionDef, NetworkError, PortPath};
use phenix_graph::{
    Body, ComponentDef, Contract, GraphOrigin, Interface, NetworkBuilder, PortDef, Process,
    ProcessContext, ProcessEmission, Step, Value,
};
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::sync::Arc;

const SEED: u64 = 0x6d6f_6465_6c66_6270;
const CAP: usize = 3;
type Edge = (&'static str, &'static str);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Map {
    mul: i64,
    add: i64,
    module: bool,
}
impl Map {
    fn apply(self, value: i64) -> i64 {
        self.mul * value + self.add
    }
}
const A: Map = Map {
    mul: 2,
    add: 1,
    module: false,
};
const B: Map = Map {
    mul: -1,
    add: 3,
    module: true,
};

struct IntegerMap(Map);
impl Process for IntegerMap {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
        Ok(match context.take("in") {
            Some(value) => Step::Emit(ProcessEmission::new(
                "out",
                Value::local(
                    self.0.mul * *value.borrow::<i64>().ok_or("expected local integer")?
                        + self.0.add,
                ),
            )),
            None => Step::Complete,
        })
    }
}
fn cap() -> NonZeroUsize {
    NonZeroUsize::new(CAP).unwrap()
}
fn port(process: &str, name: &str) -> PortPath {
    PortPath {
        process: process.into(),
        port: name.into(),
    }
}
fn key((a, b): Edge) -> ConnectionKey {
    ConnectionKey::new(port(a, "out"), port(b, "in"))
}
fn edge((a, b): Edge) -> ConnectionDef {
    ConnectionDef {
        from: port(a, "out"),
        to: port(b, "in"),
        capacity: cap(),
    }
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
    Contract::new("model/integer-map", interface()).unwrap()
}
fn definition(map: Map) -> Arc<ComponentDef> {
    let primitive = Arc::new(ComponentDef::primitive(
        interface(),
        Arc::new(move || IntegerMap(map)),
    ));
    let body = if map.module {
        let mut builder = NetworkBuilder::new();
        let leaf = builder.add("leaf", primitive).unwrap();
        builder
            .expose_input("in", leaf.input::<i64>("in").unwrap(), cap())
            .unwrap();
        builder
            .expose_output("out", leaf.output::<i64>("out").unwrap())
            .unwrap();
        ComponentDef::composite(builder.build().unwrap())
    } else {
        (*primitive).clone()
    };
    Arc::new(body.claim(&contract()).unwrap())
}

#[derive(Clone, Debug)]
enum Op {
    Put(&'static str, Map, bool), // bool distinguishes Add from Replace
    Wire(Edge, bool),
    Input(&'static str, &'static str),
    Output(&'static str, &'static str),
    Splice(&'static str, &'static str), // inserted node, expected predecessor
}
// This authority description is authored from model operations, not patch.footprints().
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Write {
    Node(&'static str),
    Edge(Edge),
    Input(&'static str),
    Output(&'static str),
}
impl Write {
    fn native(&self) -> Footprint {
        match *self {
            Self::Node(p) => Footprint::Component(p.into()),
            Self::Edge(e) => Footprint::Connection(key(e)),
            Self::Input(p) => Footprint::InputExport(p.into()),
            Self::Output(p) => Footprint::OutputExport(p.into()),
        }
    }
}
impl Op {
    fn writes(&self) -> Vec<Write> {
        match *self {
            Self::Put(p, _, _) => vec![Write::Node(p)],
            Self::Wire(e, _) => vec![Write::Edge(e)],
            Self::Input(n, _) => vec![Write::Input(n)],
            Self::Output(n, _) => vec![Write::Output(n)],
            Self::Splice(p, predecessor) => vec![
                Write::Edge(("a", "b")),
                Write::Edge((predecessor, "b")),
                Write::Edge((predecessor, p)),
                Write::Edge((p, "b")),
            ],
        }
    }
    fn native(&self, definitions: &BTreeMap<Map, Arc<ComponentDef>>) -> Edit {
        match *self {
            Self::Put(p, m, true) => Edit::Add {
                path: p.into(),
                definition: definitions[&m].clone(),
            },
            Self::Put(p, m, false) => Edit::Replace {
                path: p.into(),
                definition: definitions[&m].clone(),
            },
            Self::Wire(e, true) => Edit::Connect(edge(e)),
            Self::Wire(e, false) => Edit::Disconnect(key(e)),
            Self::Input(n, p) => Edit::ExposeInput {
                name: n.into(),
                targets: vec![port(p, "in")],
                capacity: cap(),
            },
            Self::Output(n, p) => Edit::ExposeOutput {
                name: n.into(),
                from: port(p, "out"),
            },
            Self::Splice(p, _) => Edit::Splice {
                anchor: key(("a", "b")),
                input: port(p, "in"),
                output: port(p, "out"),
            },
        }
    }
}
#[derive(Clone, Debug)]
struct Spec {
    id: String,
    deps: BTreeSet<String>,
    priority: i64,
    origin: GraphOrigin,
    ops: Vec<Op>,
}
impl Spec {
    fn new(id: &str, deps: &[&str], priority: i64, ops: Vec<Op>) -> Self {
        Self {
            id: id.into(),
            deps: deps.iter().map(|s| (*s).into()).collect(),
            priority,
            origin: GraphOrigin::default(),
            ops,
        }
    }
    fn writes(&self) -> BTreeSet<Write> {
        self.ops.iter().flat_map(Op::writes).collect()
    }
    fn native(&self, definitions: &BTreeMap<Map, Arc<ComponentDef>>) -> Registration {
        Registration::new(
            &self.id,
            self.deps.clone(),
            self.priority,
            self.origin.clone(),
            self.writes().iter().map(Write::native),
            NetworkPatch::new(self.ops.iter().map(|op| op.native(definitions))),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Topology {
    nodes: BTreeMap<&'static str, (Map, GraphOrigin)>,
    edges: BTreeSet<Edge>,
    inputs: BTreeMap<&'static str, &'static str>,
    outputs: BTreeMap<&'static str, &'static str>,
}
impl Topology {
    fn base(origin: &GraphOrigin) -> Self {
        Self {
            nodes: [("a", (A, origin.clone())), ("b", (B, origin.clone()))].into(),
            edges: [("a", "b")].into(),
            inputs: [("in", "a")].into(),
            outputs: [("out", "b")].into(),
        }
    }
    fn replay(&mut self, spec: &Spec) {
        for op in &spec.ops {
            match *op {
                Op::Put(p, map, add) => {
                    assert_eq!(
                        !self.nodes.contains_key(p),
                        add,
                        "invalid model operation: {spec:?}"
                    );
                    self.nodes.insert(p, (map, spec.origin.clone()));
                }
                Op::Wire(e, true) => {
                    assert!(self.edges.insert(e));
                }
                Op::Wire(e, false) => {
                    assert!(self.edges.remove(&e));
                }
                Op::Input(n, p) => {
                    self.inputs.insert(n, p);
                }
                Op::Output(n, p) => {
                    self.outputs.insert(n, p);
                }
                Op::Splice(p, predecessor) => {
                    assert!(self.edges.remove(&(predecessor, "b")));
                    assert!(self.edges.insert((predecessor, p)));
                    assert!(self.edges.insert((p, "b")));
                }
            }
        }
    }
    fn values(&self, start: &'static str, value: i64) -> BTreeMap<&'static str, Vec<i64>> {
        let mut pending = vec![(start, value)];
        let mut captures: BTreeMap<_, Vec<_>> = self.outputs.keys().map(|n| (*n, vec![])).collect();
        let mut remaining = 32;
        while let Some((node, input)) = pending.pop() {
            assert!(remaining > 0, "model networks must be acyclic");
            remaining -= 1;
            let output = self.nodes[node].0.apply(input);
            for (name, source) in &self.outputs {
                if *source == node {
                    captures.get_mut(name).unwrap().push(output);
                }
            }
            for (from, to) in &self.edges {
                if *from == node {
                    pending.push((*to, output));
                }
            }
        }
        captures
    }
}

fn depends(registry: &BTreeMap<String, Spec>, id: &str, on: &str) -> bool {
    registry[id]
        .deps
        .iter()
        .any(|dep| dep == on || depends(registry, dep, on))
}
fn overlap(a: &Spec, b: &Spec, registry: &BTreeMap<String, Spec>) -> bool {
    !depends(registry, &a.id, &b.id)
        && !depends(registry, &b.id, &a.id)
        && !a.writes().is_disjoint(&b.writes())
}

// Enumerate truth assignments satisfying dependency closure and priority defeat.
// This is not the composer's priority-sort/iterative elimination algorithm.
fn selection(registry: &BTreeMap<String, Spec>) -> Result<BTreeSet<String>, ()> {
    let regs: Vec<_> = registry.values().collect();
    for (i, a) in regs.iter().enumerate() {
        for b in &regs[i + 1..] {
            if overlap(a, b, registry)
                && (a.priority == b.priority
                    || (a.ops.iter().any(|op| matches!(op, Op::Splice(..)))
                        && b.ops.iter().any(|op| matches!(op, Op::Splice(..)))))
            {
                return Err(());
            }
        }
    }
    let mut solutions = vec![];
    for mask in 0..1_usize << regs.len() {
        let active: BTreeSet<_> = regs
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, r)| r.id.clone())
            .collect();
        let valid = regs.iter().all(|r| {
            let prerequisites = r.deps.is_subset(&active);
            let blocked = regs.iter().any(|winner| {
                active.contains(&winner.id)
                    && winner.priority > r.priority
                    && overlap(r, winner, registry)
            });
            if active.contains(&r.id) {
                prerequisites && !blocked
            } else {
                !prerequisites || blocked
            }
        });
        if valid {
            solutions.push(active);
        }
    }
    assert_eq!(
        solutions.len(),
        1,
        "oracle requires a unique solution: {registry:?}"
    );
    Ok(solutions.pop().unwrap())
}

struct Harness {
    composer: Composer,
    registry: BTreeMap<String, Spec>,
    base: Topology,
    definitions: BTreeMap<Map, Arc<ComponentDef>>,
}
impl Harness {
    fn new(maps: impl IntoIterator<Item = Map>) -> Self {
        let definitions: BTreeMap<_, _> = maps
            .into_iter()
            .chain([A, B])
            .map(|m| (m, definition(m)))
            .collect();
        let origin = GraphOrigin::default();
        let mut builder = NetworkBuilder::new();
        let a = builder
            .add(
                "a",
                Arc::new((*definitions[&A]).clone().with_origin(origin.clone())),
            )
            .unwrap();
        let b = builder
            .add(
                "b",
                Arc::new((*definitions[&B]).clone().with_origin(origin.clone())),
            )
            .unwrap();
        builder
            .connect(
                a.output::<i64>("out").unwrap(),
                b.input::<i64>("in").unwrap(),
                cap(),
            )
            .unwrap();
        builder
            .expose_input("in", a.input::<i64>("in").unwrap(), cap())
            .unwrap();
        builder
            .expose_output("out", b.output::<i64>("out").unwrap())
            .unwrap();
        Self {
            composer: Composer::new(builder.build().unwrap()).unwrap(),
            registry: BTreeMap::new(),
            base: Topology::base(&origin),
            definitions,
        }
    }
    fn check(&self) {
        let active = selection(&self.registry).unwrap();
        let mut expected = self.base.clone();
        let mut done = BTreeSet::new();
        while done != active {
            let next = active
                .iter()
                .find(|id| !done.contains(*id) && self.registry[*id].deps.is_subset(&done))
                .unwrap();
            expected.replay(&self.registry[next]);
            done.insert(next.clone());
        }
        let current = self.composer.current();
        assert_eq!(current.active(), &active);
        assert_eq!(
            current.footprints(),
            &active
                .iter()
                .map(|id| (
                    id.clone(),
                    self.registry[id]
                        .writes()
                        .iter()
                        .map(Write::native)
                        .collect()
                ))
                .collect()
        );
        assert_eq!(current.network().instances().len(), expected.nodes.len());
        for (path, (map, origin)) in &expected.nodes {
            let actual = &current.network().instances()[*path];
            assert_eq!(
                actual.definition_key(),
                self.definitions[map].definition_key()
            );
            assert_eq!(matches!(actual.body(), Body::Composite(_)), map.module);
            let inspection = current.compiled().inspect(path).unwrap();
            assert_eq!(inspection.declared_origin.as_ref(), Some(origin));
            let physical = if map.module {
                format!("{path}/leaf")
            } else {
                (*path).into()
            };
            assert_eq!(
                current
                    .compiled()
                    .inspect(&physical)
                    .unwrap()
                    .origin
                    .as_ref(),
                Some(origin)
            );
        }
        assert_eq!(
            current.network().connections(),
            expected.edges.iter().copied().map(edge).collect::<Vec<_>>()
        );
        assert_eq!(
            current.network().input_projections().len(),
            expected.inputs.len()
        );
        for (name, target) in &expected.inputs {
            let projection = &current.network().input_projections()[*name];
            assert_eq!(projection.targets, vec![port(target, "in")]);
            assert_eq!(projection.capacity, cap());
        }
        assert_eq!(
            current.network().output_projections(),
            &expected
                .outputs
                .iter()
                .map(|(n, p)| ((*n).into(), port(p, "out")))
                .collect()
        );
        let paths: BTreeSet<_> = current
            .compiled()
            .processes()
            .iter()
            .map(|p| p.path().to_owned())
            .collect();
        assert_eq!(
            paths,
            expected
                .nodes
                .iter()
                .map(|(p, (m, _))| if m.module {
                    format!("{p}/leaf")
                } else {
                    (*p).into()
                })
                .collect()
        );
        let physical = |p: &str| {
            if expected.nodes[p].0.module {
                format!("{p}/leaf")
            } else {
                p.to_owned()
            }
        };
        let compiled = current.compiled();
        let actual_edges: BTreeSet<_> = compiled
            .connections()
            .iter()
            .map(|e| {
                assert_eq!(e.from().1, "out");
                assert_eq!(e.to().1, "in");
                assert_eq!(e.capacity(), cap());
                (
                    compiled.processes()[e.from().0].path().to_owned(),
                    compiled.processes()[e.to().0].path().to_owned(),
                )
            })
            .collect();
        assert_eq!(
            actual_edges,
            expected
                .edges
                .iter()
                .map(|(a, b)| (physical(a), physical(b)))
                .collect()
        );
        for (name, target) in &expected.inputs {
            let endpoints = &compiled.inputs()[*name];
            assert_eq!(endpoints.len(), 1);
            assert_eq!(
                compiled.processes()[endpoints[0].0].path(),
                physical(target)
            );
            assert_eq!(endpoints[0].1, "in");
            assert_eq!(compiled.ingress_capacities()[*name], cap());
        }
        for (name, target) in &expected.outputs {
            let endpoint = &compiled.outputs()[*name];
            assert_eq!(compiled.processes()[endpoint.0].path(), physical(target));
            assert_eq!(endpoint.1, "out");
        }
        for (input_name, start) in &expected.inputs {
            let mut session = current.compiled().start();
            for input in [-3, 0, 7] {
                session.inject(input_name, Value::local(input)).unwrap();
                session.run_until_idle(1000);
                for (name, values) in expected.values(start, input) {
                    let mut actual = vec![];
                    while let Some(packet) = session.take_output(name).unwrap() {
                        actual.push(*packet.value().borrow::<i64>().unwrap());
                    }
                    assert_eq!(
                        actual, values,
                        "input={input_name}:{input}, topology={expected:?}"
                    );
                }
            }
        }
    }
    fn register(&mut self, specs: Vec<Spec>, reverse: bool) -> bool {
        let mut candidate = self.registry.clone();
        for spec in &specs {
            assert!(candidate.insert(spec.id.clone(), spec.clone()).is_none());
        }
        let expected = selection(&candidate);
        let published = self.composer.current().clone();
        let binding = published
            .compiled()
            .bind("a", &contract())
            .unwrap()
            .output::<i64>("out")
            .unwrap();
        let mut native: Vec<_> = specs.iter().map(|s| s.native(&self.definitions)).collect();
        if reverse {
            native.reverse();
        }
        let result = self.composer.register(native);
        match expected {
            Ok(_) => {
                result.unwrap();
                self.registry = candidate;
                assert!(matches!(
                    binding.resolve(self.composer.current().compiled()),
                    Err(NetworkError::ForeignScope)
                ));
            }
            Err(()) => {
                assert!(matches!(result, Err(ComposeError::Conflict { .. })));
                binding.resolve(self.composer.current().compiled()).unwrap();
                assert_eq!(self.composer.current().footprints(), published.footprints());
                for path in published.network().instances().keys() {
                    assert_eq!(
                        self.composer
                            .current()
                            .compiled()
                            .inspect(path)
                            .unwrap()
                            .declared_origin,
                        published.compiled().inspect(path).unwrap().declared_origin
                    );
                }
            }
        }
        self.check();
        if expected.is_err() {
            for spec in specs {
                assert!(self.composer.revoke(&spec.id).unwrap().is_empty());
            }
            self.check();
        }
        expected.is_ok()
    }
    fn revoke(&mut self, id: &str) {
        let removed: BTreeSet<_> = self
            .registry
            .keys()
            .filter(|other| other.as_str() == id || depends(&self.registry, other, id))
            .cloned()
            .collect();
        let published = self.composer.current().clone();
        let binding = published
            .compiled()
            .bind("a", &contract())
            .unwrap()
            .output::<i64>("out")
            .unwrap();
        assert_eq!(self.composer.revoke(id).unwrap(), removed);
        assert!(matches!(
            binding.resolve(self.composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        ));
        binding.resolve(published.compiled()).unwrap();
        self.registry.retain(|id, _| !removed.contains(id));
        self.check();
    }
}

#[test]
fn bounded_priority_dependency_selections_match_independent_subsets() {
    // 3^3 priority assignments x 4 dependency shapes x 2 batch orders = 216 cases.
    for p in -1..=1 {
        for q in -1..=1 {
            for r in -1..=1 {
                for shape in 0..4 {
                    for reverse in [false, true] {
                        let mut h = Harness::new([A, B]);
                        let specs = vec![
                            Spec::new("low", &[], p, vec![Op::Put("a", B, false)]),
                            Spec::new(
                                "rival",
                                if shape & 1 != 0 { &["low"] } else { &[] },
                                q,
                                vec![Op::Put("a", A, false)],
                            ),
                            Spec::new(
                                "dependent",
                                if shape & 2 != 0 { &["low"] } else { &[] },
                                r,
                                vec![
                                    Op::Put("x", B, true),
                                    Op::Input("extra-in", "x"),
                                    Op::Output("extra", "x"),
                                ],
                            ),
                            Spec::new(
                                "transitive",
                                &["dependent"],
                                100,
                                vec![Op::Put("y", A, true)],
                            ),
                        ];
                        h.register(specs, reverse);
                        h.revoke("rival");
                        h.revoke("low");
                        h.revoke("dependent");
                        assert!(h.registry.is_empty());
                    }
                }
            }
        }
    }
}

fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

#[test]
fn fixed_seed_replacement_rewire_export_and_revoke_sequences_match_model() {
    // 16 sequences x 32 operations, each replayed in forward and reverse batch order.
    let mut seed = SEED;
    for sequence in 0..16 {
        let mut forward = Harness::new([A, B]);
        let mut reverse = Harness::new([A, B]);
        for step in 0..32 {
            let choice = next(&mut seed) % 7;
            let id = format!("s{sequence}-{step}");
            let priority = (next(&mut seed) % 5) as i64 - 2;
            if choice == 0 && !forward.registry.is_empty() {
                let index = next(&mut seed) as usize % forward.registry.len();
                let victim = forward.registry.keys().nth(index).unwrap().clone();
                forward.revoke(&victim);
                reverse.revoke(&victim);
            } else {
                // Limit the exhaustive oracle to at most eight registrations.
                if forward.registry.len() > 5 {
                    let victim = forward.registry.keys().next().unwrap().clone();
                    forward.revoke(&victim);
                    reverse.revoke(&victim);
                }
                let ops = match choice {
                    0 | 1 => vec![Op::Put(
                        "a",
                        if next(&mut seed) & 1 == 0 { A } else { B },
                        false,
                    )],
                    2 => vec![Op::Put(
                        "b",
                        if next(&mut seed) & 1 == 0 { A } else { B },
                        false,
                    )],
                    3 => vec![
                        Op::Put("c", A, true),
                        Op::Wire(("a", "b"), false),
                        Op::Wire(("a", "c"), true),
                        Op::Wire(("c", "b"), true),
                    ],
                    4 => vec![Op::Input("alias-in", "a"), Op::Output("alias-out", "b")],
                    5 => vec![Op::Put("x", B, true), Op::Output("side", "x")],
                    _ => vec![Op::Put("a", B, false), Op::Put("b", A, false)],
                };
                let mut specs = vec![Spec::new(&id, &[], priority, ops)];
                if choice == 1 {
                    let child = format!("{id}-child");
                    specs.push(Spec::new(&child, &[&id], 100, vec![Op::Put("d", A, true)]));
                }
                assert_eq!(
                    forward.register(specs.clone(), false),
                    reverse.register(specs, true),
                    "seed={SEED:#x} sequence={sequence} step={step}"
                );
            }
            assert_eq!(
                forward.composer.current().active(),
                reverse.composer.current().active()
            );
            assert_eq!(
                forward.composer.current().network().connections(),
                reverse.composer.current().network().connections()
            );
        }
        let ids: Vec<_> = forward.registry.keys().cloned().collect();
        for id in ids {
            forward.revoke(&id);
            reverse.revoke(&id);
        }
        assert!(forward.registry.is_empty());
    }
}

#[test]
fn fixed_seed_ordered_splices_match_reference_topology_and_noncommuting_arithmetic() {
    let mut seed = SEED ^ 0x7370_6c69_6365;
    for _ in 0..32 {
        let x = Map {
            mul: 2,
            add: (next(&mut seed) % 7) as i64 - 3,
            module: next(&mut seed) & 1 == 0,
        };
        let y = Map {
            mul: -1,
            add: (next(&mut seed) % 7) as i64 + 1,
            module: !x.module,
        };
        let specs = vec![
            Spec::new(
                "first",
                &[],
                9,
                vec![Op::Put("x", x, true), Op::Splice("x", "a")],
            ),
            Spec::new(
                "second",
                &["first"],
                -9,
                vec![Op::Put("y", y, true), Op::Splice("y", "x")],
            ),
            Spec::new(
                "replace",
                &["second"],
                0,
                vec![Op::Put("a", B, false), Op::Output("middle", "x")],
            ),
        ];
        for reverse in [false, true] {
            let mut h = Harness::new([x, y]);
            assert!(h.register(specs.clone(), reverse));
            h.revoke("second");
            h.revoke("first");
            assert!(h.registry.is_empty());
            let unordered = vec![
                specs[0].clone(),
                Spec::new(
                    "unordered",
                    &[],
                    -9,
                    vec![Op::Put("y", y, true), Op::Splice("y", "a")],
                ),
            ];
            assert!(!h.register(unordered, reverse));
            assert!(h.registry.is_empty());
        }
    }
}

#[test]
fn missing_exact_grant_rejects_whole_batch_and_preserves_generation_and_provenance() {
    for missing in [
        Write::Node("c"),
        Write::Edge(("a", "c")),
        Write::Input("alias"),
        Write::Output("alias"),
        Write::Edge(("x", "y")),
    ] {
        let mut h = Harness::new([A, B]);
        h.register(
            vec![Spec::new("existing", &[], 0, vec![Op::Put("a", B, false)])],
            false,
        );
        let before = h.composer.current().clone();
        let binding = before
            .compiled()
            .bind("a", &contract())
            .unwrap()
            .output::<i64>("out")
            .unwrap();
        let spec = Spec::new(
            "unauthorized",
            &[],
            5,
            vec![
                Op::Put("c", A, true),
                Op::Wire(("a", "c"), true),
                Op::Input("alias", "c"),
                Op::Output("alias", "c"),
            ],
        );
        let splice = Spec::new(
            "splices",
            &[],
            0,
            vec![
                Op::Put("x", A, true),
                Op::Put("y", B, true),
                Op::Splice("x", "a"),
                Op::Splice("y", "x"),
            ],
        );
        let bad = if missing == Write::Edge(("x", "y")) {
            &splice
        } else {
            &spec
        };
        let grants: Vec<_> = bad
            .writes()
            .iter()
            .filter(|w| **w != missing)
            .map(Write::native)
            .collect();
        let registration = Registration::new(
            &bad.id,
            [],
            bad.priority,
            bad.origin.clone(),
            grants,
            bad.native(&h.definitions).patch().clone(),
        );
        let innocent = Spec::new("innocent", &[], 0, vec![Op::Output("innocent", "b")]);
        assert!(
            matches!(h.composer.register([innocent.native(&h.definitions), registration]), Err(ComposeError::Unauthorized { footprint, .. }) if footprint == missing.native())
        );
        binding.resolve(h.composer.current().compiled()).unwrap();
        h.check();
        assert_eq!(
            h.composer
                .current()
                .compiled()
                .inspect("a")
                .unwrap()
                .declared_origin,
            before.compiled().inspect("a").unwrap().declared_origin
        );
        assert!(h.composer.revoke(&bad.id).unwrap().is_empty());
        assert!(h.composer.revoke("innocent").unwrap().is_empty());
        h.revoke("existing");
    }
}
