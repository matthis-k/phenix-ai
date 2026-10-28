# Development and validation

Enter `nix develop`. Dependencies are pinned by `flake.lock` and the two Cargo
lockfiles. M1 is the sole supported contract, value wire format and native ABI.

| Command | Behavior |
| --- | --- |
| `nix flake check -L` | Run all contemporary protocol and semantic gates |
| `nix run .#phenix-m1-demo` | Invoke packaged external provider and consumer through the CLI |
| `nix build .#phenix-m1-release` | Collect conformance, public SDK and shipping evidence |
| `nix build .#checks.x86_64-linux.m1-contract` | Formatting, warnings-denied Clippy, Rust semantics and compile failures |
| `nix build .#checks.x86_64-linux.m1-native-authoring` | Generated native contracts, fields, typed imports and pending property control |
| `nix build .#checks.x86_64-linux.m1-imports` | Pending child authentication, cancellation, attenuation and adversarial ABI commands |
| `nix fmt -- flake.nix` | Format the Nix definition |

Kernel, artifact tooling and public SDK have separate source inputs. External
fixtures build from the public SDK distribution with private kernel crates
absent. Nix writes package records after ELF fixups. Composition supplies explicit
named provider selections and operation/resource grants.

The CLI inspects a composition without loading native artifacts. Invocation adds
a canonical binary request. CLI packages must live in the immutable Nix store.
The trusted Rust control API also accepts caller-attested read-only sources.
Mutable files cannot provide a safe validation-to-load boundary.

A caught invocation panic poisons that instance. Poll/cancel remain available to
drain physical work. Native code shares the process and its OS privileges.
Physical leases retain executable images until work drains; process termination
provides no cleanup guarantee.

CI checks out the exact PR head. Its artifact records `kernel-commit.txt`,
`ci-run.txt`, realized Nix paths and the full conformance/release reports. Bind
acceptance to that commit and evidence. Historical M0 documents describe retired
profiles; the M1 matrices own current behavior and remaining stage gates.

The pull-request full-inventory gate uses the single-store `flake-local-dag`
backend from `phenix-flake-ci`: one exact-head plan and native Nix DAG, with
trusted-main cache restore read-only and no PR publisher or remote credentials.
Keep its reusable workflow and `library_ref` pinned together. The trusted-main
remote-builder workflow and independent ACP, formatting and policy checks remain
separate. Use focused local checks while iterating; the complete exact-head CI
evidence, not an interrupted local build or a different commit's pass, governs
integration. A warm-cache improvement does not establish cold-build latency.

## Basic history verification

The packaged products have three distinct levels:

- `phenix-kernel`: the M1 kernel without Basic.
- `phenix-basic`: the packaged Basic runtime and its fixed plugin composition, with `terminal`, `jsonl` and `acp` entry modes. Existing `phenix-basic-terminal`, `phenix-basic-jsonl` and `phenix-basic-acp` wrappers remain available.
- `phenix-basic-verification`: the same Basic runtime and providers, with one extra test-only Model plugin selected for deterministic transcript checks.

The dedicated `Basic deterministic session history` CI workflow checks formatting, runs the packaged model and requires the fixed evidence list. Run `nix build .#checks.x86_64-linux.basic-history-verification` to verify user and assistant history, a real tool call/result, session isolation and history after restarting the process. For manual use, set `PHENIX_BASIC_SQLITE_PATH` to a writable database path and run `nix run .#phenix-basic --` for the normal Basic session or `nix run .#phenix-basic-verification -- terminal` / `-- jsonl` for the deterministic model. The commands `remember:key=value`, `recall:key`, `verify-assistant:key`, `tools:hello`, and `recall-tool` exercise the Model history contract.

The test plugin has no state. It derives every response from the Model request. It is never selected by the normal Basic packages. Compaction, system-message configuration and non-text content are outside this verification package.

The next composition preset will be `phenix-advanced`. It does not ship in P1. Its configuration, providers and agent behavior remain separate from the Basic and verification presets.
