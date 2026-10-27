//! Usage-first, typed graph-v2 module authoring (structural V0 only).
//! This authors offline IR. It does not execute, authorize or package v2 yet.
use crate::{
    graph_v2::{
        FlowV2, GraphContributionV2, InRefV2, ModuleDeclV2, ModuleEntryV2, ModuleExitV2,
        NodeDeclV2, NodeKindV2, OutRefV2, PortDeclV2, PortRefV2,
    },
    Code, Failure, Operation, Portable, ValueSchema,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
};

fn invalid(message: impl Into<String>) -> Failure {
    Failure::new(Code::DescriptorInvalid, message)
}

pub struct Input<T> {
    location: InRefV2,
    _type: PhantomData<fn(T)>,
}

pub struct Output<T> {
    location: OutRefV2,
    _type: PhantomData<fn() -> T>,
}

impl<T> Clone for Input<T> {
    fn clone(&self) -> Self {
        Self {
            location: self.location.clone(),
            _type: PhantomData,
        }
    }
}
impl<T> Clone for Output<T> {
    fn clone(&self) -> Self {
        Self {
            location: self.location.clone(),
            _type: PhantomData,
        }
    }
}

pub struct Invoke<O: Operation> {
    node: String,
    _type: PhantomData<fn() -> O>,
}
impl<O: Operation> Invoke<O> {
    pub fn input(&self) -> Input<O::Request> {
        Input {
            location: InRefV2::Node(PortRefV2 {
                node: self.node.clone(),
                port: "in".into(),
            }),
            _type: PhantomData,
        }
    }
    pub fn ok(&self) -> Output<O::Response> {
        Output {
            location: OutRefV2::Node(PortRefV2 {
                node: self.node.clone(),
                port: "ok".into(),
            }),
            _type: PhantomData,
        }
    }
    pub fn err(&self) -> Output<O::Error> {
        Output {
            location: OutRefV2::Node(PortRefV2 {
                node: self.node.clone(),
                port: "err".into(),
            }),
            _type: PhantomData,
        }
    }
}

#[derive(Clone)]
pub struct Module {
    id: String,
    revision: u32,
    prefix: String,
    members: BTreeSet<String>,
    entries: BTreeMap<String, ValueSchema>,
    exits: BTreeMap<String, ValueSchema>,
}

impl Module {
    pub fn input<T: Portable>(&self, name: &str) -> Result<Input<T>, Failure> {
        let schema = T::SCHEMA.owned()?;
        if self.entries.get(name) != Some(&schema) {
            return Err(invalid(format!(
                "unknown or incompatible input on module {} revision {}: {name}",
                self.id, self.revision
            )));
        }
        Ok(Input {
            location: InRefV2::ModuleEntry {
                module: self.id.clone(),
                port: name.into(),
            },
            _type: PhantomData,
        })
    }

    pub fn output<T: Portable>(&self, name: &str) -> Result<Output<T>, Failure> {
        let schema = T::SCHEMA.owned()?;
        if self.exits.get(name) != Some(&schema) {
            return Err(invalid(format!(
                "unknown or incompatible output on module {} revision {}: {name}",
                self.id, self.revision
            )));
        }
        Ok(Output {
            location: OutRefV2::ModuleExit {
                module: self.id.clone(),
                port: name.into(),
            },
            _type: PhantomData,
        })
    }

    /// An internal node ID is discoverable for inspection/advanced patches.
    /// Its identity is not a stable public interface or a permission grant.
    pub fn internal(&self, local: &str) -> Result<String, Failure> {
        let node = format!("{}.{}", self.prefix, local);
        if !self.members.contains(&node) {
            return Err(invalid(format!("unknown internal node {node}")));
        }
        Ok(node)
    }

    pub fn revision(&self) -> u32 {
        self.revision
    }
}

pub struct Graph {
    plan: GraphContributionV2,
}

impl Graph {
    pub fn new(contributor: &str) -> Self {
        Self {
            plan: GraphContributionV2 {
                contributor: contributor.into(),
                nodes: Vec::new(),
                modules: Vec::new(),
                flows: Vec::new(),
            },
        }
    }

