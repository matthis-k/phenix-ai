# Agent instructions

## Documentation discipline

Document durable knowledge, not the process of acquiring it.

- Maintain one authoritative location for each contract, requirement and architectural decision. Use [the central foundation specification](docs/fbp-design.md) for architecture and requirements.
- Put API contracts, invariants, usage examples and non-obvious implementation rationale in Rustdoc or comments on the owning code. Prefer executable doctests and regression tests to prose descriptions of behavior.
- Update or replace existing documentation rather than accumulating revisions. Remove superseded information and obsolete references. Git preserves history.
- Create a new documentation file only when it has a distinct, enduring responsibility that cannot reasonably be covered by an existing document or the code. Explain that responsibility in the PR.
- Keep design exploration, alternatives, critique, implementation reports and review findings in PR descriptions/discussions or temporary working files outside the committed tree. Preserve the design and review process, not its intermediate paperwork.
- Track outstanding work in issues and verification in CI. Keep raw benchmark samples and logs in CI artifacts. Do not commit rolling issue ledgers, subjective completion percentages, dated acceptance diaries or CI transcripts.
- Change the central specification only when durable requirements or architecture change; implementation progress alone is not a specification change.
- During review, check that changed code has accurate documentation, examples compile, obsolete documentation is removed, and no new duplicate authority or broken reference is introduced.
- Documentation is not a substitute for implementation or verification. Do not use code-to-documentation ratios or line quotas as quality targets.

### Development outputs

| Phase | Persistent output |
| --- | --- |
| Requirements | Central specification changes, only when needed |
| Design and critique | PR description and discussion |
| Implementation | Code, Rustdoc and tests |
| Final review | PR review |
| Verification | CI results and artifacts |
