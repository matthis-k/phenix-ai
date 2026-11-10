//! Final-state write accounting. Opaque allocations are compared, never executed.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Requirement {
    Graph,
    Node(String),
    Instance(String),
    Connection(String),
    PublicInput(String),
    PublicOutput(String),
    Trigger(String),
}
pub(super) struct Observation {
    pub required: Vec<Requirement>,
    pub events: Vec<AssemblyChange>,
}

fn sequence<T>(a: &[T], b: &[T], eq: impl Fn(&T, &T) -> bool) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| eq(a, b))
}
fn claims(a: &[GraphContract], b: &[GraphContract]) -> bool {
    sequence(a, b, interfaces::stored_contract_eq)
}
fn requests(a: &[(String, GraphContract)], b: &[(String, GraphContract)]) -> bool {
    sequence(a, b, |a, b| {
        a.0 == b.0 && interfaces::stored_contract_eq(&a.1, &b.1)
    })
}
fn optional<T>(a: &Option<T>, b: &Option<T>, eq: impl Fn(&T, &T) -> bool) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

// The destructuring is intentionally exhaustive: new stored fields need a rule.
macro_rules! fields_eq {
    ($fn:ident, $ty:ident, $($field:ident),+ $(,)?) => {
        fn $fn(a: &$ty, b: &$ty) -> bool {
            let $ty { $($field),+ } = a;
            true $(&& $field == &b.$field)+
        }
    };
}
fields_eq!(input_slot, InputSlot, id, name, schema, local_type, buffer);
fields_eq!(output_slot, OutputSlot, id, name, schema, local_type);
fields_eq!(need, ResourceNeed, key, access);
fields_eq!(instance, ModuleInstance, name, definition);
fields_eq!(edge, ModuleEdge, declaration_id, from, to);
fields_eq!(
    input,
    ModuleInput,
    id,
    name,
    port,
    slot,
    schema,
    local_type,
    targets
);
fields_eq!(output, ModuleOutput, id, name, schema, local_type, source);
fields_eq!(trigger, ModuleTrigger, id, name, targets);
fields_eq!(limits, CompileLimits, max_depth, max_instances, max_work);

fn port(a: &Port, b: &Port) -> bool {
    let Port {
        id,
        name,
        slots,
        policy,
        args,
    } = a;
    id == &b.id
        && name == &b.name
        && sequence(slots, &b.slots, input_slot)
        && Arc::ptr_eq(policy, &b.policy)
        && optional(args, &b.args, Arc::ptr_eq)
}
fn node(a: &NodeTemplate, b: &NodeTemplate) -> bool {
    let NodeTemplate {
        composition_origin,
        claims: c,
        id,
        name,
        declaring_scope,
        declaration_id,
        ports,
        output,
        grants,
        cohort_mode,
        logic_factory,
    } = a;
    composition_origin == &b.composition_origin
        && claims(c, &b.claims)
        && id == &b.id
        && name == &b.name
        && declaring_scope == &b.declaring_scope
        && declaration_id == &b.declaration_id
        && sequence(ports, &b.ports, port)
        && output_slot(output, &b.output)
        && sequence(grants, &b.grants, need)
        && cohort_mode == &b.cohort_mode
        && Arc::ptr_eq(logic_factory, &b.logic_factory)
}
fn definition(a: &ModuleDef, b: &ModuleDef) -> bool {
    let ModuleDef {
        declaration_ids,
        claims: c,
        contract_requests,
        id,
        nodes,
        instances,
        edges,
        inputs,
        output: o,
    } = a;
    declaration_ids == &b.declaration_ids
        && claims(c, &b.claims)
        && requests(contract_requests, &b.contract_requests)
        && id == &b.id
        && sequence(nodes, &b.nodes, node)
        && sequence(instances, &b.instances, instance)
        && sequence(edges, &b.edges, edge)
        && sequence(inputs, &b.inputs, input)
        && optional(o, &b.output, output)
}
fn entry(a: &ModuleEntry, b: &ModuleEntry) -> bool {
    let ModuleEntry {
        definition: d,
        triggers,
    } = a;
    definition(d, &b.definition) && sequence(triggers, &b.triggers, trigger)
}
fn keys<K: Ord + Clone, V, W>(a: &BTreeMap<K, V>, b: &BTreeMap<K, W>) -> BTreeSet<K> {
    a.keys().chain(b.keys()).cloned().collect()
}
fn map_value<T>(a: Option<&T>, b: Option<&T>, eq: impl Fn(&T, &T) -> bool) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => eq(a, b),
        (None, None) => true,
        _ => false,
    }
}
fn survivors<T: PartialEq>(before: &[T], after: &[T]) -> bool {
    before
        .iter()
        .filter(|v| after.contains(v))
        .eq(after.iter().filter(|v| before.contains(v)))
}

