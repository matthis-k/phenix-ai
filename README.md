# Phenix clean graph rewrite

**Experimental branch.** The full P1 implementation and all removed plugin sources are retained at [reference/pre-clean-plugins-20261026](https://github.com/matthis-k/phenix-ai/tree/reference/pre-clean-plugins-20261026) (`31f86f454fd4cd3316379b5802b02ba19ecae69c`). Working P1 launchers remain on `main`. This branch is not a release.

Target: [graph architecture v2.4](spec/graph-architecture-v2.4.md). Start with typed native graph execution and small plugins, then build a behaviorally equivalent P1 and P2 domains. No graph-v1 plugin compatibility requirement.

- [Architecture and domain graph layout](docs/clean-graph-plan.md)
- [Implementation and verification ledger](docs/clean-rewrite-progress.md)

Initial Cargo workspace contains only kernel/ABI/SDK/contracts and two minimal test plugins. Legacy Nix product wiring is intentionally pending replacement; do not deploy this branch to Neovim.
