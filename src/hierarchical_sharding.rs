//! Communication-aware hierarchical sharding.
//!
//! This is the active deterministic placement planner used by the distributed
//! compatibility executor and the stable-shard adapter. It keeps biological
//! area/layer ownership separate from physical hosts while returning a
//! concrete, executable host choice for every sub-shard. A plan is
//! deterministic for the same topology, workload, measured latency matrix and
//! resource inventory.

use crate::deterministic::{NeuronId, ShardId, StateDigest, StateDigestBuilder, SubShardId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const HIERARCHICAL_SHARDING_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeuralUnit {
    pub network_id: String,
    pub area_id: u64,
    /// Stable human readable identity for the physical area. The numeric ID
    /// is used in wire identities; this label makes the placement explainable
    /// and is derived from persisted topology evidence or a deterministic
    /// topology fallback.
    #[serde(default)]
    pub area_label: String,
    pub layer: u32,
    pub neuron_id: NeuronId,
    pub work_units: u64,
    pub state_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeuralNetworkInput {
    pub network_id: String,
    /// The node from which this network normally enters the fabric. It is a
    /// placement hint only; it never grants authority or admission.
    pub home_node: String,
    pub units: Vec<NeuralUnit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkInteraction {
    pub source_network: String,
    pub target_network: String,
    pub events_per_tick: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostCapacity {
    pub node_id: String,
    pub capacity_units: u64,
    pub memory_bytes: u64,
}

/// Measured one-way or round-trip values must use one declared convention at
/// the adapter boundary. The planner only compares the integer values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatencyMatrix {
    pub nodes: Vec<String>,
    pub latency_us: BTreeMap<(String, String), u64>,
}

impl LatencyMatrix {
    pub fn new(nodes: Vec<String>, measurements: Vec<(String, String, u64)>) -> Self {
        let mut latency_us = BTreeMap::new();
        for node in &nodes {
            latency_us.insert((node.clone(), node.clone()), 0);
        }
        for (from, to, latency) in measurements {
            latency_us.insert((from, to), latency);
        }
        Self { nodes, latency_us }
    }

    pub(crate) fn latency(&self, from: &str, to: &str) -> Option<u64> {
        self.latency_us
            .get(&(from.to_owned(), to.to_owned()))
            .copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HierarchicalShardingRequest {
    pub networks: Vec<NeuralNetworkInput>,
    pub interactions: Vec<NetworkInteraction>,
    pub hosts: Vec<HostCapacity>,
    pub latency: LatencyMatrix,
    /// A layer is split into bounded sub-shards when its measured work exceeds
    /// this target. A host capacity shortage can split it further.
    pub target_sub_shard_work_units: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkGroupPlacement {
    pub group_id: String,
    pub network_ids: Vec<String>,
    pub communication_weight: u64,
    pub anchor_node: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubShardPlacement {
    pub sub_shard_id: SubShardId,
    pub parent_shard_id: ShardId,
    pub layer: u32,
    pub neuron_ids: Vec<NeuronId>,
    pub work_units: u64,
    pub state_bytes: u64,
    pub active_node: String,
    pub latency_to_group_anchor_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerPlacement {
    pub layer: u32,
    pub sub_shards: Vec<SubShardPlacement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AreaShardPlacement {
    pub shard_id: ShardId,
    pub network_id: String,
    pub group_id: String,
    pub area_id: u64,
    pub area_label: String,
    pub layers: Vec<LayerPlacement>,
    pub total_work_units: u64,
    pub total_state_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HierarchicalShardPlan {
    pub schema_version: u32,
    pub groups: Vec<NetworkGroupPlacement>,
    pub shards: Vec<AreaShardPlacement>,
    pub digest: StateDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HierarchicalShardingError {
    #[error("at least one network and host are required")]
    EmptyInput,
    #[error("network identity is empty or duplicated: {0}")]
    InvalidNetwork(String),
    #[error("network {network} has invalid home node {node}")]
    InvalidHome { network: String, node: String },
    #[error("network interaction references unknown network: {0}")]
    UnknownNetwork(String),
    #[error("neural unit {neuron} is duplicated in network {network}")]
    DuplicateNeuron { network: String, neuron: NeuronId },
    #[error("neural unit {neuron} has zero work or state in network {network}")]
    ZeroDemand { network: String, neuron: NeuronId },
    #[error("host identity is empty, duplicated, or has zero capacity: {0}")]
    InvalidHost(String),
    #[error("target sub-shard work must be non-zero")]
    InvalidTarget,
    #[error("missing measured latency from {from} to {to}")]
    MissingLatency { from: String, to: String },
    #[error("sub-shard {sub_shard} cannot fit on any latency-reachable host")]
    NoCapacity { sub_shard: SubShardId },
    #[error("stable shard identity collision for key {0}")]
    IdentityCollision(String),
    #[error("numeric overflow while building hierarchical shard plan")]
    Overflow,
}

#[derive(Default)]
struct DisjointSet {
    parent: BTreeMap<String, String>,
}

impl DisjointSet {
    fn add(&mut self, value: &str) {
        self.parent
            .entry(value.to_owned())
            .or_insert_with(|| value.to_owned());
    }

    fn find(&mut self, value: &str) -> String {
        let parent = self.parent.get(value).cloned().expect("value was added");
        if parent == value {
            parent
        } else {
            let root = self.find(&parent);
            self.parent.insert(value.to_owned(), root.clone());
            root
        }
    }

    fn union(&mut self, left: &str, right: &str) {
        let left_root = self.find(left);
        let right_root = self.find(right);
        if left_root != right_root {
            let (small, large) = if left_root < right_root {
                (left_root, right_root)
            } else {
                (right_root, left_root)
            };
            self.parent.insert(small, large);
        }
    }
}

fn stable_id(material: &str) -> u64 {
    // FNV-1a gives stable IDs without depending on a random process hash.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in material.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash.max(1)
}

/// A persisted physical/topological observation for one biological neuron.
/// `index` is only a deterministic fallback identity; when coordinates or a
/// region are present they are preferred for area assignment.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicalNeuronLocation {
    pub layer: u32,
    pub index: u64,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub z: Option<f32>,
    pub region_label: Option<String>,
}

/// Return the stable numeric identity used by area shards and their wire
/// projections. This deliberately uses the canonical, deterministic hash
/// above rather than a process-random hash.
pub fn stable_area_id(area_label: &str) -> u64 {
    stable_id(area_label)
}

fn normalized_region_label(raw: &str) -> Option<String> {
    let label = raw.trim().to_ascii_lowercase();
    (!label.is_empty()).then_some(label)
}

fn spatial_area_label(x: f32, y: f32, z: f32) -> String {
    // Eight bins per axis cover the normalised growth volume. Quantising the
    // persisted position keeps nearby newly grown cells in their parent's
    // area while making imports and replay independent of input order.
    fn bin(value: f32) -> i32 {
        if !value.is_finite() {
            return 0;
        }
        (((value.clamp(-1.0, 1.0) + 1.0) * 4.0).floor() as i32).clamp(0, 7)
    }
    format!("spatial:x{}:y{}:z{}", bin(x), bin(y), bin(z))
}

/// Build deterministic area/layer units from persisted physical observations.
///
/// The returned unit is an executable planning slice, not a biological state
/// mutation. Explicit region labels win, then persisted coordinates, then a
/// fixed topology block. The last fallback is intentionally append-stable:
/// adding neurons to a grown network does not relabel earlier blocks.
pub fn area_layer_units(
    network_id: &str,
    layer_counts: &BTreeMap<u32, u64>,
    locations: &[PhysicalNeuronLocation],
    state_bytes_per_neuron: u64,
) -> Vec<NeuralUnit> {
    let mut locations_by_neuron = BTreeMap::<(u32, u64), &PhysicalNeuronLocation>::new();
    for location in locations {
        locations_by_neuron.insert((location.layer, location.index), location);
    }
    let mut aggregates = BTreeMap::<(u64, u32), (String, u64)>::new();
    for (&layer, &count) in layer_counts {
        for index in 0..count {
            let label = locations_by_neuron
                .get(&(layer, index))
                .and_then(|location| {
                    location
                        .region_label
                        .as_deref()
                        .and_then(normalized_region_label)
                        .or_else(|| match (location.x, location.y, location.z) {
                            (Some(x), Some(y), Some(z))
                                if x.is_finite() && y.is_finite() && z.is_finite() =>
                            {
                                Some(spatial_area_label(x, y, z))
                            }
                            _ => None,
                        })
                })
                .unwrap_or_else(|| format!("topology:l{layer}:block{}", index / 32));
            let area_id = stable_area_id(&label);
            let entry = aggregates.entry((area_id, layer)).or_insert((label, 0));
            entry.1 = entry.1.saturating_add(1);
        }
    }

    aggregates
        .into_iter()
        .map(|((area_id, layer), (area_label, count))| NeuralUnit {
            network_id: network_id.to_owned(),
            area_id,
            area_label,
            layer,
            // A planning unit represents the area/layer slice. Its stable ID
            // is independent of the order in which source neurons arrived.
            neuron_id: NeuronId::new(stable_id(&format!(
                "{network_id}:area:{area_id}:layer:{layer}"
            )))
            .expect("stable area/layer ID is non-zero"),
            work_units: count.max(1),
            state_bytes: count.max(1).saturating_mul(state_bytes_per_neuron.max(1)),
        })
        .collect()
}

fn communication_key(left: &str, right: &str) -> (String, String) {
    if left <= right {
        (left.to_owned(), right.to_owned())
    } else {
        (right.to_owned(), left.to_owned())
    }
}

fn add_u64(left: &mut u64, right: u64) -> Result<(), HierarchicalShardingError> {
    *left = left
        .checked_add(right)
        .ok_or(HierarchicalShardingError::Overflow)?;
    Ok(())
}

pub fn plan_hierarchical_shards(
    mut request: HierarchicalShardingRequest,
) -> Result<HierarchicalShardPlan, HierarchicalShardingError> {
    if request.networks.is_empty() || request.hosts.is_empty() {
        return Err(HierarchicalShardingError::EmptyInput);
    }
    if request.target_sub_shard_work_units == 0 {
        return Err(HierarchicalShardingError::InvalidTarget);
    }

    request
        .networks
        .sort_by(|left, right| left.network_id.cmp(&right.network_id));
    request
        .hosts
        .sort_by(|left, right| left.node_id.cmp(&right.node_id));
    let mut networks = BTreeMap::new();
    for network in request.networks {
        if network.network_id.trim().is_empty() || networks.contains_key(&network.network_id) {
            return Err(HierarchicalShardingError::InvalidNetwork(
                network.network_id,
            ));
        }
        if network.home_node.trim().is_empty() {
            return Err(HierarchicalShardingError::InvalidHome {
                network: network.network_id,
                node: network.home_node,
            });
        }
        if !request
            .latency
            .nodes
            .iter()
            .any(|node| node == &network.home_node)
        {
            return Err(HierarchicalShardingError::InvalidHome {
                network: network.network_id,
                node: network.home_node,
            });
        }
        let mut units = network.units;
        units.sort_by_key(|unit| (unit.area_id, unit.layer, unit.neuron_id));
        let mut neurons = BTreeSet::new();
        for unit in &units {
            if unit.network_id != network.network_id {
                return Err(HierarchicalShardingError::InvalidNetwork(
                    unit.network_id.clone(),
                ));
            }
            if !neurons.insert(unit.neuron_id) {
                return Err(HierarchicalShardingError::DuplicateNeuron {
                    network: network.network_id.clone(),
                    neuron: unit.neuron_id,
                });
            }
            if unit.work_units == 0 || unit.state_bytes == 0 {
                return Err(HierarchicalShardingError::ZeroDemand {
                    network: network.network_id.clone(),
                    neuron: unit.neuron_id,
                });
            }
        }
        networks.insert(network.network_id.clone(), (network.home_node, units));
    }

    let mut hosts = BTreeMap::new();
    for host in request.hosts {
        if host.node_id.trim().is_empty()
            || host.capacity_units == 0
            || host.memory_bytes == 0
            || hosts.insert(host.node_id.clone(), host).is_some()
        {
            return Err(HierarchicalShardingError::InvalidHost(
                "host identity or capacity".to_owned(),
            ));
        }
    }
    for node in hosts.keys() {
        if !request.latency.nodes.contains(node) {
            return Err(HierarchicalShardingError::MissingLatency {
                from: node.clone(),
                to: node.clone(),
            });
        }
    }
    for network in networks.values() {
        if !hosts.contains_key(&network.0) {
            return Err(HierarchicalShardingError::InvalidHome {
                network: "unknown".to_owned(),
                node: network.0.clone(),
            });
        }
    }

    let network_ids = networks.keys().cloned().collect::<BTreeSet<_>>();
    let mut interactions = BTreeMap::<(String, String), u64>::new();
    let mut dsu = DisjointSet::default();
    for network in &network_ids {
        dsu.add(network);
    }
    for interaction in request.interactions {
        if !network_ids.contains(&interaction.source_network) {
            return Err(HierarchicalShardingError::UnknownNetwork(
                interaction.source_network,
            ));
        }
        if !network_ids.contains(&interaction.target_network) {
            return Err(HierarchicalShardingError::UnknownNetwork(
                interaction.target_network,
            ));
        }
        if interaction.source_network != interaction.target_network
            && interaction.events_per_tick > 0
        {
            dsu.union(&interaction.source_network, &interaction.target_network);
        }
        let key = communication_key(&interaction.source_network, &interaction.target_network);
        let value = interactions.entry(key).or_default();
        add_u64(value, interaction.events_per_tick)?;
    }

    let mut group_members = BTreeMap::<String, Vec<String>>::new();
    for network in &network_ids {
        let root = dsu.find(network);
        group_members.entry(root).or_default().push(network.clone());
    }
    for members in group_members.values_mut() {
        members.sort();
    }

    let mut groups = Vec::new();
    let mut group_by_network = BTreeMap::new();
    let mut used_group_ids = BTreeSet::new();
    for members in group_members.values() {
        let group_id = format!("group-{:016x}", stable_id(&members.join("|")));
        if !used_group_ids.insert(group_id.clone()) {
            return Err(HierarchicalShardingError::IdentityCollision(group_id));
        }
        let mut weight = 0;
        for (left, right) in interactions.keys() {
            if members.binary_search(left).is_ok() && members.binary_search(right).is_ok() {
                add_u64(&mut weight, interactions[&(left.clone(), right.clone())])?;
            }
        }
        let mut best: Option<(u128, String)> = None;
        for candidate in hosts.keys() {
            let mut cost = 0u128;
            for network in members {
                let home = &networks[network].0;
                let incident = interactions
                    .iter()
                    .filter(|((left, right), _)| left == network || right == network)
                    .map(|(_, value)| *value)
                    .sum::<u64>()
                    .max(1);
                let latency = request.latency.latency(home, candidate).ok_or_else(|| {
                    HierarchicalShardingError::MissingLatency {
                        from: home.clone(),
                        to: candidate.clone(),
                    }
                })?;
                cost = cost
                    .checked_add(u128::from(latency) * u128::from(incident))
                    .ok_or(HierarchicalShardingError::Overflow)?;
            }
            let candidate_key = (cost, candidate.clone());
            if best.as_ref().is_none_or(|current| candidate_key < *current) {
                best = Some(candidate_key);
            }
        }
        let anchor_node = best.expect("hosts is non-empty").1;
        let placement = NetworkGroupPlacement {
            group_id: group_id.clone(),
            network_ids: members.clone(),
            communication_weight: weight,
            anchor_node,
        };
        for network in members {
            group_by_network.insert(network.clone(), group_id.clone());
        }
        groups.push(placement);
    }
    groups.sort_by_key(|group| group.group_id.clone());

    let mut used_capacity = BTreeMap::<String, (u64, u64)>::new();
    let mut used_shard_ids = BTreeSet::new();
    let mut used_sub_shard_ids = BTreeSet::new();
    let mut shards = Vec::new();
    for (network_id, (_, units)) in &networks {
        let group_id = group_by_network[network_id].clone();
        let anchor = groups
            .iter()
            .find(|group| group.group_id == group_id)
            .expect("group exists")
            .anchor_node
            .clone();
        let mut by_area = BTreeMap::<u64, BTreeMap<u32, Vec<&NeuralUnit>>>::new();
        for unit in units {
            by_area
                .entry(unit.area_id)
                .or_default()
                .entry(unit.layer)
                .or_default()
                .push(unit);
        }
        for (area_id, by_layer) in by_area {
            let area_label = by_layer
                .values()
                .flat_map(|units| units.iter())
                .map(|unit| unit.area_label.as_str())
                .next()
                .unwrap_or("topology:unlabelled")
                .to_owned();
            let shard_key = format!("{network_id}:area:{area_id}");
            let shard_id = ShardId::new(stable_id(&shard_key))
                .map_err(|_| HierarchicalShardingError::IdentityCollision(shard_key.clone()))?;
            if !used_shard_ids.insert(shard_id) {
                return Err(HierarchicalShardingError::IdentityCollision(shard_key));
            }
            let mut total_work = 0;
            let mut total_state = 0;
            let mut layers = Vec::new();
            for (layer, layer_units) in by_layer {
                let mut chunks: Vec<Vec<&NeuralUnit>> = Vec::new();
                for unit in layer_units {
                    let start_new = chunks.last().is_none_or(|chunk| {
                        let work = chunk.iter().map(|item| item.work_units).sum::<u64>();
                        work > 0
                            && work
                                .checked_add(unit.work_units)
                                .is_none_or(|next| next > request.target_sub_shard_work_units)
                    });
                    if start_new {
                        chunks.push(Vec::new());
                    }
                    chunks.last_mut().expect("chunk exists").push(unit);
                }
                let mut sub_shards = Vec::new();
                for (chunk_index, chunk) in chunks.into_iter().enumerate() {
                    let mut work = 0;
                    let mut state = 0;
                    let mut neuron_ids = Vec::new();
                    for unit in chunk {
                        add_u64(&mut work, unit.work_units)?;
                        add_u64(&mut state, unit.state_bytes)?;
                        neuron_ids.push(unit.neuron_id);
                    }
                    let sub_key =
                        format!("{network_id}:area:{area_id}:layer:{layer}:part:{chunk_index}");
                    let sub_shard_id = SubShardId::new(stable_id(&sub_key)).map_err(|_| {
                        HierarchicalShardingError::IdentityCollision(sub_key.clone())
                    })?;
                    if !used_sub_shard_ids.insert(sub_shard_id) {
                        return Err(HierarchicalShardingError::IdentityCollision(sub_key));
                    }
                    let active_node = choose_host(
                        &request.latency,
                        &hosts,
                        &mut used_capacity,
                        &anchor,
                        sub_shard_id,
                        work,
                        state,
                    )?;
                    let latency_to_group_anchor_us = request
                        .latency
                        .latency(&anchor, &active_node)
                        .ok_or_else(|| HierarchicalShardingError::MissingLatency {
                            from: anchor.clone(),
                            to: active_node.clone(),
                        })?;
                    add_u64(&mut total_work, work)?;
                    add_u64(&mut total_state, state)?;
                    sub_shards.push(SubShardPlacement {
                        sub_shard_id,
                        parent_shard_id: shard_id,
                        layer,
                        neuron_ids,
                        work_units: work,
                        state_bytes: state,
                        active_node,
                        latency_to_group_anchor_us,
                    });
                }
                layers.push(LayerPlacement { layer, sub_shards });
            }
            shards.push(AreaShardPlacement {
                shard_id,
                network_id: network_id.clone(),
                group_id: group_id.clone(),
                area_id,
                area_label,
                layers,
                total_work_units: total_work,
                total_state_bytes: total_state,
            });
        }
    }
    shards.sort_by_key(|shard| shard.shard_id);
    let mut plan = HierarchicalShardPlan {
        schema_version: HIERARCHICAL_SHARDING_SCHEMA_VERSION,
        groups,
        shards,
        digest: StateDigest([0; 16]),
    };
    plan.digest = plan.calculate_digest();
    Ok(plan)
}

fn choose_host(
    latency: &LatencyMatrix,
    hosts: &BTreeMap<String, HostCapacity>,
    used: &mut BTreeMap<String, (u64, u64)>,
    anchor: &str,
    sub_shard_id: SubShardId,
    work: u64,
    state: u64,
) -> Result<String, HierarchicalShardingError> {
    let mut candidates = Vec::<(u64, u64, u64, String)>::new();
    for (node, host) in hosts {
        let Some(network_latency) = latency.latency(anchor, node) else {
            return Err(HierarchicalShardingError::MissingLatency {
                from: anchor.to_owned(),
                to: node.clone(),
            });
        };
        let (used_work, used_state) = used.get(node).copied().unwrap_or_default();
        if used_work.saturating_add(work) <= host.capacity_units
            && used_state.saturating_add(state) <= host.memory_bytes
        {
            // Latency is the primary placement key. Remaining headroom makes
            // equal-latency choices deterministic without overriding locality.
            candidates.push((
                network_latency,
                used_work.saturating_add(work),
                used_state.saturating_add(state),
                node.clone(),
            ));
        }
    }
    candidates.sort();
    let Some((_, _, _, node)) = candidates.into_iter().next() else {
        return Err(HierarchicalShardingError::NoCapacity {
            sub_shard: sub_shard_id,
        });
    };
    let entry = used.entry(node.clone()).or_default();
    entry.0 = entry
        .0
        .checked_add(work)
        .ok_or(HierarchicalShardingError::Overflow)?;
    entry.1 = entry
        .1
        .checked_add(state)
        .ok_or(HierarchicalShardingError::Overflow)?;
    Ok(node)
}

impl HierarchicalShardPlan {
    fn calculate_digest(&self) -> StateDigest {
        let mut material = self.clone();
        material.digest = StateDigest([0; 16]);
        let bytes = serde_json::to_vec(&material).expect("hierarchical plan is serialisable");
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("hierarchical-sharding:v1", bytes);
        digest.finish()
    }

    pub fn verify(&self) -> Result<(), HierarchicalShardingError> {
        if self.schema_version != HIERARCHICAL_SHARDING_SCHEMA_VERSION {
            return Err(HierarchicalShardingError::InvalidTarget);
        }
        let expected = self.calculate_digest();
        if expected != self.digest {
            return Err(HierarchicalShardingError::IdentityCollision(
                "plan digest mismatch".to_owned(),
            ));
        }
        let mut shard_ids = BTreeSet::new();
        let mut sub_ids = BTreeSet::new();
        for shard in &self.shards {
            if !shard_ids.insert(shard.shard_id) {
                return Err(HierarchicalShardingError::IdentityCollision(
                    "duplicate shard".to_owned(),
                ));
            }
            for layer in &shard.layers {
                for sub_shard in &layer.sub_shards {
                    if !sub_ids.insert(sub_shard.sub_shard_id)
                        || sub_shard.parent_shard_id != shard.shard_id
                        || sub_shard.layer != layer.layer
                    {
                        return Err(HierarchicalShardingError::IdentityCollision(
                            "invalid sub-shard ownership".to_owned(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn sub_shard_count(&self) -> usize {
        self.shards
            .iter()
            .flat_map(|shard| &shard.layers)
            .map(|layer| layer.sub_shards.len())
            .sum()
    }
}
