# P0 and M0.0 acceptance matrix

Branch `rewrite/minimal-kernel` starts at `11cdf0eca7e80965d0dfca3762d1fd65bb19e10f`. The GitHub root commit has no parents. Its merge base with observed main `1dd0ffc8ba882d46c4e6c6d5a8cee328fa8f8063` is that same root, 1,215 commits behind. Existing branches and PRs remain references.

Statuses: implemented means code exists; tested means the named semantic check passed; unproven means acceptance evidence is still absent. This file owns the task matrix. CI evidence records own exact artifact identities.

| Task | Files | Acceptance criterion | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| Root-based branch | Git ancestry | Root has no parents and is ancestor of main | Tested | Local rev-list and GitHub compare agree |
| Contract freeze | spec/m0-contracts.md, include/phenix.h | Concrete values, descriptor, ABI, deployment and inspection | Implemented | Review against executable tests |
| P0 ELF extraction | contract/descriptor.rs, kernel/package.rs | Extract without execution, canonical payload, SHA-256 | Tested locally | Rust 1.99 release, thin LTO, symbols stripped; Nix proof pending |
| P0 retention matrix | flake.nix, tests/descriptor.py | Nix GNU and LLVM linkers, LTO off/thin/fat, stripping | Implemented | Canonical Nix check pending |
| Public typed SDK | crates/phenix-sdk, crates/phenix-macros | Annotations generate imports, exports and ABI glue | Tested locally | Separate examples release build passes |
| Canonical wire | contract/value.rs | RFC bytes, bounded decoding, ordering and malformed input | Tested locally | Unit test passes; expand vectors |
| Offline package validation | kernel/package.rs | Hash final artifact, verify extracted bytes, reject tampering | Partial | Extraction exists; package preparation next |
| Native loader | kernel/native.rs | Header negotiation, ownership and instance teardown | Not started | Depends on P0 proof |
| Resolver | kernel/resolve.rs | Explicit selection, missing/ambiguous/schema errors before load | Not started | Depends on offline packages |
| Scoped dispatcher | kernel/runtime.rs | Root and import share dispatch, authenticated scope, deny before effect | Not started | Depends on loader and resolver |
| Trusted CLI / inspection | kernel/bin/phenix-kernel.rs | Typed response and parsed versioned JSON | Not started | Depends on runtime |
| External plugin pair | examples/* | Public SDK only, separate workspace and lockfile | Tested locally | Native artifacts compile; actual cross-plugin call pending |
| Native conformance | tests/native.rs or product driver | Calls, domain errors, denied counter, panic, thread/cycle rejection | Not started | Actual artifacts required |
| Nix packaging | flake.nix | Distinct kernel/plugins, final package records, closure | Implemented | Real builds running |
| Exact-head CI | .github/workflows/minimal-kernel.yml | Run canonical checks on recorded branch SHA | Not started | Push after first coherent gate |

Dependency order: accept P0, complete offline package preparation and resolver, load/invoke, prove denial and ownership, assemble product closure, run exact-head CI. M0.1 onward stays deferred.
