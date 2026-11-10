use super::*;

/// Append-only authoring transaction. Existing declarations and configuration
/// ownership stay in place; errors and unwinding discard only these additions.
/// Edits made through separately held configuration handles are not rolled back.
pub struct GraphAdditions<'a> {
    graph: &'a mut GraphBuilder,
    nodes: usize,
    edges: usize,
    inputs: usize,
    triggers: usize,
    next_connection: u32,
    next_node_id: u32,
    committed: bool,
}

impl GraphBuilder {
    /// Add a related set of nodes, connections and refresh routes atomically.
    /// The restricted view cannot replace or remove existing declarations.
    pub fn additions<T, E>(
        &mut self,
        edit: impl FnOnce(&mut GraphAdditions<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        let mut transaction = GraphAdditions {
            nodes: self.module.nodes.len(),
            edges: self.module.edges.len(),
            inputs: self.module.inputs.len(),
            triggers: self.module.triggers.len(),
            next_connection: self.module.next_connection,
            next_node_id: self.module.next_node_id,
            graph: self,
            committed: false,
        };
        let result = edit(&mut transaction);
        transaction.committed = result.is_ok();
        result
    }
}

impl GraphAdditions<'_> {
    pub fn discover(&self, name: &str) -> Result<DiscoveredOperation, String> {
        self.graph.discover(name)
    }
    pub fn additions<T, E>(
        &mut self,
        edit: impl FnOnce(&mut GraphAdditions<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        self.graph.additions(edit)
    }

    pub fn node<A, O, D, Mode>(
        &mut self,
        name: impl Into<String>,
        definition: D,
    ) -> Result<NodeHandle<A, O, D::Layout, D, Mode>, GraphBuildError>
    where
        A: Send + Sync + 'static,
        O: Send + Sync + 'static,
        D: NodeDefinition<A, O, Mode> + 'static,
    {
        self.graph.node(name, definition)
    }

    pub fn add(
        &mut self,
        name: impl Into<String>,
        template: NodeTemplate,
    ) -> Result<NamedNodeHandle, ModuleNameError> {
        self.graph.add(name, template)
    }

    pub fn connect<A, B>(&mut self, from: A, to: B) -> Result<(), String>
    where
        (A, B): ConnectEndpoints,
    {
        self.graph.connect(from, to).map(|_| ())
    }

    pub fn expose_input<T: Send + Sync + 'static, Mode>(
        &mut self,
        name: impl Into<String>,
        to: Input<T, Mode>,
    ) -> Result<PublicInput, String> {
        self.graph.expose_input(name, to)
    }

    pub fn trigger<T: TriggerEndpoint>(
        &mut self,
        name: impl Into<String>,
        inputs: impl IntoIterator<Item = T>,
    ) -> Result<TriggerId, String> {
        self.graph.trigger(name, inputs)
    }
}

