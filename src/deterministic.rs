//! Deterministic identity, logical-time and numerical primitives.
//!
//! These types are deliberately independent of transport, persistence, UI and
//! consensus.  They provide the shared vocabulary used by later execution
//! phases while legacy vector-indexed execution remains available.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;
use thiserror::Error;

/// Errors raised before an authoritative primitive mutates state.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PrimitiveError {
    #[error("stable identifier must be non-zero")]
    ZeroId,
    #[error("invalid stable identifier: {0}")]
    InvalidId(String),
    #[error("logical-time overflow while {operation}")]
    LogicalTimeOverflow { operation: &'static str },
    #[error("logical tag moved backwards from {current} to {next}")]
    BackwardsTag {
        current: LogicalTag,
        next: LogicalTag,
    },
    #[error("duplicate stable identifier in generation map: {0}")]
    DuplicateStableId(u64),
    #[error("stable/dense map belongs to generation {actual}, not {expected}")]
    GenerationMismatch {
        expected: TopologyGeneration,
        actual: TopologyGeneration,
    },
    #[error("dense index {index} is outside the map")]
    DenseIndexOutOfBounds { index: usize },
    #[error("schema version must be non-zero")]
    InvalidSchemaVersion,
    #[error("numeric value must be finite")]
    NonFiniteNumeric,
    #[error("numeric value is outside the representable range")]
    NumericOutOfRange,
    #[error("numeric denominator must be positive")]
    InvalidDenominator,
}

macro_rules! stable_id {
    ($name:ident) => {
        /// A stable, generation-independent identity.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Construct an identifier, rejecting the reserved zero value.
            pub const fn new(raw: u64) -> Result<Self, PrimitiveError> {
                if raw == 0 {
                    Err(PrimitiveError::ZeroId)
                } else {
                    Ok(Self(raw))
                }
            }

            /// Return the wire/storage representation.
            pub const fn raw(self) -> u64 {
                self.0
            }
        }

        impl TryFrom<u64> for $name {
            type Error = PrimitiveError;

            fn try_from(value: u64) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl FromStr for $name {
            type Err = PrimitiveError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                let raw = value
                    .parse::<u64>()
                    .map_err(|_| PrimitiveError::InvalidId(value.to_owned()))?;
                Self::new(raw)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl From<$name> for u64 {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let raw = u64::deserialize(deserializer)?;
                Self::new(raw).map_err(serde::de::Error::custom)
            }
        }
    };
}

stable_id!(BrainId);
stable_id!(ShardId);
stable_id!(NeuronId);
stable_id!(SynapseId);
stable_id!(TerminalId);
stable_id!(RouteId);
stable_id!(StreamId);
stable_id!(EventId);
stable_id!(ComponentId);

/// A monotonically increasing topology generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct TopologyGeneration(u64);

impl TopologyGeneration {
    pub const INITIAL: Self = Self(1);

    pub const fn new(raw: u64) -> Result<Self, PrimitiveError> {
        if raw == 0 {
            Err(PrimitiveError::ZeroId)
        } else {
            Ok(Self(raw))
        }
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TopologyGeneration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl TryFrom<u64> for TopologyGeneration {
    type Error = PrimitiveError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl FromStr for TopologyGeneration {
    type Err = PrimitiveError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<u64>()
            .map_err(|_| PrimitiveError::InvalidId(value.to_owned()))
            .and_then(Self::new)
    }
}

impl<'de> Deserialize<'de> for TopologyGeneration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = u64::deserialize(deserializer)?;
        Self::new(raw).map_err(serde::de::Error::custom)
    }
}

/// A partition generation; it is distinct from a biological topology generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PartitionGeneration(u64);

impl PartitionGeneration {
    pub const INITIAL: Self = Self(1);

    pub const fn new(raw: u64) -> Result<Self, PrimitiveError> {
        if raw == 0 {
            Err(PrimitiveError::ZeroId)
        } else {
            Ok(Self(raw))
        }
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PartitionGeneration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl TryFrom<u64> for PartitionGeneration {
    type Error = PrimitiveError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl FromStr for PartitionGeneration {
    type Err = PrimitiveError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<u64>()
            .map_err(|_| PrimitiveError::InvalidId(value.to_owned()))
            .and_then(Self::new)
    }
}

impl<'de> Deserialize<'de> for PartitionGeneration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = u64::deserialize(deserializer)?;
        Self::new(raw).map_err(serde::de::Error::custom)
    }
}

