// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Pure deterministic CPU neuron-transition kernels.
//!
//! These functions contain no runner, transport, or scheduling state.  A
//! caller supplies the complete pre-transition state and receives the next
//! state in one value, which makes serial and parallel CPU execution use the
//! same numerical reference.  Device kernels are deliberately not routed
//! through this module until their equivalence gate is complete.

use crate::config::{AarnnBioParams, IzhikevichParams, LIFParams};

/// Result of one LIF transition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LifTransition {
    pub voltage: f64,
    pub refractory: i32,
    pub fired: bool,
}

/// Apply one deterministic LIF transition from captured pre-state.
#[inline]
pub(crate) fn lif_transition(
    old_voltage: f64,
    old_refractory: i32,
    current: f64,
    decay: f64,
    params: LIFParams,
) -> LifTransition {
    let voltage = (old_voltage * decay + current).clamp(-5.0, 5.0);
    let fired = old_refractory <= 0 && voltage >= params.v_th;
    if fired {
        LifTransition {
            voltage: params.v_reset,
            refractory: params.refractory as i32,
            fired: true,
        }
    } else {
        LifTransition {
            voltage,
            refractory: (old_refractory - 1).max(0),
            fired: false,
        }
    }
}

/// Result of one Izhikevich/AARNN transition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct IzhTransition {
    pub voltage: f64,
    pub recovery: f64,
    pub threshold_offset: f64,
    pub refractory: i32,
    pub fired: bool,
    pub unstable: bool,
}

