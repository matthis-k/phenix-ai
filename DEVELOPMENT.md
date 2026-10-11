# Development and validation

Enter `nix develop`. Run `nix flake check -L` for acceptance. All dependencies are pinned by `flake.lock` and the two Cargo lockfiles.

| Command | Behavior |
| --- | --- |
| `nix run .#phenix-m0-demo` | Invoke the Nix-built consumer and provider through the CLI |
| `nix build .#phenix-m0-conformance` | Materialize all four checks and their evidence |
| `nix build .#checks.x86_64-linux.m0-contract` | Formatting, clippy, Rust semantics, compile-fail cases and C ABI layout |
| `nix build .#checks.x86_64-linux.m0-descriptor` | Release/linker/strip retention and descriptor identity |
| `nix build .#checks.x86_64-linux.m0-native` | Real native artifacts, resolution, authority and failure behavior |
| `nix build .#checks.x86_64-linux.m0-product` | Trusted immutable deployment and CLI JSON |
| `nix fmt -- flake.nix` | Format the Nix definition |

Kernel, artifact tooling and public SDK have separate source inputs. Fixture builds start from the SDK distribution and their own workspace. They cannot resolve private kernel crates because those files are absent. Nix emits package records after ELF fixups. Logical bindings and grants live in explicit deployment data consumed by the kernel.

Inspection accepts a deployment document without loading its artifacts. Invocation adds a canonical binary request. The CLI accepts immutable Nix store packages. The trusted Rust control-plane API also accepts caller-attested read-only sources; mutable files cannot provide a safe validation-to-load boundary.

A caught invocation panic poisons that instance. A new process can construct a fresh instance. Abort, signals and process termination provide no cleanup or recovery guarantee. Native code shares the process and its operating-system privileges.

The CI workflow checks out the exact branch head even for PR events. It stores `kernel-commit.txt`, `ci-run.txt`, Nix paths and parsed native/descriptor/product JSON as an artifact. Report that run before claiming milestone acceptance.

The complete suite has fourteen gates. Added gates are m02-imports, m03-generations, m04-public-sdk and m04-shipping. `nix build .#phenix-m0-release` collects all realized outputs. The default package contains the kernel; source distributions and native fixtures remain separate products.
