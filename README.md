# Phenix AI

Phenix is a fully customizable coding agent built from plugins. Plugins define what the agent does; the M1 kernel defines how contributions execute. The generic runtime follows from that separation rather than being a separate product goal. [The product decision](spec/agent-product-decision.md) records the rationale, while the [M1 roadmap](spec/m1-roadmap.md) and [Advanced roadmap](spec/p2-advanced-agent-roadmap.md) own implementation requirements.

The native kernel has typed contracts, explicit provider selection and inherited authority. M1 is its only supported contract, value wire format and runtime profile. Native plugins use ABI 3. Definition-only packages carry the same canonical contract metadata without executable code.

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
have M1 tests. The transition passed exact-head CI at `367420e33b`.
[PR #781](https://github.com/matthis-k/phenix-ai/pull/781) completes S3b at
`75a255b9`, with full CI and verified release evidence.

## Opt-in P1 HTTP model

`phenix-basic-http-acp` runs the accepted Basic agent with the existing
`phenix.model.http` provider instead of the scripted model. The ordinary
`phenix-basic` package and its deterministic checks do not change. The HTTP
provider accepts OpenAI-compatible streaming chat-completions responses and
uses the same Basic tools and durable SQLite journal.

Configure the executable with environment variables in the shell that starts
the frontend. Keep secrets out of flake configuration and the Nix store:

```sh
export OPENAI_API_KEY="..."  # or PHENIX_MODEL_HTTP_API_KEY
export PHENIX_MODEL_HTTP_MODEL="gpt-4.1-mini"
# Optional, for another compatible provider:
# export PHENIX_MODEL_HTTP_ENDPOINT="https://provider.example/v1/chat/completions"
nix run .#phenix-basic-http -- acp
```

`phenix-basic-http-acp` is the named ACP launcher for a Neovim client.
It defaults to the OpenAI chat-completions endpoint, chooses the model from
`PHENIX_MODEL_HTTP_MODEL`, and stores its P1 SQLite state under
`$XDG_STATE_HOME/phenix/basic-http.sqlite` unless
`PHENIX_BASIC_SQLITE_PATH` overrides it. The transport reads a credential
only at execution. `OPENAI_API_KEY` is an alias supported by the launcher.

The `basic-http-product` check drives the installed Basic HTTP composition
against a local streaming HTTP fixture. That deterministic check has no
external network calls or paid model dependency. Live-provider behavior and
billing are external and must be tested explicitly.

P1 Basic is an accepted composed agent product. The Advanced agent remains under development through independently replaceable P2 plugins; neither agent is embedded in the kernel. The M1 and P2 roadmaps own current status and acceptance evidence.
