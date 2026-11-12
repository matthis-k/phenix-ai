# H2 — Host event callback panic containment (Phase 0–1)

Baseline: `89e00d06498979a2f231dfc8ca780eba44140d27` on `impl/graph-consolidation-20261110`. This is a
scoped follow-up to the H2 item in `fbp-issue-ledger-20261111.md`.

## Phase 0 — problem and baseline alternative

`Kernel::drain` catches producer polling panics and converts them to
per-producer failures; `run_until` also contains wake-registration panics.
However, the host-provided `on_event` callback runs without containment and
can unwind past the driver's public `Result<(), DriverError>` boundary.

Doing nothing leaves hosts to install their own unwind guards. That remains
reasonable for a one-off direct `drain` consumer, but is inconsistent with
the bounded `run_until` driver. No new callback trait, runtime scheduler,
capability, or generic panic infrastructure is required.

## Phase 1 — smallest change

- Add `DriverError::EventCallbackPanicked { producer: Key }` to report the
  producer whose event was being delivered.
- Catch unwind only around the event callback invocation, after `drain(1)`
  has released the producer drain lock. No callback runs under inventory or
  session locks.
- Retain ordinary event ordering. A failed delivery terminates this driver
  invocation immediately; there is no automatic replay of a partly consumed
  event or mutation rollback.
- Preserve normal typed producer errors, wake registration, session state,
  and the existing synchronous callback signature.
- Add a native regression: a producer reports a terminal error, the host
  callback panics, `run_until` returns the typed error, and subsequent
  producer/session access is still possible.

## Accepted limitations

Rust `panic=abort` cannot be caught. Host callback side effects cannot be
rolled back, and an event already passed into the callback cannot safely be
replayed after an unwind. This change does not promise that arbitrary
externally held locks survive a callback panic. It deliberately leaves direct
`Kernel::drain` consumers responsible for their own event handlers.
These boundaries do not violate the FBP baseline's delivery semantics.
