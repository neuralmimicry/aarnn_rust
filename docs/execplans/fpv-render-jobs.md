# FPV Neural Network Render Jobs

## Goal

Let an operator plan a camera flight over a bounded, read-only projection of a
large active neural network, submit an asynchronous render job, disconnect, and
later retrieve progress and the completed MP4 from shared storage. The network
continues its normal distributed execution throughout capture and rendering.

## Requirements

- The FPV planner uses bounded display projections and route-local spatial
  tiles; it never pauses or changes the execution graph to plan a camera path.
- A submitted job owns an immutable view/capture reference, camera path,
  resolution, frame rate, and rendering policy. Rendering does not advance,
  pause, or mutate the live network.
- Jobs have durable IDs and persisted queued/running/complete/failed state.
  Progress, retry, cancellation, and artifact retrieval remain available after
  a browser disconnect or web process restart.
- Every job is scoped to the authenticated user. List, status, cancellation,
  retry, and video retrieval enforce owner checks; no cross-user administrative
  view is currently exposed.
- Web, Android, and Rust UI clients submit to the shared API and show the same
  user-owned job list. The Rust CLI can list, inspect, cancel, or retry jobs.
- Frame work is independently claimable by multiple workers sharing the job
  store. Workers use bounded frame inputs/outputs, durable leases, and
  idempotent frame publication so a failed worker does not lose completed work.
- Camera focus can follow activity captured for the job. The user can also
  define waypoints in a minimal whole-network overview. MP4 encoding and
  per-frame intermediates are written incrementally to a configured shared
  output area.
- Optional video, audio, or AER synchronization is explicit. Replay must use an
  isolated saved network copy by default so a render cannot inject input into
  or perturb the always-running live network.
- UI traffic and inter-worker transfer use bounded chunks. Total snapshot or
  artifact size has no 64 MiB protocol ceiling; storage and process allocation
  failures are reported as job failures with resumable progress.
- The shared nine-stage visualisation selector is available while planning a
  route. Each camera waypoint stores either a manual stage or `Auto`; immutable
  jobs resolve those keyframes per frame and publish the selected-stage track
  with the MP4 metadata. FPV geometry remains a read-only projection.

## Implementation Checklist

- [x] Add an FPV Studio surface with a sampled topology overview, waypoint
  editing, selectable overview detail (512/2,048/4,096 neurons), output
  resolution/frame-rate controls, automatic activity focus, and a reconnectable
  jobs list. Add waypoint planning and shared job submission/status to Android
  and Rust UI.
- [x] Define a versioned job status/request contract, validate geometry and resource
  bounds, and persist them beneath the deployment-configured shared runtime
  root.
- [x] Add a multi-worker frame queue with process-shared leases, retries,
  cancellation, monotonic progress, and idempotent frame publication.
- [x] Render frames deterministically from immutable display/activity inputs;
  encode MP4 without buffering the full video in memory.
- [x] Add synchronized normalized AER/audio/video event tracks on an isolated
  imported network copy and record snapshot/input hashes with replay progress.
  Media decoders must convert source files to timestamped sensory events before
  submission; decoding/container-specific adapters remain future work.
- [x] Add route-local spatial projection requests that filter neurons before
  applying tile budgets. Web jobs merge up to 16 route tiles into a bounded
  scene; the per-job ceiling is 65,536 neurons and 131,072 edges. This bounds
  transfer and frame memory while keeping the live network independent.
- [x] Mount the shared job area for the web gateway and worker deployments,
  place three dedicated workers across eligible hosts, and include/verify
  FFmpeg in the web-ui runtime image.
- [ ] Extend tests for process restart, duplicate worker claims, damaged frame
  detection, and large output transfer. Basic owner isolation, frame rendering,
  MP4 publication, retry, and storage bounds are covered.
- [x] Add Rust CLI commands (`fpv jobs`, `fpv status`, `fpv cancel`, `fpv retry`)
  against the same authenticated API used by the UI clients.
- [x] Carry manual/automatic per-waypoint visualisation stages through Rust,
  web and Android planners into the persistent render request; test deterministic
  route interpolation and stage-filtered frame output. Rust tests cover
  immutable zoom/latency-based Auto selection and per-frame stage tracks,
  including policy-version persistence and rejection of unknown versions; the
  shared browser fixture checks stage and label parity; Android unit tests and
  Kotlin compilation pass.

## Current Status

The live display path returns bounded projections. Web, Android, and Rust UI
planners submit durable jobs to one authenticated API; Rust UI and CLI clients
can inspect the same owner-scoped progress. The web planner requests spatial
tiles around route waypoints and caps a render scene at 65,536 neurons. Regional
selection currently scans the source layer arrays, so a spatial index is still
needed to keep tile lookup latency low for billion-neuron networks. Optional
replay uses timestamped sensory events on a separately imported snapshot; file
decoders for raw video/audio/AER formats remain outside this worker contract.
VIS-12 waypoint settings and the policy version are captured in the durable
request and resolved into an immutable per-frame stage track. Validation covers
route interpolation, stage-filtered frame output and fail-closed version
handling. Android tests pass on Java 21 with the installed SDK; this client
boundary has no iOS FPV planner.

CLI examples (the token and API URL can instead be supplied with
`NM_AARNN_ACCESS_TOKEN` and `NM_AARNN_API_URL`):

```sh
aarnn_rust fpv jobs --api-url https://aarnn.example
aarnn_rust fpv status fpv-<job-id> --api-url https://aarnn.example
aarnn_rust fpv cancel fpv-<job-id> --api-url https://aarnn.example
aarnn_rust fpv retry fpv-<job-id> --api-url https://aarnn.example
```
