# Native Component Binding

Plugin contributions are typed in-process values retained by exact-revision
`PluginSnapshot` and `ContributionHandle` leases. Registration is neither a dynamic
library ABI, a sandbox nor a write grant. Catalog, plugin key, staging, batch commit,
revision, failure-context and lease APIs remain independent of network execution.

Component-facing contributions implement this optional locator:

```rust,ignore
impl phenix_plugin::Contribution for MyOffer {
    const KIND: &'static str = "app.network";
    fn id(&self) -> &str { &self.id }
    fn component_path(&self) -> Option<&str> { Some(&self.path) }
}
```

`snapshot.bind::<MyOffer>(id, &contract, &compiled_network)` accepts canonical
`phenix_graph::contract::Contract` and `network::CompiledNetwork`. It resolves the
actual component with `compiled.inspect(path)`, verifies its opaque origin belongs
to the exact leased revision, and invokes
`compiled.check_implementation(path, &contract)`. Missing locators, paths, origins,
claims and incompatible contracts fail closed. No copied implementation shape is
stored or trusted. Binding errors retain plugin/contribution/phase context and
the typed native error source.

Trusted hosts supply `GraphOrigin` through native composer `Registration` envelopes
or `ComponentDef::with_origin`. `PluginSnapshot::graph_origin()` and
`ContributionHandle::graph_origin()` expose the exact revision token. Kernel assigns
a distinct child to each producing contribution. Allocation identity, not a plugin
name or numeric revision, prevents sibling sets and abandoned candidates from
aliasing provenance. This token is not itself a permission or executable-code lease.

Primitive and composite boundaries use the same compiled validation. Matching shape
alone does not create semantic membership: definitions explicitly claim canonical
contracts. Native local ports use Rust type identity; wire ports use complete schema
identity and may be implemented by independently compiled Rust types. Descriptor
equality does not establish codec honesty.

The returned handle pins plugin code/state, not a network or a transferable
validation certificate. Revalidate each final network after composition. Raw native
hosts must attach a snapshot/code lease with `CompiledNetwork::with_lease` before
exposing executable declarations. Kernel does this automatically, including
candidate-context compiled views and any application-provided lease. Detached
compiled clones, factories, processes, sessions, jobs and queued values retain it.

Constructor failures retain their originating factory. Contribution failures retain
partial registered contributions, the implementation, and its factory in that drop
order; binding failures retain the exact loaded revision. The escaping error owns
the lease, so inventory/catalog teardown cannot invalidate its methods or destructor.
`CallbackError` remains a boxed error. `lease_callback_error(error, lease)` lets host
adapters apply the same rule: its private wrapper drops the original cause before
the lease, delegates formatting, and preserves the concrete error in the source
chain. Downcast by walking that chain rather than relying on a fixed source depth.

Staging and commit remain explicit host operations, and existing leased handles
survive reload and logical unload. Retained errors can intentionally extend code
lifetimes beyond candidate rejection or logical unload.
