// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Engine-neutral numerics of the preserved legacy sensory names, matching the
//! MuJoCo reference (`adapters/mujoco/src/legacy.rs`) term for term. Webots
//! measurements are gathered by the controller; this module only normalises.
//!
//! Exact in Webots: `legacy-angle`, `legacy-contact` (with and without a
//! bumper direction), `legacy-gyro`, `legacy-flow`, `legacy-depth`,
//! `legacy-pitch`. Engineering proxies (declared in the run evidence):
//! `legacy-accel` (finite-differenced supervisor velocity plus gravity, not
//! MuJoCo's `cacc`), `legacy-range` (the root laser ray nearest in yaw, not a
//! ray from the declared mount point) and `legacy-camera` (rendered luminance
//! of the cell centre, not MuJoCo's ray-cast light proxy).

/// Kinds this adapter projects. Anything else keeps the neural binding refused.
pub const SUPPORTED: [&str; 9] = [
    "legacy-angle",
    "legacy-contact",
    "legacy-gyro",
    "legacy-flow",
    "legacy-depth",
    "legacy-pitch",
    "legacy-accel",
    "legacy-range",
    "legacy-camera",
];
/// Kinds whose Webots value is an engineering proxy of the MuJoCo reference.
pub const PROXY: [&str; 3] = ["legacy-accel", "legacy-range", "legacy-camera"];

pub fn norm(v: f64, a: f64, b: f64) -> f64 {
    ((v - a) / (b - a)).clamp(0., 1.)
}
/// DeviceMapper position heuristic: -3.14..3.14 rad (deliberately not PI).
pub fn angle(q: f64) -> f64 {
    const HALF_RANGE: f64 = 314. / 100.;
    norm(q, -HALF_RANGE, HALF_RANGE)
}
pub fn rotate(r: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| (0..3).map(|j| r[i][j] * v[j]).sum())
}
pub fn local(r: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| (0..3).map(|j| r[j][i] * v[j]).sum())
}
/// Legacy Y-up device axes from the native Z-up body frame.
pub fn axes(v: [f64; 3], mode: Option<&str>) -> [f64; 3] {
    if mode == Some("legacy-y-up") {
        [v[0], v[2], -v[1]]
    } else {
        v
    }
}
/// Accelerometer (`legacy-accel`) and gyro (`legacy-gyro`) normalisation.
pub fn inertial(v: [f64; 3], axis: usize, mode: Option<&str>, accel: bool) -> f64 {
    let bound = if accel { 20. } else { 10. };
    norm(axes(v, mode)[axis], -bound, bound)
}
/// Specific force in world coordinates from two consecutive COM velocities.
pub fn specific_force(previous: [f64; 3], current: [f64; 3], dt: f64) -> [f64; 3] {
    std::array::from_fn(|i| (current[i] - previous[i]) / dt + if i == 2 { 9.81 } else { 0. })
}
/// Bumper contact on the side of `direction` (body frame) of the body origin.
pub fn directional(
    position: [f64; 3],
    r: [[f64; 3]; 3],
    points: &[[f64; 3]],
    direction: [f64; 3],
) -> f64 {
    let hit = points.iter().any(|p| {
        let l = local(r, std::array::from_fn(|i| p[i] - position[i]));
        (0..3).map(|j| l[j] * direction[j]).sum::<f64>() > 0.
    });
    if hit { 1. } else { 0. }
}
/// Ultrasonic/IR reading from a distance (m) and optional lookup table.
pub fn range_reading(
    distance: f64,
    reach: f64,
    lookup: Option<&[[f64; 2]]>,
) -> Result<f64, String> {
    let distance = distance.min(reach);
    if let Some(table) = lookup {
        let mut raw = table.last().ok_or("empty range lookup")?[1];
        for pair in table.windows(2) {
            if distance <= pair[1][0] {
                let t = norm(distance, pair[0][0], pair[1][0]);
                raw = pair[0][1] + t * (pair[1][1] - pair[0][1]);
                break;
            }
        }
        Ok(norm(raw, 120., 1000.))
    } else {
        Ok(1. - distance / reach)
    }
}
/// Index of the root laser (`proximity_i`, yaw 2*pi*i/count) nearest in yaw.
pub fn nearest_ray(direction: [f64; 3], count: usize) -> usize {
    let yaw = direction[1]
        .atan2(direction[0])
        .rem_euclid(std::f64::consts::TAU);
    let step = std::f64::consts::TAU / count as f64;
    ((yaw / step).round() as usize) % count
}
/// `legacy-flow`: local linear velocity component, rectified by side.
pub fn flow(local_velocity: [f64; 3], axis: usize, mode: Option<&str>) -> f64 {
    let v = local_velocity[axis];
    match mode {
        Some("left") => v.clamp(0., 1.),
        Some("right") => (-v).clamp(0., 1.),
        _ => v.abs().min(1.),
    }
}
pub fn depth(z: f64) -> f64 {
    ((0.4 - z) / 0.4).clamp(0., 1.)
}
pub fn pitch(r: [[f64; 3]; 3]) -> f64 {
    norm(
        (-r[2][0]).clamp(-1., 1.).asin(),
        -std::f64::consts::FRAC_PI_2,
        std::f64::consts::FRAC_PI_2,
    )
}
/// Pixel of a retina cell centre in a `width` x `height` image.
pub fn cell_pixel(cell: [usize; 2], shape: [usize; 2], width: usize, height: usize) -> usize {
    let [y, x] = cell;
    let [rows, cols] = shape;
    let px = (((x as f64 + 0.5) / cols as f64) * width as f64) as usize;
    let py = (((y as f64 + 0.5) / rows as f64) * height as f64) as usize;
    py.min(height - 1) * width + px.min(width - 1)
}
/// Mean luminance of the (up to) four pixels around a cell centre.
pub fn cell_luminance(
    image: &[f64],
    cell: [usize; 2],
    shape: [usize; 2],
    width: usize,
    height: usize,
) -> f64 {
    let [y, x] = cell;
    let [rows, cols] = shape;
    let cx = (x as f64 + 0.5) / cols as f64 * width as f64 - 0.5;
    let cy = (y as f64 + 0.5) / rows as f64 * height as f64 - 0.5;
    let mut sum = 0.;
    let mut n = 0.;
    for py in [cy.floor(), cy.ceil()] {
        for px in [cx.floor(), cx.ceil()] {
            let (px, py) = (
                px.clamp(0., (width - 1) as f64),
                py.clamp(0., (height - 1) as f64),
            );
            sum += image[py as usize * width + px as usize];
            n += 1.;
        }
    }
    (sum / n).clamp(0., 1.)
}

