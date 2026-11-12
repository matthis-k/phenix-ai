# A7 — bounded identifier admission

Phase 0/1, baseline `285ddde6f09e012cf60e398be2e0083e44e671b3`.

## Problem

Most graph/compiler work is already bounded by recursion depth, instance count
and work budget, and contract identity strings have a 512-byte bound.
But port and process names, composition edit paths, and plugin-contribution
keys could be arbitrarily long. They become keys in multiple BTreeMaps,
diagnostic strings, physical path joins and compiled interface identities.
This permits disproportionate memory and comparison work before a compiled
work limit triggers.

The requirement is a **single predictable per-identifier byte limit**, not
an arbitrary global path cutoff: composed paths join many independently
bounded identifiers, subject to `CompileLimits::max_depth`.

## Phase 1 comparison

- No change: unknown memory amplification from untrusted names.
- Truncate or hash names: destroys unique identity and provenance; reject.
- Global path length limit: conflicts with legitimate bounded nested modules.
- **Use the existing 512-byte contract identity budget as
  `MAX_IDENTIFIER_BYTES`** and validate process/port/composition segments
  plus plugin owner and contribution IDs at the earliest owning boundary.

The limit is byte-based (UTF-8) and performs no allocation. Contract
identities retain their stricter existing lexical grammar. Public graph
process names, ports and plugin identifiers otherwise keep their pre-existing
grammars; valid names are not normalized/truncated.

## Phase 4

Test exact boundary and next-byte rejection for interface ports, names and
contract identity; verify existing graph composition, typed authoring, nested
module and plugin lifecycle tests. Exact-head CI required before close.