impl Drop for GraphAdditions<'_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let graph = &mut self.graph;
        for node in graph.module.nodes.drain(self.nodes..) {
            graph.module.node_ids.remove(&node.id.0);
            if let Some(name) = node.name {
                graph.module.node_names.remove(&name);
                graph.pending.borrow_mut().remove(&name);
            }
        }
        graph.module.next_node_id = self.next_node_id;
        graph.module.edges.truncate(self.edges);
        graph.module.next_connection = self.next_connection;
        for input in graph.module.inputs.drain(self.inputs..) {
            graph.public_input_ids.remove(&input.name);
        }
        for trigger in graph.module.triggers.drain(self.triggers..) {
            graph.trigger_names.remove(&trigger.name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_indexes_preserve_ids_failures_clones_and_replacements() {
        let mut graph = GraphBuilder::new("ids");
        let mut explicit = NodeTemplate::function(|v: i64| v);
        explicit.id = NodeId(2);
        graph.add("explicit", explicit.clone()).unwrap();
        assert!(graph.add("duplicate_id", explicit).is_err());
        graph
            .add("first", NodeTemplate::function(|v: i64| v))
            .unwrap();
        assert!(
            graph
                .add("first", NodeTemplate::function(|v: i64| v))
                .is_err()
        );
        let mut fork = graph.clone();
        graph
            .replace("first", NodeTemplate::function(|v: i64| v + 1))
            .unwrap();
        graph
            .add("next", NodeTemplate::function(|v: i64| v))
            .unwrap();
        fork.add("next", NodeTemplate::function(|v: i64| v))
            .unwrap();
        for builder in [&graph, &fork] {
            let ids: Vec<_> = builder.module.nodes.iter().map(|n| n.id.0).collect();
            assert_eq!(ids, [2, 1, 3]);
            assert_eq!(builder.module.named_node("next").unwrap().id, NodeId(3));
            assert!(builder.module.named_node("duplicate_id").is_none());
        }
        graph.compile().unwrap();
        fork.compile().unwrap();
    }

    #[test]
    fn registration_id_exhaustion_does_not_wrap_or_register_a_name() {
        let mut module = ModuleDefBuilder::new("exhaustion");
        for id in 1..=u16::MAX {
            let mut node = NodeTemplate::function(|v: i64| v);
            node.id = NodeId(id);
            module.add(node).unwrap();
        }
        assert!(matches!(
            module.register("overflow", NodeTemplate::function(|v: i64| v)),
            Err(ModuleNameError::ExhaustedNodeIds)
        ));
        assert!(module.named_node("overflow").is_none());
        assert_eq!(module.clone().next_node_id, u16::MAX as u32 + 1);
    }

    #[test]
    fn additions_run_once_and_nested_rollback_preserves_handles_and_bindings() {
        let mut graph = GraphBuilder::new("transactions");
        let user = graph.node("user", |v: (i64, i64)| v.0 + v.1).unwrap();
        let mut calls = 0;
        let added = graph
            .additions(|g| -> Result<_, String> {
                calls += 1;
                let node = g.node("added", |v: i64| v).map_err(|e| e.to_string())?;
                let input = g.expose_input("added", node.inlet())?;
                g.trigger("added", [input])?;
                let failed: Result<(), String> = g.additions(|g| {
                    let child = g.node("child", |v: i64| v).map_err(|e| e.to_string())?;
                    g.connect(node.output(), child.inlet())?;
                    let input = g.expose_input("child", child.inlet())?;
                    g.trigger("child", [input])?;
                    Err("abort child".into())
                });
                assert!(failed.is_err());
                let child = g.node("child", |v: i64| v).map_err(|e| e.to_string())?;
                g.connect(node.output(), child.inlet())?;
                Ok(node)
            })
            .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(graph.module.nodes.len(), 3);
        assert_eq!(graph.module.edges.len(), 1);
        assert_eq!(graph.module.next_connection, 1);
        assert_eq!(graph.module.inputs.len(), 1);
        assert_eq!(graph.module.triggers.len(), 1);
        assert_eq!(graph.public_input_ids.len(), 1);
        assert_eq!(graph.trigger_names.len(), 1);
        user.input(Layout::fields()).unwrap();
        added.input(Layout::atomic()).unwrap();
        graph.compile().unwrap();
    }

    #[test]
    fn outer_error_and_panic_discard_successful_nested_additions() {
        let mut graph = GraphBuilder::new("rollback");
        for panic in [false, true] {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                graph.additions(|g| -> Result<(), String> {
                    g.additions(|g| -> Result<(), String> {
                        let a = g.node("a", |v: i64| v).map_err(|e| e.to_string())?;
                        let b = g.node("b", |v: i64| v).map_err(|e| e.to_string())?;
                        g.connect(a.output(), b.inlet())?;
                        let input = g.expose_input("a", a.inlet())?;
                        g.trigger("a", [input])?;
                        Ok(())
                    })?;
                    assert!(!panic, "abort transaction");
                    Err("abort transaction".into())
                })
            }));
            assert!(matches!(result, Ok(Err(_)) | Err(_)));
            assert!(graph.module.nodes.is_empty());
            assert!(graph.module.node_names.is_empty());
            assert!(graph.module.node_ids.is_empty());
            assert_eq!(graph.module.next_node_id, 1);
            assert!(graph.module.edges.is_empty());
            assert_eq!(graph.module.next_connection, 0);
            assert!(graph.module.inputs.is_empty());
            assert!(graph.module.triggers.is_empty());
            assert!(graph.pending.borrow().is_empty());
            assert!(graph.public_input_ids.is_empty());
            assert!(graph.trigger_names.is_empty());
        }
        graph.node("a", |v: i64| v).unwrap();
        graph.compile().unwrap();
    }

    #[test]
    fn rollback_revokes_escaped_handles_even_when_names_and_addresses_are_reused() {
        for panic in [false, true] {
            let mut graph = GraphBuilder::new("escaped");
            let existing = graph.node("existing", |v: i64| v).unwrap();
            let mut escaped = None;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                graph.additions(|g| -> Result<(), String> {
                    let node = g.node("temporary", |v: i64| v).unwrap();
                    let public = g.expose_input("temporary", node.inlet())?;
                    g.trigger("temporary", [public.clone()])?;
                    g.connect(existing.output(), node.inlet())?;
                    escaped = Some((node, public));
                    assert!(!panic, "rollback escaped handles");
                    Err("rollback".into())
                })
            }));
            assert_eq!(result.is_err(), panic);
            if let Ok(result) = result {
                assert!(result.is_err());
            }
            let (stale, public) = escaped.unwrap();
            let replacement = graph.node("temporary", |v: i64| v).unwrap();
            let current = graph
                .expose_input("temporary", replacement.inlet())
                .unwrap();
            assert_eq!(public.id, current.id);
            assert_ne!(public.declaration_id, current.declaration_id);
            assert_ne!(stale.declaration_id, replacement.declaration_id);
            assert!(graph.connect(stale.output(), existing.inlet()).is_err());
            assert!(graph.connect(existing.output(), stale.inlet()).is_err());
            assert!(graph.trigger("stale", [public]).is_err());
            assert!(stale.input(Layout::atomic()).is_err());
            graph.trigger("temporary", [current]).unwrap();
            graph
                .connect(existing.output(), replacement.inlet())
                .unwrap();
            assert_eq!(graph.module.edges.len(), 1);
            assert_eq!(graph.module.next_connection, 1);
            graph.compile().unwrap();
        }
    }

    #[test]
    fn rollback_preserves_interior_configuration_and_pending_map_identity() {
        for panic in [false, true] {
            let mut graph = GraphBuilder::new("interior");
            let existing = graph.node("existing", |v: (i64, i64)| v.0 + v.1).unwrap();
            let old_output = existing.output();
            let pending = Rc::downgrade(&graph.pending);
            let original_id = graph.pending.borrow()["existing"].declaration_id;
            let factory = Arc::downgrade(&graph.module.nodes[0].logic_factory);
            let factory_owners = factory.strong_count();
            let mut configured = None;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                graph.additions(|g| -> Result<(), String> {
                    assert!(Weak::ptr_eq(&pending, &Rc::downgrade(&g.graph.pending)));
                    assert_eq!(Rc::strong_count(&g.graph.pending), 1);
                    // Cloning prior templates would add owners of their factories.
                    assert_eq!(factory.strong_count(), factory_owners);
                    configured = Some(existing.input(Layout::fields()).unwrap());
                    let temporary = g.node("temporary", |v: i64| v).unwrap();
                    g.connect(configured.as_ref().unwrap().output(), temporary.inlet())?;
                    assert!(!panic, "rollback with interior edit");
                    Err("rollback".into())
                })
            }));
            assert_eq!(result.is_err(), panic);
            if let Ok(result) = result {
                assert!(result.is_err());
            }
            assert!(Weak::ptr_eq(&pending, &Rc::downgrade(&graph.pending)));
            assert_ne!(
                graph.pending.borrow()["existing"].declaration_id,
                original_id
            );
            assert_eq!(graph.pending.borrow().len(), 1);
            assert!(graph.module.edges.is_empty());
            let sink = graph.node("temporary", |v: i64| v).unwrap();
            assert!(graph.connect(old_output, sink.inlet()).is_err());
            let configured = configured.unwrap().input(Layout::atomic()).unwrap();
            graph.connect(configured.output(), sink.inlet()).unwrap();
            graph.compile().unwrap();
        }
    }
}