#[cfg(test)]
mod tests {
    use super::*;
    const I: [[f64; 3]; 3] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    #[test]
    fn angle_and_inertial_match_the_device_mapper() {
        assert_eq!(angle(0.), 0.5);
        assert_eq!(angle(-3.14), 0.);
        assert_eq!(angle(3.2), 1.);
        assert!((angle(0.7) - (0.7 + 3.14) / 6.28).abs() < 1e-15);
        // At rest an accelerometer reads +g on the up axis.
        let g = specific_force([0.; 3], [0.; 3], 0.002);
        assert!((inertial(g, 2, None, true) - (9.81 + 20.) / 40.).abs() < 1e-12);
        assert!((inertial(g, 1, Some("legacy-y-up"), true) - (9.81 + 20.) / 40.).abs() < 1e-12);
        assert_eq!(inertial([0., 0., 10.], 2, None, false), 1.);
    }
    #[test]
    fn range_lookup_and_ray_choice_match_the_reference() {
        let table = [[0., 1000.], [0.4, 620.], [0.8, 280.], [1.2, 120.]];
        assert_eq!(range_reading(0., 1.2, Some(&table)).unwrap(), 1.);
        assert!(
            (range_reading(0.2, 1.2, Some(&table)).unwrap() - (810. - 120.) / 880.).abs() < 1e-12
        );
        assert_eq!(range_reading(5., 1.2, Some(&table)).unwrap(), 0.);
        assert!((range_reading(0.3, 1.2, None).unwrap() - 0.75).abs() < 1e-12);
        assert_eq!(nearest_ray([0.98, 0.199, 0.], 8), 0);
        assert_eq!(nearest_ray([0.98, -0.199, 0.], 8), 0);
        assert_eq!(nearest_ray([0., 1., 0.], 8), 2);
        assert_eq!(nearest_ray([-1., -0.01, 0.], 8), 4);
    }
    #[test]
    fn contacts_flow_and_posture() {
        let points = [[0.1, 0., 0.]];
        assert_eq!(directional([0.; 3], I, &points, [1., 0., 0.]), 1.);
        assert_eq!(directional([0.; 3], I, &points, [-1., 0., 0.]), 0.);
        assert_eq!(directional([0.; 3], I, &[], [1., 0., 0.]), 0.);
        assert_eq!(flow([0.4, -0.3, 0.], 1, Some("right")), 0.3);
        assert_eq!(flow([0.4, -0.3, 0.], 1, Some("left")), 0.);
        assert_eq!(depth(0.2), 0.5);
        assert_eq!(pitch(I), 0.5);
        assert_eq!(cell_pixel([0, 0], [1, 1], 12, 8), 4 * 12 + 6);
        let image: Vec<f64> = (0..96)
            .map(|i| if i == 3 * 12 + 5 { 1. } else { 0. })
            .collect();
        assert_eq!(cell_luminance(&image, [0, 0], [1, 1], 12, 8), 0.25);
    }
}
