//! Bounded physical placement proposals for explicitly imported SNNs.
//! Proposals preserve the existing brain and do not grant mutation authority.

use super::{
    ConnectomeRole, MorphologyError, PointOnlyConnectome, PointOnlyNeuron, Vec3, insert_soma,
    nearby_somas,
};
use crate::deterministic::{
    BrainId, NeuronId, StateDigest, StateDigestBuilder, TopologyGeneration,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeInputPlacementPolicy {
    #[default]
    SensoryNearExistingOutputs,
}

/// Distances are caller-calibrated millimetres, never renderer coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentPolicy {
    pub maximum_contact_distance_mm: f64,
    pub minimum_soma_separation_mm: f64,
    pub maximum_neuron_records: usize,
    pub maximum_candidates: usize,
    /// Bounds distance calculations and neuron visits, including empty cells.
    pub maximum_distance_checks: usize,
}

pub struct ExistingSnn<'a> {
    pub brain_id: BrainId,
    pub topology_generation: TopologyGeneration,
    pub connectome: &'a PointOnlyConnectome,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SensoryContact {
    pub sensory: NeuronId,
    pub existing_output: NeuronId,
    pub distance_mm: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeAttachmentPlan {
    pub schema_version: u16,
    pub brain_id: BrainId,
    pub topology_generation: TopologyGeneration,
    pub target_digest: StateDigest,
    pub source_digest: StateDigest,
    pub policy: AttachmentPolicy,
    pub translation_mm: Vec3,
    pub sensory_contacts: Vec<SensoryContact>,
}

