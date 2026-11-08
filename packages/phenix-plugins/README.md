# Plugin Graph Binding

Plugin contributions remain typed, in-process values retained by exact-revision
`PluginSnapshot` and `ContributionHandle` leases. Native Rust registration is not
a dynamic-library ABI, sandbox or authorization grant.

Graph-facing contributions can implement the optional locator-only method:

```rust,ignore
impl phenix_plugin::Contribution for MyOffer {
    const KIND: &'static str = "app.graph";
    fn id(&self) -> &str { &self.id }
    fn graph_unit(&self) -> Option<&str> { Some(&self.unit_path) }
}
```

Use `snapshot.bind::<MyOffer>(id, &requested_contract, &compiled_graph)` to obtain
a leased contribution after validation. Binding resolves the contribution's
actual unit through `CompiledGraph::find` and delegates to the graph-owned
contract checker. It also requires the actual unit's opaque composition origin
to match this exact plugin revision. Missing origins, locators/paths, and absent
or incompatible claims fail closed. The registrar stores no copy of a purported
implementation shape.

Trusted hosts must compose provider declarations inside
`graph.with_origin(snapshot.graph_origin(), |graph| { /* compose */ })`.
`ContributionHandle::graph_origin()` exposes the same leased revision token.
Tokens are allocation identities, not plugin names or numeric revision counters:
another plugin set or an aborted candidate cannot alias them. Merely inspecting
or claiming an existing unit inside that scope does not change its origin.
Successful replacements record the replacing scope; unscoped replacements lose
the old origin. Pending layout changes preserve the original handler's origin
while Graph independently revalidates its final interface.

Module origins identify the actual instantiation/replacement boundary, not all
code inside an imported definition. Nested declarations retain their own origins
through packaging. Composite roots and unscoped declarations have no attributable
origin and cannot be bound by this API; offer a named node or module instance.
The token is trusted-host provenance, not proof of native handler behavior or
authorization. It does not itself lease executable plugin code.

The former copied-bounds metadata integration is removed. Construct expectations
with `GraphContract::new(id, InterfaceShape)`,
attach explicit graph claims during composition, compile, then bind against that
immutable final graph. Matching shape alone is not semantic membership.

The returned handle pins plugin code/state, not the graph or a transferable
validation certificate. Do not treat binding against one graph as validation of
another graph, infer authorization from it, or assume it proves a native closure
uses the offered unit. Keep handles/snapshots alive alongside executable graphs.
The kernel package provides generic definition/offer/requirement contributions
for hosts that need cross-plugin final-target validation without importing the
provider's implementation type.

Staging and commit remain explicit host operations. `bind` does not commit a
candidate, switch generations or coordinate activation. Existing handles stay
pinned after reload and logical unload.