    pub fn module(
        &mut self,
        id: &str,
        revision: u32,
        build: impl FnOnce(&mut ModuleBuilder<'_>) -> Result<(), Failure>,
    ) -> Result<Module, Failure> {
        if !crate::identifier(id)
            || revision == 0
            || self.plan.modules.iter().any(|module| module.id == id)
        {
            return Err(invalid(format!("invalid or duplicate module {id}")));
        }
        let before_nodes = self.plan.nodes.len();
        let before_flows = self.plan.flows.len();
        let mut builder = ModuleBuilder {
            plan: &mut self.plan,
            id: id.into(),
            revision,
            members: Vec::new(),
            entries: Vec::new(),
            exits: Vec::new(),
        };
        if let Err(error) = build(&mut builder) {
            self.plan.nodes.truncate(before_nodes);
            self.plan.flows.truncate(before_flows);
            return Err(error);
        }
        let module = Module {
            id: builder.id.clone(),
            revision,
            prefix: format!("{}/{}", builder.plan.contributor, builder.id),
            members: builder.members.iter().cloned().collect(),
            entries: builder
                .entries
                .iter()
                .map(|entry| (entry.name.clone(), entry.schema.clone()))
                .collect(),
            exits: builder
                .exits
                .iter()
                .map(|exit| (exit.name.clone(), exit.schema.clone()))
                .collect(),
        };
        builder.plan.modules.push(ModuleDeclV2 {
            id: builder.id,
            revision: builder.revision,
            members: builder.members,
            entries: builder.entries,
            exits: builder.exits,
        });
        Ok(module)
    }

    /// One typed Flow between logical module interfaces or concrete node ports.
    pub fn flow<T: Portable>(&mut self, from: Output<T>, to: Input<T>) {
        append_flow(&mut self.plan, from.location, to.location);
    }

    pub fn finish(self) -> Result<GraphContributionV2, Failure> {
        self.plan.normalized()
    }
}

pub struct ModuleBuilder<'a> {
    plan: &'a mut GraphContributionV2,
    id: String,
    revision: u32,
    members: Vec<String>,
    entries: Vec<ModuleEntryV2>,
    exits: Vec<ModuleExitV2>,
}

impl ModuleBuilder<'_> {
    fn node_id(&self, local: &str) -> Result<String, Failure> {
        if !crate::identifier(local) {
            return Err(invalid(format!("invalid local node name {local}")));
        }
        let id = format!("{}/{}.{}", self.plan.contributor, self.id, local);
        if self.plan.nodes.iter().any(|node| node.id == id) {
            return Err(invalid(format!("duplicate internal node {id}")));
        }
        Ok(id)
    }

    pub fn invoke<O: Operation>(&mut self, local: &str) -> Result<Invoke<O>, Failure> {
        self.selected_invoke::<O>(local, "")
    }

    pub fn selected_invoke<O: Operation>(
        &mut self,
        local: &str,
        selector: &str,
    ) -> Result<Invoke<O>, Failure> {
        let node = self.node_id(local)?;
        self.plan.nodes.push(NodeDeclV2 {
            id: node.clone(),
            module: self.id.clone(),
            kind: NodeKindV2::Invoke {
                operation: O::KEY.into(),
                selector: selector.into(),
            },
            inputs: vec![PortDeclV2 {
                name: "in".into(),
                schema: O::Request::SCHEMA.owned()?,
            }],
            outputs: vec![
                PortDeclV2 {
                    name: "ok".into(),
                    schema: O::Response::SCHEMA.owned()?,
                },
                PortDeclV2 {
                    name: "err".into(),
                    schema: O::Error::SCHEMA.owned()?,
                },
            ],
        });
        self.members.push(node.clone());
        Ok(Invoke {
            node,
            _type: PhantomData,
        })
    }

    pub fn source<T: Portable>(&mut self, local: &str, event: &str) -> Result<Output<T>, Failure> {
        let node = self.node_id(local)?;
        self.plan.nodes.push(NodeDeclV2 {
            id: node.clone(),
            module: self.id.clone(),
            kind: NodeKindV2::Source {
                event: event.into(),
            },
            inputs: vec![],
            outputs: vec![PortDeclV2 {
                name: "out".into(),
                schema: T::SCHEMA.owned()?,
            }],
        });
        self.members.push(node.clone());
        Ok(Output {
            location: OutRefV2::Node(PortRefV2 {
                node,
                port: "out".into(),
            }),
            _type: PhantomData,
        })
    }

    pub fn flow<T: Portable>(&mut self, from: Output<T>, to: Input<T>) -> Result<(), Failure> {
        let OutRefV2::Node(ref source) = from.location else {
            return Err(invalid("internal Flow must originate inside the module"));
        };
        let InRefV2::Node(ref target) = to.location else {
            return Err(invalid("internal Flow must end inside the module"));
        };
        if !self.members.contains(&source.node) || !self.members.contains(&target.node) {
            return Err(invalid("internal Flow crosses module ownership"));
        }
        append_flow(self.plan, from.location, to.location);
        Ok(())
    }

    pub fn entry<T: Portable>(&mut self, name: &str, target: Input<T>) -> Result<(), Failure> {
        let InRefV2::Node(target) = target.location else {
            return Err(invalid("module entry must target an internal node"));
        };
        if !self.members.contains(&target.node)
            || self.entries.iter().any(|entry| entry.name == name)
        {
            return Err(invalid("duplicate entry or foreign target"));
        }
        self.entries.push(ModuleEntryV2 {
            name: name.into(),
            schema: T::SCHEMA.owned()?,
            targets: vec![target],
        });
        Ok(())
    }

    pub fn exit<T: Portable>(&mut self, name: &str, source: Output<T>) -> Result<(), Failure> {
        let OutRefV2::Node(source) = source.location else {
            return Err(invalid("module exit must use an internal node"));
        };
        if !self.members.contains(&source.node) || self.exits.iter().any(|exit| exit.name == name) {
            return Err(invalid("duplicate exit or foreign source"));
        }
        self.exits.push(ModuleExitV2 {
            name: name.into(),
            schema: T::SCHEMA.owned()?,
            source,
        });
        Ok(())
    }
}

