//! Graph-v2 V0 selected handoffs. No business policy, retry engine, graph-v1 shim,
//! stream or deferred wave. Only Kernel's existing call_graph dispatcher invokes
//! native code; the simulator in unit tests uses the identical handoff traversal.
use crate::{
    resolve::{Prepared, Selection},
    runtime::{Outcome, Root},
    Call,
};
use phenix_contract::{
    graph_v2::{GraphContributionV2, NodeKindV2, PortRefV2},
    Code, Failure, Value, ValueSchema,
};
use std::collections::{BTreeMap, BTreeSet};

fn invalid(message: impl Into<String>) -> Failure {
    Failure::new(Code::DescriptorInvalid, message)
}

/// Traces intentionally never contain prompt text, model replies, credentials,
/// request bytes or serialized values. Only endpoint identities and digests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceKindV2 {
    Source,
    Handoff,
    InvokeOk,
    InvokeDomainError,
    Sink,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEntryV2 {
    pub kind: TraceKindV2,
    pub from: String,
    pub to: String,
    pub operation: Option<String>,
    pub digest: String,
}
#[derive(Debug)]
pub struct FlowRunV2 {
    pub trace: Vec<TraceEntryV2>,
    /// Logical terminal outputs are returned to the trusted caller, not emitted
    /// as part of a trace or observation log.
    pub terminal: BTreeMap<PortRefV2, Value>,
}
/// A graph is admitted only after binding and grants are checked by Prepared.
/// Its selected nodes are resolved once for the pinned composition.
pub struct PreparedFlowV2 {
    plan: GraphContributionV2,
    selected: BTreeMap<String, Selection>,
    edges: BTreeMap<PortRefV2, Vec<PortRefV2>>,
}
impl PreparedFlowV2 {
    fn compile_with(
        original: &GraphContributionV2,
        mut resolve: impl FnMut(&str, &str, &str) -> Result<Selection, Failure>,
    ) -> Result<Self, Failure> {
        let plan = original.normalized()?;
        let mut selected = BTreeMap::new();
        for node in &plan.nodes {
            if let NodeKindV2::Invoke {
                operation,
                selector,
            } = &node.kind
            {
                let resolved = resolve(&plan.contributor, selector, operation)?;
                if resolved.key() != *operation {
                    return Err(invalid(format!("selection does not match {}", node.id)));
                }
                selected.insert(node.id.clone(), resolved);
            }
        }
        let mut edges: BTreeMap<PortRefV2, Vec<PortRefV2>> = BTreeMap::new();
        let mut destinations = BTreeSet::new();
        for edge in plan.lowered_flows()? {
            if !destinations.insert(edge.to.clone()) {
                return Err(invalid(format!(
                    "ambiguous V0 handoff to {}.{}; explicit merge required",
                    edge.to.node, edge.to.port
                )));
            }
            edges.entry(edge.from).or_default().push(edge.to);
        }
        for endpoints in edges.values_mut() {
            endpoints.sort();
        }
        Ok(Self {
            plan,
            selected,
            edges,
        })
    }