/// A fencing term issued by the control plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct LeaseTerm(u64);

impl LeaseTerm {
    pub const INITIAL: Self = Self(1);

    pub const fn new(raw: u64) -> Result<Self, PrimitiveError> {
        if raw == 0 {
            Err(PrimitiveError::ZeroId)
        } else {
            Ok(Self(raw))
        }
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for LeaseTerm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl TryFrom<u64> for LeaseTerm {
    type Error = PrimitiveError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl FromStr for LeaseTerm {
    type Err = PrimitiveError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<u64>()
            .map_err(|_| PrimitiveError::InvalidId(value.to_owned()))
            .and_then(Self::new)
    }
}

impl<'de> Deserialize<'de> for LeaseTerm {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = u64::deserialize(deserializer)?;
        Self::new(raw).map_err(serde::de::Error::custom)
    }
}

/// A version of a schema or persisted DTO.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SchemaVersion(u16);

impl SchemaVersion {
    pub const CURRENT: Self = Self(1);

    pub const fn new(raw: u16) -> Result<Self, PrimitiveError> {
        if raw == 0 {
            Err(PrimitiveError::InvalidSchemaVersion)
        } else {
            Ok(Self(raw))
        }
    }

    pub const fn raw(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for SchemaVersion {
    type Error = PrimitiveError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for SchemaVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = u16::deserialize(deserializer)?;
        Self::new(raw).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Additive version envelope used by DTOs at persistence and protocol edges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionEnvelope<T> {
    pub schema_version: SchemaVersion,
    pub payload: T,
}

impl<T> VersionEnvelope<T> {
    pub const fn current(payload: T) -> Self {
        Self {
            schema_version: SchemaVersion::CURRENT,
            payload,
        }
    }
}

/// An exact superdense logical timestamp ordered lexicographically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LogicalTag {
    pub tick: u64,
    pub microstep: u32,
}

impl LogicalTag {
    pub const ZERO: Self = Self {
        tick: 0,
        microstep: 0,
    };

    pub const fn new(tick: u64, microstep: u32) -> Self {
        Self { tick, microstep }
    }

    /// Advance a same-tick causal consequence to the next microstep.
    pub const fn zero_delay(self) -> Result<Self, PrimitiveError> {
        match self.microstep.checked_add(1) {
            Some(microstep) => Ok(Self {
                tick: self.tick,
                microstep,
            }),
            None => Err(PrimitiveError::LogicalTimeOverflow {
                operation: "advancing the microstep",
            }),
        }
    }

    /// Advance a positive-delay consequence to the next eligible tick.
    pub const fn positive_delay(self, delay_ticks: u64) -> Result<Self, PrimitiveError> {
        match self.tick.checked_add(delay_ticks) {
            Some(tick) if delay_ticks > 0 => Ok(Self { tick, microstep: 0 }),
            Some(_) => Err(PrimitiveError::LogicalTimeOverflow {
                operation: "advancing a positive delay",
            }),
            None => Err(PrimitiveError::LogicalTimeOverflow {
                operation: "advancing the tick",
            }),
        }
    }

    /// Apply the specification's zero-delay or positive-delay rule.
    pub const fn advance(self, delay_ticks: u64) -> Result<Self, PrimitiveError> {
        if delay_ticks == 0 {
            self.zero_delay()
        } else {
            self.positive_delay(delay_ticks)
        }
    }

    /// Move unresolved work to the next biological quantum.
    pub const fn next_quantum(self) -> Result<Self, PrimitiveError> {
        match self.tick.checked_add(1) {
            Some(tick) => Ok(Self { tick, microstep: 0 }),
            None => Err(PrimitiveError::LogicalTimeOverflow {
                operation: "deferring to the next quantum",
            }),
        }
    }

    /// Validate monotonic admission without changing the caller's state.
    pub const fn ensure_not_before(self, current: Self) -> Result<Self, PrimitiveError> {
        if self.tick < current.tick
            || (self.tick == current.tick && self.microstep < current.microstep)
        {
            Err(PrimitiveError::BackwardsTag {
                current,
                next: self,
            })
        } else {
            Ok(self)
        }
    }
}

impl fmt::Display for LogicalTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "({}, {})", self.tick, self.microstep)
    }
}

