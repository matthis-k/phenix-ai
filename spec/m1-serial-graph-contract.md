# G3 serial graph execution

Base: PR #785 at `164f82595b7bad930650e2c062ac579344f71403`.
Owners: `phenix-kernel/src/graph.rs`, the external graph examples and
`phenix-conformance/src/bin/phenix-m1-graph.rs`.

## Admission and authority

Action admission validates the discovered action, payload and root requirements
before creating a root. Every node uses the existing dispatcher, its declaring
owner's prepared grant and the admitted root's authority and resource.
Property access requires both an operation grant and its property resource.
The native fixture must grant these resources; narrowed scopes must still deny
effects. Native and property permissions keep their existing meaning.

## Active joins and failures

Only matching triggers activate nodes. Ordering uses the active projection;
dormant predecessors neither execute nor block a join. All active predecessors
must succeed before their successor runs.

A typed domain outcome follows the failed node's declared policy:

| Policy | Result |
| --- | --- |
| `FailRoot` | Report the domain result and stop the remaining execution |
| `Skip` | Report the domain result, suppress dependent nodes transitively and continue independent nodes |

Authorization, schema, infrastructure and other kernel failures always stop
the root and return the original failure. Skip never hides these failures.
Suppressed nodes perform zero native or state effects. Earlier committed
writes remain committed; serial graph execution is not a transaction.

## Host output observation

A trusted host may supply a synchronous observer to scoped action submission.
The kernel emits admission, node result, suppressed-node and terminal updates.
Every update carries the admitted root identity. Admission also names the
action, event, compiled graph and resource. Updates borrow results during
delivery. The kernel stores no journal or output history.

Delivery happens on the admission thread while the existing root stays live.
The observer cannot admit a second root. A frontend owns rendering or buffering;
Basic output contracts and durable replay remain plugin responsibilities.
This API observes serial execution only. It does not claim model streaming,
diagnostic subscription, cancellation, generation switching or G4 acceptance.

## Acceptance

The independent native gate must prove committed-state fan-in, per-action root
correlation, results observed before later nodes execute, a terminal update on
domain and kernel failures, denied operation/resource scopes, skip suppression
and continued independent work. Offline tests retain dormant-predecessor and
ordering-edge activation checks. Full exact-head Nix shipping CI remains required.
