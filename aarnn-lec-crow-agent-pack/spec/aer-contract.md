# AER and brain binding contract

## Verified protocol boundaries

| Interface | Existing source | Contract and limit |
|---|---|---|
| AER1 reference TCP/UDS | `src/aer.rs`, `examples/nn_tcp_server.rs`, `examples/nn_uds_server.rs` | Framed legacy simulator protocol; no production peripheral-lease guarantee follows from a packet. |
| AARNN-AER/1 | `src/aer_transport.rs`, `src/peripheral.rs` | Separate governed envelope with endpoint/device epochs, sequences, capture mapping/version/uncertainty, address-space version and effect metadata. |
| Browser inference | `src/bin/web_ui.rs`, `web_ui/webgl-sim.js` | `/api/aer/infer` calls the existing peripheral authorisation path; preserve the exact brain/session grant contract. |
| Managed virtual input | ADR-0007 and `/api/simulation/aer/inject` | Authenticated allow-listed simulation service plus exact brain-scoped input grant, stable producer session and source step. Input admission is not itself an output lease. |
| FPAA UDP fabric | `aer_fabric_bridge/` | Separate 64-bit synapse-addressed experimental fabric. Do not reinterpret AER1 bytes as its wire format or truncate its addresses. |

## AER1 compatibility details

Frames use a four-byte little-endian payload length. The initial JSON handshake supplies ordered `s_names`, `o_names`, `sensory` and `output`; the response returns `expected_s`/`expected_o`. Validate shape and the pinned external mapping before a neural network is bound. Existing reference servers can adapt shape, so the new binding layer must reject accidental mismatch rather than resize a trained brain.

An event payload is `AER1` + little-endian u64 base timestamp in microseconds + unsigned varint delta-time/address/value triples. Input base is 4096 and output base 16384 by default; validate negotiated address windows rather than assuming them globally. Header-only payloads represent no spikes; the Rust helper’s empty Vec case must not become an invalid zero-length TCP frame. Raw-float fallback is a different negotiated reference path, not an implicit reinterpretation of arbitrary bytes.

In the inspected Rust decoder, value zero is ignored by spike application and nonzero is a spike. A negative motor command must therefore not be encoded as value zero. For new joint controllers, prefer separate positive/negative output populations; filter spike counts using simulation time and apply the declared bounded signed drive. Zero neural output returns active drive towards its declared neutral policy while inertia/gravity remain physical. Store filter/integrator state for replay.

Frame parsing must bound lengths/events/varints, validate address ranges and reject overflow/malformed input. Do not silently inherit the legacy decoder’s permissive casts as the new admission validation policy. Keep wire changes versioned and backwards-compatible, with golden fixtures across Rust/C++/C#/Java/JavaScript/Python as applicable.

## Per-instance binding

Required binding metadata: `robot_instance_id`, `robot_profile_id`, `variant_id`, `model_digest`, `io_digest`, `mapping_version`, `brain_id`, `brain_generation`, `binding_generation`, input/output endpoint IDs, session/stream IDs, reset/device/path epochs, sensory/output address windows, capture sequence/time/mapping/uncertainty, source physics step and output fence/effect cursor. Reuse the repository’s stable typed identities and DTOs; these task field names describe information requirements, not a new unversioned wire schema.

One body has one active physics authority and one active sensory producer for a declared stream. A WebGL/Minecraft viewer attached to a physics service does not send a duplicate sensory frame. Multiple viewers cannot acquire independent actuator writers. Independent worlds cloning the same brain snapshot receive separate brain/session identities unless an explicitly authorised shared-controller experiment defines otherwise.

## Admission, output and clocks

Simulation capture time maps through a pinned rational/source-clock mapping to the brain’s logical tags; retain capture sequence, mapping version and uncertainty. Network arrival time never becomes biological time. Fractional source steps must not accumulate rounding drift. World pacing may be real-time, accelerated, paused or replayed without changing these semantics.

Use existing durable admission and commit boundaries when claiming governed mode. Preserve EffectId, binding generation, source tag, quality, lease/fence, dedupe and applied/unknown states. Do not automatically retry an ambiguous effect as a new command after reconnect. Reference/sandbox mode must say which of these guarantees it lacks. Do not describe a successful TCP exchange as proof of production effects.

Bound queues separately for each robot and modality. Late pre-admission samples follow a declared reject/resample/gap policy; committed events are never silently dropped. Stop/revoke has a reserved local path and disarms before a network round trip. Disconnect/restart clears stale output buffers and creates the appropriate epoch; visual interpolation is never fed back as sensed state.

## Optional FPAA bridge

If enabled in an explicitly selected experimental configuration, map stable channel/neural identities to registered 64-bit synapse IDs through a versioned LUT. Validate ownership, permissions, clock translation, TTL, topology generation and positive delays. Use mock hardware for initial tests. The simulation expansion does not require GPIO, SPI, Pi.Ka or physical robot access.

## Required end-to-end evidence

Known scene perturbation -> changed named sensor sample -> correctly timestamped AER admission into the intended brain -> committed output with identity -> correct named drive -> measured physical response -> changed next sensor sample. Repeat for two same-model instances and different brains; deliberately swap a mapping/brain, duplicate a reply, restart an endpoint and revoke access. A mocked packet is only protocol evidence; final neural-loop evidence must run the actual AARNN runtime.
