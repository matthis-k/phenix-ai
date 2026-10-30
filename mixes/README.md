# Production and domain testing mixes

Agent composition and testing selections are independent from the kernel.

## User packages

`mixes/production/basic.nix` and `advanced.nix` validate the native
production compositions assembled by `flake.nix`. Both use
`production/native-only.nix` to reject known test-only artifact packages.
These modules do not silently register plugins or add authority, slots or
dependencies. User packages `phenix-basic`, `phenix-advanced` and the
public Rust SDK are unaffected by test-domain selection.

## Test domains

Each test domain has its own module under `mixes/testing/`.

| Module | Mix | Explicit selection |
| --- | --- | --- |
| `model.nix` | `modelHistory` | Add a test Model and replace exactly one selected slot |
| `providers.nix` | `providerLoopback` | Select the loopback artifact and management entry |
| `graph.nix` | `graph` | Native graph conformance check |
| `storage.nix` | `storage` | Storage, project and membership checks |
| `frontend.nix` | `frontend` | Terminal/JSONL/ACP product checks |
| `advanced.nix` | `advanced` | Advanced adapter, checkpoints and evidence checks |
| `python-poc.nix` | `pythonPoc` | Source-only, standalone experimental Python adapter |

The latter four native-check domains are **named selections of existing
flake checks**, not new plugin compositions or a parallel test executor.
The Python proof of concept remains only the trivial public
`examples/python-trivial/greeter.py` and internal fixtures, outside the
production Rust SDK source and either agent product.

`nix build .#phenix-domain-test-mixes` publishes selected domain/check
names and composition paths. `nix build .#phenix-python-poc-source`
provides the separate Python experiment source.

`nix build .#checks.x86_64-linux.agent-mix-boundaries` asserts the
selected artifacts, unchanged product grants/authority, correct test-domain
catalog, SDK isolation, and Basic/Advanced Nix closures. The existing
cache-isolation workflow mutates only the model test mix for its case H and
still checks that changing a test composition does not rebuild Basic.

These boundaries concern packaging and explicit selection. Native ABI,
graph authorization, lifecycle and security acceptance remain the kernel's
own independent conformance gates.
