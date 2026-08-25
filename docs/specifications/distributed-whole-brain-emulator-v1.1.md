**NEURALMIMICRY ENGINEERING SPECIFICATION**

Distributed Event-Driven  
Whole-Brain Emulator

Critical multidisciplinary revision covering superdense logical time, deterministic computation, adaptive sharding, multi-brain federation, resilient recovery, authorised management and workstation sensory/actuator streams

**CRITICAL REVIEW BASELINE • VERSION 1.1**

| **Document attribute** | **Value**                                                                                              |
|------------------------|--------------------------------------------------------------------------------------------------------|
| **Prepared for**       | NeuralMimicry distributed emulator programme                                                           |
| **Primary audience**   | JetBrains IDE with Codex; Rust, platform, UI, QA and operations engineers                              |
| **Date**               | 20 August 2026                                                                                         |
| **Status**             | Corrected requirements baseline following neuroscience and HPC engineering review                      |
| **Language**           | British professional English                                                                           |
| **Code scope**         | Attached Rust emulator, transport and UI sources plus associated protocol, API, storage and test files |

**Purpose.** This document is an executable engineering contract. Codex shall use it to plan, implement, test, document and verify the migration without weakening any stated invariant.

# Document control and use

## Executive directive

This specification defines the required migration from layer-redundant, loosely timed distributed execution to a biologically event-driven, causally coherent, resilient and multi-tenant whole-brain emulation platform. It is written as an implementation contract for Codex operating inside JetBrains IDE. Every use of **shall** is mandatory; **should** expresses a preferred design that may be varied only with a recorded architectural decision; **may** is optional.

This revision has been subjected to two independent critical readings and a combined reconciliation: one from the standpoint of a neuroscientist concerned with biological meaning, timing, plasticity, sensory transduction and behavioural output; the other from the standpoint of an HPC software engineer concerned with conservative distributed progress, determinism, throughput, heterogeneous placement, fault tolerance, security and operability. Where those perspectives conflict, this document distinguishes model semantics from execution policy instead of allowing resource pressure to change biology silently.

> **NON-NEGOTIABLE:** The implementation shall preserve the biological event graph while introducing only the minimum artificial coordination needed for causal correctness, bounded settling, recovery and observation. It shall not introduce a whole-network convergence barrier at every biological timestamp.

The work shall be delivered as small, reviewable, backward-compatible increments. Codex shall inspect the full repository before editing, identify generated files and protocol build steps, preserve unrelated user changes, and run the relevant formatters, lints, unit tests, integration tests and documentation checks at every phase gate.

## Approved decisions

The following decisions are locked for this implementation baseline.

| **ID** | **Approved decision**                                                                                                                                                                                         | **Consequence**                                                                                                                                                                                                                                 |
|--------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| D-001  | Use superdense logical time (t, μ) with integer biological ticks and causal microsteps.                                                                                                                       | Same-biological-time cascades settle without abusing wall-clock time or floating-point timestamps.                                                                                                                                              |
| D-002  | Anchor coherence to a precisely defined, single-owner SynapticTransition, using energise/discharge as the project term and release/conductance change as the scientific interpretation selected by the model. | Presynaptic spike departure, axonal arrival, release outcome, postsynaptic conductance and plasticity updates are not conflated; every transition is calculated once and consumed at the same logical tag.                                      |
| D-003  | Use explicit per-stream watermarks for safe horizons and component-scoped distributed termination detection for cyclic same-tick work.                                                                        | A receiver can prove that no earlier route event is expected, while a distributed zero-delay SCC additionally proves that all participants are passive and no causal message is in flight. Silence and route watermarks alone are insufficient. |
| D-004  | Determine true quiescence for an active causal component, not for the whole brain.                                                                                                                            | Unrelated shards and other whole-brains continue in parallel.                                                                                                                                                                                   |
| D-005  | At settling_limit, commit a recorded provisional state and defer unresolved events to the next configured biological quantum with deferred_from_nonconvergence.                                               | Work is not dropped and non-convergence becomes observable, replayable scheduler evidence.                                                                                                                                                      |
| D-006  | Use deterministic or fixed-point accumulation wherever exact reproducibility is required.                                                                                                                     | Canonical ordering, specified rounding and deterministic random streams become part of the model contract.                                                                                                                                      |
| D-007  | Learn the settling and amplification behaviour of shards and strongly connected components.                                                                                                                   | Placement, resource allocation, fidelity and settling budgets adapt from measured causal work.                                                                                                                                                  |
| D-008  | Support multiple independent and federated whole-brain emulations concurrently.                                                                                                                               | Every event, state object, control operation and metric is scoped by stable brain and federation identity.                                                                                                                                      |
| D-009  | Use one fenced active writer per shard, a warm backup receiving synchronously replicated causal log entries, periodic immutable checkpoints, and an optional cold copy.                                       | Failover can reconstruct committed state without permitting two active writers.                                                                                                                                                                 |
| D-010  | Make the web UI and Rust UI workstation clients of the same authorised orchestrator API.                                                                                                                      | Either interface can manage any permitted brain without requiring local ownership or direct worker access.                                                                                                                                      |
| D-011  | Make authorised workstation peripherals first-class sensory and actuator endpoints.                                                                                                                           | Microphone, camera, display/screen, keyboard, pointer and bidirectional USB AER streams can be bound concurrently to one selected brain through an I/O gateway with explicit time mapping, capabilities, consent, backpressure and audit.          |
| D-012  | Separate committed neural output from irreversible external side effects.                                                                                                                                     | Audio/video presentation and keyboard/pointer actuation use stable effect identities, an actuator lease and client deduplication; replay or failover cannot repeat a physical action silently.                                                  |
| D-013  | Treat non-convergence deferral as an explicit model discontinuity, not an exact biological result.                                                                                                            | The settling cut is causally complete through its last microstep, unresolved work is preserved, downstream consumers are notified, and validation quantifies the effect of moving it to a later quantum.                                        |
| D-014  | Support concurrent bidirectional USB AER exchange alongside workstation audio, visual and HID channels.                                                                                                      | USB AER is an independently sequenced, clocked, authorised and backpressured peripheral modality; its hot-plug, failure or congestion does not stop or retimestamp other active modalities.                                                      |

## Critical review findings resolved in version 1.1

| **Finding**                                                                              | **Risk in the earlier baseline**                                                                                                 | **Resolution in this revision**                                                                                                             |
|------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------|
| “Synapse firing” and energise/discharge were underspecified.                             | Presynaptic spike, axonal arrival, vesicle release, conductance and plasticity could be evaluated twice or at inconsistent tags. | Define model stages, one authoritative owner and separate event/eligible tags.                                                              |
| Per-route watermarks were presented too close to a complete quiescence proof.            | A cyclic computation could reactivate a sender after it promised local silence, producing false convergence.                     | Retain watermarks as safe horizons and add component-scoped distributed termination detection with in-flight accounting.                    |
| Zero-delay cycles could be read as biologically ordinary.                                | The executor abstraction could be mistaken for evidence of instantaneous chemical synapses.                                      | Require explicit zero-delay justification and distinguish algebraic/model-order dependencies from non-zero biological delays.               |
| Settling-limit state could be frozen while same-tag messages remained unknown in flight. | The recorded unresolved set would be incomplete and replay could diverge.                                                        | Require a causally complete microstep settling cut; transport failure is Blocked/Failed, not non-convergence.                               |
| Deferral preserved events but its scientific consequence was understated.                | Moving work to a later quantum changes trajectories despite no event loss.                                                       | Label provisional state/discontinuity, propagate quality and quantify divergence against a higher-limit reference.                          |
| Fixed-point was described as a broadly sufficient format.                                | One Q format may overflow or lose important precision across heterogeneous biological variables.                                 | Require per-variable units/range/error analysis, dimensioned formats and high-precision oracle validation.                                  |
| “No whole-brain synchronisation in biology” was too absolute.                            | It ignored oscillations, phase coordination and travelling waves.                                                                | Forbid only artificial universal lockstep; preserve modelled large-scale synchronisation as biological dynamics.                            |
| Brain-level export needed a stronger consistent-cut statement.                           | Independently latest shard checkpoints can omit in-transit causal state.                                                         | Require a documented distributed-snapshot/consistent-cut algorithm and channel/log-tail state.                                              |
| External output replay semantics were incomplete.                                        | Failover might repeat an irreversible keyboard/pointer or other side effect.                                                     | Add committed EffectId, actuator lease, retained dedupe and explicit ambiguous-ack states.                                                  |
| Workstation peripheral I/O was absent.                                                   | The UI could manage a brain but could not serve as its environment/sensory-effector endpoint.                                    | Add session, capability, media gateway, transducer, actuator, time mapping, privacy, resilience and QA requirements.                        |
| Browser and native capabilities were not distinguished.                                  | A web implementation might falsely promise global keyboard/mouse capture or injection.                                           | Define platform capability tiers; reserve privileged virtual HID for an optional, separately secured native adapter.                        |
| Deterministic replay of live sensory input was not closed.                               | Raw live media is non-repeatable and lossy transforms/codecs may vary.                                                           | Record admitted-event provenance and optionally encrypted source; pin mapping/transducer versions and mark non-replayable input explicitly. |
| USB AER and A/V/HID coexistence was not explicit.                                        | An implementation could make a USB neuromorphic device mutually exclusive with microphone, camera, display, keyboard or pointer streams, or allow one channel to starve another. | Define independent concurrent channels, device epochs, clock mappings, bounded fair multiplexing, hot-plug isolation and bidirectional AER acceptance tests. |

## Scope

This document covers logical time, event semantics, partitioning, execution, deterministic numerics, workload learning, resource scheduling, multiple brains, federation, growth, transport, persistence, checkpoints, high availability, recovery, split-brain avoidance, APIs, security, web and Rust user interfaces, concurrent workstation audio/video/keyboard/pointer and bidirectional USB AER sensory/actuator streams, observability, migration, documentation and verification.

This document does not prescribe a particular consensus library, object store, database vendor, GPU framework or identity provider. Implementations shall be hidden behind narrow traits and configuration so that an engineering decision can be changed without rewriting biological execution.

## Contents

1\. Current-state assessment and migration intent

2\. Architectural boundaries and invariants

3\. Identity, terminology and logical-time model

4\. Target platform architecture

5\. Virtual sharding and biological graph partitioning

6\. Superdense event processing and causal closure

7\. Quiescence, convergence and non-convergence

8\. Deterministic computation and numerical profiles

9\. Runner and biological-engine refactoring

10\. Transport, reliability and backpressure

11\. Scheduling, learned workload and heterogeneous resources

12\. Multiple whole-brains and federation

13\. Growth, plasticity and topology transactions

14\. Checkpoints, logs and durability

15\. Failure, recovery, migration and split-brain prevention

16\. Remote management, workstation I/O, security and both user interfaces

17\. Protocols, APIs, storage schemas and configuration

18\. Observability and operational behaviour

19\. File and module change plan

20\. Staged delivery plan

21\. Verification, validation and acceptance

22\. Codex implementation instructions

23\. Appendices: reference types, state machines, traceability, glossary and evidence basis

# 1. Current-state assessment and migration intent

## 1.1 Observed distributed behaviour

The attached implementation distributes work by layer ranges rather than by stable neuron, synapse or graph-component ownership. With seven nodes and a network containing two hidden layers plus an output layer, the small-network fallback assigns all layers to an anchor node and one redundant layer to each remaining node. Assigned layers are marked redundant. Nodes retain complete network structures while runner.rs restricts calculation to an assigned layer range. Active spikes from redundant layers are sent to peers as layer-oriented SpikeBatch payloads.

This behaviour creates overlapping computation, broadcasts data that many recipients do not need, and cannot scale a small but complex biological graph across many physical nodes. It also couples biological structure to physical placement and makes replicated growth or morphology vulnerable to divergence.

Current persistent gRPC fallback after burst-transport timeouts keeps the services active but is a degraded path. A correct new design shall treat fallback, retries, duplicate delivery, reordering and partial sends as normal transport conditions, not as exceptions that can alter biological results.

## 1.2 Code-grounded findings

| **Current file or area**                                                 | **Current responsibility or concern**                                                                                                                                                                                                                                                | **Required direction**                                                                                                                                                |
|--------------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| distributed.rs::build_sharded_node_assignments                           | Builds overlapping layer ranges and the anchor-node fallback.                                                                                                                                                                                                                        | Replace with stable virtual-shard plans derived from the biological graph, zero-delay strongly connected components, weighted cost and placement constraints.         |
| distributed.rs::ManagedNetwork                                           | Owns a full Runner, layer assignments and remote forward/backward spike maps.                                                                                                                                                                                                        | Split brain metadata, immutable graph, shard-owned mutable state, event inbox/outbox, replica state and placement state.                                              |
| distributed.rs::run_simulation                                           | Iterates managed networks, holds locks across Runner::step, sends redundant spikes and adjusts depth using step time.                                                                                                                                                                | Replace with a non-blocking, work-conserving executor of ready shard/component work items and explicit scheduler decisions.                                           |
| distributed.rs::handle_incoming_spike_batch                              | Converts batches to layer vectors; step maps can overwrite; timing is not superdense.                                                                                                                                                                                                | Decode typed causal envelopes, validate epoch/term/generation, deduplicate, enqueue by tag and acknowledge durable receipt.                                           |
| distributed.rs::send_spike_batches                                       | Chooses MPI, persistent gRPC or burst gRPC and can split/retry.                                                                                                                                                                                                                      | Route per-destination causal events through a reliability layer with sequence numbers, retransmission, credits and transport-independent semantics.                   |
| runner.rs::Runner::step                                                  | Advances all biological work and then increments time in one monolithic call.                                                                                                                                                                                                        | Refactor into prepare, deliver, accumulate, update, emit and commit phases that operate on an explicit logical tag and shard view.                                    |
| morphology.rs::Morphology::evolve                                        | Mutates structure within the local runner.                                                                                                                                                                                                                                           | Execute topology mutation through one authoritative topology transaction and publish a new topology generation.                                                       |
| aer.rs                                                                   | Carries a timestamp but converts input to vectors, losing causal timing.                                                                                                                                                                                                             | Preserve source timestamp, time-domain mapping, event identity and deduplication through ingestion.                                                                   |
| index.html and app.js I/O source                                         | Offer only an external AER HTTP/HTTPS NDJSON source and synchronously forward individual frames through the management/API surface. They do not capture workstation microphone, camera, screen, keyboard or pointer streams, support a concurrent local USB AER device, or present brain-directed actuator output. | Replace with a dedicated, authorised peripheral-session experience and media/data plane. Retain AER as one independently multiplexed transducer source, not as the universal sensory transport. |
| transmission.rs                                                          | Uses integer step delays and vector-position synapse indices.                                                                                                                                                                                                                        | Introduce stable SynapseId, configured biological delay ticks and deterministic release identities.                                                                   |
| bridge.rs::ExternalRunnerBridge::step                                    | Directly drives a monolithic runner step.                                                                                                                                                                                                                                            | Submit timestamped external events through a gateway and observe committed state/output streams.                                                                      |
| ui.rs::connect_cluster_client                                            | Creates a large unauthenticated gRPC client.                                                                                                                                                                                                                                         | Use an authenticated generated management client with TLS, deadlines, retry policy and server-side authorisation.                                                     |
| ui.rs::apply_cluster_control                                             | Can optimistically update local state and send direct control calls to workers.                                                                                                                                                                                                      | Submit one idempotent, version-checked operation to the orchestrator; workers shall reject end-user management calls.                                                 |
| ui.rs::queue_import                                                      | Reports that import is unsupported for a cluster view in important paths.                                                                                                                                                                                                            | Stage and validate import through the orchestrator for any authorised remote brain.                                                                                   |
| app.js::applyRemoteJsonPayload, updateNetworkSettings, sendControlAction | Posts simple mutable requests and sometimes relies on client-side access checks.                                                                                                                                                                                                     | Use a common versioned management API, operation resources, expected versions and server-enforced policy.                                                             |
| service-access.js                                                        | Computes convenient access-level flags in the browser.                                                                                                                                                                                                                               | Retain for presentation only; do not treat browser-derived grants as authority.                                                                                       |

## 1.3 Migration intent

The existing implementation shall remain runnable behind a temporary compatibility feature until deterministic reference tests, protocol compatibility tests and migration tooling show that the new path is safe. Compatibility must not leak old layer-broadcast semantics into the new event model. An adapter may translate old input/output at the boundary, but the new engine shall use stable event and shard identities internally.

> **CAUTION:** Codex shall not perform a mechanical rename of “layer” to “shard”. The change is semantic: a virtual shard owns a graph partition and mutable biological state; a physical node is only its current placement.

# 2. Architectural boundaries and invariants

## 2.1 Three independent representations

The implementation shall keep these representations separate:

- **Biological representation:** neurons, synapses, morphology, delays, plasticity, modulatory fields and growth rules. It expresses the model, not where it runs.

- **Virtual execution representation:** stable shards, zero-delay strongly connected components, event routes, logical tags, checkpoint streams and replica roles. It expresses causal ownership.

- **Physical representation:** compute nodes, CPU/GPU devices, NUMA domains, RAM, VRAM, storage tiers, network links and failure domains. It expresses placement at a particular control-plane generation.

Rebalancing shall alter only the virtual-to-physical mapping unless an explicit, versioned repartition transaction changes the virtual representation. Physical node identifiers shall never be embedded in stable biological identifiers or checkpoint contents other than placement metadata.

![Target execution, control and durability planes, showing authorised workstation clients, the replicated control plane, multiple independent whole-brains, heterogeneous compute nodes and durable services.](media/image1.png "Architecture overview")

**Figure 1. Architecture overview.** Target execution, control and durability planes, showing authorised workstation clients, the replicated control plane, multiple independent whole-brains, heterogeneous compute nodes, and durable services.

## 2.2 Mandatory invariants

| **Invariant ID** | **Invariant**                                                                                                                                 | **Enforcement**                                                                                                               |
|------------------|-----------------------------------------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------|
| INV-001          | Each committed shard state has exactly one active writer term.                                                                                | Consensus-backed lease, fencing token and receiver rejection of stale terms.                                                  |
| INV-002          | An event carries the logical time of the synaptic transition it represents.                                                                   | Type-safe LogicalTag; validation at every ingress.                                                                            |
| INV-003          | Same-time causal output advances the microstep.                                                                                               | (t, μ) may emit zero-delay work only at (t, μ + 1).                                                                           |
| INV-004          | Positive delay advances biological time.                                                                                                      | (t, μ) with delay δ \> 0 emits at (t + δ, 0).                                                                                 |
| INV-005          | No receiver declares closure from silence.                                                                                                    | Explicit watermarks/closures and reliable stream accounting.                                                                  |
| INV-006          | Settling-limit exhaustion is not quiescence.                                                                                                  | Separate outcome types and metrics; unresolved work is deferred and marked.                                                   |
| INV-007          | Events are never silently dropped to preserve responsiveness.                                                                                 | Backpressure, bounded queues, spill or controlled rejection before admission; committed events persist.                       |
| INV-008          | Deterministic-reference replay is independent of thread scheduling and transport order.                                                       | Canonical ordering, fixed-point arithmetic, deterministic RNG and deterministic kernels.                                      |
| INV-009          | Topology changes become visible atomically at an agreed logical boundary.                                                                     | Topology transaction and generation checks.                                                                                   |
| INV-010          | Unrelated brains and causal components do not wait for one another.                                                                           | Component-scoped settlement and fair work-conserving executor.                                                                |
| INV-011          | Management clients cannot bypass the orchestrator.                                                                                            | Network policy, worker mTLS identity and server-side authorisation.                                                           |
| INV-012          | A checkpoint is immutable after publication.                                                                                                  | Content address or unique version key, checksum and write-once policy.                                                        |
| INV-013          | Route watermarks do not prove termination of a distributed same-tick cycle.                                                                   | Component-tag activity epochs plus durable sent/received accounting or a proven termination-detection algorithm.              |
| INV-014          | Every axon terminal, synapse, weight, delay, release state and plasticity trace has exactly one authoritative owner in a topology generation. | Ownership map validation, generation fencing and single-owner checkpoint schema.                                              |
| INV-015          | An admitted peripheral sample retains capture sequence/time, clock-mapping version and uncertainty.                                           | PeripheralSampleEnvelope; logical eligibility derives from the declared mapping, never packet arrival time.                   |
| INV-016          | An external side effect is issued only from committed neural output and is not repeated by replay.                                            | Stable EffectId, output commit log, per-channel actuator lease, client dedupe and acknowledgement.                            |
| INV-017          | Peripheral access is explicit, scoped, revocable and locally visible.                                                                         | Separate I/O permissions, user gesture/consent where the platform requires it, live indicators, expiry and an emergency stop. |

## 2.3 Control, data and management planes

The **data plane** executes biological events, routes causal envelopes, applies local closures and persists the shard log. The **control plane** owns membership, consensus, leases, placement, topology generations, scheduler decisions and failover. The **management plane** authenticates users and services, authorises operations, exposes APIs and streams status. A separate **peripheral media plane** carries bounded real-time audio/video and keyboard/pointer samples or actuator effects through an I/O gateway; it shall not share unrestricted worker credentials or turn lossy media transport into the authoritative causal log. These planes shall use distinct message types, permissions and metrics even if early deployments share processes.

