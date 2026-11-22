//! Ephemeral bounded expansion, lowering, and final compilation checks.

use super::validation::{qualify, validate_network};
use super::{
    CompiledNetwork, ComponentSnapshot, DeclarationLeases, FlatConnection, FlatProcess, NetworkDef,
    NetworkError, ResolvedBoundary,
};
use crate::component::Body;
use crate::contract::{Contract, catalog};
use crate::interface::InterfacePhase;
use crate::{CompileLimit, CompileLimits, GraphOrigin, LimitKind};
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::sync::Arc;

impl NetworkDef {
    pub fn compile(&self) -> Result<CompiledNetwork, NetworkError> {
        self.compile_with_limits(CompileLimits::default())
    }
    pub fn compile_with_limits(
        &self,
        limits: CompileLimits,
    ) -> Result<CompiledNetwork, NetworkError> {
        Compiler::new(limits).compile(self)
    }
}

/// Bounded Q2 fast path: a previously valid compilation stays valid when the
/// *only* change to the frozen network is removal of explicitly started
/// zero-input root primitives. Every physical index, interface, contract,
/// requirement, boundary, queue and factory is unchanged; only the list of
/// automatically scheduled sources shrinks. Removing roots cannot increase
/// compile work/instance/depth charges, so a previous default-limits success
/// remains a default-limits success.
impl CompiledNetwork {
    pub(crate) fn reuse_after_source_pruning(
        &self,
        before: &NetworkDef,
        after: &NetworkDef,
    ) -> Option<Self> {
        if after.sources.len() >= before.sources.len() || !same_compilation_scope(before, after) {
            return None;
        }
        // Replay of the same immutable declaration may produce a new Arc
        // wrapper with the same DefinitionKey (for example, host origin
        // stamping). Check every author-visible claim and origin as well.
        // Composite copy-on-write revisions always receive a fresh key.
        if !same_declarations(before, after) {
            return None;
        }
        let remaining: BTreeSet<&str> = after.sources.iter().map(String::as_str).collect();
        if !after
            .sources
            .iter()
            .all(|path| before.sources.contains(path))
        {
            return None;
        }
        let mut removed_indices = BTreeSet::new();
        for path in before
            .sources
            .iter()
            .filter(|path| !remaining.contains(path.as_str()))
        {
            let definition = before.instances.get(path)?;
            if !matches!(definition.body(), Body::Primitive(_))
                || !definition.interface().inputs().is_empty()
            {
                return None;
            }
            let index = self.inspections.get(path)?.physical_index?;
            if !self.sources.contains(&index) {
                return None;
            }
            removed_indices.insert(index);
        }
        if removed_indices.len() != before.sources.len() - after.sources.len() {
            return None;
        }
        let mut compiled = self.with_fresh_generation();
        compiled
            .sources
            .retain(|index| !removed_indices.contains(index));
        // Each revoked direct primitive start removes one root-source
        // declaration and one resolved physical source charge.
        let saved_work = u64::try_from(removed_indices.len()).ok()?.checked_mul(2)?;
        compiled.compile_work = compiled.compile_work.checked_sub(saved_work)?;
        Some(compiled)
    }

    /// A changed selected root primitive does not require graph flattening
    /// if *every* other structural field is identical, the port interface and
    /// semantic claims are unchanged, and the replacement remains primitive.
    /// Patch only affected physical factories and authoritative declarations.
    /// No handler is spawned and no old generation binding is transferable.
    pub(crate) fn reuse_after_root_primitive_replacements(
        &self,
        before: &NetworkDef,
        after: &NetworkDef,
    ) -> Option<Self> {
        if before.sources != after.sources || !same_compilation_scope(before, after) {
            return None;
        }
        let mut replacements = Vec::new();
        for (path, old) in &before.instances {
            let new = after.instances.get(path)?;
            if old.definition_key() == new.definition_key()
                && old.interface() == new.interface()
                && old.claims() == new.claims()
                && old.origin() == new.origin()
            {
                continue;
            }
            // Primitive replacement preserves physical indexing, all port
            // identities, and the complete contract catalog. Nested modules,
            // altered claims, and changed interfaces still compile normally.
            if old.interface() != new.interface() || old.claims() != new.claims() {
                return None;
            }
            if !matches!(old.body(), Body::Primitive(_)) {
                return None;
            }
            let Body::Primitive(factory) = new.body() else {
                return None;
            };
            let index = self.inspections.get(path)?.physical_index?;
            if self.processes.get(index)?.path != *path {
                return None;
            }
            replacements.push((path.as_str(), new.as_ref(), index, factory.clone()));
        }
        if replacements.is_empty() {
            return None;
        }
        let mut compiled = self.with_fresh_generation();
        for (path, definition, index, factory) in replacements {
            let process = compiled.processes.get_mut(index)?;
            process.factory = factory;
            process.interface = definition.interface().clone();
            process.origin = definition
                .origin()
                .cloned()
                .or_else(|| after.origin.clone());
            let declaration = compiled.inspections.get_mut(path)?;
            declaration.interface = definition.interface().clone();
            declaration.claims = definition.claims().to_vec();
            declaration.origin = process.origin.clone();
            declaration.declared_origin = definition.origin().cloned();
            declaration.definition_key = Some(definition.definition_key());
        }
        Some(compiled)
    }

    /// Strictly monotone disconnection of direct root primitive edges. Every
    /// remaining edge keeps its capacity and ordered physical endpoints; no
    /// component, boundary, claim, source or requirement changes. Deleting
    /// edges cannot create an invalid port type or exceed a compiler budget.
    /// Nested/module edges and any additions always use canonical lowering.
    pub(crate) fn reuse_after_root_connection_pruning(
        &self,
        before: &NetworkDef,
        after: &NetworkDef,
    ) -> Option<Self> {
        if after.connections.len() >= before.connections.len()
            || before.sources != after.sources
            || !same_scope_without_connections(before, after)
            || !same_declarations(before, after)
        {
            return None;
        }
        let old: BTreeSet<_> = before
            .connections
            .iter()
            .map(|edge| (&edge.from, &edge.to, edge.capacity))
            .collect();
        let desired: BTreeSet<_> = after
            .connections
            .iter()
            .map(|edge| (&edge.from, &edge.to, edge.capacity))
            .collect();
        if old.len() != before.connections.len()
            || desired.len() != after.connections.len()
            || !desired.is_subset(&old)
        {
            return None;
        }
        let mut removed = BTreeSet::new();
        for edge in before
            .connections
            .iter()
            .filter(|edge| !desired.contains(&(&edge.from, &edge.to, edge.capacity)))
        {
            let from = before.instances.get(&edge.from.process)?;
            let to = before.instances.get(&edge.to.process)?;
            if !matches!(from.body(), Body::Primitive(_))
                || !matches!(to.body(), Body::Primitive(_))
            {
                return None;
            }
            let from_index = self.inspections.get(&edge.from.process)?.physical_index?;
            let to_index = self.inspections.get(&edge.to.process)?.physical_index?;
            removed.insert((
                from_index,
                edge.from.port.as_str(),
                to_index,
                edge.to.port.as_str(),
                edge.capacity,
            ));
        }
        if removed.len() != before.connections.len() - after.connections.len() {
            return None;
        }
        let mut compiled = self.with_fresh_generation();
        let old_count = compiled.connections.len();
        compiled.connections.retain(|edge| {
            !removed.contains(&(
                edge.from.0,
                edge.from.1.as_str(),
                edge.to.0,
                edge.to.1.as_str(),
                edge.capacity,
            ))
        });
        if old_count - compiled.connections.len() != removed.len() {
            return None;
        }
        // One symbolic root edge and exactly one primitive target per revoke.
        let saved_work = u64::try_from(removed.len()).ok()?.checked_mul(2)?;
        compiled.compile_work = compiled.compile_work.checked_sub(saved_work)?;
        Some(compiled)
    }

    /// Monotone root edge additions preserve every flattened process/boundary.
    /// One direct primitive-to-primitive edge charges exactly one symbolic
    /// root connection plus one physical target under the default compiler.
    /// Reject changed capacities, duplicates, nested endpoints, mixed edits
    /// and unbudgeted additions; revalidate exact port type identities.
    pub(crate) fn reuse_after_root_connection_additions(
        &self,
        before: &NetworkDef,
        after: &NetworkDef,
    ) -> Option<Self> {
        if after.connections.len() <= before.connections.len()
            || before.sources != after.sources
            || !same_scope_without_connections(before, after)
            || !same_declarations(before, after)
            || self.compilation_limits != CompileLimits::default()
        {
            return None;
        }
        let old: BTreeSet<_> = before
            .connections
            .iter()
            .map(|edge| (&edge.from, &edge.to, edge.capacity))
            .collect();
        let new: BTreeSet<_> = after
            .connections
            .iter()
            .map(|edge| (&edge.from, &edge.to, edge.capacity))
            .collect();
        let endpoints: BTreeSet<_> = after
            .connections
            .iter()
            .map(|edge| (&edge.from, &edge.to))
            .collect();
        if old.len() != before.connections.len()
            || new.len() != after.connections.len()
            || endpoints.len() != after.connections.len()
            || !old.is_subset(&new)
        {
            return None;
        }
        let added = after.connections.len() - before.connections.len();
        let extra_work = u64::try_from(added).ok()?.checked_mul(2)?;
        let work = self.compile_work.checked_add(extra_work)?;
        if work > self.compilation_limits.max_work {
            return None;
        }
        let mut physical_edges = Vec::with_capacity(added);
        for edge in after
            .connections
            .iter()
            .filter(|edge| !old.contains(&(&edge.from, &edge.to, edge.capacity)))
        {
            let from = before.instances.get(&edge.from.process)?;
            let to = before.instances.get(&edge.to.process)?;
            if !matches!(from.body(), Body::Primitive(_))
                || !matches!(to.body(), Body::Primitive(_))
            {
                return None;
            }
            let output = from.interface().output(&edge.from.port)?;
            let input = to.interface().input(&edge.to.port)?;
            if output.value != input.value {
                return None;
            }
            let from_index = self.inspections.get(&edge.from.process)?.physical_index?;
            let to_index = self.inspections.get(&edge.to.process)?.physical_index?;
            if self.processes.get(from_index)?.path != edge.from.process
                || self.processes.get(to_index)?.path != edge.to.process
            {
                return None;
            }
            physical_edges.push(FlatConnection {
                from: (from_index, edge.from.port.clone()),
                to: (to_index, edge.to.port.clone()),
                capacity: edge.capacity,
            });
        }
        if physical_edges.len() != added {
            return None;
        }
        let mut compiled = self.with_fresh_generation();
        compiled.connections.extend(physical_edges);
        compiled
            .connections
            .sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
        compiled.compile_work = work;
        Some(compiled)
    }
    /// A selected Connect that changes only the bounded FIFO capacity of an
    /// existing direct root primitive edge does not change graph lowering.
    /// Every declaration, endpoint, projection and compile-work charge stays
    /// identical. Patch the physical capacity only, with new binding authority.
    /// Added/removed edges, nested endpoints, or mixed edits compile normally.
    pub(crate) fn reuse_after_root_connection_capacity_changes(
        &self,
        before: &NetworkDef,
        after: &NetworkDef,
    ) -> Option<Self> {
        if before.connections.len() != after.connections.len()
            || before.sources != after.sources
            || !same_scope_without_connections(before, after)
            || !same_declarations(before, after)
        {
            return None;
        }
        let before_edges: BTreeMap<_, _> = before
            .connections
            .iter()
            .map(|edge| ((&edge.from, &edge.to), edge.capacity))
            .collect();
        let after_edges: BTreeMap<_, _> = after
            .connections
            .iter()
            .map(|edge| ((&edge.from, &edge.to), edge.capacity))
            .collect();
        if before_edges.len() != before.connections.len()
            || after_edges.len() != after.connections.len()
            || !before_edges.keys().eq(after_edges.keys())
        {
            return None;
        }
        let mut changes = BTreeMap::new();
        for edge in &after.connections {
            if before_edges.get(&(&edge.from, &edge.to)) == Some(&edge.capacity) {
                continue;
            }
            let from = before.instances.get(&edge.from.process)?;
            let to = before.instances.get(&edge.to.process)?;
            if !matches!(from.body(), Body::Primitive(_))
                || !matches!(to.body(), Body::Primitive(_))
            {
                return None;
            }
            let from_index = self.inspections.get(&edge.from.process)?.physical_index?;
            let to_index = self.inspections.get(&edge.to.process)?.physical_index?;
            if self.processes.get(from_index)?.path != edge.from.process
                || self.processes.get(to_index)?.path != edge.to.process
            {
                return None;
            }
            changes.insert(
                (
                    from_index,
                    edge.from.port.as_str(),
                    to_index,
                    edge.to.port.as_str(),
                ),
                edge.capacity,
            );
        }
        if changes.is_empty() {
            return None;
        }
        let mut compiled = self.with_fresh_generation();
        let mut updated = 0;
        for edge in &mut compiled.connections {
            if let Some(capacity) = changes.get(&(
                edge.from.0,
                edge.from.1.as_str(),
                edge.to.0,
                edge.to.1.as_str(),
            )) {
                edge.capacity = *capacity;
                updated += 1;
            }
        }
        (updated == changes.len()).then_some(compiled)
    }