// Every direct alias owns its entire cycle-safe closure in both endpoint states.
fn owners(
    g: &GraphBuilder,
    required: &mut BTreeSet<Requirement>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut result = BTreeMap::<String, BTreeSet<String>>::new();
    for alias in &g.module.instances {
        let mut pending = vec![alias.definition.clone()];
        let mut seen = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            result
                .entry(id.clone())
                .or_default()
                .insert(alias.name.clone());
            match g.dependencies.get(&id) {
                Some(entry) if entry.definition.id == id => {
                    pending.extend(
                        entry
                            .definition
                            .instances
                            .iter()
                            .map(|i| i.definition.clone()),
                    );
                }
                _ => {
                    required.insert(Requirement::Graph);
                }
            }
        }
    }
    result
}
fn charge(owners: Option<&BTreeSet<String>>, r: &mut BTreeSet<Requirement>) {
    if let Some(owners) = owners.filter(|o| !o.is_empty()) {
        r.extend(owners.iter().cloned().map(Requirement::Instance));
    } else {
        r.insert(Requirement::Graph);
    }
}

fn validate(
    g: &GraphBuilder,
    pending: &BTreeMap<String, NodeTemplate>,
    r: &mut BTreeSet<Requirement>,
) {
    let ModuleDefBuilder {
        declaration_ids,
        claims: _,
        contract_requests: _,
        id: _,
        scope: _,
        nodes,
        node_names,
        node_ids,
        next_node_id: _,
        instances,
        edges,
        next_connection: _,
        inputs,
        output: _,
        triggers,
    } = &g.module;
    let names = nodes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| n.name.clone().map(|n| (n, i)))
        .collect::<BTreeMap<_, _>>();
    let ids = nodes.iter().map(|n| n.id.0).collect::<BTreeSet<_>>();
    if &names != node_names
        || &ids != node_ids
        || ids.len() != nodes.len()
        || names.len() != nodes.len()
        || declaration_ids
            .keys()
            .any(|n| !instances.iter().any(|i| &i.name == n))
        || instances
            .iter()
            .any(|i| !declaration_ids.contains_key(&i.name) || names.contains_key(&i.name))
        || instances
            .iter()
            .map(|i| &i.name)
            .collect::<BTreeSet<_>>()
            .len()
            != instances.len()
        || pending
            .iter()
            .any(|(name, n)| !names.contains_key(name) || n.name.as_ref() != Some(name))
        || inputs
            .iter()
            .any(|i| !g.public_input_ids.contains_key(&i.name))
        || g.public_input_ids
            .keys()
            .any(|n| !inputs.iter().any(|i| &i.name == n))
        || inputs
            .iter()
            .map(|i| &i.name)
            .collect::<BTreeSet<_>>()
            .len()
            != inputs.len()
        || triggers
            .iter()
            .any(|t| g.trigger_names.get(&t.name) != Some(&t.id.0))
        || g.trigger_names
            .iter()
            .any(|(n, id)| !triggers.iter().any(|t| &t.name == n && t.id.0 == *id))
        || triggers
            .iter()
            .map(|t| &t.name)
            .collect::<BTreeSet<_>>()
            .len()
            != triggers.len()
        || edges
            .iter()
            .map(|e| e.declaration_id)
            .collect::<BTreeSet<_>>()
            .len()
            != edges.len()
    {
        r.insert(Requirement::Graph);
    }
}

