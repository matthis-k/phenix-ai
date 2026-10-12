# Definition-only event and action contributions

Base: G3 serial execution at `672b7f09`, PR #786.
Owners: artifact package verification, definition macro, existing resolver and
the external graph fixture.

Definition-only packages may declare events and actions through the canonical
GraphContribution encoding. The current package record gains an optional graph
file and its SHA-256 digest. Verification checks the digest, canonical bytes and
matching package owner before preparation. Package identity includes graph
bytes when present. Native package verification retains its ELF graph section.

Definition-only metadata has no executable imports, exports or properties.
Its graph has no executable nodes. Native plugins consume its event and action
declarations and contribute independently authorized nodes. A definitions
plugin uses `graph(GRAPH)` and exposes generated graph metadata bytes, with no
native table. The existing resolver combines verified declarations once.

The native graph gate moves the shared event and action into an independently
built definitions package. It proves discovery, activation and state-backed
execution through that package. Digest tampering, re-signed owner mismatch and
attempted executable nodes reject before any constructor runs.