    /// Symmetric mixed revocation: a selected root primitive and only its
    /// direct incident root edges are removed in one transaction. First prune
    /// connections while the old primitive still exists; then revoke its
    /// isolated declaration. Both existing bounded Q2 validators must accept.
    /// This is not a general graph-cut or arbitrary multi-component deletion.
    pub(crate) fn reuse_after_connected_root_removal(
        &self,
        before: &NetworkDef,
        after: &NetworkDef,
    ) -> Option<Self> {
        if before.instances.len() != after.instances.len().checked_add(1)?
            || before.connections.len() <= after.connections.len()
        {
            return None;
        }
        let mut removed = before
            .instances
            .keys()
            .filter(|name| !after.instances.contains_key(*name));
        let name = removed.next()?;
        if removed.next().is_some() {
            return None;
        }
        let after_edges: BTreeSet<_> = after
            .connections
            .iter()
            .map(|edge| (&edge.from, &edge.to, edge.capacity))
            .collect();
        let mut found = false;
        for edge in &before.connections {
            if after_edges.contains(&(&edge.from, &edge.to, edge.capacity)) {
                continue;
            }
            if edge.from.process != *name && edge.to.process != *name {
                return None;
            }
            found = true;
        }
        if !found {
            return None;
        }
        let mut intermediate = before.clone();
        intermediate.connections = after.connections.clone();
        let pruned = self.reuse_after_root_connection_pruning(before, &intermediate)?;
        pruned.reuse_after_isolated_root_removal(&intermediate, after)
    }
    /// Reuse existing, independently bounded lowering steps for a selected
    /// Add + Connect transaction. Only one new root primitive and monotone
    /// direct edges are eligible; at least one edge must attach to that new
    /// component. Reconstruct a private intermediate network with the new
    /// primitive but no new edges, then run both strict Q2 validators.
    ///
    /// Each stage pays exact instance/work charges and issues fresh authority.
    /// No intermediate generation escapes: Composer publishes only the final
    /// validated graph, or falls back to canonical compilation atomically.
    pub(crate) fn reuse_after_connected_root_addition(
        &self,
        before: &NetworkDef,
        after: &NetworkDef,
    ) -> Option<Self> {
        if after.instances.len() != before.instances.len().checked_add(1)?
            || after.connections.len() <= before.connections.len()
        {
            return None;
        }
        let mut inserted = after
            .instances
            .keys()
            .filter(|name| !before.instances.contains_key(*name));
        let name = inserted.next()?;
        if inserted.next().is_some() {
            return None;
        }
        let old_edges: BTreeSet<_> = before
            .connections
            .iter()
            .map(|edge| (&edge.from, &edge.to, edge.capacity))
            .collect();
        if !after.connections.iter().any(|edge| {
            (edge.from.process == *name || edge.to.process == *name)
                && !old_edges.contains(&(&edge.from, &edge.to, edge.capacity))
        }) {
            return None;
        }
        let mut intermediate = after.clone();
        intermediate.connections = before.connections.clone();
        let inserted = self.reuse_after_isolated_root_addition(before, &intermediate)?;
        inserted.reuse_after_root_connection_additions(&intermediate, after)
    }
    /// Exactly one new, unclaimed, unreferenced root primitive can be inserted
    /// without expanding the existing graph. Reindex all physical references
    /// and pay the *exact* new compiler charges against stored default limits.
    /// Never accept mixed changes, already claimed contracts, custom-limit
    /// baselines, or a change to an existing declaration.
    pub(crate) fn reuse_after_isolated_root_addition(
        &self,
        before: &NetworkDef,
        after: &NetworkDef,
    ) -> Option<Self> {
        if after.instances.len() != before.instances.len().checked_add(1)?
            || before.connections != after.connections
            || before.sources != after.sources
            || !same_scope_except_instances(before, after)
            || !same_declarations(before, after)
            || self.compilation_limits != CompileLimits::default()
        {
            return None;
        }
        let mut added = after
            .instances
            .iter()
            .filter(|(name, _)| !before.instances.contains_key(*name));
        let (name, definition) = added.next()?;
        if added.next().is_some()
            || !matches!(definition.body(), Body::Primitive(_))
            || !definition.claims().is_empty()
        {
            return None;
        }
        super::validation::validate_process_name(name).ok()?;
        // The old successful graph already validated every connection, source,
        // boundary and obligation, and those declarations are *identical*.
        // One isolated primitive changes root instance charge (+1), its own
        // declared inputs/outputs and one physical instance, but no fanouts,
        // root projections, starts, requirements or nested expansion.
        let added_work = u64::try_from(
            definition
                .interface()
                .inputs()
                .len()
                .checked_add(definition.interface().outputs().len())?
                .checked_add(1)?,
        )
        .ok()?;
        let next_work = self.compile_work.checked_add(added_work)?;
        let next_instances = self.compile_instances.checked_add(1)?;
        if next_work > self.compilation_limits.max_work
            || next_instances > self.compilation_limits.max_instances
            || self.compilation_limits.max_depth == 0
        {
            return None;
        }
        let insertion = self
            .processes
            .binary_search_by(|process| process.path.as_str().cmp(name.as_str()))
            .err()?;
        let Body::Primitive(factory) = definition.body() else {
            return None;
        };
        let mut compiled = self.with_fresh_generation();
        compiled.shift_physical_indexes(insertion, true);
        let origin = definition
            .origin()
            .cloned()
            .or_else(|| after.origin.clone());
        compiled.processes.insert(
            insertion,
            FlatProcess {
                path: name.clone(),
                interface: definition.interface().clone(),
                factory: factory.clone(),
                origin: origin.clone(),
            },
        );
        compiled.inspections.insert(
            name.clone(),
            ComponentSnapshot {
                interface: definition.interface().clone(),
                claims: Vec::new(),
                origin: origin.clone(),
                declared_origin: definition.origin().cloned(),
                definition_key: Some(definition.definition_key()),
                physical_index: Some(insertion),
                path: name.clone(),
            },
        );
        compiled.boundaries.insert(
            name.clone(),
            ResolvedBoundary {
                inputs: definition
                    .interface()
                    .inputs()
                    .keys()
                    .map(|port| (port.clone(), vec![(insertion, port.clone())]))
                    .collect(),
                outputs: definition
                    .interface()
                    .outputs()
                    .keys()
                    .map(|port| (port.clone(), (insertion, port.clone())))
                    .collect(),
            },
        );
        compiled.compile_work = next_work;
        compiled.compile_instances = next_instances;
        Some(compiled)
    }

    /// Rebase every stored physical reference (including composite public
    /// projections) after one insertion or one already-checked removal.
    /// Removal callers guarantee no surviving reference equals the removed
    /// index; therefore they shift only later indexes.
    fn shift_physical_indexes(&mut self, start: usize, insertion: bool) {
        let shift = |index: &mut usize| {
            if *index >= start {
                if insertion {
                    *index += 1;
                } else {
                    *index -= 1;
                }
            }
        };
        for snapshot in self.inspections.values_mut() {
            if let Some(index) = &mut snapshot.physical_index {
                shift(index);
            }
        }
        for edge in &mut self.connections {
            shift(&mut edge.from.0);
            shift(&mut edge.to.0);
        }
        for targets in self.inputs.values_mut() {
            for (index, _) in targets {
                shift(index);
            }
        }
        for (index, _) in self.outputs.values_mut() {
            shift(index);
        }
        for index in &mut self.sources {
            shift(index);
        }
        for boundary in self.boundaries.values_mut() {
            for targets in boundary.inputs.values_mut() {
                for (index, _) in targets {
                    shift(index);
                }
            }
            for (index, _) in boundary.outputs.values_mut() {
                shift(index);
            }
        }
    }
    /// A selected, isolated root primitive can be revoked without flattening
    /// unchanged declarations. Remove its one physical process and rebase all
    /// later physical indexes, preserving exact compiled order. Reject claims,
    /// obligations, starts, edges, exports, and replacement seams: none can be
    /// silently discarded with the component. Never reuse binding authority.
    pub(crate) fn reuse_after_isolated_root_removal(
        &self,
        before: &NetworkDef,
        after: &NetworkDef,
    ) -> Option<Self> {
        if after.instances.len().checked_add(1) != Some(before.instances.len())
            || before.connections != after.connections
            || before.sources != after.sources
            || !same_scope_except_instances(before, after)
            || !same_declarations(after, before)
        {
            return None;
        }
        let mut removed = before
            .instances
            .iter()
            .filter(|(name, _)| !after.instances.contains_key(*name));
        let (name, definition) = removed.next()?;
        if removed.next().is_some()
            || !matches!(definition.body(), Body::Primitive(_))
            || !definition.claims().is_empty()
            || before.replacement_points.contains(name)
            || before.sources.contains(name)
            || before
                .requirements
                .iter()
                .any(|(path, _)| path.split('/').next() == Some(name.as_str()))
            || before
                .connections
                .iter()
                .any(|edge| edge.from.process == *name || edge.to.process == *name)
            || before.inputs.values().any(|projection| {
                projection
                    .targets
                    .iter()
                    .any(|target| target.process == *name)
            })
            || before
                .outputs
                .values()
                .any(|source| source.process == *name)
        {
            return None;
        }
        let removed_index = self.inspections.get(name)?.physical_index?;
        if self.processes.get(removed_index)?.path != *name || !self.boundaries.contains_key(name) {
            return None;
        }
        // Every compiled reference outside the removed declaration must
        // survive reindexing; never silently redirect a former endpoint.
        if self.sources.contains(&removed_index)
            || self
                .connections
                .iter()
                .any(|edge| edge.from.0 == removed_index || edge.to.0 == removed_index)
            || self
                .inputs
                .values()
                .flatten()
                .any(|(index, _)| *index == removed_index)
            || self
                .outputs
                .values()
                .any(|(index, _)| *index == removed_index)
            || self.boundaries.iter().any(|(path, boundary)| {
                path != name
                    && (boundary
                        .inputs
                        .values()
                        .flatten()
                        .any(|(index, _)| *index == removed_index)
                        || boundary
                            .outputs
                            .values()
                            .any(|(index, _)| *index == removed_index))
            })
        {
            return None;
        }
        let removed_work = u64::try_from(
            definition
                .interface()
                .inputs()
                .len()
                .checked_add(definition.interface().outputs().len())?
                .checked_add(definition.claims().len())?
                .checked_add(1)?,
        )
        .ok()?;
        let mut compiled = self.with_fresh_generation();
        compiled.processes.remove(removed_index);
        compiled.inspections.remove(name)?;
        compiled.boundaries.remove(name)?;
        compiled.shift_physical_indexes(removed_index.checked_add(1)?, false);
        compiled.compile_work = compiled.compile_work.checked_sub(removed_work)?;
        compiled.compile_instances = compiled.compile_instances.checked_sub(1)?;
        Some(compiled)
    }
}

/// These fixed fields are verified before any incremental reuse; changing
/// a contract, export, origin or author-approved extension seam is never
/// eligible. Each supported delta may relax only its own exact dimension.
fn same_scope_except_instances(before: &NetworkDef, after: &NetworkDef) -> bool {
    before.interface == after.interface
        && before.inputs == after.inputs
        && before.outputs == after.outputs
        && before.claims == after.claims
        && before.origin == after.origin
        && before.requirements == after.requirements
        && before.replacement_points == after.replacement_points
        && before.extensions_allowed == after.extensions_allowed
}

fn same_scope_without_connections(before: &NetworkDef, after: &NetworkDef) -> bool {
    before.instances.len() == after.instances.len() && same_scope_except_instances(before, after)
}

fn same_compilation_scope(before: &NetworkDef, after: &NetworkDef) -> bool {
    before.connections == after.connections && same_scope_without_connections(before, after)
}

fn same_declarations(before: &NetworkDef, after: &NetworkDef) -> bool {
    before.instances.iter().all(|(name, old)| {
        after.instances.get(name).is_some_and(|new| {
            old.definition_key() == new.definition_key()
                && old.interface() == new.interface()
                && old.claims() == new.claims()
                && old.origin() == new.origin()
        })
    })
}