pub(super) fn observe(
    before: &GraphBuilder,
    pending: &BTreeMap<String, NodeTemplate>,
    after: &GraphBuilder,
) -> Observation {
    let GraphBuilder {
        active_origin,
        composition_origins,
        module: _,
        pending: _,
        dependencies,
        identities,
        definition_id: _,
        public_input_ids,
        trigger_names,
        compile_limits,
    } = before;
    let mut r = BTreeSet::new();
    let new_pending = after.pending.borrow();
    validate(before, pending, &mut r);
    validate(after, &new_pending, &mut r);
    let a = &before.module;
    let b = &after.module;
    if active_origin != &after.active_origin
        || a.id != b.id
        || a.scope != b.scope
        || !limits(compile_limits, &after.compile_limits)
        || !claims(&a.claims, &b.claims)
    {
        r.insert(Requirement::Graph);
    }
    // definition_id and pending Rc identity alone are deliberately clone-normalized.
    let old_nodes = a
        .nodes
        .iter()
        .filter_map(|n| n.name.as_ref().map(|name| (name.clone(), n)))
        .collect::<BTreeMap<_, _>>();
    let new_nodes = b
        .nodes
        .iter()
        .filter_map(|n| n.name.as_ref().map(|name| (name.clone(), n)))
        .collect::<BTreeMap<_, _>>();
    for name in keys(&old_nodes, &new_nodes).union(&keys(pending, &new_pending)) {
        if !map_value(
            old_nodes.get(name).copied(),
            new_nodes.get(name).copied(),
            node,
        ) || !map_value(pending.get(name), new_pending.get(name), node)
        {
            r.insert(Requirement::Node(name.clone()));
        }
    }
    let old_instances = a
        .instances
        .iter()
        .map(|i| (i.name.clone(), i))
        .collect::<BTreeMap<_, _>>();
    let new_instances = b
        .instances
        .iter()
        .map(|i| (i.name.clone(), i))
        .collect::<BTreeMap<_, _>>();
    for name in
        keys(&old_instances, &new_instances).union(&keys(&a.declaration_ids, &b.declaration_ids))
    {
        if !map_value(
            old_instances.get(name).copied(),
            new_instances.get(name).copied(),
            instance,
        ) || a.declaration_ids.get(name) != b.declaration_ids.get(name)
        {
            r.insert(Requirement::Instance(name.clone()));
        }
    }
    let mut own = owners(before, &mut r);
    for (id, aliases) in owners(after, &mut r) {
        own.entry(id).or_default().extend(aliases);
    }
    for id in keys(dependencies, &after.dependencies).union(&keys(identities, &after.identities)) {
        if !map_value(dependencies.get(id), after.dependencies.get(id), entry)
            || identities.get(id) != after.identities.get(id)
        {
            charge(own.get(id), &mut r);
        }
        for g in [before, after] {
            if g.dependencies
                .get(id)
                .is_some_and(|e| &e.definition.id != id)
                || g.dependencies.contains_key(id) != g.identities.contains_key(id)
            {
                r.insert(Requirement::Graph);
            }
        }
    }
    let mut origin_owners = BTreeMap::<u64, BTreeSet<String>>::new();
    for g in [before, after] {
        for (name, token) in &g.module.declaration_ids {
            if g.module.instances.iter().any(|i| &i.name == name) {
                origin_owners
                    .entry(*token)
                    .or_default()
                    .insert(name.clone());
            }
        }
        for (id, entry) in &g.dependencies {
            if let Some(aliases) = own.get(id) {
                for token in entry.definition.declaration_ids.values() {
                    origin_owners
                        .entry(*token)
                        .or_default()
                        .extend(aliases.iter().cloned());
                }
            }
        }
    }
    for token in keys(composition_origins, &after.composition_origins) {
        if composition_origins.get(&token) != after.composition_origins.get(&token) {
            charge(origin_owners.get(&token), &mut r);
        }
    }

    // Derive only actual cross-kind endpoint substitutions. Other cells stay owned
    // by the public export, including metadata, multiplicity and target order.
    let mut targets = Vec::new();
    let mut sources = Vec::new();
    let mut lifts = Vec::new();
    for name in keys(&old_nodes, &new_nodes) {
        let transition =
            if let (Some(n), Some(i)) = (old_nodes.get(&name), new_instances.get(&name)) {
                after
                    .dependencies
                    .get(&i.definition)
                    .map(|e| (pending.get(&name).unwrap_or(n), &e.definition, true))
            } else if let (Some(i), Some(n)) = (old_instances.get(&name), new_nodes.get(&name)) {
                before
                    .dependencies
                    .get(&i.definition)
                    .map(|e| (new_pending.get(&name).unwrap_or(n), &e.definition, false))
            } else {
                None
            };
        let Some((n, m, to_module)) = transition else {
            continue;
        };
        if !to_module {
            let block = m
                .contract_requests
                .iter()
                .map(|(path, c)| {
                    (
                        if path.is_empty() {
                            name.clone()
                        } else {
                            format!("{name}/{path}")
                        },
                        c.clone(),
                    )
                })
                .collect::<Vec<_>>();
            if !block.is_empty() {
                lifts.push(block);
            }
        }
        for i in &m.inputs {
            let Some(p) = n.ports.iter().find(|p| p.name.as_ref() == Some(&i.port)) else {
                continue;
            };
            let Some(s) = p.slots.iter().find(|s| s.name.as_ref() == Some(&i.slot)) else {
                continue;
            };
            let nn = ModuleInputTarget::NodeNamed {
                node: name.clone(),
                port: i.port.clone(),
                slot: i.slot.clone(),
            };
            let mn = ModuleInputTarget::InstanceNamed {
                name: name.clone(),
                port: i.name.clone(),
            };
            if to_module {
                targets.push((nn, mn.clone()));
                targets.push((
                    ModuleInputTarget::Node(InputAddr {
                        node: n.id,
                        port: p.id,
                        slot: s.id,
                    }),
                    mn,
                ));
            } else {
                targets.push((mn, nn.clone()));
                targets.push((
                    ModuleInputTarget::Instance {
                        name: name.clone(),
                        port: i.id,
                    },
                    nn,
                ));
            }
        }
        if let (Some(o), Some(output_name)) = (&m.output, &n.output.name) {
            let nn = ModuleOutputSource::NodeNamed {
                node: name.clone(),
                output: output_name.clone(),
            };
            let mn = ModuleOutputSource::InstanceNamed {
                name: name.clone(),
                port: o.name.clone(),
            };
            if to_module {
                sources.push((nn, mn.clone()));
                sources.push((
                    ModuleOutputSource::Node(OutputAddr {
                        node: n.id,
                        slot: n.output.id,
                    }),
                    mn,
                ));
            } else {
                sources.push((mn, nn.clone()));
                sources.push((
                    ModuleOutputSource::Instance {
                        name: name.clone(),
                        port: o.id,
                    },
                    nn,
                ));
            }
        }
    }
    let retarget_input = |v: &ModuleInputTarget| {
        targets
            .iter()
            .find(|(from, _)| from == v)
            .map_or_else(|| v.clone(), |(_, to)| to.clone())
    };
    let retarget_output = |v: &ModuleOutputSource| {
        sources
            .iter()
            .find(|(from, _)| from == v)
            .map_or_else(|| v.clone(), |(_, to)| to.clone())
    };
    let prefix = a.contract_requests.len();
    let mut suffix = b.contract_requests.get(prefix..).unwrap_or(&[]);
    let mut exact_lift = b.contract_requests.len() >= prefix
        && requests(&a.contract_requests, &b.contract_requests[..prefix]);
    // Different transitions have different first path segments, so block starts
    // are unambiguous even when contracts or requests repeat within a block.
    while !suffix.is_empty() {
        let Some(index) = lifts.iter().position(|block| {
            suffix.len() >= block.len() && requests(block, &suffix[..block.len()])
        }) else {
            exact_lift = false;
            break;
        };
        suffix = &suffix[lifts.remove(index).len()..];
    }
    if !exact_lift || !lifts.is_empty() {
        r.insert(Requirement::Graph);
    }

    let old_inputs = a
        .inputs
        .iter()
        .map(|i| (i.name.clone(), i))
        .collect::<BTreeMap<_, _>>();
    let new_inputs = b
        .inputs
        .iter()
        .map(|i| (i.name.clone(), i))
        .collect::<BTreeMap<_, _>>();
    for name in
        keys(&old_inputs, &new_inputs).union(&keys(public_input_ids, &after.public_input_ids))
    {
        let equal = map_value(
            old_inputs.get(name).copied(),
            new_inputs.get(name).copied(),
            |old, new| {
                let mut expected = old.clone();
                expected.targets = old.targets.iter().map(&retarget_input).collect();
                input(&expected, new)
            },
        );
        if !equal || public_input_ids.get(name) != after.public_input_ids.get(name) {
            r.insert(Requirement::PublicInput(name.clone()));
        }
    }
    if !optional(&a.output, &b.output, |old, new| {
        let mut expected = old.clone();
        expected.source = retarget_output(&old.source);
        output(&expected, new)
    }) {
        r.extend(
            a.output
                .iter()
                .chain(&b.output)
                .map(|o| Requirement::PublicOutput(o.name.clone())),
        );
    }
    let old_triggers = a
        .triggers
        .iter()
        .map(|t| (t.name.clone(), t))
        .collect::<BTreeMap<_, _>>();
    let new_triggers = b
        .triggers
        .iter()
        .map(|t| (t.name.clone(), t))
        .collect::<BTreeMap<_, _>>();
    for name in keys(&old_triggers, &new_triggers).union(&keys(trigger_names, &after.trigger_names))
    {
        if !map_value(
            old_triggers.get(name).copied(),
            new_triggers.get(name).copied(),
            trigger,
        ) || trigger_names.get(name) != after.trigger_names.get(name)
        {
            r.insert(Requirement::Trigger(name.clone()));
        }
    }
    if !survivors(
        &a.nodes.iter().map(|n| &n.name).collect::<Vec<_>>(),
        &b.nodes.iter().map(|n| &n.name).collect::<Vec<_>>(),
    ) || !survivors(
        &a.instances.iter().map(|i| &i.name).collect::<Vec<_>>(),
        &b.instances.iter().map(|i| &i.name).collect::<Vec<_>>(),
    ) || !survivors(
        &a.inputs.iter().map(|i| &i.name).collect::<Vec<_>>(),
        &b.inputs.iter().map(|i| &i.name).collect::<Vec<_>>(),
    ) || !survivors(
        &a.triggers.iter().map(|t| &t.name).collect::<Vec<_>>(),
        &b.triggers.iter().map(|t| &t.name).collect::<Vec<_>>(),
    ) {
        r.insert(Requirement::Graph);
    }
    let mut occupied = a.node_ids.clone();
    for n in &a.nodes {
        if !b
            .nodes
            .iter()
            .any(|new| new.name == n.name && new.id == n.id)
        {
            occupied.remove(&n.id.0);
        }
    }
    let mut cursor = a.next_node_id;
    for n in &b.nodes {
        if let Some(old) = old_nodes.get(n.name.as_deref().unwrap_or("")) {
            if old.id != n.id {
                r.insert(Requirement::Graph);
            }
        } else {
            occupied.insert(n.id.0);
            while cursor <= u16::MAX as u32 && occupied.contains(&(cursor as u16)) {
                cursor += 1;
            }
        }
    }
    if cursor != b.next_node_id {
        r.insert(Requirement::Graph);
    }
    let old_edges = a
        .edges
        .iter()
        .map(|e| (e.declaration_id, e))
        .collect::<BTreeMap<_, _>>();
    let new_edges = b
        .edges
        .iter()
        .map(|e| (e.declaration_id, e))
        .collect::<BTreeMap<_, _>>();
    for id in keys(&old_edges, &new_edges) {
        if !map_value(
            old_edges.get(&id).copied(),
            new_edges.get(&id).copied(),
            edge,
        ) {
            for e in old_edges.get(&id).into_iter().chain(new_edges.get(&id)) {
                r.insert(Requirement::Connection(patch_connection_key(
                    &e.from, &e.to,
                )));
            }
        }
        if let (Some(old), Some(new)) = (old_edges.get(&id), new_edges.get(&id))
            && (retarget_input(&old.to) != new.to || retarget_output(&old.from) != new.from)
        {
            // Retained record IDs are valid only for unchanged records or
            // the checked in-place cross-kind endpoint substitution.
            r.insert(Requirement::Graph);
        }
    }
    let mut cursor = a.next_connection;
    let mut appended = false;
    for e in &b.edges {
        if !old_edges.contains_key(&e.declaration_id) {
            appended = true;
            if cursor.checked_add(1) != Some(e.declaration_id) {
                r.insert(Requirement::Graph);
            }
            cursor = e.declaration_id;
        } else if appended {
            r.insert(Requirement::Graph);
        }
    }
    if cursor != b.next_connection
        || !survivors(
            &a.edges.iter().map(|e| e.declaration_id).collect::<Vec<_>>(),
            &b.edges.iter().map(|e| e.declaration_id).collect::<Vec<_>>(),
        )
    {
        r.insert(Requirement::Graph);
    }

    // Legacy diagnostics intentionally cannot represent removals, same-name
    // exports, or Graph residuals. They are never used for coverage.
    let mut events = Vec::new();
    for n in &b.nodes {
        if old_nodes
            .get(n.name.as_deref().unwrap_or(""))
            .is_some_and(|old| !claims(&old.claims, &n.claims))
        {
            events.push(AssemblyChange::NodeReplaced(
                n.name.clone().unwrap_or_default(),
            ));
        }
    }
    if !claims(&a.claims, &b.claims) || !requests(&a.contract_requests, &b.contract_requests) {
        events.push(AssemblyChange::InstanceRebound(String::new()));
    }
    for i in &b.instances {
        if old_instances.contains_key(&i.name) && r.contains(&Requirement::Instance(i.name.clone()))
        {
            events.push(AssemblyChange::InstanceRebound(i.name.clone()));
        }
    }
    events.extend(assembly_changes(before, after));
    let mut seen = Vec::new();
    events.retain(|e| {
        if seen.contains(e) {
            false
        } else {
            seen.push(e.clone());
            true
        }
    });
    Observation {
        required: r.into_iter().collect(),
        events,
    }
}

#[cfg(test)]
mod tests;
