//! Deterministic safety gate for automatic physical placement changes.
//!
//! PlacementPlanner answers “can this plan fit?” This module answers the
//! different question “may automation apply this plan now?”. Keeping those
//! decisions separate prevents a capacity proposal from bypassing residence
//! hysteresis, transfer budgets, or an in-flight limit.
//!
//! Review is pure. A caller performs the checkpoint and cutover through its
//! authoritative adapter, then calls PlacementController::commit only after
//! committed evidence exists. Rejected or failed operations therefore cannot
//! advance residence timers or make a plan authoritative.

use crate::deterministic::{LogicalTag, ShardId, StateDigest};
use crate::placement::{PlacementIntent, PlacementPlan, ResourceObservation, ShardDemand};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const PLACEMENT_CONTROLLER_SCHEMA_VERSION: u32 = 2;

/// Bounds applied to automation-generated movement. Explicit operator
/// commands still pass planner and registry authority checks, but do not need
/// to demonstrate an optimisation benefit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomaticPlacementPolicy {
    /// Minimum biological quanta a shard remains in place before automation
    /// may move it again, unless its current node is no longer eligible.
    pub minimum_residence_quanta: u64,
    /// Required relative improvement in the deterministic balance score,
    /// expressed in milli-units (0..=1000).
    pub minimum_improvement_milli: u16,
    /// Maximum placement operations allowed to be active while a new
    /// automatic operation is admitted.
    pub maximum_concurrent_migrations: u16,
    /// Maximum checkpoint/WAL bytes one automatic operation may transfer.
    pub migration_budget_bytes: u64,
}

impl Default for AutomaticPlacementPolicy {
    fn default() -> Self {
        Self {
            minimum_residence_quanta: 100,
            minimum_improvement_milli: 50,
            maximum_concurrent_migrations: 1,
            migration_budget_bytes: 64 * 1024 * 1024,
        }
    }
}

impl AutomaticPlacementPolicy {
    fn validate(&self) -> Result<(), PlacementControllerError> {
        if self.minimum_improvement_milli > 1_000
            || self.maximum_concurrent_migrations == 0
            || self.migration_budget_bytes == 0
        {
            return Err(PlacementControllerError::InvalidPolicy);
        }
        Ok(())
    }
}

/// Last committed placement boundary for one shard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardResidence {
    pub last_committed_tag: LogicalTag,
}

/// The result of a review. It is evidence for the next adapter step, not an
/// authority grant or a replacement for cutover evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementReview {
    pub schema_version: u32,
    /// Digest of the controller's currently authoritative plan when the
    /// review was created. This prevents a review from being replayed after
    /// another placement has committed.
    pub source_plan_digest: StateDigest,
    /// Digest of the exact candidate plan reviewed by the controller.
    pub proposed_plan_digest: StateDigest,
    pub approved: bool,
    pub requires_migration: bool,
    pub emergency: bool,
    pub moved_shards: Vec<ShardId>,
    pub estimated_transfer_bytes: u64,
    pub improvement_milli: u16,
    pub reason: String,
}