type PhysicalPort = (String, String);
#[derive(Default, Clone)]
struct Boundary {
    inputs: BTreeMap<String, Vec<PhysicalPort>>,
    outputs: BTreeMap<String, PhysicalPort>,
    sources: BTreeSet<String>,
}
struct Compiler {
    limits: CompileLimits,
    instances: u64,
    work: u64,
    active: BTreeSet<usize>,
    processes: BTreeMap<String, FlatProcess>,
    connections: Vec<(PhysicalPort, PhysicalPort, NonZeroUsize)>,
    edges: BTreeSet<(PhysicalPort, PhysicalPort)>,
    inspections: BTreeMap<String, ComponentSnapshot>,
    boundaries: BTreeMap<String, Boundary>,
    requirements: Vec<(String, Contract)>,
}
impl Compiler {
    fn new(limits: CompileLimits) -> Self {
        Self {
            limits,
            instances: 0,
            work: 0,
            active: BTreeSet::new(),
            processes: BTreeMap::new(),
            connections: Vec::new(),
            edges: BTreeSet::new(),
            inspections: BTreeMap::new(),
            boundaries: BTreeMap::new(),
            requirements: Vec::new(),
        }
    }
    fn charge(&mut self, amount: usize, path: &str) -> Result<(), NetworkError> {
        let attempted = u64::try_from(amount)
            .ok()
            .and_then(|amount| self.work.checked_add(amount));
        self.work = attempted
            .filter(|work| *work <= self.limits.max_work)
            .ok_or_else(|| NetworkError::LimitExceeded {
                limit: CompileLimit {
                    kind: LimitKind::Work,
                    configured: self.limits.max_work,
                    attempted,
                },
                path: path.into(),
            })?;
        Ok(())
    }
    fn compile(mut self, network: &NetworkDef) -> Result<CompiledNetwork, NetworkError> {
        let boundary = self.expand(network, "", 0, network.origin.as_ref())?;
        self.inspections.insert(
            String::new(),
            ComponentSnapshot {
                interface: network.interface.clone(),
                claims: network.claims.clone(),
                origin: network.origin.clone(),
                declared_origin: network.origin.clone(),
                definition_key: None,
                physical_index: None,
                path: String::new(),
            },
        );
        self.boundaries.insert(String::new(), boundary.clone());
        let contracts: Vec<_> = self
            .inspections
            .values()
            .flat_map(|s| s.claims.iter().cloned())
            .chain(self.requirements.iter().map(|(_, c)| c.clone()))
            .collect();
        catalog(&contracts)?;
        for (path, contract) in &self.requirements {
            let snapshot = self
                .inspections
                .get(path)
                .ok_or_else(|| NetworkError::UnknownProcess(path.clone()))?;
            contract
                .check(&snapshot.interface, &snapshot.claims)
                .map_err(|e| e.context(InterfacePhase::Compiled, path, Some(contract.id())))?;
        }
        let indexes: BTreeMap<_, _> = self
            .processes
            .keys()
            .enumerate()
            .map(|(index, path)| (path.clone(), index))
            .collect();
        let lower = |port: PhysicalPort| (indexes[&port.0], port.1);
        for (path, snapshot) in &mut self.inspections {
            snapshot.physical_index = indexes.get(path).copied();
        }
        let boundaries = self
            .boundaries
            .into_iter()
            .map(|(path, boundary)| {
                (
                    path,
                    ResolvedBoundary {
                        inputs: boundary
                            .inputs
                            .into_iter()
                            .map(|(name, ports)| (name, ports.into_iter().map(&lower).collect()))
                            .collect(),
                        outputs: boundary
                            .outputs
                            .into_iter()
                            .map(|(name, port)| (name, lower(port)))
                            .collect(),
                    },
                )
            })
            .collect();
        let mut connections: Vec<_> = self
            .connections
            .into_iter()
            .map(|(from, to, capacity)| FlatConnection {
                from: lower(from),
                to: lower(to),
                capacity,
            })
            .collect();
        connections.sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
        let inputs = boundary
            .inputs
            .into_iter()
            .map(|(name, targets)| (name, targets.into_iter().map(&lower).collect()))
            .collect();
        Ok(CompiledNetwork {
            inspections: self.inspections,
            boundaries,
            contracts,
            generation: Arc::new(()),
            leases: DeclarationLeases::default(),
            interface: network.interface.clone(),
            processes: self.processes.into_values().collect(),
            connections,
            inputs,
            ingress_capacities: network
                .inputs
                .iter()
                .map(|(name, projection)| (name.clone(), projection.capacity))
                .collect(),
            outputs: boundary
                .outputs
                .into_iter()
                .map(|(name, port)| (name, lower(port)))
                .collect(),
            sources: boundary
                .sources
                .into_iter()
                .map(|path| indexes[&path])
                .collect(),
            compilation_limits: self.limits,
            compile_work: self.work,
            compile_instances: self.instances,
        })
    }
    fn expand(
        &mut self,
        network: &NetworkDef,
        prefix: &str,
        depth: usize,
        inherited_origin: Option<&GraphOrigin>,
    ) -> Result<Boundary, NetworkError> {
        let identity = std::ptr::from_ref(network) as usize;
        if !self.active.insert(identity) {
            return Err(NetworkError::RecursiveComposite(prefix.into()));
        }
        // Charge declarations before validation allocates any of its working sets.
        self.charge(network.instances.len(), prefix)?;
        self.charge(network.connections.len(), prefix)?;
        self.charge(network.interface.inputs().len(), prefix)?;
        self.charge(network.interface.outputs().len(), prefix)?;
        self.charge(network.sources.len(), prefix)?;
        for projection in network.inputs.values() {
            self.charge(projection.targets.len(), prefix)?;
        }
        validate_network(network)?;
        self.charge(network.claims.len(), prefix)?;
        self.charge(network.requirements.len(), prefix)?;
        self.requirements.extend(
            network
                .requirements
                .iter()
                .map(|(path, c)| (qualify(prefix, path), c.clone())),
        );
        let mut children = BTreeMap::new();
        for (name, component) in &network.instances {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if depth >= self.limits.max_depth {
                return Err(NetworkError::LimitExceeded {
                    limit: CompileLimit {
                        kind: LimitKind::Depth,
                        configured: self.limits.max_depth as u64,
                        attempted: u64::try_from(depth)
                            .ok()
                            .and_then(|depth| depth.checked_add(1)),
                    },
                    path,
                });
            }
            let attempted = self.instances.checked_add(1);
            self.instances = attempted
                .filter(|count| *count <= self.limits.max_instances)
                .ok_or_else(|| NetworkError::LimitExceeded {
                    limit: CompileLimit {
                        kind: LimitKind::Instances,
                        configured: self.limits.max_instances,
                        attempted,
                    },
                    path: path.clone(),
                })?;
            self.charge(component.interface.inputs().len(), &path)?;
            self.charge(component.interface.outputs().len(), &path)?;
            // Closest explicit declaration origin wins; inheritance is provenance
            // only and cannot establish semantic membership or host authorization.
            let origin = component.origin.as_ref().or(inherited_origin);
            self.charge(component.claims.len(), &path)?;
            self.inspections.insert(
                path.clone(),
                ComponentSnapshot {
                    interface: component.interface.clone(),
                    claims: component.claims.clone(),
                    origin: origin.cloned(),
                    declared_origin: component.origin.clone(),
                    definition_key: Some(component.definition_key()),
                    physical_index: None,
                    path: path.clone(),
                },
            );
            let boundary = match &component.body {
                Body::Primitive(factory) => {
                    self.processes.insert(
                        path.clone(),
                        FlatProcess {
                            path: path.clone(),
                            interface: component.interface.clone(),
                            factory: factory.clone(),
                            origin: origin.cloned(),
                        },
                    );
                    Boundary {
                        inputs: component
                            .interface
                            .inputs()
                            .keys()
                            .map(|port| (port.clone(), vec![(path.clone(), port.clone())]))
                            .collect(),
                        outputs: component
                            .interface
                            .outputs()
                            .keys()
                            .map(|port| (port.clone(), (path.clone(), port.clone())))
                            .collect(),
                        sources: if component.interface.inputs().is_empty() {
                            BTreeSet::from([path.clone()])
                        } else {
                            BTreeSet::new()
                        },
                    }
                }
                Body::Composite(child) => self.expand(child, &path, depth + 1, origin)?,
            };
            self.boundaries.insert(path, boundary.clone());
            children.insert(name.clone(), boundary);
        }
        for edge in &network.connections {
            let from = &children[&edge.from.process].outputs[&edge.from.port];
            let targets = &children[&edge.to.process].inputs[&edge.to.port];
            self.charge(targets.len(), prefix)?;
            for to in targets {
                if !self.edges.insert((from.clone(), to.clone())) {
                    return Err(NetworkError::DuplicateConnection {
                        from: edge.from.clone(),
                        to: edge.to.clone(),
                    });
                }
                self.connections
                    .push((from.clone(), to.clone(), edge.capacity));
            }
        }
        let mut boundary = Boundary::default();
        for (name, projection) in &network.inputs {
            let mut targets = BTreeSet::new();
            for target in &projection.targets {
                let resolved = &children[&target.process].inputs[&target.port];
                self.charge(resolved.len(), prefix)?;
                for target in resolved {
                    if !targets.insert(target.clone()) {
                        return Err(NetworkError::InvalidProjection(name.clone()));
                    }
                }
            }
            boundary
                .inputs
                .insert(name.clone(), targets.into_iter().collect());
        }
        for (name, source) in &network.outputs {
            boundary.outputs.insert(
                name.clone(),
                children[&source.process].outputs[&source.port].clone(),
            );
        }
        for source in &network.sources {
            let resolved = &children[source].sources;
            self.charge(resolved.len(), prefix)?;
            boundary.sources.extend(resolved.iter().cloned());
        }
        self.active.remove(&identity);
        Ok(boundary)
    }
}

#[cfg(test)]
mod isolated_root_addition_tests {
    use super::*;
    use crate::compose::{Composer, Edit, Footprint, NetworkPatch, Registration};
    use crate::network::{ConnectionDef, NetworkBuilder, PortPath};

    fn base() -> (NetworkDef, Contract) {
        let mut builder = NetworkBuilder::new();
        let source = builder
            .add(
                "alpha",
                Arc::new(crate::ComponentDef::source(|| std::iter::once(7_i64))),
            )
            .unwrap();
        builder.start(&source).unwrap();
        let worker = crate::ComponentDef::map(|n: i64| n * 2);
        let contract = Contract::new("test/add-root-omega", worker.interface().clone()).unwrap();
        let omega = builder
            .add("omega", Arc::new(worker.claim(&contract).unwrap()))
            .unwrap();
        builder
            .expose_output("out", omega.output::<i64>("output").unwrap())
            .unwrap();
        let mut network = builder.build().unwrap();
        network.connections.push(ConnectionDef {
            from: PortPath {
                process: "alpha".into(),
                port: "output".into(),
            },
            to: PortPath {
                process: "omega".into(),
                port: "input".into(),
            },
            capacity: NonZeroUsize::new(1).unwrap(),
        });
        (network, contract)
    }

    fn with_middle(mut network: NetworkDef) -> NetworkDef {
        network.instances.insert(
            "middle".into(),
            Arc::new(crate::ComponentDef::map(|n: i64| n)),
        );
        network
    }

