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
        if after.sources.len() >= before.sources.len()
            || before.interface != after.interface
            || before.connections != after.connections
            || before.inputs != after.inputs
            || before.outputs != after.outputs
            || before.claims != after.claims
            || before.origin != after.origin
            || before.requirements != after.requirements
            || before.replacement_points != after.replacement_points
            || before.extensions_allowed != after.extensions_allowed
            || before.instances.len() != after.instances.len()
        {
            return None;
        }
        // Replay of the same immutable declaration may produce a new Arc
        // wrapper with the same DefinitionKey (for example, host origin
        // stamping). Check every author-visible claim and origin as well.
        // Composite copy-on-write revisions always receive a fresh key.
        if !before.instances.iter().all(|(name, old)| {
            after.instances.get(name).is_some_and(|new| {
                old.definition_key() == new.definition_key()
                    && old.interface() == new.interface()
                    && old.claims() == new.claims()
                    && old.origin() == new.origin()
            })
        }) {
            return None;
        }
        let remaining: BTreeSet<&str> = after.sources.iter().map(String::as_str).collect();
        if !after.sources.iter().all(|path| before.sources.contains(path)) {
            return None;
        }
        let mut removed_indices = BTreeSet::new();
        for path in before.sources.iter().filter(|path| !remaining.contains(path.as_str())) {
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
        compiled.sources.retain(|index| !removed_indices.contains(index));
        Some(compiled)
    }
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
        let reused = original.reuse_after_source_pruning(&before, &after).unwrap();
        let fresh = after.compile().unwrap();
        assert_eq!(reused.source_starts(), &[1]);
        assert_eq!(reused.source_starts(), fresh.source_starts());
        assert_eq!(reused.connections(), fresh.connections());
        assert_eq!(reused.inputs(), fresh.inputs());
        assert_eq!(reused.outputs(), fresh.outputs());
        assert_eq!(reused.processes().len(), fresh.processes().len());
        assert_eq!(
            reused.processes().iter().map(|p| p.path()).collect::<Vec<_>>(),
            fresh.processes().iter().map(|p| p.path()).collect::<Vec<_>>()
        );
        assert!(!Arc::ptr_eq(&reused.generation, &original.generation));
        assert!(Arc::ptr_eq(
            reused.processes()[1].factory(),
            original.processes()[1].factory()
        ));

        after.sources.clear();
        let reused_again = reused.reuse_after_source_pruning(
            &NetworkDef {
                sources: vec!["b".into()],
                ..before.clone()
            },
            &after,
        ).unwrap();
        assert!(reused_again.source_starts().is_empty());
        assert_eq!(reused_again.source_starts(), after.compile().unwrap().source_starts());
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
        assert!(compiled.reuse_after_source_pruning(&before, &after).is_none());

        after = before.clone();
        after.sources.clear();
        after.instances.insert("a".into(), source());
        assert!(compiled.reuse_after_source_pruning(&before, &after).is_none());

        after = before.clone();
        after.sources.clear();
        after.requirements.push((
            "a".into(),
            Contract::new("test/changed", Arc::new(crate::Interface::default())).unwrap()
        ));
        assert!(compiled.reuse_after_source_pruning(&before, &after).is_none());
    }

    #[test]
    fn composer_revoke_source_changes_selected_writer_but_keeps_bound_authority_fresh() {
        let plain = crate::ComponentDef::source(|| std::iter::once(7_i64));
        let contract = Contract::new("test/source", plain.interface().clone()).unwrap();
        let mut builder = NetworkBuilder::new();
        builder.add("source", Arc::new(plain.claim(&contract).unwrap())).unwrap();
        let mut composer = Composer::new(builder.build().unwrap()).unwrap();
        assert!(composer.current().compiled().source_starts().is_empty());

        let source_edit = NetworkPatch::new([Edit::AddSource("source".into())]);
        composer.register([Registration::new(
            "start",
            [],
            0,
            GraphOrigin::default(),
            [Footprint::Source("source".into())],
            source_edit,
        )]).unwrap();
        assert_eq!(composer.current().compiled().source_starts(), &[0]);
        let bound = composer.current().compiled().bind("source", &contract).unwrap()
            .output::<i64>("output").unwrap();
        assert_eq!(bound.resolve(composer.current().compiled()), Ok((0, "output")));
        assert_eq!(composer.revoke("start").unwrap(), BTreeSet::from(["start".into()]));
        assert!(composer.current().compiled().source_starts().is_empty());
        assert_eq!(
            bound.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        assert_eq!(
            composer.current().compiled().source_starts(),
            composer.current().network().compile().unwrap().source_starts()
        );

        // Source add is not an eligible pruning operation. It must still
        // compile and restore the correct automatic start after re-admission.
        composer.register([Registration::new(
            "restart",
            [],
            0,
            GraphOrigin::default(),
            [Footprint::Source("source".into())],
            NetworkPatch::new([Edit::AddSource("source".into())]),
        )]).unwrap();
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