The data plane shall continue safely during a temporary management UI outage. Loss of control-plane quorum shall freeze new ownership, repartition and destructive management actions; a configurable safe mode may allow already leased active shards to continue only until their leases expire or a bounded grace policy is reached. The policy and its safety trade-off shall be documented and tested.

# 3. Identity, terminology and logical-time model

## 3.1 Stable identity types

Raw strings and vector indices shall not be passed across module, process or persistence boundaries where a stable typed identifier is required. Newtypes shall be serialisable, orderable and documented.

| **Type**                                    | **Purpose**                                                                              | **Stability rule**                                                                                |
|---------------------------------------------|------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------------------|
| FederationId                                | Groups explicitly linked brains and shared policy.                                       | Stable for federation lifetime.                                                                   |
| BrainId                                     | Identifies one independently scheduled whole-brain emulation.                            | Stable across restart, migration and export/import unless intentionally cloned.                   |
| ShardId                                     | Identifies a virtual graph partition.                                                    | Stable across physical placement; changes only in a recorded repartition.                         |
| ComponentId                                 | Identifies a zero-delay strongly connected region within a topology generation.          | Recomputed only when relevant topology or minimum-delay rules change.                             |
| NeuronId                                    | Stable biological neuron identity.                                                       | Never inferred from current layer/vector position.                                                |
| SynapseId                                   | Stable biological synapse identity.                                                      | Survives compaction and migration; retirement is tombstoned.                                      |
| EventId                                     | Unique causal event identity.                                                            | Derived from source identity, source sequence, tag and event kind or allocated deterministically. |
| TopologyGeneration                          | Version of biological connectivity and component graph.                                  | Monotonically increasing per brain.                                                               |
| PartitionGeneration                         | Version of component-to-shard assignment.                                                | Monotonically increasing per brain.                                                               |
| LeaseTerm and FencingToken                  | Authorise one active writer.                                                             | Monotonic and checked at all write/receive/output boundaries.                                     |
| OperationId                                 | Tracks asynchronous management work.                                                     | Globally unique and idempotently returned for a repeated request key.                             |
| PeripheralSessionId and PeripheralChannelId | Bind one authorised workstation session and one media/HID/USB-AER channel to a permitted brain. | Ephemeral, leased, non-reusable and never inferred from a browser tab, USB handle or socket.       |
| LocalDeviceBindingId and DeviceEpoch         | Identify one authorised local USB AER device binding and one validated connection epoch. | Binding is session-scoped; epoch changes on reset, removal, reconnect or material renegotiation.  |
| PeripheralSampleId                          | Deduplicates a captured audio/video/keyboard/pointer sample before biological admission. | Stable across reconnect/retransmission; unique within the session channel.                        |
| EffectId                                    | Identifies a committed actuator intent.                                                  | Stable across failover/replay and applied at most once by a workstation channel.                  |
| ClockMappingVersion and TransducerVersion   | Identify the external-clock mapping and sensory/actuator transform.                      | Versioned, auditable and recorded with admitted events and replay artefacts.                      |

## 3.2 Superdense logical tag

The logical tag shall be lexicographically ordered:

\#\[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)\]  
pub struct LogicalTag {  
pub tick: u64,  
pub microstep: u32,  
}

tick is an integer count of the brain’s configured base biological quantum. It is not wall-clock time and shall not be represented as f64. microstep orders a finite sequence of zero-biological-time consequences. Human-readable time is derived using the brain’s immutable or versioned time-base metadata.

The implementation shall define overflow behaviour. tick overflow is a fatal model-lifetime error detected before mutation. microstep exhaustion shall be treated as non-convergence and use the same recorded deferral mechanism as settling_limit; it shall not wrap.

## 3.3 Presynaptic, synaptic and postsynaptic event semantics

The project phrase “a synapse is energised/discharged” shall be retained only as the implementation name SynapticTransition; it shall not be used as if a biological synapse were itself a spiking neuron. Each supported model shall define which scientifically meaningful state change that transition represents: for example transmitter-release success/failure, receptor-channel conductance onset/offset, gap-junction current change or another named mechanism.

The causal stages shall be kept distinct:

1\. SpikeDecision: a source neuron crosses its firing rule at tag T_f.

2\. AxonalDeparture: a spike is launched into a modelled axon or route.

3\. AxonalArrival: the spike reaches one presynaptic terminal after conduction delay.

4\. SynapticTransition: the single authoritative synapse owner evaluates release, delay, short-term state and applicable presynaptic plasticity at T_s.

5\. PostsynapticEffect: the target-owned conductance/current contribution becomes eligible at T_p.

6\. PlasticityUpdate: pre/post/neuromodulatory traces are updated by one declared owner at a specified tag.

The topology schema shall state which of these stages exist for every synapse class and which virtual shard owns each stage. The recommended partition places the terminal, synapse weight, release state and plasticity traces with the target shard or a co-located boundary shard. A source then routes an AxonalDeparture/AxonalArrival candidate; the receiver computes exactly one SynapticTransition. If a specialised model places the synapse with the source, the source routes the committed SynapticTransition or postsynaptic effect instead. A route shall never cause both sides to evaluate release or update the same weight.

Coherence across shards is anchored to T_s, the tag of the authoritative SynapticTransition. All consumers see the same transition identity, outcome and tag. T_f, T_s and T_p shall be carried explicitly when the model distinguishes them; none is inferred from packet arrival. Total delay is the declared composition of axonal conduction, terminal/release and postsynaptic-effect delay, with units and rounding specified per synapse class.

Where a causal consequence has the same biological tick, it shall still advance from microstep μ to μ + 1. This orders computation without asserting that real chemical transmission has zero duration. A biologically non-zero lower-bound delay shall advance tick; zero-delay edges are an explicit model abstraction and require the SCC protocol in Sections 5–7.

## 3.4 Wall-clock mapping

Wall-clock time is a pacing and service-level concern. It shall never decide causal order. A real-time controller may map committed biological ticks to deadlines, run slower than real time, pause or fast-forward. Its state shall be separate from logical tags and excluded from deterministic state digests except as recorded telemetry.

Every bridge between external timestamps and a brain clock shall use a versioned TimeDomainMapping defining source clock identity, monotonic epoch, scale, offset/drift estimate, rounding, uncertainty and discontinuity policy. Capture time, gateway-receipt time and brain-eligible tag are separate fields. Late or future external events shall follow an explicit reject, clamp, buffer, defer or offline-replay policy; silent retimestamping and use of network arrival time as biological time are prohibited. Rollback is not permitted for live irreversible workstation output.

Biological coordination shall not be described as complete absence of synchronisation. Brains exhibit local and large-scale oscillatory coordination, phase relationships and travelling waves; these are modelled dynamics. What the emulator shall avoid is an artificial universal lockstep barrier unrelated to those dynamics.

# 4. Target platform architecture

## 4.1 Services and responsibilities

| **Component**               | **Required responsibilities**                                                                                                                   | **Explicit non-responsibilities**                                                      |
|-----------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------|
| Control-plane quorum        | Membership, node health, leases, fencing, placement generations, scheduler decisions, operation metadata and policy versions.                   | Biological state mutation or rendering.                                                |
| Orchestrator API            | Authentication, authorisation, idempotency, optimistic concurrency, staged management operations and status streams.                            | Direct execution of neuron updates.                                                    |
| Shard executor              | Ordered event processing, local component settlement, state commit, WAL replication, checkpoint production and metrics.                         | User identity decisions or global placement.                                           |
| Event router                | Route lookup, reliable ordered streams, deduplication, acknowledgements, credits and retransmission.                                            | Biological computation or scheduler policy.                                            |
| Scheduler                   | Predict workload, place active/backup shards, allocate compute/memory budgets, migrate and rebalance.                                           | Modify model fidelity without a committed decision.                                    |
| Topology service            | Stable IDs, topology transaction validation, SCC/component graph and generation publication.                                                    | Physical node ownership.                                                               |
| Durability service          | Immutable checkpoint catalogue, causal-log retention, checksums, manifests and restore discovery.                                               | Choose the next active writer.                                                         |
| Federation gateway          | Authorised mapping and routing between independent brain timelines.                                                                             | Implicit shared clock or federation-wide barrier.                                      |
| UI clients                  | Present permitted state, submit operations, stream progress and resolve conflicts.                                                              | Enforce final authorisation or call workers directly.                                  |
| Peripheral-session service  | Authorise, lease and audit one workstation’s declared capture/render/actuation capabilities for selected brains.                                | Carry high-rate media or grant implicit brain-control permission.                      |
| I/O media gateway           | Terminate secure real-time media/data sessions, sequence samples/effects, maintain bounded jitter buffers and feed transducer/actuator workers. | Decide biological time from arrival, evaluate neural state or bypass causal admission. |
| Transducer/actuator workers | Transform media/HID samples to versioned sensory events and committed neural outputs to bounded workstation effects.                            | Own user authorisation, silently change mappings or repeat effects during replay.      |

## 4.2 Concurrency model

The preferred runtime model is one logical actor or single-writer task per active shard, backed by bounded work queues and a shared CPU/GPU executor. An actor need not own an operating-system thread. Ready work shall be dispatched through work-stealing or equivalent pools while preserving deterministic per-shard commit order.

Blocking file, object-store, DNS, authentication, codec initialisation, device enumeration and legacy conversion work shall run on dedicated bounded blocking pools. Async tasks shall never hold a synchronous mutex, read/write guard or shard-state lock across .await. Long biological or transducer kernels shall operate on isolated state slices or command buffers and publish results through a short commit section. Real-time capture/render loops shall use bounded queues and shall not execute neural kernels, checkpoint I/O or management operations on their callback/render threads.

## 4.3 Resource discovery

On joining, every node shall advertise and benchmark:

- CPU architecture, logical and physical core topology, SIMD capabilities, NUMA domains and measured deterministic-kernel throughput;

- GPU/accelerator type, supported deterministic modes, compute throughput, device memory, transfer bandwidth and peer-to-peer capability;

- available and reservable RAM, VRAM, local durable storage, ephemeral storage and checkpoint cache;

- network latency, bandwidth and failure-domain labels;

- supported emulator/protocol/schema versions and enabled numerical profiles.

Benchmarks shall be repeatable, bounded and tagged with software/kernel version. Scheduler decisions shall use measured effective capacity and current load, not core count alone.

# 5. Virtual sharding and biological graph partitioning

## 5.1 Many virtual shards, fewer or more physical nodes

The system shall normally create substantially more virtual shards than physical nodes so that work can be redistributed as nodes appear, disappear or change load. A three-layer network is not inherently limited to three useful partitions. Partition candidates shall be based on biological graph connectivity, mutable state size, component structure and measured cost.

Shard size shall be bounded by configurable target and maximum state/working-set sizes. Large shards shall be eligible for safe repartition at a committed topology boundary. Very small shards may be co-scheduled, but shall retain stable identities and independent metrics.

## 5.2 Zero-delay strongly connected components

The topology service shall compute strongly connected components over edges whose **minimum possible effective biological delay** is zero. Dynamic delays require a conservative lower bound: if an edge can become zero under a permitted model state, it belongs to the zero-delay graph.

An SCC is the preferred indivisible placement atom because an instantaneous cycle otherwise requires distributed microstep fencing. The partitioner shall co-locate an SCC whenever its resource requirements fit an eligible node/device. It shall then partition the resulting component DAG using weighted cuts.

If an SCC cannot fit one node, the system may distribute it only with a component-scoped microstep protocol. All participants in that SCC shall close each microstep before advancing that SCC. No unrelated component or brain shall join the fence.

Zero-delay edges shall be rare, explicit and justified. Chemical synapses and axonal propagation ordinarily have non-zero latency; a zero-delay edge normally represents an algebraic/model-order dependency, an electrical coupling abstraction or resolution finer than the configured tick. The importer and topology validator shall report zero-delay cycles and offer model-owner diagnostics, but shall never insert a positive delay solely to simplify scheduling.

## 5.3 Weighted graph cost

The partitioner and scheduler shall optimise a multi-dimensional cost rather than neuron count:

- observed events, synaptic operations and microsteps per biological tick;

- morphology, plasticity, growth and neuromodulation cost;

- state size, working-set size, checkpoint delta size and memory bandwidth;

- predicted cross-shard event count, bytes and route fan-out;

- CPU and deterministic GPU kernel time;

- amplification factor, residual energy and non-convergence frequency;

- causal criticality: how many downstream components are waiting;

- anti-affinity, device eligibility, failure-domain and replica-placement constraints.

Weights shall be configuration with safe defaults and visible in scheduler explanations. Repartition shall include hysteresis, minimum residence time, migration cost and a material-benefit threshold to prevent oscillation.

## 5.4 Placement plan

A placement plan shall be immutable, versioned and take effect at an explicit committed logical boundary. It shall include BrainId, PartitionGeneration, topology generation, each shard’s active node and device, lease term, fencing token, backup nodes, failure-domain labels and capacity reservations.

Executors shall reject an event or command whose brain, partition generation, topology generation or fencing term is stale or impossible. Rejection shall produce a typed response that lets the sender refresh routing and retry without silently losing the event.

# 6. Superdense event processing and causal closure

## 6.1 Causal envelope

Every cross-shard biological message shall be a typed envelope rather than a whole layer vector. At minimum it shall carry:

pub struct CausalEnvelope {  
pub federation_id: Option\<FederationId\>,  
pub brain_id: BrainId,  
pub source_shard: ShardId,  
pub destination_shard: ShardId,  
pub topology_generation: TopologyGeneration,  
pub partition_generation: PartitionGeneration,  
pub lease_term: LeaseTerm,  
pub fencing_token: FencingToken,  
pub stream_id: StreamId,  
pub sequence: u64,  
pub event_id: EventId,  
pub event_tag: LogicalTag,  
pub eligible_tag: LogicalTag,  
pub causal_parent: Option\<EventId\>,  
pub target: EventTarget,  
pub payload: BiologicalEvent,  
pub provenance: EventProvenance,  
}

BiologicalEvent shall distinguish spike decision, axonal departure/arrival, synaptic transition with release outcome, postsynaptic effect, plasticity update, modulatory field update, topology-effective event, external sensory input and federation input. event_tag is when the represented transition occurs; eligible_tag is the earliest tag at which its destination may apply the effect. For a committed SynapticTransition, every consumer shall receive the same event_tag, event identity and outcome. A receiver shall never need to inspect an untyped JSON payload to establish ordering.

## 6.2 Event progression rules

For an event processed at (t, μ):

- a zero-delay direct consequence shall be stamped (t, μ + 1);

- a positive delay of δ ticks shall be stamped (t + δ, 0);

- an event shall not target a lower tag;

- a component shall consume all admissible input for its current tag in canonical order before committing that microstep;

- output shall be staged until the state transition is successfully logged and committed;

- duplicate EventId or (stream_id, sequence) delivery shall be acknowledged but not applied twice.

The engine shall distinguish **microstep closure** from **tick closure**. Microstep closure proves that the component can no longer generate work for (t, μ) and may reveal/process (t, μ + 1). Tick closure is reached only when a closed microstep generated no eligible same-tick work and all pending work is at a later tick. This local protocol does not prevent another component at the same tick from advancing independently.

![Superdense logical time uses biological tick t and causal microstep mu. Quiescence is component-scoped; unresolved work at the settling limit is checkpointed and deferred.](media/image2.png "Superdense time and quiescence")

**Figure 2. Superdense time and quiescence.** Superdense logical time uses biological tick t and causal microstep mu. Quiescence is component-scoped; unresolved work at the settling limit is checkpointed and deferred.

## 6.3 Stream watermarks and closure

Silence is not proof of completion. Each ordered source-to-destination route shall carry data and control frames. A StreamWatermark { through: LogicalTag } asserts that the source will not later emit an event on that stream with a tag less than or equal to through for the current generations and term. It establishes a route safe horizon; it does **not**, by itself, establish termination of a cyclic distributed computation whose receipt can reactivate the sender.

Watermarks shall be monotonic, durable enough to recover stream position, and covered by the same sequence/deduplication protocol as events. A late event at or below an accepted watermark is a protocol violation: quarantine the stream, preserve evidence and invoke a configured recovery path. Do not apply it speculatively.

Route creation, removal and topology change require a handshake so that the expected-producer set is stable for the tag being settled. The receiver shall not declare quiescence while an unclosed producer may still exist.

For each distributed zero-delay component and tag, implement a proven, component-scoped termination detector (for example a credit/acknowledgement or Safra/Dijkstra-Scholten-family protocol adapted to stable membership). The reference implementation shall use a ComponentTagEpoch and the following auditable proof:

1\. Every accepted causal send increments a durable sent balance before transmission; durable receipt increments the matching received balance exactly once.

2\. A participant reports Passive only after its local queue for the tag is empty, compute/output staging is empty and all sends generated by its last activity epoch are in the balance.

3\. The report carries monotonically increasing local activity epoch, sent/received positions and membership/generation. Receipt of new work invalidates an earlier passive report.

4\. The component coordinator/rotating token holder declares the microstep closed only when all current participants are passive in one consistent observation, the component outstanding balance is zero, and no report was invalidated.

5\. AdvanceMicrostep carries a component fence/epoch; stale closure or advancement messages are rejected.

The coordinator is an optimisation and failure-recovery role, not a whole-brain barrier or source of biological time. Its state shall be replicated/reconstructible, and a coordinator change shall not allow two closure decisions for one component tag.

## 6.4 Downstream and circular dependencies

If shard D computes at (t, μ) and sends an instantaneous event to shard A, A schedules that event at (t, μ + 1) even if A has already produced provisional output at microstep μ. A’s tick is not committed as quiescent until safe route horizons cover the required tag and, for a cyclic distributed component, its termination proof establishes participant passivity, zero durable outstanding balance and no local or in-flight event.

If the route participates in a zero-delay cycle, all affected nodes are in the same SCC. Prefer co-location. Where distributed, the component-scoped microstep fence advances only after the termination detector proves participant passivity and zero in-flight causal balance for the current epoch. Per-route watermarks remain useful for safe horizons and recovery but do not replace that proof.

Positive-delay feedback is not a same-tick cycle. It is scheduled at a future tick and does not block current quiescence.

## 6.5 Global fields

Resonance, homeostasis, ambient drive or other AARNN effects that influence neurons without direct synapses shall be explicit scheduled field events. Their scope, cadence, aggregation rule, deterministic reduction and effective tag shall be part of configuration. They shall not be hidden reasons for every shard to wait at every biological tick.

A truly whole-brain field update may use asynchronous snapshot/GVT information and publish at a declared future tag. Its cadence may be coarser than the base quantum. Approximation and staleness bounds shall be visible in state and metrics.

# 7. Quiescence, convergence and non-convergence

## 7.1 True quiescence

An active causal component is quiescent at tag (t, μ) only when all of the following are true:

1\. Every participant has completed its deterministic local transition for the tag.

2\. Its local ready queue contains no event at the tag.

3\. All emitted event frames at the tag are durably accounted for by destination acknowledgements or the termination detector’s proven in-flight count.

4\. Every acyclic or external expected producer has advanced its safe horizon beyond the tag; cyclic participants are covered by the component termination proof and shall not be required to make a logically circular watermark promise.

5\. All participants are passive in one valid ComponentTagEpoch; any later receipt has invalidated the earlier report.

6\. No topology transaction can add a producer retrospectively for the tag.

7\. The component-wide outstanding-event balance is zero and the closure decision is fenced against duplicate coordinators.

When quiescent at biological tick t, the component may commit its state for t and advance to its smallest queued future tag. It does not wait for unrelated components at t.

## 7.2 Three independent depths

The implementation shall not overload AARNN depth, observed cascade depth and a safety cap:

| **Concept**             | **Meaning**                                                             | **Ownership**                                                                                                           |
|-------------------------|-------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------|
| fidelity_depth          | Configured biological calculation detail or AARNN depth.                | Versioned model policy, changed only by an authorised model/control decision; scheduler pressure alone is insufficient. |
| observed_settling_depth | Actual number of microsteps required for an active component at a tick. | Measured outcome, never a limit.                                                                                        |
| settling_limit          | Maximum microsteps permitted before the non-convergence path.           | Scheduler budget with min/max bounds and decision provenance.                                                           |

A high-fidelity local computation may take less wall-clock time on a fast GPU than a shallow computation on a busy CPU. Scheduling shall therefore compare predicted causal work and measured resource performance, not force equal biological depth on all shards.

## 7.3 Settling outcomes

The executor shall return an explicit outcome:

pub enum SettlementOutcome {  
Quiescent { committed_through: LogicalTag, microsteps: u32 },  
DeferredNonConvergent { record_id: NonConvergenceId, resume_at: LogicalTag },  
Blocked { waiting_on: Vec\<CausalDependency\> },  
Failed { error: ExecutionError },  
}

DeferredNonConvergent shall never be labelled or counted as quiescence.

## 7.4 Required non-convergence procedure

When a component reaches settling_limit, the executor shall:

1\. Stop admitting new external or federation input into the component’s capped tick and announce a component-local settling cut; unrelated components continue.

2\. Finish the current microstep deterministically; do not interrupt part-way through accumulation.

3\. Complete the component termination proof for that microstep so no unknown event for the completed tag remains in flight. If transport or control failure prevents this proof, return Blocked or Failed and recover; do not mislabel the state as non-convergent.

4\. Freeze a terminal ProvisionalNonConvergent state and calculate its digest.

5\. Persist a NonConvergenceRecord and a replayable immutable checkpoint or checkpoint reference.

6\. Persist the complete canonical ordered set of unresolved known events that would otherwise continue the same tick, with producer and stream positions. “In flight but unknown” is not an acceptable set.

