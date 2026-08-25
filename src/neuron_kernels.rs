//! Pure deterministic CPU neuron-transition kernels.
//!
//! These functions contain no runner, transport, or scheduling state.  A
//! caller supplies the complete pre-transition state and receives the next
//! state in one value, which makes serial and parallel CPU execution use the
//! same numerical reference.  Device kernels are deliberately not routed
//! through this module until their equivalence gate is complete.

use crate::config::{IzhikevichParams, LIFParams};

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

#[inline]
fn integrate(
    old_voltage: f64,
    old_recovery: f64,
    current: f64,
    params: IzhikevichParams,
) -> (f64, f64, bool) {
    let (v0, u0, reset0) = sanitize_state(old_voltage, old_recovery, params);
    let current = sanitize_current(current);
    let next_voltage = v0 + params.dt * (0.04 * v0 * v0 + 5.0 * v0 + 140.0 - u0 + current);
    let next_recovery = u0
        + params.dt
            * (params.recovery_time_constant_a
                * (params.recovery_sensitivity_b * next_voltage - u0));
    let (next_voltage, next_recovery, reset1) = sanitize_state(next_voltage, next_recovery, params);
    (next_voltage, next_recovery, reset0 || reset1)
}

/// Apply one deterministic Izhikevich/AARNN transition.
///
/// `adaptive_threshold` selects whether the supplied threshold offset is used
/// and updated.  `old_refractory` is `None` when the model has no additional
/// refractory state.  The returned refractory value is still deterministic
/// (`0` when the state is disabled), which keeps serial and parallel result
/// collection uniform.
#[inline]
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
    let (voltage, recovery, unstable) = integrate(old_voltage, old_recovery, current, params);
    let input_threshold_offset = threshold_offset;
    let threshold_offset = if adaptive_threshold {
        input_threshold_offset.clamp(threshold_min, threshold_max)
    } else {
        0.0
    };
    let blocked_by_refractory = old_refractory.is_some_and(|value| value > 0);
    let fired = !unstable && !blocked_by_refractory && voltage >= (params.v_th + threshold_offset);
    let (next_voltage, next_recovery) = if fired {
        (
            params.membrane_reset_potential_c,
            recovery + params.recovery_increment_d,
        )
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
}