    /// One finite V0 wave. The real entrypoint on Root uses this traversal with
    /// the kernel's existing dispatcher; only tests supply an in-process handler.
    fn walk(
        &self,
        event: &str,
        input: &Value,
        mut invoke: impl FnMut(&Selection, &Value) -> Result<Outcome, Failure>,
    ) -> Result<FlowRunV2, Failure> {
        let mut trace = Vec::new();
        let mut terminal = BTreeMap::new();
        let mut pending: BTreeMap<String, Value> = BTreeMap::new();
        let mut emitted = BTreeSet::new();
        let mut found = false;
        for node in &self.plan.nodes {
            if let NodeKindV2::Source {
                event: source_event,
            } = &node.kind
            {
                if source_event != event {
                    continue;
                }
                found = true;
                let output = &node.outputs[0];
                if !output.schema.accepts(input) {
                    return Err(Failure::new(
                        Code::SchemaRejected,
                        format!("source {} input schema mismatch", node.id),
                    ));
                }
                Self::route(
                    &self.edges,
                    &mut pending,
                    &mut terminal,
                    &mut trace,
                    &node.id,
                    &output.name,
                    input,
                    &output.schema,
                    &self.plan,
                )?;
                emitted.insert(node.id.as_str());
                trace.push(TraceEntryV2 {
                    kind: TraceKindV2::Source,
                    from: event.into(),
                    to: node.id.clone(),
                    operation: None,
                    digest: digest(input)?,
                });
            }
        }
        if !found {
            return Err(invalid(format!("unregistered V0 source event: {event}")));
        }
        // Ready nodes are chosen by stable node identity, not declaration order.
        // No unbounded waves or synthesized input defaults.
        loop {
            let Some(node) = self.plan.nodes.iter().find(|node| {
                matches!(node.kind, NodeKindV2::Invoke { .. })
                    && !emitted.contains(node.id.as_str())
                    && pending.contains_key(&node.id)
            }) else {
                break;
            };
            let request = pending.remove(&node.id).expect("ready node input");
            let contract = self
                .selected
                .get(&node.id)
                .ok_or_else(|| invalid(format!("unselected operation node {}", node.id)))?;
            if !node.inputs[0].schema.accepts(&request) {
                return Err(Failure::new(
                    Code::SchemaRejected,
                    "V0 handoff request schema mismatch",
                ));
            }
            let outcome = invoke(contract, &request)?;
            let (port, value, kind) = match outcome {
                Outcome::Success(value) => ("ok", value, TraceKindV2::InvokeOk),
                Outcome::Domain(value) => ("err", value, TraceKindV2::InvokeDomainError),
            };
            let output = node
                .outputs
                .iter()
                .find(|output| output.name == port)
                .ok_or_else(|| invalid("missing selected output port"))?;
            if !output.schema.accepts(&value) {
                return Err(Failure::new(
                    Code::SchemaRejected,
                    "native result violates V0 output schema",
                ));
            }
            trace.push(TraceEntryV2 {
                kind,
                from: node.id.clone(),
                to: format!("{}.{}", node.id, port),
                operation: Some(contract.key()),
                digest: digest(&value)?,
            });
            Self::route(
                &self.edges,
                &mut pending,
                &mut terminal,
                &mut trace,
                &node.id,
                port,
                &value,
                &output.schema,
                &self.plan,
            )?;
            emitted.insert(node.id.as_str());
        }
        Ok(FlowRunV2 { trace, terminal })
    }

    #[allow(clippy::too_many_arguments)]
    fn route(
        edges: &BTreeMap<PortRefV2, Vec<PortRefV2>>,
        pending: &mut BTreeMap<String, Value>,
        terminal: &mut BTreeMap<PortRefV2, Value>,
        trace: &mut Vec<TraceEntryV2>,
        from_node: &str,
        port: &str,
        value: &Value,
        schema: &ValueSchema,
        plan: &GraphContributionV2,
    ) -> Result<(), Failure> {
        let origin = PortRefV2 {
            node: from_node.into(),
            port: port.into(),
        };
        let digest = digest(value)?;
        if let Some(destinations) = edges.get(&origin) {
            for target in destinations {
                let receiver = plan
                    .nodes
                    .iter()
                    .find(|node| node.id == target.node)
                    .ok_or_else(|| invalid("missing selected handoff destination"))?;
                let input = receiver
                    .inputs
                    .iter()
                    .find(|p| p.name == target.port)
                    .ok_or_else(|| invalid("unknown selected input port"))?;
                if input.schema != *schema || !input.schema.accepts(value) {
                    return Err(Failure::new(
                        Code::SchemaRejected,
                        "V0 dataflow schema mismatch",
                    ));
                }
                if pending.insert(target.node.clone(), value.clone()).is_some() {
                    return Err(invalid("multiple deliveries to one V0 invocation"));
                }
                trace.push(TraceEntryV2 {
                    kind: TraceKindV2::Handoff,
                    from: format!("{from_node}.{port}"),
                    to: format!("{}.{}", target.node, target.port),
                    operation: None,
                    digest: digest.clone(),
                });
            }
        } else {
            terminal.insert(origin.clone(), value.clone());
            trace.push(TraceEntryV2 {
                kind: TraceKindV2::Sink,
                from: format!("{from_node}.{port}"),
                to: "terminal".into(),
                operation: None,
                digest,
            });
        }
        Ok(())
    }
}

fn digest(value: &Value) -> Result<String, Failure> {
    Ok(phenix_artifact::digest(&value.encode()?))
}