7\. Commit the provisional state as the recorded terminal result for that tick and emit an explicit NonConvergenceDiscontinuity to dependent components and observers.

8\. Retag unresolved events to (t + configured_biological_quantum, 0) or the precisely configured next tick.

9\. Set deferred_from_nonconvergence = true, retain original event and eligible tags, and record the deferral chain.

10\. Publish scheduler evidence and a visible warning without blocking unrelated work. External actuator policies shall decide explicitly whether provisional output is suppressed, presented with a quality flag or permitted for a named low-risk channel.

11\. Continue from the next quantum; never discard, silently damp or repeatedly retry the same capped tick.

If a deferred event is again deferred, the chain shall remain bounded in metadata size using a root origin plus count and recent-history window. Operators shall be able to trace the complete history from the causal log.

Moving same-tick work to a later biological quantum changes the simulated trajectory and is therefore a controlled approximation, even though it preserves every event. Validation shall measure deferral rate, behavioural divergence from a higher-limit reference and downstream impact. The default production policy shall alert and automatically improve future resource or settling-budget decisions within authorised bounds; it shall not claim scientific equivalence.

## 7.5 Non-convergence record

The durable record shall include at least:

- brain, shard, component, topology and partition identities;

- start tag, last completed microstep, settling limit and fidelity depth;

- event count by microstep, amplification ratios, residual energy/activity and convergence estimator values;

- canonical pending-event count and digest, plus durable object reference;

- initial and terminal state digests and checkpoint reference;

- model/configuration version, numerical profile, deterministic kernel version and random-stream coordinates;

- lease term, fencing token, node/device identity and measured CPU/GPU/wall-clock usage;

- durable-log positions, transport outstanding counts and active producer set;

- previous deferral root/count, reason code and operator-visible explanation.

## 7.6 Convergence estimators

Optional residual or energy estimators may predict non-convergence and inform scheduling, but shall not replace the exact quiescence proof. An early-exit approximation is permitted only as a separately named biological profile with stated error bounds, explicit authorisation and validation tests. It shall not be used in DeterministicReference.

# 8. Deterministic computation and numerical profiles

## 8.1 Canonical event order

All simultaneously eligible inputs to a target accumulation domain shall be sorted by a stable total key. The default key is:

(logical_tag, target_neuron_id, target_synapse_id,  
source_brain_id, source_shard_id, source_event_sequence, event_id)

Transport arrival order, hash-map iteration order, Rayon scheduling, work stealing and GPU block order shall have no semantic effect. Collections used in committed deterministic paths shall be ordered or explicitly sorted before reduction. Hash maps may be used for lookup only when their iteration order cannot influence state or output.

## 8.2 Fixed-point accumulation

The deterministic-reference path shall use documented signed fixed-point representations for exact accumulations. Q32.32 with i64 storage and i128 intermediates is a candidate baseline, not a universal biological unit. Before adopting it, perform range/error analysis separately for membrane potential, current/conductance, weights, plasticity traces, probabilities and field variables. Define named dimensioned formats and conversion boundaries where one scale cannot provide both range and precision.

The numerical module shall centralise:

- conversion from configuration values and imported formats;

- round-to-nearest, ties-to-even unless a biological rule requires another named method;

- checked multiply/add using i128 intermediates;

- defined saturation or typed overflow error for every operation; deterministic reference shall fail before commit unless saturation is itself a documented model rule;

- canonical serialisation and state hashing;

- dimensional/unit checks and property tests at minimum/maximum, half-way and overflow boundaries;

- error budgets against a high-precision oracle and representative biological trajectories.

Floating-point values may be used for display and non-authoritative telemetry. They shall not be converted back into committed deterministic state without a versioned, tested conversion. Fixed-point arithmetic provides repeatability, not biological validity; scientific validation shall compare dynamics, spike timing and plasticity behaviour with a higher-precision reference.

## 8.3 Numerical profiles

| **Profile**            | **Intended use**                                                                         | **Required guarantees**                                                                                                                                           |
|------------------------|------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| DeterministicReference | Verification, replay, migration validation, failover catch-up and reproducible research. | Canonical order; fixed-point causal accumulation; counter-based RNG; deterministic CPU kernels and only certified deterministic GPU kernels; exact state digests. |
| FastBiological         | High-throughput exploratory runs where bounded numerical variation is accepted.          | Canonical event order; declared f32/f64 operations; stable tolerance contracts; no claim of bitwise equality across hardware.                                     |

Each brain shall select a profile at creation. Changing it is a versioned management operation effective at a safe committed boundary and recorded in the causal log. Checkpoints shall record the profile and kernel versions.

## 8.4 Randomness

Replace shared or call-order-dependent fastrand use in authoritative paths with a counter-based deterministic random function. A draw shall be addressed by stable coordinates such as (brain_seed, topology_generation, neuron_or_synapse_id, logical_tag, biological_rule, draw_index). Parallel scheduling must not change the result.

Imported seeds, automatic seeds and cloned-brain seed policies shall be explicit. A clone may preserve the seed for exact continuation or derive a new seed for divergence; the user shall choose and the audit record shall state which.

## 8.5 State digests

At configured commit intervals, compute hierarchical digests: neuron/synapse chunk, shard, active component and brain checkpoint manifest. Canonical serialisation shall exclude wall-clock timestamps, node identifiers and non-authoritative metrics. Digests shall detect replay and replica divergence and shall identify the smallest differing region to support diagnosis.

> **NON-NEGOTIABLE:** The project shall not claim bitwise whole-simulation reproducibility while authoritative state depends on platform f64 transcendental functions, shared random generators, unordered parallel reductions or uncertified GPU kernels. Such paths belong only to a profile with an explicit tolerance contract.

# 9. Runner and biological-engine refactoring

## 9.1 Replace the monolithic step

Runner::step currently performs a broad set of biological updates and advances its own time. It shall be decomposed so that an orchestrated shard executor supplies the logical tag and controls commit. The refactoring shall produce narrow, testable phases, for example:

pub trait ShardKernel {  
fn prepare(&mut self, ctx: &ExecutionContext, tag: LogicalTag) -\> Result\<PreparedWork\>;  
fn deliver(&mut self, inputs: &\[CanonicalEvent\]) -\> Result\<DeliverySummary\>;  
fn accumulate(&mut self, work: PreparedWork) -\> Result\<AccumulationResult\>;  
fn update_state(&mut self, accumulated: AccumulationResult) -\> Result\<StateDelta\>;  
fn emit(&self, delta: &StateDelta) -\> Result\<Vec\<PendingEmission\>\>;  
fn commit(&mut self, commit: CommitRecord) -\> Result\<CommittedState\>;  
}

Names may change, but the boundaries shall remain explicit. prepare and compute phases shall not mutate committed state. commit shall be short, single-writer and coupled to the durable causal log. Failed work shall be discardable or retryable without partial mutation.

## 9.2 Shard-owned state

The new runner view shall contain only the mutable state owned by a shard, plus read-only or versioned references required to evaluate local rules. It shall not require every node to retain a second mutable copy of the entire network. Shared immutable topology pages, weight chunks or model parameters may be cached and content-addressed.

Cross-shard synapses shall have an explicit owner. The recommended rule is that the target shard or a co-located boundary shard owns the terminal, delay queue, release state, postsynaptic weight and plasticity traces, while the source route carries a presynaptic spike/axonal event and required provenance. The owner emits the authoritative SynapticTransition; downstream neuronal state never re-evaluates it. If a plasticity rule requires joint pre/post state, define an authoritative owner and timestamped trace messages or a co-location constraint; never update the same weight independently on two shards.

## 9.3 Compatibility facade

A temporary LegacyRunnerFacade may preserve existing standalone and test APIs by translating one legacy step into a sequence of local superdense operations. It shall be clearly marked, isolated from distributed execution and removed after callers migrate. New code shall not add dependencies on layer_range or remote layer spike vectors.

## 9.4 State layout and parallel kernels

Within a shard, biological arrays shall be stored in cache- and device-efficient structures of arrays where it improves measured performance. Stable IDs shall map through generation-scoped dense indices for kernels, but dense positions shall never be persisted as the sole identity.

Parallel kernels shall partition output domains so that each task writes to an exclusive state slice or a private command buffer. Accumulation into shared targets shall use deterministic segmented reduction, not contended atomics whose order changes results. GPU transfers shall use pinned buffers and overlapping streams where beneficial, with bounded staging memory and explicit backpressure.

## 9.5 Memory management

Each shard shall expose estimates and live measurements for resident state, working buffers, queued events, checkpoint staging and accelerator copies. The executor shall enforce per-brain and per-node budgets. Pools shall be bounded, reused and observable. Queue growth shall trigger credits/backpressure before memory exhaustion.

The system may spill future-tag events or cold checkpoint pages to fast local storage using checksummed segments. It shall not spill current-microstep state in a way that can deadlock the causal critical path. Eviction priority shall prefer rebuildable caches over authoritative state or unacknowledged events.

## 9.6 Time ownership

Remove unconditional self.t += 1 and self.t_ms += dt from biological kernels. Logical time belongs to the brain timeline and is supplied in ExecutionContext. Compatibility display values shall be derived. No nested morphology, bridge or UI code shall independently advance the clock.

# 10. Transport, reliability and backpressure

## 10.1 Transport-independent semantics

MPI, persistent gRPC, burst gRPC, shared memory, RDMA or another transport may implement a route, but none shall define biological semantics. The reliability layer above transports shall supply:

- ordered logical streams with monotonically increasing sequence numbers;

- event and control-frame deduplication;

- durable or recoverable acknowledgements;

- bounded retransmission with reconnect/resume;

- credit-based flow control and receiver-advertised capacity;

- generation, term and fencing validation;

- health metrics and deterministic fault injection for tests.

Switching transport after a timeout shall resume from the last acknowledged sequence. It shall not duplicate a partially sent batch or drop already committed output. A batch is an encoding optimisation only; each event retains identity and tag.

## 10.2 Route ownership and ordering

Each source/destination shard pair shall use a logical StreamId scoped by brain, topology generation and partition generation. Sequence allocation shall be single-writer and recovered from the log. If traffic is split across connections or transports, a merge layer shall restore sequence order before enqueueing.

Events to distinct destination shards may progress independently. Events on one stream shall not be blocked by a large payload on an unrelated stream; use per-route queues and fair multiplexing. Control frames such as fencing and closure shall have reserved capacity so that data saturation cannot prevent progress.

## 10.3 Delivery and commit contract

The recommended minimum is effectively-once application over at-least-once transport:

1\. Source computes and appends an output record to its causal log.

2\. Source sends the event with stable identity and sequence.

3\. Destination validates, durably records receipt or makes it reconstructible, and acknowledges.

4\. Destination applies the event once in canonical tag order.

5\. Source may release retained payload after acknowledgement and checkpoint/log-retention rules permit.

The exact acknowledgement stage shall be documented. It must be strong enough that an active-source failure cannot make a receiver’s accepted watermark untrue.

## 10.4 Batching

Batches shall be bounded by bytes, event count and maximum wait. They shall not combine different brains, incompatible generations or fencing terms. Sorting and compression shall be deterministic. The receiver shall reject a malformed whole batch atomically or identify individually invalid records without applying a prefix ambiguously.

Adaptive batching may respond to link latency and event rate, but shall not delay events past their service-level budget. Same-node routes should use zero-copy or shared-memory channels where safe, while retaining exactly the same envelope and sequence semantics.

## 10.5 Backpressure and overload

Every queue shall have a capacity and an owner. When destination credits are exhausted, the source shall stop scheduling additional non-critical output for that route, prioritise causally blocking work and expose pressure to the scheduler. The scheduler may co-locate shards, migrate, reserve memory, slow wall-clock pacing or propose an adaptive-fidelity decision. Any fidelity change shall already be permitted by brain policy, be versioned, recorded and applied only at its declared future safe tag; queue pressure shall never cause an implicit in-place numerical or biological-model change.

Before accepting an external event or management import, the gateway shall ensure durable capacity or return a typed retryable overload response. Once an event is committed, overload shall not justify silent loss.

# 11. Scheduling, learned workload and heterogeneous resources

## 11.1 Scheduler objective

The scheduler shall maximise useful biological fidelity and aggregate throughput subject to causal correctness, declared responsiveness targets, memory constraints, durability, authorisation quotas and failure-domain policy. Equal contribution means equitable use of **effective capacity**, not equal shard count, equal neurons or equal wall-clock busy percentage.

The scheduler shall remain work-conserving: if eligible work exists and a permitted resource is idle, it shall schedule work, warm a backup, fast-forward a replica, build a checkpoint, prefetch state or perform an approved migration. Background work shall yield to causally blocking active work.

## 11.2 Workload observations

For each brain, shard, component, numerical profile and fidelity depth, collect:

- events received/emitted, fan-out and bytes by route;

- microsteps to quiescence and distribution percentiles;

- synaptic, neuron, morphology, plasticity and growth operations;

- amplification per microstep, residual activity and deferral frequency;

- CPU time, instructions if available, memory bandwidth and cache/NUMA indicators;

- GPU kernel, transfer, occupancy and VRAM usage;

- queue delay, transport wait, acknowledgement latency and time causally blocked;

- checkpoint/log bytes, compression cost and fast-forward rate;

- deadline slack, wall-clock pacing lag and user priority class.

- workstation I/O capture-to-admission and commit-to-presentation latency, jitter-buffer occupancy, codec/transducer CPU/GPU time, dropped-before-admission samples and actuator acknowledgement lag.

Metrics used for a scheduler decision shall be recorded with the decision’s model/version so the choice is explainable and replayable at the policy level.

## 11.3 Predictor interface

The initial implementation shall use a deterministic, explainable baseline behind a trait:

pub trait WorkloadPredictor: Send + Sync {  
fn observe(&mut self, observation: WorkloadObservation);  
fn predict(&self, candidate: &PlacementCandidate) -\> WorkloadPrediction;  
fn version(&self) -\> PredictorVersion;  
fn explain(&self, candidate: &PlacementCandidate) -\> PredictionExplanation;  
}

The baseline should combine exponentially weighted means/variance, bounded histograms or quantile sketches, trend detection and confidence intervals. Prediction shall include expected value, upper confidence bound and uncertainty. A future learned model may be plugged in only after offline evaluation, safe fallback and versioned rollout controls exist.

## 11.4 Deep or amplifying cascade learning

The scheduler shall explicitly learn which SCCs and shards produce deep or amplifying cascades. Signals shall include observed settling depth, early-microstep branching ratio, residual slope, repeated non-convergence, upstream event mix and fidelity depth.

Available actions, selected at a future committed boundary, include:

- increase settling_limit within authorised bounds when spare capacity exists;

- reserve faster CPU/GPU or more memory;

- co-locate strongly communicating components;

- migrate or repartition a hot shard;

- select a validated specialised SCC solver;

- reduce fidelity depth only if the brain’s policy permits it and record the approximation;

- introduce a positive delay only when biologically justified by the model owner; the scheduler shall never fabricate delay merely to make computation easier.

Repeated deferral shall first improve placement, co-location, reserved compute/memory and the future settling_limit within the brain owner’s configured envelope. settling_limit is an execution approximation budget whose value affects whether deferral occurs and therefore shall be logged for replay. fidelity_depth changes the model itself and shall never be reduced merely because a node is busy. An automatic fidelity change is allowed only when the owner has enabled a named adaptive-fidelity policy with bounds, validation evidence, effective tag and visible audit.

## 11.5 Decision protocol

All state-affecting scheduler choices shall be SchedulingDecision records with input metrics/model version, reason, old/new values, effective logical tag, authorising policy and rollback condition. The control plane shall agree and publish them. Executors shall not independently change AARNN fidelity or settling limits based solely on local step time.

Placement-only decisions may take effect through the migration protocol. Numerical/fidelity decisions shall be part of the causal log and checkpoint manifest. UI clients shall show the reason and expected impact.

## 11.6 Fairness across brains and tenants

Use hierarchical weighted fair scheduling:

1\. Reserve safety/control capacity for leases, fencing, acknowledgements and recovery.

2\. Allocate tenant/team shares and enforce hard quotas.

3\. Allocate each brain its configured weight, deadline class and minimum service.

4\. Prioritise causally blocking ready components within a brain.

5\. Borrow idle quota work-conservingly, with prompt revocation when the owner becomes active.

Prevent starvation using age and minimum-share rules. Recovery of one large brain shall not monopolise the fleet unless an authorised emergency priority says so. Report throttling and quota decisions explicitly.

## 11.7 Device placement

GPU placement shall consider kernel suitability, deterministic certification, transfer cost, batching opportunity and VRAM residency. Small irregular SCCs may be faster on CPU. Large dense or vectorisable components may benefit from GPU. Scheduler predictions shall be learned per device and kernel version.

CPU work shall respect NUMA locality and avoid oversubscription by coordinating async runtime workers, Rayon pools, BLAS libraries and GPU driver threads. Configuration shall expose one central concurrency budget rather than independent libraries each assuming all cores.

I/O gateway and transducer work shall be scheduled as a separate latency-sensitive resource class with capped shares. It may use CPU/GPU acceleration and idle capacity, but capture/render callbacks and control-plane safety traffic retain reserved headroom. A workstation becoming an authorised peripheral endpoint does not automatically enrol it as a compute worker; compute enrolment is a separate authenticated capability and placement decision.

# 12. Multiple whole-brains and federation

## 12.1 Independent brain domains

The emulator shall run many brains concurrently. Each BrainId owns its own:

- logical clock and base biological quantum;

- topology and partition generations;

- event routes, watermarks, component settlement and GVT estimate;

- numerical profile, fidelity policy and deterministic seed space;

- shard placements, lease terms, checkpoints and causal logs;

- quotas, priority, authorisation policy and retention configuration;

- external inputs, output commitments and audit history.

- peripheral bindings, clock mappings, consent/retention policy and active actuator leases.

No global static runner, seed, timestep, network registry lock or implicit “active network” may affect all brains. APIs shall require BrainId or a scoped resource URI. Metrics shall be cardinality-controlled but brain-scoped.

A peripheral channel shall bind to exactly one BrainId and one binding generation at a time unless an explicit federation fan-out policy names every destination. Reusing a browser media track, native device handle, effect dedupe window or transducer mutable state across brains is prohibited. Multiple users may manage distinct authorised brains concurrently without seeing, hearing or controlling one another’s channels.

## 12.2 Federation

A federation links selected outputs of one brain to authorised inputs of another. It shall use explicit FederationLink configuration containing source/target identities, event schema mapping, time-domain mapping, allowed lateness, buffering, backpressure, security policy and failure policy.

Federated brains retain independent clocks. The link maps a committed source tag to a target-eligible tag; it shall not force both brains into a common microstep or barrier. Cycles across brains are permitted only if mapping guarantees positive effective delay or if an explicitly engineered distributed SCC protocol spans the link. The default shall reject zero-delay federation cycles.

## 12.3 Federation failure policy

Each link shall declare whether source interruption causes target buffering, degraded input, pause of only the dependent target region, or failure. Missing input shall be represented by an explicit model event or state, not silently treated as zero unless that is the declared rule.

Cross-tenant federation requires authorisation by both resource owners and shall be auditable. Revocation shall take effect at a safe tag and close streams cleanly.

## 12.4 Clone and branch

Users with permission may clone a brain from an immutable checkpoint. The new brain receives a new BrainId, its own clock policy and explicit seed choice. Checkpoint data may be copy-on-write/content-addressed. A clone shall never share mutable shard state, event streams, leases or output deduplication state with its source.

# 13. Growth, plasticity and topology transactions

## 13.1 Stable biological identity

Growth shall allocate stable neuron and synapse IDs from deterministic, brain-scoped namespaces. Deletion shall tombstone IDs for replay and audit. Vector compaction may remap dense indices within a generation but shall retain an ID map in checkpoints and events.

Released events shall refer to SynapseId, not an index into the current synapse vector. Any current limit used only for visualisation, such as retaining a small released-event window, shall be separate from the authoritative causal event queue.

## 13.2 Authoritative topology mutation

Morphology and growth shall not execute independently on replicated full runners. Proposed changes shall become a deterministic TopologyTransaction containing precondition generation, additions/removals/updates, deterministic provenance, expected resource impact and requested effective tag.

The topology service shall validate biological constraints, stable-ID uniqueness, quota/capacity, delay lower bounds and partition consequences. It then publishes a new immutable topology generation effective only after affected components reach a safe committed boundary.

## 13.3 SCC and route recomputation

Every topology transaction shall calculate the affected zero-delay graph incrementally where safe or fully where necessary. If SCC membership changes, component IDs, routes and expected-producer sets for the new generation shall be published atomically with a transition plan. In-flight events from the old generation shall drain, translate by an explicit rule or be rejected and replayed; they shall not enter the new topology ambiguously.

## 13.4 Growth and capacity

Growth admission shall reserve predicted active and replica memory, checkpoint/log growth and route capacity before commit. If capacity is unavailable, the biological policy shall receive a typed constrained-growth result at a logical boundary. The system shall not partially create a topology that only some replicas can store.

The scheduler shall anticipate growth trends and pre-place capacity or repartition. User interfaces shall show growth pressure, rejected proposals and their biological/operational consequences.

# 14. Checkpoints, causal log and durability

## 14.1 Recommended durability arrangement

Every active shard shall have:

- one fenced active writer;

- at least one warm backup in another eligible failure domain receiving causal write-ahead log entries synchronously before durable commit;

