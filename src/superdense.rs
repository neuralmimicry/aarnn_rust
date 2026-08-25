//! Opt-in local superdense execution adapter.
//!
//! This module is the Phase 2 migration seam around the legacy biological
//! kernel.  The executor owns admission and logical tags; `Runner` is invoked
//! only as the transition processor for one admitted event.  Keeping this
//! boundary explicit lets later phases replace the processor with shard-owned
//! biological phases without changing the engine facade.

use crate::causal::{
    CausalEvent, LocalExecutor, SettlementError, SettlementOutcome, TransitionProcessor,
};
use crate::deterministic::{
    CanonicalEventKey, ComponentId, EventId, EventStage, LogicalTag, TopologyGeneration,
};
use crate::field_events::{FieldEvent, FieldEventError, FieldKind, FieldReduction, FieldScope};
use crate::runner::{Runner, StepOut};
use crate::topology_model::{CompiledExecutionPlan, ExecutionPlanError, ExecutionPlanRegistry};
use std::collections::BTreeMap;
use thiserror::Error;

const COMPONENT_CAPACITY: usize = 1024;
const SETTLING_LIMIT: usize = 64;

#[derive(Debug, Error)]
pub enum SuperdenseError {
    #[error("superdense input event id overflow")]
    EventIdOverflow,
    #[error("superdense input tag overflow: {0}")]
    Tag(#[from] crate::deterministic::PrimitiveError),
    #[error(transparent)]
    Settlement(#[from] SettlementError),
    #[error("superdense transition produced no runner output")]
    MissingOutput,
    #[error(transparent)]
    Field(#[from] FieldEventError),
    #[error(transparent)]
    Plan(#[from] ExecutionPlanError),
    #[error("field admission requires an explicit owner in the active execution plan")]
    UnplannedFieldScope,
}

struct RunnerTransition<'a> {
    runner: &'a mut Runner,
    sensory: Option<Vec<i8>>,
    output: Option<StepOut>,
    field_tag: Option<LogicalTag>,
    field_accumulators: BTreeMap<FieldAggregationKey, FieldAccumulator>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FieldAggregationKey {
    scope: FieldScope,
    kind: FieldKind,
    reduction: FieldReduction,
}

#[derive(Debug, Clone, Copy)]
struct FieldAccumulator {
    sum: f64,
    count: u64,
    maximum: f64,
    last_applied: f64,
    has_applied: bool,
}

impl FieldAccumulator {
    fn new(value: f64) -> Self {
        Self {
            sum: value,
            count: 1,
            maximum: value,
            last_applied: 0.0,
            has_applied: false,
        }
    }

    fn add(&mut self, value: f64) {
        self.sum += value;
        self.count = self.count.saturating_add(1);
        self.maximum = self.maximum.max(value);
    }

    fn mean(self) -> f64 {
        self.sum / self.count as f64
    }
}

impl<'a> RunnerTransition<'a> {
    fn prepare_field_application(
        &mut self,
        mut field: FieldEvent,
    ) -> Result<Option<FieldEvent>, SettlementError> {
        if self.field_tag != Some(field.effective_tag) {
            self.field_tag = Some(field.effective_tag);
            self.field_accumulators.clear();
        }

        match field.reduction {
            FieldReduction::Replace | FieldReduction::ExponentialMovingAverage { .. } => {
                Ok(Some(field))
            }
            FieldReduction::Sum if matches!(field.kind, FieldKind::HomeostaticThresholdDelta) => {
                // Threshold updates are deltas, so applying each contribution
                // is exactly the declared sum and does not double-count a
                // previously applied aggregate.
                Ok(Some(field))
            }
            reduction => {
                let key = FieldAggregationKey {
                    scope: field.scope,
                    kind: field.kind,
                    reduction,
                };
                let accumulator = match self.field_accumulators.entry(key) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(FieldAccumulator::new(field.value))
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        entry.get_mut().add(field.value);
                        entry.into_mut()
                    }
                };

                let aggregate = match reduction {
                    FieldReduction::Sum => accumulator.sum,
                    FieldReduction::Mean => accumulator.mean(),
                    FieldReduction::Maximum => accumulator.maximum,
                    FieldReduction::Replace | FieldReduction::ExponentialMovingAverage { .. } => {
                        unreachable!()
                    }
                };
                if !aggregate.is_finite()
                    || aggregate.abs() > crate::field_events::MAX_FIELD_ABS_VALUE
                {
                    return Err(SettlementError::Model(
                        "field reduction exceeded its finite bounded range".to_owned(),
                    ));
                }

                // Maximum is monotone within one canonical field batch. Do
                // not apply a later lower value and accidentally undo it.
                if matches!(reduction, FieldReduction::Maximum)
                    && accumulator.has_applied
                    && aggregate <= accumulator.last_applied
                {
                    return Ok(None);
                }
                let applied = if matches!(field.kind, FieldKind::HomeostaticThresholdDelta) {
                    aggregate - accumulator.last_applied
                } else {
                    aggregate
                };
                accumulator.last_applied = aggregate;
                accumulator.has_applied = true;
                if !applied.is_finite() {
                    return Err(SettlementError::Model(
                        "field reduction produced a non-finite delta".to_owned(),
                    ));
                }
                field.value = applied;
                field.reduction = FieldReduction::Replace;
                Ok(Some(field))
            }
        }
    }
}

impl TransitionProcessor for RunnerTransition<'_> {
    fn apply(&mut self, event: &CausalEvent) -> Result<Vec<CausalEvent>, SettlementError> {
        if event.key.stage == EventStage::FieldUpdate {
            let mut field = FieldEvent::from_payload(&event.payload)
                .map_err(|error| SettlementError::Model(error.to_string()))?;
            // Non-convergence deferral retags the causal envelope. The
            // payload remains the original provenance record, so apply the
            // field at the envelope's actual eligible tag and continue the
            // cadence from that tag.
            field.effective_tag = event.key.tag;
            let next = field
                .next_occurrence(event.key.tag)
                .map_err(|error| SettlementError::Model(error.to_string()))?;
            if let Some(application) = self.prepare_field_application(field)? {
                self.runner
                    .apply_field_event(&application)
                    .map_err(|error| SettlementError::Model(error.to_string()))?;
            }
            return match next {
                Some(next) => next
                    .causal_event()
                    .map(|event| vec![event])
                    .map_err(|error| SettlementError::Model(error.to_string())),
                None => Ok(Vec::new()),
            };
        }
        if event.key.stage != EventStage::SpikeDecision {
            return Err(SettlementError::UnsupportedStage {
                stage: event.key.stage,
            });
        }
        let sensory = if event.payload.is_empty() {
            None
        } else {
            Some(
                event
                    .payload
                    .iter()
                    .map(|value| *value as i8)
                    .collect::<Vec<_>>(),
            )
        };
        // The event payload is the canonical admission record.  The optional
        // value is kept on the processor so a future phase can add explicit
        // decoding/versioning without changing LocalExecutor.
        let sensory = sensory.or_else(|| self.sensory.take());
        self.output = Some(
            self.runner
                .step_at(event.key.tag, sensory.as_deref())
                .map_err(|error| SettlementError::Model(error.to_string()))?,
        );
        self.runner
            .derive_next_global_field_events(event.key.tag)
            .map_err(|error| SettlementError::Model(error.to_string()))?
            .into_iter()
            .map(|field| {
                field
                    .causal_event()
                    .map_err(|error| SettlementError::Model(error.to_string()))
            })
            .collect()
    }
}

/// State owned by the opt-in local executor path.
#[derive(Debug)]
pub struct SuperdenseController {
    executor: LocalExecutor,
    plan_registry: Option<ExecutionPlanRegistry>,
}

impl SuperdenseController {
    pub fn new() -> Self {
        Self {
            executor: LocalExecutor::new(
                ComponentId::new(1).expect("constant component id"),
                COMPONENT_CAPACITY,
            ),
            plan_registry: None,
        }
    }

