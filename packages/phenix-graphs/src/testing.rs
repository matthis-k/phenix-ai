//! Test support for the **production** graph authoring and executor APIs.
//!
//! A test builds a graph with `GraphBuilder` or `PatchPlanner`, creates a
//! `Fixture` from its compiled graph, then runs real waves through `Engine`.
//! No mock scheduler, test-specific graph IR, or second readiness algorithm.
//!
//! ```
//! use phenix_graph::{GraphBuilder, Value};
//! use phenix_graph::testing::{Fixture, ExpectWave};
//!
//! let mut graph = GraphBuilder::new("example");
//! let node = graph.node("increment", |x: i64| x + 1).unwrap();
//! let input = graph.expose_input("request", node.inlet()).unwrap();
//! graph.expose_output("reply", node.output()).unwrap();
//! let trigger = graph.trigger("run", [input]).unwrap();
//!
//! let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
//! fixture.fire(trigger.get(), vec![Value::local(41_i64)]).unwrap()
//!     .expect_success().expect_local_at(fixture.output("increment", "value"), &42_i64);
//! ```
use crate::{
    CompiledGraph, ContractValue, Deployment, Engine, ExecutionSession, FindResult, InputAddr,
    OutputAddr, ReportRetention, TriggerSelector, Value, WaveError, WaveReport,
};
use std::fmt::Debug;
use std::sync::Arc;

/// A reusable per-test engine. It preserves store and action instance state
/// across waves, allowing isolation and cancellation regressions to be tested.
/// It delegates to the real `Deployment` / `Engine`; no scheduler is copied.
pub struct Fixture {
    engine: Engine,
    graph: Arc<CompiledGraph>,
}

impl Fixture {
    /// Common single-thread/local scenario with full payload retention for
    /// useful assertions. Tests with custom execution bindings can use
    /// `from_deployment` instead.
    pub fn new(graph: Arc<CompiledGraph>) -> Result<Self, String> {
        Self::from_deployment(
            Deployment::new(graph).report_retention(ReportRetention::RetainPayloads),
        )
    }

    /// Inject a production deployment directly: custom executors, store,
    /// adapters and report retention follow the exact production path.
    pub fn from_deployment(deployment: Deployment) -> Result<Self, String> {
        let graph = deployment.compiled_graph();
        Ok(Self {
            engine: deployment.build()?,
            graph,
        })
    }

    /// Resolve compiler-assigned endpoints by stable hierarchical node/output
    /// names. Tests should not depend on physical IDs or insertion order.
    pub fn output(&self, node: &str, output: &str) -> OutputAddr {
        let Some(FindResult::Node(node_ref)) = self.graph.find(node) else {
            panic!("fixture has no node at {node:?}");
        };
        node_ref
            .output(output)
            .unwrap_or_else(|| panic!("fixture node {node:?} has no output {output:?}"))
    }

    /// Resolve an input from its named physical node, port, and slot.
    /// Intended for assertions about exact selected routes.
    pub fn input(&self, node: &str, port: &str, slot: &str) -> InputAddr {
        let Some(FindResult::Node(node_ref)) = self.graph.find(node) else {
            panic!("fixture has no node at {node:?}");
        };
        node_ref
            .input(port, slot)
            .unwrap_or_else(|| panic!("fixture node {node:?} has no input {port:?}/{slot:?}"))
    }

    pub fn node_id(&self, name: &str) -> u16 {
        let Some(FindResult::Node(node)) = self.graph.find(name) else {
            panic!("fixture has no node at {name:?}");
        };
        node.id()
    }

    /// Execute a real graph wave. Errors returned by `fire` are trigger
    /// admission errors; invocation failures are recorded in `WaveReport`.
    pub fn fire<T: TriggerSelector>(
        &self,
        trigger: T,
        input: Vec<Value>,
    ) -> Result<WaveReport, String> {
        self.engine.fire(trigger, input)
    }

    /// Drive the same externally-completable wave as `Engine::begin`, allowing
    /// real pending, cancellation and interleaving assertions in Rust tests.
    pub fn begin<T: TriggerSelector>(
        &self,
        trigger: T,
        input: Vec<Value>,
    ) -> Result<ExecutionSession<'_>, String> {
        self.engine.begin(trigger, input)
    }

    /// Use the real execution session for controlled completion ordering,
    /// pending/cancellation tests, and explicit assertion of in-flight work.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }
}