- periodic immutable shard checkpoints written asynchronously;

- optionally, a second cold checkpoint/log copy in a separate storage or geographic failure domain according to policy.

The warm backup may lag in **applied state** while still holding all records required to catch up. Track durable_log_tag separately from applied_tag and expose both. A policy shall define the maximum permitted lag and what happens if no synchronous backup is available. The default shall pause affected commits rather than silently reduce durability.

![An active shard synchronously replicates its causal log to a warm backup, writes periodic immutable checkpoints asynchronously, and uses fencing plus deterministic replay for failover.](media/image3.png "Recommended durability arrangement")

**Figure 3. Recommended durability arrangement.** An active shard synchronously replicates its causal log to a warm backup, writes periodic immutable checkpoints asynchronously, and uses fencing plus deterministic replay for failover.

## 14.2 Causal write-ahead log

The append-only log shall contain enough information to reproduce committed state and outputs:

- accepted input and federation events with canonical identities/tags;

- deterministic state-transition or command records as selected by the persistence design;

- outgoing event commitments and acknowledgement positions;

- watermarks and closure-relevant stream state;

- topology/growth transactions and effective generations;

- fidelity, settling-limit, scheduler and numerical-profile decisions;

- non-convergence records and deferred_from_nonconvergence events;

- deterministic RNG coordinates or inputs where reconstruction requires them;

- lease/ownership transitions, migration cutovers and failover discontinuities;

- external output commitment/deduplication records.

- admitted peripheral sample metadata, mapping/transducer version and either encrypted content reference or an explicit non-replayable-live-input marker; raw camera, microphone, screen or HID payload retention follows consent and data-classification policy.

- actuator-intent commit, delivery acknowledgement and dedupe horizon, without storing workstation credentials or ephemeral media keys.

Segments shall have versioned headers, checksums and a hash chain or Merkle manifest. Corruption shall fail closed, identify the segment and attempt another replica; it shall never be skipped silently.

## 14.3 Immutable checkpoint contents

A shard checkpoint shall include:

- brain/shard identity, topology and partition generations;

- committed logical tag, lease term of the writer and causal-log resume position;

- numerical profile, kernel/model/config versions and time-base metadata;

- canonical biological state, stable-to-dense ID maps and local route state;

- future event queue, deduplication windows and producer/watermark state;

- non-convergence deferral metadata;

- state digests, object checksums and manifest references;

- compatibility/schema version and required migration identifiers.

Checkpoint objects are immutable. Write to a new temporary object, verify checksum, atomically publish the manifest and then make it discoverable. Never overwrite a previously committed checkpoint name.

## 14.4 Checkpoint cadence

Checkpoint triggers shall include biological-tick interval, wall-clock interval, log bytes, predicted recovery time, topology change, management request and pre-migration. The scheduler may stagger background checkpoints to avoid fleet-wide I/O bursts. It shall prioritise recovery-point objectives while yielding CPU, network and storage bandwidth to causal critical paths.

Incremental/delta checkpoints may be used, but restoration depth shall be bounded by periodic full checkpoints or compaction. A compaction produces a new immutable object and manifest; it never mutates old objects.

## 14.5 Consistent brain-level export

An export of a running brain shall select a causally consistent cut at or behind an asynchronously computed GVT/safe tag. Each shard produces or references an immutable checkpoint at that cut plus the required log tail and in-transit channel state. The implementation shall use a documented distributed-snapshot/consistent-cut algorithm; selecting independently “latest” shard checkpoints is insufficient. The export manifest records all shard digests, route positions and any peripheral-input replay limitations. The running brain need not stop globally; only bounded per-shard snapshot mechanics may pause local commit.

Exports shall not serialise a mutable in-memory view while execution continues. If the user requests “latest”, the operation shall report the exact exported tag and any lag from the current frontier.

## 14.6 Retention and deletion

Retention shall be policy-driven by tenant, brain and data classification. A checkpoint may be deleted only when no active restore, clone, export, replica or retained log chain references it. Deleting a brain shall first create a tombstone, revoke leases, halt external output and enter a recoverable retention period unless an authorised purge is explicitly requested and confirmed.

All destructive deletion shall be asynchronous, idempotent and audited. The UI shall distinguish “remove from active service”, “delete after retention” and irreversible “purge”.

# 15. Failure, recovery, migration and split-brain prevention

## 15.1 Control-plane quorum and leases

Use a three-member quorum for a small deployment and five members where failure-domain requirements justify it. Consensus shall store membership, placement generations, active lease terms, fencing tokens and operation state. A singleton orchestrator with heartbeat-based deletion is not sufficient for safe ownership.

An active shard lease shall be renewable, bounded and tied to a monotonically increasing term/fencing token. Every event, log append, checkpoint publication and externally visible output shall carry or be validated against the active token. Receivers and gateways shall reject stale tokens even if an old node continues running.

## 15.2 Node lifecycle

The control plane shall model explicit states:

- A node enters Joining, progresses to Benchmarking, and becomes Healthy only after compatibility, capability and safety checks pass.

- A healthy node may enter Draining for controlled maintenance or migration.

- Failed health evidence moves a node from Healthy to Suspect; quorum and lease evidence may then classify it as Failed.

- A returned or resynchronising node enters Recovering, then becomes Healthy only after fast-forward and validation.

- Corruption, protocol violation or digest mismatch moves the node to Quarantined until policy or an operator clears it.

Heartbeats shall report health, leases, resource use, protocol versions, checkpoint/log progress and clock diagnostics. A missed heartbeat first creates Suspect; it shall not immediately erase placement metadata. Failure decisions shall use quorum-observed lease expiry and configured evidence.

## 15.3 Failover procedure

On active-node failure:

1\. Mark the node/shard suspect and stop assigning new work.

2\. Allow or force lease expiry and commit revocation through quorum.

3\. Fence the old term and tell routers/output gateways to reject it.

4\. Stall only causal regions that depend on the affected shard; other brains and components continue.

5\. Select the most suitable warm backup using durable-log completeness, applied lag, capacity and failure domain.

6\. Load the latest valid immutable checkpoint if necessary.

7\. Replay the canonical log at maximum safe speed without wall-clock pacing, rendering or optional telemetry.

8\. Validate expected intermediate/final state digests and stream positions.

9\. Issue a new lease term/fencing token and placement generation.

10\. Resume routes from acknowledged sequences and advertise watermarks only after reconstruction is sound.

11\. Record and expose a FailoverDiscontinuity.

## 15.4 Fast-forward mode

Fast-forward shall use the recorded numerical profile, fidelity depth and scheduler decisions from the log. It may use more resources and batch future work, but shall not change semantics. In deterministic-reference mode its final digest must match the expected digest exactly. In fast-biological mode it shall meet the documented tolerance and emit a comparison report.

Fast-forward shall suppress duplicate external side effects. Output records already committed by the failed active are reconstructed as committed but not emitted again. Gateways use event/output IDs and fencing terms for deduplication.

Peripheral sessions may remain connected while a shard fails, but their behaviour shall be explicit. Sensory channels use a bounded gateway buffer; on overflow they reject/drop **before admission** according to the declared sampling policy and report the gap. Actuator channels retain their last acknowledged EffectId, reject old fencing terms and resume only from a new committed output cursor. Clock mapping is revalidated after a workstation or gateway reconnect; a mapping discontinuity creates a new mapping version rather than rewriting earlier capture tags.

## 15.5 Failover discontinuity

The record shall contain old/new node, term and placement generation; last known committed, durable and applied tags; replay interval; estimated lost uncommitted work; observed interruption; upstream retransmission interval; reason/evidence; state digest result; and whether any model-visible glitch occurred. The user interfaces and audit API shall make this acknowledgement visible without implying that a whole brain was destroyed.

## 15.6 Recovered nodes

A recovered former active enters Recovering. It shall not resume old work. It clears or quarantines stale in-memory queues, obtains current consensus state, validates software compatibility, downloads checkpoint/log data and rejoins as a backup or new active only through a new placement plan. All messages from its old term remain fenced.

## 15.7 Split-brain avoidance

Network isolation can leave a process alive but unauthorised. Safety relies on quorum leases and receiver/output fencing, not on trusting the isolated process to stop. Workers shall accept management/control connections only from authenticated control-plane identities. Data-plane peers shall cache current/next valid terms and refresh on typed stale-route responses.

If quorum is unavailable, do not promote a new active. A configurable availability-over-consistency mode is outside this baseline and shall not be added without a separate hazard analysis.

## 15.8 Live migration and rebalancing

Migration shall use:

1\. Validate destination eligibility and reserve resources.

2\. Create/reference an immutable source checkpoint.

3\. Transfer checkpoint pages and verify digests.

4\. Stream the causal log while the source remains active.

5\. Fast-forward destination to a bounded lag.

6\. Select a committed cutover tag and briefly fence new source commits at that boundary.

7\. Drain/transfer remaining events, validate digests and issue a new term/generation.

8\. Redirect routes atomically and resume.

9\. Retain the old source as a backup or release it only after durability policy is restored.

Migrations shall be cancellable before cutover and recoverable after any crash. The scheduler shall rate-limit concurrent migrations by CPU, network and storage budgets.

## 15.9 Replica anti-affinity and graceful degradation

An active and its only warm backup shall not share a compute node, power/failure domain or storage object where labels are available. Placement shall spread replicas while accounting for latency. When capacity is constrained, the system should reduce backup warmth, optional cold-copy cadence or non-critical cache before reducing active biological fidelity. It shall report any unmet redundancy; it shall not silently claim the configured level.

# 16. Remote management, workstation I/O, security and both user interfaces

## 16.1 Management principle

The web UI and Rust UI shall be interchangeable remote clients of the orchestrator management plane. Either shall run on any authorised workstation, discover or accept one or more orchestrator endpoints, authenticate, list permitted resources and manage any authorised whole-brain emulation. Neither UI shall require the selected brain to run locally. With separate peripheral permissions and local consent, either may also attach that workstation’s supported audio, visual, keyboard, pointer and bidirectional USB AER channels to the selected brain. USB AER shall be usable at the same time as the other active modalities rather than replacing them.

Both clients shall use the same versioned OpenAPI/gRPC contracts and preferably generated clients from a shared schema. Biological/model validation, authorisation, idempotency and state transitions belong on the server. Clients may hide unavailable actions for usability, but a forged request must still be denied.

![Management authorises a peripheral session; a separate secure media/data plane maps captured workstation streams to sensory events and committed neural outputs to bounded, deduplicated actuator effects.](media/image4.png "Authorised workstation I/O")

**Figure 4. Authorised workstation I/O.** Management authorises a peripheral session; a separate secure media/data plane maps captured workstation streams to sensory events and committed neural outputs to bounded, deduplicated actuator effects.

## 16.2 Resource hierarchy

The management API shall model resources rather than a single global active network:

organisation / team / project  
federation  
brain  
topology generation  
shard placement and replicas  
checkpoint / export / import  
operation / audit event / metric stream
peripheral session / local device / channel / binding / actuator lease

Every list and watch operation shall be scope-filtered server-side. Identifiers in URLs and gRPC requests shall be checked against the authenticated principal; possession of an ID is not authorisation.

## 16.3 Required operations

| **Operation**                      | **Expected semantics**                                                                                                         | **Minimum permission**                                        |
|------------------------------------|--------------------------------------------------------------------------------------------------------------------------------|---------------------------------------------------------------|
| Create/add brain                   | Validate model, quotas, durability and initial placement; return an asynchronous operation.                                    | brain.create in project/team.                                 |
| Start/resume                       | Acquire placements and advance from committed state.                                                                           | brain.run.                                                    |
| Pause/stop                         | Stop at a safe committed boundary and retain state.                                                                            | brain.control.                                                |
| Restart                            | Fail-safe stop then resume from latest committed state; not a reset.                                                           | brain.control.                                                |
| Reset                              | Create a checkpoint if policy requires, then restore configured baseline as a new generation.                                  | brain.reset; elevated confirmation.                           |
| Repeat                             | Explicitly define whether this means reset-to-baseline-and-run; use a named server operation, not ambiguous UI-only behaviour. | brain.reset and brain.run.                                    |
| Import                             | Upload/stage, scan, validate, report differences, authorise and commit at a safe boundary.                                     | brain.import.                                                 |
| Export                             | Build an immutable consistent-cut manifest and downloadable artefact.                                                          | brain.export; possible data-classification condition.         |
| Clone                              | Create a new brain from an immutable checkpoint with seed policy.                                                              | brain.create plus source brain.read_checkpoint.               |
| Remove/delete                      | Tombstone, stop/fence, retain and later purge according to policy.                                                             | brain.delete; elevated confirmation.                          |
| Checkpoint/restore                 | Create or restore immutable state at an explicit tag.                                                                          | checkpoint.create or checkpoint.restore.                      |
| Federate/unfederate                | Create or close an authorised explicit link.                                                                                   | federation.manage on both sides.                              |
| Rebalance/migrate                  | Request scheduler analysis or a constrained placement change.                                                                  | placement.manage.                                             |
| Fail over                          | Request controlled promotion where safety preconditions hold.                                                                  | failover.manage; emergency audit.                             |
| Change fidelity/settling policy    | Version and apply at a future safe tag.                                                                                        | brain.configure_fidelity.                                     |
| Create/renew peripheral session    | Bind declared workstation channels, including concurrent USB AER input/output, to one authorised brain with expiry, capability grant and local consent. | io.session.create plus per-channel and local-device permission. |
| Bind/unbind transducer or actuator | Version the stable receptor/effector mapping and apply at a safe tag.                                                          | io.binding.manage.                                            |
| Arm/disarm actuator                | Acquire/release the sole actuator lease for a channel; default disarmed.                                                       | io.actuator.arm; step-up/local confirmation for OS-level HID. |
| Start/stop recording               | Retain encrypted source/effect content for deterministic replay according to classification and consent.                       | io.record; separate from live use.                            |

All mutating operations shall accept an idempotency key, request ID, expected resource version and optional dry-run. Long operations return 202 Accepted or a gRPC long-running operation reference. The client watches progress; it shall not hold a request open for minutes.

## 16.4 Authentication

Production web deployments shall use secure OIDC/OAuth session exchange with HTTP-only, secure, same-site cookies, CSRF protection and short-lived backend sessions. The Rust UI shall support OIDC device authorisation or system-browser PKCE, short-lived access tokens, refresh without exposing tokens in logs, and optional mTLS for managed workstations.

All gRPC and HTTP connections shall use TLS in production, validate server identity and support configured certificate authorities. Raw unauthenticated gRPC from a workstation to workers shall be removed or disabled by default. Local-development authentication bypass shall require an explicit non-production build/configuration and display a persistent warning.

## 16.5 Authorisation

Use default-deny RBAC combined with resource attributes where needed. The existing none/request/observe/use/control presentation levels may remain as coarse UI groupings, but the server shall evaluate granular permissions per organisation, team, project, federation, brain and action.

Policy decisions shall consider principal, group/role, resource owner, operation, environment, data classification and optional conditions such as maintenance windows or approval. Destructive or cross-tenant actions may require step-up authentication or dual approval. Denials shall return a safe reason code without leaking resources the principal cannot observe.

service-access.js fallback calculations are not authoritative. In authMode === "none", public production endpoints shall not automatically gain control. Server middleware/interceptors shall authorise every request, including streaming subscriptions and generated download URLs.

Peripheral permissions shall be orthogonal to brain.control. At minimum distinguish session creation, microphone capture, camera capture, display capture, focused keyboard input, pointer input, USB device access, USB AER input, USB AER output, audio presentation, video/canvas presentation, in-application keyboard/pointer output, native/global HID actuation and raw-stream recording. A user authorised to observe or manage a brain is not automatically authorised to expose workstation devices or permit that brain to actuate the operating system or a USB peripheral. AER output requires a committed output binding and cannot be inferred from permission to read AER input.

## 16.6 Optimistic concurrency and multiple workstations

Every mutable resource shall expose a monotonically increasing resource_version or ETag. Update requests carry expected_version; a mismatch returns a conflict containing the current version and a safe summary. The clients shall offer reload, compare and intentionally retry. Silent last-write-wins is prohibited.

Idempotency records shall be scoped to principal, resource and operation and retained for a documented period. Repeating a request with the same key and same body returns the same OperationId; a different body returns a conflict. This protects against double-clicks, retries and workstation reconnects.

## 16.7 Operation resource

An operation shall expose:

- operation ID, type, target resource and initiating principal/workstation;

- requested and accepted time, expected resource version and idempotency key digest;

- state: queued, validating, waiting-for-safe-tag, running, cancelling, succeeded, failed or rolled-back;

- progress phase/percentage where meaningful and a human-readable British-English explanation;

- safe cancellation support and point-of-no-return;

- structured error, retryability, resulting resource version and audit references.

Operation execution shall be resumable after orchestrator failover. A new leader reconstructs state from consensus/durable operation records and does not duplicate side effects.

## 16.8 Status and live data

Provide filtered streams using WebSocket/SSE for web and server-streaming gRPC for Rust, with a common event model. Subscriptions shall specify brain IDs, event types, metric sampling and maximum rate. Streams shall use sequence cursors, resumable reconnect, heartbeat and backpressure.

The server should send deltas and summaries, not frequent full network snapshots. Large topology/state views shall use paginated/chunked snapshot endpoints with a consistent version. UI frame rate shall not drive emulator sampling. Slow clients may skip non-authoritative visual frames but shall not lose operation/audit/failure notifications; distinguish stream classes.

These management/status streams shall not carry raw camera, microphone, high-rate screen or USB AER event content. Peripheral media and AER payloads use the dedicated I/O media/data plane and their own congestion, jitter and privacy policy. Keyboard/pointer samples, sequenced AER frames and actuator effects may use a low-latency reliable or partially reliable data channel as configured, while causal admission and committed effect records remain durable server-side.

## 16.9 Dangerous-operation experience

Reset, restore, delete, purge, force failover, federation change and fidelity reduction shall show scope, current version, target tag, durability consequence and whether external output may be interrupted. Confirmation shall include the brain’s human name and stable short ID. Purge or cross-tenant operations may require re-authentication.

The server shall still validate confirmation tokens; a client-side modal alone is insufficient. All outcomes, including denials and cancellations, shall be audited.

## 16.10 Import workflow

Imports shall be staged and non-blocking:

1\. Client obtains an upload session and streams the file with size/checksum.

2\. Server stores it in quarantine and scans/parses with resource limits.

3\. A validator reports schema, biological, numerical-profile, ID, topology, quota, security and compatibility findings.

4\. The user reviews a diff and chooses replace, new brain, clone/branch or supported merge semantics.

5\. Server reserves resources and chooses a safe effective tag.

6\. Commit creates new immutable manifests/generations and preserves the old baseline for rollback according to policy.

Untrusted import parsing shall run in a sandboxed process with CPU, memory, time and decompression limits. Never execute embedded scripts. JSON and archive nesting/decompression bombs shall be rejected safely.

## 16.11 Export workflow

Exports shall be generated server-side from an immutable consistent cut. Supported formats shall have explicit loss/approximation reports. A download shall use a short-lived, principal-bound URL or authenticated stream with checksum. The audit log records tag, manifest digest, format, exporter version and recipient principal.

The Rust UI’s existing local Python/tool integration may remain for standalone conversion, but remote exports shall be orchestrator operations. The web and Rust UI shall show the same formats and limitations derived from API capability discovery.

## 16.12 Web UI changes

app.js, index.html, service-access.js, shell.js and swagger.html shall be reorganised into modular, testable components or modules rather than extending one large script. Required changes include:

- replace direct /api/update_network and /api/control_network calls with generated/versioned management client methods;

- propagate authentication, request IDs, idempotency keys and ETags automatically;

- replace polling loops for high-frequency status with resumable streams and bounded fallback polling;

- expose orchestrator connection/health, selected organisation/team/project/federation/brain and current resource version;

- add operation centre views for import/export/reset/restart/checkpoint/restore/migration/failover;

- expose active/warm/cold shard placement, replica lag, non-convergence and durability status without leaking unauthorised resources;

- implement accessible keyboard navigation, focus management, status announcements and colour-independent warnings;

- centralise error mapping and avoid optimistic mutation before server acceptance; show pending intent distinctly;

- retain OpenAPI documentation only for authorised developer roles and protect “try it” with the same policy.

- add a peripheral-session panel that selects one brain, enumerates only browser-exposed devices, requests microphone/camera/screen/pointer and supported WebUSB AER permissions through explicit user gestures, shows every concurrently active channel and device epoch, and provides per-channel plus always-visible stop controls. Where WebUSB is unavailable, integrate only with an authenticated, origin-bound local USB companion exposing the narrow AER protocol—not a general device proxy.

Client modules shall have unit tests using a mock generated client, stream reconnect tests and browser integration tests. DOM code shall not contain policy logic beyond presentation.

## 16.13 Rust UI changes

ui.rs shall be decomposed so rendering, local standalone execution, remote management clients, operation state, file conversion and live subscriptions are separate modules. Required changes include:

- replace DistributedNeuromorphicClient::connect to worker addresses with an authenticated orchestrator-management channel;

- apply connect/request deadlines, exponential backoff with jitter, TLS validation and token refresh;

- remove direct fan-out of control updates to worker nodes from apply_cluster_control;

- stop treating locally cached optimistic state as authoritative; display pending operation then streamed committed state;

- support remote add, remove, import, export, start, stop, restart, reset, repeat, checkpoint, restore, clone, federation, migration and failover according to capabilities and permissions;