    /// Construct a controller bound to one component of an immutable
    /// topology/partition plan. This is the opt-in Phase 3 seam; the legacy
    /// controller remains available as the rollback path.
    pub fn new_with_plan(
        plan: CompiledExecutionPlan,
        component: ComponentId,
    ) -> Result<Self, SuperdenseError> {
        if plan.component_owner(component).is_none() {
            return Err(ExecutionPlanError::UnknownComponent(component).into());
        }
        Ok(Self {
            executor: LocalExecutor::new(component, COMPONENT_CAPACITY),
            plan_registry: Some(ExecutionPlanRegistry::new(plan)),
        })
    }

    pub fn reset(&mut self) {
        let component = self.executor.component();
        let plan_registry = self.plan_registry.clone();
        self.executor = LocalExecutor::new(component, COMPONENT_CAPACITY);
        self.plan_registry = plan_registry;
    }

    pub fn active_execution_plan(&self) -> Option<&CompiledExecutionPlan> {
        self.plan_registry
            .as_ref()
            .map(ExecutionPlanRegistry::active)
    }

    pub fn propose_execution_plan(
        &mut self,
        effective_tag: LogicalTag,
        current_tag: LogicalTag,
        plan: CompiledExecutionPlan,
    ) -> Result<(), SuperdenseError> {
        let registry = self
            .plan_registry
            .as_mut()
            .ok_or_else(|| ExecutionPlanError::UnknownComponent(self.executor.component()))?;
        registry.propose(effective_tag, current_tag, plan)?;
        Ok(())
    }

