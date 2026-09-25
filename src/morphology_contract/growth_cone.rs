//! Bounded, renderer-independent growth-cone reference policy (version 1).
//!
//! All lengths are millimetres. Guidance vectors are dimensionless, with norm
//! at most one; their weights are model assumptions, not biological constants.
//! A caller supplies a complete local capsule neighbourhood from an immutable
//! spatial revision. Somas can be represented by zero-length capsules. An
//! incomplete query defers growth rather than treating missing space as free.
//!
//! This policy proposes geometry only. The owner must validate occupancy again
//! at commit, allocate stable identities, and advance a tip/resource counter
//! only after acceptance. It neither forms synapses nor activates routes.

use super::*;
use crate::deterministic::{BrainId, CounterRng, EventId, RngCoordinate};

const MAX_CAPSULES: usize = 4096;
const MAX_OBSTACLES: usize = 1024;
const MAX_HISTORY: usize = 256;
const MAX_CANDIDATES: u16 = 64;
const POLICY_VERSION: u32 = 1;

/// Independent biological growth coordinates; no host or presentation clock.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GrowthClock {
    pub brain: BrainId,
    pub seed: u64,
    /// Starts at one. Retrying a step retains this coordinate.
    pub step: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrowthConePolicy {
    pub version: u32,
    pub kind: AnatomicalKind,
    pub max_step_mm: LengthMm,
    /// At most pi/3, including branching proposals, so a tip cannot reverse.
    pub max_turn_radians: f64,
    pub candidates: u16,
    pub sensing_distance_mm: LengthMm,
    /// Persistence, attraction, repulsion, avoidance, tissue orientation.
    pub weights: [f64; 5],
    pub noise: f64,
    /// Arbitrary resource units per millimetre; must be recorded with the model.
    pub resource_per_mm: f64,
    pub provenance: String,
}

impl GrowthConePolicy {
    pub fn validate(&self) -> Result<(), ConeError> {
        if self.version != POLICY_VERSION
            || !matches!(self.kind, AnatomicalKind::Axon | AnatomicalKind::Dendrite)
            || !self.max_step_mm.0.is_finite()
            || self.max_step_mm.0 <= 0.0
            || !self.max_turn_radians.is_finite()
            || !(0.0..=std::f64::consts::FRAC_PI_3).contains(&self.max_turn_radians)
            || self.candidates == 0
            || self.candidates > MAX_CANDIDATES
            || !self.sensing_distance_mm.0.is_finite()
            || self.sensing_distance_mm.0 <= 0.0
            || self
                .weights
                .iter()
                .any(|w| !w.is_finite() || !(0.0..=100.0).contains(w))
            || !self.noise.is_finite()
            || !(0.0..=1.0).contains(&self.noise)
            || !self.resource_per_mm.is_finite()
            || self.resource_per_mm <= 0.0
            || self.provenance.is_empty()
            || self.provenance.len() > 1024
        {
            return Err(ConeError::InvalidPolicy);
        }
        Ok(())
    }
}

/// Attraction does not imply synaptic eligibility or a connection target.
#[derive(Debug, Clone, Copy, Default)]
pub struct GrowthGuidance {
    pub attraction: [f64; 3],
    pub repulsion: [f64; 3],
    pub tissue: [f64; 3],
}

/// Finite-width occupied geometry. Parent contact is allowed only at a
/// matching terminal, with a forward direction and no increase of radius.
#[derive(Debug, Clone, Copy)]
pub struct GrowthCapsule {
    pub path: AnatomicalId,
    pub owner: NeuronId,
    pub start_mm: Vec3,
    pub end_mm: Vec3,
    pub radius_mm: LengthMm,
}