- move blocking filesystem, conversion and network work off the UI/render thread using bounded task executors;

- stream summarised activity and fetch topology chunks on demand instead of repeatedly copying full snapshots;

- store tokens only in an operating-system credential mechanism where available and redact them from diagnostics;

- provide connection profiles for multiple orchestrators and a clear scope switcher without mixing resources;

- retain a deliberate standalone mode, visibly separate from remote production mode.

- add native peripheral adapters behind traits for microphone/camera/screen, focused keyboard/pointer input, bidirectional USB AER, audio/video rendering and separately privileged optional virtual-HID output; never perform USB/device callbacks or OS injection on the render thread.

Remote and standalone code paths shall share model/view types where appropriate but shall not share mutable runner locks with network client tasks.

## 16.14 Audit

Every management request shall create an append-only audit event including actor, authenticated subject, groups/role snapshot, organisation/team/project, workstation/client ID and version, request/operation ID, target, action, old/new version, decision policy/version, result and timestamp. Sensitive payloads/tokens shall be redacted; content digests and immutable object references may be stored.

Audit access is separately authorised. Clocks used for audit are wall-clock with synchronisation diagnostics; causal state changes also record logical tags.

## 16.15 Workstation I/O capability tiers

The workstation shall expose only capabilities that its platform can safely provide. The orchestrator advertises the requested channel; the client reports actual capability and the user grants it locally. Unsupported capability is a typed result, never simulated silently.

| **Capability tier**                            | **Web UI**                                                                                                                                            | **Rust desktop UI**                                                                                                 | **Required boundary**                                                                      |
|------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------------------|
| Microphone and camera sensory input            | Browser media-capture APIs in a secure context with user permission.                                                                                  | Native capture adapters with operating-system permission.                                                           | Encoded/raw frames enter the I/O gateway, then a versioned transducer.                     |
| Screen/window sensory input                    | Explicit browser display-capture prompt; the page cannot choose a screen secretly.                                                                    | Native screen/window capture under OS permission and allow-list.                                                    | Capture surface identity and privacy classification are session metadata.                  |
| Keyboard and pointer sensory input             | Events only while the page/authorised element has focus; pointer lock may provide relative motion after user gesture. No claim of global key capture. | Focused input by default; optional global capture only as a separately privileged, visible capability.              | Normalised HID samples are sequenced and mapped; raw secrets/password fields are excluded. |
| Bidirectional USB AER exchange                 | WebUSB only in a supported secure browser after an explicit device-selection gesture; otherwise an authenticated origin-bound local companion with a narrow AER interface. | Native asynchronous USB adapter with OS/device permission, descriptor allow-list and hot-plug handling.             | Independent AER input/output channels, device epoch, sequencing, clock provenance, bounded transfers and no direct worker access. |
| Audio and video/canvas output                  | Browser playback/rendering in the authorised page, subject to autoplay and focus rules.                                                               | Native playback/rendering in an isolated window/device.                                                             | Only committed actuator intents are presented.                                             |
| Keyboard/pointer output inside the emulator UI | Dispatch to a sandboxed virtual environment/canvas, never synthetic trusted browser/OS events.                                                        | Dispatch to an in-application virtual environment.                                                                  | Default safe actuator tier.                                                                |
| Native/global keyboard or pointer actuation    | Not supported by an ordinary browser and shall be reported as unavailable.                                                                            | Optional OS-specific virtual-HID/accessibility adapter with least privilege, allow-list, arming and emergency stop. | Highest-risk tier; separate permission and actuator lease.                                 |

## 16.16 Peripheral session and binding model

A PeripheralSession is an ephemeral leased resource beneath a brain, not a property of the UI process. It shall include principal/workstation identity, selected BrainId, session expiry, capability grants, client/server protocol versions, negotiated media codecs and USB AER protocol capabilities, channel/local-device list, consent state, retention policy and current mapping/binding/device generations. Closing the browser, signing out, losing authorisation or pressing the local stop control revokes the session, closes local USB handles and disarms actuators.

Each PeripheralChannel has one direction and modality: audio input, video/camera input, display input, keyboard input, pointer input, USB AER input, audio output, video/canvas output, USB AER output, virtual keyboard output or virtual pointer output. One PeripheralSession may keep all authorised modalities active simultaneously. Every channel has an independent identifier, sequence space, clock mapping, queue/credit budget, cancellation token, health state and quality stream; a shared session must not collapse them into one clock or FIFO. Input fan-in from multiple workstations is permitted only through a declared mixer/arbitration policy. Output fan-out is permitted for passive audio/video observers, but an effectful actuator or USB AER output channel has exactly one fenced ActuatorLease at a time.

A LocalDeviceBinding associates a user-selected USB device with AER input/output channels using an allow-listed vendor/product identifier, optional serial-number hash, protocol/firmware version, endpoint descriptors and a monotonically changing DeviceEpoch. Raw serial numbers and unrelated USB interfaces are not exposed to the brain or ordinary logs. Disconnect, reset, re-enumeration, firmware change or endpoint renegotiation closes the old epoch; reconnection requires capability validation and cannot silently inherit an old clock mapping or output lease.

An InputBinding maps a channel through a versioned transducer to stable receptor/neuron/region IDs. An OutputBinding maps stable effector/output IDs through a versioned decoder to an actuator channel. Bindings specify units, sample rate/frame rate, normalisation, feature extraction, spike/event encoding, calibration, missing-sample policy, quality flags and effective logical tag. They shall never depend solely on mutable vector positions. Binding changes are topology/model transactions at safe boundaries and appear in checkpoints and audit.

## 16.17 Sensory input pipeline

The required pipeline is:

1\. Capture on the workstation with PeripheralSampleId, channel sequence, monotonic capture timestamp, duration and format.

2\. Apply only local transformations necessary for privacy or transport; label lossy conversion, gain control, resampling, cropping and compression.

3\. Send through the secure media/data session to the I/O gateway. Gateway receipt time is telemetry, not biological time.

4\. Validate session lease, brain/binding generation, sequence, limits and consent; deduplicate retransmission.

5\. Apply the pinned transducer version in a bounded CPU/GPU worker, producing stable sensory events plus quality/provenance.

6\. Map capture time to a brain-eligible tag using the current clock model and late-input policy.

7\. Admit the event durably to the brain input log/queue. After admission it follows normal causal ordering and shall not be dropped for congestion.

Raw and derived representations shall be distinct. For example, a video frame may be transformed into event-camera/AER events, visual features or receptor currents; a USB AER address/polarity/timestamp tuple may map through an address table to stable receptors; audio may become waveform blocks, spectral features or spikes. The biological model owner chooses and versions that mapping. The emulator shall not imply that a codec frame or raw USB packet is itself an admitted biological event.

Backpressure operates before causal admission. Audio/video may be resampled, coalesced or dropped according to a declared policy when a bounded jitter/transducer queue is full; every gap is counted and represented by a SensoryGap or quality flag. Keyboard button transitions and pointer button transitions default to reliable ordered delivery; high-rate pointer motion may be coalesced while preserving total displacement and interval. USB AER uses bounded transfer pools and a declared lossless-or-gap policy: sequence loss, device FIFO overflow, malformed frame, endpoint stall or host overrun creates explicit AER gap/quality evidence and never silently fabricates events. Fair per-channel scheduling and reserved management/safety capacity prevent a saturated video, HID or AER channel from starving another. No policy may invent missing key-up/button-up or AER events silently.

## 16.18 External clocks, pacing and replay

Live I/O involves at least three clocks: workstation monotonic capture time, gateway monotonic receipt time and brain logical time; a USB AER device may add its own monotonic device clock. Wall-clock UTC is retained for audit only. A TimeDomainMapping shall estimate offset, drift and uncertainty using bounded clock-sync observations and record a new ClockMappingVersion after sleep/resume, device reset, route change or material clock discontinuity. Prefer a validated device-provided event timestamp. If the AER device supplies none, timestamp at the host USB read/completion boundary, identify that source explicitly and include transfer/buffering uncertainty; packet receipt at the gateway remains non-authoritative.

The brain selects one named admission mode:

- RealTimeBuffered: map capture time to a future eligible tag with a configured playout/sensory buffer; late samples follow the declared reject, gap or next-quantum policy.

- BestEffortLive: favour responsiveness with bounded pre-admission resampling/drop and explicit quality metadata; committed biological events remain exact.

- RecordedReplay: read immutable recorded samples and the original/pinned mapping/transducer versions; pacing may be disabled and deterministic-reference digests shall be reproducible.

- OfflineDataset: map dataset timestamps directly through a versioned epoch/scale without pretending the workstation is live.

The scheduler may slow biological pacing, allocate faster resources or reduce an authorised sensory sampling rate. It shall not use media arrival order to reorder causal events or silently alter neural fidelity. End-to-end latency and mapping uncertainty are observable separately from logical correctness.

## 16.19 Neural output and actuator semantics

Neural output becomes an ActuatorIntent only after the producing shard state and output record are committed. The intent shall carry EffectId, brain/binding generation, source logical tag, output quality (Converged, ProvisionalNonConvergent, DegradedInput), presentation deadline, payload/format and safety class. The actuator gateway checks authorisation, lease, fence and policy before delivery.

Exactly-once physical side effects cannot be inferred from network acknowledgement alone. The required contract is at-most-once client application within a retained dedupe horizon, with explicit states Committed, Delivered, Applied, Rejected, Expired and UnknownAfterDisconnect. The client persists recent EffectId values where appropriate, applies an effect only under the current actuator lease, and acknowledges after the local presentation/injection decision. Replay and fast-forward reconstruct output state but do not re-emit an already committed effect.

Audio/video output may drop an expired presentation frame rather than increase latency; the committed effect remains observable as Expired. Stateful decoders receive keyframes/resynchronisation after loss. USB AER output is produced only from a committed OutputBinding, uses stable EffectId/frame sequence and allow-listed address ranges, and is acknowledged after the adapter's declared local write/acceptance boundary. If disconnect makes device application unknowable, the state is UnknownAfterDisconnect rather than automatically retransmitted under a new device epoch. Keyboard/pointer actuation uses a state machine that guarantees release of held keys/buttons on disarm, disconnect, focus/target loss or emergency stop. Rate, coordinate region, target application/window and allowed key/button sets are bounded. Text, clipboard, shell commands and credential fields are denied by default.

ProvisionalNonConvergent output is suppressed by default for native/global HID and other high-risk actuators. A model owner may permit it for low-risk audio/video or sandboxed virtual environments through a named policy that exposes the quality flag to the workstation.

## 16.20 Media and data transport

Use the management API only for session creation, capability grants, signalling, bindings, audit and status. Use WebRTC-compatible secure media transport for interactive audio/video where available, including congestion control, encrypted media and NAT traversal/relay. Use secure data channels or an equivalent low-latency protocol for sequenced keyboard/pointer samples, USB AER frames, effect commands and acknowledgements. A deployment may use a gateway/SFU/relay, but workers shall not accept public workstation connections.

Transport selection shall be behind PeripheralTransport traits. The causal log stores admitted sensory events and committed effects, not every lossy network packet. Reconnect resumes from channel/effect cursors, creates a new media epoch when codec state requires it, and never reuses keys or leases from an expired session. TURN/relay credentials shall be short-lived and brain/session scoped.

Local USB access shall sit behind a separate UsbAerDevice trait and shall not be treated as the remote transport. The adapter shall negotiate protocol/firmware, endpoint type and maximum frame/event rate; validate framing, length, address range, sequence and CRC/checksum where available; use bounded asynchronous transfers; and support cancellation-safe close. USB control transfers are limited to the documented AER protocol and configuration allow-list. The workstation multiplexer forwards normalised AER frames through the governed peripheral data channel while concurrently servicing A/V/HID.

## 16.21 Web UI implementation requirements

The web client shall require HTTPS/secure context for capture. Device labels remain hidden until permission is granted; permission denial/revocation is a normal state. getUserMedia supplies microphone/camera, display capture supplies a user-selected surface, UI/Pointer Events provide focused input, and Pointer Lock may supply relative mouse movement after a gesture. Where supported, WebUSB may expose only a user-selected allow-listed AER device after an explicit gesture. Where unsupported, the web UI may use an authenticated, origin-bound, signed local companion exposing only the versioned AER operations; it shall report the capability unavailable if neither path exists. Browser capability detection shall drive the UI; experimental or unavailable APIs shall not be assumed.

Capture, USB transfer handling, encoding, decoding and feature-preparation work shall use browser workers/worklets or the local companion where appropriate and bounded transferable buffers; the main thread remains responsive. A visible session bar shall show brain name/short ID, every live capture/AER source, device epoch, every armed output, latency/quality and a one-action stop/disarm control. It shall also allow the AER channel to be stopped/reconnected without stopping A/V/HID. Navigating away or losing the authenticated session closes the peripheral session and local USB handle. Browser code shall never attempt to manufacture trusted OS keyboard/mouse events or claim global device control.

## 16.22 Rust UI implementation requirements

The native client shall provide the same session/binding workflow and common protocol types. Platform-specific capture, render, USB AER and virtual-HID code shall implement narrow traits and remain outside management/domain logic. Use a maintained libusb/rusb-equivalent implementation behind the trait, with platform permission/driver guidance, descriptor allow-lists, asynchronous bounded transfers and hot-plug monitoring. USB completions, audio callbacks, camera callbacks, render loop and OS event hooks shall communicate through lock-free or bounded channels where justified; they shall not block on network, disk, neural execution or token refresh.

Native/global keyboard or pointer actuation shall be an optional feature disabled at build and deployment by default. Enabling it requires OS-specific permission, an allow-listed target, step-up server authorisation, local arming gesture, persistent indicator, rate/region/key restrictions, inactivity timeout and a hardware-independent emergency shortcut that the emulation cannot intercept. On crash, watchdog loss or lease expiry, the adapter releases all held state and disables itself.

## 16.23 Privacy, safety and human control

Camera, microphone, display, HID and USB AER streams may contain sensitive or identifying data. Default retention is derived events and minimal metadata only; raw recording requires explicit purpose, permission, encryption, retention period and deletion/export policy. USB serial numbers are hashed/redacted unless operationally required under separate access; unrelated interfaces and descriptors are not exposed. Screen capture shall identify the chosen surface without logging window titles or pixels unnecessarily. Keyboard capture shall exclude password/secure-entry controls and shall not log text unless the model and user explicitly require an approved text-input channel.

Every session shall show a local non-spoofable or application-persistent indicator as the platform permits. Permission revocation, sign-out, scope switch, brain pause/reset/delete, session expiry, network loss beyond grace, or control-plane fencing disarms effectful output. The emergency stop is handled locally before any server round trip and is audited after connectivity returns.

## 16.24 Peripheral lifecycle, resilience and observability

The lifecycle is Requested → AwaitingLocalConsent → Negotiating → Active → Degraded/Reconnecting → Draining → Closed, with Denied, Expired and Revoked terminal paths. Actuator state is independently Disarmed → Arming → Armed → Disarming → Disarmed, and any safety fault transitions directly to Disarmed/Faulted.

Metrics shall include capture-to-gateway, capture-to-causal-admission and neural-commit-to-presentation latency; clock offset/drift/uncertainty; media bitrate, loss, jitter and queue occupancy; USB device epoch, event/frame rate, transfer latency, endpoint stalls, sequence gaps, FIFO/host overruns and reconnects; per-channel fairness/starvation; pre-admission drops/coalescing; transducer throughput; sensory-gap count; actuator delivered/applied/expired/rejected/unknown counts; lease renewals; consent changes; emergency stops; and cross-brain/session rejection. High-cardinality media/device labels and raw payloads shall not enter ordinary logs.

Multiple workstations may manage and observe a brain concurrently. One workstation may concurrently maintain microphone, camera, display, keyboard, pointer and USB AER input/output channels for the selected brain. Conflicting binding edits use resource versions; input mixing follows the committed policy; passive output fan-out is independent; and effectful channels require the sole actuator lease. A channel-specific failure changes only that channel's state and quality unless the brain’s explicit missing-input policy says that its dependent component should pause. Losing a UI workstation cannot otherwise stop the neural computation.

# 17. Protocols, APIs, storage schemas and configuration

## 17.1 Protocol evolution

The associated .proto and OpenAPI files shall be updated; generated outputs shall not be hand-edited. Protocols shall use additive evolution, reserved removed fields, explicit enums with an unknown value, maximum sizes and validation. A compatibility matrix shall cover rolling upgrades.

Do not extend the current layer SpikeBatch until it becomes an unbounded union. Define separate causal data-plane and management messages. The legacy batch may be supported by an edge adapter during migration.

## 17.2 Data-plane service sketch

service ShardDataPlane {  
rpc OpenRoute(stream RouteFrame) returns (stream RouteFrame);  
rpc FetchCheckpoint(CheckpointFetchRequest) returns (stream CheckpointChunk);  
rpc ReplicateLog(stream LogReplicationFrame) returns (stream LogReplicationAck);  
}  
  
message RouteFrame {  
RouteHeader header = 1;  
oneof body {  
CausalEventBatch events = 10;  
StreamWatermark watermark = 11;  
DeliveryAck acknowledgement = 12;  
CreditUpdate credit = 13;  
RouteError error = 14;  
ComponentActivity component_activity = 15;  
ComponentClosureProof closure_proof = 16;  
AdvanceMicrostep advance_microstep = 17;  
}  
}

The stream handshake shall negotiate protocol/schema, numerical capabilities, maximum frame, compression and current route generation. Unsupported versions fail before biological events are accepted.

## 17.3 Management API sketch

Use resource-oriented endpoints or equivalent gRPC methods:

GET /v1/brains?project=...  
POST /v1/brains  
GET /v1/brains/{brain_id}  
PATCH /v1/brains/{brain_id} If-Match: \<etag\>  
POST /v1/brains/{brain_id}:start  
POST /v1/brains/{brain_id}:stop  
POST /v1/brains/{brain_id}:restart  
POST /v1/brains/{brain_id}:reset  
POST /v1/brains/{brain_id}/imports  
POST /v1/brains/{brain_id}/exports  
POST /v1/brains/{brain_id}/checkpoints  
POST /v1/brains/{brain_id}:restore  
POST /v1/brains/{brain_id}:migrate  
GET /v1/operations/{operation_id}  
GET /v1/events:stream?cursor=...  
POST /v1/brains/{brain_id}/peripheral-sessions  
PATCH /v1/peripheral-sessions/{session_id}  
DELETE /v1/peripheral-sessions/{session_id}  
POST /v1/peripheral-sessions/{session_id}:signal  
POST /v1/peripheral-sessions/{session_id}/local-devices  
DELETE /v1/peripheral-sessions/{session_id}/local-devices/{device_binding_id}  
POST /v1/peripheral-sessions/{session_id}/bindings  
POST /v1/peripheral-channels/{channel_id}:arm  
POST /v1/peripheral-channels/{channel_id}:disarm

Every mutating request shall include Idempotency-Key and X-Request-ID; every versioned mutation uses If-Match or expected_version. OpenAPI and protobuf annotations shall generate consistent Rust and JavaScript/TypeScript clients. Session signalling shall not disclose one workstation’s network/device metadata to unauthorised observers.

## 17.4 Peripheral protocol sketch

Management negotiates a PeripheralSessionDescriptor; high-rate payloads use the media/data plane. At minimum define:

pub struct PeripheralSampleEnvelope {  
pub session_id: PeripheralSessionId,  
pub channel_id: PeripheralChannelId,  
pub sample_id: PeripheralSampleId,  
pub channel_sequence: u64,  
pub source_epoch: PeripheralSourceEpoch,  
pub capture_clock: CaptureClockId,  
pub capture_time_ns: u64,  
pub duration_ns: u64,  
pub mapping_version: ClockMappingVersion,  
pub binding_version: BindingVersion,  
pub format: PeripheralPayloadFormat,  
pub quality: SampleQuality,  
pub payload: BoundedPayloadOrReference,  
}  
  
pub struct ActuatorIntent {  
pub effect_id: EffectId,  
pub brain_id: BrainId,  
pub channel_id: PeripheralChannelId,  
pub binding_version: BindingVersion,  
pub source_tag: LogicalTag,  
pub output_quality: OutputQuality,  
pub presentation_deadline: Option\<MonotonicDeadline\>,  
pub safety_class: ActuatorSafetyClass,  
pub payload: BoundedActuatorPayload,  
}

pub struct UsbAerFrame {  
pub device_epoch: DeviceEpoch,  
pub frame_sequence: u64,  
pub device_time_ns: Option\<u64\>,  
pub host_completion_time_ns: u64,  
pub timestamp_source: AerTimestampSource,  
pub protocol_version: UsbAerProtocolVersion,  
pub address_width_bits: u8,  
pub event_count: u32,  
pub overflow_or_gap: Option\<AerGapEvidence\>,  
pub checksum: Option\<u32\>,  
pub events: BoundedAerEventPayload,  
}

All payload formats shall have explicit maximum sizes, rate limits, schema/codec identifiers and validation. HID messages model key/button down/up and motion separately. Audio/video timestamps and durations survive codec/gateway boundaries. USB AER formats define byte order, address/polarity encoding, endpoint/framing mode, timestamp units/wrap behaviour, sequence, overflow marker and optional CRC/checksum. Input and output use distinct channels and direction-checked payloads. Session secrets, ICE/TURN credentials, raw USB serial numbers and user consent tokens are control-plane/session material and never enter brain checkpoints.

