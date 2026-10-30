# P1 release gates: independently owned evidence

The broad `basic-host-smoke` check previously invoked provider-management,
model selection and model-routing restart scenarios. Those scenarios had their
own fixtures, but **their execution and evidence were coupled to Basic smoke**.
A provider-specific test failure therefore obscured whether terminal/JSONL/
ordinary ACP session tests had failed.

These three scenarios now have separate Nix check outputs:

| Check | Evidence | Responsibility |
| --- | --- | --- |
| `basic-acp-provider-management` | `acp-provider-management.json` | Provider management, capability and auth-status ACP visibility |
| `p1-native-model-selection` | `native-model-acceptance.json` | Selectable concrete models, CAS, HTTP execution, revocation, concurrent clients |
| `p1-native-routing-restart` | `native-routing-restart.json` | Fail-closed recovery of invalid durable selection and witness state |
| `basic-host-smoke` | terminal, JSONL, observer, ACP and storage evidence | Default Basic frontend operation only |

`p1-release-preflight` explicitly receives provider management, independent model selection,
and restart derivation outputs, retaining every previous semantic assertion
including native CAS, revocation, session reload, and denied corrupt routing
records. Its self-test now mutates those *separate* evidence directories to
prove that missing or forged evidence still fails the release gate.

This is a movement of **test ownership only**, not a model-selection runtime
change. It does not validate real external credentials and does not introduce
a second production provider or Model contract.

The remaining combined API-key/OAuth cross-connection integration check stays
unchanged until its release evidence can likewise be split without losing
cross-connection grant checks.
