"""Quasi-steady reduced panels v1. SI, X chord/Y span/Z normal.

Model assumptions are in policy.json. Forces derive from local air velocity,
never spike counts. Lift is perpendicular to relative velocity; drag always
removes mechanical energy in still air. Calibration/flight skills remain open.
"""
import math
import numpy as np


def panel_force(velocity_world, rotation, area, density=1.225):
    velocity = np.asarray(velocity_world, dtype=float)
    rotation = np.asarray(rotation, dtype=float).reshape(3,3)
    speed = np.linalg.norm(velocity)
    if not np.isfinite(speed) or area <= 0: raise ValueError('invalid aerodynamic inputs')
    if speed < 1e-9: return np.zeros(3)
    direction = velocity/speed
    local = rotation.T @ velocity
    # A downward-moving wing has positive incidence and upward lift. Reverse
    # flow is deliberately drag dominated rather than creating free lift.
    alpha = math.atan2(-local[2], abs(local[0])+1e-12)
    cl = max(-1.5,min(1.5,4.5*alpha)) * max(0.,math.cos(alpha))
    cd = .08 + .15*cl*cl + 1.2*math.sin(alpha)**2
    normal = rotation[:,2]
    lift = normal - direction*np.dot(normal,direction)
    length = np.linalg.norm(lift)
    if length > 1e-9: lift /= length
    return .5*density*speed*speed*area*(cl*lift - cd*direction)


def apply_panels(model,data,panels,wind):
    import mujoco
    for bid, local_point, area in panels:
        rotation=data.xmat[bid].reshape(3,3)
        point=data.xpos[bid]+rotation@np.asarray(local_point)
        velocity=np.zeros(6)
        mujoco.mj_objectVelocity(model,data,mujoco.mjtObj.mjOBJ_BODY,bid,velocity,0)
        local_velocity=velocity[3:]+np.cross(velocity[:3],point-data.xipos[bid])-wind
        force=panel_force(local_velocity,rotation,area)
        mujoco.mj_applyFT(model,data,force,np.zeros(3),point,bid,data.qfrc_applied)
