# Post-P1 CI cutover — acceptance plan

## Order and freeze

**P1 is accepted** on main SHA `68f46f6379ff129b5bb9f2bad27229269aec0110`
after all four post-integration push workflows succeeded. The pre-rewrite
main is preserved as `oldmain` at
`1dd0ffc8ba882d46c4e6c6d5a8cee328fa8f8063`.

The next roadmap priority is **CI cutover, then product code**. Do not
start K1–K3, logging, introspection, ACP v2, advanced-agent, or multi-client
feature work ahead of the verified CI transition. A CI regression is not
a reason to silently weaken P1 shipping, ABI/security or product coverage.

## Existing building blocks — do not rewrite

- `phenix-flake-ci` [#17](https://github.com/matthis-k/phenix-flake-ci/pull/17):
  exact-head evaluated Nix `checks.<system>` inventory, deterministic
  matrix, shared read-only Nix store, fail-closed report aggregation
- [#18](https://github.com/matthis-k/phenix-flake-ci/pull/18):
  build DAG from real Nix derivation dependencies; stage dependent shipping
  roots only after prerequisites
- [#19](https://github.com/matthis-k/phenix-flake-ci/pull/19):
  authenticated, ephemeral Tailscale `ssh-ng` Nix builders
- [#20](https://github.com/matthis-k/phenix-flake-ci/pull/20):
  declarative backend selector, pinned reusable caller, trusted push/manual
  dispatch, **4 builders × 2 build slots** by default
- Consumer policy: `ci/flake-check-config.nix`,
  `ci/flake-backend.nix`, `ci/render-flake-workflow.nix`.
  The flake's evaluated check set is the sole authoritative inventory;
  do not copy it into a handwritten YAML check list.

Use Nix's scheduler, the official GitHub Actions reusable workflows,
and existing Cargo/Rust/Nix tools. Do not build a new graph scheduler,
SSH protocol, cache database or bespoke configuration framework.

## P1 reference measurements

The final P1 candidate (`68f46f63`) passed these workflows in parallel:

| Workflow | Reference wall time | Status |
| --- | ---: | --- |
| Independent official ACP SDK | 7.7 min | Passed |
| Generated Nix DAG | 19.1 min | Passed |
| Serial canonical conformance | 27.1 min | Passed |
| Flake matrix | 33.2 min | Passed |

These reference times are **not** evidence that the new remote backend is
faster: compare contemporary runs of the same check inventory on the
same input revision and distinguish cold store, warm store, GitHub queue
time, cache transfer and total billed runner minutes.

## Release sequence

1. **C0 — tests first.** [Phenix #877](https://github.com/matthis-k/phenix-ai/pull/877)
   introduces a regression gate for remote trust, exact pins, protected
   release coverage and 4×2 configuration, with deliberate red baseline.
2. **C1 — stage remote builders.** [Phenix #878](https://github.com/matthis-k/phenix-ai/pull/878)
   enables the same-repository push branch
   `ci/m1-p1-ci-cutover` with the reviewed, immutable
   `phenix-flake-ci` library. No remote `pull_request` event and no
   remote `main` push before acceptance.
3. **C2 — equivalent outputs and security.** Require on a real exact-head
   run: successful four-builder startup, authenticated `ssh-ng` stores,
   one complete evaluated inventory, exact-head and DAG digest matching,
   every Nix check including `p1-release-preflight`, `m1-shipping`,
   `basic-host-smoke`, `basic-pending-http-product`, full native
   ABI/import and security negatives, and clean final release evidence.
   Deny missing, duplicate, stale or spoofed reports. Keep official ACP
   SDK interoperability independent.
4. **C3 — cold/warm and trust benchmarks.** Measure at least one cold
   and one warm 4-worker run against the current canonical, matrix and
   staged-DAG workflows on equivalent inputs. Confirm cache identity
   includes `flake.lock`, Cargo lockfiles and relevant strategy changes;
   untrusted PRs must never write a privileged cache or receive
   Tailscale credentials. Review tailnet tags/grants and unnecessary
   secret exposure, remote runner minutes, network transfer and failures.
5. **C4 — promote a single authority.** Trusted `main`/release builds
   use the one accepted generated flake-CI backend. Untrusted PRs use
   a credential-free, full-inventory matrix or DAG runner. Preserve a
   cheap `cargo fmt --check` (including external examples) and the
   independent official ACP SDK check. Make the complete evidence
   aggregate required before retiring duplicate serial and candidate
   workflows. Keep a manual, pinned fallback until the first accepted
   main push run passes.
6. **C5 — only then resume product code.** Revisit K1–K3, observability
   plugins, Advanced agent and multi-client frontend work in normal
   roadmap order.

**No P1 gate may be lost just to improve wall time.** FC2 selective
source-impact planning remains optional and must fall back to a full
check inventory for unknown changes. A trusted 4-worker backend may
be slower on tiny PR changes; the PR mode can use the proven faster
credential-free backend without reducing coverage.

## Promotion evidence ledger

| Gate | Status |
| --- | --- |
| P1 integrated on main, preserved oldmain | ACCEPTED |
| Safe staged remote workflow policy | Implementing / CI gate pending |
| Exact-head 4-worker Nix inventory and shipping | Pending remote run |
| Same-head parity and adverse-evidence negatives | Pending |
| Cold/warm cache and runtime comparison | Pending |
| Trusted-only remote credentials and cache policy | Pending final audit |
| Authority switch and duplicate-workflow retirement | Not started |
| First authoritative CI on new main | Not started |

Update this ledger with real workflow run links and actual metrics. Do not
mark the cutover accepted based on a passing generator-only test.
