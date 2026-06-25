use crate::aer::{AerEvent, SynapseId};
use crate::routing::{EndpointId, EndpointType, SynapseEndpoint};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoftwareKernel {
    SynapticFilter,
    ShortTermPlasticity,
    AdaptiveThresholdHomeostasis,
    ActiveDendrite,
    GapJunctionField,
    MorphologyTransmission,
    TripletScalingDaleHybrid,
}

impl SoftwareKernel {
    pub fn default_for_endpoint(endpoint_type: EndpointType) -> Self {
        match endpoint_type {
            EndpointType::DendriteBouton => Self::SynapticFilter,
            EndpointType::MotorOutput => Self::MorphologyTransmission,
            EndpointType::SensoryInput => Self::GapJunctionField,
            EndpointType::AxonBouton => Self::ShortTermPlasticity,
            EndpointType::HostSubscriber => Self::TripletScalingDaleHybrid,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SoftwareKernelResult {
    pub kernel: SoftwareKernel,
    pub output_value: f64,
    pub delay_ns: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct KernelStateKey {
    synapse_id: SynapseId,
    endpoint: EndpointId,
    kernel: SoftwareKernel,
}

#[derive(Debug, Clone)]
enum KernelState {
    SynapticFilter {
        ampa: f64,
        nmda: f64,
        gaba: f64,
    },
    ShortTermPlasticity {
        utilization: f64,
        resources: f64,
    },
    AdaptiveThreshold {
        threshold: f64,
        rate_ema: f64,
    },
    ActiveDendrite {
        calcium: f64,
        plateau: f64,
    },
    GapJunctionField {
        mean_field: f64,
    },
    MorphologyTransmission {
        fatigue: f64,
    },
    TripletHybrid {
        pre_mean: f64,
        post_mean: f64,
        rate_mean: f64,
        scale: f64,
    },
}

#[derive(Default)]
pub struct SoftwareKernelEngine {
    states: Mutex<HashMap<KernelStateKey, KernelState>>,
}

impl SoftwareKernelEngine {
    pub fn execute_endpoint(
        &self,
        endpoint: &SynapseEndpoint,
        event: &AerEvent,
    ) -> SoftwareKernelResult {
        let kernel = endpoint
            .software_kernel
            .unwrap_or_else(|| SoftwareKernel::default_for_endpoint(endpoint.endpoint_type));
        self.execute_kernel(kernel, endpoint, event)
    }

    fn execute_kernel(
        &self,
        kernel: SoftwareKernel,
        endpoint: &SynapseEndpoint,
        event: &AerEvent,
    ) -> SoftwareKernelResult {
        let key = KernelStateKey {
            synapse_id: event.synapse_id,
            endpoint: endpoint.endpoint_id(),
            kernel,
        };
        let mut states = self.states.lock();
        let state = states
            .entry(key)
            .or_insert_with(|| default_state_for(kernel));
        run_kernel_step(kernel, state, endpoint, event)
    }
}

fn default_state_for(kernel: SoftwareKernel) -> KernelState {
    match kernel {
        SoftwareKernel::SynapticFilter => KernelState::SynapticFilter {
            ampa: 0.0,
            nmda: 0.0,
            gaba: 0.0,
        },
        SoftwareKernel::ShortTermPlasticity => KernelState::ShortTermPlasticity {
            utilization: 0.2,
            resources: 1.0,
        },
        SoftwareKernel::AdaptiveThresholdHomeostasis => KernelState::AdaptiveThreshold {
            threshold: 0.0,
            rate_ema: 0.0,
        },
        SoftwareKernel::ActiveDendrite => KernelState::ActiveDendrite {
            calcium: 0.0,
            plateau: 0.0,
        },
        SoftwareKernel::GapJunctionField => KernelState::GapJunctionField { mean_field: 0.0 },
        SoftwareKernel::MorphologyTransmission => {
            KernelState::MorphologyTransmission { fatigue: 1.0 }
        }
        SoftwareKernel::TripletScalingDaleHybrid => KernelState::TripletHybrid {
            pre_mean: 0.0,
            post_mean: 0.0,
            rate_mean: 0.0,
            scale: 1.0,
        },
    }
}

fn run_kernel_step(
    kernel: SoftwareKernel,
    state: &mut KernelState,
    endpoint: &SynapseEndpoint,
    event: &AerEvent,
) -> SoftwareKernelResult {
    match (kernel, state) {
        (SoftwareKernel::SynapticFilter, KernelState::SynapticFilter { ampa, nmda, gaba }) => {
            let signed_drive = event.value as f64 * endpoint_gain(endpoint);
            let exc = signed_drive.max(0.0);
            let inh = (-signed_drive).max(0.0);
            let nmda_ratio = 0.25;
            let decay_ampa = 0.90;
            let decay_nmda = 0.99;
            let decay_gaba = 0.95;
            *ampa = (*ampa * decay_ampa) + exc * (1.0 - nmda_ratio);
            *nmda = (*nmda * decay_nmda) + exc * nmda_ratio;
            *gaba = (*gaba * decay_gaba) + inh;
            let out = (*ampa + *nmda - *gaba) * endpoint_gain(endpoint);
            SoftwareKernelResult {
                kernel,
                output_value: out,
                delay_ns: endpoint_delay(endpoint, event),
            }
        }
        (
            SoftwareKernel::ShortTermPlasticity,
            KernelState::ShortTermPlasticity {
                utilization,
                resources,
            },
        ) => {
            let baseline_u = 0.2;
            let recovery_decay = 0.99;
            let facilitation_decay = 0.90;
            *utilization =
                (*utilization * facilitation_decay) + baseline_u * (1.0 - facilitation_decay);
            *resources = (*resources * recovery_decay) + (1.0 - recovery_decay);
            let spike = event.value != 0;
            let release = if spike {
                let rel = (*utilization * *resources).clamp(0.0, 1.0);
                *resources = (*resources - rel).max(0.0);
                *utilization = (*utilization + baseline_u * (1.0 - *utilization)).clamp(0.0, 1.0);
                rel
            } else {
                0.0
            };
            SoftwareKernelResult {
                kernel,
                output_value: release * event.value as f64 * endpoint_gain(endpoint),
                delay_ns: endpoint_delay(endpoint, event),
            }
        }
        (
            SoftwareKernel::AdaptiveThresholdHomeostasis,
            KernelState::AdaptiveThreshold {
                threshold,
                rate_ema,
            },
        ) => {
            let dt_ms = 1.0f64;
            let thr_decay = (-(dt_ms / 200.0f64)).exp();
            let homeo_decay = (-(dt_ms / 2000.0f64)).exp();
            let base_target = 3.0 * dt_ms / 1000.0;
            *threshold *= thr_decay;
            *rate_ema *= homeo_decay;
            if event.value != 0 {
                *threshold = (*threshold + 0.5).clamp(-2.0, 5.0);
                *rate_ema += 1.0 - homeo_decay;
            }
            let error = *rate_ema - base_target;
            *threshold = (*threshold + 0.25 * error).clamp(-2.0, 5.0);
            let out = (event.value as f64 - *threshold).max(0.0) * endpoint_gain(endpoint);
            SoftwareKernelResult {
                kernel,
                output_value: out,
                delay_ns: endpoint_delay(endpoint, event),
            }
        }
        (SoftwareKernel::ActiveDendrite, KernelState::ActiveDendrite { calcium, plateau }) => {
            let dt_ms = 1.0f64;
            let tau_ca = 120.0;
            let tau_plateau = 350.0;
            let ca_decay = (-dt_ms / tau_ca).exp();
            let plateau_decay = (-dt_ms / tau_plateau).exp();
            let branch_factor = endpoint
                .fpaa_index
                .map(|idx| (1.0 + (idx as f64 * 0.2)).clamp(1.0, 3.0))
                .unwrap_or(1.5);
            let mut curr = event.value as f64 * endpoint_gain(endpoint);
            let exc = curr.max(0.0);
            let drive = 0.75 * exc + 0.25 * branch_factor;
            *calcium = (*calcium * ca_decay + drive).clamp(0.0, 1.0e6);
            let over = (*calcium - 0.1).max(0.0);
            let trigger = over / (1.0 + over);
            *plateau = (*plateau * plateau_decay + trigger * (1.0 - plateau_decay)).clamp(0.0, 1.0);
            let gain = (1.0 + *plateau * branch_factor).clamp(1.0, 3.0);
            if curr >= 0.0 {
                curr *= gain;
            } else {
                curr *= 1.0 + 0.25 * (gain - 1.0);
            }
            SoftwareKernelResult {
                kernel,
                output_value: curr,
                delay_ns: endpoint_delay(endpoint, event),
            }
        }
        (SoftwareKernel::GapJunctionField, KernelState::GapJunctionField { mean_field }) => {
            let value = event.value as f64 * endpoint_gain(endpoint);
            *mean_field = 0.95 * *mean_field + 0.05 * value;
            let gap_current = 0.15 * (*mean_field - value);
            let distance_norm = endpoint
                .fpaa_index
                .map(|idx| idx as f64 / 4.0)
                .unwrap_or(0.25);
            let sigma = 0.35;
            let field = (1.0 + 0.4 * (-(distance_norm.powi(2) / (2.0 * sigma * sigma))).exp())
                .clamp(0.5, 2.5);
            SoftwareKernelResult {
                kernel,
                output_value: (value + gap_current) * field,
                delay_ns: endpoint_delay(endpoint, event),
            }
        }
        (
            SoftwareKernel::MorphologyTransmission,
            KernelState::MorphologyTransmission { fatigue },
        ) => {
            if event.value == 0 {
                *fatigue = (*fatigue + 0.01).min(1.0);
            } else {
                *fatigue = (*fatigue - 0.005).max(0.05);
            }
            let base_steps = 1 + (endpoint_delay(endpoint, event) / 1_000) as usize;
            let axon_length = endpoint.bouton_id.unwrap_or(1) as f64 / 64.0;
            let dendrite_length = endpoint.neuron_id.unwrap_or(1) as f64 / 256.0;
            let attenuation = (-0.15f64 * (axon_length + dendrite_length))
                .exp()
                .clamp(1.0e-2, 1.0);
            let myelin_level = endpoint
                .fpaa_index
                .map(|idx| (idx as f64 / 3.0).clamp(0.0, 1.0))
                .unwrap_or(0.5);
            let conduction_gain = 1.0 + 2.0 * myelin_level;
            let mut steps = ((base_steps as f64) / conduction_gain).round() as usize;
            if *fatigue < 0.5 {
                steps = (steps as f64 * (1.0 + (0.5 - *fatigue))).round() as usize;
            }
            SoftwareKernelResult {
                kernel,
                output_value: event.value as f64 * attenuation * endpoint_gain(endpoint),
                delay_ns: (steps as u64 * 1_000).min(u32::MAX as u64) as u32,
            }
        }
        (
            SoftwareKernel::TripletScalingDaleHybrid,
            KernelState::TripletHybrid {
                pre_mean,
                post_mean,
                rate_mean,
                scale,
            },
        ) => {
            let spike = if event.value == 0 { 0.0 } else { 1.0 };
            *pre_mean = 0.9 * *pre_mean + 0.1 * spike;
            *post_mean = 0.9 * *post_mean + 0.1 * spike;
            *rate_mean = 0.99 * *rate_mean + 0.01 * spike;
            let eta =
                (1.0 + (0.25 * *pre_mean * *post_mean) - (0.15 * *rate_mean)).clamp(0.05, 5.0);
            let dale_sign = if endpoint_gain(endpoint).is_sign_negative() {
                -1.0
            } else {
                1.0
            };
            *scale = (0.99 * *scale + 0.01 * eta).clamp(0.25, 4.0);
            let output = event.value as f64 * endpoint_gain(endpoint).abs() * *scale * dale_sign;
            SoftwareKernelResult {
                kernel,
                output_value: output,
                delay_ns: endpoint_delay(endpoint, event),
            }
        }
        _ => SoftwareKernelResult {
            kernel,
            output_value: event.value as f64,
            delay_ns: endpoint_delay(endpoint, event),
        },
    }
}

fn endpoint_gain(endpoint: &SynapseEndpoint) -> f64 {
    endpoint.weight.unwrap_or(1.0).max(-8.0).min(8.0) as f64
}

fn endpoint_delay(endpoint: &SynapseEndpoint, event: &AerEvent) -> u32 {
    endpoint
        .delay_ns
        .or(endpoint.pulse_width_ns)
        .unwrap_or(event.pulse_width_ns.max(1))
}

#[cfg(test)]
mod tests {
    use super::{SoftwareKernel, SoftwareKernelEngine};
    use crate::aer::{AerEvent, AerFlags, SynapseId};
    use crate::routing::endpoint_table::{EndpointRoute, EndpointType};
    use crate::routing::synapse_table::SynapseEndpoint;

    fn make_endpoint(kernel: Option<SoftwareKernel>) -> SynapseEndpoint {
        SynapseEndpoint {
            endpoint_id: None,
            endpoint_type: EndpointType::DendriteBouton,
            location: None,
            node_slot: 0,
            fpaa_index: Some(0),
            neuron_id: Some(1),
            bouton_id: Some(1),
            io_name: None,
            route: EndpointRoute::LocalSameFpaa,
            weight: Some(1.0),
            delay_ns: None,
            pulse_width_ns: Some(5_000),
            gpio_line: Some("FPAA0_IO5P".to_string()),
            gpio_mask: None,
            software_kernel: kernel,
        }
    }

    fn make_event(value: u32) -> AerEvent {
        AerEvent {
            synapse_id: SynapseId(0x5001_0002_0000_1234),
            flags: AerFlags::empty(),
            value,
            event_time_ns: 1,
            pulse_width_ns: 5_000,
            ttl: 8,
            source_node_slot: 0,
            sequence: 1,
        }
    }

    #[test]
    fn default_kernel_for_dendrite_is_synaptic_filter() {
        let engine = SoftwareKernelEngine::default();
        let endpoint = make_endpoint(None);
        let result = engine.execute_endpoint(&endpoint, &make_event(3));
        assert_eq!(result.kernel, SoftwareKernel::SynapticFilter);
    }

    #[test]
    fn stp_kernel_accumulates_state() {
        let engine = SoftwareKernelEngine::default();
        let endpoint = make_endpoint(Some(SoftwareKernel::ShortTermPlasticity));
        let first = engine.execute_endpoint(&endpoint, &make_event(1));
        let second = engine.execute_endpoint(&endpoint, &make_event(1));
        assert!(first.output_value > 0.0);
        assert!(second.output_value.is_finite());
        let quiet = engine.execute_endpoint(&endpoint, &make_event(0));
        assert_eq!(quiet.output_value, 0.0);
    }
}