#[derive(Debug, Error)]
pub enum AttachmentError {
    #[error("invalid calibrated placement policy")]
    InvalidPolicy,
    #[error("an existing output and an imported sensory neuron are required")]
    MissingInterface,
    #[error("the imported interface does not match the converted mesh")]
    InterfaceMismatch,
    #[error("imported identities collide with the existing brain")]
    IdentityCollision,
    #[error("placement work or memory budget is exhausted")]
    BudgetExceeded,
    #[error("the imported SNN has overlapping somas")]
    SourceOverlap,
    #[error("no rigid placement satisfies sensory proximity and soma clearance")]
    NoPlacement,
    #[error("the attachment proposal is stale, changed or malformed")]
    StalePlan,
    #[error(transparent)]
    Geometry(#[from] MorphologyError),
}

struct Budget(usize);
impl Budget {
    fn visit(&mut self) -> Result<(), AttachmentError> {
        self.0 = self
            .0
            .checked_sub(1)
            .ok_or(AttachmentError::BudgetExceeded)?;
        Ok(())
    }
    fn distance(&mut self, a: Vec3, b: Vec3) -> Result<f64, AttachmentError> {
        self.visit()?;
        Ok(a.distance(b))
    }
}

fn validate_inputs(
    existing: &PointOnlyConnectome,
    incoming: &PointOnlyConnectome,
    policy: &AttachmentPolicy,
) -> Result<(), AttachmentError> {
    if !policy.maximum_contact_distance_mm.is_finite()
        || !policy.minimum_soma_separation_mm.is_finite()
        || policy.minimum_soma_separation_mm <= 0.0
        || policy.maximum_contact_distance_mm <= policy.minimum_soma_separation_mm
        || !(1..=1_000_000).contains(&policy.maximum_neuron_records)
        || !(1..=4096).contains(&policy.maximum_candidates)
        || !(1..=10_000_000).contains(&policy.maximum_distance_checks)
    {
        return Err(AttachmentError::InvalidPolicy);
    }
    if existing
        .neurons
        .len()
        .saturating_add(incoming.neurons.len())
        > policy.maximum_neuron_records
        || existing
            .connections
            .len()
            .saturating_add(incoming.connections.len())
            > 1_000_000
    {
        return Err(AttachmentError::BudgetExceeded);
    }
    existing.validate()?;
    incoming.validate()?;
    if existing
        .connections
        .iter()
        .chain(&incoming.connections)
        .any(|c| c.kind.len() > 256)
    {
        return Err(AttachmentError::Geometry(
            MorphologyError::InvalidConnectome,
        ));
    }
    let ids: std::collections::BTreeSet<_> = existing.neurons.iter().map(|n| n.id).collect();
    let edges: std::collections::BTreeSet<_> = existing.connections.iter().map(|c| c.id).collect();
    if incoming.neurons.iter().any(|n| ids.contains(&n.id))
        || incoming.connections.iter().any(|c| edges.contains(&c.id))
    {
        return Err(AttachmentError::IdentityCollision);
    }
    Ok(())
}

fn digest(connectome: &PointOnlyConnectome) -> Result<StateDigest, AttachmentError> {
    let mut canonical = connectome.clone();
    canonical.neurons.sort_by_key(|n| n.id);
    canonical.connections.sort_by_key(|c| c.id);
    let mut builder = StateDigestBuilder::default();
    builder.add_domain(
        "knowledge-import-connectome:v1",
        serde_json::to_vec(&canonical).map_err(|_| AttachmentError::StalePlan)?,
    );
    Ok(builder.finish())
}

type SomaIndex = BTreeMap<(i64, i64, i64), Vec<(NeuronId, Vec3)>>;
fn index(connectome: &PointOnlyConnectome, width: f64) -> Result<SomaIndex, AttachmentError> {
    let mut result = BTreeMap::new();
    for neuron in &connectome.neurons {
        insert_soma(&mut result, neuron.id, neuron.position_mm, width)?;
    }
    Ok(result)
}

fn check_source(
    ordered: &[&PointOnlyNeuron],
    policy: &AttachmentPolicy,
    budget: &mut Budget,
) -> Result<(), AttachmentError> {
    let mut source = BTreeMap::new();
    for neuron in ordered {
        budget.visit()?;
        for (_, other) in nearby_somas(
            &source,
            neuron.position_mm,
            policy.minimum_soma_separation_mm,
        )? {
            if budget.distance(neuron.position_mm, other)? < policy.minimum_soma_separation_mm {
                return Err(AttachmentError::SourceOverlap);
            }
        }
        insert_soma(
            &mut source,
            neuron.id,
            neuron.position_mm,
            policy.minimum_soma_separation_mm,
        )?;
    }
    Ok(())
}

fn contacts(
    ordered: &[&PointOnlyNeuron],
    translation: Vec3,
    policy: &AttachmentPolicy,
    occupied: &SomaIndex,
    outputs: &SomaIndex,
    budget: &mut Budget,
) -> Result<Option<Vec<SensoryContact>>, AttachmentError> {
    if !translation.is_finite() {
        return Ok(None);
    }
    let Some(reference) = ordered.first() else {
        return Ok(None);
    };
    let translated_reference = reference.position_mm.add(translation);
    let mut witnesses = Vec::new();
    let mut translated_source = BTreeMap::new();
    for neuron in ordered {
        budget.visit()?;
        let position = neuron.position_mm.add(translation);
        if !position.is_finite() {
            return Ok(None);
        }
        // Bound floating-point loss by source-relative geometry, never by the
        // magnitude of world coordinates. Large translations must not collapse
        // or warp a fine sensory layer. This is arithmetic, not model tolerance.
        let original = neuron.position_mm.sub(reference.position_mm);
        let translated = position.sub(translated_reference);
        for (a, b) in [
            (original.x, translated.x),
            (original.y, translated.y),
            (original.z, translated.z),
        ] {
            if !a.is_finite()
                || !b.is_finite()
                || (a - b).abs()
                    > 64.0 * f64::EPSILON * a.abs().max(policy.minimum_soma_separation_mm)
            {
                return Ok(None);
            }
        }
        for (_, other) in nearby_somas(
            &translated_source,
            position,
            policy.minimum_soma_separation_mm,
        )? {
            if budget.distance(position, other)? < policy.minimum_soma_separation_mm {
                return Ok(None);
            }
        }
        insert_soma(
            &mut translated_source,
            neuron.id,
            position,
            policy.minimum_soma_separation_mm,
        )?;
        for (_, other) in nearby_somas(occupied, position, policy.minimum_soma_separation_mm)? {
            if budget.distance(position, other)? < policy.minimum_soma_separation_mm {
                return Ok(None);
            }
        }
        if neuron.role == ConnectomeRole::Sensory {
            let mut nearest = None;
            for (id, point) in nearby_somas(outputs, position, policy.maximum_contact_distance_mm)?
            {
                let distance = budget.distance(position, point)?;
                if distance <= policy.maximum_contact_distance_mm
                    && nearest.as_ref().is_none_or(|previous: &SensoryContact| {
                        distance.total_cmp(&previous.distance_mm).is_lt()
                            || (distance == previous.distance_mm && id < previous.existing_output)
                    })
                {
                    nearest = Some(SensoryContact {
                        sensory: neuron.id,
                        existing_output: id,
                        distance_mm: distance,
                    });
                }
            }
            let Some(witness) = nearest else {
                return Ok(None);
            };
            witnesses.push(witness);
        }
    }
    Ok(Some(witnesses))
}

/// Plan a rigid translation; every sensory input must meet the declared bound.
/// Missing anchors, overlap or exhausted search budgets refuse the import.
/// Searches six axis directions about stable output anchors; this deliberately
/// bounded search may refuse a model that another rigid placement could fit.
pub fn plan_attachment(
    target: ExistingSnn<'_>,
    incoming: &PointOnlyConnectome,
    policy: AttachmentPolicy,
) -> Result<KnowledgeAttachmentPlan, AttachmentError> {
    validate_inputs(target.connectome, incoming, &policy)?;
    let first_sensory = incoming
        .neurons
        .iter()
        .filter(|n| n.role == ConnectomeRole::Sensory)
        .min_by_key(|n| n.id)
        .ok_or(AttachmentError::MissingInterface)?;
    let mut anchors: Vec<_> = target
        .connectome
        .neurons
        .iter()
        .filter(|n| n.role == ConnectomeRole::Output)
        .collect();
    anchors.sort_by_key(|n| n.id);
    if anchors.is_empty() {
        return Err(AttachmentError::MissingInterface);
    }
    let occupied = index(target.connectome, policy.minimum_soma_separation_mm)?;
    let mut outputs = BTreeMap::new();
    for output in &anchors {
        insert_soma(
            &mut outputs,
            output.id,
            output.position_mm,
            policy.maximum_contact_distance_mm,
        )?;
    }
    let mut budget = Budget(policy.maximum_distance_checks);
    let mut ordered: Vec<_> = incoming.neurons.iter().collect();
    ordered.sort_by_key(|n| n.id);
    check_source(&ordered, &policy, &mut budget)?;
    let gap = policy.minimum_soma_separation_mm * 0.5 + policy.maximum_contact_distance_mm * 0.5;
    let directions = [
        Vec3 {
            x: 1.,
            y: 0.,
            z: 0.,
        },
        Vec3 {
            x: -1.,
            y: 0.,
            z: 0.,
        },
        Vec3 {
            x: 0.,
            y: 1.,
            z: 0.,
        },
        Vec3 {
            x: 0.,
            y: -1.,
            z: 0.,
        },
        Vec3 {
            x: 0.,
            y: 0.,
            z: 1.,
        },
        Vec3 {
            x: 0.,
            y: 0.,
            z: -1.,
        },
    ];
    let mut attempts = 0;
    for anchor in anchors {
        for direction in directions {
            if attempts >= policy.maximum_candidates {
                return Err(AttachmentError::BudgetExceeded);
            }
            attempts += 1;
            let translation = anchor
                .position_mm
                .add(direction.scale(gap))
                .sub(first_sensory.position_mm);
            if let Some(sensory_contacts) = contacts(
                &ordered,
                translation,
                &policy,
                &occupied,
                &outputs,
                &mut budget,
            )? {
                return Ok(KnowledgeAttachmentPlan {
                    schema_version: 1,
                    brain_id: target.brain_id,
                    topology_generation: target.topology_generation,
                    target_digest: digest(target.connectome)?,
                    source_digest: digest(incoming)?,
                    policy,
                    translation_mm: translation,
                    sensory_contacts,
                });
            }
        }
    }
    Err(AttachmentError::NoPlacement)
}

impl KnowledgeAttachmentPlan {
    /// Recheck fresh geometry/policy and prepare data for the authorised topology
    /// transaction. This does not commit ownership, add cross-links or run neurons.
    pub fn prepare_connectome(
        &self,
        target: ExistingSnn<'_>,
        incoming: &PointOnlyConnectome,
        policy: &AttachmentPolicy,
    ) -> Result<PointOnlyConnectome, AttachmentError> {
        validate_inputs(target.connectome, incoming, policy)?;
        if self.schema_version != 1
            || self.brain_id != target.brain_id
            || self.topology_generation != target.topology_generation
            || &self.policy != policy
            || self.target_digest != digest(target.connectome)?
            || self.source_digest != digest(incoming)?
        {
            return Err(AttachmentError::StalePlan);
        }
        let occupied = index(target.connectome, policy.minimum_soma_separation_mm)?;
        let mut outputs = BTreeMap::new();
        for n in target
            .connectome
            .neurons
            .iter()
            .filter(|n| n.role == ConnectomeRole::Output)
        {
            insert_soma(
                &mut outputs,
                n.id,
                n.position_mm,
                policy.maximum_contact_distance_mm,
            )?;
        }
        if self.sensory_contacts.len() > incoming.neurons.len() {
            return Err(AttachmentError::StalePlan);
        }
        let mut ordered: Vec<_> = incoming.neurons.iter().collect();
        ordered.sort_by_key(|n| n.id);
        let mut budget = Budget(policy.maximum_distance_checks);
        check_source(&ordered, policy, &mut budget)?;
        let actual = contacts(
            &ordered,
            self.translation_mm,
            policy,
            &occupied,
            &outputs,
            &mut budget,
        )?
        .ok_or(AttachmentError::StalePlan)?;
        if actual.is_empty() || actual != self.sensory_contacts {
            return Err(AttachmentError::StalePlan);
        }
        let mut combined = target.connectome.clone();
        combined
            .neurons
            .extend(incoming.neurons.iter().cloned().map(|mut n| {
                n.position_mm = n.position_mm.add(self.translation_mm);
                n
            }));
        combined
            .connections
            .extend(incoming.connections.iter().cloned());
        combined.validate()?;
        Ok(combined)
    }
}

#[cfg(test)]
mod tests;