## 17.5 Error model

Errors shall have stable codes, safe messages, retryability, request/operation ID and relevant current version/route hint. At minimum define:

| **Code**                          | **Meaning**                                             | **Client action**                                      |
|-----------------------------------|---------------------------------------------------------|--------------------------------------------------------|
| STALE_RESOURCE_VERSION            | Optimistic-concurrency conflict.                        | Reload/compare; do not auto-overwrite.                 |
| STALE_PLACEMENT_GENERATION        | Sender route is obsolete.                               | Refresh placement and retry retained event.            |
| FENCED_LEASE_TERM                 | Caller/producer is no longer active.                    | Stop writes and recover through control plane.         |
| NOT_AUTHORISED                    | Policy denied action.                                   | Disable action/show safe reason; do not retry blindly. |
| CAPACITY_UNAVAILABLE              | Admission cannot meet active/durability budgets.        | Retry later or change authorised constraints.          |
| SETTLING_DEFERRED                 | Component hit limit and work moved forward.             | Inform/observe; scheduler handles evidence.            |
| IMPORT_VALIDATION_FAILED          | Staged input invalid or unsafe.                         | Display structured findings.                           |
| CHECKPOINT_CORRUPT                | Digest/checksum failed.                                 | Try another replica and alert.                         |
| CONTROL_PLANE_NO_QUORUM           | Ownership-changing action unsafe.                       | Wait; do not force a second active.                    |
| PERIPHERAL_CONSENT_REQUIRED       | Local platform/user grant is missing or expired.        | Prompt through a user gesture; never retry invisibly.  |
| PERIPHERAL_CAPABILITY_UNAVAILABLE | Requested browser/OS capability is unsupported.         | Offer supported tier; do not emulate global access.    |
| CLOCK_MAPPING_UNCERTAIN           | Capture-to-brain mapping exceeds uncertainty bound.     | Buffer/recalibrate or apply declared gap/late policy.  |
| SAMPLE_LATE_OR_DUPLICATE          | Sample is outside admission window or already accepted. | Show quality/gap status; do not retimestamp silently.  |
| ACTUATOR_NOT_ARMED                | Effectful output lacks current lease/local arming.      | Keep disarmed; require explicit workflow.              |
| EFFECT_ALREADY_APPLIED            | Replay/retransmission repeats an effect ID.             | Acknowledge dedupe; never apply again.                 |

## 17.6 Storage interfaces

Hide persistence behind traits such as CausalLog, CheckpointStore, CheckpointCatalogue, AuditStore and OperationStore. Interfaces shall express durability level, conditional create, range read, checksum and cancellation. Tests shall run against an in-memory deterministic implementation and at least one production implementation.

Object keys shall include tenant-safe hashed/scoped identifiers and immutable version IDs. Do not trust caller-supplied filenames. Encryption at rest, key rotation and access logging shall follow deployment policy.

## 17.7 Configuration

Configuration shall be typed, validated once at startup or update, and documented. Environment variables may override files through one precedence layer. Required categories include:

- logical time base, default/max settling limits and non-convergence quantum;

- numerical profile and allowed deterministic kernels;

- shard target/max size, partition weights and migration hysteresis;

- worker/thread/device concurrency and memory/queue budgets;

- batching, stream credits, timeouts, retransmission and frame sizes;

- checkpoint cadence, replication factor, lag bounds, retention and object storage;

- quorum, leases, heartbeat/suspicion thresholds and failure domains;

- authentication, TLS, identity provider, policy engine and audit retention;

- tenant quotas, fairness weights and operation limits;

- UI sampling rates and snapshot chunk limits.

- peripheral session expiry/grace, supported codecs/transports, jitter and per-channel queue/credit/fairness limits, input admission modes, clock uncertainty bounds, transducer/actuator versions, raw-data retention, effect dedupe horizon, USB AER device/protocol/VID-PID/interface/endpoint allow-lists, transfer-pool/timeout/event-rate bounds, native-HID allow-lists and emergency-stop policy.

Invalid combinations shall fail startup or reject the update with a precise message. Examples include deterministic profile with uncertified GPU kernel, replication factor greater than eligible failure domains, or settling quantum of zero.

## 17.8 Versioning and upgrade

Each checkpoint, log segment, protocol and management resource shall carry a schema version. Migration functions shall be pure, version-to-version and tested with golden fixtures. Rolling upgrade shall keep old and new nodes from becoming active for an unsupported schema/profile combination. Downgrade limitations shall be documented before deployment.

# 18. Observability and operational behaviour

## 18.1 Structured telemetry

Logs shall be structured and carry request/operation ID, brain/shard/component, logical tag, topology/partition generation and lease term where applicable. Never log tokens, complete imported models, raw sensitive neural data or unbounded event payloads.

Metrics shall cover service health, event throughput, queue depth, route credits, retransmits, watermark lag, component activity epochs, sent/received causal balance, closure retries, microstep depth, non-convergence, scheduler prediction error, fairness, resource utilisation, backup durability/applied lag, checkpoint age/duration, failover/migration, UI stream clients and the peripheral metrics in Section 16.24.

Tracing shall connect management operation to control-plane decision, shard work, log replication and checkpoint/output commitments using sampled spans. High-volume per-event tracing must be opt-in and bounded.

## 18.2 Global virtual time

Compute an asynchronous global virtual time or safe recovery horizon for monitoring, log reclamation, consistent export and checkpoint coordination. GVT shall not be a per-biological-tick execution barrier. Its algorithm shall account for queued and in-flight messages and be testable under reordering and failure.

The UI shall display local frontiers, safe/GVT tag and lag so operators understand that independent components can be at different logical ticks without incoherence.

## 18.3 Health and readiness

Separate liveness, readiness and safety health. A process may be alive but fenced, catching up, lacking durability or unable to accept new work. Health APIs shall report structured reasons. Load balancers shall not route active data-plane work to Recovering or Quarantined nodes.

## 18.4 Alerts and runbooks

Provide alerts and runbooks for control-plane quorum loss, no eligible warm backup, replica log lag, checkpoint age/RPO breach, repeated non-convergence, causal stream/termination protocol violation, digest mismatch, memory/queue pressure, migration thrash, authentication/policy failure, audit-store unavailability, peripheral clock uncertainty, sustained sensory gaps, actuator acknowledgement ambiguity and emergency disarm.

Alerts shall identify affected brains/shards and state whether biological progress stopped, degraded or continued. Avoid cluster-wide severity where only one causal region is affected.

## 18.5 Operational safeguards

Support drain mode before maintenance, capacity cordoning, read-only diagnostic bundles, bounded event sampling and deterministic replay packages. Diagnostic export shall redact secrets and tenant data by default and require permission for detailed state.

# 19. File and module change plan

## 19.1 Existing files

| **File**          | **Required refactoring**                                                                                                                                                                                                                                |
|-------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| distributed.rs    | Reduce to orchestration adapters and composition. Remove layer assignment/broadcast from new path; introduce shard executor integration, placement cache, route manager, replica state and operation hooks. Split this large file into focused modules. |
| runner.rs         | Extract explicit logical-tag phases and shard state; preserve a temporary legacy facade; remove independent clock advancement.                                                                                                                          |
| network.rs        | Introduce stable biological IDs, immutable topology views, generation-scoped dense maps and partitionable state layout.                                                                                                                                 |
| topology.rs       | Add topology transaction validation, zero-delay SCC/component DAG, affected-region recomputation and version manifests.                                                                                                                                 |
| transmission.rs   | Replace vector-index identity with stable synapse/event identity; make delay/tags and release provenance explicit.                                                                                                                                      |
| aer.rs            | Preserve source/device time mapping, device epoch, event identity, sequence and gap evidence; ingest through the causal gateway instead of layer vectors. Keep AER as one independently multiplexed sensory/effector format, not the transport for audio/video/HID. |
| bridge.rs         | Submit/observe timestamped events and committed state; eliminate direct ownership of time progression.                                                                                                                                                  |
| dynamics.rs       | Separate authoritative deterministic accumulation from floating-point profile; expose pure kernels and range tests.                                                                                                                                     |
| morphology.rs     | Emit validated topology proposals/transactions rather than mutating replicated runners independently.                                                                                                                                                   |
| transport.rs      | Define transport trait/capabilities; implement ordered resume, credit and failure reporting without biological semantics.                                                                                                                               |
| ui.rs             | Split into remote management, authentication, operation model, subscriptions, rendering, local standalone, peripheral sessions/adapters—including asynchronous USB AER—and conversion modules; remove worker fan-out. |
| app.js            | Split API/session/store/stream/operations/views and peripheral capture/render/USB-AER modules; replace the current AER-only loop with concurrent bounded session channels; use generated clients and ETags/idempotency. |
| service-access.js | Presentation mapping only; consume server permissions/capabilities; remove permissive production fallback.                                                                                                                                              |
| shell.js          | Harden session/OIDC handling, CSRF and safe redirect; expose connection/identity state without token leakage.                                                                                                                                           |
| index.html        | Add accessible scope selector, operation centre, durability/failover/non-convergence panels, peripheral session/binding panel, live privacy/actuator indicator, emergency stop and conflict dialogs.                                                    |
| swagger.html      | Point to versioned management OpenAPI, authorise access, disable unsafe persistence of credentials.                                                                                                                                                     |

Where attached names include suffixes such as runner(1).rs or morphology(1).rs, Codex shall locate the canonical repository path rather than creating duplicate modules with those uploaded names.

## 19.2 Proposed modules

The exact crate boundaries shall follow the repository workspace, but responsibilities should be separated approximately as follows:

time/ logical_tag, time_domain, pacing  
identity/ stable newtypes, ID allocation  
event/ envelope, payloads, canonical_order, dedupe  
topology/ transactions, generations, zero_delay_scc  
partition/ cost model, virtual shards, plans  
executor/ shard_actor, settlement, work_pools, kernels  
numeric/ fixed_point, profiles, deterministic_rng, digest  
route/ streams, watermark, component_termination, credits, retransmit  
scheduler/ observations, predictor, policy, decisions  
durability/ causal_log, checkpoint, manifest, retention  
replication/ warm_backup, fast_forward, validation  
control_plane/ consensus, membership, lease, fencing, placement  
federation/ link, time_mapping, gateway  
peripheral/ session, channel, capability, consent, clock_mapping  
device/ usb_aer discovery, protocol, asynchronous transfer, hotplug  
peripheral/ multiplexer, per_channel_budget, local_device_binding  
media_gateway/ signalling, transport, jitter, sequencing, reconnect  
transducer/ audio, video, display, keyboard, pointer, AER mappings  
actuator/ intents, leases, dedupe, audio_video, virtual_hid, safety  
management_api/ authn, authz, resources, operations, audit  
observability/ metrics, tracing, health, diagnostic_bundle  
ui_client/ generated/common client models

Large modules shall be split before adding more responsibility. Public APIs require Rustdoc with invariants, units, error behaviour and examples. Unsafe code is prohibited unless justified by a dedicated safety comment, test and review.

## 19.3 Dependency rules

Biological kernels may depend on identity, time and numeric primitives but shall not depend on gRPC, HTTP, UI or consensus. Transport shall not depend on model-specific neuron implementations. UI clients shall depend on generated management contracts, not executor internals. Persistence formats shall use explicit schema DTOs, not raw in-memory structs whose layout changes accidentally.

Enforce key boundaries using crates/modules, visibility and architecture tests where practical.

# 20. Staged delivery plan

## 20.1 Phase 0 — Baseline and safety net

- Catalogue all current code paths, protocols, generated artefacts, environment variables and deployment scripts.

- Capture golden standalone and seven-node scenarios, including burst-to-persistent gRPC fallback.

- Add deterministic input fixtures, state snapshots and current behaviour metrics without claiming current cross-node correctness.

- Establish CI formatting, clippy/lint, unit, integration, web tests, security scans and documentation checks.

- Write ADRs for logical time, event boundary, numerical profiles, durability and control-plane fencing.

**Gate:** Current supported builds/tests pass; baseline artefacts are reproducible; no runtime semantic change.

## 20.2 Phase 1 — Types, stable IDs and deterministic primitives

- Introduce typed IDs, LogicalTag, stable event IDs and schema versions.

- Implement fixed-point arithmetic, counter-based RNG, canonical order and digest utilities.

- Add stable neuron/synapse mapping alongside legacy indices.

- Add pure unit/property/golden tests.

**Gate:** Deterministic primitive tests pass on at least two CPU architectures where CI permits; serialisation compatibility is fixed by golden fixtures.

## 20.3 Phase 2 — Local superdense executor

- Refactor Runner::step behind phase APIs and a legacy facade.

- Run a complete brain locally through virtual shards and event queues.

- Implement component quiescence, settling limit, non-convergence checkpoint/deferral and explicit field events.

- Compare local new-path results against reference scenarios.

**Gate:** Exact deterministic replay; non-convergence never loses events; no global barrier inside one process.

## 20.4 Phase 3 — Topology, SCC and virtual partitioning

- Build zero-delay SCC/component DAG and topology generations.

- Implement weighted virtual shard plans and stable routes.

- Convert growth/morphology to transactions.

- Add online repartition planning without live cutover initially.

**Gate:** Property tests prove each neuron/synapse has one owner, every route is complete and zero-delay cycles are contained or explicitly distributed.

## 20.5 Phase 4 — Reliable distributed data plane

- Implement causal envelopes, streams, sequence/dedupe, watermarks, credits, retransmission and reconnect/resume.

- Add component-scoped distributed settlement for an oversized SCC.

- Integrate current transports behind the transport-independent route layer.

- Remove new-path layer broadcasts.

**Gate:** Reordering, duplicate, loss, timeout and transport-switch fault tests produce the same deterministic digest with no event loss.

## 20.6 Phase 5 — Multi-brain executor and scheduler

- Add brain isolation, hierarchical fairness and work-conserving dispatch.

- Add resource discovery/benchmarking and workload observations.

- Implement explainable predictor and versioned scheduling decisions.

- Add CPU/GPU placement with deterministic capability checks.

**Gate:** Multiple brains meet isolation/fairness tests, idle capacity is borrowed, and no shared global clock/seed/runner state exists.

## 20.7 Phase 6 — Durability and recovery

- Implement causal WAL, synchronous warm-backup replication and periodic immutable checkpoints.

- Implement fast-forward, digest validation, failover discontinuity and output dedupe.

- Add live migration, anti-affinity, retention and consistent-cut export.

**Gate:** Chaos tests show loss of any one compute node does not destroy a brain; recovered nodes cannot resume stale terms; RPO/RTO policy is measured.

## 20.8 Phase 7 — Replicated control plane and authorised management

- Introduce quorum, leases, fencing, membership/lifecycle and operation store.

- Implement management API, granular server-side policy, audit and streaming status.

- Migrate web and Rust clients; remove workstation-to-worker control paths.

- Implement all required remote lifecycle/import/export/checkpoint/federation operations.

- Implement peripheral-session, channel, binding, consent and actuator-lease resources plus capability discovery; keep high-rate media disabled until the next phase gate.

**Gate:** Two workstations can concurrently manage their authorised brains; conflicts are explicit; unauthorised and stale-term calls are rejected; leader failover does not duplicate operations.

## 20.9 Phase 8 — Workstation I/O, federation, optimisation and legacy removal

- Implement the secure peripheral media/data gateway, clock mapping, bounded jitter/backpressure and reconnect/resume.

- Implement pinned audio/video/display/keyboard/pointer transducers plus concurrent bidirectional USB AER device channels, committed audio/video/AER/sandboxed actuator output, recorded replay and quality/discontinuity events.
- Implement bounded fair per-channel multiplexing so USB AER, A/V and HID operate simultaneously; one modality's saturation, hot-plug or failure shall not block or retimestamp the others.

- Implement browser capability/consent UX and native adapters; keep native/global HID disabled until its separate safety gate passes.

- Implement explicit federation links/time mapping and cyclic-link validation.

- Tune scheduler, checkpoint, batching and GPU paths from benchmark evidence.

- Migrate saved data and deployments; remove the legacy layer-distributed path after rollback criteria are met.

- Complete runbooks, threat model and operator/user documentation.

**Gate:** Full acceptance suite, performance baselines, upgrade/rollback drill and independent review complete. Live microphone/camera/screen/keyboard/pointer input, simultaneous bidirectional USB AER exchange and committed audio/video/AER/sandbox output work from both clients with timing provenance, fair bounded multiplexing and channel-failure isolation; native/global HID ships only after explicit hazard review and emergency-stop tests.

## 20.10 Feature flags and rollback

Use coarse, temporary flags such as superdense_executor, causal_transport, replicated_durability and management_v1; avoid combinatorial per-function flags. Persist which path created a checkpoint. A rollback shall never make a newer checkpoint appear readable if it is not. Flag removal is part of the definition of done.

# 21. Verification, validation and acceptance

## 21.1 QA principles

Verification answers “did we implement the specified mechanism correctly?” Validation answers “does the workflow and biological timing intent remain true under realistic use?” Both are mandatory. Tests shall be deterministic, independent, bounded and classified by speed. Failures shall retain seeds, event traces, scheduler decisions, logs and checkpoint references needed to reproduce them.

Scientific validation shall separate numerical agreement, event-timing agreement and biological/behavioural validity. Passing a distributed digest test proves reproducible implementation, not that the chosen neuron, synapse, transducer or plasticity model is biologically adequate. Validation reports shall state the reference model/data, parameter provenance, accepted error measure, sensitivity to tick resolution/zero-delay abstractions and limitations.

CI shall run formatting, Clippy with agreed deny/warn settings, Rust unit/integration/doc tests, protocol compatibility, JavaScript lint/unit tests, browser tests, dependency and licence checks, unsafe-code policy, documentation links and schema golden tests. Nightly or scheduled environments shall run multi-node, chaos, soak, performance and heterogeneous CPU/GPU suites.

## 21.2 Unit and property test catalogue

| **Test ID**     | **Subject**                       | **Required assertion**                                                                                                                                          |
|-----------------|-----------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------|
| UT-TIME-001     | LogicalTag order                  | Lexicographic order, serialisation and boundary values are exact.                                                                                               |
| UT-TIME-002     | Event progression                 | Zero delay yields μ + 1; positive delay yields t + δ, 0; backwards tags are rejected.                                                                           |
| UT-TIME-003     | Overflow                          | Tick/microstep overflow is detected before mutation and follows the specified failure/deferral path.                                                            |
| UT-ID-001       | Stable IDs                        | Dense compaction and migration preserve neuron/synapse identity and event targets.                                                                              |
| UT-NUM-001      | Q32.32 conversion                 | Round-to-nearest ties-to-even and representable limits match golden values.                                                                                     |
| UT-NUM-002      | Arithmetic                        | i128 intermediates, saturation/error policy and negative values satisfy property tests.                                                                         |
| UT-RNG-001      | Counter RNG                       | Draws are identical across iteration/thread order and distinct for different stable coordinates.                                                                |
| UT-ORDER-001    | Canonical sorting                 | Every permutation of the same input multiset produces the same order/digest.                                                                                    |
| UT-DEDUPE-001   | Event dedupe                      | Duplicate event IDs/sequences apply once and acknowledge consistently across restart.                                                                           |
| UT-WM-001       | Watermark monotonicity            | Regressing or contradictory watermarks are rejected and evidence retained.                                                                                      |
| UT-SCC-001      | Component calculation             | Generated graphs match a trusted SCC oracle, including self-loops and dynamic zero lower bounds.                                                                |
| UT-PART-001     | Ownership                         | A partition plan assigns every biological object exactly once and generates all required routes.                                                                |
| UT-SETTLE-001   | Quiescence proof                  | Empty local queue alone is insufficient; every stated closure condition is necessary.                                                                           |
| UT-SETTLE-002   | Settling cap                      | Cap outcome is DeferredNonConvergent, never Quiescent.                                                                                                          |
| UT-DEF-001      | Deferred events                   | Count, payload, original tag and canonical digest are preserved when retagged.                                                                                  |
| UT-LOG-001      | Log checksum/hash chain           | Any record/segment mutation is detected; a valid chain replays exactly.                                                                                         |
| UT-CHK-001      | Immutable publish                 | Partial object is not discoverable; published manifest cannot be overwritten.                                                                                   |
| UT-FENCE-001    | Lease validation                  | Every stale term/token/generation is rejected at event, log, checkpoint and output boundaries.                                                                  |
| UT-AUTH-001     | Policy default deny               | Missing/unknown permission never grants access, including auth-mode misconfiguration.                                                                           |
| UT-IDEMP-001    | Idempotency                       | Same key/body returns same operation; same key/different body conflicts.                                                                                        |
| UT-TERM-001     | Distributed component termination | Closure is impossible while a participant is active, a send is unreceived or a passive report was invalidated; one closure decision occurs per component epoch. |
| UT-SYN-001      | Synapse-stage ownership           | Every terminal/release/weight/plasticity object has one owner; source- and target-owned variants emit exactly one authoritative transition.                     |
| UT-IOTIME-001   | Peripheral time mapping           | Drift, uncertainty, discontinuity and rounding produce the declared eligible tag; arrival-time perturbation does not.                                           |
| UT-IOSAMPLE-001 | Peripheral sequencing             | Duplicate/reordered samples admit once; gaps, coalescing and pre-admission drop follow modality policy.                                                         |
| UT-EFFECT-001   | Actuator effect dedupe            | Retransmission, reconnect, failover and replay never apply one EffectId twice; unknown-after-disconnect remains explicit.                                       |
| UT-HID-001      | HID state safety                  | Disarm/crash/lease expiry releases every held key/button and prevents stale-term actuation.                                                                     |
| UT-AERUSB-001   | USB AER protocol and epoch        | Golden framing validates direction, length, address/polarity, timestamp source/units, sequence, overflow, CRC and device-epoch transitions; malformed input is rejected before admission. |
| UT-AERUSB-002   | Concurrent channel multiplexer    | A/V, HID and USB AER maintain independent order, clocks and bounded queues; saturation, cancellation or reconnect of one cannot starve, stop or retimestamp another. |

