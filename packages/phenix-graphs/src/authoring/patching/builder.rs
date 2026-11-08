//! A first-class patch declaration. Both handwritten Rust and `patch!` lower
//! into this builder. It only stores edits; execution still uses GraphBuilder.
use super::*;

/// A patch is a sequence of pre-compilation edits plus its conflict footprint.
/// Nothing here is executable against a compiled graph or an Engine.
pub struct PatchBuilder {
    spec: PatchSpec,
    edits: Vec<Edit>,
    splice: Option<PlannedSplice>,
}
impl PatchBuilder {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            spec: PatchSpec::new(id),
            edits: Vec::new(),
            splice: None,
        }
    }
    /// Advanced, low-level edit within the SAME patch type as declarative
    /// nodes and edges. The declared footprint is checked on application.
    /// This is not a second planner or an imperative public assembly API.
    pub fn custom<F>(spec: PatchSpec, edit: F) -> Self
    where
        F: FnOnce(&mut GraphBuilder) -> Result<(), String> + 'static,
    {
        Self {
            spec,
            edits: vec![Box::new(edit)],
            splice: None,
        }
    }
    /// A splice is an ordinary contributed node plus metadata describing how
    /// the planner should replace the selected existing edge. It uses the same
    /// patch entry point, dependency planner, and compile operation.
    pub fn splice<T, F>(
        mut spec: PatchSpec,
        anchor: PatchAnchor,
        position: SplicePosition,
        splice_order: Option<i32>,
        node: impl Into<String>,
        handler: F,
    ) -> Self
    where
        T: Clone + Send + Sync + 'static,
        F: Fn(T) -> T + Send + Sync + 'static,
    {
        let node = node.into();
        spec.writes.insert(PatchTarget::Node(node.clone()));
        spec.writes.insert(PatchTarget::Splice(anchor.clone()));
        let contributor = spec.id.clone();
        let inserted_name = node.clone();
        let mut patch = Self::custom(spec, move |g| {
            g.node(inserted_name, handler)
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
        patch.splice = Some(PlannedSplice {
            contributor,
            anchor,
            position,
            splice_order,
            node,
        });
        patch
    }
    pub fn after(&mut self, id: impl Into<String>) -> &mut Self {
        self.spec.after.insert(id.into());
        self
    }
    pub fn priority(&mut self, priority: i32) -> &mut Self {
        self.spec.priority = Some(priority);
        self
    }
    pub fn node<A, O, D, Mode>(&mut self, name: impl Into<String>, definition: D) -> &mut Self
    where
        A: Send + Sync + 'static,
        O: Send + Sync + 'static,
        D: NodeDefinition<A, O, Mode> + 'static,
    {
        let name = name.into();
        self.spec.writes.insert(PatchTarget::Node(name.clone()));
        self.edits.push(Box::new(move |graph| {
            graph
                .node(name, definition)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }));
        self
    }
    /// Contribute an already-defined node action (including a selected plugin
    /// operation). This is the same patch path as `node`, so plugin dispatch
    /// requires no graph-special node kind or parallel execution engine.
    pub fn add(&mut self, name: impl Into<String>, template: NodeTemplate) -> &mut Self {
        let name = name.into();
        self.spec.writes.insert(PatchTarget::Node(name.clone()));
        self.edits.push(Box::new(move |g| {
            g.add(name, template).map(|_| ()).map_err(|e| e.to_string())
        }));
        self
    }
    /// Replace a node or module instance through the same interface-checked
    /// operation as direct graph construction, with the correct write claim.
    pub fn replace<R: Replacement + 'static>(
        &mut self,
        name: impl Into<String>,
        replacement: R,
    ) -> &mut Self {
        let name = name.into();
        self.spec.writes.insert(match R::kind() {
            ReplacementKind::Node => PatchTarget::Node(name.clone()),
            ReplacementKind::Module => PatchTarget::Instance(name.clone()),
        });
        self.edits
            .push(Box::new(move |g| g.replace(&name, replacement)));
        self
    }
    pub fn instance(&mut self, name: impl Into<String>, module: ModulePackage) -> &mut Self {
        let name = name.into();
        self.spec.writes.insert(PatchTarget::Instance(name.clone()));
        self.edits
            .push(Box::new(move |g| g.instance(name, &module).map(|_| ())));
        self
    }
    pub fn connect(&mut self, from: &str, to: &str) -> Result<&mut Self, PatchError> {
        self.spec
            .writes
            .insert(PatchTarget::connection_pattern(from, to)?);
        let from = from.to_owned();
        let to = to.to_owned();
        self.edits.push(Box::new(move |g| {
            g.connect(from.as_str(), to.as_str()).map(|_| ())
        }));
        Ok(self)
    }
    pub fn disconnect(&mut self, from: &str, to: &str) -> Result<&mut Self, PatchError> {
        self.spec
            .writes
            .insert(PatchTarget::connection_pattern(from, to)?);
        let from = from.to_owned();
        let to = to.to_owned();
        self.edits.push(Box::new(move |g| {
            g.disconnect(from.as_str(), to.as_str()).map(|_| ())
        }));
        Ok(self)
    }
    pub fn expose_input(&mut self, name: impl Into<String>, path: impl Into<String>) -> &mut Self {
        let name = name.into();
        let path = path.into();
        self.spec
            .writes
            .insert(PatchTarget::PublicInput(name.clone()));
        self.edits.push(Box::new(move |g| {
            g.expose_input(&name, path.as_str()).map(|_| ())
        }));
        self
    }
    pub fn expose_output(&mut self, name: impl Into<String>, path: impl Into<String>) -> &mut Self {
        let name = name.into();
        let path = path.into();
        self.spec
            .writes
            .insert(PatchTarget::PublicOutput(name.clone()));
        self.edits
            .push(Box::new(move |g| g.expose_output(&name, path.as_str())));
        self
    }
    pub fn trigger(&mut self, name: impl Into<String>, inputs: &[&str]) -> &mut Self {
        let name = name.into();
        let names: Vec<String> = inputs.iter().map(|s| (*s).to_owned()).collect();
        self.spec.writes.insert(PatchTarget::Trigger(name.clone()));
        self.edits.push(Box::new(move |g| {
            let refs: Vec<_> = names.iter().map(String::as_str).collect();
            g.trigger(&name, refs).map(|_| ())
        }));
        self
    }
    pub fn write_claim(&mut self, claim: PatchTarget) -> &mut Self {
        self.spec.writes.insert(claim);
        self
    }
    pub fn spec(&self) -> &PatchSpec {
        &self.spec
    }
}
impl PatchPlanner {
    /// Register a first-class patch. Dependencies and edit footprint are the
    /// same whether it came from Rust calls or from the `patch!` proc macro.
    pub fn add_patch(&mut self, patch: PatchBuilder) -> Result<(), PatchError> {
        let PatchBuilder {
            spec,
            edits,
            splice,
        } = patch;
        if let Some(insert) = &splice {
            validate_name_segment(&insert.node)
                .map_err(|_| PatchError::InvalidId(insert.node.clone()))?;
        }
        let id = spec.id.clone();
        self.register(spec, move |graph| {
            for edit in edits {
                edit(graph)?;
            }
            Ok(())
        })?;
        self.entries.get_mut(&id).expect("registered patch").splice = splice;
        Ok(())
    }
}
