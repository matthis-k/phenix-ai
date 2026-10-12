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