impl Prepared {
    /// Validate selected operations, their ABI schemas and owner grants before
    /// constructing an executable graph-v2 handoff plan.
    pub fn compile_flow_v2(&self, graph: &GraphContributionV2) -> Result<PreparedFlowV2, Failure> {
        if !self.packages.contains_key(&graph.contributor) {
            return Err(Failure::new(
                Code::MissingBinding,
                "graph owner is not selected",
            ));
        }
        PreparedFlowV2::compile_with(graph, |owner, selector, key| {
            let provider = if selector.is_empty() {
                owner.to_owned()
            } else {
                let identity = phenix_contract::import_identity(selector, key);
                self.bindings
                    .get(&(owner.to_owned(), identity))
                    .cloned()
                    .ok_or_else(|| Failure::new(Code::MissingBinding, "unselected V0 import"))?
            };
            let (interface, operation) = key
                .split_once('/')
                .ok_or_else(|| invalid("invalid graph-v2 operation key"))?;
            let selected = Selection {
                interface: interface.into(),
                operation: operation.into(),
                provider,
            };
            let prepared = self
                .operations
                .get(&(selected.provider.clone(), key.to_owned()))
                .ok_or_else(|| {
                    Failure::new(Code::MissingBinding, "selected V0 operation absent")
                })?;
            let original = graph.nodes.iter().find(|node| {
                matches!(&node.kind, NodeKindV2::Invoke { operation, .. } if operation == key)
            }).ok_or_else(|| invalid("missing original V0 operation"))?;
            if prepared.schema.request != original.inputs[0].schema
                || prepared.schema.response
                    != original
                        .outputs
                        .iter()
                        .find(|port| port.name == "ok")
                        .unwrap()
                        .schema
                || prepared.schema.error
                    != original
                        .outputs
                        .iter()
                        .find(|port| port.name == "err")
                        .unwrap()
                        .schema
            {
                return Err(Failure::new(
                    Code::SchemaRejected,
                    "selected V0 ABI schema mismatch",
                ));
            }
            if !self.permits(owner, key)
                || !self
                    .grants
                    .contains_key(&(owner.to_owned(), key.to_owned()))
            {
                return Err(Failure::new(
                    Code::Denied,
                    "V0 operation lacks prepared grant",
                ));
            }
            Ok(selected)
        })
    }
}
impl Root<'_> {
    /// Execute one admitted source wave against the same selected kernel
    /// dispatcher as ordinary native calls. Pending/deferred waves are not V0.
    pub fn run_flow_v2(
        &self,
        graph: &PreparedFlowV2,
        event: &str,
        request: &Value,
    ) -> Result<FlowRunV2, Failure> {
        graph.walk(event, request, |selected, value| {
            match self.call_graph(&graph.plan.contributor, selected, value, false)? {
                Call::Ready(result) => Ok(result),
                Call::Pending(_) => Err(invalid("V0 cannot suspend a native invocation")),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_basic_shell as shell;

    fn resolved(_: &str, _: &str, key: &str) -> Result<Selection, Failure> {
        let (interface, operation) = key.split_once('/').unwrap();
        Ok(Selection {
            interface: interface.into(),
            operation: operation.into(),
            provider: "mock.selected".into(),
        })
    }
    fn wrapped(key: &str, value: &Value) -> Result<Outcome, Failure> {
        let value = match value {
            Value::Map(fields) => fields
                .get("text")
                .or_else(|| fields.get("generation"))
                .or_else(|| fields.get("provider"))
                .ok_or_else(|| invalid("missing typed shell input field"))?
                .clone(),
            _ => return Err(invalid("not a shell payload")),
        };
        let output = match key {
            "phenix.shell.session@1/admit" => Value::map([
                ("session", Value::String("fixture-session-1".into())),
                ("run", Value::String("fixture-run-1".into())),
                ("text", value),
            ]),
            "phenix.shell.journal@1/commit"
            | "phenix.shell.observer@1/record"
            | "phenix.shell.auth@1/authenticate" => Value::map([("accepted", Value::Bool(true))]),
            "phenix.shell.lifecycle@1/initialize" => Value::map([("generation", value)]),
            "phenix.shell.recall@1/search"
            | "phenix.shell.context@1/assemble"
            | "phenix.shell.budget@1/measure"
            | "phenix.shell.model@1/complete" => Value::map([("text", value)]),
            _ => return Err(invalid(format!("unhandled shell stage {key}"))),
        };
        Ok(Outcome::Success(output))
    }

    #[test]
    fn a_prompt_crosses_every_selected_handoff_and_fans_out_exactly_once() {
        let plan = PreparedFlowV2::compile_with(&shell::topology().unwrap(), resolved).unwrap();
        let mut invoked = Vec::new();
        let secret = "SENSITIVE PROMPT PAYLOAD";
        let result = plan
            .walk(
                "phenix.shell.prompt",
                &Value::map([("text", Value::String(secret.into()))]),
                |selection, request| {
                    invoked.push(selection.key());
                    wrapped(&selection.key(), request)
                },
            )
            .unwrap();
        assert_eq!(invoked.len(), 7);
        assert_eq!(
            invoked.iter().filter(|k| k.ends_with("/complete")).count(),
            1
        );
        assert_eq!(invoked.iter().filter(|k| k.ends_with("/commit")).count(), 1);
        assert_eq!(invoked.iter().filter(|k| k.ends_with("/record")).count(), 1);
        assert_eq!(result.terminal.len(), 2);
        assert_eq!(
            result
                .trace
                .iter()
                .filter(|t| t.kind == TraceKindV2::Handoff)
                .count(),
            7
        );
        assert!(!format!("{:?}", result.trace).contains(secret));
        assert!(result.trace.iter().all(|t| !t.digest.is_empty()));
    }

    #[test]
    fn distinct_roots_do_not_accidentally_activate_prompt_or_model() {
        let plan = PreparedFlowV2::compile_with(&shell::topology().unwrap(), resolved).unwrap();
        for (event, field, text) in [
            ("phenix.shell.begin", "generation", "rev1"),
            ("phenix.shell.auth_request", "provider", "test"),
        ] {
            let mut invoked = Vec::new();
            let result = plan
                .walk(
                    event,
                    &Value::map([(field, Value::String(text.into()))]),
                    |selection, request| {
                        invoked.push(selection.key());
                        wrapped(&selection.key(), request)
                    },
                )
                .unwrap();
            assert_eq!(invoked.len(), 1);
            assert_eq!(result.terminal.len(), 1);
        }
    }

    #[test]
    fn missing_event_and_wrong_payload_fail_closed() {
        let plan = PreparedFlowV2::compile_with(&shell::topology().unwrap(), resolved).unwrap();
        assert!(plan
            .walk("unregistered", &Value::Null, |_, _| unreachable!())
            .is_err());
        assert!(plan
            .walk("phenix.shell.prompt", &Value::Null, |_, _| unreachable!())
            .is_err());
    }

    #[test]
    fn domain_error_halts_success_path_without_dispatching_followups() {
        let plan = PreparedFlowV2::compile_with(&shell::topology().unwrap(), resolved).unwrap();
        let mut calls = 0;
        let result = plan
            .walk(
                "phenix.shell.prompt",
                &Value::map([("text", Value::String("x".into()))]),
                |_, _| {
                    calls += 1;
                    Ok(Outcome::Domain(Value::String("denied".into())))
                },
            )
            .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(result.terminal.len(), 1);
        assert_eq!(
            result
                .trace
                .iter()
                .filter(|t| t.kind == TraceKindV2::InvokeDomainError)
                .count(),
            1
        );
    }

    #[test]
    fn forged_response_and_ambiguous_handoff_fail_closed() {
        let plan = PreparedFlowV2::compile_with(&shell::topology().unwrap(), resolved).unwrap();
        assert!(plan
            .walk(
                "phenix.shell.prompt",
                &Value::map([("text", Value::String("x".into()))]),
                |_, _| Ok(Outcome::Success(Value::Null)),
            )
            .is_err());
        let mut graph = shell::topology().unwrap();
        let original = graph
            .flows
            .iter()
            .find(|f| f.id.contains("memory"))
            .unwrap()
            .clone();
        // Give the same consumer a second source via a different logical edge.
        let mut duplicate = original;
        duplicate.id.push_str(".duplicate");
        graph.flows.push(duplicate);
        assert!(PreparedFlowV2::compile_with(&graph, resolved).is_err());
    }
}

#[cfg(test)]
mod live_session_handoff_tests {
    use super::*;
    use crate::resolve::{Composition, ScopedGrant};
    use phenix_artifact::{digest, extract, PackageRecord};
    use phenix_basic_shell::{Prompt, Session, SessionTurn};
    use phenix_contract::{Authority, Portable};
    use std::{collections::BTreeSet, fs, path::Path};

    fn selected() -> Selection {
        Selection {
            interface: "phenix.shell.session@1".into(),
            operation: "admit".into(),
            provider: "phenix.shell.session".into(),
        }
    }

    fn single_stage() -> GraphContributionV2 {
        use phenix_basic_shell::SessionAdmit;
        // The SDK lowers the same public typed author API into canonical IR.
        let mut graph = phenix_sdk::graph::Graph::new("phenix.shell.session");
        let source = graph
            .module("frontend", 1, |m| {
                let source = m.source::<Prompt>("prompt", "phenix.shell.prompt")?;
                m.exit("submitted", source)
            })
            .unwrap();
        let session = graph
            .module("session", 1, |m| {
                let operation = m.invoke::<SessionAdmit>("admit")?;
                m.entry("prompt", operation.input())?;
                m.exit("turn", operation.ok())
            })
            .unwrap();
        graph.flow(
            source.output::<Prompt>("submitted").unwrap(),
            session.input::<Prompt>("prompt").unwrap(),
        );
        graph.finish().unwrap()
    }

    /// An explicit test-only trust root; never used for mutable production
    /// packages. The native ELF and embedded metadata are independently checked.
    fn packaged_session(native: &Path) -> String {
        let path = if native.is_absolute() {
            native.to_path_buf()
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(native)
        };
        let image = fs::read(&path).unwrap();
        let descriptor = extract(&image).unwrap();
        let path = std::env::temp_dir().join(format!(
            "phenix-native-graph-v2-session-{}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("session.so"), &image).unwrap();
        fs::write(path.join("descriptor.bin"), &descriptor).unwrap();
        let record = PackageRecord {
            format: "phenix.native-package.v2".into(),
            artifact: "session.so".into(),
            descriptor: "descriptor.bin".into(),
            target: "x86_64-unknown-linux-gnu".into(),
            artifact_sha256: digest(&image),
            descriptor_sha256: digest(&descriptor),
        };
        fs::write(
            path.join("package.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        path.to_str().unwrap().to_owned()
    }

    fn composition(package: String, with_graph_grant: bool) -> Composition {
        let entry = selected();
        let grant = |caller: &str| ScopedGrant {
            caller: caller.into(),
            interface: entry.interface.clone(),
            operation: entry.operation.clone(),
            resources: BTreeSet::new(),
        };
        let mut grants = vec![grant("root")];
        if with_graph_grant {
            grants.push(grant("phenix.shell.session"));
        }
        Composition {
            format: "phenix.composition.v3".into(),
            artifacts: vec![package],
            definitions: Vec::new(),
            entry: entry.clone(),
            slots: Vec::new(),
            grants,
            excluded: BTreeSet::new(),
            substitutions: Vec::new(),
            authority: Authority {
                operations: [entry.key()].into(),
                resources: BTreeSet::new(),
            },
            resource: None,
        }
    }

    #[test]
    fn selected_native_session_handoff_requires_grants_and_preserves_owner_state() {
        // Ordinary workspace tests remain portable; CI explicitly supplies
        // the release-built fixture for this stronger native-composed gate.
        let Ok(filename) = std::env::var("PHENIX_SESSION_FIXTURE_NATIVE") else {
            return;
        };
        let package = packaged_session(Path::new(&filename));
        let topology = single_stage();

        let denied = crate::resolve::Prepared::prepare(composition(package.clone(), false))
            .unwrap()
            .compile_flow_v2(&topology);
        assert!(matches!(
            denied,
            Err(Failure {
                code: Code::Denied,
                ..
            })
        ));

        let prepared = crate::resolve::Prepared::prepare(composition(package, true)).unwrap();
        let selected_graph = prepared.compile_flow_v2(&topology).unwrap();
        let kernel = prepared.load().unwrap();

        for (prompt, expected_run) in [("hello", "fixture-run-1"), ("next", "fixture-run-2")] {
            let root = kernel.admit_default_root().unwrap();
            let result = root
                .run_flow_v2(
                    &selected_graph,
                    "phenix.shell.prompt",
                    &Prompt {
                        text: prompt.into(),
                    }
                    .to_value(),
                )
                .unwrap();
            let (_, value) = result.terminal.iter().next().unwrap();
            let turn = SessionTurn::from_value(value.clone()).unwrap();
            assert_eq!(turn.session, "fixture-session-1");
            assert_eq!(turn.run, expected_run);
            assert_eq!(turn.text, prompt);
            assert_eq!(
                result
                    .trace
                    .iter()
                    .filter(|step| step.kind == TraceKindV2::Handoff)
                    .count(),
                1
            );
            assert!(!format!("{:?}", result.trace).contains(prompt));
        }
        let root = kernel.admit_default_root().unwrap();
        let result = root
            .run_flow_v2(
                &selected_graph,
                "phenix.shell.prompt",
                &Prompt { text: "  ".into() }.to_value(),
            )
            .unwrap();
        assert!(result
            .trace
            .iter()
            .any(|step| step.kind == TraceKindV2::InvokeDomainError));
        assert_eq!(result.terminal.len(), 1);
    }
}
