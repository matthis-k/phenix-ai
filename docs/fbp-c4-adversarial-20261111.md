# Adversarial critique — C4 scoped preparation binding

Status: Phase 2 challenge, written after the Phase 1 note and before code changes.

1. **Does a renamed field solve confidentiality?** No. The field must be private, with no alternate public `compiled()` or unrestricted `inspect(path)` exposed on CandidateContext. An independently possessed compiled graph remains inspectable; this is a specific *capability-passing* improvement, not a process sandbox.
2. **Could an import token from a different consumer escape the scope?** If the new binding method accepts a caller-provided `ResolvedImport`, yes. Require an import *ID* and resolve it from `self.imports` before calling the existing authoritative graph binder.
3. **Could forged contract metadata bypass shape checks?** Do not construct a replacement contract or path. Reuse the exact `ResolvedImport` and `CompiledNetwork::bind` combination. Existing wrong direction, wrong type, and undeclared-import failures must remain.
4. **Does this break primary authoring cases?** Audit native consumers and examples for `context.compiled`; existing declared-import projection maps directly to `bind_import`. A plugin deliberately relying on arbitrary topology enumeration will lose that implicit read privilege. Such callers require explicit future host-granted inspection, not a hidden global fallback.
5. **Could binding make preparation mutable or introduce a lease bug?** The result borrows the same candidate compiled network; no session, process factory, or writable builder is exposed. Returned lifetime must not outlive the callback's compiled view.
6. **Evidence gaps:** A runtime rejection test alone is insufficient: include compile-fail enforcement of inaccessible `compiled` and existing typed projection regressions. Verify exact final-head CI; no success claim from the design note alone.

Phase 2 verdict: conditionally acceptable for implementation without a new abstraction. Reject if any ordinary Prepare call still receives the full compiled network through this API, if an arbitrary import token may be supplied to the binding method, or if native consumers cannot migrate without broader authority.


## Phase 2 revision — challenge host-granted inspection

The first implementation failed the established detached compiled export test
and the portable consumer fixture. Therefore the previous conditional acceptance
**does not apply to the incomplete implementation**.

The revised design passes Phase 2 only if:

1. The full inspection grant is a host-controlled exact `Key { owner, id }`,
   defaulting to empty. No plugin registration, `ResolvedImport`, or patch
   payload may turn it on.
2. `CandidateContext.compiled` remains private, and the inspection accessor
   returns an error for ungranted consumers. No unguarded alternative is added.
3. Scoped `bind_import` uses the current context's import map, never a
   caller-supplied import token.
4. Authorized `inspect_network` returns the same leased immutable compiled
   view previously used by the code-leases test; cloning/exporting it still pins
   both plugin and application leases.
5. The independently compiled portable consumer uses `bind_import` and
   preserves all type, direction, contract, and wire tests.
6. The host policy does not get conflated with `CompositionPolicy.grants`,
   which are expressly **write** grants.
7. The exact code head passes strict rustfmt, Clippy, local/portable regression
   suites and remote CI. No architectural-completion claim before that evidence.

Verdict: conditionally accepted for Phase 3 under these gates. This is a
deliberate least-privilege API with an explicit host escape hatch, *not* a
confidentiality guarantee against native Rust code in the same process.