/// A generation-scoped stable-to-dense mapping for array kernels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableDenseMap<I> {
    generation: TopologyGeneration,
    dense_to_stable: Vec<I>,
    stable_to_dense: BTreeMap<I, usize>,
}

impl<I: Ord + Copy> StableDenseMap<I> {
    pub fn new(
        generation: TopologyGeneration,
        dense_to_stable: Vec<I>,
    ) -> Result<Self, PrimitiveError>
    where
        I: Into<u64>,
    {
        let mut stable_to_dense = BTreeMap::new();
        for (dense, stable) in dense_to_stable.iter().copied().enumerate() {
            if stable_to_dense.insert(stable, dense).is_some() {
                return Err(PrimitiveError::DuplicateStableId(stable.into()));
            }
        }
        Ok(Self {
            generation,
            dense_to_stable,
            stable_to_dense,
        })
    }

    pub const fn generation(&self) -> TopologyGeneration {
        self.generation
    }

    pub fn stable_to_dense(&self, stable: I) -> Option<usize> {
        self.stable_to_dense.get(&stable).copied()
    }

    pub fn dense_to_stable(&self, dense: usize) -> Result<I, PrimitiveError> {
        self.dense_to_stable
            .get(dense)
            .copied()
            .ok_or(PrimitiveError::DenseIndexOutOfBounds { index: dense })
    }

    pub const fn len(&self) -> usize {
        self.dense_to_stable.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.dense_to_stable.is_empty()
    }

    pub fn require_generation(&self, generation: TopologyGeneration) -> Result<(), PrimitiveError> {
        if generation == self.generation {
            Ok(())
        } else {
            Err(PrimitiveError::GenerationMismatch {
                expected: generation,
                actual: self.generation,
            })
        }
    }
}

/// Model stage used to make event ordering and ownership explicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum EventStage {
    SpikeDecision = 0,
    AxonalDeparture = 1,
    AxonalArrival = 2,
    SynapticTransition = 3,
    PostsynapticEffect = 4,
    PlasticityUpdate = 5,
    /// An explicit global/component field update.  This is appended to keep
    /// the wire/storage discriminants of the existing stages stable; the
    /// canonical key ordering places it before biological work at a tag.
    FieldUpdate = 6,
}

/// Stable sort key for causal events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CanonicalEventKey {
    pub tag: LogicalTag,
    pub stage: EventStage,
    pub source: u64,
    pub target: u64,
    pub event: u64,
}

impl Ord for CanonicalEventKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.tag
            .cmp(&other.tag)
            .then_with(|| stage_order(self.stage).cmp(&stage_order(other.stage)))
            .then_with(|| self.source.cmp(&other.source))
            .then_with(|| self.target.cmp(&other.target))
            .then_with(|| self.event.cmp(&other.event))
    }
}

impl PartialOrd for CanonicalEventKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn stage_order(stage: EventStage) -> u8 {
    match stage {
        // Field updates at a boundary must be visible before the spike
        // decision at that same tag.  The explicit discriminant remains
        // append-only for compatibility.
        EventStage::FieldUpdate => 0,
        EventStage::SpikeDecision => 1,
        EventStage::AxonalDeparture => 2,
        EventStage::AxonalArrival => 3,
        EventStage::SynapticTransition => 4,
        EventStage::PostsynapticEffect => 5,
        EventStage::PlasticityUpdate => 6,
    }
}

impl CanonicalEventKey {
    pub const fn new(
        tag: LogicalTag,
        stage: EventStage,
        source: u64,
        target: u64,
        event: u64,
    ) -> Self {
        Self {
            tag,
            stage,
            source,
            target,
            event,
        }
    }
}

/// A payload-bearing event used by the deterministic canonicaliser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalEvent {
    pub key: CanonicalEventKey,
    pub payload: Vec<u8>,
}

/// A compact deterministic digest. This is an ordering/replay digest, not an
/// authentication primitive; authenticated persistence uses a later phase's
/// cryptographic storage boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StateDigest(pub [u8; 16]);