/// `complete` means all occupancy intersecting the sensing/step neighbourhood
/// is present, including self geometry, somas and remote reservations. Stale
/// halo data must never be declared complete for authoritative admission.
pub struct GrowthNeighbourhood<'a> {
    pub morphology_revision: u64,
    pub environment: &'a GrowthEnvironment,
    pub capsules: &'a [GrowthCapsule],
    pub parent_path: Option<AnatomicalId>,
    pub complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConeStall {
    ResourceExhausted,
    Obstructed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConeDeferral {
    IncompleteNeighbourhood,
    WorkBudget,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConeDecision {
    Extend(ConeExtension),
    Stalled(ConeStall),
    Deferred(ConeDeferral),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConeExtension {
    pub tip: AnatomicalId,
    pub kind: AnatomicalKind,
    pub growth_step: u64,
    pub base_morphology_revision: u64,
    pub base_environment_revision: u64,
    pub start_mm: Vec3,
    pub end_mm: Vec3,
    pub heading: Vec3,
    pub resource_cost: f64,
}

/// Identities come from the owning allocator, never from a random draw.
pub struct GrowthAllocation {
    pub proposal_id: u64,
    pub element: AnatomicalElement,
    pub path_id: AnatomicalId,
    pub parent_path: Option<AnatomicalId>,
    pub conduction_velocity: VelocityMPerS,
    pub activation: LogicalTag,
}

impl ConeExtension {
    /// Convert a candidate into the existing versioned proposal contract.
    /// Resource consumption and daughter-tip admission remain owner decisions.
    pub fn into_proposal(
        self,
        tip: GrowthTip,
        allocation: GrowthAllocation,
    ) -> Result<GrowthProposal, ConeError> {
        if allocation.proposal_id == 0
            || self.tip != tip.id
            || self.start_mm != tip.position_mm
            || self.kind != allocation.element.kind
            || tip.owner != allocation.element.owner
            || allocation.element.lifecycle != LifecycleState::Proposed
            || tip.resource < self.resource_cost
        {
            return Err(ConeError::InvalidInput);
        }
        AnatomicalId::new(
            allocation.element.id.value,
            allocation.element.id.generation,
        )?;
        AnatomicalId::new(allocation.path_id.value, allocation.path_id.generation)?;
        let new_path = PhysicalPath {
            id: allocation.path_id,
            owner: tip.owner,
            kind: self.kind,
            samples: vec![
                PathSample {
                    position_mm: self.start_mm,
                    radius_mm: tip.radius_mm,
                },
                PathSample {
                    position_mm: self.end_mm,
                    radius_mm: tip.radius_mm,
                },
            ],
            conduction_velocity: allocation.conduction_velocity,
        };
        new_path.validate()?;
        Ok(GrowthProposal {
            proposal_id: allocation.proposal_id,
            base_morphology_revision: self.base_morphology_revision,
            base_environment_revision: self.base_environment_revision,
            tip,
            end_mm: self.end_mm,
            new_element: allocation.element,
            new_path,
            parent_path: allocation.parent_path,
            activation: allocation.activation,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ConeError {
    #[error("invalid or unsupported growth-cone policy")]
    InvalidPolicy,
    #[error("invalid growth tip, guidance or neighbourhood")]
    InvalidInput,
    #[error(transparent)]
    Morphology(#[from] MorphologyError),
}

/// Propose one short extension (also usable by a separately budgeted daughter
/// tip). Work is bounded by 64 candidates x 4096 capsules / 1024 obstacles.
/// The input is borrowed and unchanged on every outcome. The policy uses the
/// shared counter RNG, addressed by brain, stable tip/generation and growth
/// step; processing order, retries and rendering cannot consume its stream.
pub fn propose_step(
    tip: &GrowthTip,
    clock: GrowthClock,
    policy: &GrowthConePolicy,
    guidance: GrowthGuidance,
    view: &GrowthNeighbourhood<'_>,
) -> Result<ConeDecision, ConeError> {
    policy.validate()?;
    if !view.complete {
        return Ok(ConeDecision::Deferred(
            ConeDeferral::IncompleteNeighbourhood,
        ));
    }
    if view.capsules.len() > MAX_CAPSULES
        || view.environment.forbidden.len() > MAX_OBSTACLES
        || tip.history.len() > MAX_HISTORY
    {
        return Ok(ConeDecision::Deferred(ConeDeferral::WorkBudget));
    }
    let vectors = [guidance.attraction, guidance.repulsion, guidance.tissue].map(|v| Vec3 {
        x: v[0],
        y: v[1],
        z: v[2],
    });
    if clock.step == 0
        || view.morphology_revision == 0
        || tip.id.value == 0
        || tip.id.generation == 0
        || !tip.position_mm.is_finite()
        || !tip.orientation.is_finite()
        || !tip.orientation.norm().is_finite()
        || tip.orientation.norm() <= 1.0e-12
        || !tip.radius_mm.0.is_finite()
        || tip.radius_mm.0 <= 0.0
        || !tip.resource.is_finite()
        || tip.resource < 0.0
        || tip.cell_type.len() > 256
        || tip.history.iter().any(|p| !p.is_finite())
        || vectors
            .iter()
            .any(|v| !v.is_finite() || v.norm() > 1.0 + 1.0e-12)
    {
        return Err(ConeError::InvalidInput);
    }
    view.environment.validate()?;
    for capsule in view.capsules {
        if capsule.path.value == 0
            || capsule.path.generation == 0
            || !capsule.start_mm.is_finite()
            || !capsule.end_mm.is_finite()
            || !capsule.radius_mm.0.is_finite()
            || capsule.radius_mm.0 < 0.0
            || !capsule
                .end_mm
                .sub(capsule.start_mm)
                .dot(capsule.end_mm.sub(capsule.start_mm))
                .is_finite()
            || !tip
                .position_mm
                .sub(capsule.start_mm)
                .dot(tip.position_mm.sub(capsule.start_mm))
                .is_finite()
            || !(capsule.radius_mm.0 + tip.radius_mm.0 + view.environment.clearance_mm).is_finite()
        {
            return Err(ConeError::InvalidInput);
        }
    }
    let cost = policy.max_step_mm.0 * policy.resource_per_mm;
    if !cost.is_finite() {
        return Err(ConeError::InvalidPolicy);
    }
    if tip.resource < cost {
        return Ok(ConeDecision::Stalled(ConeStall::ResourceExhausted));
    }
    let heading = tip.orientation.normalised();
    let mut avoidance = Vec3::ZERO;
    // Neighbour order must not change floating-point accumulation: selecting
    // the nearest surface with a stable geometric tie-break avoids a sum.
    let mut nearest = f64::INFINITY;
    for capsule in view.capsules {
        if terminal_attachment(tip, view.parent_path, capsule, heading) {
            continue;
        }
        let closest = closest_point(tip.position_mm, capsule.start_mm, capsule.end_mm);
        let separation = tip.position_mm.distance(closest) - capsule.radius_mm.0 - tip.radius_mm.0;
        let direction = tip.position_mm.sub(closest).normalised();
        let key = (direction.x, direction.y, direction.z);
        let previous = (avoidance.x, avoidance.y, avoidance.z);
        if separation < policy.sensing_distance_mm.0
            && (separation < nearest || (separation == nearest && key < previous))
        {
            nearest = separation;
            avoidance = direction;
        }
    }
    let preferred = heading
        .scale(policy.weights[0])
        .add(vectors[0].scale(policy.weights[1]))
        .sub(vectors[1].scale(policy.weights[2]))
        .add(avoidance.scale(policy.weights[3]))
        .add(vectors[2].scale(policy.weights[4]));
    let rng = CounterRng::new(clock.seed, 0x434f_4e45_0000_0001);
    let purpose = (u64::from(tip.id.generation) << 32)
        | match policy.kind {
            AnatomicalKind::Axon => 1,
            _ => 2,
        };
    let event = EventId::new(clock.step).map_err(|_| ConeError::InvalidInput)?;
    let draw = |index| {
        rng.uniform01(RngCoordinate {
            brain: clock.brain,
            entity: tip.id.value,
            event,
            purpose,
            draw: index,
        }) * 2.0
            - 1.0
    };
    let mut best: Option<(f64, Vec3, Vec3)> = None;
    for candidate in 0..policy.candidates {
        let index = u64::from(candidate) * 3;
        let variation = Vec3 {
            x: draw(index),
            y: draw(index + 1),
            z: draw(index + 2),
        };
        let direction = forward_direction(
            preferred.add(variation.scale(policy.noise)),
            heading,
            policy.max_turn_radians,
        );
        let end = tip.position_mm.add(direction.scale(policy.max_step_mm.0));
        if !end.is_finite() || tip.position_mm.distance(end) <= 1.0e-12 {
            continue;
        }
        match view
            .environment
            .validate_step(tip.position_mm, end, tip.radius_mm.0)
        {
            Ok(()) => {}
            Err(MorphologyError::OutsideEnvironment | MorphologyError::ObstacleCollision) => {
                continue;
            }
            Err(error) => return Err(error.into()),
        }
        if view.capsules.iter().any(|capsule| {
            let distance =
                segment_distance(&tip.position_mm, &end, &capsule.start_mm, &capsule.end_mm);
            !terminal_attachment(tip, view.parent_path, capsule, direction)
                && (!distance.is_finite()
                    || distance
                        <= tip.radius_mm.0 + capsule.radius_mm.0 + view.environment.clearance_mm)
        }) {
            continue;
        }
        let score = direction.dot(preferred.normalised());
        // Stable candidate order breaks equal scores; never use queue order.
        if best.as_ref().is_none_or(|(old, _, _)| score > *old) {
            best = Some((score, end, direction));
        }
    }
    Ok(match best {
        Some((_, end_mm, heading)) => ConeDecision::Extend(ConeExtension {
            tip: tip.id,
            kind: policy.kind,
            growth_step: clock.step,
            base_morphology_revision: view.morphology_revision,
            base_environment_revision: view.environment.revision,
            start_mm: tip.position_mm,
            end_mm,
            heading,
            resource_cost: cost,
        }),
        None => ConeDecision::Stalled(ConeStall::Obstructed),
    })
}

fn closest_point(point: Vec3, start: Vec3, end: Vec3) -> Vec3 {
    let delta = end.sub(start);
    let squared = delta.dot(delta);
    if squared <= 1.0e-24 {
        return start;
    }
    start.add(delta.scale((point.sub(start).dot(delta) / squared).clamp(0.0, 1.0)))
}

pub(super) fn terminal_attachment(
    tip: &GrowthTip,
    parent: Option<AnatomicalId>,
    capsule: &GrowthCapsule,
    direction: Vec3,
) -> bool {
    parent == Some(capsule.path)
        && tip.owner == capsule.owner
        && tip.radius_mm.0 <= capsule.radius_mm.0
        && tip.position_mm.distance(capsule.end_mm) <= 1.0e-12
        && capsule.start_mm.distance(capsule.end_mm) > 1.0e-12
        && direction.dot(capsule.end_mm.sub(capsule.start_mm).normalised()) >= 0.5
}

fn forward_direction(preference: Vec3, heading: Vec3, max_turn: f64) -> Vec3 {
    let direction = preference.normalised();
    if direction.dot(heading) >= max_turn.cos() {
        return direction;
    }
    let lateral = direction
        .sub(heading.scale(direction.dot(heading)))
        .normalised();
    if lateral.norm() <= 1.0e-12 {
        return heading;
    }
    heading
        .scale(max_turn.cos())
        .add(lateral.scale(max_turn.sin()))
        .normalised()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }
    fn id(value: u64) -> AnatomicalId {
        AnatomicalId::new(value, 1).unwrap()
    }
    fn environment() -> GrowthEnvironment {
        GrowthEnvironment {
            revision: 1,
            frame: CoordinateFrame::default(),
            volume: AxisAlignedBox {
                min: v(-10.0, -10.0, -10.0),
                max: v(10.0, 10.0, 10.0),
            },
            forbidden: vec![],
            clearance_mm: 0.001,
        }
    }
    fn tip() -> GrowthTip {
        GrowthTip {
            id: id(1),
            owner: NeuronId::new(1).unwrap(),
            position_mm: Vec3::ZERO,
            orientation: v(1.0, 0.0, 0.0),
            radius_mm: LengthMm(0.01),
            cell_type: "uncalibrated fixture".into(),
            resource: 100.0,
            history: vec![],
        }
    }
    fn policy() -> GrowthConePolicy {
        serde_json::from_str(include_str!(
            "../../qa/fixtures/morphology/growth-cone-v1.json"
        ))
        .unwrap()
    }
    fn clock(step: u64) -> GrowthClock {
        GrowthClock {
            brain: BrainId::new(1).unwrap(),
            seed: 42,
            step,
        }
    }
    fn view<'a>(
        env: &'a GrowthEnvironment,
        capsules: &'a [GrowthCapsule],
    ) -> GrowthNeighbourhood<'a> {
        GrowthNeighbourhood {
            morphology_revision: 1,
            environment: env,
            capsules,
            parent_path: None,
            complete: true,
        }
    }
    fn extension(decision: ConeDecision) -> ConeExtension {
        match decision {
            ConeDecision::Extend(value) => value,
            other => panic!("expected extension: {other:?}"),
        }
    }

    #[test]
    fn cone_replay_is_order_independent_forward_limited_and_resource_bounded() {
        let env = environment();
        let capsules = [
            GrowthCapsule {
                path: id(10),
                owner: tip().owner,
                start_mm: v(0.0, 0.5, 0.0),
                end_mm: v(1.0, 0.5, 0.0),
                radius_mm: LengthMm(0.01),
            },
            GrowthCapsule {
                path: id(11),
                owner: tip().owner,
                start_mm: v(0.0, -0.5, 0.0),
                end_mm: v(1.0, -0.5, 0.0),
                radius_mm: LengthMm(0.01),
            },
        ];
        let reversed = [capsules[1], capsules[0]];
        let policy = policy();
        let guidance = GrowthGuidance {
            attraction: [-1.0, 0.0, 0.0],
            tissue: [0.0, 0.0, 1.0],
            ..GrowthGuidance::default()
        };
        let mut tip = tip();
        for step in 1..=20 {
            let decision =
                propose_step(&tip, clock(step), &policy, guidance, &view(&env, &capsules)).unwrap();
            let _unrelated = propose_step(
                &tip,
                clock(step + 100),
                &policy,
                guidance,
                &view(&env, &capsules),
            )
            .unwrap();
            assert_eq!(
                decision,
                propose_step(&tip, clock(step), &policy, guidance, &view(&env, &reversed)).unwrap()
            );
            if let ConeDecision::Extend(change) = decision {
                assert!((change.start_mm.distance(change.end_mm) - 0.25).abs() < 1.0e-12);
                assert!(
                    change.heading.dot(tip.orientation) >= policy.max_turn_radians.cos() - 1.0e-12
                );
                assert_eq!(change.resource_cost, 0.25);
                tip.position_mm = change.end_mm;
                tip.orientation = change.heading;
                tip.resource -= change.resource_cost;
            }
        }
    }

    #[test]
    fn cone_avoids_capsules_in_three_dimensions_but_never_connects_by_proximity() {
        let env = environment();
        let barrier = GrowthCapsule {
            path: id(10),
            owner: tip().owner,
            start_mm: v(0.125, -1.0, 0.0),
            end_mm: v(0.125, 1.0, 0.0),
            radius_mm: LengthMm(0.02),
        };
        let grown = extension(
            propose_step(
                &tip(),
                clock(1),
                &policy(),
                GrowthGuidance::default(),
                &view(&env, &[barrier]),
            )
            .unwrap(),
        );
        assert!(
            segment_distance(
                &grown.start_mm,
                &grown.end_mm,
                &barrier.start_mm,
                &barrier.end_mm
            ) > 0.031
        );
        assert!(grown.end_mm.z.abs() > 0.03);
        let above = GrowthCapsule {
            start_mm: v(0.125, -1.0, 0.1),
            end_mm: v(0.125, 1.0, 0.1),
            ..barrier
        };
        let straight = GrowthConePolicy {
            noise: 0.0,
            weights: [1.0, 0.0, 0.0, 0.0, 0.0],
            ..policy()
        };
        assert_eq!(
            extension(
                propose_step(
                    &tip(),
                    clock(1),
                    &straight,
                    GrowthGuidance::default(),
                    &view(&env, &[above])
                )
                .unwrap()
            )
            .end_mm,
            v(0.25, 0.0, 0.0)
        );
    }

    #[test]
    fn cone_stalls_at_thin_walls_and_defers_incomplete_or_over_budget_work() {
        let mut env = environment();
        env.forbidden.push(AxisAlignedBox {
            min: v(0.10, -10.0, -10.0),
            max: v(0.10000001, 10.0, 10.0),
        });
        assert_eq!(
            propose_step(
                &tip(),
                clock(1),
                &policy(),
                GrowthGuidance::default(),
                &view(&env, &[])
            )
            .unwrap(),
            ConeDecision::Stalled(ConeStall::Obstructed)
        );
        let mut local = view(&env, &[]);
        local.complete = false;
        assert_eq!(
            propose_step(
                &tip(),
                clock(1),
                &policy(),
                GrowthGuidance::default(),
                &local
            )
            .unwrap(),
            ConeDecision::Deferred(ConeDeferral::IncompleteNeighbourhood)
        );
        let mut many = tip();
        many.history = vec![Vec3::ZERO; MAX_HISTORY + 1];
        assert_eq!(
            propose_step(
                &many,
                clock(1),
                &policy(),
                GrowthGuidance::default(),
                &view(&env, &[])
            )
            .unwrap(),
            ConeDecision::Deferred(ConeDeferral::WorkBudget)
        );
        let mut depleted = tip();
        depleted.resource = 0.01;
        assert_eq!(
            propose_step(
                &depleted,
                clock(1),
                &policy(),
                GrowthGuidance::default(),
                &view(&env, &[])
            )
            .unwrap(),
            ConeDecision::Stalled(ConeStall::ResourceExhausted)
        );
        let mut invalid = policy();
        invalid.noise = f64::NAN;
        assert!(
            propose_step(
                &tip(),
                clock(1),
                &invalid,
                GrowthGuidance::default(),
                &view(&env, &[])
            )
            .is_err()
        );
    }

    #[test]
    fn cone_parent_contact_is_local_forward_and_owner_checked() {
        let env = environment();
        let capsule = GrowthCapsule {
            path: id(10),
            owner: tip().owner,
            start_mm: v(-1.0, 0.0, 0.0),
            end_mm: Vec3::ZERO,
            radius_mm: LengthMm(0.01),
        };
        let capsules = [capsule];
        let mut local = view(&env, &capsules);
        let straight = GrowthConePolicy {
            noise: 0.0,
            ..policy()
        };
        assert_eq!(
            propose_step(
                &tip(),
                clock(1),
                &straight,
                GrowthGuidance::default(),
                &local
            )
            .unwrap(),
            ConeDecision::Stalled(ConeStall::Obstructed)
        );
        local.parent_path = Some(id(10));
        extension(
            propose_step(
                &tip(),
                clock(1),
                &straight,
                GrowthGuidance::default(),
                &local,
            )
            .unwrap(),
        );
        let backwards = GrowthTip {
            orientation: v(-1.0, 0.0, 0.0),
            ..tip()
        };
        assert_eq!(
            propose_step(
                &backwards,
                clock(1),
                &straight,
                GrowthGuidance::default(),
                &local
            )
            .unwrap(),
            ConeDecision::Stalled(ConeStall::Obstructed)
        );
        let foreign = GrowthTip {
            owner: NeuronId::new(2).unwrap(),
            ..tip()
        };
        assert_eq!(
            propose_step(
                &foreign,
                clock(1),
                &straight,
                GrowthGuidance::default(),
                &local
            )
            .unwrap(),
            ConeDecision::Stalled(ConeStall::Obstructed)
        );
    }

    #[test]
    fn cone_proposals_commit_as_geometry_and_reject_stale_retries() {
        let env = environment();
        let mut store = MorphologyStore::new(env.clone(), 2).unwrap();
        let mut tip = tip();
        let mut parent = None;
        for step in 1..=8 {
            let capsules: Vec<_> = store
                .state()
                .paths
                .values()
                .flat_map(|path| {
                    route_segments(path).into_iter().map(|s| GrowthCapsule {
                        path: path.id,
                        owner: path.owner,
                        start_mm: s.start,
                        end_mm: s.end,
                        radius_mm: LengthMm(s.radius_mm),
                    })
                })
                .collect();
            let local = GrowthNeighbourhood {
                morphology_revision: store.state().revision,
                parent_path: parent,
                ..view(&env, &capsules)
            };
            let result = extension(
                propose_step(
                    &tip,
                    clock(step),
                    &policy(),
                    GrowthGuidance::default(),
                    &local,
                )
                .unwrap(),
            );
            let new_tip = GrowthTip {
                position_mm: result.end_mm,
                orientation: result.heading,
                resource: tip.resource - result.resource_cost,
                ..tip.clone()
            };
            let proposal = result
                .into_proposal(
                    tip,
                    GrowthAllocation {
                        proposal_id: step,
                        path_id: id(step + 100),
                        parent_path: parent,
                        element: AnatomicalElement {
                            id: id(step + 200),
                            owner: new_tip.owner,
                            kind: AnatomicalKind::Axon,
                            parent: parent.map(|parent| Attachment {
                                parent,
                                position: 1.0,
                            }),
                            lifecycle: LifecycleState::Proposed,
                        },
                        conduction_velocity: VelocityMPerS(1.0),
                        activation: LogicalTag::new(step, 0),
                    },
                )
                .unwrap();
            let mut stale = proposal.clone();
            stale.proposal_id += 1000;
            assert_eq!(
                store
                    .apply_proposals(vec![proposal])
                    .unwrap()
                    .applied_proposals,
                vec![step]
            );
            assert!(
                store
                    .apply_proposals(vec![stale])
                    .unwrap()
                    .applied_proposals
                    .is_empty()
            );
            tip = new_tip;
            parent = Some(id(step + 100));
        }
        assert_eq!(store.state().paths.len(), 8);
        assert_eq!(store.state().topology_epoch, 1);
        assert!(store.state().synapses.is_empty());
        assert!(
            store
                .state()
                .elements
                .values()
                .all(|e| e.lifecycle == LifecycleState::Proposed)
        );
    }
}