    /// Admit a routed event only after validating both generation fences and
    /// the immutable route endpoint contract. Remote delivery remains a Phase
    /// 4 transport responsibility, so this executor accepts only local target
    /// components.
    pub fn schedule_routed_event(
        &mut self,
        topology_generation: TopologyGeneration,
        partition_generation: crate::deterministic::PartitionGeneration,
        from: ComponentId,
        to: ComponentId,
        route: Option<crate::deterministic::RouteId>,
        event: CausalEvent,
    ) -> Result<(), SuperdenseError> {
        let component = self.executor.component();
        let registry = self
            .plan_registry
            .as_ref()
            .ok_or_else(|| ExecutionPlanError::UnknownComponent(component))?;
        if to != component {
            return Err(ExecutionPlanError::EventNotForComponent {
                local: component,
                target: to,
            }
            .into());
        }
        registry.validate_event(topology_generation, partition_generation, from, to, route)?;
        self.executor.admit(event)?;
        Ok(())
    }

    pub fn pending_len(&self) -> usize {
        self.executor.pending_len()
    }

    /// Admit a field event into the same bounded executor queue as neural
    /// events.  Future events therefore survive a current-tick settlement and
    /// are not held in a hidden scheduler-side buffer.
    pub fn schedule_field_event(&mut self, event: FieldEvent) -> Result<(), SuperdenseError> {
        if self.plan_registry.is_some() {
            return Err(SuperdenseError::UnplannedFieldScope);
        }
        self.executor.admit(event.causal_event()?)?;
        Ok(())
    }

    pub fn committed_count(&self) -> usize {
        self.executor.committed_count()
    }

    pub fn committed_total(&self) -> u64 {
        self.executor.committed_total()
    }

