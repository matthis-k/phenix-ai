# H3b — adversarial critique of same-thread lock reentry guard

Status: separate Phase 2 challenge to the H3b design, before code changes.

1. **A global Boolean is wrong.** Two independent Kernels (or separate
   session and drain locks in one Kernel) must not be treated as the same
   lock. Track the identity of each actual `Mutex` per thread, not just
   whether any callback is active.
2. **A `try_lock` replacement is wrong.** A legal, unrelated thread may
   hold the lock. Preserve blocking mutex acquisition for other threads.
3. **A guard dropped too early is wrong.** Keep it alive until the corresponding
   `MutexGuard` has been released, including panics. Ensure explicit drop
   ordering when a drain gate is released before producer destructors.
4. **An unconditional `drain` retry is wrong.** Recursive poll/drain or
   wake-registration/drain must not reenter and retrieve another producer
   or block behind itself. The current `Vec` return type has no error
   channel; make the unsupported recursion fail fast and rely only on the
   established callback panic boundaries. Never pretend the call did nothing.
5. **A caught panic does not certify plugin state.** Continue to retire the
   failing producer after a panicked poll; wake-registration failure retains
   ownership but does not promise the plugin is safe to retry.
6. **`ConnectionError::Cancelled` is a lie for recursion.** Use an
   unambiguous diagnostic and return the unchanged `Value`.
7. **State tracking must not outlive the call.** Unwind-safe RAII cleanup
   is mandatory. No thread-local pointer can remain after the lock has
   been released and may be reallocated.
8. **Only same-thread recursion is solved.** Lock inversions involving
   callbacks waiting on other threads, host publication and arbitrary
   plugin-controlled locks remain open. Mark H3b **partial**, not fully
   resolved. Do not change the whole-drain locking boundary.

Phase 2 verdict: conditionally accepted for this private guard with tests
covering recursive session/drain/wake, normal independent kernels and exact
head CI. If it requires a second scheduler or weakens retirement's whole
drain guarantee, return to Phase 1.

## Phase 2 extension — publish/retire must share the lock policy

The first implementation guarded `Kernel::drain` but not
`KernelHost::publish`, whose *same* gate can be reentered by a callback.
A guard covering only one acquisition path is insufficient.

Accepted correction: wrap publication's acquisition of the retiring
generation's exact gate in the same thread-local identity scope. On
same-thread reentry, return `LifecycleError::ReentrantDrain` **before**
`PluginSet::commit_batch`, inventory mutation or epoch advancement.
This must not use `try_lock` and reject unrelated threads.

A test holds that exact gate and scope to model a running poll callback,
attempts publication on the same thread, checks typed failure and unchanged
generation, releases the gate, then publishes normally. Cross-thread
publisher waiting for an in-flight drain remains an existing guaranteed
behavior.

Unresolved explicitly: `KernelHost::Drop` cannot return an error and
must not be assumed safe to invoke from a callback holding the same gate;
other multi-thread callback wait cycles remain outside this mitigation.