fn append_flow(plan: &mut GraphContributionV2, from: OutRefV2, to: InRefV2) {
    // Stable logical identifiers must never depend on Rust Debug formatting.
    let source = match &from {
        OutRefV2::Node(port) => format!("node:{}/{}", port.node, port.port),
        OutRefV2::ModuleExit { module, port } => format!("exit:{module}/{port}"),
    };
    let target = match &to {
        InRefV2::Node(port) => format!("node:{}/{}", port.node, port.port),
        InRefV2::ModuleEntry { module, port } => format!("entry:{module}/{port}"),
    };
    plan.flows.push(FlowV2 {
        id: format!("{source}->{target}"),
        from,
        to,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;
    impl Operation for Echo {
        type Request = String;
        type Response = String;
        type Error = String;
        const KEY: &'static str = "example.echo@1/complete";
    }

    fn fixture(reverse_order: bool) -> GraphContributionV2 {
        let mut graph = Graph::new("example.graph");
        let producer = |g: &mut Graph| {
            g.module("producer", 1, |m| {
                let source = m.source::<String>("submitted", "example.prompt")?;
                let first = m.invoke::<Echo>("first")?;
                m.flow(source, first.input())?;
                m.exit("result", first.ok())
            })
        };
        let consumer = |g: &mut Graph| {
            g.module("consumer", 1, |m| {
                let second = m.invoke::<Echo>("second")?;
                m.entry("query", second.input())?;
                m.exit("result", second.ok())
            })
        };
        let (a, b) = if reverse_order {
            let b = consumer(&mut graph).unwrap();
            let a = producer(&mut graph).unwrap();
            (a, b)
        } else {
            let a = producer(&mut graph).unwrap();
            let b = consumer(&mut graph).unwrap();
            (a, b)
        };
        graph.flow(
            a.output::<String>("result").unwrap(),
            b.input::<String>("query").unwrap(),
        );
        graph.finish().unwrap()
    }

    #[test]
    fn small_author_ux_retains_stable_module_boundaries() {
        let graph = fixture(false);
        assert_eq!(graph.nodes.len(), 3);
        assert_eq!(graph.modules.len(), 2);
        assert_eq!(graph.flows.len(), 2);
        assert_eq!(graph.lowered_flows().unwrap().len(), 2);
        assert!(
            matches!(graph.flows[1].from, OutRefV2::ModuleExit { .. })
                || matches!(graph.flows[0].from, OutRefV2::ModuleExit { .. })
        );
        assert_eq!(graph, fixture(true));
        let consumer = graph.modules.iter().find(|m| m.id == "consumer").unwrap();
        assert_eq!(consumer.entries[0].name, "query");
        assert_eq!(
            consumer.entries[0].targets[0].node,
            "example.graph/consumer.second"
        );
    }

    #[test]
    fn wrong_interface_type_is_an_error_without_a_cast() {
        let mut graph = Graph::new("example.graph");
        let module = graph
            .module("echo", 1, |m| {
                let invoke = m.invoke::<Echo>("echo")?;
                m.entry("query", invoke.input())?;
                m.exit("result", invoke.ok())
            })
            .unwrap();
        assert!(module.input::<u64>("query").is_err());
        assert!(module.output::<u64>("result").is_err());
        assert_eq!(module.internal("echo").unwrap(), "example.graph/echo.echo");
        assert!(module.internal("nonexistent").is_err());
        assert!(graph.finish().is_ok());
    }

    #[test]
    fn foreign_member_and_instantaneous_cycle_reject() {
        let mut graph = fixture(false);
        graph.modules[0]
            .members
            .push("example.graph/producer.first".into());
        assert!(graph.validate().is_err());

        let mut graph = fixture(false);
        graph.flows.push(FlowV2 {
            id: "cycle".into(),
            from: OutRefV2::Node(PortRefV2 {
                node: "example.graph/consumer.second".into(),
                port: "ok".into(),
            }),
            to: InRefV2::Node(PortRefV2 {
                node: "example.graph/producer.first".into(),
                port: "in".into(),
            }),
        });
        assert!(graph.validate().is_err());
    }

    #[test]
    fn duplicate_module_and_unbound_interface_reject() {
        let mut graph = Graph::new("example.graph");
        graph
            .module("a", 1, |m| {
                let invoke = m.invoke::<Echo>("echo")?;
                m.entry("query", invoke.input())
            })
            .unwrap();
        assert!(graph.module("a", 1, |_m| Ok(())).is_err());
        let mut graph = fixture(false);
        graph.modules[0].entries.push(ModuleEntryV2 {
            name: "bad".into(),
            schema: String::SCHEMA.owned().unwrap(),
            targets: vec![],
        });
        assert!(graph.validate().is_err());
    }
}
