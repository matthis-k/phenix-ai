# Agent mix boundary

Mixes select verified plugin artifacts, slots and grants. A testing configuration
must be an **explicit domain mix**, not an incidental addition to Basic or
Advanced. Production mixes are defined in `flake.nix` and checked by
`production.basic` and `production.advanced` from `default.nix`.

The `testing` constructors preserve production grants and authority:
- `modelHistory` changes exactly the selected Model/Upstream binding and adds
  one history-test provider artifact.
- `providerLoopback` changes the provider-management entry and substitutes
  its sole loopback fixture artifact.
- The Graph V2 Python proof of concept remains a standalone test domain. It is
  **not** a Basic/Advanced plugin, a default composition dependency, or part
  of the public Rust SDK package.

`nix build .#phenix-domain-test-mixes` exposes the domain selection manifest.
`nix build .#phenix-python-poc-source` explicitly retrieves experimental
Python test sources. The default `phenix-basic` and `phenix-advanced`
presets contain neither of these experimental packages.

`nix build .#checks.x86_64-linux.agent-mix-boundaries` validates the
compositions, exact fixture deltas, independent test-domain catalog, public SDK
source isolation, and the Nix closures of both shipping agent binaries.

These are packaging and composition guarantees; native `.so` plugin safety
and runtime authority remain enforced by the existing kernel and ABI gates.
