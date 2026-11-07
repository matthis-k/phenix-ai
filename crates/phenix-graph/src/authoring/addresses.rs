//! Symbolic endpoint resolution shared by manual patches and the optional
//! `patch!` macro. An omitted port/slot is accepted only if exactly one
//! concrete endpoint exists; values are never selected by type-guessing.
use super::*;

impl GraphBuilder {
    fn path_owner<'a>(
        &'a self,
        path: &'a str,
        direction: &str,
    ) -> Result<(&'a str, Option<&'a str>, bool), String> {
        let mut matches = Vec::new();
        for node in &self.module.nodes {
            if let Some(name) = node.name.as_deref() {
                if let Some(suffix) = match_endpoint_prefix(path, name, direction) {
                    matches.push((name, suffix, false));
                }
            }
        }
        for instance in &self.module.instances {
            if let Some(suffix) = match_endpoint_prefix(path, &instance.name, direction) {
                matches.push((instance.name.as_str(), suffix, true));
            }
        }
        match matches.as_slice() {
            [] => Err(format!(
                "no accessible {direction} endpoint at {path:?}; module internals are not addressable"
            )),
            [only] => Ok(*only),
            _ => Err(format!(
                "ambiguous endpoint {path:?}; use an explicit .{direction} port"
            )),
        }
    }

    fn source_path(&self, path: &str) -> Result<(Source, TypeIdentity), String> {
        let (name, specified, is_instance) = self.path_owner(path, "out")?;
        if is_instance {
            let entry = self.dependency_for_instance(name)?;
            let outputs = &entry.definition.outputs;
            let output = match specified {
                Some(selected) => outputs
                    .iter()
                    .find(|p| p.name == selected)
                    .ok_or_else(|| format!("unknown module output {path:?}"))?,
                None => exactly_one(outputs, path, "output")?,
            };
            Ok((
                Source::Instance {
                    name: name.into(),
                    port: output.name.clone(),
                },
                output.identity(),
            ))
        } else {
            let node = self
                .module
                .nodes
                .iter()
                .find(|n| n.name.as_deref() == Some(name))
                .ok_or_else(|| format!("unknown source node {name:?}"))?;
            let output = match specified {
                Some(selected) => node
                    .outputs
                    .iter()
                    .find(|p| p.name.as_deref() == Some(selected))
                    .ok_or_else(|| format!("unknown node output {path:?}"))?,
                None => exactly_one(&node.outputs, path, "output")?,
            };
            let named = output.name.as_deref().ok_or_else(|| {
                format!("unnamed output on {name:?} cannot be symbolically addressed")
            })?;
            Ok((
                Source::Node {
                    node: name.into(),
                    output: named.into(),
                },
                output.identity(),
            ))
        }
    }

    fn target_path(&self, path: &str) -> Result<(Target, TypeIdentity), String> {
        let (name, specified, is_instance) = self.path_owner(path, "in")?;
        if is_instance {
            let entry = self.dependency_for_instance(name)?;
            let inputs = &entry.definition.inputs;
            let input = match specified {
                Some(selected) => inputs
                    .iter()
                    .find(|p| p.name == selected)
                    .ok_or_else(|| format!("unknown module input {path:?}"))?,
                None => exactly_one(inputs, path, "input")?,
            };
            Ok((
                Target::Instance {
                    name: name.into(),
                    port: input.name.clone(),
                },
                input.identity(),
            ))
        } else {
            let node = self
                .module
                .nodes
                .iter()
                .find(|n| n.name.as_deref() == Some(name))
                .ok_or_else(|| format!("unknown target node {name:?}"))?;
            // Defaults count *all* physical slots, including unnamed slots
            // that cannot be referenced symbolically. Never infer the single
            // named slot if additional unnamed slots exist.
            if specified.is_none() {
                let slots: usize = node.ports.iter().map(|port| port.slots.len()).sum();
                if slots != 1 {
                    return Err(format!(
                        "ambiguous input {path:?}; select an explicit port and slot"
                    ));
                }
            }
            let mut matched = Vec::new();
            for port in &node.ports {
                let Some(port_name) = port.name.as_deref() else {
                    continue;
                };
                for slot in &port.slots {
                    let Some(slot_name) = slot.name.as_deref() else {
                        continue;
                    };
                    let accepts = match specified {
                        None => true,
                        Some(selector) => {
                            selector == port_name || selector == format!("{port_name}.{slot_name}")
                        }
                    };
                    if accepts {
                        matched.push((port_name, slot_name, slot.identity()));
                    }
                }
            }
            let (port, slot, kind) = match matched.as_slice() {
                [] => return Err(format!("no input slot at {path:?}")),
                [one] => *one,
                _ => {
                    return Err(format!(
                        "ambiguous input {path:?}; specify .in.<port>.<slot>"
                    ));
                }
            };
            Ok((
                Target::Node {
                    node: name.into(),
                    port: port.into(),
                    slot: slot.into(),
                },
                kind,
            ))
        }
    }

    /// Canonical macro-independent symbolic connection API. `a -> b` resolves
    /// to the sole output and input slot; explicit `a.out.value` and
    /// `b.in.run.left` select the same concrete endpoint objects.
    pub(super) fn connect_symbolic(&mut self, from: &str, to: &str) -> Result<&mut Self, String> {
        let (source, output_ty) = self.source_path(from)?;
        let (target, input_ty) = self.target_path(to)?;
        if output_ty != input_ty {
            return Err(format!(
                "connection type mismatch: {from:?} ({output_ty:?}) -> {to:?} ({input_ty:?})"
            ));
        }
        let src = self.verify_source(&source, output_ty)?;
        let dst = self.verify_target(&target, input_ty)?;
        if self
            .module
            .edges
            .iter()
            .any(|edge| edge.from == src && edge.to == dst)
        {
            return Err(format!("duplicate connection {from:?} -> {to:?}"));
        }
        self.module.connect(src, dst);
        Ok(self)
    }

    pub(super) fn disconnect_symbolic(
        &mut self,
        from: &str,
        to: &str,
    ) -> Result<&mut Self, String> {
        let (source, output_ty) = self.source_path(from)?;
        let (target, input_ty) = self.target_path(to)?;
        if output_ty != input_ty {
            return Err("connection type mismatch".into());
        }
        let src = self.verify_source(&source, output_ty)?;
        let dst = self.verify_target(&target, input_ty)?;
        let before = self.module.edges.len();
        self.module
            .edges
            .retain(|edge| edge.from != src || edge.to != dst);
        if before == self.module.edges.len() {
            return Err(format!("connection {from:?} -> {to:?} not found"));
        }
        Ok(self)
    }

    pub(super) fn expose_input_symbolic(
        &mut self,
        name: &str,
        to: &str,
    ) -> Result<PublicInput, String> {
        let (target, kind) = self.target_path(to)?;
        let resolved = self.verify_target(&target, kind)?;
        validate_name_segment(name).map_err(|err| err.to_string())?;
        if self.module.inputs.iter().any(|input| input.name == name) {
            return Err(format!("duplicate public input {name:?}"));
        }
        let id =
            u16::try_from(self.module.inputs.len()).map_err(|_| "public input limit exceeded")?;
        let (schema, local_type) = self.endpoint_identity(kind, &target)?;
        self.module
            .input_with_identity(id, name, schema, local_type, vec![resolved]);
        Ok(PublicInput {
            scope: self.module.scope,
            id,
            name: name.into(),
            schema,
            local_type,
        })
    }

    pub(super) fn expose_output_symbolic(&mut self, name: &str, from: &str) -> Result<(), String> {
        let (source, kind) = self.source_path(from)?;
        let resolved = self.verify_source(&source, kind)?;
        validate_name_segment(name).map_err(|err| err.to_string())?;
        if self.module.outputs.iter().any(|output| output.name == name) {
            return Err(format!("duplicate public output {name:?}"));
        }
        let id =
            u16::try_from(self.module.outputs.len()).map_err(|_| "public output limit exceeded")?;
        let (schema, local_type) = self.source_identity(kind, &source)?;
        self.module
            .output_with_identity(id, name, schema, local_type, resolved);
        Ok(())
    }

    // Preserve the exact declared local/wire representation, including when
    // an author registers a low-level NodeTemplate instead of a typed handle.
    fn endpoint_identity(
        &self,
        _kind: TypeIdentity,
        target: &Target,
    ) -> Result<(&'static str, Option<TypeId>), String> {
        match target {
            Target::Node { node, port, slot } => {
                let n = self
                    .module
                    .nodes
                    .iter()
                    .find(|n| n.name.as_deref() == Some(node))
                    .ok_or("node missing")?;
                let p = n
                    .ports
                    .iter()
                    .find(|p| p.name.as_deref() == Some(port))
                    .ok_or("port missing")?;
                let s = p
                    .slots
                    .iter()
                    .find(|s| s.name.as_deref() == Some(slot))
                    .ok_or("slot missing")?;
                Ok((s.schema, s.local_type))
            }
            Target::Instance { name, port } => {
                let entry = self.dependency_for_instance(name)?;
                let p = entry
                    .definition
                    .inputs
                    .iter()
                    .find(|p| &p.name == port)
                    .ok_or("input missing")?;
                Ok((p.schema, p.local_type))
            }
        }
    }
    fn source_identity(
        &self,
        _kind: TypeIdentity,
        source: &Source,
    ) -> Result<(&'static str, Option<TypeId>), String> {
        match source {
            Source::Node { node, output } => {
                let n = self
                    .module
                    .nodes
                    .iter()
                    .find(|n| n.name.as_deref() == Some(node))
                    .ok_or("node missing")?;
                let p = n
                    .outputs
                    .iter()
                    .find(|p| p.name.as_deref() == Some(output))
                    .ok_or("output missing")?;
                Ok((p.schema, p.local_type))
            }
            Source::Instance { name, port } => {
                let entry = self.dependency_for_instance(name)?;
                let p = entry
                    .definition
                    .outputs
                    .iter()
                    .find(|p| &p.name == port)
                    .ok_or("output missing")?;
                Ok((p.schema, p.local_type))
            }
        }
    }

    pub fn public_input(&self, name: &str) -> Result<PublicInput, String> {
        let p = self
            .module
            .inputs
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| format!("unknown public input {name:?}"))?;
        Ok(PublicInput {
            scope: self.module.scope,
            id: p.id,
            name: p.name.clone(),
            schema: p.schema,
            local_type: p.local_type,
        })
    }
}

fn match_endpoint_prefix<'a>(
    path: &'a str,
    node: &str,
    direction: &str,
) -> Option<Option<&'a str>> {
    if path == node {
        return Some(None);
    }
    let rest = path.strip_prefix(node)?;
    if rest == format!(".{direction}") {
        return Some(None);
    }
    let suffix = rest.strip_prefix(&format!(".{direction}."))?;
    (!suffix.is_empty()).then_some(Some(suffix))
}

fn exactly_one<'a, T>(items: &'a [T], path: &str, kind: &str) -> Result<&'a T, String> {
    match items {
        [] => Err(format!("no {kind} at {path:?}")),
        [one] => Ok(one),
        _ => Err(format!(
            "ambiguous {kind} at {path:?}; select an explicit named port or slot"
        )),
    }
}