    #[test]
    fn isolated_middle_addition_matches_full_compiler_rebases_and_executes() {
        let (before, contract) = base();
        let old = before.compile().unwrap();
        let old_handle = old
            .bind("omega", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        let after = with_middle(before.clone());
        let incremental = old
            .reuse_after_isolated_root_addition(&before, &after)
            .unwrap();
        let canonical = after.compile().unwrap();
        assert_eq!(incremental.processes().len(), 3);
        assert_eq!(
            incremental.inspect("middle").unwrap().physical_index,
            Some(1)
        );
        assert_eq!(
            incremental.inspect("omega").unwrap().physical_index,
            Some(2)
        );
        assert_eq!(incremental.connections(), canonical.connections());
        assert_eq!(incremental.compile_work, canonical.compile_work);
        assert_eq!(incremental.source_starts(), canonical.source_starts());
        assert_eq!(incremental.inputs(), canonical.inputs());
        assert_eq!(incremental.outputs(), canonical.outputs());
        assert_eq!(
            incremental.structural_reachability(),
            canonical.structural_reachability()
        );
        assert_eq!(
            incremental.structural_feedback_cycles(),
            canonical.structural_feedback_cycles()
        );
        for path in ["", "alpha", "middle", "omega"] {
            let a = incremental.inspect(path).unwrap();
            let b = canonical.inspect(path).unwrap();
            assert_eq!(a.physical_index, b.physical_index);
            assert_eq!(a.definition_key, b.definition_key);
            assert_eq!(a.origin, b.origin);
        }
        assert_eq!(incremental.compile_work, canonical.compile_work);
        assert_eq!(incremental.compile_instances, canonical.compile_instances);
        assert_eq!(
            old_handle.resolve(&incremental),
            Err(NetworkError::ForeignScope)
        );
        let bound = incremental.declaration("middle").unwrap();
        assert!(bound.input::<i64>("input").is_ok());
        let mut session = incremental.start();
        session.run_until_idle(64);
        assert_eq!(
            session
                .take_output("out")
                .unwrap()
                .unwrap()
                .value()
                .borrow::<i64>(),
            Some(&14)
        );
    }

    #[test]
    fn rejects_claims_multiple_additions_declaration_mixed_and_invalid_name() {
        let (before, contract) = base();
        let old = before.compile().unwrap();
        let mut after = with_middle(before.clone());
        after.instances.insert(
            "second".into(),
            Arc::new(crate::ComponentDef::map(|n: i64| n)),
        );
        assert!(
            old.reuse_after_isolated_root_addition(&before, &after)
                .is_none()
        );
        after = with_middle(before.clone());
        after.instances.insert(
            "middle".into(),
            Arc::new(
                crate::ComponentDef::map(|n: i64| n)
                    .claim(&contract)
                    .unwrap(),
            ),
        );
        assert!(
            old.reuse_after_isolated_root_addition(&before, &after)
                .is_none()
        );
        after = with_middle(before.clone());
        after.connections.clear();
        assert!(
            old.reuse_after_isolated_root_addition(&before, &after)
                .is_none()
        );
        after = with_middle(before.clone());
        after.instances.insert(
            "omega".into(),
            Arc::new(crate::ComponentDef::map(|n: i64| n * 3)),
        );
        assert!(
            old.reuse_after_isolated_root_addition(&before, &after)
                .is_none()
        );
        after = before.clone();
        after.instances.insert(
            "../escape".into(),
            Arc::new(crate::ComponentDef::map(|n: i64| n)),
        );
        assert!(
            old.reuse_after_isolated_root_addition(&before, &after)
                .is_none()
        );
    }

    #[test]
    fn custom_limits_and_exhausted_default_limits_fall_back() {
        let (before, _) = base();
        let limits = CompileLimits {
            max_instances: 2,
            ..CompileLimits::default()
        };
        let old = before.compile_with_limits(limits).unwrap();
        let after = with_middle(before.clone());
        assert!(
            old.reuse_after_isolated_root_addition(&before, &after)
                .is_none()
        );
        assert!(matches!(
            after.compile_with_limits(limits),
            Err(NetworkError::LimitExceeded { .. })
        ));
        let mut simulated_full = before.compile().unwrap();
        simulated_full.compile_instances = CompileLimits::default().max_instances;
        assert!(
            simulated_full
                .reuse_after_isolated_root_addition(&before, &after)
                .is_none()
        );
        simulated_full = before.compile().unwrap();
        simulated_full.compile_work = CompileLimits::default().max_work;
        assert!(
            simulated_full
                .reuse_after_isolated_root_addition(&before, &after)
                .is_none()
        );
    }

    #[test]
    fn real_composer_add_revoke_and_readmit_isolated_root() {
        let (before, contract) = base();
        let mut composer = Composer::new(before).unwrap();
        let patch = NetworkPatch::new([Edit::Add {
            path: "middle".into(),
            definition: Arc::new(crate::ComponentDef::map(|n: i64| n)),
        }]);
        let registration = |id: &str| {
            Registration::new(
                id,
                [],
                1,
                GraphOrigin::default(),
                [Footprint::Component("middle".into())],
                patch.clone(),
            )
        };
        let initial = composer
            .current()
            .compiled()
            .bind("omega", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        composer.register([registration("one")]).unwrap();
        assert_eq!(
            composer
                .current()
                .compiled()
                .inspect("middle")
                .unwrap()
                .physical_index,
            Some(1)
        );
        assert_eq!(
            composer
                .current()
                .compiled()
                .inspect("omega")
                .unwrap()
                .physical_index,
            Some(2)
        );
        assert_eq!(
            initial.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        assert_eq!(
            composer.current().compiled().connections(),
            composer
                .current()
                .network()
                .compile()
                .unwrap()
                .connections()
        );
        composer.revoke("one").unwrap();
        assert!(composer.current().compiled().inspect("middle").is_err());
        composer.register([registration("two")]).unwrap();
        assert_eq!(
            composer
                .current()
                .compiled()
                .inspect("middle")
                .unwrap()
                .physical_index,
            Some(1)
        );
        assert_eq!(
            composer.current().compiled().compile_work,
            composer.current().network().compile().unwrap().compile_work
        );
        assert_eq!(
            composer.current().compiled().compile_instances,
            composer
                .current()
                .network()
                .compile()
                .unwrap()
                .compile_instances
        );
    }
}

#[cfg(test)]
mod isolated_root_removal_tests {
    use super::*;
    use crate::compose::{Composer, Edit, Footprint, NetworkPatch, Registration};
    use crate::network::{ConnectionDef, NetworkBuilder, PortPath};

    fn edge() -> ConnectionDef {
        ConnectionDef {
            from: PortPath {
                process: "alpha".into(),
                port: "output".into(),
            },
            to: PortPath {
                process: "omega".into(),
                port: "input".into(),
            },
            capacity: NonZeroUsize::new(1).unwrap(),
        }
    }

    fn fixture(with_middle: bool) -> (NetworkDef, Contract) {
        let mut builder = NetworkBuilder::new();
        let source = builder
            .add(
                "alpha",
                Arc::new(crate::ComponentDef::source(|| std::iter::once(7_i64))),
            )
            .unwrap();
        builder.start(&source).unwrap();
        if with_middle {
            builder
                .add("middle", Arc::new(crate::ComponentDef::map(|n: i64| n)))
                .unwrap();
        }
        let worker = crate::ComponentDef::map(|n: i64| n * 2);
        let contract = Contract::new("test/isolated-omega", worker.interface().clone()).unwrap();
        let omega = builder
            .add("omega", Arc::new(worker.claim(&contract).unwrap()))
            .unwrap();
        builder
            .expose_output("out", omega.output::<i64>("output").unwrap())
            .unwrap();
        let mut network = builder.build().unwrap();
        network.connections.push(edge());
        (network, contract)
    }

    #[test]
    fn middle_process_removal_rebases_every_physical_index_and_matches_full_compiler() {
        let (before, contract) = fixture(true);
        let old = before.compile().unwrap();
        assert_eq!(old.inspect("omega").unwrap().physical_index, Some(2));
        let old_handle = old
            .bind("omega", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        let mut after = before.clone();
        after.instances.remove("middle");
        let incremental = old
            .reuse_after_isolated_root_removal(&before, &after)
            .unwrap();
        let canonical = after.compile().unwrap();
        assert_eq!(incremental.processes().len(), 2);
        assert!(matches!(
            incremental.inspect("middle"),
            Err(NetworkError::UnknownProcess(_))
        ));
        assert_eq!(
            incremental.inspect("omega").unwrap().physical_index,
            Some(1)
        );
        assert_eq!(incremental.connections(), canonical.connections());
        assert_eq!(incremental.source_starts(), canonical.source_starts());
        assert_eq!(incremental.inputs(), canonical.inputs());
        assert_eq!(incremental.outputs(), canonical.outputs());
        assert_eq!(
            incremental.structural_reachability(),
            canonical.structural_reachability()
        );
        assert_eq!(
            incremental.structural_feedback_cycles(),
            canonical.structural_feedback_cycles()
        );
        assert_eq!(
            old_handle.resolve(&incremental),
            Err(NetworkError::ForeignScope)
        );
        assert!(!Arc::ptr_eq(&old.generation, &incremental.generation));
        let mut session = incremental.start();
        session.run_until_idle(64);
        assert_eq!(
            session
                .take_output("out")
                .unwrap()
                .unwrap()
                .value()
                .borrow::<i64>(),
            Some(&14)
        );
    }

    #[test]
    fn referenced_claimed_and_mixed_delta_components_force_full_compiler() {
        let (before, contract) = fixture(true);
        let old = before.compile().unwrap();
        let mut after = before.clone();
        after.instances.remove("middle");
        after.connections.clear();
        assert!(
            old.reuse_after_isolated_root_removal(&before, &after)
                .is_none()
        );

        after = before.clone();
        after.instances.remove("middle");
        after.instances.insert(
            "omega".into(),
            Arc::new(
                crate::ComponentDef::map(|n: i64| n * 3)
                    .claim(&contract)
                    .unwrap(),
            ),
        );
        assert!(
            old.reuse_after_isolated_root_removal(&before, &after)
                .is_none()
        );

        let mut connected = before.clone();
        connected.connections.push(ConnectionDef {
            from: edge().from,
            to: PortPath {
                process: "middle".into(),
                port: "input".into(),
            },
            capacity: NonZeroUsize::new(1).unwrap(),
        });
        let compiled = connected.compile().unwrap();
        let connected_before = connected.clone();
        connected.instances.remove("middle");
        assert!(
            compiled
                .reuse_after_isolated_root_removal(&connected_before, &connected)
                .is_none()
        );

        let mut claimed = before.clone();
        claimed.instances.insert(
            "middle".into(),
            Arc::new(
                crate::ComponentDef::map(|n: i64| n)
                    .claim(&contract)
                    .unwrap(),
            ),
        );
        let compiled = claimed.compile().unwrap();
        let mut removed = claimed.clone();
        removed.instances.remove("middle");
        assert!(
            compiled
                .reuse_after_isolated_root_removal(&claimed, &removed)
                .is_none()
        );

        let mut obligated = before.clone();
        obligated.requirements.push(("middle".into(), contract));
        let compiled = obligated.compile();
        // A contract obligation on an unclaimed orphan is invalid already.
        assert!(compiled.is_err());
    }

    #[test]
    fn composer_add_and_revoke_isolated_middle_node_preserves_other_bindings_only_in_same_generation()
     {
        let (base, contract) = fixture(false);
        let mut composer = Composer::new(base).unwrap();
        let patch = NetworkPatch::new([Edit::Add {
            path: "middle".into(),
            definition: Arc::new(crate::ComponentDef::map(|n: i64| n)),
        }]);
        let contribution = |id: &str| {
            Registration::new(
                id,
                [],
                3,
                GraphOrigin::default(),
                [Footprint::Component("middle".into())],
                patch.clone(),
            )
        };
        composer.register([contribution("orphan")]).unwrap();
        assert_eq!(
            composer
                .current()
                .compiled()
                .inspect("omega")
                .unwrap()
                .physical_index,
            Some(2)
        );
        let previous = composer
            .current()
            .compiled()
            .bind("omega", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        assert_eq!(
            composer.revoke("orphan").unwrap(),
            BTreeSet::from(["orphan".into()])
        );
        assert!(matches!(
            composer.current().compiled().inspect("middle"),
            Err(NetworkError::UnknownProcess(_))
        ));
        assert_eq!(
            composer
                .current()
                .compiled()
                .inspect("omega")
                .unwrap()
                .physical_index,
            Some(1)
        );
        assert_eq!(
            previous.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        assert_eq!(
            composer.current().compiled().connections(),
            composer
                .current()
                .network()
                .compile()
                .unwrap()
                .connections()
        );
        composer.register([contribution("new-orphan")]).unwrap();
        assert_eq!(
            composer
                .current()
                .compiled()
                .inspect("omega")
                .unwrap()
                .physical_index,
            Some(2)
        );
    }
}

#[cfg(test)]
mod capacity_reuse_tests {
    use super::*;
    use crate::compose::{Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration};
    use crate::network::{ConnectionDef, NetworkBuilder, PortPath};

    fn edge(capacity: usize) -> ConnectionDef {
        ConnectionDef {
            from: PortPath {
                process: "source".into(),
                port: "output".into(),
            },
            to: PortPath {
                process: "worker".into(),
                port: "input".into(),
            },
            capacity: NonZeroUsize::new(capacity).unwrap(),
        }
    }

    fn base() -> (NetworkDef, Contract) {
        let mut builder = NetworkBuilder::new();
        let source = builder
            .add(
                "source",
                Arc::new(crate::ComponentDef::source(|| std::iter::once(7_i64))),
            )
            .unwrap();
        builder.start(&source).unwrap();
        let definition = crate::ComponentDef::map(|n: i64| n * 2);
        let contract =
            Contract::new("test/capacity-worker", definition.interface().clone()).unwrap();
        let worker = builder
            .add("worker", Arc::new(definition.claim(&contract).unwrap()))
            .unwrap();
        builder
            .expose_output("out", worker.output::<i64>("output").unwrap())
            .unwrap();
        (builder.build().unwrap(), contract)
    }

    #[test]
    fn physical_capacity_change_matches_fresh_compile_and_invalidates_old_handles() {
        let (mut before, contract) = base();
        before.connections.push(edge(1));
        let old = before.compile().unwrap();
        let handle = old
            .bind("worker", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        let mut after = before.clone();
        after.connections[0].capacity = NonZeroUsize::new(8).unwrap();
        let incremental = old
            .reuse_after_root_connection_capacity_changes(&before, &after)
            .unwrap();
        let canonical = after.compile().unwrap();
        assert_eq!(incremental.connections(), canonical.connections());
        assert_eq!(incremental.source_starts(), canonical.source_starts());
        assert_eq!(incremental.inputs(), canonical.inputs());
        assert_eq!(incremental.outputs(), canonical.outputs());
        assert_eq!(
            incremental.connections()[0].capacity(),
            NonZeroUsize::new(8).unwrap()
        );
        assert_eq!(
            old.connections()[0].capacity(),
            NonZeroUsize::new(1).unwrap()
        );
        assert_eq!(
            handle.resolve(&incremental),
            Err(NetworkError::ForeignScope)
        );
        assert!(!Arc::ptr_eq(&old.generation, &incremental.generation));
        let mut session = incremental.start();
        session.run_until_idle(64);
        assert_eq!(
            session
                .take_output("out")
                .unwrap()
                .unwrap()
                .value()
                .borrow::<i64>(),
            Some(&14)
        );
    }

    #[test]
    fn mixed_endpoints_declarations_and_duplicate_edges_fall_back() {
        let (mut before, contract) = base();
        before.connections.push(edge(1));
        let old = before.compile().unwrap();
        let mut after = before.clone();
        assert!(
            old.reuse_after_root_connection_capacity_changes(&before, &after)
                .is_none()
        );
        after.connections.push(edge(2));
        assert!(
            old.reuse_after_root_connection_capacity_changes(&before, &after)
                .is_none()
        );
        after = before.clone();
        after.connections.clear();
        assert!(
            old.reuse_after_root_connection_capacity_changes(&before, &after)
                .is_none()
        );
        after = before.clone();
        after.connections[0].to.port = "unknown".into();
        assert!(
            old.reuse_after_root_connection_capacity_changes(&before, &after)
                .is_none()
        );
        after = before.clone();
        after.connections[0].capacity = NonZeroUsize::new(4).unwrap();
        after.instances.insert(
            "worker".into(),
            Arc::new(
                crate::ComponentDef::map(|n: i64| n * 3)
                    .claim(&contract)
                    .unwrap(),
            ),
        );
        assert!(
            old.reuse_after_root_connection_capacity_changes(&before, &after)
                .is_none()
        );
        after = before.clone();
        after.connections[0].capacity = NonZeroUsize::new(4).unwrap();
        after.outputs.clear();
        assert!(
            old.reuse_after_root_connection_capacity_changes(&before, &after)
                .is_none()
        );
    }

    #[test]
    fn selected_connect_preemption_and_revoke_updates_capacity_without_transferring_authority() {
        let (network, contract) = base();
        let mut composer = Composer::new(network).unwrap();
        let registration = |id: &str, priority: i64, capacity| {
            let connection = edge(capacity);
            Registration::new(
                id,
                [],
                priority,
                GraphOrigin::default(),
                [Footprint::Connection(ConnectionKey::new(
                    connection.from.clone(),
                    connection.to.clone(),
                ))],
                NetworkPatch::new([Edit::Connect(connection)]),
            )
        };
        composer.register([registration("low", 1, 1)]).unwrap();
        let low = composer
            .current()
            .compiled()
            .bind("worker", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        composer.register([registration("high", 5, 4)]).unwrap();
        assert_eq!(
            composer.current().compiled().connections()[0].capacity(),
            NonZeroUsize::new(4).unwrap()
        );
        assert_eq!(
            low.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        assert_eq!(
            composer.current().compiled().connections(),
            composer
                .current()
                .network()
                .compile()
                .unwrap()
                .connections()
        );
        let high = composer
            .current()
            .compiled()
            .bind("worker", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        assert_eq!(
            composer.revoke("high").unwrap(),
            BTreeSet::from(["high".into()])
        );
        assert_eq!(
            composer.current().compiled().connections()[0].capacity(),
            NonZeroUsize::new(1).unwrap()
        );
        assert_eq!(
            high.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        assert_eq!(
            composer.current().compiled().connections(),
            composer
                .current()
                .network()
                .compile()
                .unwrap()
                .connections()
        );
    }
}


#[cfg(test)]
mod connected_root_revocation_tests {
    use super::*;
    use crate::compose::{Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration};
    use crate::network::{ConnectionDef, NetworkBuilder, PortPath};

    fn edge(from: &str, to: &str) -> ConnectionDef {
        ConnectionDef {
            from: PortPath {
                process: from.into(),
                port: "output".into(),
            },
            to: PortPath {
                process: to.into(),
                port: "input".into(),
            },
            capacity: NonZeroUsize::new(2).unwrap(),
        }
    }

    fn base() -> (NetworkDef, Contract) {
        let mut builder = NetworkBuilder::new();
        let source = builder
            .add(
                "alpha",
                Arc::new(crate::ComponentDef::source(|| std::iter::once(7_i64))),
            )
            .unwrap();
        builder.start(&source).unwrap();
        let worker = crate::ComponentDef::map(|n: i64| n * 3);
        let contract = Contract::new("test/connected-remove", worker.interface().clone()).unwrap();
        let omega = builder
            .add("omega", Arc::new(worker.claim(&contract).unwrap()))
            .unwrap();
        builder
            .expose_output("out", omega.output::<i64>("output").unwrap())
            .unwrap();
        let mut network = builder.build().unwrap();
        network.connections.push(edge("alpha", "omega"));
        (network, contract)
    }

    fn with_chain(mut base: NetworkDef) -> NetworkDef {
        base.instances.insert(
            "middle".into(),
            Arc::new(crate::ComponentDef::map(|n: i64| n + 1)),
        );
        base.connections.push(edge("alpha", "middle"));
        base.connections.push(edge("middle", "omega"));
        base
    }

    #[test]
    fn removing_connected_middle_preserves_unrelated_edge_and_rebases_indexes() {
        let (after, contract) = base();
        let before = with_chain(after.clone());
        let old = before.compile().unwrap();
        let bound = old
            .bind("omega", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        let incremental = old
            .reuse_after_connected_root_removal(&before, &after)
            .unwrap();
        let canonical = after.compile().unwrap();
        assert_eq!(incremental.processes().len(), 2);
        assert_eq!(incremental.connections(), canonical.connections());
        assert_eq!(incremental.source_starts(), canonical.source_starts());
        assert_eq!(incremental.inputs(), canonical.inputs());
        assert_eq!(incremental.outputs(), canonical.outputs());
        assert_eq!(incremental.compile_work, canonical.compile_work);
        assert_eq!(incremental.compile_instances, canonical.compile_instances);
        assert_eq!(incremental.structural_reachability(), canonical.structural_reachability());
        assert_eq!(incremental.structural_feedback_cycles(), canonical.structural_feedback_cycles());
        assert_eq!(incremental.inspect("omega").unwrap().physical_index, Some(1));
        assert!(incremental.inspect("middle").is_err());
        assert_eq!(bound.resolve(&incremental), Err(NetworkError::ForeignScope));
        let mut session = incremental.start();
        session.run_until_idle(64);
        assert_eq!(
            session.take_output("out").unwrap().unwrap().value().borrow::<i64>(),
            Some(&21)
        );
    }

    #[test]
    fn unrelated_edge_change_and_mixed_components_bypass_reuse() {
        let (after, contract) = base();
        let before = with_chain(after.clone());
        let old = before.compile().unwrap();

        let mut different = after.clone();
        different.connections[0].capacity = NonZeroUsize::new(5).unwrap();
        assert!(old.reuse_after_connected_root_removal(&before, &different).is_none());

        different = after.clone();
        different.instances.insert(
            "omega".into(),
            Arc::new(crate::ComponentDef::map(|n: i64| n * 4).claim(&contract).unwrap()),
        );
        assert!(old.reuse_after_connected_root_removal(&before, &different).is_none());

        different = after.clone();
        different.instances.remove("alpha");
        assert!(old.reuse_after_connected_root_removal(&before, &different).is_none());

        different = after.clone();
        different.outputs.clear();
        assert!(old.reuse_after_connected_root_removal(&before, &different).is_none());

        let mut claimed = before.clone();
        claimed.instances.insert(
            "middle".into(),
            Arc::new(crate::ComponentDef::map(|n: i64| n + 1).claim(&contract).unwrap()),
        );
        let compiled = claimed.compile().unwrap();
        assert!(compiled.reuse_after_connected_root_removal(&claimed, &after).is_none());

        let mut unrelated = before.clone();
        unrelated.connections.push(edge("omega", "alpha"));
        assert!(old.reuse_after_connected_root_removal(&before, &unrelated).is_none());

        let mut without_edges = before.clone();
        without_edges.connections = after.connections.clone();
        assert!(old.reuse_after_connected_root_removal(&before, &without_edges).is_none());
    }

    #[test]
    fn actual_composer_revocation_cleans_edges_and_component_atomically() {
        let (base, contract) = base();
        let from = edge("alpha", "middle");
        let to = edge("middle", "omega");
        let patch = NetworkPatch::new([
            Edit::Add {
                path: "middle".into(),
                definition: Arc::new(crate::ComponentDef::map(|n: i64| n + 1)),
            },
            Edit::Connect(from.clone()),
            Edit::Connect(to.clone()),
        ]);
        let grants = [
            Footprint::Component("middle".into()),
            Footprint::Connection(ConnectionKey::new(from.from.clone(), from.to.clone())),
            Footprint::Connection(ConnectionKey::new(to.from.clone(), to.to.clone())),
        ];
        let registration = Registration::new(
            "chain",
            [],
            1,
            GraphOrigin::default(),
            grants,
            patch,
        );
        let mut composer = Composer::new(base).unwrap();
        composer.register([registration]).unwrap();
        let old = composer.current().compiled()
            .bind("omega", &contract).unwrap().output::<i64>("output").unwrap();
        assert_eq!(composer.current().compiled().connections().len(), 3);
        assert_eq!(
            composer.revoke("chain").unwrap(),
            BTreeSet::from(["chain".into()])
        );
        assert!(composer.current().compiled().inspect("middle").is_err());
        assert_eq!(composer.current().compiled().connections().len(), 1);
        assert_eq!(old.resolve(composer.current().compiled()), Err(NetworkError::ForeignScope));
        let canonical = composer.current().network().compile().unwrap();
        assert_eq!(composer.current().compiled().connections(), canonical.connections());
        assert_eq!(composer.current().compiled().compile_work, canonical.compile_work);
        assert_eq!(composer.current().compiled().compile_instances, canonical.compile_instances);
        let mut session = composer.current().compiled().start();
        session.run_until_idle(64);
        assert_eq!(
            session.take_output("out").unwrap().unwrap().value().borrow::<i64>(),
            Some(&21)
        );
    }
}

#[cfg(test)]
mod connected_root_admission_tests {
    use super::*;
    use crate::compose::{Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration};
    use crate::network::{ConnectionDef, NetworkBuilder, PortPath};

    fn edge(from: &str, to: &str) -> ConnectionDef {
        ConnectionDef {
            from: PortPath {
                process: from.into(),
                port: "output".into(),
            },
            to: PortPath {
                process: to.into(),
                port: "input".into(),
            },
            capacity: NonZeroUsize::new(2).unwrap(),
        }
    }

    fn base() -> (NetworkDef, Contract) {
        let mut builder = NetworkBuilder::new();
        let source = builder
            .add(
                "alpha",
                Arc::new(crate::ComponentDef::source(|| std::iter::once(7_i64))),
            )
            .unwrap();
        builder.start(&source).unwrap();
        let worker = crate::ComponentDef::map(|n: i64| n * 3);
        let contract = Contract::new("test/connected-add", worker.interface().clone()).unwrap();
        let omega = builder
            .add("omega", Arc::new(worker.claim(&contract).unwrap()))
            .unwrap();
        builder
            .expose_output("out", omega.output::<i64>("output").unwrap())
            .unwrap();
        (builder.build().unwrap(), contract)
    }

    fn with_chain(mut before: NetworkDef) -> NetworkDef {
        before.instances.insert(
            "middle".into(),
            Arc::new(crate::ComponentDef::map(|n: i64| n + 1)),
        );
        before.connections.push(edge("alpha", "middle"));
        before.connections.push(edge("middle", "omega"));
        before
    }

    #[test]
    fn two_edge_new_component_chain_matches_canonical_and_emits() {
        let (before, contract) = base();
        let previous = before.compile().unwrap();
        let old_handle = previous
            .bind("omega", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        let after = with_chain(before.clone());
        let incremental = previous
            .reuse_after_connected_root_addition(&before, &after)
            .unwrap();
        let canonical = after.compile().unwrap();
        assert_eq!(incremental.connections(), canonical.connections());
        assert_eq!(incremental.source_starts(), canonical.source_starts());
        assert_eq!(incremental.inputs(), canonical.inputs());
        assert_eq!(incremental.outputs(), canonical.outputs());
        assert_eq!(incremental.compile_work, canonical.compile_work);
        assert_eq!(incremental.compile_instances, canonical.compile_instances);
        assert_eq!(
            incremental.structural_reachability(),
            canonical.structural_reachability()
        );
        assert_eq!(
            incremental.structural_feedback_cycles(),
            canonical.structural_feedback_cycles()
        );
        for path in ["", "alpha", "middle", "omega"] {
            let actual = incremental.inspect(path).unwrap();
            let expected = canonical.inspect(path).unwrap();
            assert_eq!(actual.physical_index, expected.physical_index);
            assert_eq!(actual.definition_key, expected.definition_key);
            assert_eq!(actual.claims, expected.claims);
            assert_eq!(actual.origin, expected.origin);
        }
        assert_eq!(
            old_handle.resolve(&incremental),
            Err(NetworkError::ForeignScope)
        );
        assert_eq!(
            incremental
                .declaration("middle")
                .unwrap()
                .input::<i64>("input")
                .unwrap()
                .resolve(&incremental)
                .unwrap(),
            &[(1, "input".into())]
        );
        let mut session = incremental.start();
        session.run_until_idle(64);
        assert_eq!(
            session
                .take_output("out")
                .unwrap()
                .unwrap()
                .value()
                .borrow::<i64>(),
            Some(&24)
        );
    }

    #[test]
    fn existing_root_edge_survives_new_component_reindexing() {
        let (mut before, _) = base();
        before.connections.push(edge("alpha", "omega"));
        let old = before.compile().unwrap();
        let after = with_chain(before.clone());
        let incremental = old
            .reuse_after_connected_root_addition(&before, &after)
            .unwrap();
        let canonical = after.compile().unwrap();
        assert_eq!(incremental.connections(), canonical.connections());
        assert_eq!(incremental.compile_work, canonical.compile_work);
        assert_eq!(incremental.compile_instances, canonical.compile_instances);
        assert_eq!(
            incremental.inspect("omega").unwrap().physical_index,
            Some(2)
        );
        assert_eq!(incremental.source_starts(), canonical.source_starts());
        assert_eq!(incremental.outputs(), canonical.outputs());
    }

    #[test]
    fn mixed_or_invalid_deltas_fall_back_with_no_partial_publication() {
        let (before, contract) = base();
        let old = before.compile().unwrap();
        let mut after = with_chain(before.clone());
        after.connections.push(edge("alpha", "middle"));
        assert!(
            old.reuse_after_connected_root_addition(&before, &after)
                .is_none()
        );

        after = with_chain(before.clone());
        after.connections[0].capacity = NonZeroUsize::new(3).unwrap();
        after.connections.push(edge("alpha", "middle"));
        assert!(
            old.reuse_after_connected_root_addition(&before, &after)
                .is_none()
        );

        after = with_chain(before.clone());
        after.instances.insert(
            "omega".into(),
            Arc::new(
                crate::ComponentDef::map(|n: i64| n * 4)
                    .claim(&contract)
                    .unwrap(),
            ),
        );
        assert!(
            old.reuse_after_connected_root_addition(&before, &after)
                .is_none()
        );

        after = with_chain(before.clone());
        after.instances.insert(
            "middle".into(),
            Arc::new(
                crate::ComponentDef::map(|n: i64| n + 1)
                    .claim(&contract)
                    .unwrap(),
            ),
        );
        assert!(
            old.reuse_after_connected_root_addition(&before, &after)
                .is_none()
        );

        after = with_chain(before.clone());
        after.instances.insert(
            "extra".into(),
            Arc::new(crate::ComponentDef::map(|n: i64| n)),
        );
        assert!(
            old.reuse_after_connected_root_addition(&before, &after)
                .is_none()
        );

        after = with_chain(before.clone());
        after.sources.push("middle".into());
        assert!(
            old.reuse_after_connected_root_addition(&before, &after)
                .is_none()
        );

        after = with_chain(before.clone());
        after.outputs.clear();
        assert!(
            old.reuse_after_connected_root_addition(&before, &after)
                .is_none()
        );

        after = before.clone();
        after.instances.insert(
            "middle".into(),
            Arc::new(crate::ComponentDef::map(|n: i64| n + 1)),
        );
        after.connections.push(edge("alpha", "omega"));
        assert!(
            old.reuse_after_connected_root_addition(&before, &after)
                .is_none()
        );
    }

    #[test]
    fn work_and_instance_limit_overflow_and_custom_limits_are_rejected() {
        let (before, _) = base();
        let after = with_chain(before.clone());
        let custom = CompileLimits {
            max_instances: 2,
            ..CompileLimits::default()
        };
        let old = before.compile_with_limits(custom).unwrap();
        assert!(
            old.reuse_after_connected_root_addition(&before, &after)
                .is_none()
        );
        assert!(matches!(
            after.compile_with_limits(custom),
            Err(NetworkError::LimitExceeded { .. })
        ));
        let mut artificially_full = before.compile().unwrap();
        artificially_full.compile_instances = CompileLimits::default().max_instances;
        assert!(
            artificially_full
                .reuse_after_connected_root_addition(&before, &after)
                .is_none()
        );
        let mut artificially_full = before.compile().unwrap();
        artificially_full.compile_work = CompileLimits::default().max_work - 2;
        assert!(
            artificially_full
                .reuse_after_connected_root_addition(&before, &after)
                .is_none()
        );
    }

    #[test]
    fn real_composer_admits_two_edges_then_revokes_atomically() {
        let (base, contract) = base();
        let mut composer = Composer::new(base).unwrap();
        let from = edge("alpha", "middle");
        let to = edge("middle", "omega");
        let patch = NetworkPatch::new([
            Edit::Add {
                path: "middle".into(),
                definition: Arc::new(crate::ComponentDef::map(|n: i64| n + 1)),
            },
            Edit::Connect(from.clone()),
            Edit::Connect(to.clone()),
        ]);
        let grants = [
            Footprint::Component("middle".into()),
            Footprint::Connection(ConnectionKey::new(from.from.clone(), from.to.clone())),
            Footprint::Connection(ConnectionKey::new(to.from.clone(), to.to.clone())),
        ];
        let registration = |id: &str| {
            Registration::new(
                id,
                [],
                3,
                GraphOrigin::default(),
                grants.clone(),
                patch.clone(),
            )
        };
        let old = composer
            .current()
            .compiled()
            .bind("omega", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        composer.register([registration("chain")]).unwrap();
        let selected = composer.current().compiled();
        assert_eq!(
            selected.connections(),
            composer
                .current()
                .network()
                .compile()
                .unwrap()
                .connections()
        );
        assert_eq!(
            selected.compile_work,
            composer.current().network().compile().unwrap().compile_work
        );
        assert_eq!(
            selected.compile_instances,
            composer
                .current()
                .network()
                .compile()
                .unwrap()
                .compile_instances
        );
        assert_eq!(old.resolve(selected), Err(NetworkError::ForeignScope));
        let mut session = selected.start();
        session.run_until_idle(64);
        assert_eq!(
            session
                .take_output("out")
                .unwrap()
                .unwrap()
                .value()
                .borrow::<i64>(),
            Some(&24)
        );
        assert_eq!(
            composer.revoke("chain").unwrap(),
            BTreeSet::from(["chain".into()])
        );
        assert!(composer.current().compiled().inspect("middle").is_err());
        assert!(composer.current().compiled().connections().is_empty());
        assert_eq!(
            composer.current().compiled().compile_work,
            composer.current().network().compile().unwrap().compile_work
        );
    }
}

#[cfg(test)]
mod edge_addition_tests {
    use super::*;
    use crate::compose::{Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration};
    use crate::network::{ConnectionDef, NetworkBuilder, PortPath};

    fn endpoint(process: &str, port: &str) -> PortPath {
        PortPath {
            process: process.into(),
            port: port.into(),
        }
    }
    fn edge(from: &str, to: &str, capacity: usize) -> ConnectionDef {
        ConnectionDef {
            from: endpoint(from, "output"),
            to: endpoint(to, "input"),
            capacity: NonZeroUsize::new(capacity).unwrap(),
        }
    }
    fn base() -> (NetworkDef, Contract) {
        let mut builder = NetworkBuilder::new();
        let source = builder
            .add(
                "alpha",
                Arc::new(crate::ComponentDef::source(|| std::iter::once(7_i64))),
            )
            .unwrap();
        builder.start(&source).unwrap();
        let worker = crate::ComponentDef::map(|n: i64| n * 2);
        let contract =
            Contract::new("test/root-edge-addition", worker.interface().clone()).unwrap();
        let omega = builder
            .add("omega", Arc::new(worker.claim(&contract).unwrap()))
            .unwrap();
        builder
            .expose_output("out", omega.output::<i64>("output").unwrap())
            .unwrap();
        (builder.build().unwrap(), contract)
    }

    #[test]
    fn monotone_direct_edge_insert_matches_full_compilation_and_execution() {
        let (before, contract) = base();
        let old = before.compile().unwrap();
        let bound = old
            .bind("omega", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        let mut after = before.clone();
        after.connections.push(edge("alpha", "omega", 5));
        let incremental = old
            .reuse_after_root_connection_additions(&before, &after)
            .unwrap();
        let canonical = after.compile().unwrap();
        assert_eq!(incremental.connections(), canonical.connections());
        assert_eq!(incremental.source_starts(), canonical.source_starts());
        assert_eq!(incremental.inputs(), canonical.inputs());
        assert_eq!(incremental.outputs(), canonical.outputs());
        assert_eq!(incremental.compile_work, canonical.compile_work);
        assert_eq!(incremental.compile_instances, canonical.compile_instances);
        assert_eq!(
            incremental.structural_reachability(),
            canonical.structural_reachability()
        );
        assert_eq!(
            incremental.structural_feedback_cycles(),
            canonical.structural_feedback_cycles()
        );
        assert_eq!(bound.resolve(&incremental), Err(NetworkError::ForeignScope));
        let mut session = incremental.start();
        session.run_until_idle(64);
        assert_eq!(
            session
                .take_output("out")
                .unwrap()
                .unwrap()
                .value()
                .borrow::<i64>(),
            Some(&14)
        );
    }

    #[test]
    fn rejects_duplicate_capacity_type_and_mixed_changes() {
        let (before, contract) = base();
        let old = before.compile().unwrap();
        let mut duplicate = before.clone();
        duplicate.connections.push(edge("alpha", "omega", 1));
        duplicate.connections.push(edge("alpha", "omega", 2));
        assert!(
            old.reuse_after_root_connection_additions(&before, &duplicate)
                .is_none()
        );

        let mut changed = before.clone();
        changed.instances.insert(
            "omega".into(),
            Arc::new(
                crate::ComponentDef::map(|n: i64| n * 3)
                    .claim(&contract)
                    .unwrap(),
            ),
        );
        changed.connections.push(edge("alpha", "omega", 1));
        assert!(
            old.reuse_after_root_connection_additions(&before, &changed)
                .is_none()
        );

        changed = before.clone();
        changed.connections.push(edge("omega", "alpha", 1));
        assert!(
            old.reuse_after_root_connection_additions(&before, &changed)
                .is_none()
        );

        changed = before.clone();
        changed.connections.push(edge("alpha", "omega", 1));
        changed.connections.push(edge("alpha", "missing", 1));
        assert!(
            old.reuse_after_root_connection_additions(&before, &changed)
                .is_none()
        );

        let mut original = before.clone();
        original.connections.push(edge("alpha", "omega", 1));
        let compiled = original.compile().unwrap();
        let mut next = original.clone();
        next.connections[0].capacity = NonZeroUsize::new(2).unwrap();
        next.connections.push(edge("alpha", "omega", 3));
        assert!(
            compiled
                .reuse_after_root_connection_additions(&original, &next)
                .is_none()
        );
    }

    #[test]
    fn work_limit_and_nondefault_compiles_bypass_fast_addition() {
        let (before, _) = base();
        let mut after = before.clone();
        after.connections.push(edge("alpha", "omega", 1));
        let custom = CompileLimits {
            max_work: 100,
            ..CompileLimits::default()
        };
        let compiled = before.compile_with_limits(custom).unwrap();
        assert!(
            compiled
                .reuse_after_root_connection_additions(&before, &after)
                .is_none()
        );
        let mut simulated_full = before.compile().unwrap();
        simulated_full.compile_work = CompileLimits::default().max_work;
        assert!(
            simulated_full
                .reuse_after_root_connection_additions(&before, &after)
                .is_none()
        );
    }

    #[test]
    fn real_composer_root_connect_revocation_and_readmission() {
        let (base, contract) = base();
        let mut composer = Composer::new(base).unwrap();
        let connection = edge("alpha", "omega", 2);
        let contribution = |id: &str| {
            Registration::new(
                id,
                [],
                3,
                GraphOrigin::default(),
                [Footprint::Connection(ConnectionKey::new(
                    connection.from.clone(),
                    connection.to.clone(),
                ))],
                NetworkPatch::new([Edit::Connect(connection.clone())]),
            )
        };
        let old = composer
            .current()
            .compiled()
            .bind("omega", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        composer.register([contribution("first")]).unwrap();
        assert_eq!(composer.current().compiled().connections().len(), 1);
        assert_eq!(
            old.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        assert_eq!(
            composer.current().compiled().connections(),
            composer
                .current()
                .network()
                .compile()
                .unwrap()
                .connections()
        );
        composer.revoke("first").unwrap();
        assert!(composer.current().compiled().connections().is_empty());
        composer.register([contribution("second")]).unwrap();
        assert_eq!(composer.current().compiled().connections().len(), 1);
        assert_eq!(
            composer.current().compiled().connections(),
            composer
                .current()
                .network()
                .compile()
                .unwrap()
                .connections()
        );
    }
}

#[cfg(test)]
mod edge_pruning_tests {
    use super::*;
    use crate::compose::{Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration};
    use crate::network::{ConnectionDef, NetworkBuilder, PortPath};

    fn endpoint(process: &str, port: &str) -> PortPath {
        PortPath {
            process: process.into(),
            port: port.into(),
        }
    }

    fn edge(from: &str, to: &str) -> ConnectionDef {
        ConnectionDef {
            from: endpoint(from, "output"),
            to: endpoint(to, "input"),
            capacity: NonZeroUsize::new(1).unwrap(),
        }
    }

    fn source_network() -> (NetworkDef, Contract) {
        let mut builder = NetworkBuilder::new();
        let start = builder
            .add(
                "source",
                Arc::new(crate::ComponentDef::source(|| std::iter::once(7_i64))),
            )
            .unwrap();
        builder.start(&start).unwrap();
        let plain = crate::ComponentDef::map(|n: i64| n * 2);
        let contract = Contract::new("test/edge-worker", plain.interface().clone()).unwrap();
        let worker = builder
            .add("worker", Arc::new(plain.claim(&contract).unwrap()))
            .unwrap();
        builder
            .expose_output("out", worker.output::<i64>("output").unwrap())
            .unwrap();
        (builder.build().unwrap(), contract)
    }

    #[test]
    fn direct_root_edge_removal_matches_full_compile_and_preserves_indexes() {
        let (mut before, contract) = source_network();
        before.connections.push(edge("source", "worker"));
        let old = before.compile().unwrap();
        assert_eq!(old.connections().len(), 1);
        let handle = old
            .bind("worker", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        let mut session = old.start();
        session.run_until_idle(64);
        assert_eq!(
            session
                .take_output("out")
                .unwrap()
                .unwrap()
                .value()
                .borrow::<i64>(),
            Some(&14)
        );

        let mut after = before.clone();
        after.connections.clear();
        let incremental = old
            .reuse_after_root_connection_pruning(&before, &after)
            .unwrap();
        let canonical = after.compile().unwrap();
        assert!(incremental.connections().is_empty());
        assert_eq!(incremental.connections(), canonical.connections());
        assert_eq!(incremental.source_starts(), canonical.source_starts());
        assert_eq!(incremental.inputs(), canonical.inputs());
        assert_eq!(incremental.outputs(), canonical.outputs());
        assert_eq!(
            incremental.inspect("worker").unwrap().definition_key,
            canonical.inspect("worker").unwrap().definition_key
        );
        assert_eq!(
            handle.resolve(&incremental),
            Err(NetworkError::ForeignScope)
        );
        assert!(!Arc::ptr_eq(&old.generation, &incremental.generation));
    }

    #[test]
    fn new_edges_changed_capacity_and_changed_component_bypass_pruning() {
        let (mut before, contract) = source_network();
        before.connections.push(edge("source", "worker"));
        let old = before.compile().unwrap();
        let mut changed = before.clone();
        changed.connections.push(edge("source", "worker"));
        assert!(
            old.reuse_after_root_connection_pruning(&before, &changed)
                .is_none()
        );

        changed = before.clone();
        changed.connections[0].capacity = NonZeroUsize::new(2).unwrap();
        assert!(
            old.reuse_after_root_connection_pruning(&before, &changed)
                .is_none()
        );

        changed = before.clone();
        changed.connections.clear();
        changed.instances.insert(
            "worker".into(),
            Arc::new(
                crate::ComponentDef::map(|n: i64| n * 3)
                    .claim(&contract)
                    .unwrap(),
            ),
        );
        assert!(
            old.reuse_after_root_connection_pruning(&before, &changed)
                .is_none()
        );

        changed = before.clone();
        changed.connections.clear();
        changed.outputs.clear();
        assert!(
            old.reuse_after_root_connection_pruning(&before, &changed)
                .is_none()
        );
    }

    #[test]
    fn real_composer_disconnects_selected_edge_with_fresh_handle_authority() {
        let (network, contract) = source_network();
        let mut composer = Composer::new(network).unwrap();
        let connection = edge("source", "worker");
        composer
            .register([Registration::new(
                "edge",
                [],
                3,
                GraphOrigin::default(),
                [Footprint::Connection(ConnectionKey::new(
                    connection.from.clone(),
                    connection.to.clone(),
                ))],
                NetworkPatch::new([Edit::Connect(connection.clone())]),
            )])
            .unwrap();
        assert_eq!(composer.current().compiled().connections().len(), 1);
        let bound = composer
            .current()
            .compiled()
            .bind("worker", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        assert_eq!(
            bound.resolve(composer.current().compiled()),
            Ok((1, "output"))
        );
        assert_eq!(
            composer.revoke("edge").unwrap(),
            BTreeSet::from(["edge".into()])
        );
        assert!(composer.current().compiled().connections().is_empty());
        assert_eq!(
            bound.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        assert_eq!(
            composer.current().compiled().connections(),
            composer
                .current()
                .network()
                .compile()
                .unwrap()
                .connections()
        );
        composer
            .register([Registration::new(
                "edge-again",
                [],
                3,
                GraphOrigin::default(),
                [Footprint::Connection(ConnectionKey::new(
                    connection.from.clone(),
                    connection.to.clone(),
                ))],
                NetworkPatch::new([Edit::Connect(connection)]),
            )])
            .unwrap();
        assert_eq!(composer.current().compiled().connections().len(), 1);
    }
}

#[cfg(test)]
mod primitive_replacement_tests {
    use super::*;
    use crate::Value;
    use crate::compose::{Composer, Edit, Footprint, NetworkPatch, Registration};
    use crate::network::NetworkBuilder;

    fn contract() -> Contract {
        let definition = crate::ComponentDef::map(|n: i64| n);
        Contract::new("test/root-primitive", definition.interface().clone()).unwrap()
    }

    fn worker(factor: i64, contract: &Contract) -> Arc<crate::ComponentDef> {
        Arc::new(
            crate::ComponentDef::map(move |n: i64| n * factor)
                .claim(contract)
                .unwrap(),
        )
    }

    fn base(contract: &Contract) -> NetworkDef {
        let mut builder = NetworkBuilder::new();
        let instance = builder.add("worker", worker(1, contract)).unwrap();
        let capacity = NonZeroUsize::new(1).unwrap();
        builder
            .expose_input("in", instance.input::<i64>("input").unwrap(), capacity)
            .unwrap();
        builder
            .expose_output("out", instance.output::<i64>("output").unwrap())
            .unwrap();
        builder.require_contract("worker", contract).unwrap();
        builder.build().unwrap()
    }

    fn result(compiled: &CompiledNetwork, input: i64) -> i64 {
        let mut session = compiled.start();
        session.inject("in", Value::local(input)).unwrap();
        session.run_until_idle(64);
        let output = session.take_output("out").unwrap().unwrap();
        *output.value().borrow::<i64>().unwrap()
    }

    #[test]
    fn isolated_primitive_replacement_matches_complete_compilation_and_refreshes_authority() {
        let contract = contract();
        let before = base(&contract);
        let original = before.compile().unwrap();
        assert_eq!(result(&original, 5), 5);
        let bound = original
            .bind("worker", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();

        let mut after = before.clone();
        after
            .instances
            .insert("worker".into(), worker(7, &contract));
        let incremental = original
            .reuse_after_root_primitive_replacements(&before, &after)
            .unwrap();
        let canonical = after.compile().unwrap();
        assert_eq!(result(&incremental, 5), 35);
        assert_eq!(result(&incremental, 5), result(&canonical, 5));
        assert_eq!(incremental.connections(), canonical.connections());
        assert_eq!(incremental.inputs(), canonical.inputs());
        assert_eq!(incremental.outputs(), canonical.outputs());
        assert_eq!(incremental.source_starts(), canonical.source_starts());
        assert_eq!(
            incremental.inspect("worker").unwrap().definition_key,
            canonical.inspect("worker").unwrap().definition_key
        );
        assert_eq!(
            incremental.inspect("worker").unwrap().claims,
            canonical.inspect("worker").unwrap().claims
        );
        assert_eq!(
            incremental.inspect("worker").unwrap().origin,
            canonical.inspect("worker").unwrap().origin
        );
        incremental
            .check_implementation("worker", &contract)
            .unwrap();
        assert_eq!(bound.resolve(&incremental), Err(NetworkError::ForeignScope));
        assert!(!Arc::ptr_eq(&incremental.generation, &original.generation));
        assert!(!Arc::ptr_eq(
            incremental.processes()[0].factory(),
            original.processes()[0].factory()
        ));
    }

    #[test]
    fn changed_interface_claims_composite_body_or_wiring_forces_full_compiler() {
        let contract = contract();
        let before = base(&contract);
        let old = before.compile().unwrap();

        let mut changed = before.clone();
        changed.instances.insert(
            "worker".into(),
            Arc::new(crate::ComponentDef::map(|n: i64| n.to_string())),
        );
        assert!(
            old.reuse_after_root_primitive_replacements(&before, &changed)
                .is_none()
        );

        changed = before.clone();
        changed.instances.insert(
            "worker".into(),
            Arc::new(crate::ComponentDef::map(|n: i64| n * 2)),
        );
        assert!(
            old.reuse_after_root_primitive_replacements(&before, &changed)
                .is_none()
        );
        assert!(changed.compile().is_err());

        changed = before.clone();
        let mut child = NetworkBuilder::new();
        let nested = child.add("inner", worker(3, &contract)).unwrap();
        child
            .expose_input(
                "input",
                nested.input::<i64>("input").unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        child
            .expose_output("output", nested.output::<i64>("output").unwrap())
            .unwrap();
        changed.instances.insert(
            "worker".into(),
            Arc::new(
                crate::ComponentDef::composite(child.build().unwrap())
                    .claim(&contract)
                    .unwrap(),
            ),
        );
        assert!(
            old.reuse_after_root_primitive_replacements(&before, &changed)
                .is_none()
        );

        changed = before.clone();
        changed.connections.push(crate::network::ConnectionDef {
            from: crate::network::PortPath {
                process: "worker".into(),
                port: "output".into(),
            },
            to: crate::network::PortPath {
                process: "worker".into(),
                port: "input".into(),
            },
            capacity: NonZeroUsize::new(1).unwrap(),
        });
        assert!(
            old.reuse_after_root_primitive_replacements(&before, &changed)
                .is_none()
        );
    }

    #[test]
    fn composer_priority_preemption_and_revoke_change_actual_code_not_just_metadata() {
        let contract = contract();
        let mut composer = Composer::new(base(&contract)).unwrap();
        let old_key = composer
            .current()
            .compiled()
            .inspect("worker")
            .unwrap()
            .definition_key;
        composer
            .register([Registration::new(
                "low",
                [],
                1,
                GraphOrigin::default(),
                [Footprint::Component("worker".into())],
                NetworkPatch::new([Edit::Replace {
                    path: "worker".into(),
                    definition: worker(2, &contract),
                }]),
            )])
            .unwrap();
        assert_eq!(result(composer.current().compiled(), 6), 12);
        let previous = composer
            .current()
            .compiled()
            .bind("worker", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();

        composer
            .register([Registration::new(
                "high",
                [],
                9,
                GraphOrigin::default(),
                [Footprint::Component("worker".into())],
                NetworkPatch::new([Edit::Replace {
                    path: "worker".into(),
                    definition: worker(3, &contract),
                }]),
            )])
            .unwrap();
        assert_eq!(
            composer.current().active(),
            &BTreeSet::from(["high".into()])
        );
        assert_eq!(result(composer.current().compiled(), 6), 18);
        assert_eq!(
            previous.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        let selected_key = composer
            .current()
            .compiled()
            .inspect("worker")
            .unwrap()
            .definition_key;
        assert_ne!(selected_key, old_key);
        assert_eq!(
            selected_key,
            composer
                .current()
                .network()
                .compile()
                .unwrap()
                .inspect("worker")
                .unwrap()
                .definition_key
        );

        composer.revoke("high").unwrap();
        assert_eq!(composer.current().active(), &BTreeSet::from(["low".into()]));
        assert_eq!(result(composer.current().compiled(), 6), 12);
        assert_eq!(
            composer.current().compiled().source_starts(),
            composer
                .current()
                .network()
                .compile()
                .unwrap()
                .source_starts()
        );
    }
}

#[cfg(test)]
mod source_pruning_tests {
    use super::*;
    use crate::compose::{Composer, Edit, Footprint, NetworkPatch, Registration};
    use crate::network::NetworkBuilder;

    fn source() -> Arc<crate::ComponentDef> {
        Arc::new(crate::ComponentDef::source(|| std::iter::once(7_i64)))
    }

    #[test]
    fn pruning_matches_full_compilation_and_fresh_generation_for_two_sources() {
        let mut builder = NetworkBuilder::new();
        let a = builder.add("a", source()).unwrap();
        let b = builder.add("b", source()).unwrap();
        builder.start(&a).unwrap();
        builder.start(&b).unwrap();
        let before = builder.build().unwrap();
        let original = before.compile().unwrap();
        assert_eq!(original.source_starts(), &[0, 1]);

        let mut after = before.clone();
        after.sources.retain(|path| path != "a");
        let reused = original
            .reuse_after_source_pruning(&before, &after)
            .unwrap();
        let fresh = after.compile().unwrap();
        assert_eq!(reused.source_starts(), &[1]);
        assert_eq!(reused.source_starts(), fresh.source_starts());
        assert_eq!(reused.connections(), fresh.connections());
        assert_eq!(reused.inputs(), fresh.inputs());
        assert_eq!(reused.outputs(), fresh.outputs());
        assert_eq!(reused.processes().len(), fresh.processes().len());
        assert_eq!(
            reused
                .processes()
                .iter()
                .map(|p| p.path())
                .collect::<Vec<_>>(),
            fresh
                .processes()
                .iter()
                .map(|p| p.path())
                .collect::<Vec<_>>()
        );
        assert!(!Arc::ptr_eq(&reused.generation, &original.generation));
        assert!(Arc::ptr_eq(
            reused.processes()[1].factory(),
            original.processes()[1].factory()
        ));

        after.sources.clear();
        let reused_again = reused
            .reuse_after_source_pruning(
                &NetworkDef {
                    sources: vec!["b".into()],
                    ..before.clone()
                },
                &after,
            )
            .unwrap();
        assert!(reused_again.source_starts().is_empty());
        assert_eq!(
            reused_again.source_starts(),
            after.compile().unwrap().source_starts()
        );
    }

    #[test]
    fn source_pruning_refuses_growth_changed_definitions_or_contract_obligations() {
        let mut builder = NetworkBuilder::new();
        let a = builder.add("a", source()).unwrap();
        builder.start(&a).unwrap();
        let before = builder.build().unwrap();
        let compiled = before.compile().unwrap();

        let mut after = before.clone();
        after.sources.push("missing".into());
        assert!(
            compiled
                .reuse_after_source_pruning(&before, &after)
                .is_none()
        );

        after = before.clone();
        after.sources.clear();
        after.instances.insert("a".into(), source());
        assert!(
            compiled
                .reuse_after_source_pruning(&before, &after)
                .is_none()
        );

        after = before.clone();
        after.sources.clear();
        after.requirements.push((
            "a".into(),
            Contract::new("test/changed", Arc::new(crate::Interface::default())).unwrap(),
        ));
        assert!(
            compiled
                .reuse_after_source_pruning(&before, &after)
                .is_none()
        );
    }

    #[test]
    fn composer_revoke_source_changes_selected_writer_but_keeps_bound_authority_fresh() {
        let plain = crate::ComponentDef::source(|| std::iter::once(7_i64));
        let contract = Contract::new("test/source", plain.interface().clone()).unwrap();
        let mut builder = NetworkBuilder::new();
        builder
            .add("source", Arc::new(plain.claim(&contract).unwrap()))
            .unwrap();
        let mut composer = Composer::new(builder.build().unwrap()).unwrap();
        assert!(composer.current().compiled().source_starts().is_empty());

        let source_edit = NetworkPatch::new([Edit::AddSource("source".into())]);
        composer
            .register([Registration::new(
                "start",
                [],
                0,
                GraphOrigin::default(),
                [Footprint::Source("source".into())],
                source_edit,
            )])
            .unwrap();
        assert_eq!(composer.current().compiled().source_starts(), &[0]);
        let bound = composer
            .current()
            .compiled()
            .bind("source", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        assert_eq!(
            bound.resolve(composer.current().compiled()),
            Ok((0, "output"))
        );
        assert_eq!(
            composer.revoke("start").unwrap(),
            BTreeSet::from(["start".into()])
        );
        assert!(composer.current().compiled().source_starts().is_empty());
        assert_eq!(
            bound.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        assert_eq!(
            composer.current().compiled().source_starts(),
            composer
                .current()
                .network()
                .compile()
                .unwrap()
                .source_starts()
        );

        // Source add is not an eligible pruning operation. It must still
        // compile and restore the correct automatic start after re-admission.
        composer
            .register([Registration::new(
                "restart",
                [],
                0,
                GraphOrigin::default(),
                [Footprint::Source("source".into())],
                NetworkPatch::new([Edit::AddSource("source".into())]),
            )])
            .unwrap();
        assert_eq!(composer.current().compiled().source_starts(), &[0]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::ComponentDef;
    use crate::interface::{Interface, PortDef};
    use crate::network::NetworkBuilder;

    #[test]
    fn work_limit_reports_exact_attempts_and_overflow_without_changing_the_counter() {
        for (work, maximum, amount, attempted) in [
            (3, 5, 3, Some(6)),
            (u64::MAX - 1, u64::MAX - 1, 1, Some(u64::MAX)),
            (u64::MAX, u64::MAX, 1, None),
        ] {
            let mut compiler = Compiler::new(CompileLimits {
                max_work: maximum,
                ..CompileLimits::default()
            });
            compiler.work = work;
            assert_eq!(
                compiler.charge(amount, "module/leaf"),
                Err(NetworkError::LimitExceeded {
                    limit: CompileLimit {
                        kind: LimitKind::Work,
                        configured: maximum,
                        attempted
                    },
                    path: "module/leaf".into(),
                })
            );
            assert_eq!(compiler.work, work);
        }
        let mut compiler = Compiler::new(CompileLimits {
            max_work: u64::MAX,
            ..CompileLimits::default()
        });
        compiler.work = u64::MAX - 1;
        compiler.charge(1, "").unwrap();
        assert_eq!(compiler.work, u64::MAX);
    }

    #[test]
    fn instance_overflow_is_not_reported_as_an_exact_count() {
        let mut builder = NetworkBuilder::new();
        builder
            .module("empty", NetworkBuilder::new().build().unwrap())
            .unwrap();
        let network = builder.build().unwrap();
        let mut compiler = Compiler::new(CompileLimits {
            max_instances: u64::MAX,
            ..CompileLimits::default()
        });
        compiler.instances = u64::MAX;
        assert!(
            matches!(compiler.expand(&network, "", 0, None), Err(NetworkError::LimitExceeded {
            limit: CompileLimit { kind: LimitKind::Instances, configured: u64::MAX, attempted: None }, path
        }) if path == "empty")
        );
        assert_eq!(compiler.instances, u64::MAX);
    }

    #[test]
    fn declaration_work_is_charged_before_validation() {
        let mut network = NetworkBuilder::new().build().unwrap();
        network.instances.insert(
            "invalid/name".into(),
            Arc::new(ComponentDef::composite(
                NetworkBuilder::new().build().unwrap(),
            )),
        );
        assert!(
            matches!(network.compile_with_limits(CompileLimits { max_work: 0, ..CompileLimits::default() }), Err(NetworkError::LimitExceeded {
            limit: CompileLimit { kind: LimitKind::Work, configured: 0, attempted: Some(1) }, path
        }) if path.is_empty())
        );
        assert_eq!(
            network.compile().unwrap_err(),
            NetworkError::InvalidName("invalid/name".into())
        );
    }

    #[test]
    fn active_ancestor_identity_is_rejected_as_recursive_not_as_a_data_cycle() {
        // Safe public construction cannot form strong Arc definition cycles.
        // Exercise the defensive ancestor check used during nested expansion.
        let network = NetworkBuilder::new().build().unwrap();
        let mut compiler = Compiler::new(CompileLimits::default());
        compiler
            .active
            .insert(std::ptr::from_ref(&network) as usize);
        assert!(
            matches!(compiler.expand(&network, "again", 1, None), Err(NetworkError::RecursiveComposite(path)) if path == "again")
        );
    }

    #[test]
    fn final_validation_fails_closed_on_unprojected_ports_and_invalid_claims() {
        for interface in [
            Interface::new([PortDef::local::<i64>("missing")], []).unwrap(),
            Interface::new([], [PortDef::local::<i64>("missing")]).unwrap(),
        ] {
            let mut network = NetworkBuilder::new().build().unwrap();
            network.interface = Arc::new(interface);
            assert_eq!(
                network.compile().unwrap_err(),
                NetworkError::IncompletePublicBoundary
            );
            let mut outer = NetworkBuilder::new().build().unwrap();
            outer
                .instances
                .insert("nested".into(), Arc::new(ComponentDef::composite(network)));
            assert_eq!(
                outer.compile().unwrap_err(),
                NetworkError::IncompletePublicBoundary
            );
        }

        let mut definition = ComponentDef::composite(NetworkBuilder::new().build().unwrap());
        definition.claims.push(
            Contract::new(
                "native/invalid",
                Arc::new(Interface::new([PortDef::local::<i64>("missing")], []).unwrap()),
            )
            .unwrap(),
        );
        let mut network = NetworkBuilder::new().build().unwrap();
        network
            .instances
            .insert("claimed".into(), Arc::new(definition));
        assert!(matches!(network.compile(), Err(NetworkError::Interface(_))));
    }

    #[test]
    fn final_validation_checks_nested_and_root_claims_even_after_internal_edits() {
        let invalid = Contract::new(
            "native/invalid",
            Arc::new(Interface::new([], [PortDef::local::<i64>("missing")]).unwrap()),
        )
        .unwrap();
        let mut child = NetworkBuilder::new().build().unwrap();
        child.claims.push(invalid.clone());
        let mut outer = NetworkBuilder::new().build().unwrap();
        let mut definition = ComponentDef::composite(child);
        definition.claims.clear();
        outer
            .instances
            .insert("nested".into(), Arc::new(definition));
        assert!(matches!(outer.compile(), Err(NetworkError::Interface(_))));
        let mut root = NetworkBuilder::new().build().unwrap();
        root.claims.push(invalid);
        assert!(matches!(root.compile(), Err(NetworkError::Interface(_))));
    }

    #[test]
    fn inherited_private_requirements_survive_direct_model_replacement() {
        let claim = Contract::new("native/empty", Arc::new(Interface::default())).unwrap();
        let mut child = NetworkBuilder::new();
        child
            .add(
                "private",
                Arc::new(
                    ComponentDef::composite(NetworkBuilder::new().build().unwrap())
                        .claim(&claim)
                        .unwrap(),
                ),
            )
            .unwrap();
        child.require_contract("private", &claim).unwrap();
        let mut root = NetworkBuilder::new();
        root.module("module", child.build().unwrap()).unwrap();
        let mut network = root.build().unwrap();
        network.instances.insert(
            "module".into(),
            Arc::new(ComponentDef::composite(
                NetworkBuilder::new().build().unwrap(),
            )),
        );
        assert!(
            matches!(network.compile(), Err(NetworkError::UnknownProcess(path)) if path == "module/private")
        );
    }
}
