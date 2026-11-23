// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Quasi-steady panels v1, SI X chord/Y span/Z normal. Uncalibrated reduced
//! crow aerodynamics; lift is perpendicular to velocity and drag dissipates.
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(x, y)| x * y).sum()
}
fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    a.map(|v| v * s)
}
pub fn panel_force(
    velocity: [f64; 3],
    rotation: [[f64; 3]; 3],
    area: f64,
) -> Result<[f64; 3], &'static str> {
    if area <= 0.
        || !velocity
            .into_iter()
            .chain(rotation.into_iter().flatten())
            .chain([area])
            .all(f64::is_finite)
    {
        return Err("invalid panel input");
    }
    let speed = dot(velocity, velocity).sqrt();
    if speed < 1e-9 {
        return Ok([0.; 3]);
    }
    let direction = scale(velocity, 1. / speed);
    let local: [f64; 3] =
        std::array::from_fn(|i| (0..3).map(|j| rotation[j][i] * velocity[j]).sum());
    let alpha = (-local[2]).atan2(local[0].abs() + 1e-12);
    let cl = (4.5 * alpha).clamp(-1.5, 1.5) * alpha.cos().max(0.);
    let cd = 0.08 + 0.15 * cl * cl + 1.2 * alpha.sin().powi(2);
    let normal: [f64; 3] = std::array::from_fn(|i| rotation[i][2]);
    let projection = dot(normal, direction);
    let mut lift: [f64; 3] = std::array::from_fn(|i| normal[i] - direction[i] * projection);
    let magnitude = dot(lift, lift).sqrt();
    if magnitude > 1e-9 {
        lift = scale(lift, 1. / magnitude);
    }
    Ok(std::array::from_fn(|i| {
        0.5 * 1.225 * speed * speed * area * (cl * lift[i] - cd * direction[i])
    }))
}
/// Quasi-steady panel with a declared polar (`quasi-steady-panel-v2`, scene
/// `aerodynamics`). Same frame and lift direction as [`panel_force`] (X chord,
/// Y span, Z normal; lift perpendicular to the relative wind, drag along
/// it), with the coefficients of an attached-flow wing instead of a flat
/// plate: `polar = [lift_slope_per_rad, lift_max, drag_zero, drag_induced,
/// drag_stall]` gives `CL = clamp(slope a, +-max) cos a`,
/// `CD = drag_zero + drag_induced CL^2 + drag_stall sin^2(max(|a| - a_stall, 0))`
/// with `a_stall = max / slope` (no stall drag without a lift slope, so a
/// zero-slope polar is isotropic body drag). Drag never supplies energy.
pub fn polar_panel_force(
    velocity: [f64; 3],
    rotation: [[f64; 3]; 3],
    area: f64,
    density: f64,
    polar: &[f64; 5],
) -> Result<[f64; 3], &'static str> {
    let [slope, lift_max, drag_zero, drag_induced, drag_stall] = *polar;
    if area <= 0.
        || density <= 0.
        || polar.iter().any(|c| *c < 0.)
        || !velocity
            .into_iter()
            .chain(rotation.into_iter().flatten())
            .chain([area, density])
            .chain(*polar)
            .all(f64::is_finite)
    {
        return Err("invalid polar panel input");
    }
    let speed = dot(velocity, velocity).sqrt();
    if speed < 1e-9 {
        return Ok([0.; 3]);
    }
    let direction = scale(velocity, 1. / speed);
    let local: [f64; 3] =
        std::array::from_fn(|i| (0..3).map(|j| rotation[j][i] * velocity[j]).sum());
    let alpha = (-local[2]).atan2(local[0].abs() + 1e-12);
    let cl = (slope * alpha).clamp(-lift_max, lift_max) * alpha.cos().max(0.);
    let stall = if slope > 0. {
        lift_max / slope
    } else {
        f64::INFINITY
    };
    let cd = drag_zero
        + drag_induced * cl * cl
        + drag_stall * (alpha.abs() - stall).max(0.).sin().powi(2);
    let normal: [f64; 3] = std::array::from_fn(|i| rotation[i][2]);
    let projection = dot(normal, direction);
    let mut lift: [f64; 3] = std::array::from_fn(|i| normal[i] - direction[i] * projection);
    let magnitude = dot(lift, lift).sqrt();
    lift = if magnitude > 1e-9 {
        scale(lift, 1. / magnitude)
    } else {
        [0.; 3]
    };
    Ok(std::array::from_fn(|i| {
        0.5 * density * speed * speed * area * (cl * lift[i] - cd * direction[i])
    }))
}
/// Water column of a `resistive-ellipsoid-v1` scene (zebrafish v2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Water {
    pub density: f64,
    pub surface_z: f64,
    pub bottom_z: f64,
    /// Depth over which a body passes linearly from dry to submerged.
    pub band: f64,
    /// Quadratic drag coefficients along the ellipsoid's own x, y, z axes.
    pub drag: [f64; 3],
    /// Scale of the strip-theory rotational drag (uses the normal `drag[1]`).
    pub rotational: f64,
}
impl Water {
    /// Fraction of a point at height `z` inside the water column.
    pub fn submersion(&self, z: f64) -> f64 {
        let band = self.band.max(1e-9);
        ((self.surface_z - z + 0.5 * band) / band).clamp(0., 1.)
            * ((z - self.bottom_z + 0.5 * band) / band).clamp(0., 1.)
    }
    /// Neutral buoyancy of a body of `mass` (displaced volume = mass / rho).
    pub fn buoyancy(&self, mass: f64, z: f64) -> [f64; 3] {
        [0., 0., mass * 9.81 * self.submersion(z)]
    }
}
/// Quasi-steady resistive drag on a body-aligned hydrodynamic ellipsoid.
///
/// `half` are the semi-axes along the body's x, y, z; `rotation` maps body to
/// world coordinates (columns are the body axes); `velocity` is the world
/// velocity of the ellipsoid centre and `angular` the world angular velocity.
/// Per body axis k: force `-0.5 rho Cd_k A_k |u_k| u_k` with the projected
/// areas `A = pi (bc, ac, ab)`; torque about axis k from strip theory,
/// `-0.5 rho Cd_n s h_k (h_i^4 + h_j^4) |w_k| w_k`. Returns world force and
/// torque, both scaled by `submerged` in [0, 1]. Both always dissipate.
pub fn ellipsoid_drag(
    water: &Water,
    half: [f64; 3],
    rotation: [[f64; 3]; 3],
    velocity: [f64; 3],
    angular: [f64; 3],
    submerged: f64,
) -> Result<([f64; 3], [f64; 3]), &'static str> {
    if !half
        .into_iter()
        .chain(rotation.into_iter().flatten())
        .chain(velocity)
        .chain(angular)
        .chain([submerged])
        .all(f64::is_finite)
        || half.iter().any(|h| *h <= 0.)
    {
        return Err("invalid hydrodynamic ellipsoid");
    }
    let local = |v: [f64; 3]| -> [f64; 3] {
        std::array::from_fn(|i| (0..3).map(|j| rotation[j][i] * v[j]).sum())
    };
    let world = |v: [f64; 3]| -> [f64; 3] {
        std::array::from_fn(|i| (0..3).map(|j| rotation[i][j] * v[j]).sum())
    };
    let (u, w) = (local(velocity), local(angular));
    let [a, b, c] = half;
    let area = [
        std::f64::consts::PI * b * c,
        std::f64::consts::PI * a * c,
        std::f64::consts::PI * a * b,
    ];
    let s = submerged.clamp(0., 1.);
    let force: [f64; 3] = std::array::from_fn(|k| {
        -0.5 * water.density * water.drag[k] * area[k] * u[k].abs() * u[k] * s
    });
    let torque: [f64; 3] = std::array::from_fn(|k| {
        let (i, j) = ((k + 1) % 3, (k + 2) % 3);
        -0.5 * water.density
            * water.drag[1]
            * water.rotational
            * half[k]
            * (half[i].powi(4) + half[j].powi(4))
            * w[k].abs()
            * w[k]
            * s
    });
    Ok((world(force), world(torque)))
}
/// Neutral-density engineering water, separate from neural control. The
/// displaced volume is mass/rho; quadratic drag always opposes translation.
pub fn water_force(mass: f64, z: f64, velocity: [f64; 3]) -> Result<[f64; 3], &'static str> {
    if mass < 0.
        || ![mass, z, velocity[0], velocity[1], velocity[2]]
            .into_iter()
            .all(f64::is_finite)
    {
        return Err("invalid water state");
    }
    let submerged = ((0.4 - z + 0.006) / 0.012).clamp(0., 1.) * ((z + 0.006) / 0.012).clamp(0., 1.);
    let volume = mass / 1000.;
    let area = volume.powf(2. / 3.);
    let mut force = velocity.map(|v| -0.5 * 1000. * area * v.abs() * v * submerged);
    force[2] += 1000. * volume * 9.81 * submerged;
    Ok(force)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn still_air_cannot_supply_energy() {
        let r = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
        for v in [[8., 0., 0.], [8., 0., -2.], [8., 0., 2.], [-3., 2., 1.]] {
            let f = panel_force(v, r, 0.045).unwrap();
            assert!(dot(f, v) <= 1e-9);
        }
        assert_eq!(panel_force([0.; 3], r, 0.045).unwrap(), [0.; 3]);
    }
    const WATER: Water = Water {
        density: 1000.,
        surface_z: 0.4,
        bottom_z: 0.,
        band: 0.012,
        drag: [0.1, 1.0, 1.0],
        rotational: 1.0,
    };
    #[test]
    fn ellipsoid_drag_is_anisotropic_dissipative_and_frame_covariant() {
        let half = [0.005, 0.004, 0.007];
        let eye = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
        // Lateral (normal) motion meets far more drag than axial motion.
        let (fx, _) = ellipsoid_drag(&WATER, half, eye, [0.1, 0., 0.], [0.; 3], 1.).unwrap();
        let (fy, _) = ellipsoid_drag(&WATER, half, eye, [0., 0.1, 0.], [0.; 3], 1.).unwrap();
        assert!(fy[1].abs() > 8. * fx[0].abs() && fx[0] < 0. && fy[1] < 0.);
        // Power is never positive, for any rotation and velocity.
        let yaw = 0.7f64;
        let r = [
            [yaw.cos(), -yaw.sin(), 0.],
            [yaw.sin(), yaw.cos(), 0.],
            [0., 0., 1.],
        ];
        for (v, w) in [
            ([0.2, -0.1, 0.05], [1., -2., 3.]),
            ([-0.3, 0.2, 0.], [0., 0., -5.]),
        ] {
            let (f, t) = ellipsoid_drag(&WATER, half, r, v, w, 1.).unwrap();
            assert!(dot(f, v) <= 0. && dot(t, w) <= 0.);
        }
        // Rotating the body and the velocity together rotates the force.
        let (f_local, _) = ellipsoid_drag(&WATER, half, eye, [0.1, 0.05, 0.], [0.; 3], 1.).unwrap();
        let v_world = [
            0.1 * yaw.cos() - 0.05 * yaw.sin(),
            0.1 * yaw.sin() + 0.05 * yaw.cos(),
            0.,
        ];
        let (f_world, _) = ellipsoid_drag(&WATER, half, r, v_world, [0.; 3], 1.).unwrap();
        let expect = [
            f_local[0] * yaw.cos() - f_local[1] * yaw.sin(),
            f_local[0] * yaw.sin() + f_local[1] * yaw.cos(),
        ];
        assert!((f_world[0] - expect[0]).abs() < 1e-12 && (f_world[1] - expect[1]).abs() < 1e-12);
        // Dry bodies feel nothing; buoyancy matches the legacy envelope.
        assert_eq!(
            ellipsoid_drag(&WATER, half, eye, [1., 1., 1.], [1.; 3], 0.).unwrap(),
            ([0.; 3], [0.; 3])
        );
        for z in [-0.01, 0.0, 0.2, 0.399, 0.41] {
            let legacy = water_force(0.002, z, [0.; 3]).unwrap()[2];
            assert!((WATER.buoyancy(0.002, z)[2] - legacy).abs() < 1e-12);
        }
        assert!(ellipsoid_drag(&WATER, [0., 1., 1.], eye, [0.; 3], [0.; 3], 1.).is_err());
    }
    #[test]
    fn buoyancy_tracks_displacement_drag_is_passive_and_dry_bodies_are_unsupported() {
        let mass = 0.002;
        for v in [[1., -2., 0.], [-1., 2., -3.], [0., 0., 1.]] {
            let f = water_force(mass, 0.2, v).unwrap();
            assert!(dot([f[0], f[1], f[2] - mass * 9.81], v) <= 0.);
        }
        assert_eq!(water_force(mass, 1., [0.; 3]).unwrap(), [0.; 3]);
        assert!((water_force(mass, 0.2, [0.; 3]).unwrap()[2] - mass * 9.81).abs() < 1e-12);
    }
    #[test]
    fn polar_panels_are_dissipative_lift_linearly_and_body_drag_is_isotropic() {
        let wing = [4.9, 1.5, 0.025, 0.055, 1.2];
        let r = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
        for v in [
            [10., 0., -1.],
            [10., 0., 1.],
            [-3., 2., 1.],
            [0., 0., -5.],
            [8., 3., -2.],
        ] {
            let f = polar_panel_force(v, r, 0.02, 1.225, &wing).unwrap();
            assert!(dot(f, v) <= 1e-9, "panel supplied energy at {v:?}");
        }
        // Moving forward and slightly down: air meets the panel from below,
        // so lift is upward; at 0.1 rad CL = 0.49 cos 0.1 and drag is small.
        let a = 0.1f64;
        let v = [10. * a.cos(), 0., -10. * a.sin()];
        let f = polar_panel_force(v, r, 0.02, 1.225, &wing).unwrap();
        let q = 0.5 * 1.225 * 100. * 0.02;
        let cl = 4.9 * a * a.cos();
        let lift = f[2] * a.cos() + f[0] * a.sin();
        assert!((lift - q * cl).abs() < 1e-9);
        assert!(lift > 9. * (-(f[0] * a.cos() - f[2] * a.sin())));
        // A zero-slope polar is pure drag, the same in every direction.
        let body = [0., 0., 0.25, 0., 0.];
        let fx = polar_panel_force([10., 0., 0.], r, 0.01, 1.225, &body).unwrap();
        let fz = polar_panel_force([0., 0., 10.], r, 0.01, 1.225, &body).unwrap();
        assert!((fx[0] + 0.5 * 1.225 * 100. * 0.01 * 0.25).abs() < 1e-12);
        assert!((fx[0] - fz[2]).abs() < 1e-12 && fx[1] == 0. && fz[0] == 0.);
        assert_eq!(
            polar_panel_force([0.; 3], r, 0.02, 1.225, &wing).unwrap(),
            [0.; 3]
        );
        assert!(polar_panel_force([1., 0., 0.], r, 0.02, 1.225, &[-1., 1., 0., 0., 0.]).is_err());
        assert!(polar_panel_force([1., 0., 0.], r, 0., 1.225, &wing).is_err());
    }
}
