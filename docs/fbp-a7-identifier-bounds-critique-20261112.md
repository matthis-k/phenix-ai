# A7 — adversarial size-limit review

1. Must reject the raw identifier **before** inserting into graph maps; do not
   simply limit rendered diagnostic strings.
2. Do not change the identity equality/normalization or alias grammar.
3. A composed physical path may exceed 512 bytes by joining segments;
   reject *each segment*, not the entire qualified path.
4. `PluginSnapshot::keys()` can provide contribution IDs from plugin code.
   Reject the ID before cloning it into Kernel maps; reject owner before
   dependency discovery. `Key::new` itself is a pure identity constructor,
   not a fallible trust boundary.
5. Do not impose a 512-byte limit on serialized schema *structures*, which
   can legitimately be large and are governed by a different future quota.
6. Existing `CompileLimits` cap expansion but do not replace name bounds.
7. A 512-byte bound is a deliberately breaking acceptance policy under
   latest-only; do not leave legacy oversized-name shims.

Accepted for Phase 3 with edge regression and full exact-head test evidence.