impl fmt::Display for StateDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for StateDigest {
    type Err = PrimitiveError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 32 {
            return Err(PrimitiveError::InvalidId(value.to_owned()));
        }
        let mut bytes = [0u8; 16];
        for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
            let text = std::str::from_utf8(chunk)
                .map_err(|_| PrimitiveError::InvalidId(value.to_owned()))?;
            bytes[index] = u8::from_str_radix(text, 16)
                .map_err(|_| PrimitiveError::InvalidId(value.to_owned()))?;
        }
        Ok(Self(bytes))
    }
}

fn fnv1a64(bytes: &[u8], seed: u64) -> u64 {
    let mut value = 0xcbf29ce484222325u64 ^ seed;
    for byte in bytes {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x100000001b3);
    }
    value
}

fn digest_bytes(bytes: &[u8]) -> StateDigest {
    let first = fnv1a64(bytes, 0);
    let second = fnv1a64(bytes, 0x9e3779b97f4a7c15);
    let mut output = [0u8; 16];
    output[..8].copy_from_slice(&first.to_be_bytes());
    output[8..].copy_from_slice(&second.to_be_bytes());
    StateDigest(output)
}

fn append_event_key(bytes: &mut Vec<u8>, key: CanonicalEventKey) {
    bytes.extend_from_slice(&key.tag.tick.to_be_bytes());
    bytes.extend_from_slice(&key.tag.microstep.to_be_bytes());
    bytes.push(key.stage as u8);
    bytes.extend_from_slice(&key.source.to_be_bytes());
    bytes.extend_from_slice(&key.target.to_be_bytes());
    bytes.extend_from_slice(&key.event.to_be_bytes());
}

/// Sort events by the authoritative key and digest keys plus payloads.
pub fn canonical_event_digest(events: &[CanonicalEvent]) -> StateDigest {
    let mut ordered = events.to_vec();
    ordered.sort_by(|left, right| {
        left.key
            .cmp(&right.key)
            .then_with(|| left.payload.cmp(&right.payload))
    });
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"aarnn:event-digest:v1\0");
    for event in ordered {
        append_event_key(&mut bytes, event.key);
        bytes.extend_from_slice(&(event.payload.len() as u64).to_be_bytes());
        bytes.extend_from_slice(&event.payload);
    }
    digest_bytes(&bytes)
}

/// Deterministic hierarchical state digest builder.
#[derive(Debug, Default)]
pub struct StateDigestBuilder {
    domains: BTreeMap<String, Vec<u8>>,
}

impl StateDigestBuilder {
    pub fn add_domain(&mut self, domain: impl Into<String>, bytes: impl AsRef<[u8]>) {
        self.domains.insert(domain.into(), bytes.as_ref().to_vec());
    }

    pub fn finish(self) -> StateDigest {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"aarnn:state-digest:v1\0");
        for (domain, value) in self.domains {
            bytes.extend_from_slice(&(domain.len() as u64).to_be_bytes());
            bytes.extend_from_slice(domain.as_bytes());
            bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
            bytes.extend_from_slice(&value);
        }
        digest_bytes(&bytes)
    }
}

/// Coordinates for a traversal-order-independent random draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RngCoordinate {
    pub brain: BrainId,
    pub entity: u64,
    pub event: EventId,
    pub purpose: u64,
    pub draw: u64,
}

/// Counter-addressed deterministic random source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CounterRng {
    seed: u64,
    domain: u64,
}

impl CounterRng {
    pub const fn new(seed: u64, domain: u64) -> Self {
        Self { seed, domain }
    }

