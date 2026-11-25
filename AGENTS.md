# Agent instructions

## Before changing code

- [Glossary](GLOSSARY.md): read when naming concepts, designing interfaces, or discussing domain behavior. Use its terms in code and conversation. Check proposed definitions against the code and resolve contradictions before editing the glossary.
- [Foundation specification](docs/fbp-design.md): read when changing library boundaries, graph semantics, host authority, lifecycle, or agent composition. Update the affected requirement when intent changes.
- [CI operations](docs/infrastructure.md): read when changing Nix checks, dependencies, or GitHub workflows.

## Documentation

- Put API contracts, invariants, examples, and local rationale in the owning Rust code. Prefer doctests and regression tests for examples and guarantees.
- Keep durable requirements in the foundation specification and domain definitions in the glossary. Read existing material before adding a file. Replace obsolete text and link to Git history for historical context.
- Put proposals, alternatives, and reviews in PR discussions, remaining tasks in issues, and verification logs and benchmark samples in CI artifacts. Do not commit progress ledgers or acceptance diaries.
- Write for lookup: use the canonical term, exact symbol, short sentences, and descriptive links. Give each document one purpose. Keep task-specific detail behind a pointer. Store facts that require judgment, not facts an agent can read from source or configuration.
- Use an architecture decision record only for a costly, surprising choice between real alternatives. Keep the central requirement authoritative and link to the decision.

Before completing a change, check every affected contract against its source or test. Remove superseded references and duplicate descriptions. Run the applicable checks from the flake; report any you could not run in the PR.

Writing references: [writing-for-agents](https://github.com/mattpocock/skills/tree/main/skills/productivity/writing-for-agents), [domain-modeling](https://github.com/mattpocock/skills/tree/main/skills/engineering/domain-modeling), and [technical-writing](https://github.com/cursor/plugins/tree/main/pstack/skills/technical-writing).
