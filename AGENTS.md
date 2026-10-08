# Phenix AI repository instructions

Treat executable code and deterministic tests as authoritative. `README.md` owns current architecture and subsystem boundaries. `DEVELOPMENT.md` owns the human-facing development and validation surface. `config/phenix/runtime.nix` owns supported product configuration. Skill-provider plugins own their skill packages; `phenix-plugin-basic-skills` owns the default static set.

Fix or remove documentation that disagrees with the current tree in the same change.

## Change discipline

- Remove superseded APIs, aliases, and compatibility paths instead of maintaining parallel versions.
- Prefer an existing platform or library abstraction when it expresses the required semantics.
- Keep semantic names even when using a library type internally.
- Make invalid states unrepresentable. Prefer enums, newtypes, non-zero types, ownership, and constructors over correlated strings, booleans, options, or later checks.
- Parse, don't validate. Convert external, configuration, and wire inputs into invariant-bearing types at the boundary. Reserve later checks for facts that depend on runtime state.
- Use guard clauses and early returns for errors and exceptional cases. Keep the success path short, shallow, and linear.
- Derive state where possible. Remove intermediate representations and branches that do not carry required behavior.
- Preserve typed errors at subsystem boundaries. Do not collapse actionable failures into generic exit states.
- Keep implementation-specific invariants next to the code and tests that enforce them instead of creating speculative documents.
- Keep plugin dependencies explicit. A plugin may depend on another plugin for a real service dependency, but must not load unrelated first-party implementations.

Before finishing an implementation, ask whether new code can be deleted, an existing abstraction can replace it, branches can be merged, stored state can be derived, intermediate representations can be removed, or custom error handling can become ordinary propagation.

## Commit candidates

- Stage only the intended candidate diff, then use `scripts/prepare-commit.sh "message"` when working in a Git checkout. It normalizes the candidate, rejects unstaged tracked edits and unrelated formatter changes, stages only candidate paths, and commits once.
- The generated pre-commit hook remains a fallback for ordinary `git commit`. Do not bypass required checks.
- CI is read-only. Never push an automated formatting or dependency-update commit from a PR workflow. Fix rejected candidates before committing or pushing.
- GitHub API contents commits do not execute Git hooks. If the agent has no checkout or normalization tools, do not claim its API commit was preformatted; rely on required read-only checks and use the manual maintenance patch workflow to propose fixes.

## Rust implementation discipline

Use Canonical's Rust best practices as the primary external Rust reference. Use `mre/idiomatic-rust` as an index to primary guidance. Repository architecture and established local conventions take precedence.

- Use exhaustive matching when new variants or fields should force compiler errors at decision points.
- Keep foreign serialization shapes at the boundary. Parse them into internal domain types.
- Prefer standard conversion traits when their semantics fit: `From`, `TryFrom`, `FromStr`, `AsRef`, and their blanket conversions.
- Model semantic modes and state machines with enums or state-specific types. Use booleans only for genuinely binary facts.
- Use newtypes for identifiers, units, validated values, and other concepts that should not be interchangeable.
- Keep values immutable by default. Scope mutability narrowly and prefer returning values over mutation used only to move data between branches.
- Keep production code panic-free for user, configuration, network, persistence, and runtime failures. Prefer invariant-bearing types, `?`, and typed errors. Use `expect` only for programmer-proven invariants and state the invariant in the message.
- Treat file length as a cohesion signal. Around 250 lines warrants review. Around 500 lines strongly favors splitting by responsibility unless one module is clearly easier to understand.
- Keep generic bounds and lifetimes no broader than required.
- Prefer borrowed inputs for views and owned outputs when ownership transfers. Do not clone only to satisfy an avoidable ownership shape.
- Prefer iterators when they make ownership, filtering, or propagation clearer. Use loops when explicit control flow reads better.
- Keep helper and boundary-only types in the narrowest useful scope.
- Follow the Rust API Guidelines for public APIs where applicable.

## Testing discipline

Test behavior, not declarations.

- Prove behavior at the highest practical deterministic boundary. Use unit tests for local invariants, integration tests for crate or API boundaries, system tests for process and protocol behavior, and product tests for realized packaging behavior.
- Prefer reusable deterministic fixtures over ad hoc mocks when behavior crosses runtime boundaries.
- Do not mirror Nix options, package selections, file declarations, or literal configuration values into tests merely to assert that the source says the same thing twice.
- Test Nix expressions directly when the expression itself contains reusable program logic such as composition, transformation, ordering, or precedence.
- Product checks should build, start, execute, or otherwise exercise realized outputs.
- Keep each behavior in one canonical execution layer. Product derivations must not rerun Cargo behavioral suites.
- Frontend-specific product tests belong in the frontend repository.
- Add focused regressions for behavioral fixes and integration coverage for cross-boundary behavior.

## Verification

Use the focused maintenance boundary while iterating and run `maintenance all` before final handoff. The current command surface and test ownership rules are documented in `DEVELOPMENT.md` and declared in `modules/development.nix`.

Do not weaken a check to make transitional code pass. Fix the implementation or remove the obsolete surface the check protected.
