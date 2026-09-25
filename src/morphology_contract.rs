//! Portable morphology, constrained growth and route timing contracts.
//!
//! This module is deliberately independent of the legacy renderer-facing
//! `morphology` module.  It is the versioned domain boundary for new structural
//! work: dense runner indices are never used as anatomical identity, growth is
//! admitted against a finite environment, and route timing is calculated from
//! stored physical paths.

use crate::deterministic::{LogicalTag, NeuronId, PrimitiveError, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error;

pub mod growth_cone;

pub const MORPHOLOGY_SCHEMA_VERSION: u16 = 2;
pub const DEFAULT_ROUTE_QUANTUM_MS: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AnatomicalId {
    #[serde(with = "identity_value")]
    pub value: u64,
    pub generation: u32,
}

// JSON numbers lose identity above 2^53 in browser clients. Schema 2 writes
// decimal strings; readers also accept the numeric schema-1 representation.
// Binary serializers retain the fixed-width unsigned representation.
mod identity_value {
    use serde::{Deserialize, Deserializer, Serializer, de};

    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(&value.to_string())
        } else {
            serializer.serialize_u64(*value)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        if !deserializer.is_human_readable() {
            return u64::deserialize(deserializer);
        }
        struct IdentityVisitor;
        impl<'de> de::Visitor<'de> for IdentityVisitor {
            type Value = u64;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an unsigned 64-bit integer or its decimal string")
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<u64, E> {
                Ok(value)
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<u64, E> {
                if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(E::custom("invalid decimal identity"));
                }
                value.parse().map_err(E::custom)
            }
        }
        deserializer.deserialize_any(IdentityVisitor)
    }
}

impl AnatomicalId {
    pub const fn new(value: u64, generation: u32) -> Result<Self, MorphologyError> {
        if value == 0 || generation == 0 {
            return Err(MorphologyError::InvalidIdentity);
        }
        Ok(Self { value, generation })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    pub fn distance(self, other: Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        dx.hypot(dy).hypot(dz)
    }

    pub fn add(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
            z: self.z + other.z,
        }
    }

