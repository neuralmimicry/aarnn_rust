# Remote observation of the headless simulation stack

The SwarmHPC Ansible role runs the six AARNN robot networks on one Intel
Ubuntu host, normally sm00 (192.168.1.66) or sm01 (192.168.1.68).
The Bedrock world and Rust companion are long-running services. The neural
brain endpoints remain loopback-only on ports 7890 through 7895; remote
clients should use the observation API or an SSH tunnel rather than exposing
the AER endpoints.

Set the active host in Ansible and retain the same host address in the commands
below:

~~~bash
SIM_HOST=192.168.1.66
ssh pbisaacs@$SIM_HOST 'sudo systemctl status aarnn-simulation-stack aarnn-sim-router'
curl http://$SIM_HOST:8790/healthz
curl -H "Authorization: Bearer $AARNN_SIM_ROUTER_TOKEN" http://$SIM_HOST:8790/api/status
ssh pbisaacs@$SIM_HOST 'sudo journalctl -u aarnn-simulation-stack -f'
~~~

The router state identifies each robot, its current environment, last reported
position, and accepted handoff. An engine adapter enters the shared gateway by
calling:

~~~http
POST /api/zone-enter
Authorization: Bearer <router-token>
Content-Type: application/json

{
  "robot_id": "celegans_0",
  "source": "webots",
  "target": "minecraft",
  "zone_id": "aarnn-simulation-gateway",
  "position": [0.0, 128.0, -8.0]
}
~~~

The router starts the declared destination systemd unit and returns the
accepted target. The engine adapter owns the final scene/entity placement and
must preserve the brain ID and profile mapping during the handoff.

## Minecraft Bedrock

Connect a compatible Bedrock client to:

~~~text
Address: 192.168.1.66
Port:    19132/UDP
World:   AARNN-Sensory-Lab
~~~

Use 192.168.1.68 after selecting sm01 as the active host. The service
constructs the six-profile lab, creates the visible gateway ring at
(0,128,-8), arms all six profiles, and keeps the world running after SSH
disconnects. Use the router status endpoint and BDS journal for machine
observation; the Bedrock client supplies the visual observation.

## Webots

The role's Webots unit uses the shared AARNN world and the six profile
configuration. Its default launch is batch/fast and therefore emits logs and
telemetry without a desktop window. Observe it remotely with:

~~~bash
ssh pbisaacs@$SIM_HOST 'sudo systemctl status aarnn-sim-engine-webots && sudo journalctl -u aarnn-sim-engine-webots -f'
curl -H "Authorization: Bearer $AARNN_SIM_ROUTER_TOKEN" http://$SIM_HOST:8790/api/status
~~~

For a graphical Webots observation, run a licensed Webots GUI on the observer
workstation and use the AARNN launcher with its remote Webots/controller
options. The headless service remains the authoritative brain host; do not
expose ports 7890–7895 directly. Supply
robot_simulation_stack_webots_archive_url before enabling the unit.

## Unreal Engine

The Unreal unit is configured for -game -nullrhi -nosound -unattended. This
is a server-side headless mode: it is suitable for autonomous execution,
teleport handoff, logs, and router state, but it intentionally produces no
remote framebuffer. Observe the running process with:

~~~bash
ssh pbisaacs@$SIM_HOST 'sudo systemctl status aarnn-sim-engine-unreal && sudo journalctl -u aarnn-sim-engine-unreal -f'
curl -H "Authorization: Bearer $AARNN_SIM_ROUTER_TOKEN" http://$SIM_HOST:8790/api/status
~~~

To view pixels, deploy a separate Unreal build configured for Pixel Streaming
or remove -nullrhi under the site's explicit graphical-runtime override.
Forward the selected streaming port over SSH or place it behind an
authenticated reverse proxy. The AARNN TCP brain endpoints still remain
loopback-only.

## Unity

The Unity unit is configured for -batchmode -nographics. It supplies the same
headless autonomous and handoff behavior as Unreal and is observed through
systemd logs and the router:

~~~bash
ssh pbisaacs@$SIM_HOST 'sudo systemctl status aarnn-sim-engine-unity && sudo journalctl -u aarnn-sim-engine-unity -f'
curl -H "Authorization: Bearer $AARNN_SIM_ROUTER_TOKEN" http://$SIM_HOST:8790/api/status
~~~

For visual observation, deploy a separate Unity Render Streaming or workstation
build and configure its authenticated streaming endpoint. -nographics cannot
produce images by itself. Supply robot_simulation_stack_unity_archive_url and
the project path before enabling the unit.

These observation paths are deliberately separate from AARNN's production
workstation I/O and from the legacy AER1 simulator transport. They expose
bounded simulator state and logs, not credentials, checkpoints, or unrestricted
actuation.
