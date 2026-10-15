# Post-P1 CI cutover: accepted release evidence

Updated: 2026-10-15

## Status and ownership

P1 is accepted on `main` at `68f46f6379ff129b5bb9f2bad27229269aec0110`.
The previous implementation is preserved on `oldmain` at `1dd0ffc8ba882d46c4e6c6d5a8cee328fa8f8063`.

C4 was promoted by [PR #890](https://github.com/matthis-k/phenix-ai/pull/890) in merge commit `b7017042e08fef52e39c9ec5635b8711beb8d5d2`. Its first production `main` run passed. The CI-first feature freeze is lifted.

The authoritative release backend is the pinned `phenix-flake-ci` Nix DAG with four Tailscale `ssh-ng` builders, two build slots per worker, and an exact-head shipping-evidence aggregate. It executes on trusted `main` pushes and manual dispatch. Pull requests use the separate credential-free full-inventory Nix matrix. Rust formatting, official ACP SDK interop, and the generated-policy verifier run as independent checks. Previous canonical and DAG workflows remain manual rollback paths.

The Nix flake owns check inventory. The pinned CI library owns scheduling, worker lifecycle, cache behavior and evidence verification. Neither consumer YAML nor runtime plugins implement a second scheduler.

## Verified evidence

| Gate | Result | Evidence |
| --- | --- | --- |
| P1 release | Accepted | `main` `68f46f63`; [release acceptance](p1-release-acceptance.md) |
| Tests-first CI policy | Accepted | RED PRs #877, #880, #882, #884, #887, #889; consolidated implementation #890 |
| Controlled 4-worker cold/warm shipping | Passed, 38/38 checks in each phase | [run 38604657851](https://github.com/matthis-k/phenix-ai/actions/runs/38604657851) |
| Cold Nix build | 595.1 seconds | Same controlled run |
| Warm Nix build | 3.3 seconds, exact-key cache restore | Same controlled run |
| Cold/warm full pool wall | 11.33 / 1.85 minutes | Same controlled run; not just Nix build time |
| Cold/warm runner usage | 52.80 / 4.58 runner-minutes | Same controlled run |
| Consolidated PR exact-head gates | All passed at `023820ba` | [matrix](https://github.com/matthis-k/phenix-ai/actions/runs/38621712013), [ACP](https://github.com/matthis-k/phenix-ai/actions/runs/38621711289), [format](https://github.com/matthis-k/phenix-ai/actions/runs/38621711367), [generated-policy](https://github.com/matthis-k/phenix-ai/actions/runs/38621711087) |
| First promoted `main` remote shipping | Passed at `b7017042` | [run 38674211157](https://github.com/matthis-k/phenix-ai/actions/runs/38674211157) |
| First promoted `main` ACP / format / policy | Passed at `b7017042` | [ACP](https://github.com/matthis-k/phenix-ai/actions/runs/38674210679), [format](https://github.com/matthis-k/phenix-ai/actions/runs/38674210747), [policy](https://github.com/matthis-k/phenix-ai/actions/runs/38674210625) |

C4 is accepted based on production exact-head evidence, not solely a generated YAML test. The cold/warm measurements apply to the tested source and cache state; they do not predict every PR's elapsed time.

## Security and follow-up

The privileged workflow admits automatic pushes only on `main`, plus manual dispatch. It has no PR trigger and uses pinned upstream workflow revisions. Untrusted PR builds have read-only repository permissions and no Tailscale/OIDC write access. Source-impact pruning must preserve a complete fallback for unknown or shared inputs. Keep negative authority and ABI conformance tests in the full check inventory.

GitHub branch protection is not a prerequisite for running CI or merging PRs. The repository reported `main.protected=false` and no rulesets at cutover. Requiring the four stable PR check names through a ruleset is optional administrative hardening, not a feature-development blocker.

## Next work

1. Finish accepted P1 client behavior. [#886](https://github.com/matthis-k/phenix-ai/pull/886) merged deterministic session-history verification in `b96182d5`. [#892](https://github.com/matthis-k/phenix-ai/pull/892) covers composition-derived ACP model display and durable accepted-user-message projection; merge only after exact-head checks pass.
2. Implement authorized runtime management from [#891](https://github.com/matthis-k/phenix-ai/issues/891): model discovery and selection, revision-checked changes, session operations and replayable subscriptions. Runtime remains authoritative; Lua only bridges authenticated client operations.
3. Follow the planned multi-client session projection and routed I/O design in [#852](https://github.com/matthis-k/phenix-ai/pull/852). Keep per-client projection and transport separate from kernel execution and session truth.
4. Continue S5/F3 declarative reactive hooks, K1-K3 kernel management and diagnostics, logging and introspection plugins, and the later Advanced composition according to their existing dependencies. Evaluate maintained crates before implementing protocol or transport logic.
5. Improve PR selectivity and cache costs only after proving output parity and fail-closed full-inventory fallback. Do not replace accepted shipping evidence with fast checks.

This document owns C4 metrics and acceptance evidence. [The M1 roadmap](m1-roadmap.md) owns stage order and current status.
