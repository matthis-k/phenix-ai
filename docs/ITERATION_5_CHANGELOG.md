# Iteration 5 — exact edge delivery, terminal retention, one patch-endpoint grammar

**Baseline:** `phenix_graph_iteration_4_20261107.zip`. This is an additional
standalone graph iteration; kernel, agent, native plugin ABI, and frontends remain
excluded. No Python test harness or compatibility executor was introduced.

## Targets set before editing

| Responsibility | Concrete friction | Acceptance condition | Change |
|---|---|---|---|
| Output routing & execution | A selector could skip edges, but reports only exposed publications, not committed edge deliveries | Every accepted edge can be inspected; rejected fan-out has *zero* recorded deliveries | Added `DeliveryEvent`, `WaveReport::delivery_events` and commit-only delivery recording |
| Report retention | `TerminalOutputs` used **structural** edge existence, discarding routed outputs which sent no data | An emission that reaches no outgoing edge retains its value | Retention now checks actual delivered edges in that wave |
| Test API | Tests only counted invoked consumers, which did not identify which edge was selected | Rust assertions compare the exact multiset of selected destinations | `Fixture::input` and `ExpectWave::expect_deliveries_from` |
| Patch authoring | Output-routing paths used a separate parser from connection-footprint paths | One parser validates both constructs, with strict explicit output names for routing | Shared private `PortSelector::parse` |
| Behavioral conformance | Negative cases did not assert absence of partial delivery facts or post-failure wave reuse | Real engine tests for no-op routing, broadcast, invalid IDs, adapter failure, metadata retention and recovery | Eight new Rust integration tests |

## Source-exact code changes

These before snippets exist in the Iteration 4 archive and the after snippets
exist in this iteration. They are not hypothetical usage designs.

```rust
// BEFORE — crates/phenix-graph/tests/patch_output_routing.rs
let report = Deployment::new(compiled)
    .report_retention(ReportRetention::RetainPayloads).build().unwrap()
    .fire("go", vec![Value::local(3_i64)]).unwrap();
assert!(report.success(), "{:?}", report.error_messages());
assert_eq!(report.invocation_count(left) + report.invocation_count(right), 1);

// AFTER — crates/phenix-graph/tests/patch_output_routing.rs
let fixture = Fixture::new(assembled.graph).unwrap();
let source = fixture.output("source", "value");
let left = fixture.input("left", "run", "value");
let right = fixture.input("right", "run", "value");
fixture.fire("go", vec![Value::local(3_i64)]).unwrap()
    .expect_success()
    .expect_deliveries_from(source, &[left])
    .expect_invocations(fixture.node_id("left"), 1)
    .expect_invocations(fixture.node_id("right"), 0);
assert_ne!(left, right, "distinct physical input slots");

// BEFORE — crates/phenix-graph/src/lib.rs (WaveState::publish_via)
ReportRetention::TerminalOutputs => !self.graph.by_output.contains_key(&from),

// AFTER — crates/phenix-graph/src/lib.rs (WaveState::publish_via)
ReportRetention::TerminalOutputs => !delivered_outputs.contains(&from),
```

## New ordinary Rust testing usage

```rust
let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
fixture.fire("go", vec![Value::local(4_i64)]).unwrap()
    .expect_success()
    .expect_deliveries_from(
        fixture.output("source", "value"),
        &[fixture.input("left", "run", "value")],
    );
```

`expect_deliveries_from` checks the complete multiset, not just "at least one"
or a guessed graph reachability path. It works with `ReportRetention::MetadataOnly`
because edge metadata is retained independently of payloads.

## Responsibility/ownership boundary

- `OutputSelector` owns the **decision** to select zero or more compiler-owned edge IDs.
- `WaveState::publish_via` validates **the entire selection** and prepares all selected values.
- `deliver_batch` checks buffer capacity/schema and commits the **whole** incoming batch, or none.
- `WaveReport` receives `DeliveryEvent`s **only after** that commit; it never feeds scheduler readiness decisions.
- `ReportRetention::TerminalOutputs` keeps emitted payloads only when they reached no downstream edge in that wave.
- `PatchBuilder` validates routing and connection footprints with the same parser; `GraphBuilder` remains responsible for actual endpoint resolution and editing.

## Tradeoffs, verification, and still-open work

New delivery telemetry is O(committed-edge count) per reported wave, even in
metadata-only mode. This is intentionally *metadata only*, not another value
copy or delivery queue. High-fan-out benchmark coverage should be added before
considering the reporting overhead negligible.

Rust tests added in `tests/iteration_five.rs` cover seven real execution cases
and the shared endpoint grammar; the previous patch-routing test is strengthened.

**Not yet verified by a Rust compiler:** `cargo fmt`, `cargo check`, `cargo test`
and Clippy. No toolchain is installed in the current environment. Source review,
static reference inspection and archive integrity cannot replace a passing Rust
suite, and scores must not be promoted on that basis.

Next graph-first work: compile and repair the Rust suite; replace remaining
string errors with structured stage-specific diagnostics where behavior benefits;
then review the patch/graph edit boundary and explicit identity for executable
actions/policies. Native plugin and kernel integration stay last.
