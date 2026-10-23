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
}
