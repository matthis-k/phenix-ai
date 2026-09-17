# Development

The flake owns the development toolchain, maintenance commands, package checks, and shipped Harness products. Repository maintenance is declared in `modules/development.nix` through `phenix-flake-ci`. Local development, git hooks, and CI use the same generated command implementations.

## Development shell

```sh
nix develop
```

## Canonical commands

Apply deterministic mechanical fixes:

```sh
maintenance fix
```

Run the complete read-only validation graph:

```sh
maintenance all
```

Stable validation boundaries are:

| Command | Boundary |
| --- | --- |
| `maintenance check source` | Formatting, source analysis, workflow synchronization, and test-target classification |
| `maintenance check rust` | Rust static analysis and compile/type-check gate |
| `maintenance test unit` | In-crate tests |
| `maintenance test doc` | Rust documentation tests |
| `maintenance test integration` | Crate and API integration targets |
| `maintenance test system` | Black-box Harness, process, and protocol targets |
| `maintenance test product` | Realized Harness and packaged-product behavior |

The current leaf commands and CI layout come from `modules/development.nix`. Use the generated command help instead of copying leaf names into documentation.

## Test ownership

The application descriptor snapshot comes from typed declarations in `phenix-application-interface`. Regenerate it with:

```sh
cargo run --manifest-path rust/Cargo.toml --locked -p phenix-application-interface \
  --bin phenix-application-descriptor > share/phenix/interfaces/phenix.application@1.json
cargo run --manifest-path rust/Cargo.toml --locked -p phenix-application-interface \
  --bin phenix-application-descriptor -- --rust share/phenix/interfaces/phenix.application@1.json \
  > rust/crates/phenix-application-interface/fixtures/application.rs
```

Source validation checks the descriptor. The crate's unit suite compiles the generated Rust fixture, compares every generated type schema, and exercises typed requests against replaceable application transports.

Each behavior has one canonical test layer. Prove it at the highest practical deterministic boundary.

### Unit and documentation

Use unit tests for pure or local invariants. Keep documentation tests separate so failures remain attributable.

### Integration

Use integration tests for public crate APIs or behavior spanning several internal components without requiring the complete process boundary.

Every Cargo integration-test target must be explicitly classified under the integration or system maintenance boundary. Source validation checks that classification against Cargo metadata.

### System

Use system tests for externally observable Rust runtime behavior across real supported binaries or process/protocol boundaries. Prefer deterministic reusable fixtures and no external credentials.

### Product

Use product tests only for behavior introduced by application composition, packaging, wrappers, or installed artifacts. Keep adapter-specific boundary tests separate when they exercise a real adapter contract.

Frontend product behavior belongs in the frontend repository. Product derivations do not rerun Rust unit, integration, or system suites.

## Nix testing rule

Do not duplicate ordinary Nix configuration into assertions. When a package, wrapper, application, or system is misconfigured, the build or run that consumes the configuration is usually the useful failure boundary.

Test Nix expressions directly when the expression itself contains reusable program logic such as composition, transformation, ordering, or precedence.

## CI and generated workflow

`modules/development.nix` is authoritative for maintenance behavior and CI presentation. `.github/workflows/ci.yml` is a generated projection and must remain synchronized with that declaration.

Do not document generated job topology in prose. It changes with the declaration and is checked mechanically.

## Pre-commit

The development shell installs the shared maintenance pre-commit integration. It runs deterministic normalization on the staged paths. Outside the shell, the generated hook may enter `nix develop` to run the same maintenance command.

Compiler errors, lint findings that require judgment, test failures, runtime failures, and Nix build failures are never auto-repaired.

## Stitch

Inspect the repository workspace graph with:

```sh
stitch workspace discover --json
```