Property tests shall use shrinking and record the minimal failing graph/event set. Model-based tests should compare the distributed protocol state machine with a small single-thread reference interpreter.

## 21.3 Logical-time and causal validation scenarios

| **Scenario ID** | **Stimulus**                                                                                  | **Expected result**                                                                                                                                               |
|-----------------|-----------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| VT-CAUSAL-001   | A→B zero-delay model edge across shards.                                                      | The declared owner produces one SynapticTransition at (t, μ) and B’s consequence applies only at (t, μ+1); packet arrival time has no effect.                     |
| VT-CAUSAL-002   | D→A→D zero-delay loop.                                                                        | SCC co-locates or uses component termination/fence; a delayed in-flight message prevents closure; result matches the reference digest.                            |
| VT-CAUSAL-003   | Positive-delay feedback.                                                                      | Current tick closes without waiting for the future feedback event.                                                                                                |
| VT-CAUSAL-004   | Slow shard and fast shard.                                                                    | Fast unrelated component advances; dependent component waits only on explicit causal closure.                                                                     |
| VT-CAUSAL-005   | Missing final event but no watermark.                                                         | Receiver waits/requests progress; it does not guess completion.                                                                                                   |
| VT-CAUSAL-006   | Accepted watermark followed by late event.                                                    | Protocol violation, quarantine/recovery and no silent application.                                                                                                |
| VT-CAUSAL-007   | Global homeostasis cadence.                                                                   | Explicit future-tag field event applies deterministically without per-tick whole-brain barrier.                                                                   |
| VT-CAUSAL-008   | Multiple independent brains at different rates.                                               | Tags and queues remain isolated; neither waits for the other.                                                                                                     |
| VT-CAUSAL-009   | Presynaptic spike, axonal delay, release failure and postsynaptic effect split across shards. | Stage tags/owners match model definition; release is evaluated once; a failed release produces no postsynaptic effect but retains provenance.                     |
| VT-CAUSAL-010   | Oscillatory/travelling-wave fixture.                                                          | Biological phase relationships arise from modelled delays/coupling; the executor adds no universal tick barrier and preserves phase/timing against the reference. |

For each scenario capture source/event/target tags, stream sequences, closures, state digests and settlement outcome. Tests shall vary CPU delay and transport jitter to prove wall-clock independence.

## 21.4 Non-convergence validation

Construct a known amplifying zero-delay loop that cannot settle before a small settling_limit. Assert:

- the current microstep completes deterministically;

- the component termination proof closes that microstep and no unknown same-tag message remains in flight;

- a valid immutable checkpoint/reference and NonConvergenceRecord exist;

- pending local and in-flight events match the expected canonical digest;

- all pending events are retagged to the configured next quantum and marked deferred_from_nonconvergence;

- no event is lost or applied twice after continuation;

- the scheduler observation records amplification, depth, residual and resource use;

- another unrelated component and brain continue while the cap path persists;

- repeated deferral retains a traceable root/count without unbounded record growth;

- replay from the checkpoint reproduces the terminal and subsequent state.

- dependent components receive the discontinuity/quality state and high-risk actuator output is suppressed by default;

- divergence from a higher settling_limit reference is quantified rather than described as equivalent.

Add a convergent deep-cascade control case proving that increasing settling_limit permits true quiescence and does not alter the result reached by the reference interpreter.

## 21.5 Determinism and numerical validation

Run the same deterministic-reference fixture with:

- one thread and maximum available threads;

- different work-stealing schedules and deterministic injected yields;

- different packet batching, reordering, duplication and retransmission;

- shard co-location and distribution across at least seven nodes/processes;

- checkpoint/restore, active-node failover and live migration mid-run;

- certified CPU and GPU deterministic kernels where supported.

- immutable recorded peripheral input replay—including USB AER device epoch, timestamp source, frames and gaps—with pinned clock mapping and transducer versions, compared with the original admitted sensory-event digest; committed USB AER output replay remains deduplicated and does not reapply physical events.

The committed hierarchical digests and output event sequence shall be exactly equal. Fast-biological tests shall define per-variable absolute/relative tolerance, trajectory/correlation acceptance and maximum accumulated deviation; they shall never reuse exact-equality labels.

## 21.6 Distributed integration test matrix

| **Test ID** | **Topology and fault**                                     | **Pass criteria**                                                                                           |
|-------------|------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------|
| IT-DIST-001 | Seven nodes, small three-layer model, many virtual shards. | Useful work distributed by cost; no anchor computes all layers by fallback; result matches local reference. |
| IT-DIST-002 | Transport changes persistent gRPC→burst→persistent.        | Resume sequence prevents duplication/loss; digest unchanged.                                                |
| IT-DIST-003 | Reordered/duplicated frames.                               | Exactly-once application and monotonic watermarks.                                                          |
| IT-DIST-004 | Backpressure on one destination.                           | Bounded memory; unrelated routes/brains progress; no closure deadlock.                                      |
| IT-DIST-005 | Oversized distributed zero-delay SCC.                      | Only component participants fence microsteps; others continue.                                              |
| IT-DIST-006 | Topology growth during activity.                           | New generation takes effect atomically; old events drain/translate by rule.                                 |
| IT-DIST-007 | Live repartition.                                          | One owner before/after; no missing/duplicate state/events; expected digest.                                 |
| IT-DIST-008 | Control-plane leader change.                               | Existing safe execution continues; operations resume idempotently.                                          |
| IT-DIST-009 | Distributed SCC coordinator fails after passive reports.   | New coordinator reconstructs one epoch; no duplicate/early closure and unrelated components progress.       |
| IT-DIST-010 | Watermarks advance but a cyclic message remains in flight. | Route horizons do not cause false quiescence; termination balance blocks closure until durable receipt.     |

Tests shall run with network emulation for latency, loss, duplication, partitions and bandwidth. Fault injection points shall be deterministic and test-only, not ad hoc sleeps.

## 21.7 Durability and chaos tests

| **Test ID** | **Failure injection**                                                 | **Required result**                                                                                                              |
|-------------|-----------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------|
| CT-001      | Kill active after local compute but before warm-log acknowledgement.  | Uncommitted transition is not exposed as committed; replay uses last durable point.                                              |
| CT-002      | Kill active after durable log but before destination acknowledgement. | Retransmit/dedupe applies event once.                                                                                            |
| CT-003      | Kill active during checkpoint upload.                                 | Partial object remains undiscoverable; previous checkpoint restores.                                                             |
| CT-004      | Corrupt newest checkpoint.                                            | Checksum fails, another replica is selected, alert/audit produced.                                                               |
| CT-005      | Isolate old active after lease expiry.                                | New term may be promoted by quorum; old writes/events/outputs are fenced.                                                        |
| CT-006      | Recover old active with stale queues.                                 | It enters Recovering and cannot resume; fast-forward/rejoin follows current plan.                                                |
| CT-007      | Lose active and one checkpoint store endpoint.                        | Warm WAL and another immutable copy meet configured RPO or affected region pauses safely.                                        |
| CT-008      | Quorum loss.                                                          | No new active/promotion/destructive operation; safety state visible.                                                             |
| CT-009      | Fail destination during live migration before cutover.                | Source remains active; reservation/temporary state cleaned idempotently.                                                         |
| CT-010      | Fail source immediately after cutover consensus commit.               | Destination resumes under new term with validated digest.                                                                        |
| CT-011      | Exhaust object-store quota.                                           | New checkpoints report failure; retention/admission policy acts; old checkpoints are not overwritten.                            |
| CT-012      | Audit store unavailable.                                              | Policy-defined fail-closed for privileged mutations; biological data plane behaviour is documented.                              |
| CT-013      | Kill peripheral gateway during live input/output.                     | Brain continues per missing-input policy; bounded sensory gap is recorded; committed effects are not duplicated after reconnect. |
| CT-014      | Workstation sleeps/resumes with clock jump.                           | Old mapping is closed, new version calibrated, uncertain samples follow policy and prior tags remain immutable.                  |
| CT-015      | Rust UI crashes while a key/button is held.                           | Watchdog/lease expiry releases state and disarms; no stale command is accepted after restart.                                    |
| CT-016      | USB AER device is removed, resets, stalls or overflows while A/V/HID remain active. | The old device epoch closes, explicit AER gap/quality state is recorded, committed AER output is not duplicated, and unaffected modalities continue. |

Measure recovery-point and recovery-time objectives. A single compute-node loss shall not destroy any brain that met its configured durability state before the fault.

## 21.8 Scheduler and resource validation

- Feed a deterministic sequence of observations and assert the predictor/decision explanation is stable.

- Place deep/amplifying SCCs on faster resources when benefit exceeds migration/hysteresis cost.

- Demonstrate that a slow/busy CPU receives less causal work than a fast/idle CPU while both contribute according to effective capacity.

- Demonstrate CPU/GPU choice changes with transfer size, deterministic capability and VRAM pressure.

- Verify total worker/thread settings do not oversubscribe available cores.

- Verify RAM/VRAM/event-queue budgets reject or backpressure before out-of-memory.

- Verify tenant minimum shares, weighted fairness, idle borrowing and starvation bounds with at least three brains and two tenants.

- Verify backup/checkpoint/catch-up work consumes idle capacity then yields to causal critical work.

- Verify repeated non-convergence changes future scheduler recommendations only through versioned decisions effective at safe tags.

- Verify placement anti-affinity and explicit degraded-durability reporting under insufficient failure domains.

Performance tests shall report baseline and new-path throughput, p50/p95/p99 event latency, microstep settlement latency, causal critical-path wait, CPU/GPU utilisation, memory, network bytes and checkpoint/recovery rates. Performance regression thresholds shall be agreed and encoded, not judged informally.

## 21.9 Multiple-brain and federation validation

Run at least four whole-brains with different sizes, tick rates, numerical profiles and owners across the same fleet. Assert identity, state, seed, quotas, logs, checkpoints, outputs and permissions never cross-contaminate. Pause/fail/reset one brain and prove the others continue except for explicit federation dependencies.

For federation, test positive-delay links, different time-base mappings, backpressure, source failure, revocation and cross-tenant dual authorisation. Reject an unapproved zero-delay federation cycle. Verify link replay/dedupe after either brain fails.

## 21.10 Management API and security tests

| **Test ID** | **Workflow**                                                   | **Pass criteria**                                                                                |
|-------------|----------------------------------------------------------------|--------------------------------------------------------------------------------------------------|
| API-001     | Two clients update same version.                               | One succeeds; the second gets STALE_RESOURCE_VERSION and no silent overwrite.                    |
| API-002     | Retry create/reset with same key.                              | One operation/side effect only.                                                                  |
| API-003     | User guesses another tenant’s brain ID.                        | Not authorised without resource disclosure.                                                      |
| API-004     | UI hides action but forged HTTP/gRPC request sends it.         | Server denies and audits.                                                                        |
| API-005     | Rust client calls worker directly.                             | Network/auth policy denies end-user identity.                                                    |
| API-006     | Token expiry during operation stream.                          | Secure refresh/reconnect from cursor; operation not duplicated.                                  |
| API-007     | CSRF against web mutation.                                     | Rejected; no operation created.                                                                  |
| API-008     | Malicious archive/JSON import.                                 | Sandboxed validator rejects within resource limits.                                              |
| API-009     | Export URL reused by another principal/after expiry.           | Denied; audit event recorded.                                                                    |
| API-010     | Leader fails during long import/migration.                     | New leader resumes operation exactly once.                                                       |
| API-011     | Local development auth bypass in production config.            | Startup fails or endpoint remains default-deny.                                                  |
| API-012     | Audit/log redaction.                                           | Tokens, cookies and sensitive payloads absent from logs and diagnostics.                         |
| API-013     | Brain controller requests microphone/camera without I/O grant. | Server denies; browser/native client cannot start a session from brain-control permission alone. |
| API-014     | Peripheral session is rebound to an unauthorised brain.        | Generation/resource check rejects without leaking target existence.                              |
| API-015     | Two clients arm the same effectful channel.                    | One fenced actuator lease wins; stale client is disarmed and rejected.                           |
| API-016     | Replay sends an old EffectId under a new connection.           | Client/gateway dedupe acknowledges without applying the effect.                                  |
| API-017     | Malicious media/HID/USB-AER payload, codec bomb or excessive rate. | Size/rate/framing/parser validation rejects before admission; queues and USB transfer pools remain bounded. |
| API-018     | Principal has USB AER input permission but requests AER output or an unrelated USB interface. | Direction/device/interface policy denies and audits; permitted A/V/HID/AER-input channels remain active. |
| API-019     | Stale USB device epoch or local-device binding is reused after reconnect/rebind. | Generation and session checks reject it; a newly consented/validated epoch is required without leaking raw device identity. |

Threat modelling shall cover spoofing, tampering, repudiation, information disclosure, denial of service and elevation of privilege across workstation, WebUSB/local companion, USB device/driver, gateway, control plane, worker, backup and storage boundaries.

## 21.11 Web UI validation

Use unit tests plus browser automation at supported viewport sizes. Verify sign-in/out, scope discovery, permission-driven controls, create/start/stop/restart/reset/repeat, staged import, consistent export, checkpoint/restore, operation cancellation, conflict resolution, stream reconnect and failure/discontinuity display.

Verify secure-context capability detection, permission grant/denial/revocation for camera/microphone/display, focused keyboard/pointer capture, Pointer Lock exit, autoplay restrictions, visible session indicator, one-action stop and the explicit absence of native/global HID claims. Where supported, verify WebUSB device selection, descriptor allow-list, disconnect and permission revocation; otherwise verify the authenticated local AER companion or an accurate unavailable capability. Mock media and USB AER devices in CI and run a real-browser device-permission suite in a controlled environment. Keep microphone/camera/HID and AER active together during responsiveness tests.

Accessibility validation shall include keyboard-only navigation, logical focus after dialogs/updates, accessible names, live-region announcements, contrast, reduced motion and no colour-only status. Large topology views shall remain responsive and memory-bounded. The UI shall not freeze while an operation runs or a stream reconnects.

## 21.12 Rust UI validation

Use mocked management clients for state transitions and integration tests against a test orchestrator. Verify secure endpoint profiles, device/PKCE authentication, token redaction, multi-orchestrator scope switching, TLS failure handling, operation progress, conflict resolution, remote import/export and cancellation.

Verify native device permission failure, USB AER descriptor/protocol negotiation, hot-plug/removal/reset/endpoint stall/FIFO overflow, concurrent AER input/output, audio under/overrun, camera format change, screen-source closure, focused input, optional virtual-HID arming, allow-list enforcement, watchdog release and emergency stop on each supported operating system. Unsupported platforms shall compile with a safe capability report and no stub that claims USB or actuation support.

Instrument the render loop: blocking network/file/conversion work shall not run there. Under a slow stream and large topology, frame responsiveness shall meet the agreed budget and memory remain bounded. Standalone and remote state shall not be confused.

## 21.13 Workstation I/O verification and validation

Run end-to-end tests from both clients through the gateway/transducer, brain input, committed output and workstation presentation. Use synthetic audio/video/HID and bidirectional USB AER sources/sinks with known timestamps, sequences and digests, plus controlled physical-device tests where CI hardware permits. The principal workload shall keep USB AER, microphone, camera/display and HID active simultaneously against one selected brain.

| **Test ID** | **Scenario**                                            | **Pass criteria**                                                                                                                   |
|-------------|---------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------|
| IO-E2E-001  | Microphone/camera frame with network jitter/reordering. | Eligible tag derives from capture clock mapping; derived sensory-event digest is stable; arrival time changes only latency metrics. |
| IO-E2E-002  | Screen capture permission denied/revoked.               | No media flows; session state and user explanation are correct; brain follows declared missing-input policy.                        |
| IO-E2E-003  | High-rate pointer motion and reliable buttons.          | Motion coalesces within error bound; button transitions remain ordered; no stuck button after loss.                                 |
| IO-E2E-004  | Recorded A/V/HID replay.                                | Pinned transform recreates exact admitted sensory events in deterministic-reference mode.                                           |
| IO-E2E-005  | Committed audio/video output misses deadline.           | Frame is applied or explicitly expired; neural state is unaffected; next keyframe resynchronises.                                   |
| IO-E2E-006  | Failover after effect commit but before client ack.     | EffectId is not applied twice; state is Applied or UnknownAfterDisconnect, never fabricated.                                        |
| IO-E2E-007  | Two brains and two workstations concurrently.           | Samples, frames, permissions, bindings, clocks and effects remain isolated under load.                                              |
| IO-E2E-008  | Non-convergent output reaches binding.                  | Quality flag propagates; high-risk actuator is suppressed; permitted sandbox/audio/video policy is observable.                      |
| IO-E2E-009  | Browser requests global keyboard/mouse actuation.       | Capability is unavailable and UI explains safe alternative; no synthetic OS action occurs.                                          |
| IO-E2E-010  | Native HID armed then network/client failure.           | Local watchdog releases keys/buttons and disarms within the specified bound.                                                        |
| IO-E2E-011  | Clock drift, workstation sleep and reconnect.           | Mapping versions split at discontinuity; uncertainty/late policy is honoured; no historical retimestamping.                         |
| IO-E2E-012  | Saturated transducer GPU/queue.                         | Admission backpressure is bounded, declared pre-admission drop/gap occurs, neural and management safety traffic continues.          |
| IO-E2E-013  | One workstation concurrently runs USB AER input/output, microphone, camera/display and HID for one brain. | All channels remain independently sequenced, clocked, authorised and bounded; fair multiplexing prevents starvation; combined admitted/output digests match the reference. |
| IO-E2E-014  | USB AER hot-unplug/reconnect, malformed frame, endpoint stall or device FIFO overflow during active A/V/HID. | Only the AER channel degrades/restarts with a new device epoch and explicit gap/quality state; no duplicate committed AER output; A/V/HID and management remain responsive. |

Measure p50/p95/p99 capture/device-to-admission and commit-to-presentation/device-acceptance latency for each modality; A/V jitter, loss and synchronisation; USB AER event rate, sequence gaps, FIFO/host overflow, transfer latency and hot-plug recovery; per-channel fairness/starvation; timestamp uncertainty; transducer throughput; browser/Rust UI responsiveness; and memory/CPU/GPU/network/USB consumption. Acceptance budgets shall be deployment profiles rather than unsubstantiated universal millisecond targets.

Validate the sensory mapping scientifically with modality-appropriate fixtures: frequency/amplitude response for audio, spatial/temporal response for visual input, event polarity/timing for AER, and calibrated displacement/button semantics for pointer/keyboard. Validate actuator decoders for bounded range, stability and fail-safe neutral state. Document that these tests validate the selected transducer/effector model, not human-equivalent perception or behaviour.

## 21.14 Documentation validation

Required documentation includes:

- project architecture overview and diagrams;

- ADRs for all locked decisions;

- Rustdoc/module docs and examples for public types/traits;

- superdense-time, quiescence and non-convergence protocol specification;

- numerical profiles, units, rounding, overflow and reproducibility statement;

- management OpenAPI/gRPC docs and generated-client usage;

- configuration reference with defaults, units and security implications;

- checkpoint/log schema, migration and retention guide;

- deployment, capacity, upgrade, rollback, backup/restore and incident runbooks;

- threat model, permissions matrix and audit guide;

- web/Rust user guides and accessibility notes;

- peripheral session, clock mapping, transducer/actuator, concurrent-channel multiplexing, USB AER protocol/device/permission/hot-plug, privacy/consent, browser capability and native-HID safety guides;

- test strategy, performance baseline and known limitations.

Examples and commands in documentation shall be exercised in CI where practical. Comments shall explain invariants and reasons, not restate obvious code. British spelling and terminology shall be used consistently in project-authored documentation and user-facing messages.

## 21.15 Definition of done

The programme is complete only when all of the following are true:

- Layer-redundant broadcast is absent from the production distributed execution path.

- Cross-shard synaptic events preserve superdense logical time and pass causal reference tests.

- Component quiescence is exact; no per-tick whole-brain barrier exists.

- Distributed zero-delay SCC closure uses tested termination detection with in-flight accounting; route watermarks alone cannot close a cycle.

- Non-convergence records, immutable state and marked deferral are replayable with no event loss.

- Non-convergence deferral is labelled as a trajectory-changing approximation and its divergence from a higher-limit reference is reported.

- Deterministic-reference results are exact under parallelism, reordering, failover and migration.

- The learned scheduler uses measured SCC/shard behaviour and all state-affecting choices are versioned/explainable.

- Multiple independent/federated brains share the fleet fairly without state, clock, permission or durability leakage.

- Every active shard meets the configured active/warm/checkpoint durability arrangement or is visibly paused/degraded according to policy.

- Node failure/recovery and fleet expansion automatically trigger safe fast-forward, placement and resource use without split brain.

- Web and Rust interfaces can remotely perform every authorised lifecycle/data operation through the orchestrator API.

