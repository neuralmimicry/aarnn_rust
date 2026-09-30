# ADR-0007: Keep server-managed simulation input available without a viewer

Status: Accepted for virtual simulation gateways only. Production enablement
still requires the per-network grant, service identity and rollout checks in
this decision; it does not enable workstation or physical-device I/O.

## Context

The Webots world and its robot controllers run as a persistent server service.
The browser is an authenticated observer and may disconnect while the world
continues. Its controllers submit virtual sensor readings under the `webots`
service identity. These readings are not captured from a user's local camera,
microphone, keyboard, display, USB device or other physical peripheral.

The ordinary `/api/aer/inject` route is a workstation peripheral route. Its
exact brain grant and short-lived locally consented PeripheralSession are
required together. Treating a robot controller's idempotency `session_id` as
that consent session would weaken INV-017. Requiring the browser observer to
renew a workstation session would also couple the independent server world to
the viewer's connection.

## Decision

1. Preserve `/api/aer/inject` and all workstation peripheral routes with their
   existing exact-scope, active-session, local-consent and revoke checks.
2. Add a separate `/api/simulation/aer/inject` route for virtual simulation
   gateways. It accepts only authenticated service accounts on the explicit
   simulation-ingress principal allow-list and requires the existing exact
   brain-scoped `PeripheralInput` grant plus `aarnn:use`. The allow-list names
   service identities; it does not duplicate the per-brain permission map.
3. The simulation route uses the same bounded, placement-aware prepare/commit
   admission as the workstation route. It requires a stable producer
   `session_id` and source step for idempotency. A producer ID is not an
   authorization credential.
4. The route accepts virtual-world observations only. It cannot capture local
   devices, create a PeripheralSession, attach USB/AER hardware or authorize
   physical/global actuation. Webots actuators remain inside the sandboxed
   simulator.
5. The service permission is independent of browser sessions. Disconnecting
   a viewer does not stop the Webots systemd service. Removing the exact
   per-brain grant, removing the service identity from the allow-list or
   stopping the world service denies further simulated inputs. The viewer
   continues to require its normal authenticated browser session.
6. Keep the route disabled unless authentication is active and both the
   service-principal allow-list and exact brain-scoped grant are configured.
   Log the service principal, network, producer session and source step, never
   the bearer or raw neural payload.
7. In a multi-robot deployment, use one shared monotonic wall-clock reference
   for pacing deadlines and measuring per-network computation/communication
   latency. Record versioned host-clock mappings and uncertainty. A network
   that finishes a cycle earlier may proceed independently; the shared
   reference never advances or orders biological events and creates no
   slowest-network barrier. Each brain retains its own logical clock and
   source-time mapping, as specified in Section 3.4.

## Consequences

The server-side ecology can continue after the browser disconnects without
falsifying workstation consent or introducing a fleet-wide neural barrier.
Network/API latency affects when a motor output reaches its simulator, while
the common Webots world clock continues independently. Controllers use the
shared monotonic reference to measure cycle deadlines and per-network latency;
a faster network need not wait for a slower one. Each AARNN brain keeps its
own logical time and versioned source-time mapping, and that wall-clock
reference never determines causal order.

Revocation is enforced on every input request through the authenticated
principal and existing brain-scoped grant. Rollout must update the API route
and Webots client together; the old and new endpoint contracts are not
interchangeable. This ADR does not claim replicated Phase 7 management
authority, browser acceptance or neural-output acceptance.

Authority: user request for a persistent shared Webots world and common
simulation clock; `INV-002`, `INV-007`, `INV-015` and `INV-017`; Sections
16.5, 16.16–16.18, 17.4 and 21.13 of the distributed emulator specification.
