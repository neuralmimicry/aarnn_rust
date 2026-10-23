# Control contract

## Frames

- Right-handed frame: +X forward, +Y left, +Z up.
- Wheel order: front-left (`FL`), front-right (`FR`), rear-left (`RL`),
  rear-right (`RR`).
- The four actuator controls are normalised torque commands in `[-1, 1]`.
  A magnitude of 1 maps to the published approximate maximum of 0.196133 N·m.

## Inverse kinematics

For desired chassis velocity `(vx, vy, wz)`, wheel radius `r`, half-wheelbase
`L`, and half-track `W`, the conventional mecanum wheel angular speeds are:

```text
FL = (vx - vy - (L + W) wz) / r
FR = (vx + vy + (L + W) wz) / r
RL = (vx + vy - (L + W) wz) / r
RR = (vx - vy + (L + W) wz) / r
```

The simulation controller uses these signs directly. Verify motor signs on a
lifted physical robot before adapting this code to hardware.

## Important limitation

The passive-roller model produces mecanum behaviour through actual contacts. It
is more realistic but more numerically demanding than a planar-force plugin.
Floor material, solver timestep, roller friction, and contact softness strongly
affect lateral motion. Calibrate those parameters against measured trajectories.

## Circle controller

`tools/drive_circle.py` implements a PI wheel-speed loop over the four normalized
torque actuators. For a left circle, the left-side wheels follow the smaller
inner radius and the right-side wheels follow the larger outer radius. Example:

```powershell
python tools/drive_circle.py --radius 0.45 --speed 0.20 --direction left
```

The requested radius must exceed `L + W` (currently 0.18 m). Because the model's
contact and inertial parameters are estimates, the geometric target is nominal;
calibration is required for an accurate physical path.