- Web and Rust interfaces can simultaneously attach supported authorised workstation audio/video/keyboard/pointer and bidirectional USB AER channels to the selected brain, with independent timing/sequence provenance, device epochs, consent, isolation, bounded fair backpressure and visible per-channel/global stop/disarm; native/global HID, if shipped, passes its separate safety gate.

- Server-side policy, optimistic concurrency, idempotency, audit and stream resumption pass security/workflow tests.

- CI, chaos, performance, upgrade/rollback and documentation gates pass with no unresolved high-severity defects.

- Legacy feature flags and obsolete layer-sharding code are removed after the agreed rollback window.

# 22. Codex implementation instructions

## 22.1 Required working method

Codex in JetBrains IDE shall follow this sequence for each phase:

1\. Read repository instructions (AGENTS.md, contributing guide, workspace manifests and generated-file notes) completely.

2\. Inspect affected code and tests using symbol/reference search; do not assume uploaded filenames are canonical paths.

3\. Present a concise phase plan mapping requirement IDs/invariants to files and tests.

4\. Identify unrelated dirty changes and preserve them.

5\. Add or update tests that fail for the missing behaviour before or alongside implementation.

6\. Implement the smallest coherent vertical slice behind traits and temporary coarse feature flags.

7\. Keep I/O non-blocking and queues bounded; never hold locks across await or long kernels.

8\. Format and run the narrowest tests, then the affected package/workspace suite, lints and documentation checks.

9\. Report changed files, satisfied requirements, test evidence, performance impact, risks and remaining work.

10\. Do not claim completion if a requirement is stubbed, a test is skipped without reason or generated protocol/client files are stale.

## 22.2 Coding standards

- Prefer small cohesive modules, traits at volatile boundaries and dependency injection for tests.

- Deduplicate concepts; there shall be one authoritative LogicalTag, ID, permission, operation state and numerical implementation.

- Use typed errors with context and stable public error codes; do not swallow transport/storage errors.

- Avoid unbounded channels, global mutable state, blocking sleeps, polling loops and spin-lock contention.

- Use cancellation tokens, deadlines and structured concurrency; task ownership/shutdown shall be explicit.

- Validate all external data and sizes before allocation. Apply secure defaults and least privilege.

- Use tracing fields consistently and avoid secrets/large payloads.

- Document units, logical-time semantics, ownership, determinism and safety invariants at type/function boundaries.

- Benchmark before introducing unsafe code or complex optimisation; keep a clear reference implementation.

- Prefer deterministic fixtures and fake clocks/transports to timing-sensitive sleeps in tests.

## 22.3 Prohibited shortcuts

Codex shall not:

- simulate timing coherence by aligning wall-clock timers;

- treat a timeout or silence as causal closure;

- treat a route watermark as sufficient termination proof for a cyclic distributed component;

- use a global barrier after every biological tick;

- call settlement-limit exhaustion quiescence;

- drop unresolved events or lose their original provenance;

- allow each node to change fidelity/depth independently;

- retain vector indices as cross-generation synapse identity;

- fan management commands from a workstation directly to workers;

- trust client-side permissions or optimistic UI state as server truth;

- overwrite checkpoints, reuse an old fencing term or promote without quorum;

- add a second implementation of canonical ordering, fixed-point arithmetic or policy evaluation for convenience;

- mark flaky distributed tests ignored instead of fixing deterministic orchestration.

- use packet arrival time as the biological timestamp of a peripheral sample;

- route raw high-rate media through management status APIs or persist it without consent/retention policy;

- emit external effects from uncommitted/replayed output or without EffectId dedupe and actuator fencing;

- claim that browser code can capture or inject global operating-system keyboard/mouse input without platform-specific privileged support;

- hide capture/actuator state, omit an emergency stop or auto-arm a high-risk output after reconnect.

## 22.4 Pull-request template requirements

Every implementation PR shall state:

Scope: requirements/invariants, architecture decisions and changed files.  
Behaviour: user-visible, logical-time/determinism, durability/failover and security impact.  
Evidence: exact tests/commands, performance results and schema/protocol compatibility.  
Delivery: rollout/rollback/flags, documentation updates and known limitations/follow-up.

# Appendix A. Reference state and protocol types

The following sketches establish semantics, not final naming or serialization layout.

pub struct ShardPlacement {  
pub brain_id: BrainId,  
pub shard_id: ShardId,  
pub topology_generation: TopologyGeneration,  
pub partition_generation: PartitionGeneration,  
pub active_node: NodeId,  
pub active_device: DeviceId,  
pub lease_term: LeaseTerm,  
pub fencing_token: FencingToken,  
pub lease_expires_at: MonotonicDeadline,  
pub replicas: Vec\<ReplicaPlacement\>,  
pub effective_at: LogicalTag,  
}  
  
pub struct ReplicaProgress {  
pub role: ReplicaRole, // Warm, Cold, CatchingUp  
pub checkpoint_tag: LogicalTag,  
pub durable_log_tag: LogicalTag,  
pub applied_tag: LogicalTag,  
pub verified_state_digest: Option\<StateDigest\>,  
}  
  
pub struct EventProvenance {  
pub original_event_tag: LogicalTag,  
pub original_eligible_tag: LogicalTag,  
pub deferred_from_nonconvergence: bool,  
pub deferral_root: Option\<NonConvergenceId\>,  
pub deferral_count: u32,  
pub external_source: Option\<ExternalSourceId\>,  
}

pub struct NonConvergenceRecord {  
pub id: NonConvergenceId,  
pub brain_id: BrainId,  
pub shard_id: ShardId,  
pub component_id: ComponentId,  
pub topology_generation: TopologyGeneration,  
pub partition_generation: PartitionGeneration,  
pub start_tag: LogicalTag,  
pub last_completed_microstep: u32,  
pub fidelity_depth: u32,  
pub settling_limit: u32,  
pub events_per_microstep: Vec\<u64\>,  
pub amplification: Vec\<FixedRatio\>,  
pub residual: FixedPoint,  
pub pending_event_count: u64,  
pub pending_event_digest: EventSetDigest,  
pub initial_state_digest: StateDigest,  
pub terminal_state_digest: StateDigest,  
pub checkpoint: CheckpointRef,  
pub numerical_profile: NumericalProfile,  
pub kernel_version: KernelVersion,  
pub lease_term: LeaseTerm,  
pub causal_log_position: LogPosition,  
pub resource_observation: ResourceObservation,  
pub termination_proof_digest: Digest,  
pub downstream_discontinuity: EventId,  
}

pub struct StreamProgress {  
pub stream_id: StreamId,  
pub next_send_sequence: u64,  
pub highest_durable_ack: u64,  
pub highest_applied_sequence: u64,  
pub watermark_through: Option\<LogicalTag\>,  
pub credits: CreditWindow,  
pub topology_generation: TopologyGeneration,  
pub partition_generation: PartitionGeneration,  
pub lease_term: LeaseTerm,  
pub component_activity_epoch: Option\<u64\>,  
pub durable_sent_balance: u64,  
pub durable_received_balance: u64,  
}

# Appendix B. State machines

## B.1 Shard replica

| **State**       | **Entry condition**                                          | **Allowed exit**                                        |
|-----------------|--------------------------------------------------------------|---------------------------------------------------------|
| Unassigned      | No placement.                                                | Provisioning.                                           |
| Provisioning    | Resources reserved; checkpoint/log fetching.                 | CatchingUp, Failed.                                     |
| CatchingUp      | Replay in progress, not serving live active writes.          | Warm, ActiveCandidate, Failed, Quarantined.             |
| Warm            | Durable WAL replication active; applied state within policy. | ActiveCandidate, CatchingUp, Draining, Failed.          |
| ActiveCandidate | Quorum selected; digest/routes verified; not yet leased.     | Active, Warm, Failed.                                   |
| Active          | Holds valid lease/fencing token; sole writer.                | Draining, Fenced, Failed.                               |
| Draining        | Cutover/checkpoint in progress; bounded new work.            | Warm, Fenced, Failed.                                   |
| Fenced          | Old term invalid; no writes/outputs accepted.                | CatchingUp, Unassigned.                                 |
| Quarantined     | Corruption/protocol/digest issue.                            | CatchingUp after operator/policy clearance, Unassigned. |

## B.2 Management operation

| **State**         | **Meaning**                                               | **Cancellation**                           |
|-------------------|-----------------------------------------------------------|--------------------------------------------|
| Queued            | Authorised and durably recorded.                          | Yes.                                       |
| Validating        | Schema, policy, quota and preconditions checked.          | Yes.                                       |
| WaitingForSafeTag | Awaiting component/brain boundary without global barrier. | Yes.                                       |
| Running           | Performing staged mutation/migration/export.              | If operation-specific safe point allows.   |
| Cancelling        | Compensating/cleaning temporary resources.                | No second cancellation.                    |
| Succeeded         | Result and resource version committed.                    | No.                                        |
| Failed            | Structured error; no ambiguous partial success.           | Retry with new/same key according to code. |
| RolledBack        | Mutation reversed to recorded safe state.                 | No.                                        |

## B.3 Peripheral session

| **State**                           | **Meaning**                                                 | **Required safety action**                                          |
|-------------------------------------|-------------------------------------------------------------|---------------------------------------------------------------------|
| Requested                           | Server request exists; no local device grant.               | No capture or output.                                               |
| AwaitingLocalConsent                | Client is presenting platform/user permission.              | No background retry or hidden capture.                              |
| Negotiating                         | Session/transport/codec and bindings are being established. | Actuators remain disarmed.                                          |
| Active                              | Leased channels are usable within grants.                   | Visible indicator and expiry heartbeat.                             |
| Degraded / Reconnecting             | Loss, clock uncertainty or gateway change.                  | Bound buffering; gaps/unknown effects explicit; revalidate mapping. |
| Draining                            | Stop requested; queues/held state closing.                  | Stop admission, release HID, finish safe acknowledgements.          |
| Closed / Denied / Expired / Revoked | Terminal session state.                                     | Revoke transport keys/leases and disarm all effectful channels.     |

## B.4 Actuator channel

| **State** | **Entry**                                               | **Allowed action**                                          |
|-----------|---------------------------------------------------------|-------------------------------------------------------------|
| Disarmed  | Default, revocation, expiry or fault.                   | Passive preview only; reject effects.                       |
| Arming    | Server grant plus required local gesture/OS permission. | Validate target/allow-list; no effect yet.                  |
| Armed     | Sole current actuator lease and local indicator.        | Apply deduplicated committed effects within safety bounds.  |
| Disarming | Stop/revocation in progress.                            | Reject new effects and release held state.                  |
| Faulted   | Watchdog, target, parser or safety failure.             | Immediate neutral/release; explicit operator/user recovery. |

# Appendix C. Scheduler observation and decision contract

pub struct WorkloadObservation {  
pub brain_id: BrainId,  
pub shard_id: ShardId,  
pub component_id: ComponentId,  
pub tag: LogicalTag,  
pub numerical_profile: NumericalProfile,  
pub fidelity_depth: u32,  
pub microsteps: u32,  
pub converged: bool,  
pub input_events: u64,  
pub output_events: u64,  
pub synaptic_operations: u64,  
pub cross_shard_bytes: u64,  
pub cpu_time_ns: u64,  
pub gpu_time_ns: u64,  
pub peak_ram_bytes: u64,  
pub peak_vram_bytes: u64,  
pub causal_wait_ns: u64,  
pub peripheral_capture_to_admission_ns: Option\<u64\>,  
pub peripheral_commit_to_present_ns: Option\<u64\>,  
pub transducer_time_ns: u64,  
pub pre_admission_drop_count: u64,  
}  
  
pub struct SchedulingDecision {  
pub decision_id: DecisionId,  
pub predictor_version: PredictorVersion,  
pub policy_version: PolicyVersion,  
pub evidence_digest: Digest,  
pub action: SchedulingAction,  
pub explanation: String,  
pub effective_at: LogicalTag,  
pub expires_or_review_at: Option\<LogicalTag\>,  
}

# Appendix D. Acceptance traceability

| **User intent**                                                    | **Primary requirements**   | **Verification evidence**                                                                        |
|--------------------------------------------------------------------|----------------------------|--------------------------------------------------------------------------------------------------|
| Same logical synapse time across shards                            | Sections 3.2–3.3, 6.1–6.4  | VT-CAUSAL-001 to 006; deterministic transport tests.                                             |
| Receiver knows no more prior events are coming                     | Sections 6.3, 7.1          | Route-watermark tests plus component termination/balance tests and missing-final-event scenario. |
| Circular dependencies without whole-brain lockstep                 | Sections 5.2, 6.4, 7.1     | SCC oracle, distributed-SCC and unrelated-progress tests.                                        |
| Superdense time and true quiescence                                | Sections 3, 6, 7           | Local reference interpreter and causal suite.                                                    |
| Record, defer and learn from non-convergence                       | Sections 7.3–7.6, 11.4     | Non-convergence validation and scheduler decision replay.                                        |
| Exact reproducibility where required                               | Section 8                  | Cross-thread/transport/failover exact digest suite.                                              |
| Multiple federated brains use all compute                          | Sections 11–12             | Fairness, isolation, federation and resource tests.                                              |
| Node loss does not destroy a brain                                 | Sections 14–15             | CT-001 to CT-012 and RPO/RTO report.                                                             |
| Periodic immutable checkpoints                                     | Section 14                 | Immutable publish, corruption and retention tests.                                               |
| Web and Rust UI manage authorised remote brains                    | Section 16                 | API security plus web/Rust end-to-end suites.                                                    |
| Workstation concurrently provides audio/video/keyboard/pointer and bidirectional USB AER input/output | Sections 16.15–16.24, 17.4 | UT-AERUSB-001/002, CT-016, IO-E2E-001 to 014, browser/native USB capability tests and privacy/safety review. |
| Failover/replay does not repeat physical action                    | Sections 14–16, 17.4       | UT-EFFECT-001, CT-013/015/016 and IO-E2E-006/010/014.                                             |
| Biological terminology and ownership are unambiguous               | Sections 3.3, 5.2, 9.2     | UT-SYN-001, VT-CAUSAL-009 and model schema validation.                                           |
| Best practice, modularity and documentation                        | Sections 9, 19, 22         | Architecture/dependency checks, CI lints and documentation validation.                           |

# Appendix E. Glossary

| **Term**             | **Definition**                                                                                                                                                                    |
|----------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Biological tick t    | Integer logical time in units of a brain’s configured base quantum.                                                                                                               |
| Microstep μ          | Zero-biological-time causal ordering dimension within a tick.                                                                                                                     |
| Synaptic transition  | Single-owner model event for a named release/conductance/electrical transition; the project’s precise replacement for the informal phrase “a synapse fires”.                      |
| Quiescence           | Proven participant passivity and absence of local or in-flight work for an active causal component/tag, using safe route horizons and cyclic termination detection as applicable. |
| Settling limit       | Safety/resource cap on microsteps; reaching it is non-convergence, not quiescence.                                                                                                |
| SCC                  | Strongly connected component of the conservative zero-delay graph.                                                                                                                |
| Virtual shard        | Stable unit of graph ownership and mutable state, independent of physical placement.                                                                                              |
| Active writer        | Sole replica authorised by current lease term/fencing token to commit shard state.                                                                                                |
| Warm backup          | Replica with synchronous durable causal log and bounded applied-state lag.                                                                                                        |
| Immutable checkpoint | Published state object that is never overwritten and is validated by checksum/digest.                                                                                             |
| Fast-forward         | Unpaced deterministic replay from checkpoint plus causal log to current committed frontier.                                                                                       |
| GVT                  | Asynchronous safe/global virtual-time estimate used for recovery and reclamation, not a per-tick barrier.                                                                         |
| Federation           | Explicit authorised links between independently timed whole-brain emulations.                                                                                                     |
| Effective capacity   | Measured useful throughput subject to device, load, memory, deterministic profile and communication cost.                                                                         |
| Peripheral session   | Ephemeral authorised, consented binding of workstation capture/render/actuation channels to one brain.                                                                            |
| Time-domain mapping  | Versioned mapping from an external monotonic capture clock to a brain logical tag, including drift and uncertainty.                                                               |
| Transducer           | Versioned transform from audio/video/display/HID or USB AER samples to biological sensory events.                                                                                 |
| USB AER channel      | Independently sequenced peripheral input or committed-output channel exchanging bounded address-event frames with an allow-listed local USB neuromorphic device.                 |
| Device epoch         | Monotonic session-local generation for one validated USB device connection; reset, removal or material renegotiation closes the old epoch.                                       |
| Actuator intent      | Committed neural output decoded into a bounded workstation presentation or action with stable EffectId.                                                                           |
| Actuator lease       | Fenced exclusive authority to apply effects on one effectful channel; passive A/V fan-out may use a different policy.                                                             |

# Appendix F. Evidence and standards basis

This appendix is informative: it records the principal basis for the corrections in this revision. Codex shall confirm the repository’s chosen libraries and the current normative status of web/platform specifications during implementation rather than copying API assumptions blindly.

| **Topic**                                            | **Source**                                                                                                                                                        | **Requirement consequence**                                                                                                                  |
|------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------|
| Superdense logical time and discrete-event semantics | Edward A. Lee, *Discrete Event Models: Getting the Semantics Right* (2006), https://ptolemy.berkeley.edu/projects/chess/pubs/205/DiscreteEventSystems_WSC.pdf     | Use ordered (tick, microstep) tags and distinguish model time from execution/wall-clock time.                                                |
| Distributed termination                              | E. W. Dijkstra and C. S. Scholten, *Termination detection for diffusing computations*, EWD684, https://www.cs.utexas.edu/~EWD/transcriptions/EWD06xx/EWD684.html  | Route silence/watermarks do not prove cyclic quiescence; require participant passivity and in-flight accounting.                             |
| Consistent distributed snapshots                     | K. M. Chandy and L. Lamport, *Distributed Snapshots: Determining Global States of Distributed Systems*, https://dl.acm.org/doi/10.1145/214451.214456              | Brain export/checkpoint manifests must represent a consistent cut including channel state without stopping all work.                         |
| Distributed simulation time management               | R. M. Fujimoto and R. M. Weatherly, *Time Management in the DoD High Level Architecture* (1996), https://sites.cc.gatech.edu/computing/pads/PAPERS/HLA-PADS96.pdf | Conservative ordering, independent federate progress and real-time bridges require explicit time-management contracts.                       |
| Consensus and single leadership                      | D. Ongaro and J. Ousterhout, *In Search of an Understandable Consensus Algorithm* (extended version), https://raft.github.io/raft.pdf                             | Membership/term decisions require quorum; fencing must extend to state, routes and external effects.                                         |
| Spiking-network simulation                           | R. Brette et al., *Simulation of networks of spiking neurons: a review of tools and strategies* (2007), https://pmc.ncbi.nlm.nih.gov/articles/PMC2638500/         | Preserve delays/event timing and state clearly which numerical/event-driven model is validated.                                              |
| Brain-wide coordination                              | R. V. Raut et al., *Global waves synchronize the brain’s functional systems with fluctuating arousal* (2021), https://pmc.ncbi.nlm.nih.gov/articles/PMC8294763/   | Avoid the overstatement that biology has no large-scale synchronisation; model oscillations/waves without imposing an executor-wide barrier. |
| Browser microphone/camera                            | W3C, *Media Capture and Streams*, https://www.w3.org/TR/mediacapture-streams/                                                                                     | Secure context, user permission, track capability and device lifecycle are part of the web requirements.                                     |
| Browser display capture                              | W3C, *Screen Capture*, https://www.w3.org/TR/screen-capture/                                                                                                      | The user selects the captured display surface; permission/termination are explicit and revocable.                                            |
| Browser keyboard/pointer boundaries                  | W3C, *UI Events*, https://www.w3.org/TR/uievents/ and *Pointer Lock 2.0*, https://www.w3.org/TR/pointerlock-2/                                                    | Web input is focused/sandboxed; Pointer Lock can provide relative motion but an ordinary page cannot claim global OS HID actuation.          |
| Real-time media and data security                    | W3C, *WebRTC*, https://www.w3.org/TR/webrtc/; IETF RFC 8831, https://www.rfc-editor.org/info/rfc8831/; RFC 8827, https://www.rfc-editor.org/info/rfc8827/         | Separate secure media/data plane, congestion control, DTLS/SRTP and explicit session signalling from management calls.                       |
| Media timestamps/codecs                              | W3C, *WebCodecs*, https://www.w3.org/TR/webcodecs/                                                                                                                | Preserve frame timestamps/durations and pin codec/transform versions for recorded replay; codec support is capability-driven.                |
| GPU reproducibility limits                           | NVIDIA, *cuBLAS Results Reproducibility*, https://docs.nvidia.com/cuda/cublas/index.html#results-reproducibility                                                  | Determinism claims are conditional on architecture, library version, stream/handle configuration and certified kernels.                      |

# Final implementation mandate

Codex shall treat causal correctness, single-owner synaptic state, one-writer durability, event preservation, external-effect deduplication, peripheral consent, server-side authorisation and reproducibility claims as safety properties. Performance work shall occur inside those boundaries. The delivered platform shall appear biologically event-driven to the model, operationally coherent to users, resilient to ordinary node/workstation loss, recovery and fleet expansion, and capable of concurrent authorised management plus explicitly bounded sensory/actuator interaction from web and Rust workstations, including simultaneous bidirectional USB AER, audio, visual and HID channels without cross-channel starvation or hidden retiming.