    pub fn step(
        &mut self,
        runner: &mut Runner,
        sensory: Option<&[i8]>,
    ) -> Result<StepOut, SuperdenseError> {
        let tick = u64::try_from(runner.t).map_err(|_| SuperdenseError::EventIdOverflow)?;
        let event_id_raw = tick
            .checked_add(1)
            .ok_or(SuperdenseError::EventIdOverflow)?;
        let tag = LogicalTag::new(tick, 0);
        let event_id = EventId::new(event_id_raw).map_err(|_| SuperdenseError::EventIdOverflow)?;
        let component = self.executor.component();
        if let Some(registry) = self.plan_registry.as_mut() {
            registry.activate_at(tag)?;
            let plan = registry.active();
            plan.validate_event(
                plan.topology_generation(),
                plan.partition_generation(),
                component,
                component,
                None,
            )?;
        }
        let payload = sensory
            .unwrap_or_default()
            .iter()
            .map(|value| *value as u8)
            .collect::<Vec<_>>();
        let event = CausalEvent::new(
            event_id,
            CanonicalEventKey::new(
                tag,
                EventStage::SpikeDecision,
                component.raw(),
                component.raw(),
                event_id_raw,
            ),
            payload,
        );
        self.executor.admit(event)?;
        if matches!(runner.neuron_model, crate::sim::NeuronModel::Aarnn) {
            const FIELD_EVENT_NAMESPACE: u64 = 0x4000_0000_0000_0000;
            let raw = FIELD_EVENT_NAMESPACE
                .checked_add(
                    tick.checked_mul(8)
                        .ok_or(SuperdenseError::EventIdOverflow)?,
                )
                .ok_or(SuperdenseError::EventIdOverflow)?;
            let ambient = FieldEvent::new(
                EventId::new(raw).map_err(|_| SuperdenseError::EventIdOverflow)?,
                tag,
                FieldScope::WholeBrain,
                crate::field_events::FieldCadence::Once,
                FieldReduction::Replace,
                FieldKind::AmbientDrive,
                f64::from(runner.net.aarnn_ambient_energy_level.max(0.0)),
            )?;
            self.executor.admit(ambient.causal_event()?)?;
        }

        let mut processor = RunnerTransition {
            runner,
            sensory: sensory.map(ToOwned::to_owned),
            output: None,
            field_tag: None,
            field_accumulators: BTreeMap::new(),
        };
        let deferred_tag = tag.next_quantum()?;
        let outcome = self
            .executor
            .settle(tag, SETTLING_LIMIT, deferred_tag, &mut processor)?;
        if !matches!(outcome, SettlementOutcome::Converged(_)) {
            return Err(SuperdenseError::Settlement(SettlementError::Model(
                "runner step unexpectedly reached non-convergence".to_owned(),
            )));
        }
        processor.output.ok_or(SuperdenseError::MissingOutput)
    }
}

impl Default for SuperdenseController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LIFParams, NetworkConfig, STDPParams};
    use crate::sim::{Learning, NeuronModel};

    #[test]
    fn legacy_transition_owns_only_spike_decision_stage() {
        let mut runner = Runner::new(
            LIFParams::default(),
            STDPParams::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        let event = CausalEvent::new(
            EventId::new(1).unwrap(),
            CanonicalEventKey::new(LogicalTag::ZERO, EventStage::SynapticTransition, 1, 2, 1),
            Vec::new(),
        );
        let mut transition = RunnerTransition {
            runner: &mut runner,
            sensory: None,
            output: None,
            field_tag: None,
            field_accumulators: BTreeMap::new(),
        };
        assert!(matches!(
            transition.apply(&event),
            Err(SettlementError::UnsupportedStage {
                stage: EventStage::SynapticTransition
            })
        ));
        assert_eq!(runner.t, 0);
    }

    #[test]
    fn controller_assigns_monotonic_tick_tags_without_retaining_unbounded_history() {
        let mut runner = Runner::new(
            LIFParams::default(),
            STDPParams::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        let mut controller = SuperdenseController::new();
        for _ in 0..(COMPONENT_CAPACITY + 8) {
            let _ = controller.step(&mut runner, None).expect("step");
        }
        assert_eq!(controller.pending_len(), 0);
        assert_eq!(controller.committed_count(), COMPONENT_CAPACITY);
        assert_eq!(
            controller.committed_total(),
            (COMPONENT_CAPACITY + 8) as u64
        );
    }

    #[test]
    fn aarnn_global_fields_are_derived_as_next_tick_causal_events() {
        let mut config = NetworkConfig::default();
        config.aarnn_ambient_energy_level = 0.25;
        config.aarnn_resonance_gain = 0.8;
        config.aarnn_neuromod_decay = 1.0;
        config.aarnn_synaptic_energy_randomness = 0.0;
        config.theta_rhythm_enabled = false;
        let mut runner = Runner::new(
            LIFParams::default(),
            STDPParams::default(),
            config,
            NeuronModel::Aarnn,
            Learning::Aarnn,
        );
        let configured_ambient = runner.net.aarnn_ambient_energy_level;
        let mut controller = SuperdenseController::new();

        controller
            .step(&mut runner, None)
            .expect("first AARNN step");
        assert_eq!(runner.ambient_field_drive, configured_ambient);
        assert_eq!(controller.pending_len(), 4);

        controller
            .step(&mut runner, None)
            .expect("second AARNN step");
        assert_eq!(
            runner.neuromod_dopamine,
            runner.net.aarnn_neuromod_baseline_dopamine
        );
        assert_eq!(runner.neuromod_ach, runner.net.aarnn_neuromod_baseline_ach);
        assert_eq!(
            runner.neuromod_serotonin,
            runner.net.aarnn_neuromod_baseline_serotonin
        );
        assert_eq!(controller.pending_len(), 4);
    }

    #[test]
    fn field_update_is_applied_before_the_same_tick_spike_decision() {
        let mut runner = Runner::new(
            LIFParams::default(),
            STDPParams::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        let mut controller = SuperdenseController::new();
        controller
            .schedule_field_event(
                FieldEvent::new(
                    EventId::new(77).unwrap(),
                    LogicalTag::ZERO,
                    crate::field_events::FieldScope::WholeBrain,
                    crate::field_events::FieldCadence::EveryQuantum,
                    crate::field_events::FieldReduction::Replace,
                    crate::field_events::FieldKind::ResonanceLevel,
                    0.75,
                )
                .unwrap(),
            )
            .unwrap();
        controller.step(&mut runner, None).unwrap();
        assert_eq!(runner.resonance_level, 0.75);
        assert_eq!(controller.pending_len(), 1);
    }

    #[test]
    fn field_reductions_are_applied_in_canonical_order_before_spikes() {
        let mut runner = Runner::new(
            LIFParams::default(),
            STDPParams::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        let mut controller = SuperdenseController::new();
        let field = |id, reduction, kind, value| {
            FieldEvent::new(
                EventId::new(id).unwrap(),
                LogicalTag::ZERO,
                FieldScope::WholeBrain,
                crate::field_events::FieldCadence::Every { period_ticks: 4 },
                reduction,
                kind,
                value,
            )
            .unwrap()
        };

        controller
            .schedule_field_event(field(10, FieldReduction::Sum, FieldKind::AmbientDrive, 0.2))
            .unwrap();
        controller
            .schedule_field_event(field(11, FieldReduction::Sum, FieldKind::AmbientDrive, 0.3))
            .unwrap();
        controller
            .schedule_field_event(field(
                12,
                FieldReduction::Mean,
                FieldKind::PerceptualErrorDrive,
                0.2,
            ))
            .unwrap();
        controller
            .schedule_field_event(field(
                13,
                FieldReduction::Mean,
                FieldKind::PerceptualErrorDrive,
                0.4,
            ))
            .unwrap();
        controller
            .schedule_field_event(field(
                14,
                FieldReduction::Maximum,
                FieldKind::ResonanceLevel,
                0.8,
            ))
            .unwrap();
        controller
            .schedule_field_event(field(
                15,
                FieldReduction::Maximum,
                FieldKind::ResonanceLevel,
                0.2,
            ))
            .unwrap();

        controller.step(&mut runner, None).unwrap();
        assert!((runner.ambient_field_drive - 0.5).abs() < f32::EPSILON);
        assert!((runner.perceptual_field_drive - 0.3).abs() < f64::EPSILON);
        assert!((runner.resonance_level - 0.8).abs() < f32::EPSILON);
        assert_eq!(controller.pending_len(), 6);
    }

    #[test]
    fn exponential_moving_average_uses_declared_alpha_and_repeats_at_cadence() {
        let mut runner = Runner::new(
            LIFParams::default(),
            STDPParams::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        let mut controller = SuperdenseController::new();
        controller
            .schedule_field_event(
                FieldEvent::new(
                    EventId::new(21).unwrap(),
                    LogicalTag::ZERO,
                    FieldScope::WholeBrain,
                    crate::field_events::FieldCadence::Every { period_ticks: 2 },
                    FieldReduction::ExponentialMovingAverage {
                        alpha_millionths: 250_000,
                    },
                    FieldKind::Dopamine,
                    3.0,
                )
                .unwrap(),
            )
            .unwrap();

        controller.step(&mut runner, None).unwrap();
        assert!((runner.neuromod_dopamine - 1.5).abs() < f32::EPSILON);
        assert_eq!(controller.pending_len(), 1);
        controller.step(&mut runner, None).unwrap();
        assert!((runner.neuromod_dopamine - 1.5).abs() < f32::EPSILON);
        assert_eq!(controller.pending_len(), 1);
        controller.step(&mut runner, None).unwrap();
        assert!((runner.neuromod_dopamine - 1.875).abs() < f32::EPSILON);
        assert_eq!(controller.pending_len(), 1);
    }
}
