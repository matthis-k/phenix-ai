# Phenix M1 kernel

Phenix provides a small native plugin kernel with typed contracts, explicit provider selection and inherited authority. M1 is its only supported contract, value wire format and runtime profile. Native plugins use ABI 3. Definition-only packages carry the same canonical contract metadata without executable code.

The kernel verifies packages offline, prepares one immutable composition and dispatches selected native or cell operations through one authority boundary. Pending imports use the existing parent/root ticket tree. Cancellation revokes effects before cleanup, and physical leases retain executable images until work drains. Roots remain pinned to their admitted generation during promotion, rollback and retirement.

Rust plugins declare contracts and implement handlers. The SDK generates metadata, serialization, ABI and lifecycle glue. Stateless handlers need no authored State:

```rust
#[phenix_sdk::contract(id = "example.text@1")]
trait Text {
    fn uppercase(&self, text: String) -> Result<String, String>;
}

#[phenix_sdk::plugin(id = "example.uppercase", define(Text), provide(Text, slot = "text"))]
mod plugin {
    use super::*;
    #[provide(Text::uppercase)]
    fn uppercase(text: String) -> String {
        text.to_uppercase()
    }
}
```

Use `nix flake check -L` for the complete M1 conformance suite, `nix run .#phenix-m1-demo` for the packaged provider/consumer example and `nix build .#phenix-m1-release` for the evidence product. SDK, fixtures and inspection tools remain separate from the production kernel closure.

[The transition contract](spec/m1-only-runtime-contract.md),
[verified audit](spec/m1-consolidation-audit.md) and [roadmap](spec/m1-roadmap.md)
record scope and acceptance. M0 acceptance documents retain historical evidence.
Their authority, cancellation, ordering, generation and retirement guarantees now
have M1 tests. The transition passed exact-head CI at `367420e33b`. S3b completion
follows in [PR #781](https://github.com/matthis-k/phenix-ai/pull/781), with its own
acceptance gate.

Reactive G1-G4 development follows consolidation and S3b acceptance. The synchronous A0 agent remains an ordinary plugin fixture, exercised by `nix run .#phenix-a0-demo`. The final Basic agent will compose triggered graph nodes through the kernel's existing execution machinery.
