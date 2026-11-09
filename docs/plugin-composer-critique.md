# Plugin composer design — Phase 2 adversarial critique

**Reviewer:** same assistant in a separate pass; independence criterion NOT met. Revisions 1 and 2 were rejected, revision 3 was conditionally accepted, revisions 4 and 5 re-reviewed after real-source ergonomics revealed dynamic metadata and Rust inference constraints. Phase 4 must still accept or reject the actual diff.

## Rejections and revisions

1. Embedding records directly into runtime composer and deferring validation until `Kernel::build` weakened registration-time validation. **Fix:** private registrar entries remain; register whole bundle early.
2. Keeping public Graph `Registrar::add(Contribution)` alongside `.register` created parallel dialects. **Fix:** only the latter is valid Graph authoring; generic non-Graph Registrar remains independent.
3. A new Graph registrar extension trait or public builder would duplicate an existing concept. **Fix:** reuse the `Contribution` name and `Registrar`.
4. Multiple inserts from `.register` could partially succeed before later duplicate rejection. **Fix:** owner-validated atomic merge of existing Contributions representation.
5. Singleton `.route`/`.after` failed dynamic real use cases. **Fix:** iterable declarations; no second bulk API.
6. Generic `.route<Item: Into<String>>` made a typical `vec!["tick".into()]` ambiguous. **Fix:** `Item=String` (or one-element array of explicit Strings).

## Reject-list audit

| Reject criterion | Disposition |
| --- | --- |
| New abstraction without invariant | Existing `Contribution`; registrar merge enforces failure-atomic bundle identity |
| Mirrored conceptual structures | Offer/import remain semantic private records, not public authoring types |
| Wrapper hiding a flawed model | Correlation keys removed from consumer API instead of masked |
| Duplicated primitives | Reuses existing registry, Kernel resolution, Graph contracts and host staging |
| Harder common consumer code | Trivial `.new(...).register`; dynamic `.route(vec)` supported |
| Runtime work resolvable earlier | Derive identities at registration, resolve dependencies before composition |
| Special cases instead of shared cause | One composer-ownership rule covers nodes, modules and multiple offers |
| Large migration for one beneficiary | Every Graph composer improves; quantify all touched sites at Phase 4 |
| Two dialects | Direct public Graph registration removed, generic plugin contributions remain separate |
| Complexity without subtraction | Deletes public standalone offer/requirement structures and hand-authored links |
| Foreseeable failure without tradeoff | Multi-offer, foreign origins, portable consumer, self-import, reload and retirement are hard test gates |

## Falsifiers and tradeoffs

- Multiple offers from one composer must resolve by explicit host selection and change execution; no implicit ambiguous fallback.
- Wrong-composer and foreign unit advertisement must fail final origin checks, even with matching contract structure.
- Third-party portable definition and independently compiled consumer must continue working; no common provider Rust type required.
- A failed Nth member of a registration bundle must leave no composer, offer or import behind, even if the caller catches its error.
- Self-import may compose its own unit and then bind; late replacement must not inherit old provenance.
- Existing loaded revisions and old explicit engine handles must survive reload; retired producers must stop admitting new notices.
- Generic type-specific `PluginSnapshot::bind` must remain possible for a normal custom contribution's graph locator, although the removed `GraphOffer` record itself is no longer publicly typable.

**Accepted by protocol default, contingent on tests:** source-breaking in-tree migration on the clean branch, no native ABI/sandbox or external-effect rollback introduced, and a caught failed bundle registers nothing. No default acceptance for broken Graph checks, host policy, portable types, lifetime safety or primary consumers.

## Phase 3/4 gates

Search for every old constructor, public import, manual producer/consumer link and obsolete README. Require zero old Graph authoring usage with explicit exception inventory. Run `cargo fmt --all --check`, workspace tests with and without default features, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `nix flake check --no-update-lock-file`, and `git diff --check` on the final exact commit. Review compiler errors, all affected call sites, multi-offer falsifier and accepted limitations before publication. Without those evidence results, keep the branch draft, not Phase 4 accepted.