impl PlacementReview {
    fn no_change(source_plan_digest: StateDigest, proposed_plan_digest: StateDigest) -> Self {
        Self {
            schema_version: PLACEMENT_CONTROLLER_SCHEMA_VERSION,
            source_plan_digest,
            proposed_plan_digest,
            approved: true,
            requires_migration: false,
            emergency: false,
            moved_shards: Vec::new(),
            estimated_transfer_bytes: 0,
            improvement_milli: 0,
            reason: "placement is unchanged".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PlacementControllerError {
    #[error("automatic placement policy is invalid")]
    InvalidPolicy,
    #[error("placement plan is invalid: {0}")]
    InvalidPlan(String),
    #[error("placement plan identity or generation does not match the controller")]
    IdentityMismatch,
    #[error("placement effective tag moved backwards")]
    TagRegression,
    #[error("shard set changed without a separate repartition transaction")]
    RepartitionRequired,
    #[error("demand for shard {0} is missing")]
    MissingDemand(ShardId),
    #[error("automatic movement is blocked: {0}")]
    Blocked(String),
    #[error("placement controller state is invalid")]
    InvalidState,
}

/// Stateful, deterministic automatic-placement gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementController {
    pub schema_version: u32,
    pub policy: AutomaticPlacementPolicy,
    pub active_plan: Option<PlacementPlan>,
    pub residence: BTreeMap<ShardId, ShardResidence>,
}

impl PlacementController {
    pub fn new(policy: AutomaticPlacementPolicy) -> Result<Self, PlacementControllerError> {
        policy.validate()?;
        Ok(Self {
            schema_version: PLACEMENT_CONTROLLER_SCHEMA_VERSION,
            policy,
            active_plan: None,
            residence: BTreeMap::new(),
        })
    }

    /// Adopt an already authoritative plan after restoring controller state.
    /// Residence starts at the plan boundary, so a restart cannot bypass the
    /// minimum-residence policy.
    pub fn adopt(&mut self, plan: PlacementPlan) -> Result<(), PlacementControllerError> {
        plan.verify()
            .map_err(|error| PlacementControllerError::InvalidPlan(error.to_string()))?;
        if let Some(current) = &self.active_plan {
            if current.brain_id != plan.brain_id
                || current.topology_generation > plan.topology_generation
                || current.partition_generation > plan.partition_generation
                || plan.effective_tag < current.effective_tag
            {
                return Err(PlacementControllerError::IdentityMismatch);
            }
        }
        let tag = plan.effective_tag;
        let shards = plan
            .placements
            .iter()
            .map(|placement| placement.shard_id)
            .collect::<BTreeSet<_>>();
        self.residence.retain(|shard, _| shards.contains(shard));
        for shard in shards {
            self.residence.entry(shard).or_insert(ShardResidence {
                last_committed_tag: tag,
            });
        }
        self.active_plan = Some(plan);
        Ok(())
    }

    /// Record a plan that has already crossed the authoritative placement
    /// registry boundary. This is intentionally separate from [`Self::adopt`]:
    /// a committed cut must restart residence hysteresis for every shard so a
    /// controller restart or cache refresh cannot immediately propose the
    /// same movement again.
    pub fn record_committed(
        &mut self,
        plan: PlacementPlan,
        committed_tag: LogicalTag,
    ) -> Result<(), PlacementControllerError> {
        plan.verify()
            .map_err(|error| PlacementControllerError::InvalidPlan(error.to_string()))?;
        if committed_tag.microstep != 0 || committed_tag < plan.effective_tag {
            return Err(PlacementControllerError::TagRegression);
        }
        if let Some(current) = &self.active_plan {
            if current.brain_id != plan.brain_id
                || current.topology_generation > plan.topology_generation
                || current.partition_generation > plan.partition_generation
                || plan.effective_tag < current.effective_tag
            {
                return Err(PlacementControllerError::IdentityMismatch);
            }
        }
        self.residence = plan
            .placements
            .iter()
            .map(|placement| {
                (
                    placement.shard_id,
                    ShardResidence {
                        last_committed_tag: committed_tag,
                    },
                )
            })
            .collect();
        self.active_plan = Some(plan);
        Ok(())
    }

    /// Review a verified candidate. Resource observations are supplied here
    /// because health and enrolment are physical state, not plan state.
    pub fn review(
        &self,
        proposed: &PlacementPlan,
        demands: &BTreeMap<ShardId, ShardDemand>,
        resources: &[ResourceObservation],
        now: LogicalTag,
        active_migrations: u16,
    ) -> Result<PlacementReview, PlacementControllerError> {
        self.policy.validate()?;
        if self.schema_version != PLACEMENT_CONTROLLER_SCHEMA_VERSION {
            return Err(PlacementControllerError::InvalidState);
        }
        proposed
            .verify()
            .map_err(|error| PlacementControllerError::InvalidPlan(error.to_string()))?;
        if now.microstep != 0 || proposed.effective_tag.microstep != 0 {
            return Err(PlacementControllerError::InvalidPlan(
                "automatic placement boundaries must be at microstep zero".to_owned(),
            ));
        }
        let Some(current) = &self.active_plan else {
            return Ok(PlacementReview {
                schema_version: PLACEMENT_CONTROLLER_SCHEMA_VERSION,
                source_plan_digest: StateDigest([0; 16]),
                proposed_plan_digest: proposed.digest,
                approved: true,
                requires_migration: false,
                emergency: false,
                moved_shards: Vec::new(),
                estimated_transfer_bytes: 0,
                improvement_milli: 0,
                reason: "initial placement has no prior authority to migrate".to_owned(),
            });
        };
        if current.brain_id != proposed.brain_id
            || current.topology_generation != proposed.topology_generation
            || current.partition_generation != proposed.partition_generation
            || proposed.effective_tag < current.effective_tag
        {
            return Err(PlacementControllerError::IdentityMismatch);
        }

        let current_ids = current
            .placements
            .iter()
            .map(|placement| placement.shard_id)
            .collect::<BTreeSet<_>>();
        let proposed_ids = proposed
            .placements
            .iter()
            .map(|placement| placement.shard_id)
            .collect::<BTreeSet<_>>();
        if current_ids != proposed_ids {
            return Err(PlacementControllerError::RepartitionRequired);
        }
        let current_by_shard = current
            .placements
            .iter()
            .map(|placement| (placement.shard_id, placement))
            .collect::<BTreeMap<_, _>>();
        let proposed_by_shard = proposed
            .placements
            .iter()
            .map(|placement| (placement.shard_id, placement))
            .collect::<BTreeMap<_, _>>();
        let mut moved = Vec::new();
        let mut transfer_bytes = 0u64;
        for shard in &current_ids {
            let before = current_by_shard
                .get(shard)
                .ok_or(PlacementControllerError::InvalidState)?;
            let after = proposed_by_shard
                .get(shard)
                .ok_or(PlacementControllerError::InvalidState)?;
            if before.active_node != after.active_node
                || before.active_device != after.active_device
                || before.active_failure_domain != after.active_failure_domain
                || before.warm_nodes != after.warm_nodes
            {
                moved.push(*shard);
                let demand = demands
                    .get(shard)
                    .ok_or(PlacementControllerError::MissingDemand(*shard))?;
                transfer_bytes = transfer_bytes
                    .checked_add(demand.checkpoint_bytes)
                    .ok_or(PlacementControllerError::InvalidState)?;
            }
        }
        if moved.is_empty() {
            return Ok(PlacementReview::no_change(current.digest, proposed.digest));
        }
        moved.sort_unstable();
        if active_migrations >= self.policy.maximum_concurrent_migrations {
            return Err(PlacementControllerError::Blocked(format!(
                "{} migration operation(s) already active; limit is {}",
                active_migrations, self.policy.maximum_concurrent_migrations
            )));
        }
        if transfer_bytes > self.policy.migration_budget_bytes {
            return Err(PlacementControllerError::Blocked(format!(
                "estimated transfer is {} bytes; budget is {} bytes",
                transfer_bytes, self.policy.migration_budget_bytes
            )));
        }

        let resource_by_node = resources
            .iter()
            .map(|resource| (resource.node_id.as_str(), resource))
            .collect::<BTreeMap<_, _>>();
        let emergency = current.placements.iter().any(|placement| {
            resource_by_node
                .get(placement.active_node.as_str())
                .is_none_or(|resource| {
                    !resource.healthy || !resource.enrolled || !resource.compute_authorised
                })
        });
        let improvement = score_improvement_milli(current, proposed);
        let explicit = !matches!(proposed.decision.intent, PlacementIntent::Automatic);
        if !explicit && !emergency {
            for shard in &moved {
                let last = self
                    .residence
                    .get(shard)
                    .map(|residence| residence.last_committed_tag)
                    .unwrap_or(current.effective_tag);
                if now.tick.saturating_sub(last.tick) < self.policy.minimum_residence_quanta {
                    return Err(PlacementControllerError::Blocked(format!(
                        "shard {} has not met the minimum residence interval",
                        shard
                    )));
                }
            }
            if improvement < self.policy.minimum_improvement_milli {
                return Err(PlacementControllerError::Blocked(format!(
                    "placement improvement is {} milli-units; minimum is {}",
                    improvement, self.policy.minimum_improvement_milli
                )));
            }
        }

        Ok(PlacementReview {
            schema_version: PLACEMENT_CONTROLLER_SCHEMA_VERSION,
            source_plan_digest: current.digest,
            proposed_plan_digest: proposed.digest,
            approved: true,
            requires_migration: true,
            emergency,
            moved_shards: moved,
            estimated_transfer_bytes: transfer_bytes,
            improvement_milli: improvement,
            reason: if emergency {
                "approved as an emergency evacuation of an ineligible active node".to_owned()
            } else if explicit {
                "approved explicit operator placement intent after bounded admission checks"
                    .to_owned()
            } else {
                "approved automatic movement after residence, benefit and budget checks".to_owned()
            },
        })
    }

    /// Publish a plan only after the migration adapter has committed its
    /// checkpoint/cutover evidence.
    pub fn commit(
        &mut self,
        proposed: PlacementPlan,
        review: &PlacementReview,
        committed_tag: LogicalTag,
    ) -> Result<(), PlacementControllerError> {
        if self.schema_version != PLACEMENT_CONTROLLER_SCHEMA_VERSION
            || review.schema_version != PLACEMENT_CONTROLLER_SCHEMA_VERSION
        {
            return Err(PlacementControllerError::InvalidState);
        }
        if !review.approved || !review.requires_migration || review.moved_shards.is_empty() {
            return Err(PlacementControllerError::InvalidState);
        }
        if committed_tag.microstep != 0 || committed_tag < proposed.effective_tag {
            return Err(PlacementControllerError::TagRegression);
        }
        proposed
            .verify()
            .map_err(|error| PlacementControllerError::InvalidPlan(error.to_string()))?;
        let current = self
            .active_plan
            .as_ref()
            .ok_or(PlacementControllerError::InvalidState)?;
        if current.digest != review.source_plan_digest
            || proposed.digest != review.proposed_plan_digest
        {
            return Err(PlacementControllerError::IdentityMismatch);
        }
        let current_ids = current
            .placements
            .iter()
            .map(|placement| placement.shard_id)
            .collect::<BTreeSet<_>>();
        let proposed_ids = proposed
            .placements
            .iter()
            .map(|placement| placement.shard_id)
            .collect::<BTreeSet<_>>();
        if current_ids != proposed_ids {
            return Err(PlacementControllerError::RepartitionRequired);
        }
        let current_by_shard = current
            .placements
            .iter()
            .map(|placement| (placement.shard_id, placement))
            .collect::<BTreeMap<_, _>>();
        let proposed_by_shard = proposed
            .placements
            .iter()
            .map(|placement| (placement.shard_id, placement))
            .collect::<BTreeMap<_, _>>();
        let mut expected_moved = current_ids
            .iter()
            .filter(|shard| {
                let before = current_by_shard
                    .get(shard)
                    .expect("current shard set contains every placement");
                let after = proposed_by_shard
                    .get(shard)
                    .expect("proposed shard set contains every placement");
                before.active_node != after.active_node
                    || before.active_device != after.active_device
                    || before.active_failure_domain != after.active_failure_domain
                    || before.warm_nodes != after.warm_nodes
            })
            .copied()
            .collect::<Vec<_>>();
        expected_moved.sort_unstable();
        let mut reviewed_moved = review.moved_shards.clone();
        reviewed_moved.sort_unstable();
        reviewed_moved.dedup();
        if reviewed_moved != review.moved_shards || reviewed_moved != expected_moved {
            return Err(PlacementControllerError::InvalidState);
        }
        for shard in &review.moved_shards {
            self.residence.insert(
                *shard,
                ShardResidence {
                    last_committed_tag: committed_tag,
                },
            );
        }
        self.active_plan = Some(proposed);
        Ok(())
    }
}

fn balance_score(plan: &PlacementPlan) -> u64 {
    let total = plan
        .reservations
        .iter()
        .map(|reservation| reservation.active_load_units)
        .sum::<u64>();
    let peak = plan
        .reservations
        .iter()
        .map(|reservation| reservation.active_load_units)
        .max()
        .unwrap_or(1);
    total.saturating_mul(1_000) / peak.max(1)
}

fn score_improvement_milli(before: &PlacementPlan, after: &PlacementPlan) -> u16 {
    let before_score = balance_score(before).max(1) as u128;
    let after_score = balance_score(after) as u128;
    if after_score <= before_score {
        return 0;
    }
    after_score
        .saturating_sub(before_score)
        .saturating_mul(1_000)
        .checked_div(before_score)
        .unwrap_or(0)
        .min(1_000) as u16
}
