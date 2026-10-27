//! Typed, intentionally behavior-free Basic graph shell.
//! This is the target *shape*, not a native executable plugin or a P1 product.
//! The ordinary domain handlers return later; the current gate is wiring.
use phenix_sdk::{graph::Graph, graph_v2::GraphContributionV2, Failure, Operation, Portable};

#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct Prompt {
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct SessionTurn {
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct Evidence {
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct ModelContext {
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct AdmittedRequest {
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct Reply {
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct Committed {
    pub accepted: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct Observed {
    pub accepted: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct AuthRequest {
    pub provider: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct AuthOutcome {
    pub accepted: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct Begin {
    pub generation: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct Ready {
    pub generation: String,
}

macro_rules! shell_operation {
    ($name:ident, $key:literal, $request:ty, $response:ty) => {
        pub struct $name;
        impl Operation for $name {
            type Request = $request;
            type Response = $response;
            type Error = String;
            const KEY: &'static str = $key;
        }
    };
}

// These markers are type signatures for later domain plugins, not pretend handlers.
shell_operation!(
    SessionAdmit,
    "phenix.shell.session@1/admit",
    Prompt,
    SessionTurn
);
shell_operation!(
    Recall,
    "phenix.shell.recall@1/search",
    SessionTurn,
    Evidence
);
shell_operation!(
    Assemble,
    "phenix.shell.context@1/assemble",
    Evidence,
    ModelContext
);
shell_operation!(
    Measure,
    "phenix.shell.budget@1/measure",
    ModelContext,
    AdmittedRequest
);
shell_operation!(
    Complete,
    "phenix.shell.model@1/complete",
    AdmittedRequest,
    Reply
);
shell_operation!(Commit, "phenix.shell.journal@1/commit", Reply, Committed);
shell_operation!(Observe, "phenix.shell.observer@1/record", Reply, Observed);
shell_operation!(
    Authenticate,
    "phenix.shell.auth@1/authenticate",
    AuthRequest,
    AuthOutcome
);
shell_operation!(
    Initialize,
    "phenix.shell.lifecycle@1/initialize",
    Begin,
    Ready
);

/// Subgraphs are declared independently. Boundary aliases remain typed, not nodes.
/// Declaration order intentionally differs from dataflow order.
pub fn topology() -> Result<GraphContributionV2, Failure> {
    let mut graph = Graph::new("phenix.basic.shell");
    let commit = graph.module("journal", 1, |module| {
        let op = module.invoke::<Commit>("commit")?;
        module.entry("reply", op.input())?;
        module.exit("record", op.ok())
    })?;
    let observe = graph.module("observer", 1, |module| {
        let op = module.invoke::<Observe>("record")?;
        module.entry("reply", op.input())?;
        module.exit("receipt", op.ok())
    })?;
    let model = graph.module("model", 1, |module| {
        let op = module.invoke::<Complete>("complete")?;
        module.entry("request", op.input())?;
        module.exit("reply", op.ok())
    })?;
    let budget = graph.module("budget", 1, |module| {
        let op = module.invoke::<Measure>("measure")?;
        module.entry("context", op.input())?;
        module.exit("request", op.ok())
    })?;
    let context = graph.module("context", 1, |module| {
        let op = module.invoke::<Assemble>("assemble")?;
        module.entry("evidence", op.input())?;
        module.exit("context", op.ok())
    })?;
    let memory = graph.module("memory", 1, |module| {
        let op = module.invoke::<Recall>("recall")?;
        module.entry("turn", op.input())?;
        module.exit("evidence", op.ok())
    })?;
    let session = graph.module("session", 1, |module| {
        let op = module.invoke::<SessionAdmit>("admit")?;
        module.entry("prompt", op.input())?;
        module.exit("turn", op.ok())
    })?;
    let prompt = graph.module("frontend", 1, |module| {
        let source = module.source::<Prompt>("prompt", "phenix.shell.prompt")?;
        module.exit("submitted", source)
    })?;
    let auth = graph.module("auth", 1, |module| {
        let op = module.invoke::<Authenticate>("authenticate")?;
        module.entry("request", op.input())?;
        module.exit("result", op.ok())
    })?;
    let auth_source = graph.module("auth_input", 1, |module| {
        let source = module.source::<AuthRequest>("request", "phenix.shell.auth_request")?;
        module.exit("request", source)
    })?;
    let lifecycle = graph.module("lifecycle", 1, |module| {
        let op = module.invoke::<Initialize>("initialize")?;
        module.entry("begin", op.input())?;
        module.exit("ready", op.ok())
    })?;
    let begin = graph.module("startup", 1, |module| {
        let source = module.source::<Begin>("begin", "phenix.shell.begin")?;
        module.exit("begin", source)
    })?;

    graph.flow(
        prompt.output::<Prompt>("submitted")?,
        session.input::<Prompt>("prompt")?,
    );
    graph.flow(
        session.output::<SessionTurn>("turn")?,
        memory.input::<SessionTurn>("turn")?,
    );
    graph.flow(
        memory.output::<Evidence>("evidence")?,
        context.input::<Evidence>("evidence")?,
    );
    graph.flow(
        context.output::<ModelContext>("context")?,
        budget.input::<ModelContext>("context")?,
    );
    graph.flow(
        budget.output::<AdmittedRequest>("request")?,
        model.input::<AdmittedRequest>("request")?,
    );
    // A single typed reply fans out. No second model execution.
    graph.flow(
        model.output::<Reply>("reply")?,
        commit.input::<Reply>("reply")?,
    );
    graph.flow(
        model.output::<Reply>("reply")?,
        observe.input::<Reply>("reply")?,
    );
    graph.flow(
        auth_source.output::<AuthRequest>("request")?,
        auth.input::<AuthRequest>("request")?,
    );
    graph.flow(
        begin.output::<Begin>("begin")?,
        lifecycle.input::<Begin>("begin")?,
    );
    graph.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_sdk::graph_v2::{NodeKindV2, OutRefV2};

    #[test]
    fn typed_basic_shell_preserves_subgraphs_and_exact_handoffs() {
        let plan = topology().unwrap();
        assert_eq!(plan.modules.len(), 12);
        assert_eq!(plan.nodes.len(), 12);
        assert_eq!(plan.flows.len(), 9);
        assert_eq!(plan.lowered_flows().unwrap().len(), 9);
        assert_eq!(
            plan.nodes
                .iter()
                .filter(|n| matches!(n.kind, NodeKindV2::Source { .. }))
                .count(),
            3
        );
        assert_eq!(
            plan.nodes
                .iter()
                .filter(|n| matches!(n.kind, NodeKindV2::Invoke { .. }))
                .count(),
            9
        );
        assert!(plan
            .flows
            .iter()
            .all(|flow| matches!(flow.from, OutRefV2::ModuleExit { .. })));
        for module in &plan.modules {
            assert_eq!(module.revision, 1);
            assert_eq!(module.members.len(), 1);
        }
        let fanout = plan
            .lowered_flows()
            .unwrap()
            .into_iter()
            .filter(|edge| edge.from.node == "phenix.basic.shell/model.complete")
            .count();
        assert_eq!(fanout, 2);
    }

    #[test]
    fn a_wrong_handoff_type_fails_before_any_native_execution() {
        let plan = topology().unwrap();
        let journal = plan.modules.iter().find(|m| m.id == "journal").unwrap();
        assert_ne!(journal.entries[0].schema, Prompt::SCHEMA.owned().unwrap());
        let mut broken = plan;
        broken
            .modules
            .iter_mut()
            .find(|m| m.id == "journal")
            .unwrap()
            .entries[0]
            .schema = Prompt::SCHEMA.owned().unwrap();
        assert!(broken.normalized().is_err());
    }

    #[test]
    fn shell_is_normalized_and_order_independent() {
        let mut changed = topology().unwrap();
        changed.modules.reverse();
        changed.nodes.reverse();
        changed.flows.reverse();
        assert_eq!(topology().unwrap(), changed.normalized().unwrap());
    }
}