/// Assertions over the ordinary `WaveReport`, not a competing result type.
/// The fluent methods return `&Self` so one test can express several invariants.
pub trait ExpectWave {
    fn expect_success(&self) -> &Self;
    fn expect_cancelled(&self) -> &Self;
    fn expect_failure_containing(&self, fragment: &str) -> &Self;
    /// Match a typed failure, rather than parsing Debug-formatted text.
    fn expect_error(&self, predicate: impl Fn(&WaveError) -> bool) -> &Self;
    /// Assert exactly how many times a specific named/resolved output emitted,
    /// regardless of the report's payload-retention policy.
    fn expect_publication_count(&self, from: OutputAddr, expected: usize) -> &Self;
    /// Assert the entire multiset of destinations that *actually received*
    /// values from one output, including no selected edges or repeated fan-out.
    /// This is based on committed delivery events, not structural reachability.
    fn expect_deliveries_from(&self, from: OutputAddr, targets: &[InputAddr]) -> &Self;
    /// Assert a specific source, never relying on "last publication" ordering.
    fn expect_local_at<T>(&self, from: OutputAddr, expected: &T) -> &Self
    where
        T: Send + Sync + PartialEq + Debug + 'static;
    fn expect_wire_at<T>(&self, from: OutputAddr, expected: T) -> &Self
    where
        T: ContractValue + PartialEq + Debug;
    fn expect_invocations(&self, node: u16, count: usize) -> &Self;
}

impl ExpectWave for WaveReport {
    fn expect_success(&self) -> &Self {
        assert!(
            self.success(),
            "expected successful wave, got {:?}",
            self.error_messages()
        );
        self
    }

    fn expect_cancelled(&self) -> &Self {
        assert!(self.cancelled(), "expected cancelled wave");
        assert!(!self.success(), "cancelled wave cannot succeed");
        self
    }

    fn expect_failure_containing(&self, fragment: &str) -> &Self {
        assert!(!self.success(), "expected failed wave, but it succeeded");
        let errors = self.error_messages();
        assert!(
            errors.iter().any(|error| error.contains(fragment)),
            "expected failure containing {fragment:?}, got {errors:?}"
        );
        self
    }

    fn expect_error(&self, predicate: impl Fn(&WaveError) -> bool) -> &Self {
        assert!(!self.success(), "expected a failed wave, but it succeeded");
        assert!(
            self.errors().iter().any(predicate),
            "expected matching failure, got {:?}",
            self.errors()
        );
        self
    }

    fn expect_publication_count(&self, from: OutputAddr, expected: usize) -> &Self {
        let observed = self
            .publication_events()
            .iter()
            .filter(|event| event.from == from)
            .count();
        assert_eq!(
            observed, expected,
            "unexpected publication count from {from:?}"
        );
        self
    }

    fn expect_deliveries_from(&self, from: OutputAddr, targets: &[InputAddr]) -> &Self {
        let mut observed: Vec<_> = self
            .delivery_events()
            .iter()
            .filter(|event| event.from == from)
            .map(|event| event.to)
            .collect();
        let mut expected = targets.to_vec();
        observed.sort();
        expected.sort();
        assert_eq!(
            observed, expected,
            "unexpected committed destinations from {from:?}"
        );
        self
    }

    fn expect_local_at<T>(&self, from: OutputAddr, expected: &T) -> &Self
    where
        T: Send + Sync + PartialEq + Debug + 'static,
    {
        let mut matching = self.publications().filter(|event| {
            event.source_node_id() == from.node_id() && event.source_slot_id() == from.slot_id()
        });
        let published = matching
            .next()
            .unwrap_or_else(|| panic!("no retained publication from {from:?}"));
        assert!(
            matching.next().is_none(),
            "expected exactly one retained publication from {from:?}"
        );
        assert_eq!(
            published.borrow::<T>().expect("wrong local output type"),
            expected
        );
        self
    }

    fn expect_wire_at<T>(&self, from: OutputAddr, expected: T) -> &Self
    where
        T: ContractValue + PartialEq + Debug,
    {
        let mut matching = self.publications().filter(|event| {
            event.source_node_id() == from.node_id() && event.source_slot_id() == from.slot_id()
        });
        let published = matching
            .next()
            .unwrap_or_else(|| panic!("no retained publication from {from:?}"));
        assert!(
            matching.next().is_none(),
            "expected exactly one retained publication from {from:?}"
        );
        assert_eq!(
            published.value::<T>().expect("wrong wire output type"),
            expected
        );
        self
    }

    fn expect_invocations(&self, node: u16, count: usize) -> &Self {
        assert_eq!(
            self.invocation_count(node),
            count,
            "unexpected node {node} invocation count"
        );
        self
    }
}
