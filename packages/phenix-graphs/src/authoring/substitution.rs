//! Cross-kind edits lower to ordinary declarations and compiler projections.
use super::*;

fn validate_boundary(
    node: &NodeTemplate,
    module: &ModuleDef,
    old_claims: &[GraphContract],
    new_claims: &[GraphContract],
) -> Result<(), String> {
    let node_shape =
        InterfaceShape::node_ports(&node.ports, &node.output).map_err(|e| e.to_string())?;
    let module_shape =
        InterfaceShape::module_ports(&module.inputs, &module.output).map_err(|e| e.to_string())?;
    if !node_shape.same_shape(&module_shape) {
        return Err("replacement changes complete public operation shape".into());
    }
    for claim in old_claims {
        let provided = new_claims
            .iter()
            .find(|c| c.id() == claim.id())
            .ok_or_else(|| format!("replacement removes contract claim {}", claim.id()))?;
        claim
            .check_definition(provided)
            .map_err(|e| e.to_string())?;
    }
    for claim in new_claims {
        claim
            .required()
            .check_shape(&node_shape)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

impl GraphBuilder {
    pub(super) fn substitute_node_with_module(
        &mut self,
        name: &str,
        package: &ModulePackage,
    ) -> Result<(), String> {
        let old = self
            .pending
            .borrow()
            .get(name)
            .cloned()
            .unwrap_or_else(|| self.module.named_node(name).expect("existing node").clone());
        validate_boundary(
            &old,
            &package.root.definition,
            &old.claims,
            &package.root.definition.claims,
        )?;
        let mut candidate = self.clone();
        let index = candidate
            .module
            .node_names
            .remove(name)
            .expect("existing node");
        candidate.module.nodes.remove(index);
        candidate.module.node_ids.remove(&old.id.0);
        candidate.module.node_names = candidate
            .module
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| n.name.clone().map(|name| (name, i)))
            .collect();
        candidate.pending.borrow_mut().remove(name);
        candidate.instance(name, package)?;
        candidate.retarget_substitution(name, &old, &package.root.definition, true)?;
        self.commit_substitution(name, candidate)
    }

    pub(super) fn substitute_module_with_node(
        &mut self,
        name: &str,
        mut node: NodeTemplate,
    ) -> Result<(), String> {
        let old = self.dependency_for_instance(name)?.definition.clone();
        validate_boundary(&node, &old, &old.claims, &node.claims)?;
        let mut candidate = self.clone();
        candidate.module.instances.retain(|i| i.name != name);
        candidate.module.declaration_ids.remove(name);
        // A package's root requirements become requirements on the replacement
        // boundary. Requirements on removed children fail final validation.
        for (path, contract) in &old.contract_requests {
            let path = if path.is_empty() {
                name.into()
            } else {
                format!("{name}/{path}")
            };
            candidate
                .module
                .contract_requests
                .push((path, contract.clone()));
        }
        node.id = NodeId(0);
        node.declaring_scope = Some(candidate.module.scope);
        candidate.add(name, node).map_err(|e| e.to_string())?;
        let node = candidate
            .module
            .named_node(name)
            .expect("registered node")
            .clone();
        candidate.retarget_substitution(name, &node, &old, false)?;
        self.commit_substitution(name, candidate)
    }

    fn retarget_substitution(
        &mut self,
        name: &str,
        node: &NodeTemplate,
        module: &ModuleDef,
        to_module: bool,
    ) -> Result<(), String> {
        let mut targets = Vec::new();
        for input in &module.inputs {
            let port = node
                .ports
                .iter()
                .find(|p| p.name.as_deref() == Some(&input.port))
                .ok_or("replacement requires explicitly named input ports")?;
            let slot = port
                .slots
                .iter()
                .find(|s| s.name.as_deref() == Some(&input.slot))
                .ok_or("replacement requires explicitly named input slots")?;
            let named_node = ModuleInputTarget::NodeNamed {
                node: name.into(),
                port: input.port.clone(),
                slot: input.slot.clone(),
            };
            let named_module = ModuleInputTarget::InstanceNamed {
                name: name.into(),
                port: input.name.clone(),
            };
            if to_module {
                targets.push((named_node, named_module.clone()));
                targets.push((
                    ModuleInputTarget::Node(InputAddr {
                        node: node.id,
                        port: port.id,
                        slot: slot.id,
                    }),
                    named_module,
                ));
            } else {
                targets.push((named_module, named_node.clone()));
                targets.push((
                    ModuleInputTarget::Instance {
                        name: name.into(),
                        port: input.id,
                    },
                    named_node,
                ));
            }
        }
        let output = module
            .output
            .as_ref()
            .ok_or("replacement requires a public output")?;
        let named_node = ModuleOutputSource::NodeNamed {
            node: name.into(),
            output: node
                .output
                .name
                .clone()
                .ok_or("replacement requires an explicitly named output")?,
        };
        let named_module = ModuleOutputSource::InstanceNamed {
            name: name.into(),
            port: output.name.clone(),
        };
        let sources = if to_module {
            vec![
                (named_node, named_module.clone()),
                (
                    ModuleOutputSource::Node(OutputAddr {
                        node: node.id,
                        slot: node.output.id,
                    }),
                    named_module,
                ),
            ]
        } else {
            vec![
                (named_module, named_node.clone()),
                (
                    ModuleOutputSource::Instance {
                        name: name.into(),
                        port: output.id,
                    },
                    named_node,
                ),
            ]
        };
        for edge in &mut self.module.edges {
            if let Some((_, to)) = targets.iter().find(|(from, _)| from == &edge.to) {
                edge.to = to.clone();
            }
            if let Some((_, to)) = sources.iter().find(|(from, _)| from == &edge.from) {
                edge.from = to.clone();
            }
        }
        for input in &mut self.module.inputs {
            for target in &mut input.targets {
                if let Some((_, to)) = targets.iter().find(|(from, _)| from == target) {
                    *target = to.clone();
                }
            }
        }
        if let Some(output) = &mut self.module.output
            && let Some((_, to)) = sources.iter().find(|(from, _)| from == &output.source)
        {
            output.source = to.clone();
        }
        Ok(())
    }

    fn commit_substitution(&mut self, name: &str, mut candidate: Self) -> Result<(), String> {
        let mut validation = candidate.clone();
        // Final-unit requests may precede unrelated declarations. Check only
        // this boundary and its implementation now; commit every obligation.
        validation.module.contract_requests.retain(|(path, _)| {
            path == name
                || path
                    .strip_prefix(name)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        });
        let mut affected = BTreeSet::new();
        let mut pending = validation
            .module
            .instances
            .iter()
            .filter(|instance| instance.name == name)
            .map(|instance| instance.definition.clone())
            .collect::<Vec<_>>();
        while let Some(id) = pending.pop() {
            if affected.insert(id.clone()) {
                pending.extend(
                    validation.dependencies[&id]
                        .definition
                        .instances
                        .iter()
                        .map(|instance| instance.definition.clone()),
                );
            }
        }
        for (id, entry) in &mut validation.dependencies {
            if !affected.contains(id) {
                entry.definition.contract_requests.clear();
            }
        }
        // Claims and all current topology still go through the real compiler.
        // Unrelated malformed edges or declarations are not deferred here.
        validation.compile().map_err(|e| e.to_string())?;
        // Keep unrelated NodeHandles attached to this builder's pending store.
        *self.pending.borrow_mut() = candidate.pending.borrow().clone();
        candidate.pending = Rc::clone(&self.pending);
        candidate.definition_id = self.definition_id;
        *self = candidate;
        Ok(())
    }
}