#[inline]
fn sanitize_current(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

#[inline]
fn sanitize_state(v: f64, u: f64, params: IzhikevichParams) -> (f64, f64, bool) {
    let rest_v = if params.membrane_reset_potential_c.is_finite() {
        params.membrane_reset_potential_c
    } else {
        -65.0
    };
    let rest_u = params.recovery_sensitivity_b * rest_v;
    if !v.is_finite() || !u.is_finite() {
        return (rest_v, rest_u, true);
    }
    let v_min = (rest_v - 120.0).min(-150.0);
    let v_max = (params.v_th + 80.0).max(40.0);
    let u_min = (rest_u - 400.0).min(-600.0);
    let u_max = (rest_u + 400.0).max(600.0);
    (v.clamp(v_min, v_max), u.clamp(u_min, u_max), false)
}

/// Largest Izhikevich integration sub-step (ms).
///
/// Izhikevich (2003) integrates `v` in 0.5 ms steps "for numerical
/// stability"; a single forward-Euler step of the quadratic membrane at
/// 1 ms overshoots the saddle-node and inflates firing rates. Transitions
/// with a larger `dt` are split into equal sub-steps no longer than this.
pub(crate) const IZH_MAX_SUBSTEP_MS: f64 = 0.5;

/// Number of equal Euler sub-steps used for one transition of length `dt`.
#[inline]
pub(crate) fn izh_substeps(dt: f64) -> u32 {
    if !(dt.is_finite() && dt > 0.0) {
        return 1;
    }
    ((dt / IZH_MAX_SUBSTEP_MS).ceil() as u32).clamp(1, 64)
}

/// Stable resting state `(v*, u*)` of an unstimulated Izhikevich neuron.
///
/// Solves `0.04 v^2 + (5 - b) v + 140 = 0` with `u = b v` and returns the
/// lower (stable) root. Presets without a real fixed point (e.g. `P`, which
/// is tonically active) fall back to `(c, b c)`.
pub fn izh_resting_state(params: IzhikevichParams) -> (f64, f64) {
    let b = params.recovery_sensitivity_b;
    let c = if params.membrane_reset_potential_c.is_finite() {
        params.membrane_reset_potential_c
    } else {
        -65.0
    };
    let k = 5.0 - b;
    let disc = k * k - 4.0 * 0.04 * 140.0;
    if !(b.is_finite() && disc.is_finite()) || disc < 0.0 {
        return (c, b * c);
    }
    let v = (-k - disc.sqrt()) / (2.0 * 0.04);
    (v, b * v)
}

/// Integrate one transition of length `params.dt`.
///
/// Returns `(v, u, unstable, crossed)`: `crossed` is set as soon as a
/// sub-step reaches `peak`, and integration stops there so the reset is not
/// applied to an over-integrated state.
#[inline]
fn integrate(
    old_voltage: f64,
    old_recovery: f64,
    current: f64,
    params: IzhikevichParams,
    peak: f64,
) -> (f64, f64, bool, bool) {
    let (mut v, mut u, reset0) = sanitize_state(old_voltage, old_recovery, params);
    let current = sanitize_current(current);
    let n = izh_substeps(params.dt);
    let h = params.dt / f64::from(n);
    let mut unstable = reset0;
    let mut crossed = false;
    for _ in 0..n {
        let next_v = v + h * (0.04 * v * v + 5.0 * v + 140.0 - u + current);
        let next_u = u + h
            * (params.recovery_time_constant_a * (params.recovery_sensitivity_b * next_v - u));
        let (sv, su, reset) = sanitize_state(next_v, next_u, params);
        v = sv;
        u = su;
        unstable |= reset;
        if unstable {
            break;
        }
        if v >= peak {
            crossed = true;
            break;
        }
    }
    (v, u, unstable, crossed)
}

/// Apply one deterministic Izhikevich/AARNN transition.
///
/// `adaptive_threshold` selects whether the supplied threshold offset is used
/// and updated. The offset is an adaptation variable in membrane-current
/// units that is subtracted from the input: raising it raises rheobase and
/// lowers the firing rate. (Shifting only the +30 mV spike-peak test, as
/// before, had no measurable effect because Izhikevich spike initiation is
/// set by the saddle-node near -50 mV.) `old_refractory` is `None` when the
/// model has no additional refractory state. The returned refractory value is
/// still deterministic (`0` when the state is disabled), which keeps serial
/// and parallel result collection uniform.
#[inline]
#[allow(clippy::too_many_arguments)]
pub(crate) fn izh_transition(
    old_voltage: f64,
    old_recovery: f64,
    current: f64,
    params: IzhikevichParams,
    threshold_offset: f64,
    adaptive_threshold: bool,
    threshold_increment: f64,
    threshold_min: f64,
    threshold_max: f64,
    old_refractory: Option<i32>,
    refractory_steps: i32,
) -> IzhTransition {
    let input_threshold_offset = threshold_offset;
    let threshold_offset = if adaptive_threshold {
        input_threshold_offset.clamp(threshold_min, threshold_max)
    } else {
        0.0
    };
    let (voltage, recovery, unstable, crossed) = integrate(
        old_voltage,
        old_recovery,
        current - threshold_offset,
        params,
        params.v_th,
    );
    let blocked_by_refractory = old_refractory.is_some_and(|value| value > 0);
    let fired = !unstable && !blocked_by_refractory && crossed;
    let (next_voltage, next_recovery) = if fired {
        (
            params.membrane_reset_potential_c,
            recovery + params.recovery_increment_d,
        )
    } else if crossed {
        // A refractory-blocked crossing still resets the spike upstroke so
        // the membrane cannot sit above the peak.
        (params.membrane_reset_potential_c, recovery)
    } else {
        (voltage, recovery)
    };
    let next_threshold_offset = if adaptive_threshold && fired {
        (input_threshold_offset + threshold_increment).clamp(threshold_min, threshold_max)
    } else {
        threshold_offset
    };
    let next_refractory = match old_refractory {
        Some(_) if fired => refractory_steps,
        Some(value) => (value - 1).max(0),
        None => 0,
    };
    IzhTransition {
        voltage: next_voltage,
        recovery: next_recovery,
        threshold_offset: next_threshold_offset,
        refractory: next_refractory,
        fired,
        unstable,
    }
}

/// Parameters of a graded (non-spiking) membrane; see `AarnnBioParams`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GradedParams {
    pub tau_ms: f64,
    pub v_rest: f64,
    pub input_resistance: f64,
    pub release_v_half: f64,
    pub release_slope: f64,
    pub release_max_hz: f64,
    pub plateau_mv: f64,
    pub plateau_v_half: f64,
    pub plateau_slope: f64,
    pub e_exc: f64,
    pub e_inh: f64,
}