    pub fn draw_u64(self, coordinate: RngCoordinate) -> u64 {
        let mut bytes = Vec::with_capacity(56);
        for value in [
            self.seed,
            self.domain,
            coordinate.brain.raw(),
            coordinate.entity,
            coordinate.event.raw(),
            coordinate.purpose,
            coordinate.draw,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        splitmix64(fnv1a64(&bytes, 0x517cc1b727220a95))
    }

    pub fn uniform01(self, coordinate: RngCoordinate) -> f64 {
        (self.draw_u64(coordinate) >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

/// Signed Q32.32 deterministic-reference value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Q32_32(i64);

impl Q32_32 {
    pub const FRACTIONAL_BITS: u32 = 32;
    pub const SCALE: i128 = 1i128 << Self::FRACTIONAL_BITS;
    pub const ZERO: Self = Self(0);

    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> i64 {
        self.0
    }

    pub fn from_f64(value: f64) -> Result<Self, PrimitiveError> {
        if !value.is_finite() {
            return Err(PrimitiveError::NonFiniteNumeric);
        }
        let scaled = value * Self::SCALE as f64;
        let rounded = round_f64_ties_even(scaled)?;
        i64::try_from(rounded)
            .map(Self)
            .map_err(|_| PrimitiveError::NumericOutOfRange)
    }

    pub fn from_ratio(numerator: i64, denominator: i64) -> Result<Self, PrimitiveError> {
        if denominator <= 0 {
            return Err(PrimitiveError::InvalidDenominator);
        }
        let scaled = i128::from(numerator) * Self::SCALE;
        let raw = round_i128_ties_even(scaled, i128::from(denominator))?;
        i64::try_from(raw)
            .map(Self)
            .map_err(|_| PrimitiveError::NumericOutOfRange)
    }

    pub fn to_f64(self) -> f64 {
        self.0 as f64 / Self::SCALE as f64
    }

    pub fn checked_add(self, other: Self) -> Result<Self, PrimitiveError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(PrimitiveError::NumericOutOfRange)
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, PrimitiveError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or(PrimitiveError::NumericOutOfRange)
    }

    pub fn checked_mul(self, other: Self) -> Result<Self, PrimitiveError> {
        let product = i128::from(self.0) * i128::from(other.0);
        let raw = round_i128_ties_even(product, Self::SCALE)?;
        i64::try_from(raw)
            .map(Self)
            .map_err(|_| PrimitiveError::NumericOutOfRange)
    }
}

fn round_f64_ties_even(value: f64) -> Result<i128, PrimitiveError> {
    if !value.is_finite() {
        return Err(PrimitiveError::NonFiniteNumeric);
    }
    let lower = value.floor();
    let fraction = value - lower;
    let rounded = match fraction.partial_cmp(&0.5).unwrap_or(Ordering::Less) {
        Ordering::Less => lower,
        Ordering::Greater => lower + 1.0,
        Ordering::Equal => {
            if (lower as i128) & 1 == 0 {
                lower
            } else {
                lower + 1.0
            }
        }
    };
    if rounded < i64::MIN as f64 - 1.0 || rounded > i64::MAX as f64 + 1.0 {
        return Err(PrimitiveError::NumericOutOfRange);
    }
    Ok(rounded as i128)
}

fn round_i128_ties_even(numerator: i128, denominator: i128) -> Result<i128, PrimitiveError> {
    if denominator <= 0 {
        return Err(PrimitiveError::InvalidDenominator);
    }
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let twice_remainder = remainder
        .abs()
        .checked_mul(2)
        .ok_or(PrimitiveError::NumericOutOfRange)?;
    if twice_remainder > denominator || (twice_remainder == denominator && (quotient & 1) != 0) {
        quotient
            .checked_add(if numerator.is_negative() { -1 } else { 1 })
            .ok_or(PrimitiveError::NumericOutOfRange)
    } else {
        Ok(quotient)
    }
}

// Keeps the marker available for future typed event payload collections
// without introducing a transport dependency in this module.
#[allow(dead_code)]
type TypedCollectionMarker<T> = PhantomData<T>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_time_obeys_superdense_progression() {
        let tag = LogicalTag::new(4, 7);
        assert_eq!(tag.advance(0).unwrap(), LogicalTag::new(4, 8));
        assert_eq!(tag.advance(3).unwrap(), LogicalTag::new(7, 0));
        assert!(LogicalTag::new(4, u32::MAX).zero_delay().is_err());
    }

    #[test]
    fn q32_rounds_ties_to_even() {
        let half_lsb = 0.5 / Q32_32::SCALE as f64;
        assert_eq!(Q32_32::from_f64(half_lsb).unwrap().raw(), 0);
        assert_eq!(
            Q32_32::from_f64(1.5 / Q32_32::SCALE as f64).unwrap().raw(),
            2
        );
    }
}
