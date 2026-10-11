# S2 consistent epoch snapshots

Status: frozen before the snapshot mechanism
Date: 2026-10-11
Extends: `m1-live-state-contract.md`

The S1b state request opcodes remain unchanged. A readonly ABI 3 host import
`phenix.host.state@1/snapshot` uses an empty slot and a v2 list of distinct refs
sorted by full location. It accepts between one and `MAX_COUNT` locations.

Before reading any snapshot, the same state authority boundary checks each ref's
owner/schema and the caller's `get` operation and property resource grants,
including root and every ancestor ceiling. Denial returns a typed
`ValueAccessError` with no partial result. Invalid order, duplicates or malformed
inputs return a kernel schema failure.

Success is `[current_epoch, [snapshot, ...]]`, in location order, captured under
one cell-store borrow. Each snapshot retains its last-change epoch and revision.
The enclosing epoch is the consistent read point, even when cells last changed
in different epochs. The import cannot mutate state or call plugin code. It is
part of the same authenticated host callback, not a raw trusted cell API.

The independent native fixture must observe a prior scalar update followed by a
two-cell batch, then read both at one epoch and verify matching batch epochs.
Denied multi-read and unsorted/duplicate requests must fail before partial data
is returned. Existing M0 and S1b wire profiles remain frozen.