impl GradedParams {
    /// Sanitised parameters of one graded cell, or `None` when it spikes.
    pub(crate) fn from_bio(bio: &AarnnBioParams) -> Option<Self> {
        if !bio.graded_enabled {
            return None;
        }
        let finite = |v: f64, d: f64| if v.is_finite() { v } else { d };
        let v_rest = finite(bio.graded_v_rest_mv, -70.0);
        Some(Self {
            tau_ms: finite(bio.graded_tau_ms, 30.0).max(0.1),
            v_rest,
            input_resistance: finite(bio.graded_input_resistance, 5.0).max(0.0),
            release_v_half: finite(bio.graded_release_v_half_mv, -50.0),
            release_slope: finite(bio.graded_release_slope_mv, 5.0).max(0.1),
            release_max_hz: finite(bio.graded_release_max_hz, 50.0).max(0.0),
            plateau_mv: finite(bio.graded_plateau_mv, 0.0).max(0.0),
            plateau_v_half: finite(bio.graded_plateau_v_half_mv, -55.0),
            plateau_slope: finite(bio.graded_plateau_slope_mv, 3.0).max(0.1),
            e_exc: finite(bio.graded_e_exc_mv, 0.0).max(v_rest + 1.0),
            e_inh: finite(bio.graded_e_inh_mv, -80.0).min(v_rest - 1.0),
        })
    }

    /// The regenerative current can hold a second stable state when its
    /// steepest slope exceeds the leak (`plateau / (4 slope) > 1`).
    pub(crate) fn is_bistable(&self) -> bool {
        self.plateau_mv > 4.0 * self.plateau_slope
    }
}

#[inline]
fn logistic(x: f64) -> f64 {
    1.0 / (1.0 + (-x.clamp(-60.0, 60.0)).exp())
}

/// Plateau depolarisation (mV) at `v`; zero at rest so rest stays a fixed
/// point whatever the plateau strength.
#[inline]
fn graded_plateau(v: f64, p: GradedParams) -> f64 {
    if p.plateau_mv <= 0.0 {
        return 0.0;
    }
    let base = logistic((p.v_rest - p.plateau_v_half) / p.plateau_slope);
    p.plateau_mv * (logistic((v - p.plateau_v_half) / p.plateau_slope) - base)
}

/// Graded transmitter release in [0, 1]: a sigmoid of voltage rescaled so
/// that release is exactly zero at rest and one at saturation.
#[inline]
pub(crate) fn graded_release(v: f64, p: GradedParams) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    let base = logistic((p.v_rest - p.release_v_half) / p.release_slope);
    let r = (logistic((v - p.release_v_half) / p.release_slope) - base) / (1.0 - base).max(1.0e-12);
    r.clamp(0.0, 1.0)
}

/// Largest graded-membrane sub-step (ms).
pub(crate) const GRADED_MAX_SUBSTEP_MS: f64 = 0.25;

/// Input conductance (relative to the leak) and reversal potential that
/// reproduce `current` at rest: `g (E - V_rest) = R I`.
#[inline]
fn graded_input_conductance(current: f64, p: GradedParams) -> (f64, f64) {
    if current >= 0.0 {
        (p.input_resistance * current / (p.e_exc - p.v_rest), p.e_exc)
    } else {
        (
            p.input_resistance * -current / (p.v_rest - p.e_inh),
            p.e_inh,
        )
    }
}

