# LLM mirror inference contract

`POST /api/llm/mirror` is a neuromorphic stimulation and inference endpoint,
not a text echo service.

Input text is projected into a deterministic bottom-hash fingerprint with a
target density of 25% (capped at 64 active sensory neurons). This prevents long
messages from saturating the sensory layer while preserving a stable,
whole-message distinction for replay.

When a network and node are configured, the web service injects the sensory AER
frame and asynchronously polls `GetNetworkActivity` until it observes a later
simulation step. Candidate `output_spike_indices` and
`output_aer_payload_hex` are taken from that actual network output.

Text decoding is optional and explicit. Configure either:

- `AARNN_LLM_OUTPUT_VOCAB_JSON`, a JSON object such as
  `{"0":"hold","1":"buy","2":"sell"}`; or
- `AARNN_LLM_OUTPUT_VOCAB_PATH`, a file containing the same object.

Only mapped output spikes produce a `network_output_decoder` candidate.
Unmapped or unavailable outputs have no reply text and `usable=false`; the
original mirrored LLM response is never returned as an AARNN answer.

`AARNN_LLM_INFERENCE_TIMEOUT_MS` controls the bounded asynchronous output wait
(default 5000 ms, allowed range 50–60000 ms).

## Memory controls

Runtime reconciliation reuses existing workspace engines rather than importing
every snapshot again on each status poll. Set
`NM_RUNTIME_MAX_LOADED_WORKSPACES` to cap resident engines; workspace creation
and restart loading fail closed once that limit is reached.
