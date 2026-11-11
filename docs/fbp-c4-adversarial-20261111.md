# Adversarial critique — C4 scoped preparation binding

Status: Phase 2 challenge, written after the Phase 1 note and before code changes.

1. **Does a renamed field solve confidentiality?** No. The field must be private, with no alternate public `compiled()` or unrestricted `inspect(path)` exposed on CandidateContext. An independently possessed compiled graph remains inspectable; this is a specific *capability-passing* improvement, not a process sandbox.
2. **Could an import token from a different consumer escape the scope?** If the new binding method accepts a caller-provided `ResolvedImport`, yes. Require an import *ID* and resolve it from `self.imports` before calling the existing authoritative graph binder.
3. **Could forged contract metadata bypass shape checks?** Do not construct a replacement contract or path. Reuse the exact `ResolvedImport` and `CompiledNetwork::bind` combination. Existing wrong direction, wrong type, and undeclared-import failures must remain.
4. **Does this break primary authoring cases?** Audit native consumers and examples for `context.compiled`; existing declared-import projection maps directly to `bind_import`. A plugin deliberately relying on arbitrary topology enumeration will lose that implicit read privilege. Such callers require explicit future host-granted inspection, not a hidden global fallback.
5. **Could binding make preparation mutable or introduce a lease bug?** The result borrows the same candidate compiled network; no session, process factory, or writable builder is exposed. Returned lifetime must not outlive the callback's compiled view.
6. **Evidence gaps:** A runtime rejection test alone is insufficient: include compile-fail enforcement of inaccessible `compiled` and existing typed projection regressions. Verify exact final-head CI; no success claim from the design note alone.

Phase 2 verdict: conditionally acceptable for implementation without a new abstraction. Reject if any ordinary Prepare call still receives the full compiled network through this API, if an arbitrary import token may be supplied to the binding method, or if native consumers cannot migrate without broader authority.