/// Advance a graded membrane by `dt` ms under constant `current` (mV/ms,
/// the Izhikevich current unit):
///
/// `tau dV/dt = -(V - V_rest) + P(V) + g (E - V)`, with `g (E - V_rest) = R I`
///
/// The input acts as a conductance with reversal `E` (`e_exc` for net
/// depolarising, `e_inh` for net hyperpolarising drive): near rest it is
/// the current `R I`, but the membrane saturates towards `E` instead of
/// growing without bound (a current source reached +85 mV and, through gap
/// junctions, over-drove its spiking partners). Exponential-midpoint
/// sub-steps (<= 0.25 ms): the plateau `P` is taken at the predicted
/// half-step voltage and the remaining linear equation is integrated
/// exactly, so with `P = 0` the result equals the analytic step response for
/// any `dt`.
#[inline]
pub(crate) fn graded_transition(old_voltage: f64, current: f64, dt: f64, p: GradedParams) -> f64 {
    let current = sanitize_current(current);
    let mut v = if old_voltage.is_finite() {
        old_voltage
    } else {
        p.v_rest
    };
    if !(dt.is_finite() && dt > 0.0) {
        return v;
    }
    let n = ((dt / GRADED_MAX_SUBSTEP_MS).ceil() as u32).clamp(1, 4096);
    let h = dt / f64::from(n);
    let (g, e) = graded_input_conductance(current, p);
    let k = 1.0 + g;
    let decay = (-h * k / p.tau_ms).exp();
    let half_decay = (-0.5 * h * k / p.tau_ms).exp();
    let target = |v: f64| (p.v_rest + graded_plateau(v, p) + g * e) / k;
    for _ in 0..n {
        // Exponential midpoint: the plateau is evaluated at the predicted
        // half-step voltage (second order; exact when the plateau is off).
        let v0 = target(v);
        let mid = v0 + (v - v0) * half_decay;
        let v_inf = target(mid);
        v = v_inf + (v - v_inf) * decay;
    }
    v.clamp(p.v_rest - 120.0, p.v_rest + 150.0)
}

#[cfg(test)]
mod tests {
    use super::{izh_transition, lif_transition};
    use crate::config::{IzhikevichParams, LIFParams};

    #[test]
    fn lif_transition_is_independent_of_iteration_order() {
        let params = LIFParams::default();
        let input = [(0.0, 0, 2.0), (0.5, 2, 2.0), (1.5, 0, -0.25)];
        let serial = input.map(|(v, r, i)| lif_transition(v, r, i, 0.95, params));
        let parallel_style = input
            .iter()
            .rev()
            .map(|&(v, r, i)| lif_transition(v, r, i, 0.95, params))
            .collect::<Vec<_>>();
        assert_eq!(serial[0], parallel_style[2]);
        assert_eq!(serial[1], parallel_style[1]);
        assert_eq!(serial[2], parallel_style[0]);
    }

    #[test]
    fn izh_transition_handles_adaptive_threshold_and_refractory() {
        let params = IzhikevichParams::from_preset("RS", 1.0);
        let first = izh_transition(
            -65.0,
            -13.0,
            1_000.0,
            params,
            0.0,
            true,
            2.0,
            0.0,
            10.0,
            Some(0),
            3,
        );
        assert!(first.voltage.is_finite());
        assert!(first.recovery.is_finite());
        assert!(first.threshold_offset.is_finite());
        if first.fired {
            assert_eq!(first.refractory, 3);
            assert_eq!(first.threshold_offset, 2.0);
        }

        let blocked = izh_transition(
            -65.0,
            -13.0,
            1_000.0,
            params,
            2.0,
            true,
            2.0,
            0.0,
            10.0,
            Some(2),
            3,
        );
        assert!(!blocked.fired);
        assert_eq!(blocked.refractory, 1);
    }

    /// Reference: the same Izhikevich equations integrated at 0.01 ms.
    fn reference_rate(current: f64, ms: f64) -> f64 {
        let p = IzhikevichParams::from_preset("RS", 0.01);
        let (mut v, mut u) = super::izh_resting_state(p);
        let mut spikes = 0;
        for _ in 0..(ms / 0.01) as usize {
            v += 0.01 * (0.04 * v * v + 5.0 * v + 140.0 - u + current);
            u += 0.01 * (p.recovery_time_constant_a * (p.recovery_sensitivity_b * v - u));
            if v >= p.v_th {
                v = p.membrane_reset_potential_c;
                u += p.recovery_increment_d;
                spikes += 1;
            }
        }
        spikes as f64 / (ms / 1000.0)
    }