    pub fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }

    pub fn scale(self, factor: f64) -> Self {
        Self {
            x: self.x * factor,
            y: self.y * factor,
            z: self.z * factor,
        }
    }

    pub fn norm(self) -> f64 {
        self.distance(Self::ZERO)
    }

    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(self, other: Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    pub fn normalised(self) -> Self {
        let norm = self.norm();
        if norm > f64::EPSILON {
            self.scale(1.0 / norm)
        } else {
            Self::ZERO
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CoordinateFrame {
    /// Position unit for this frame. Current reference paths use millimetres.
    pub millimetres_per_unit: f64,
    pub origin_mm: Vec3,
    /// Axis orientation is explicit so a display transform cannot silently
    /// become the physical coordinate frame.
    pub axis_sign: [i8; 3],
}

impl Default for CoordinateFrame {
    fn default() -> Self {
        Self {
            millimetres_per_unit: 1.0,
            origin_mm: Vec3::ZERO,
            axis_sign: [1, 1, 1],
        }
    }
}

impl CoordinateFrame {
    pub fn validate(&self) -> Result<(), MorphologyError> {
        if !self.millimetres_per_unit.is_finite() || self.millimetres_per_unit <= 0.0 {
            return Err(MorphologyError::InvalidCoordinateFrame);
        }
        if !self.origin_mm.is_finite() || self.axis_sign.iter().any(|axis| ![-1, 1].contains(axis))
        {
            return Err(MorphologyError::InvalidCoordinateFrame);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct LengthMm(pub f64);

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct VelocityMPerS(pub f64);

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct TimeMs(pub f64);

impl LengthMm {
    pub fn validate(self) -> Result<Self, MorphologyError> {
        if !self.0.is_finite() || self.0 < 0.0 {
            Err(MorphologyError::InvalidLength)
        } else {
            Ok(self)
        }
    }
}

impl VelocityMPerS {
    pub fn validate(self) -> Result<Self, MorphologyError> {
        if !self.0.is_finite() || self.0 <= 0.0 {
            Err(MorphologyError::InvalidVelocity)
        } else {
            Ok(self)
        }
    }
}

impl TimeMs {
    pub fn validate(self) -> Result<Self, MorphologyError> {
        if !self.0.is_finite() || self.0 < 0.0 {
            Err(MorphologyError::InvalidTime)
        } else {
            Ok(self)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnatomicalKind {
    Soma,
    AxonHillock,
    Axon,
    Dendrite,
    Bouton,
    PostsynapticSite,
    Spine,
    Synapse,
}

/// The semantic role supplied by a point-only connectome importer.  This is
/// display metadata; it does not participate in route timing or ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectomeRole {
    Sensory,
    Hidden,
    Output,
    Unassigned,
}

/// A neuron record from an imported connectome which has positions but no
/// physical neurite routes.  `formation_order` is an optional source hint;
/// the reconstruction still orders by position and local density first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointOnlyNeuron {
    pub id: NeuronId,
    pub position_mm: Vec3,
    pub role: ConnectomeRole,
    pub layer: Option<usize>,
    pub cell_type: String,
    pub formation_order: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointOnlyConnection {
    pub id: u64,
    pub pre: NeuronId,
    pub post: NeuronId,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointOnlyConnectome {
    pub neurons: Vec<PointOnlyNeuron>,
    pub connections: Vec<PointOnlyConnection>,
}

impl PointOnlyConnectome {
    pub fn validate(&self) -> Result<(), MorphologyError> {
        if self.neurons.is_empty() {
            return Err(MorphologyError::EmptyConnectome);
        }
        let mut neurons = BTreeSet::new();
        for neuron in &self.neurons {
            if !neurons.insert(neuron.id) || !neuron.position_mm.is_finite() {
                return Err(MorphologyError::InvalidConnectome);
            }
            if neuron.cell_type.len() > 256 {
                return Err(MorphologyError::InvalidConnectome);
            }
        }
        let mut connections = BTreeSet::new();
        for connection in &self.connections {
            if connection.id == 0
                || !connections.insert(connection.id)
                || !neurons.contains(&connection.pre)
                || !neurons.contains(&connection.post)
            {
                return Err(MorphologyError::InvalidConnectome);
            }
        }
        Ok(())
    }
}

/// Controls for deterministic reconstruction of anatomy from point-only data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReconstructionConfig {
    pub seed: u64,
    pub soma_radius_mm: f64,
    pub neurite_radius_mm: f64,
    pub synaptic_gap_mm: f64,
    pub clearance_mm: f64,
    pub density_radius_mm: f64,
    pub branch_offset_mm: f64,
    pub conduction_velocity_m_per_s: f64,
    pub synaptic_delay_ms: f64,
    pub receiving_response_ms: f64,
    pub effective_at: LogicalTag,
    pub route_epoch: u64,
    pub max_route_attempts: usize,
}

impl Default for ReconstructionConfig {
    fn default() -> Self {
        Self {
            seed: 0xA4A1_5EED,
            soma_radius_mm: 0.04,
            neurite_radius_mm: 0.006,
            synaptic_gap_mm: 0.02,
            clearance_mm: 0.002,
            density_radius_mm: 0.25,
            branch_offset_mm: 0.04,
            conduction_velocity_m_per_s: 1.0,
            synaptic_delay_ms: 0.5,
            receiving_response_ms: 0.0,
            effective_at: LogicalTag::ZERO,
            route_epoch: 1,
            max_route_attempts: 12,
        }
    }
}

impl ReconstructionConfig {
    fn validate(&self) -> Result<(), MorphologyError> {
        if self.seed == 0
            || !self.soma_radius_mm.is_finite()
            || self.soma_radius_mm <= 0.0
            || !self.neurite_radius_mm.is_finite()
            || self.neurite_radius_mm <= 0.0
            || !self.synaptic_gap_mm.is_finite()
            || self.synaptic_gap_mm <= 0.0
            || !self.clearance_mm.is_finite()
            || self.clearance_mm < 0.0
            || !self.density_radius_mm.is_finite()
            || self.density_radius_mm <= 0.0
            || !self.branch_offset_mm.is_finite()
            || self.branch_offset_mm < 0.0
            || !self.conduction_velocity_m_per_s.is_finite()
            || self.conduction_velocity_m_per_s <= 0.0
            || !self.synaptic_delay_ms.is_finite()
            || self.synaptic_delay_ms < 0.0
            || !self.receiving_response_ms.is_finite()
            || self.receiving_response_ms < 0.0
            || self.route_epoch == 0
            || self.max_route_attempts == 0
        {
            return Err(MorphologyError::InvalidReconstructionConfig);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleState {
    Proposed,
    Active,
    Maturing,
    Retiring,
    Retired,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub parent: AnatomicalId,
    /// Fraction along the parent path, allowing attachment within a segment.
    pub position: f64,
}

impl Attachment {
    fn validate(&self) -> Result<(), MorphologyError> {
        if !self.position.is_finite() || !(0.0..=1.0).contains(&self.position) {
            return Err(MorphologyError::InvalidAttachment);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnatomicalElement {
    pub id: AnatomicalId,
    pub owner: NeuronId,
    pub kind: AnatomicalKind,
    pub parent: Option<Attachment>,
    pub lifecycle: LifecycleState,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PathSample {
    pub position_mm: Vec3,
    pub radius_mm: LengthMm,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhysicalPath {
    pub id: AnatomicalId,
    pub owner: NeuronId,
    pub kind: AnatomicalKind,
    pub samples: Vec<PathSample>,
    pub conduction_velocity: VelocityMPerS,
}

impl PhysicalPath {
    pub fn validate(&self) -> Result<(), MorphologyError> {
        if !matches!(
            self.kind,
            AnatomicalKind::Axon | AnatomicalKind::Dendrite | AnatomicalKind::AxonHillock
        ) {
            return Err(MorphologyError::InvalidPathKind);
        }
        self.conduction_velocity.validate()?;
        if self.samples.len() < 2 {
            return Err(MorphologyError::PathNeedsTwoSamples);
        }
        for sample in &self.samples {
            if !sample.position_mm.is_finite() {
                return Err(MorphologyError::NonFiniteGeometry);
            }
            sample.radius_mm.validate()?;
        }
        if self
            .samples
            .windows(2)
            .all(|pair| pair[0].position_mm.distance(pair[1].position_mm) <= 1.0e-12)
        {
            return Err(MorphologyError::DegeneratePath);
        }
        Ok(())
    }

    /// Arc length is the piecewise-linear interpolation of stored centreline
    /// samples. The tolerance is applied by callers when comparing lengths;
    /// no mesh tessellation participates in this calculation.
    pub fn arc_length_mm(&self) -> Result<LengthMm, MorphologyError> {
        self.validate()?;
        let length = self
            .samples
            .windows(2)
            .map(|pair| pair[0].position_mm.distance(pair[1].position_mm))
            .sum();
        LengthMm(length).validate()
    }

    pub fn swept_radius_mm(&self) -> Result<f64, MorphologyError> {
        self.validate()?;
        Ok(self
            .samples
            .iter()
            .map(|sample| sample.radius_mm.0)
            .fold(0.0, f64::max))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SynapticRoute {
    pub synapse_id: AnatomicalId,
    pub axon: PhysicalPath,
    pub dendrite: Option<PhysicalPath>,
    pub synaptic_delay: TimeMs,
    pub receiving_response: TimeMs,
    pub effective_at: LogicalTag,
    pub route_epoch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RouteTiming {
    pub axonal_delay: TimeMs,
    pub dendritic_delay: TimeMs,
    pub synaptic_delay: TimeMs,
    pub receiving_response: TimeMs,
    pub total_delay: TimeMs,
}

impl SynapticRoute {
    pub fn timing(&self) -> Result<RouteTiming, MorphologyError> {
        let axon_mm = self.axon.arc_length_mm()?.0;
        let axonal_delay = TimeMs(axon_mm / self.axon.conduction_velocity.0);
        let dendritic_delay = if let Some(dendrite) = &self.dendrite {
            TimeMs(dendrite.arc_length_mm()?.0 / dendrite.conduction_velocity.0)
        } else {
            TimeMs(0.0)
        };
        self.synaptic_delay.validate()?;
        self.receiving_response.validate()?;
        let total = TimeMs(
            axonal_delay.0 + dendritic_delay.0 + self.synaptic_delay.0 + self.receiving_response.0,
        );
        total.validate()?;
        Ok(RouteTiming {
            axonal_delay,
            dendritic_delay,
            synaptic_delay: self.synaptic_delay,
            receiving_response: self.receiving_response,
            total_delay: total,
        })
    }

    pub fn quantised_arrival(
        &self,
        emitted_at: LogicalTag,
        quantum_ms: f64,
    ) -> Result<(LogicalTag, TimeMs), MorphologyError> {
        if !quantum_ms.is_finite() || quantum_ms <= 0.0 {
            return Err(MorphologyError::InvalidQuantum);
        }
        let timing = self.timing()?;
        let ticks = (timing.total_delay.0 / quantum_ms).ceil() as u64;
        let represented = TimeMs(ticks as f64 * quantum_ms);
        let tag = emitted_at
            .advance(ticks)
            .map_err(|_| MorphologyError::LogicalTimeOverflow)?;
        Ok((tag, represented))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SynapseContract {
    pub id: AnatomicalId,
    pub pre_site: AnatomicalId,
    pub post_site: AnatomicalId,
    pub route: SynapticRoute,
    pub effective_at: LogicalTag,
    pub lifecycle: LifecycleState,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AxisAlignedBox {
    pub min: Vec3,
    pub max: Vec3,
}

impl AxisAlignedBox {
    pub fn validate(&self) -> Result<(), MorphologyError> {
        if !self.min.is_finite()
            || !self.max.is_finite()
            || self.min.x > self.max.x
            || self.min.y > self.max.y
            || self.min.z > self.max.z
        {
            return Err(MorphologyError::InvalidEnvironment);
        }
        Ok(())
    }

    fn expanded(self, radius: f64) -> Self {
        let pad = radius;
        Self {
            min: self.min.sub(Vec3 {
                x: pad,
                y: pad,
                z: pad,
            }),
            max: self.max.add(Vec3 {
                x: pad,
                y: pad,
                z: pad,
            }),
        }
    }

    fn contains_point(self, point: Vec3) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
            && point.z >= self.min.z
            && point.z <= self.max.z
    }

    fn contains_segment(self, start: Vec3, end: Vec3) -> bool {
        // Slab intersection, including the complete interpolated segment.
        let delta = end.sub(start);
        let mut near: f64 = 0.0;
        let mut far: f64 = 1.0;
        for (origin, direction, min, max) in [
            (start.x, delta.x, self.min.x, self.max.x),
            (start.y, delta.y, self.min.y, self.max.y),
            (start.z, delta.z, self.min.z, self.max.z),
        ] {
            if direction.abs() < 1.0e-12 {
                if origin < min || origin > max {
                    return false;
                }
                continue;
            }
            let mut t0 = (min - origin) / direction;
            let mut t1 = (max - origin) / direction;
            if t0 > t1 {
                std::mem::swap(&mut t0, &mut t1);
            }
            near = near.max(t0);
            far = far.min(t1);
            if near > far {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrowthEnvironment {
    pub revision: u64,
    pub frame: CoordinateFrame,
    pub volume: AxisAlignedBox,
    pub forbidden: Vec<AxisAlignedBox>,
    pub clearance_mm: f64,
}

impl GrowthEnvironment {
    pub fn validate(&self) -> Result<(), MorphologyError> {
        self.frame.validate()?;
        self.volume.validate()?;
        if self.revision == 0 || !self.clearance_mm.is_finite() || self.clearance_mm < 0.0 {
            return Err(MorphologyError::InvalidEnvironment);
        }
        for obstacle in &self.forbidden {
            obstacle.validate()?;
        }
        Ok(())
    }

    pub fn validate_step(
        &self,
        start: Vec3,
        end: Vec3,
        radius_mm: f64,
    ) -> Result<(), MorphologyError> {
        self.validate()?;
        if !start.is_finite() || !end.is_finite() || !radius_mm.is_finite() || radius_mm < 0.0 {
            return Err(MorphologyError::NonFiniteGeometry);
        }
        // A convex volume contains the complete capsule only if both ends
        // lie in its erosion. Intersection alone admits escaping segments.
        let interior = self.volume.expanded(-(radius_mm + self.clearance_mm));
        if !interior.contains_point(start) || !interior.contains_point(end) {
            return Err(MorphologyError::OutsideEnvironment);
        }
        if self.forbidden.iter().any(|obstacle| {
            obstacle
                .expanded(radius_mm + self.clearance_mm)
                .contains_segment(start, end)
        }) {
            return Err(MorphologyError::ObstacleCollision);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrowthTip {
    pub id: AnatomicalId,
    pub owner: NeuronId,
    pub position_mm: Vec3,
    pub orientation: Vec3,
    pub radius_mm: LengthMm,
    pub cell_type: String,
    pub resource: f64,
    pub history: Vec<Vec3>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrowthProposal {
    pub proposal_id: u64,
    pub base_morphology_revision: u64,
    pub base_environment_revision: u64,
    pub tip: GrowthTip,
    pub end_mm: Vec3,
    pub new_element: AnatomicalElement,
    pub new_path: PhysicalPath,
    pub parent_path: Option<AnatomicalId>,
    pub activation: LogicalTag,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MorphologyError {
    #[error("anatomical identity is invalid")]
    InvalidIdentity,
    #[error("morphology value is not finite")]
    NonFiniteGeometry,
    #[error("length must be finite and non-negative")]
    InvalidLength,
    #[error("velocity must be finite and positive")]
    InvalidVelocity,
    #[error("time must be finite and non-negative")]
    InvalidTime,
    #[error("coordinate frame is invalid")]
    InvalidCoordinateFrame,
    #[error("path kind is not a propagating neurite")]
    InvalidPathKind,
    #[error("a physical path needs at least two ordered samples")]
    PathNeedsTwoSamples,
    #[error("a physical path must contain a non-zero segment")]
    DegeneratePath,
    #[error("attachment position is outside [0, 1]")]
    InvalidAttachment,
    #[error("growth environment is invalid")]
    InvalidEnvironment,
    #[error("growth step leaves the finite environment")]
    OutsideEnvironment,
    #[error("growth step intersects a forbidden volume")]
    ObstacleCollision,
    #[error("growth proposal {0} is already known")]
    DuplicateProposal(u64),
    #[error("growth proposal is based on morphology revision {actual}, current is {expected}")]
    StaleMorphologyRevision { expected: u64, actual: u64 },
    #[error("growth proposal is based on environment revision {actual}, current is {expected}")]
    StaleEnvironmentRevision { expected: u64, actual: u64 },
    #[error("growth proposal conflicts with committed occupancy")]
    OccupancyConflict,
    #[error("growth proposal references an existing identity")]
    IdentityConflict,
    #[error("route quantum must be finite and positive")]
    InvalidQuantum,
    #[error("logical time overflow while scheduling route delivery")]
    LogicalTimeOverflow,
    #[error("display snapshot version is invalid")]
    InvalidDisplayVersion,
    #[error("morphology ownership or endpoint links are invalid")]
    InvalidOwnership,
    #[error("growth proposal geometry does not match its tip and endpoint")]
    ProposalGeometryMismatch,
    #[error("growth geometry must remain proposed until electrical activation commits")]
    ElectricalActivationRequired,
    #[error("the imported point-only connectome is empty or malformed")]
    InvalidConnectome,
    #[error("the imported point-only connectome contains no neurons")]
    EmptyConnectome,
    #[error("point-only reconstruction configuration is invalid")]
    InvalidReconstructionConfig,
    #[error(
        "two soma volumes overlap and cannot be reconstructed without changing source positions"
    )]
    SomaOverlap,
    #[error("no collision-free route candidate was found for connection {0}")]
    RouteUnavailable(u64),
    #[error("schema or primitive validation failed: {0}")]
    Primitive(#[from] PrimitiveError),
}

// JSON object keys cannot contain generation-bearing identity objects. Store
// sorted (identity, value) entries, preserving both fields and canonical order.
// The only previously encodable map was empty; accept that legacy `{}` form.
mod anatomical_id_map {
    use super::AnatomicalId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser::SerializeSeq};
    use std::{collections::BTreeMap, fmt, marker::PhantomData};

    pub fn serialize<S: Serializer, V: Serialize>(
        map: &BTreeMap<AnatomicalId, V>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut entries = serializer.serialize_seq(Some(map.len()))?;
        for entry in map {
            entries.serialize_element(&entry)?;
        }
        entries.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>, V: Deserialize<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<AnatomicalId, V>, D::Error> {
        struct Entries<V>(PhantomData<V>);
        impl<'de, V: Deserialize<'de>> de::Visitor<'de> for Entries<V> {
            type Value = BTreeMap<AnatomicalId, V>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("anatomical identity entries or an empty legacy object")
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut map = BTreeMap::new();
                while let Some((id, value)) = seq.next_element::<(AnatomicalId, V)>()? {
                    if id.value == 0 || id.generation == 0 || map.insert(id, value).is_some() {
                        return Err(de::Error::custom(
                            "invalid or duplicate anatomical identity",
                        ));
                    }
                }
                Ok(map)
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                if map
                    .next_entry::<de::IgnoredAny, de::IgnoredAny>()?
                    .is_some()
                {
                    return Err(de::Error::custom(
                        "anatomical identity map must use entry records",
                    ));
                }
                Ok(BTreeMap::new())
            }
        }
        deserializer.deserialize_any(Entries(PhantomData))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MorphologyState {
    pub schema_version: SchemaVersion,
    pub revision: u64,
    pub environment_revision: u64,
    pub topology_epoch: u64,
    pub coordinate_frame: CoordinateFrame,
    #[serde(with = "anatomical_id_map")]
    pub elements: BTreeMap<AnatomicalId, AnatomicalElement>,
    #[serde(with = "anatomical_id_map")]
    pub paths: BTreeMap<AnatomicalId, PhysicalPath>,
    #[serde(with = "anatomical_id_map")]
    pub synapses: BTreeMap<AnatomicalId, SynapseContract>,
}

impl MorphologyState {
    pub fn empty(environment: &GrowthEnvironment) -> Result<Self, MorphologyError> {
        environment.validate()?;
        Ok(Self {
            schema_version: SchemaVersion::new(MORPHOLOGY_SCHEMA_VERSION)?,
            revision: 1,
            environment_revision: environment.revision,
            topology_epoch: 1,
            coordinate_frame: environment.frame,
            elements: BTreeMap::new(),
            paths: BTreeMap::new(),
            synapses: BTreeMap::new(),
        })
    }

    pub fn validate(&self) -> Result<(), MorphologyError> {
        if !(1..=MORPHOLOGY_SCHEMA_VERSION).contains(&self.schema_version.raw())
            || self.revision == 0
            || self.environment_revision == 0
            || self.topology_epoch == 0
        {
            return Err(MorphologyError::InvalidOwnership);
        }
        self.coordinate_frame.validate()?;
        for (id, element) in &self.elements {
            if id != &element.id {
                return Err(MorphologyError::InvalidOwnership);
            }
            if let Some(parent) = element.parent {
                parent.validate()?;
                if !self.elements.contains_key(&parent.parent)
                    && !self.paths.contains_key(&parent.parent)
                {
                    return Err(MorphologyError::InvalidOwnership);
                }
            }
        }
        for (id, path) in &self.paths {
            if id != &path.id {
                return Err(MorphologyError::InvalidOwnership);
            }
            path.validate()?;
            if !self
                .elements
                .values()
                .any(|element| element.owner == path.owner && element.kind == path.kind)
            {
                return Err(MorphologyError::InvalidOwnership);
            }
        }
        for (id, synapse) in &self.synapses {
            if id != &synapse.id
                || synapse.route.synapse_id != synapse.id
                || synapse.route.route_epoch == 0
                || synapse.route.effective_at != synapse.effective_at
                || !self.elements.contains_key(&synapse.pre_site)
                || !self.elements.contains_key(&synapse.post_site)
            {
                return Err(MorphologyError::InvalidOwnership);
            }
            let pre = self
                .elements
                .get(&synapse.pre_site)
                .ok_or(MorphologyError::InvalidOwnership)?;
            let post = self
                .elements
                .get(&synapse.post_site)
                .ok_or(MorphologyError::InvalidOwnership)?;
            if self.paths.get(&synapse.route.axon.id) != Some(&synapse.route.axon)
                || synapse.route.axon.owner != pre.owner
                || synapse.route.dendrite.as_ref().is_some_and(|dendrite| {
                    self.paths.get(&dendrite.id) != Some(dendrite) || dendrite.owner != post.owner
                })
            {
                return Err(MorphologyError::InvalidOwnership);
            }
            synapse.route.timing()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReconstructionConnectionStatus {
    Reconstructed {
        synapse_id: AnatomicalId,
        axon_path_id: AnatomicalId,
        dendrite_path_id: AnatomicalId,
    },
    Rejected {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReconstructionConnectionResult {
    pub connection_id: u64,
    pub pre: NeuronId,
    pub post: NeuronId,
    pub status: ReconstructionConnectionStatus,
}

/// The result of importing a point-only connectome.  Failed routes remain in
/// this report and the source connection list is retained, so a failed
/// geometric reconstruction can never silently remove a recurrent edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointOnlyReconstruction {
    pub schema_version: SchemaVersion,
    pub source: String,
    pub seed: u64,
    pub connectome: PointOnlyConnectome,
    pub environment: GrowthEnvironment,
    pub state: MorphologyState,
    pub neuron_order: Vec<NeuronId>,
    pub soma_ids: BTreeMap<NeuronId, AnatomicalId>,
    pub connections: Vec<ReconstructionConnectionResult>,
}

impl PointOnlyReconstruction {
    pub const SCHEMA_VERSION: u16 = 2;

    pub fn display_snapshot(
        &self,
        sequence: u64,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<DisplaySnapshot, MorphologyError> {
        let mut nodes = Vec::with_capacity(self.connectome.neurons.len());
        for neuron in &self.connectome.neurons {
            let id = *self
                .soma_ids
                .get(&neuron.id)
                .ok_or(MorphologyError::InvalidOwnership)?;
            nodes.push(DisplayNode {
                id,
                role: match neuron.role {
                    ConnectomeRole::Sensory => DisplayRole::Sensory,
                    ConnectomeRole::Hidden => DisplayRole::Hidden,
                    ConnectomeRole::Output => DisplayRole::Output,
                    ConnectomeRole::Unassigned => DisplayRole::Unassigned,
                },
                layer: neuron.layer,
                position_mm: neuron.position_mm,
                kind: AnatomicalKind::Soma,
                colour_slot: 0,
            });
        }
        let owner_for_site =
            |id: AnatomicalId| self.state.elements.get(&id).map(|element| element.owner);
        let mut edges = Vec::new();
        for result in &self.connections {
            let ReconstructionConnectionStatus::Reconstructed { synapse_id, .. } = result.status
            else {
                continue;
            };
            let Some(synapse) = self.state.synapses.get(&synapse_id) else {
                continue;
            };
            let Some(source) = self.soma_ids.get(&result.pre).copied() else {
                continue;
            };
            let Some(target) = self.soma_ids.get(&result.post).copied() else {
                continue;
            };
            let mut points = synapse
                .route
                .axon
                .samples
                .iter()
                .map(|s| s.position_mm)
                .collect::<Vec<_>>();
            if let Some(dendrite) = &synapse.route.dendrite {
                points.extend(dendrite.samples.iter().rev().map(|s| s.position_mm));
            }
            if owner_for_site(synapse.pre_site) == Some(result.pre)
                && owner_for_site(synapse.post_site) == Some(result.post)
            {
                edges.push(DisplayEdge {
                    source,
                    target,
                    points_mm: points,
                    multiplicity: 1,
                    kind: "procedural_route".to_owned(),
                });
            }
        }
        // Preserve each committed physical neurite as first-class display
        // geometry.  The combined route edge above is useful for tracing a
        // synapse, but it cannot show which part is axonal and which part is
        // dendritic and it omits branches that do not terminate a synapse.
        // These paths are derived from the same immutable route state used by
        // timing, so the renderer cannot accidentally invent anatomy.
        let mut paths = self
            .state
            .paths
            .values()
            .filter_map(|path| {
                let owner = self
                    .soma_ids
                    .iter()
                    .find_map(|(neuron, soma)| (*neuron == path.owner).then_some(*soma))?;
                let points_mm = path
                    .samples
                    .iter()
                    .map(|sample| sample.position_mm)
                    .collect::<Vec<_>>();
                (points_mm.len() >= 2).then_some(DisplayPath {
                    id: path.id,
                    owner,
                    kind: path.kind,
                    points_mm,
                    radius_mm: path.swept_radius_mm().ok()?,
                })
            })
            .collect::<Vec<_>>();
        paths.sort_by_key(|path| path.id);

        let mut markers = Vec::new();
        for result in &self.connections {
            let ReconstructionConnectionStatus::Reconstructed { synapse_id, .. } = result.status
            else {
                continue;
            };
            let Some(synapse) = self.state.synapses.get(&synapse_id) else {
                continue;
            };
            let Some(source) = self.soma_ids.get(&result.pre).copied() else {
                continue;
            };
            let Some(target) = self.soma_ids.get(&result.post).copied() else {
                continue;
            };
            let Some(pre_position) = synapse
                .route
                .axon
                .samples
                .last()
                .map(|sample| sample.position_mm)
            else {
                continue;
            };
            let Some(post_position) = synapse
                .route
                .dendrite
                .as_ref()
                .and_then(|path| path.samples.last())
                .map(|sample| sample.position_mm)
            else {
                continue;
            };
            let marker_id = |kind: u64| {
                AnatomicalId::new(
                    (synapse_id.value.saturating_mul(4)).saturating_add(kind),
                    synapse_id.generation,
                )
            };
            if let Ok(id) = marker_id(1) {
                markers.push(DisplayMarker {
                    id,
                    owner: source,
                    kind: AnatomicalKind::Bouton,
                    position_mm: pre_position,
                    synapse_id: Some(synapse_id),
                });
            }
            if let Ok(id) = marker_id(2) {
                markers.push(DisplayMarker {
                    id,
                    owner: target,
                    kind: AnatomicalKind::PostsynapticSite,
                    position_mm: post_position,
                    synapse_id: Some(synapse_id),
                });
            }
            if let Ok(id) = marker_id(3) {
                markers.push(DisplayMarker {
                    id,
                    owner: source,
                    kind: AnatomicalKind::Synapse,
                    position_mm: pre_position.add(post_position).scale(0.5),
                    synapse_id: Some(synapse_id),
                });
            }
        }

        DisplaySnapshot::bounded_with_paths_and_markers(
            self.state.revision,
            self.state.topology_epoch,
            self.state
                .synapses
                .values()
                .map(|synapse| synapse.route.route_epoch)
                .max()
                .unwrap_or(1),
            sequence,
            DisplayMode::Anatomical,
            DisplayProvenance::ProceduralAnatomy,
            Some(self.environment.volume),
            nodes,
            edges,
            paths,
            markers,
            max_nodes,
            max_edges,
            None,
        )
    }

    /// Produce the synthetic column view from the same stable anatomical
    /// identities and source connectome. This keeps selection and edge
    /// multiplicity stable when a client switches between display modes.
    pub fn synthetic_display_snapshot(
        &self,
        sequence: u64,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<DisplaySnapshot, MorphologyError> {
        let hidden_layers = self
            .connectome
            .neurons
            .iter()
            .filter_map(|neuron| neuron.layer)
            .max()
            .map(|layer| layer + 1)
            .unwrap_or(0);
        let layer_count = hidden_layers + 2;
        let mut counts = vec![0usize; layer_count];
        for neuron in &self.connectome.neurons {
            let layer = match neuron.role {
                ConnectomeRole::Sensory => 0,
                ConnectomeRole::Output => layer_count.saturating_sub(1),
                ConnectomeRole::Hidden => neuron.layer.unwrap_or(0).saturating_add(1),
                ConnectomeRole::Unassigned => neuron
                    .layer
                    .unwrap_or(0)
                    .saturating_add(1)
                    .min(layer_count.saturating_sub(1)),
            };
            if let Some(count) = counts.get_mut(layer) {
                *count += 1;
            }
        }
        let mut seen_by_layer = vec![0usize; layer_count];
        let mut nodes = Vec::with_capacity(self.connectome.neurons.len());
        for neuron in &self.connectome.neurons {
            let id = *self
                .soma_ids
                .get(&neuron.id)
                .ok_or(MorphologyError::InvalidOwnership)?;
            let layer = match neuron.role {
                ConnectomeRole::Sensory => 0,
                ConnectomeRole::Output => layer_count.saturating_sub(1),
                ConnectomeRole::Hidden => neuron.layer.unwrap_or(0).saturating_add(1),
                ConnectomeRole::Unassigned => neuron
                    .layer
                    .unwrap_or(0)
                    .saturating_add(1)
                    .min(layer_count.saturating_sub(1)),
            };
            let index = seen_by_layer[layer];
            seen_by_layer[layer] += 1;
            let x = if layer_count <= 1 {
                0.0
            } else {
                -1.0 + 2.0 * layer as f64 / (layer_count - 1) as f64
            };
            let y = if counts[layer] <= 1 {
                0.0
            } else {
                -1.0 + 2.0 * index as f64 / (counts[layer] - 1) as f64
            };
            nodes.push(DisplayNode {
                id,
                role: match neuron.role {
                    ConnectomeRole::Sensory => DisplayRole::Sensory,
                    ConnectomeRole::Hidden => DisplayRole::Hidden,
                    ConnectomeRole::Output => DisplayRole::Output,
                    ConnectomeRole::Unassigned => DisplayRole::Unassigned,
                },
                layer: matches!(neuron.role, ConnectomeRole::Hidden)
                    .then_some(layer.saturating_sub(1)),
                position_mm: Vec3 { x, y, z: 0.0 },
                kind: AnatomicalKind::Soma,
                colour_slot: 0,
            });
        }
        let mut edges = Vec::with_capacity(self.connectome.connections.len());
        for connection in &self.connectome.connections {
            let source = *self
                .soma_ids
                .get(&connection.pre)
                .ok_or(MorphologyError::InvalidOwnership)?;
            let target = *self
                .soma_ids
                .get(&connection.post)
                .ok_or(MorphologyError::InvalidOwnership)?;
            edges.push(DisplayEdge {
                source,
                target,
                points_mm: Vec::new(),
                multiplicity: 1,
                kind: "connectome".to_owned(),
            });
        }
        DisplaySnapshot::bounded(
            self.state.revision,
            self.state.topology_epoch,
            self.state.topology_epoch.max(1),
            sequence,
            DisplayMode::SyntheticColumns,
            DisplayProvenance::SyntheticTopology,
            None,
            nodes,
            edges,
            max_nodes,
            max_edges,
            None,
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct OccupiedSegment {
    start: Vec3,
    end: Vec3,
    radius_mm: f64,
}

/// Reconstruct physical routes for a point-only connectome.
///
/// Each connection is proposed as a paired axonal and dendritic branch. Both
/// branches are generated before either is reserved, then admitted as one
/// deterministic transaction. The input order and worker scheduling therefore
/// cannot change the result: neuron formation order is canonical and each
/// connection batch is sorted by stable connection ID. A rejected connection
/// is reported explicitly and its logical edge remains in `connections`.
pub fn reconstruct_point_only_connectome(
    connectome: PointOnlyConnectome,
    environment: GrowthEnvironment,
    config: ReconstructionConfig,
) -> Result<PointOnlyReconstruction, MorphologyError> {
    connectome.validate()?;
    environment.validate()?;
    config.validate()?;

    let mut neurons = connectome.neurons.clone();
    let centre = neurons
        .iter()
        .fold(Vec3::ZERO, |sum, neuron| sum.add(neuron.position_mm))
        .scale(1.0 / neurons.len() as f64);
    let mut density = BTreeMap::new();
    for neuron in &neurons {
        let count = neurons
            .iter()
            .filter(|other| {
                other.id != neuron.id
                    && other.position_mm.distance(neuron.position_mm) <= config.density_radius_mm
            })
            .count();
        density.insert(neuron.id, count);
    }
    neurons.sort_by(|left, right| {
        let left_radius = left.position_mm.distance(centre);
        let right_radius = right.position_mm.distance(centre);
        left_radius
            .total_cmp(&right_radius)
            .then_with(|| density[&right.id].cmp(&density[&left.id]))
            .then_with(|| left.formation_order.cmp(&right.formation_order))
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut state = MorphologyState::empty(&environment)?;
    let mut next_id = 1u64;
    let mut soma_ids = BTreeMap::new();
    for neuron in &neurons {
        if neurons.iter().any(|other| {
            other.id != neuron.id
                && other.position_mm.distance(neuron.position_mm)
                    < 2.0 * config.soma_radius_mm + config.clearance_mm
        }) {
            return Err(MorphologyError::SomaOverlap);
        }
        environment.validate_step(
            neuron.position_mm,
            neuron.position_mm,
            config.soma_radius_mm,
        )?;
        let id = next_anatomical_id(&mut next_id)?;
        soma_ids.insert(neuron.id, id);
        state.elements.insert(
            id,
            AnatomicalElement {
                id,
                owner: neuron.id,
                kind: AnatomicalKind::Soma,
                parent: None,
                lifecycle: LifecycleState::Active,
            },
        );
    }

    let mut occupied = Vec::new();
    let mut connection_list = connectome.connections.clone();
    connection_list.sort_by_key(|connection| connection.id);
    let rank = neurons
        .iter()
        .enumerate()
        .map(|(index, neuron)| (neuron.id, index))
        .collect::<BTreeMap<_, _>>();
    let mut connection_results = Vec::with_capacity(connection_list.len());
    for neuron in &neurons {
        let mut incident = connection_list
            .iter()
            .filter(|connection| {
                rank[&connection.pre] == rank[&neuron.id]
                    || rank[&connection.post] == rank[&neuron.id]
            })
            .collect::<Vec<_>>();
        incident.sort_by_key(|connection| connection.id);
        for connection in incident {
            if connection_results
                .iter()
                .any(|result: &ReconstructionConnectionResult| {
                    result.connection_id == connection.id
                })
            {
                continue;
            }
            let result = build_connection_route(
                connection,
                &connectome,
                &soma_ids,
                &mut state,
                &environment,
                &config,
                &mut occupied,
                &mut next_id,
            )?;
            connection_results.push(result);
        }
    }
    // The incident traversal above covers every valid edge. Keep a defensive
    // pass so future ordering changes cannot accidentally omit an edge.
    for connection in &connection_list {
        if !connection_results
            .iter()
            .any(|result| result.connection_id == connection.id)
        {
            connection_results.push(ReconstructionConnectionResult {
                connection_id: connection.id,
                pre: connection.pre,
                post: connection.post,
                status: ReconstructionConnectionStatus::Rejected {
                    reason: MorphologyError::RouteUnavailable(connection.id).to_string(),
                },
            });
        }
    }
    connection_results.sort_by_key(|result| result.connection_id);
    state.revision = if connection_results.iter().any(|result| {
        matches!(
            result.status,
            ReconstructionConnectionStatus::Reconstructed { .. }
        )
    }) {
        2
    } else {
        1
    };
    state.topology_epoch = state.revision;
    state.validate()?;
    let neuron_order = neurons.iter().map(|neuron| neuron.id).collect();
    Ok(PointOnlyReconstruction {
        schema_version: SchemaVersion::new(PointOnlyReconstruction::SCHEMA_VERSION)?,
        source: "point_only_connectome_procedural_reconstruction".to_owned(),
        seed: config.seed,
        connectome: PointOnlyConnectome {
            neurons,
            connections: connection_list,
        },
        environment,
        state,
        neuron_order,
        soma_ids,
        connections: connection_results,
    })
}

fn next_anatomical_id(next: &mut u64) -> Result<AnatomicalId, MorphologyError> {
    let id = AnatomicalId::new(*next, 1)?;
    *next = next
        .checked_add(1)
        .ok_or(MorphologyError::InvalidIdentity)?;
    Ok(id)
}

fn deterministic_unit(seed: u64, salt: u64) -> Vec3 {
    let mut value = seed
        .wrapping_add(salt.wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add(0xD1B5_4A32_D192_ED03);
    let mut next = || {
        value ^= value >> 30;
        value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value ^= value >> 27;
        value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^= value >> 31;
        (value as f64 / u64::MAX as f64) * 2.0 - 1.0
    };
    Vec3 {
        x: next(),
        y: next(),
        z: next(),
    }
    .normalised()
}

fn route_segments(path: &PhysicalPath) -> Vec<OccupiedSegment> {
    path.samples
        .windows(2)
        .map(|pair| OccupiedSegment {
            start: pair[0].position_mm,
            end: pair[1].position_mm,
            radius_mm: pair[0].radius_mm.0.max(pair[1].radius_mm.0),
        })
        .collect()
}

fn path_conflicts(
    path: &PhysicalPath,
    occupied: &[OccupiedSegment],
    environment: &GrowthEnvironment,
) -> bool {
    route_segments(path).iter().any(|candidate| {
        occupied.iter().any(|existing| {
            segment_distance(
                &candidate.start,
                &candidate.end,
                &existing.start,
                &existing.end,
            ) <= candidate.radius_mm + existing.radius_mm + environment.clearance_mm
        })
    })
}

fn point_segment_distance(point: Vec3, start: Vec3, end: Vec3) -> f64 {
    let direction = end.sub(start);
    let length_squared = direction.dot(direction);
    if length_squared <= f64::EPSILON {
        return point.distance(start);
    }
    let fraction = point.sub(start).dot(direction) / length_squared;
    point.distance(start.add(direction.scale(fraction.clamp(0.0, 1.0))))
}

fn path_hits_foreign_soma(
    path: &PhysicalPath,
    connectome: &PointOnlyConnectome,
    config: &ReconstructionConfig,
) -> bool {
    route_segments(path).iter().any(|segment| {
        connectome.neurons.iter().any(|neuron| {
            point_segment_distance(neuron.position_mm, segment.start, segment.end)
                <= config.soma_radius_mm + segment.radius_mm + config.clearance_mm
        })
    })
}

fn candidate_path(
    id: AnatomicalId,
    owner: NeuronId,
    kind: AnatomicalKind,
    start: Vec3,
    end: Vec3,
    bend: Vec3,
    radius_mm: f64,
    velocity: f64,
) -> PhysicalPath {
    PhysicalPath {
        id,
        owner,
        kind,
        samples: vec![
            PathSample {
                position_mm: start,
                radius_mm: LengthMm(radius_mm),
            },
            PathSample {
                position_mm: bend,
                radius_mm: LengthMm(radius_mm),
            },
            PathSample {
                position_mm: end,
                radius_mm: LengthMm(radius_mm),
            },
        ],
        conduction_velocity: VelocityMPerS(velocity),
    }
}

fn build_connection_route(
    connection: &PointOnlyConnection,
    connectome: &PointOnlyConnectome,
    soma_ids: &BTreeMap<NeuronId, AnatomicalId>,
    state: &mut MorphologyState,
    environment: &GrowthEnvironment,
    config: &ReconstructionConfig,
    occupied: &mut Vec<OccupiedSegment>,
    next_id: &mut u64,
) -> Result<ReconstructionConnectionResult, MorphologyError> {
    let pre = connectome
        .neurons
        .iter()
        .find(|neuron| neuron.id == connection.pre)
        .ok_or(MorphologyError::InvalidConnectome)?;
    let post = connectome
        .neurons
        .iter()
        .find(|neuron| neuron.id == connection.post)
        .ok_or(MorphologyError::InvalidConnectome)?;
    let vector = post.position_mm.sub(pre.position_mm);
    let direction = if vector.norm() > f64::EPSILON {
        vector.normalised()
    } else {
        deterministic_unit(config.seed, connection.id)
    };
    let mut lateral = direction
        .cross(deterministic_unit(config.seed, connection.id ^ 0xA5A5))
        .normalised();
    if lateral.norm() <= f64::EPSILON {
        lateral = direction
            .cross(Vec3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            })
            .normalised();
    }
    if lateral.norm() <= f64::EPSILON {
        lateral = Vec3 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        };
    }
    let start_offset =
        config.soma_radius_mm + config.neurite_radius_mm + config.clearance_mm + 1.0e-6;
    let gap_half = config.synaptic_gap_mm * 0.5;
    let self_connection = pre.id == post.id;
    let pre_start = pre.position_mm.add(direction.scale(start_offset));
    let post_start = post.position_mm.sub(direction.scale(start_offset));
    let midpoint = pre.position_mm.add(post.position_mm).scale(0.5);
    let mut last_reason = MorphologyError::RouteUnavailable(connection.id).to_string();
    for attempt in 0..config.max_route_attempts {
        let sign = if attempt % 2 == 0 { 1.0 } else { -1.0 };
        let jitter = deterministic_unit(config.seed ^ connection.id, attempt as u64 + 0xC0DE);
        let lateral_offset = lateral
            .add(jitter.scale(0.35))
            .normalised()
            .scale(config.branch_offset_mm * (1.0 + attempt as f64 * 0.15) * sign);
        let (contact, axon_bend, dendrite_bend) = if self_connection {
            let contact = pre
                .position_mm
                .add(lateral.scale(start_offset + gap_half))
                .add(lateral_offset.scale(0.25));
            let outer_axon = pre.position_mm.add(
                direction
                    .add(lateral)
                    .normalised()
                    .scale(start_offset * 2.0),
            );
            let outer_dendrite = pre.position_mm.add(
                lateral
                    .sub(direction)
                    .normalised()
                    .scale(start_offset * 2.0),
            );
            (contact, outer_axon, outer_dendrite)
        } else {
            let contact = midpoint.add(lateral_offset.scale(0.25));
            (
                contact,
                pre_start.add(contact).scale(0.5),
                post_start.add(contact).scale(0.5),
            )
        };
        let axon_end = contact.sub(lateral.scale(gap_half));
        let dendrite_end = contact.add(lateral.scale(gap_half));
        let axon_id = AnatomicalId::new(*next_id, 1)?;
        let dendrite_id = AnatomicalId::new(
            next_id
                .checked_add(1)
                .ok_or(MorphologyError::InvalidIdentity)?,
            1,
        )?;
        let axon = candidate_path(
            axon_id,
            pre.id,
            AnatomicalKind::Axon,
            pre_start,
            axon_end,
            axon_bend,
            config.neurite_radius_mm,
            config.conduction_velocity_m_per_s,
        );
        let dendrite = candidate_path(
            dendrite_id,
            post.id,
            AnatomicalKind::Dendrite,
            post_start,
            dendrite_end,
            dendrite_bend,
            config.neurite_radius_mm,
            config.conduction_velocity_m_per_s,
        );
        let valid_environment = axon
            .samples
            .windows(2)
            .chain(dendrite.samples.windows(2))
            .all(|pair| {
                environment
                    .validate_step(
                        pair[0].position_mm,
                        pair[1].position_mm,
                        config.neurite_radius_mm,
                    )
                    .is_ok()
            });
        let own_conflict = path_conflicts(&axon, &route_segments(&dendrite), environment)
            || path_conflicts(&dendrite, &route_segments(&axon), environment);
        let soma_conflict = path_hits_foreign_soma(&axon, connectome, config)
            || path_hits_foreign_soma(&dendrite, connectome, config);
        if !valid_environment
            || own_conflict
            || soma_conflict
            || path_conflicts(&axon, occupied, environment)
            || path_conflicts(&dendrite, occupied, environment)
        {
            last_reason = if !valid_environment {
                "candidate leaves the finite environment".to_owned()
            } else {
                "candidate conflicts with reserved swept volume".to_owned()
            };
            continue;
        }
        let axon_element_id = AnatomicalId::new(
            next_id
                .checked_add(2)
                .ok_or(MorphologyError::InvalidIdentity)?,
            1,
        )?;
        let dendrite_element_id = AnatomicalId::new(
            next_id
                .checked_add(3)
                .ok_or(MorphologyError::InvalidIdentity)?,
            1,
        )?;
        let bouton_id = AnatomicalId::new(
            next_id
                .checked_add(4)
                .ok_or(MorphologyError::InvalidIdentity)?,
            1,
        )?;
        let post_site_id = AnatomicalId::new(
            next_id
                .checked_add(5)
                .ok_or(MorphologyError::InvalidIdentity)?,
            1,
        )?;
        let synapse_id = AnatomicalId::new(
            next_id
                .checked_add(6)
                .ok_or(MorphologyError::InvalidIdentity)?,
            1,
        )?;
        state.elements.insert(
            axon_element_id,
            AnatomicalElement {
                id: axon_element_id,
                owner: pre.id,
                kind: AnatomicalKind::Axon,
                parent: Some(Attachment {
                    parent: soma_ids[&pre.id],
                    position: 0.0,
                }),
                lifecycle: LifecycleState::Active,
            },
        );
        state.elements.insert(
            dendrite_element_id,
            AnatomicalElement {
                id: dendrite_element_id,
                owner: post.id,
                kind: AnatomicalKind::Dendrite,
                parent: Some(Attachment {
                    parent: soma_ids[&post.id],
                    position: 0.0,
                }),
                lifecycle: LifecycleState::Active,
            },
        );
        state.elements.insert(
            bouton_id,
            AnatomicalElement {
                id: bouton_id,
                owner: pre.id,
                kind: AnatomicalKind::Bouton,
                parent: Some(Attachment {
                    parent: axon.id,
                    position: 1.0,
                }),
                lifecycle: LifecycleState::Active,
            },
        );
        state.elements.insert(
            post_site_id,
            AnatomicalElement {
                id: post_site_id,
                owner: post.id,
                kind: AnatomicalKind::PostsynapticSite,
                parent: Some(Attachment {
                    parent: dendrite.id,
                    position: 1.0,
                }),
                lifecycle: LifecycleState::Active,
            },
        );
        state.paths.insert(axon.id, axon.clone());
        state.paths.insert(dendrite.id, dendrite.clone());
        let route = SynapticRoute {
            synapse_id,
            axon: axon.clone(),
            dendrite: Some(dendrite.clone()),
            synaptic_delay: TimeMs(config.synaptic_delay_ms),
            receiving_response: TimeMs(config.receiving_response_ms),
            effective_at: config.effective_at,
            route_epoch: config.route_epoch,
        };
        state.synapses.insert(
            synapse_id,
            SynapseContract {
                id: synapse_id,
                pre_site: bouton_id,
                post_site: post_site_id,
                route,
                effective_at: config.effective_at,
                lifecycle: LifecycleState::Active,
            },
        );
        occupied.extend(route_segments(&axon));
        occupied.extend(route_segments(&dendrite));
        *next_id = next_id
            .checked_add(7)
            .ok_or(MorphologyError::InvalidIdentity)?;
        return Ok(ReconstructionConnectionResult {
            connection_id: connection.id,
            pre: connection.pre,
            post: connection.post,
            status: ReconstructionConnectionStatus::Reconstructed {
                synapse_id,
                axon_path_id: axon.id,
                dendrite_path_id: dendrite.id,
            },
        });
    }
    *next_id = next_id
        .checked_add(7)
        .ok_or(MorphologyError::InvalidIdentity)?;
    Ok(ReconstructionConnectionResult {
        connection_id: connection.id,
        pre: connection.pre,
        post: connection.post,
        status: ReconstructionConnectionStatus::Rejected {
            reason: last_reason,
        },
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedGrowth {
    pub proposal_id: u64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrowthCommit {
    pub morphology_revision: u64,
    pub topology_epoch: u64,
    pub applied_proposals: Vec<u64>,
    pub rejected_proposals: Vec<RejectedGrowth>,
}

#[derive(Debug, Clone)]
pub struct MorphologyStore {
    environment: GrowthEnvironment,
    state: MorphologyState,
    retained: VecDeque<MorphologyState>,
    max_retained: usize,
    seen_proposals: BTreeSet<u64>,
}

impl MorphologyStore {
    pub fn new(
        environment: GrowthEnvironment,
        max_retained: usize,
    ) -> Result<Self, MorphologyError> {
        let state = MorphologyState::empty(&environment)?;
        Ok(Self {
            environment,
            state,
            retained: VecDeque::new(),
            max_retained: max_retained.max(1),
            seen_proposals: BTreeSet::new(),
        })
    }

    pub fn state(&self) -> &MorphologyState {
        &self.state
    }

    /// Commit reference geometry only. Executable topology and synapses must
    /// be activated by the topology service at its causal boundary.
    pub fn apply_proposals(
        &mut self,
        mut proposals: Vec<GrowthProposal>,
    ) -> Result<GrowthCommit, MorphologyError> {
        proposals.sort_by_key(|proposal| proposal.proposal_id);
        let initial_revision = self.state.revision;
        let initial_environment = self.environment.revision;
        let mut next = self.state.clone();
        let mut accepted = Vec::new();
        let mut rejected = Vec::new();
        let mut batch_seen = BTreeSet::new();
        for proposal in proposals {
            if self.seen_proposals.contains(&proposal.proposal_id)
                || !batch_seen.insert(proposal.proposal_id)
            {
                return Err(MorphologyError::DuplicateProposal(proposal.proposal_id));
            }
            let result =
                self.validate_proposal(&proposal, initial_revision, initial_environment, &next);
            match result {
                Ok(()) => {
                    next.elements
                        .insert(proposal.new_element.id, proposal.new_element);
                    next.paths.insert(proposal.new_path.id, proposal.new_path);
                    accepted.push(proposal.proposal_id);
                }
                Err(error) => rejected.push(RejectedGrowth {
                    proposal_id: proposal.proposal_id,
                    reason: error.to_string(),
                }),
            }
        }
        next.validate()?;
        self.seen_proposals.extend(batch_seen);
        if !accepted.is_empty() {
            self.retained.push_back(self.state.clone());
            while self.retained.len() > self.max_retained {
                self.retained.pop_front();
            }
            next.revision = self.state.revision.saturating_add(1);
            self.state = next;
        }
        Ok(GrowthCommit {
            morphology_revision: self.state.revision,
            topology_epoch: self.state.topology_epoch,
            applied_proposals: accepted,
            rejected_proposals: rejected,
        })
    }

    fn validate_proposal(
        &self,
        proposal: &GrowthProposal,
        revision: u64,
        environment_revision: u64,
        state: &MorphologyState,
    ) -> Result<(), MorphologyError> {
        if proposal.base_morphology_revision != revision {
            return Err(MorphologyError::StaleMorphologyRevision {
                expected: revision,
                actual: proposal.base_morphology_revision,
            });
        }
        if proposal.base_environment_revision != environment_revision {
            return Err(MorphologyError::StaleEnvironmentRevision {
                expected: environment_revision,
                actual: proposal.base_environment_revision,
            });
        }
        proposal.tip.radius_mm.validate()?;
        if proposal.new_element.lifecycle != LifecycleState::Proposed {
            return Err(MorphologyError::ElectricalActivationRequired);
        }
        if !proposal.tip.position_mm.is_finite()
            || !proposal.tip.orientation.is_finite()
            || !proposal.tip.resource.is_finite()
            || proposal.tip.resource < 0.0
            || proposal.tip.history.iter().any(|point| !point.is_finite())
        {
            return Err(MorphologyError::NonFiniteGeometry);
        }
        if proposal.new_element.owner != proposal.tip.owner
            || state.elements.contains_key(&proposal.new_element.id)
            || state.paths.contains_key(&proposal.new_path.id)
        {
            return Err(MorphologyError::IdentityConflict);
        }
        proposal.new_path.validate()?;
        if proposal.new_path.owner != proposal.tip.owner {
            return Err(MorphologyError::IdentityConflict);
        }
        if proposal.new_element.kind != proposal.new_path.kind {
            return Err(MorphologyError::InvalidOwnership);
        }
        if proposal
            .new_path
            .samples
            .first()
            .is_none_or(|sample| sample.position_mm.distance(proposal.tip.position_mm) > 1.0e-9)
            || proposal
                .new_path
                .samples
                .last()
                .is_none_or(|sample| sample.position_mm.distance(proposal.end_mm) > 1.0e-9)
        {
            return Err(MorphologyError::ProposalGeometryMismatch);
        }
        if let Some(parent_id) = proposal.parent_path {
            let parent = state
                .paths
                .get(&parent_id)
                .ok_or(MorphologyError::InvalidOwnership)?;
            if parent.owner != proposal.tip.owner
                || parent.kind != proposal.new_path.kind
                || proposal.new_element.parent
                    != Some(Attachment {
                        parent: parent_id,
                        position: 1.0,
                    })
                || parent.samples.last().is_none_or(|sample| {
                    sample.position_mm.distance(proposal.tip.position_mm) > 1.0e-12
                })
            {
                return Err(MorphologyError::InvalidAttachment);
            }
        } else if proposal.new_element.parent.is_some() {
            return Err(MorphologyError::InvalidAttachment);
        }
        // Validate the stored curve, not the chord between its endpoints.
        // `state` includes earlier accepted proposals, so this also reserves
        // their full swept volumes against later contenders in the batch.
        for (index, candidate) in route_segments(&proposal.new_path).iter().enumerate() {
            self.environment
                .validate_step(candidate.start, candidate.end, candidate.radius_mm)?;
            for path in state.paths.values() {
                for existing in route_segments(path) {
                    let capsule = growth_cone::GrowthCapsule {
                        path: path.id,
                        owner: path.owner,
                        start_mm: existing.start,
                        end_mm: existing.end,
                        radius_mm: LengthMm(existing.radius_mm),
                    };
                    let attachment = index == 0
                        && candidate.radius_mm <= proposal.tip.radius_mm.0
                        && growth_cone::terminal_attachment(
                            &proposal.tip,
                            proposal.parent_path,
                            &capsule,
                            candidate.end.sub(candidate.start).normalised(),
                        );
                    let distance = segment_distance(
                        &candidate.start,
                        &candidate.end,
                        &existing.start,
                        &existing.end,
                    );
                    if !attachment
                        && (!distance.is_finite()
                            || distance
                                <= candidate.radius_mm
                                    + existing.radius_mm
                                    + self.environment.clearance_mm)
                    {
                        return Err(MorphologyError::OccupancyConflict);
                    }
                }
            }
        }
        Ok(())
    }
}

fn segment_distance(a0: &Vec3, a1: &Vec3, b0: &Vec3, b1: &Vec3) -> f64 {
    // Closest points on two finite line segments. This catches skew and
    // crossing paths even when their midpoints and endpoints are separated.
    let u = a1.sub(*a0);
    let v = b1.sub(*b0);
    let w = a0.sub(*b0);
    let a = u.dot(u);
    let b = u.dot(v);
    let c = v.dot(v);
    let d = u.dot(w);
    let e = v.dot(w);
    let denominator = a * c - b * b;
    let (mut s, mut t);
    if a <= 1.0e-24 && c <= 1.0e-24 {
        return a0.distance(*b0);
    }
    if a <= 1.0e-24 {
        s = 0.0;
        t = (e / c).clamp(0.0, 1.0);
    } else if c <= 1.0e-24 {
        t = 0.0;
        s = (-d / a).clamp(0.0, 1.0);
    } else {
        s = if denominator > f64::EPSILON * a * c {
            ((b * e - c * d) / denominator).clamp(0.0, 1.0)
        } else {
            0.0
        };
        t = (b * s + e) / c;
        if t < 0.0 {
            t = 0.0;
            s = (-d / a).clamp(0.0, 1.0);
        } else if t > 1.0 {
            t = 1.0;
            s = ((b - d) / a).clamp(0.0, 1.0);
        }
    }
    a0.add(u.scale(s)).distance(b0.add(v.scale(t)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayMode {
    Anatomical,
    SyntheticColumns,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayRole {
    Sensory,
    Hidden,
    Output,
    Unassigned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayProvenance {
    ObservedAnatomy,
    ProceduralAnatomy,
    SyntheticTopology,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayCoverage {
    pub region: Option<AxisAlignedBox>,
    /// Actual ellipsoidal membrane, when available. `region` alone is only
    /// a coverage box and must not be mistaken for the tissue boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub membrane: Option<DisplayMembrane>,
    pub complete: bool,
    pub truncated: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DisplayMembrane {
    pub centre_mm: Vec3,
    pub radii_mm: Vec3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplaySnapshot {
    pub schema_version: SchemaVersion,
    pub morphology_revision: u64,
    pub topology_epoch: u64,
    pub route_epoch: u64,
    pub sequence: u64,
    pub mode: DisplayMode,
    pub provenance: DisplayProvenance,
    pub coverage: DisplayCoverage,
    pub nodes: Vec<DisplayNode>,
    pub edges: Vec<DisplayEdge>,
    /// Stored continuous neurite paths, including branches without a
    /// committed synapse. These remain derived display geometry.
    #[serde(default)]
    pub paths: Vec<DisplayPath>,
    /// Point markers for biological contacts, kept separate from centreline
    /// geometry so renderers do not invent decorative neurite segments.
    #[serde(default)]
    pub markers: Vec<DisplayMarker>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayNode {
    pub id: AnatomicalId,
    pub role: DisplayRole,
    pub layer: Option<usize>,
    pub position_mm: Vec3,
    pub kind: AnatomicalKind,
    /// Deterministic presentation slot. Adjacent nodes in this snapshot are
    /// assigned different slots; renderers derive their hue from this value.
    #[serde(default)]
    pub colour_slot: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayEdge {
    pub source: AnatomicalId,
    pub target: AnatomicalId,
    pub points_mm: Vec<Vec3>,
    pub multiplicity: u32,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayPath {
    pub id: AnatomicalId,
    pub owner: AnatomicalId,
    pub kind: AnatomicalKind,
    pub points_mm: Vec<Vec3>,
    pub radius_mm: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayMarker {
    pub id: AnatomicalId,
    pub owner: AnatomicalId,
    pub kind: AnatomicalKind,
    pub position_mm: Vec3,
    #[serde(default)]
    pub synapse_id: Option<AnatomicalId>,
}

impl DisplaySnapshot {
    pub const SCHEMA_VERSION: u16 = 2;

    /// Construct a bounded immutable view for a renderer or management client.
    ///
    /// The input vectors are sorted into canonical identity order and clipped
    /// before publication. A clipped view is explicitly incomplete; it is never
    /// represented as a complete anatomical witness.
    pub fn bounded(
        morphology_revision: u64,
        topology_epoch: u64,
        route_epoch: u64,
        sequence: u64,
        mode: DisplayMode,
        provenance: DisplayProvenance,
        coverage_region: Option<AxisAlignedBox>,
        nodes: Vec<DisplayNode>,
        edges: Vec<DisplayEdge>,
        max_nodes: usize,
        max_edges: usize,
        unavailable_reason: Option<String>,
    ) -> Result<Self, MorphologyError> {
        Self::bounded_with_paths(
            morphology_revision,
            topology_epoch,
            route_epoch,
            sequence,
            mode,
            provenance,
            coverage_region,
            nodes,
            edges,
            Vec::new(),
            max_nodes,
            max_edges,
            unavailable_reason,
        )
    }

    pub fn bounded_with_paths(
        morphology_revision: u64,
        topology_epoch: u64,
        route_epoch: u64,
        sequence: u64,
        mode: DisplayMode,
        provenance: DisplayProvenance,
        coverage_region: Option<AxisAlignedBox>,
        mut nodes: Vec<DisplayNode>,
        mut edges: Vec<DisplayEdge>,
        mut paths: Vec<DisplayPath>,
        max_nodes: usize,
        max_edges: usize,
        unavailable_reason: Option<String>,
    ) -> Result<Self, MorphologyError> {
        if morphology_revision == 0 || topology_epoch == 0 || route_epoch == 0 {
            return Err(MorphologyError::InvalidDisplayVersion);
        }
        if let Some(region) = coverage_region {
            region.validate()?;
        }
        let schema_version = SchemaVersion::new(Self::SCHEMA_VERSION)?;
        let max_nodes = max_nodes.max(1);
        let max_edges = max_edges.max(1);
        nodes.sort_by_key(|node| node.id);
        edges.sort_by_key(|edge| (edge.source, edge.target));
        for node in &nodes {
            if !node.position_mm.is_finite() {
                return Err(MorphologyError::NonFiniteGeometry);
            }
        }
        let mut seen = BTreeSet::new();
        nodes.retain(|node| seen.insert(node.id));
        let truncated_nodes = nodes.len() > max_nodes;
        nodes.truncate(max_nodes);
        let visible = nodes.iter().map(|node| node.id).collect::<BTreeSet<_>>();
        paths.sort_by_key(|path| path.id);
        paths.retain(|path| {
            path.points_mm.len() >= 2
                && path.points_mm.iter().all(|point| point.is_finite())
                && path.radius_mm.is_finite()
                && path.radius_mm >= 0.0
                && visible.contains(&path.owner)
        });
        let truncated_paths = paths.len() > max_edges;
        paths.truncate(max_edges);
        edges.retain(|edge| {
            edge.points_mm.iter().all(|point| point.is_finite())
                && visible.contains(&edge.source)
                && visible.contains(&edge.target)
                && edge.multiplicity > 0
        });
        let truncated_edges = edges.len() > max_edges;
        edges.truncate(max_edges);
        assign_display_colour_slots(&mut nodes, &edges);
        let truncated = truncated_nodes || truncated_edges || truncated_paths;
        let complete = !nodes.is_empty()
            && !truncated
            && unavailable_reason.is_none()
            && provenance != DisplayProvenance::Unavailable;
        Ok(Self {
            schema_version,
            morphology_revision,
            topology_epoch,
            route_epoch,
            sequence,
            mode,
            provenance,
            coverage: DisplayCoverage {
                region: coverage_region,
                membrane: None,
                complete,
                truncated,
                unavailable_reason,
            },
            nodes,
            edges,
            paths,
            markers: Vec::new(),
        })
    }

    pub fn bounded_with_paths_and_markers(
        morphology_revision: u64,
        topology_epoch: u64,
        route_epoch: u64,
        sequence: u64,
        mode: DisplayMode,
        provenance: DisplayProvenance,
        coverage_region: Option<AxisAlignedBox>,
        nodes: Vec<DisplayNode>,
        edges: Vec<DisplayEdge>,
        paths: Vec<DisplayPath>,
        mut markers: Vec<DisplayMarker>,
        max_nodes: usize,
        max_edges: usize,
        unavailable_reason: Option<String>,
    ) -> Result<Self, MorphologyError> {
        let mut snapshot = Self::bounded_with_paths(
            morphology_revision,
            topology_epoch,
            route_epoch,
            sequence,
            mode,
            provenance,
            coverage_region,
            nodes,
            edges,
            paths,
            max_nodes,
            max_edges,
            unavailable_reason,
        )?;
        markers.sort_by_key(|marker| marker.id);
        let visible = snapshot
            .nodes
            .iter()
            .map(|node| node.id)
            .collect::<BTreeSet<_>>();
        markers.retain(|marker| marker.position_mm.is_finite() && visible.contains(&marker.owner));
        if markers.len() > max_edges.max(1) {
            snapshot.coverage.truncated = true;
            snapshot.coverage.complete = false;
            markers.truncate(max_edges.max(1));
        }
        snapshot.markers = markers;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), MorphologyError> {
        if !(1..=Self::SCHEMA_VERSION).contains(&self.schema_version.raw())
            || self.morphology_revision == 0
            || self.topology_epoch == 0
            || self.route_epoch == 0
        {
            return Err(MorphologyError::InvalidDisplayVersion);
        }
        if let Some(region) = self.coverage.region {
            region.validate()?;
        }
        if self.nodes.iter().any(|node| !node.position_mm.is_finite())
            || self
                .edges
                .iter()
                .flat_map(|edge| edge.points_mm.iter())
                .any(|point| !point.is_finite())
            || self.paths.iter().any(|path| {
                path.points_mm.iter().any(|point| !point.is_finite())
                    || !path.radius_mm.is_finite()
                    || path.radius_mm < 0.0
            })
            || self
                .markers
                .iter()
                .any(|marker| !marker.position_mm.is_finite())
        {
            return Err(MorphologyError::NonFiniteGeometry);
        }
        Ok(())
    }
}

/// Assign stable presentation slots with a deterministic greedy graph
/// colouring. The slot is derived from identity first, then probed only when
/// an already-coloured neighbour uses it. This keeps unrelated neurons free to
/// share a palette colour while ensuring adjacent neurons remain distinguishable
/// in every renderer consuming the contract.
fn assign_display_colour_slots(nodes: &mut [DisplayNode], edges: &[DisplayEdge]) {
    let visible = nodes.iter().map(|node| node.id).collect::<BTreeSet<_>>();
    let mut neighbours = BTreeMap::<AnatomicalId, BTreeSet<AnatomicalId>>::new();
    for edge in edges {
        if edge.source == edge.target
            || !visible.contains(&edge.source)
            || !visible.contains(&edge.target)
        {
            continue;
        }
        neighbours
            .entry(edge.source)
            .or_default()
            .insert(edge.target);
        neighbours
            .entry(edge.target)
            .or_default()
            .insert(edge.source);
    }

    let mut assigned = BTreeMap::<AnatomicalId, u32>::new();
    for node in nodes.iter_mut() {
        let mut hash = node.id.value ^ (u64::from(node.id.generation) << 32);
        hash ^= hash >> 30;
        hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        hash ^= hash >> 27;
        hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
        hash ^= hash >> 31;
        let mut slot = (hash as u32) % 2048;
        let used = neighbours
            .get(&node.id)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| assigned.get(id))
            .copied()
            .collect::<BTreeSet<_>>();
        while used.contains(&slot) {
            slot = slot.wrapping_add(1);
        }
        node.colour_slot = slot;
        assigned.insert(node.id, slot);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn anatomy_identity_json_preserves_full_unsigned_range_and_legacy_input() {
        for value in [1, (1u64 << 60) + 1, (1u64 << 60) + 2, u64::MAX] {
            let id = super::AnatomicalId::new(value, 7).unwrap();
            let encoded = serde_json::to_value(id).unwrap();
            assert_eq!(encoded["value"].as_str(), Some(value.to_string().as_str()));
            assert_eq!(
                serde_json::from_value::<super::AnatomicalId>(encoded).unwrap(),
                id
            );
            let old = serde_json::json!({"value": value, "generation": 7});
            assert_eq!(
                serde_json::from_value::<super::AnatomicalId>(old).unwrap(),
                id
            );
        }
        for invalid in ["-1", "+1", "1.5", "18446744073709551616"] {
            assert!(
                serde_json::from_value::<super::AnatomicalId>(
                    serde_json::json!({"value": invalid, "generation": 1})
                )
                .is_err()
            );
        }
    }

    #[test]
    fn anatomical_identity_maps_round_trip_and_reject_duplicate_generations() {
        #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        struct Fixture {
            #[serde(with = "super::anatomical_id_map")]
            entries: std::collections::BTreeMap<super::AnatomicalId, u64>,
        }
        let one = super::AnatomicalId::new(7, 1).unwrap();
        let two = super::AnatomicalId::new(7, 2).unwrap();
        let fixture = Fixture {
            entries: [(one, 10), (two, 20)].into(),
        };
        let json = serde_json::to_string(&fixture).unwrap();
        assert_eq!(fixture, serde_json::from_str(&json).unwrap());
        let legacy: Fixture = serde_json::from_str(r#"{"entries":{}}"#).unwrap();
        assert!(legacy.entries.is_empty());
        assert!(
            serde_json::from_str::<Fixture>(
                r#"{"entries":[[{"value":7,"generation":1},10],[{"value":7,"generation":1},20]]}"#
            )
            .is_err()
        );
    }

    use super::*;

    fn environment() -> GrowthEnvironment {
        GrowthEnvironment {
            revision: 1,
            frame: CoordinateFrame::default(),
            volume: AxisAlignedBox {
                min: Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                max: Vec3 {
                    x: 10.0,
                    y: 10.0,
                    z: 10.0,
                },
            },
            forbidden: vec![AxisAlignedBox {
                min: Vec3 {
                    x: 4.0,
                    y: 0.0,
                    z: 0.0,
                },
                max: Vec3 {
                    x: 6.0,
                    y: 10.0,
                    z: 10.0,
                },
            }],
            clearance_mm: 0.1,
        }
    }

    fn path(id: AnatomicalId, owner: NeuronId, start: Vec3, end: Vec3) -> PhysicalPath {
        PhysicalPath {
            id,
            owner,
            kind: AnatomicalKind::Axon,
            samples: vec![
                PathSample {
                    position_mm: start,
                    radius_mm: LengthMm(0.1),
                },
                PathSample {
                    position_mm: end,
                    radius_mm: LengthMm(0.1),
                },
            ],
            conduction_velocity: VelocityMPerS(1.0),
        }
    }

    #[test]
    fn route_delay_uses_arc_length_and_never_arrives_early() {
        let route = SynapticRoute {
            synapse_id: AnatomicalId::new(3, 1).unwrap(),
            axon: path(
                AnatomicalId::new(4, 1).unwrap(),
                NeuronId::new(1).unwrap(),
                Vec3::ZERO,
                Vec3 {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            ),
            dendrite: None,
            synaptic_delay: TimeMs(0.5),
            receiving_response: TimeMs(0.0),
            effective_at: LogicalTag::ZERO,
            route_epoch: 1,
        };
        let timing = route.timing().unwrap();
        assert_eq!(timing.total_delay.0, 1.5);
        let (arrival, represented) = route.quantised_arrival(LogicalTag::ZERO, 1.0).unwrap();
        assert_eq!(arrival, LogicalTag::new(2, 0));
        assert!(represented.0 >= timing.total_delay.0);
    }

    #[test]
    fn obstacle_swept_volume_blocks_tunnelling() {
        let env = environment();
        assert_eq!(
            env.validate_step(
                Vec3 {
                    x: 1.0,
                    y: 5.0,
                    z: 5.0
                },
                Vec3 {
                    x: 9.0,
                    y: 5.0,
                    z: 5.0
                },
                0.1
            ),
            Err(MorphologyError::ObstacleCollision)
        );
    }

    #[test]
    fn finite_volume_requires_both_endpoints_and_full_radius_inside() {
        let env = GrowthEnvironment {
            forbidden: vec![],
            ..environment()
        };
        let middle = Vec3 {
            x: 5.0,
            y: 5.0,
            z: 5.0,
        };
        for outside in [-1.0, 0.1, 9.9, 11.0] {
            let end = Vec3 {
                x: outside,
                ..middle
            };
            assert_eq!(
                env.validate_step(middle, end, 0.1),
                Err(MorphologyError::OutsideEnvironment)
            );
            assert_eq!(
                env.validate_step(end, middle, 0.1),
                Err(MorphologyError::OutsideEnvironment)
            );
        }
        assert_eq!(
            env.validate_step(middle, middle, 6.0),
            Err(MorphologyError::OutsideEnvironment)
        );
        assert!(
            env.validate_step(middle, Vec3 { x: 9.0, ..middle }, 0.1)
                .is_ok()
        );
    }

    #[test]
    fn capsule_distance_is_symmetric_for_parallel_and_tiny_segments() {
        let x = |x| Vec3 { x, y: 0.0, z: 0.0 };
        for scale in [1.0, 1.0e-8] {
            for (a, b) in [(0.0, 2.0), (2.0, 0.0)] {
                for (c, d) in [(1.0, 3.0), (3.0, 1.0)] {
                    assert!(
                        segment_distance(
                            &x(a * scale),
                            &x(b * scale),
                            &x(c * scale),
                            &x(d * scale)
                        ) < 1.0e-12
                    );
                }
            }
            assert!(
                (segment_distance(&x(0.0), &x(scale), &x(2.0 * scale), &x(3.0 * scale)) - scale)
                    .abs()
                    < 1.0e-12
            );
        }
    }

    #[test]
    fn admission_checks_bends_and_occupancy_from_previous_batches() {
        use growth_cone::*;
        let env = GrowthEnvironment {
            forbidden: vec![],
            ..environment()
        };
        let mut store = MorphologyStore::new(env.clone(), 2).unwrap();
        let owner = NeuronId::new(1).unwrap();
        let start = Vec3 {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        };
        let end = Vec3 { x: 3.0, ..start };
        let tip = GrowthTip {
            id: AnatomicalId::new(100, 1).unwrap(),
            owner,
            position_mm: start,
            orientation: end.sub(start).normalised(),
            radius_mm: LengthMm(0.1),
            cell_type: "test".into(),
            resource: 100.0,
            history: vec![],
        };
        let proposal = |id, revision| {
            ConeExtension {
                tip: tip.id,
                kind: AnatomicalKind::Axon,
                growth_step: 1,
                base_morphology_revision: revision,
                base_environment_revision: 1,
                start_mm: start,
                end_mm: end,
                heading: tip.orientation,
                resource_cost: 2.0,
            }
            .into_proposal(
                tip.clone(),
                GrowthAllocation {
                    proposal_id: id,
                    path_id: AnatomicalId::new(id + 200, 1).unwrap(),
                    parent_path: None,
                    element: AnatomicalElement {
                        id: AnatomicalId::new(id + 300, 1).unwrap(),
                        owner,
                        kind: AnatomicalKind::Axon,
                        parent: None,
                        lifecycle: LifecycleState::Proposed,
                    },
                    conduction_velocity: VelocityMPerS(1.0),
                    activation: LogicalTag::ZERO,
                },
            )
            .unwrap()
        };
        let mut curved = proposal(1, 1);
        curved.new_path.samples.insert(
            1,
            PathSample {
                position_mm: Vec3 { y: 11.0, ..start },
                radius_mm: LengthMm(0.1),
            },
        );
        let rejected = store.apply_proposals(vec![curved]).unwrap();
        assert_eq!(
            rejected.rejected_proposals[0].reason,
            MorphologyError::OutsideEnvironment.to_string()
        );
        assert!(store.state().paths.is_empty());
        let mut premature = proposal(4, 1);
        premature.new_element.lifecycle = LifecycleState::Active;
        assert_eq!(
            store
                .apply_proposals(vec![premature])
                .unwrap()
                .rejected_proposals[0]
                .reason,
            MorphologyError::ElectricalActivationRequired.to_string()
        );
        assert_eq!(
            store
                .apply_proposals(vec![proposal(2, 1)])
                .unwrap()
                .applied_proposals,
            vec![2]
        );
        let rejected = store.apply_proposals(vec![proposal(3, 2)]).unwrap();
        assert_eq!(
            rejected.rejected_proposals[0].reason,
            MorphologyError::OccupancyConflict.to_string()
        );
        assert_eq!(store.state().paths.len(), 1);
    }

    #[test]
    fn growth_batch_is_deterministic_and_rejects_competing_occupancy() {
        let owner = NeuronId::new(1).unwrap();
        let mut store = MorphologyStore::new(
            GrowthEnvironment {
                forbidden: Vec::new(),
                ..environment()
            },
            2,
        )
        .unwrap();
        let tip = |id| GrowthTip {
            id: AnatomicalId::new(id, 1).unwrap(),
            owner,
            position_mm: Vec3 {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
            orientation: Vec3 {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            radius_mm: LengthMm(0.1),
            cell_type: "test".to_owned(),
            resource: 1.0,
            history: Vec::new(),
        };
        let proposal = |id, end| GrowthProposal {
            proposal_id: id,
            base_morphology_revision: 1,
            base_environment_revision: 1,
            tip: tip(id + 10),
            end_mm: end,
            new_element: AnatomicalElement {
                id: AnatomicalId::new(id + 100, 1).unwrap(),
                owner,
                kind: AnatomicalKind::Axon,
                parent: None,
                lifecycle: LifecycleState::Proposed,
            },
            new_path: path(
                AnatomicalId::new(id + 200, 1).unwrap(),
                owner,
                Vec3 {
                    x: 1.0,
                    y: 1.0,
                    z: 1.0,
                },
                end,
            ),
            parent_path: None,
            activation: LogicalTag::ZERO,
        };
        let result = store
            .apply_proposals(vec![
                proposal(
                    2,
                    Vec3 {
                        x: 3.0,
                        y: 1.0,
                        z: 1.0,
                    },
                ),
                proposal(
                    1,
                    Vec3 {
                        x: 3.0,
                        y: 1.0,
                        z: 1.0,
                    },
                ),
            ])
            .unwrap();
        assert_eq!(result.applied_proposals, vec![1]);
        assert_eq!(result.rejected_proposals.len(), 1);
        assert_eq!(store.state().revision, 2);
    }

    #[test]
    fn occupancy_detects_crossing_segments_without_shared_endpoints() {
        let distance = segment_distance(
            &Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            &Vec3 {
                x: 2.0,
                y: 2.0,
                z: 0.0,
            },
            &Vec3 {
                x: 0.0,
                y: 2.0,
                z: 0.0,
            },
            &Vec3 {
                x: 2.0,
                y: 0.0,
                z: 0.0,
            },
        );
        assert!(distance <= 1.0e-12);
    }

    fn reconstruction_environment() -> GrowthEnvironment {
        GrowthEnvironment {
            revision: 1,
            frame: CoordinateFrame::default(),
            volume: AxisAlignedBox {
                min: Vec3 {
                    x: -2.0,
                    y: -2.0,
                    z: -2.0,
                },
                max: Vec3 {
                    x: 2.0,
                    y: 2.0,
                    z: 2.0,
                },
            },
            forbidden: Vec::new(),
            clearance_mm: 0.002,
        }
    }

    fn point_connectome(reversed: bool) -> PointOnlyConnectome {
        let mut neurons = vec![
            PointOnlyNeuron {
                id: NeuronId::new(1).unwrap(),
                position_mm: Vec3 {
                    x: -0.8,
                    y: 0.0,
                    z: 0.0,
                },
                role: ConnectomeRole::Sensory,
                layer: None,
                cell_type: "input".to_owned(),
                formation_order: 0,
            },
            PointOnlyNeuron {
                id: NeuronId::new(2).unwrap(),
                position_mm: Vec3 {
                    x: 0.0,
                    y: 0.1,
                    z: 0.0,
                },
                role: ConnectomeRole::Hidden,
                layer: Some(0),
                cell_type: "hidden".to_owned(),
                formation_order: 1,
            },
            PointOnlyNeuron {
                id: NeuronId::new(3).unwrap(),
                position_mm: Vec3 {
                    x: 0.8,
                    y: 0.0,
                    z: 0.0,
                },
                role: ConnectomeRole::Output,
                layer: None,
                cell_type: "output".to_owned(),
                formation_order: 2,
            },
        ];
        let mut connections = vec![
            PointOnlyConnection {
                id: 20,
                pre: NeuronId::new(2).unwrap(),
                post: NeuronId::new(3).unwrap(),
                kind: "forward".to_owned(),
            },
            PointOnlyConnection {
                id: 10,
                pre: NeuronId::new(1).unwrap(),
                post: NeuronId::new(2).unwrap(),
                kind: "input".to_owned(),
            },
        ];
        if reversed {
            neurons.reverse();
            connections.reverse();
        }
        PointOnlyConnectome {
            neurons,
            connections,
        }
    }

    #[test]
    fn point_only_reconstruction_is_canonical_and_orders_inner_dense_neurons_first() {
        let first = reconstruct_point_only_connectome(
            point_connectome(false),
            reconstruction_environment(),
            ReconstructionConfig::default(),
        )
        .unwrap();
        let second = reconstruct_point_only_connectome(
            point_connectome(true),
            reconstruction_environment(),
            ReconstructionConfig::default(),
        )
        .unwrap();
        assert_eq!(first, second);
        assert_eq!(first.neuron_order[0], NeuronId::new(2).unwrap());
        assert_eq!(first.connections.len(), 2);
        assert!(
            first.connections.iter().all(|connection| matches!(
                connection.status,
                ReconstructionConnectionStatus::Reconstructed { .. }
            )),
            "{:?}",
            first.connections
        );
    }

    #[test]
    fn point_only_reconstruction_commits_paired_routes_without_overlap() {
        let reconstruction = reconstruct_point_only_connectome(
            point_connectome(false),
            reconstruction_environment(),
            ReconstructionConfig::default(),
        )
        .unwrap();
        for connection in &reconstruction.connections {
            let ReconstructionConnectionStatus::Reconstructed { synapse_id, .. } =
                connection.status
            else {
                panic!("fixture route should be reconstructable");
            };
            let route = &reconstruction.state.synapses[&synapse_id].route;
            assert!(route.dendrite.is_some());
            assert!(route.axon.samples.len() >= 3);
            assert!(route.dendrite.as_ref().unwrap().samples.len() >= 3);
            route.timing().unwrap();
        }
        let paths = reconstruction.state.paths.values().collect::<Vec<_>>();
        for (index, left) in paths.iter().enumerate() {
            for right in paths.iter().skip(index + 1) {
                let separated = route_segments(left).iter().all(|a| {
                    route_segments(right).iter().all(|b| {
                        segment_distance(&a.start, &a.end, &b.start, &b.end)
                            > a.radius_mm + b.radius_mm + reconstruction.environment.clearance_mm
                    })
                });
                assert!(separated, "reconstructed paths overlap");
            }
        }
    }

    #[test]
    fn point_only_anatomical_display_exposes_paired_axon_and_dendrite_paths() {
        let reconstruction = reconstruct_point_only_connectome(
            point_connectome(false),
            reconstruction_environment(),
            ReconstructionConfig::default(),
        )
        .unwrap();
        let snapshot = reconstruction.display_snapshot(1, 64, 128).unwrap();
        assert!(
            snapshot
                .paths
                .iter()
                .any(|path| path.kind == AnatomicalKind::Axon)
        );
        assert!(
            snapshot
                .paths
                .iter()
                .any(|path| path.kind == AnatomicalKind::Dendrite)
        );
        assert!(
            snapshot
                .edges
                .iter()
                .any(|edge| edge.kind == "procedural_route")
        );
        assert!(
            snapshot
                .markers
                .iter()
                .any(|marker| marker.kind == AnatomicalKind::Bouton)
        );
        assert!(
            snapshot
                .markers
                .iter()
                .any(|marker| marker.kind == AnatomicalKind::PostsynapticSite)
        );
        assert!(
            snapshot
                .markers
                .iter()
                .any(|marker| marker.kind == AnatomicalKind::Synapse)
        );
        snapshot.validate().unwrap();
    }

    #[test]
    fn display_colour_slots_separate_adjacent_neurons() {
        let a = AnatomicalId::new(1, 1).unwrap();
        let b = AnatomicalId::new(2, 1).unwrap();
        let c = AnatomicalId::new(3, 1).unwrap();
        let snapshot = DisplaySnapshot::bounded(
            1,
            1,
            1,
            1,
            DisplayMode::Anatomical,
            DisplayProvenance::ProceduralAnatomy,
            None,
            vec![
                DisplayNode {
                    id: a,
                    role: DisplayRole::Hidden,
                    layer: Some(0),
                    position_mm: Vec3 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    kind: AnatomicalKind::Soma,
                    colour_slot: 0,
                },
                DisplayNode {
                    id: b,
                    role: DisplayRole::Hidden,
                    layer: Some(0),
                    position_mm: Vec3 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    kind: AnatomicalKind::Soma,
                    colour_slot: 0,
                },
                DisplayNode {
                    id: c,
                    role: DisplayRole::Hidden,
                    layer: Some(0),
                    position_mm: Vec3 {
                        x: 2.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    kind: AnatomicalKind::Soma,
                    colour_slot: 0,
                },
            ],
            vec![
                DisplayEdge {
                    source: a,
                    target: b,
                    points_mm: vec![],
                    multiplicity: 1,
                    kind: "connection".to_owned(),
                },
                DisplayEdge {
                    source: b,
                    target: c,
                    points_mm: vec![],
                    multiplicity: 1,
                    kind: "connection".to_owned(),
                },
            ],
            10,
            10,
            None,
        )
        .unwrap();
        let slots = snapshot
            .nodes
            .iter()
            .map(|node| (node.id, node.colour_slot))
            .collect::<BTreeMap<_, _>>();
        assert_ne!(slots[&a], slots[&b]);
        assert_ne!(slots[&b], slots[&c]);
    }

    #[test]
    fn failed_route_is_reported_without_dropping_the_connectome_edge() {
        let mut connectome = point_connectome(false);
        connectome.connections.push(PointOnlyConnection {
            id: 30,
            pre: NeuronId::new(1).unwrap(),
            post: NeuronId::new(3).unwrap(),
            kind: "crossing".to_owned(),
        });
        let mut config = ReconstructionConfig::default();
        config.max_route_attempts = 1;
        let reconstruction =
            reconstruct_point_only_connectome(connectome, reconstruction_environment(), config)
                .unwrap();
        assert_eq!(reconstruction.connectome.connections.len(), 3);
        assert_eq!(reconstruction.connections.len(), 3);
        assert!(reconstruction.state.validate().is_ok());
    }
}
