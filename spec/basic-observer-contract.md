# Durable Basic observer checkpoints

B2 owns this plugin-only extension. The existing EventJournal and BasicOutput
contracts add `observer` and `checkpoint_observer` operations. Both use the
authenticated session resource. The trusted frontend host selects a stable
observer identity; observer names separate replay offsets within that session.
They do not authenticate clients or grant access to another session.

`ObserverRequest` contains session and observer identity. `ObserverCheckpoint`
contains those identities, the last delivered sequence in `after`, and the
Storage CAS revision. An absent checkpoint returns offset and revision zero.
The journal persists a versioned record per session and observer through its
selected Storage provider. BasicOutput forwards these operations through its
selected Journal import. The kernel gains no journal or observer behavior.

Checkpoint updates require the current revision and a monotonic offset no
greater than the journal tail. A stale revision returns Conflict. Invalid names,
backward offsets and future offsets reject without writing. The provider
validates persisted identities, revision and sequence bounds on every load.
Corrupt records return StorageFailed. Failed CAS never advances the cursor.

Terminal and JSONL restore their own named checkpoint on process startup. The
host writes and flushes each rendered page before acknowledging its last
sequence. A failed write or flush leaves that page unacknowledged. A crash after
delivery and before checkpoint commit may replay a page. A crash after commit
resumes after that page. These are at-least-once delivery semantics; writing to
an arbitrary output stream and committing SQLite cannot be one transaction.

ACP `session/load` explicitly replays session history. Its connection offsets
remain transient until a protocol-specific observer identity is selected.

Acceptance uses separately packaged Journal and SQLite providers in independent
processes. It proves two persistent observers, stale revision rejection,
monotonic and tail bounds, cross-session denial, corrupt-record rejection,
and process termination before and after acknowledgement. Product restart tests
prove that Terminal and JSONL checkpoints remain independent. Full B2 still
requires storage migrations and the remaining transaction fault cases.