    fn kernel_rate(current: f64, ms: f64, dt: f64, adaptive: bool) -> f64 {
        let p = IzhikevichParams::from_preset("RS", dt);
        let (mut v, mut u) = super::izh_resting_state(p);
        let mut theta = 0.0;
        let mut spikes = 0;
        for _ in 0..(ms / dt) as usize {
            let t = izh_transition(v, u, current, p, theta, adaptive, 0.5, -2.0, 5.0, None, 0);
            v = t.voltage;
            u = t.recovery;
            theta = t.threshold_offset
                * if adaptive {
                    (-dt / 200.0_f64).exp()
                } else {
                    1.0
                };
            spikes += usize::from(t.fired);
        }
        spikes as f64 / (ms / 1000.0)
    }

    #[test]
    fn izh_resting_state_is_the_stable_fixed_point() {
        let p = IzhikevichParams::from_preset("RS", 1.0);
        let (v, u) = super::izh_resting_state(p);
        assert!(
            (v + 70.0).abs() < 1e-9 && (u + 14.0).abs() < 1e-9,
            "{v} {u}"
        );
        let mut s = (v, u);
        for _ in 0..2000 {
            let t = izh_transition(s.0, s.1, 0.0, p, 0.0, false, 0.0, 0.0, 0.0, None, 0);
            assert!(!t.fired);
            s = (t.voltage, t.recovery);
        }
        assert!((s.0 - v).abs() < 1e-6);
    }

    /// Sub-stepping keeps the 1 ms transition close to a 0.01 ms reference;
    /// a single 1 ms Euler step overshoots the quadratic upstroke.
    #[test]
    fn izh_substeps_track_fine_reference_firing_rate() {
        for current in [6.0, 10.0, 20.0] {
            let reference = reference_rate(current, 2000.0);
            let kernel = kernel_rate(current, 2000.0, 1.0, false);
            assert!(reference > 1.0);
            assert!(
                ((kernel - reference) / reference).abs() < 0.2,
                "I={current}: kernel {kernel} Hz vs reference {reference} Hz"
            );
        }
    }

    /// The adaptive threshold must lower a sustained firing rate.
    #[test]
    fn adaptive_threshold_reduces_sustained_rate() {
        let plain = kernel_rate(10.0, 3000.0, 1.0, false);
        let adapted = kernel_rate(10.0, 3000.0, 1.0, true);
        assert!(
            adapted < 0.9 * plain,
            "adaptive {adapted} Hz vs plain {plain} Hz"
        );
    }

    fn graded(plateau: f64) -> super::GradedParams {
        let mut bio = crate::config::AarnnBioParams::default();
        bio.graded_enabled = true;
        bio.graded_plateau_mv = plateau;
        super::GradedParams::from_bio(&bio).unwrap()
    }

    /// Fine reference: classical RK4 at 0.001 ms of the same equation.
    fn graded_reference(
        v0: f64,
        current: &dyn Fn(f64) -> f64,
        ms: f64,
        p: super::GradedParams,
    ) -> Vec<f64> {
        let h = 0.001;
        let f = |v: f64, i: f64| {
            let (g, e) = super::graded_input_conductance(i, p);
            (-(v - p.v_rest) + super::graded_plateau(v, p) + g * (e - v)) / p.tau_ms
        };
        let mut v = v0;
        let mut out = Vec::new();
        let per_ms = (1.0 / h) as usize;
        for step in 0..(ms as usize) {
            let i = current(step as f64);
            for _ in 0..per_ms {
                let k1 = f(v, i);
                let k2 = f(v + 0.5 * h * k1, i);
                let k3 = f(v + 0.5 * h * k2, i);
                let k4 = f(v + h * k3, i);
                v += h / 6.0 * (k1 + 2.0 * k2 + 2.0 * k3 + k4);
            }
            out.push(v);
        }
        out
    }

