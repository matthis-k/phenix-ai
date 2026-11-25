# Phenix foundation

Phenix composes independently usable libraries into an extensible agent runtime. This glossary fixes the language shared by the libraries and their consumers.

## Language

### Graph

**Component definition**:
A reusable unit of work with a declared interface. Its body is a primitive or a composite.
_Avoid_: Node template, operation package

**Primitive**:
A component whose body describes a process rather than a nested network.
_Avoid_: Leaf module, action

**Composite**:
A component whose body is a network with a public interface.
_Avoid_: Subgraph wrapper, executable module

**Network**:
A set of component instances, connections, public ports, and explicitly started sources.
_Avoid_: Wave graph, engine

**Compiled network**:
An immutable executable interpretation of a network definition.
_Avoid_: Host generation, deployment

**Process**:
A stateful executable instance of a primitive in a network session.
_Avoid_: Component definition, node factory

**Activation**:
One unit of work for a process, which can span multiple cooperative steps.
_Avoid_: Wave, step (when referring to the entire activation)

**Source**:
A graph process that starts without an incoming packet.
_Avoid_: Host producer

**Capture**:
An observation of an exported graph output.
_Avoid_: Tool result, callback (unless that is the application meaning)

**Network session**:
One execution of a compiled network with its own process state and transport.
_Avoid_: Agent conversation, host generation

**Request scope**:
The identity and lifetime of one admitted unit of demand within a network session.
_Avoid_: Network session, correlation payload

### Composition and hosting

**Patch**:
An owned declaration of changes to a network.
_Avoid_: Registration, plugin

**Registration**:
A host-approved patch and its identity, dependencies, priority, provenance, and grants.
_Avoid_: Contribution

**Composition**:
The selected registrations and the network they produce.
_Avoid_: Plugin inventory, runtime

**Contract**:
An explicit semantic obligation over a component interface.
_Avoid_: Schema, structural compatibility

**Claim**:
A component's declaration that it fulfills a contract.
_Avoid_: Shape match, grant

**Grant**:
Host-issued authority for an exact operation or resource.
_Avoid_: Claim, request, provenance

**Contribution**:
A named value provided by a plugin revision for host consumption.
_Avoid_: Registration, plugin instance

**Offer**:
A contribution's declaration of a component available for a contract.

**Import**:
A contribution's declared need for a component satisfying a contract.
_Avoid_: Connection, automatic wiring

**Host producer**:
An application-owned source of events for admission to a network session.
_Avoid_: Graph source, process

**Host generation**:
A published kernel and its selected plugin revisions.
_Avoid_: Compiled network, network session

**Lease**:
An ownership relationship that keeps a required code, plugin, or resource owner alive.
_Avoid_: Grant, revocation token

### Storage

**Resource identity**:
The stable identity of a logical resource, independent of where or how it is stored.
_Avoid_: Database path, Git remote

**Storage session**:
An authorized view of a storage registry.
_Avoid_: Network session

**Invalidation**:
A notice that previously read storage data may need to be read again.
_Avoid_: Change log, row event

**Derived value**:
A value computed from authorized storage snapshots.
_Avoid_: Stored value, independent reactive runtime

### Agent application

**Agent loop**:
The composed exchange of model output, tool results, and subsequent model input until completion.
_Avoid_: Scheduler, single agent process

**Agent harness**:
The application composition that hosts an agent loop and its supporting capabilities.
_Avoid_: Kernel, graph runtime

**Tool environment**:
The application context in which a tool executes.
_Avoid_: Network session, plugin host