    #[test]
    fn graded_membrane_rests_silently_and_matches_the_analytic_step_response() {
        let p = graded(0.0);
        assert!(super::GradedParams::from_bio(&crate::config::AarnnBioParams::default()).is_none());
        let mut v = p.v_rest;
        for _ in 0..10_000 {
            v = super::graded_transition(v, 0.0, 1.0, p);
        }
        assert_eq!(v, p.v_rest);
        assert_eq!(super::graded_release(v, p), 0.0);
        // Without a plateau the membrane is linear for constant input:
        // V(t) = V_inf + (V_rest - V_inf) exp(-(1 + g) t / tau) with
        // g = R I / (E - V_rest) and V_inf = (V_rest + g E) / (1 + g); any dt.
        for (current, e) in [(3.0_f64, p.e_exc), (-3.0, p.e_inh)] {
            let g = p.input_resistance * current.abs() / (e - p.v_rest).abs();
            let v_inf = (p.v_rest + g * e) / (1.0 + g);
            for dt in [0.1, 1.0, 2.0] {
                let mut v = p.v_rest;
                for k in 1..=200 {
                    v = super::graded_transition(v, current, dt, p);
                    let t = k as f64 * dt;
                    let exact = v_inf + (p.v_rest - v_inf) * (-(1.0 + g) * t / p.tau_ms).exp();
                    assert!(
                        (v - exact).abs() < 1e-9,
                        "I {current} dt {dt} t {t}: {v} vs {exact}"
                    );
                }
            }
        }
        // Small inputs are the current R I; large ones saturate at the
        // reversal potentials instead of running away.
        let settle =
            |i: f64| (0..2000).fold(p.v_rest, |v, _| super::graded_transition(v, i, 1.0, p));
        let small = settle(0.01) - p.v_rest;
        assert!(
            (small / (p.input_resistance * 0.01) - 1.0).abs() < 1e-3,
            "{small}"
        );
        assert!(settle(1.0e4) < p.e_exc && settle(1.0e4) > p.e_exc - 0.1);
        assert!(settle(-1.0e4) > p.e_inh && settle(-1.0e4) < p.e_inh + 0.1);
        // Release: zero at rest, 0.5 near the half voltage, saturating, monotone.
        let mut last = -1.0;
        for k in 0..400 {
            let v = -100.0 + 0.25 * k as f64;
            let r = super::graded_release(v, p);
            assert!((0.0..=1.0).contains(&r) && r >= last);
            last = r;
        }
        assert!((super::graded_release(p.release_v_half, p) - 0.5).abs() < 0.02);
        assert!(super::graded_release(0.0, p) > 0.99);
        assert_eq!(super::graded_release(f64::NAN, p), 0.0);
        assert_eq!(
            super::graded_transition(f64::NAN, f64::INFINITY, 1.0, p),
            p.v_rest
        );
    }

    /// A plateau above 4 x slope gives hysteresis: a brief depolarising
    /// pulse latches the cell up, a hyperpolarising pulse resets it. Below
    /// that bound the same pulse decays. The 1 ms kernel tracks a 0.001 ms
    /// RK4 reference throughout.
    #[test]
    fn graded_plateau_is_bistable_and_tracks_a_fine_reference() {
        let pulse = |t: f64| {
            if (100.0..150.0).contains(&t) {
                3.0
            } else if (600.0..650.0).contains(&t) {
                -6.0
            } else {
                0.0
            }
        };
        for (plateau, bistable) in [(30.0, true), (10.0, false)] {
            let p = graded(plateau);
            assert_eq!(p.is_bistable(), bistable);
            let reference = graded_reference(p.v_rest, &pulse, 1000.0, p);
            let mut v = p.v_rest;
            let mut worst: f64 = 0.0;
            let mut trace = Vec::new();
            for (step, r) in reference.iter().enumerate() {
                v = super::graded_transition(v, pulse(step as f64), 1.0, p);
                worst = worst.max((v - r).abs());
                trace.push(v);
            }
            assert!(worst < 0.25, "plateau {plateau}: max error {worst} mV");
            let held = trace[550];
            let after_reset = trace[999];
            if bistable {
                assert!(held > p.plateau_v_half, "latched up: {held}");
                assert!(super::graded_release(held, p) > 0.3);
            } else {
                assert!(held - p.v_rest < 1.0, "monostable decays: {held}");
            }
            assert!(after_reset - p.v_rest < 1.0, "reset: {after_reset}");
        }
    }
}
