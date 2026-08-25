//! Independent-brain admission and deterministic fair dispatch.

use crate::deterministic::{BrainId, LogicalTag, ShardId};
use std::collections::{BTreeMap, VecDeque};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    pub brain: BrainId,
    pub tag: LogicalTag,
    pub sequence: u64,
    pub cost: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SchedulerError {
    #[error("brain {0} is not admitted")]
    UnknownBrain(BrainId),
    #[error("brain {brain} queue capacity {capacity} exceeded")]
    BrainQueueFull { brain: BrainId, capacity: usize },
    #[error("global scheduler capacity {capacity} exceeded")]
    GlobalQueueFull { capacity: usize },
    #[error("resource request cannot be placed")]
    NoPlacement,
    #[error("brain work sequence exhausted")]
    SequenceOverflow,
}

#[derive(Debug, Clone)]
struct BrainQueue {
    capacity: usize,
    pending: BTreeMap<LogicalTag, VecDeque<WorkItem>>,
    queued: usize,
    next_sequence: u64,
}

#[derive(Debug, Clone)]
pub struct FairScheduler {
    brains: BTreeMap<BrainId, BrainQueue>,
    order: Vec<BrainId>,
    cursor: usize,
    global_capacity: usize,
    queued: usize,
}

impl FairScheduler {
    pub fn new(global_capacity: usize) -> Self {
        Self {
            brains: BTreeMap::new(),
            order: Vec::new(),
            cursor: 0,
            global_capacity,
            queued: 0,
        }
    }

    pub fn admit_brain(&mut self, brain: BrainId, capacity: usize) {
        if self
            .brains
            .insert(
                brain,
                BrainQueue {
                    capacity,
                    pending: BTreeMap::new(),
                    queued: 0,
                    next_sequence: 0,
                },
            )
            .is_none()
        {
            self.order.push(brain);
            self.order.sort_unstable();
        }
    }

    pub fn admit(
        &mut self,
        brain: BrainId,
        tag: LogicalTag,
        cost: u32,
    ) -> Result<u64, SchedulerError> {
        if self.queued >= self.global_capacity {
            return Err(SchedulerError::GlobalQueueFull {
                capacity: self.global_capacity,
            });
        }
        let queue = self
            .brains
            .get_mut(&brain)
            .ok_or(SchedulerError::UnknownBrain(brain))?;
        if queue.queued >= queue.capacity {
            return Err(SchedulerError::BrainQueueFull {
                brain,
                capacity: queue.capacity,
            });
        }
        let sequence = queue.next_sequence;
        queue.next_sequence = queue
            .next_sequence
            .checked_add(1)
            .ok_or(SchedulerError::SequenceOverflow)?;
        queue.pending.entry(tag).or_default().push_back(WorkItem {
            brain,
            tag,
            sequence,
            cost,
        });
        queue.queued += 1;
        self.queued += 1;
        Ok(sequence)
    }

    /// Dispatch at most one item. Brain queues rotate independently and no
    /// brain-wide/tick-wide barrier is consulted.
    pub fn dispatch_one(&mut self) -> Option<WorkItem> {
        if self.order.is_empty() {
            return None;
        }
        for offset in 0..self.order.len() {
            let index = (self.cursor + offset) % self.order.len();
            let brain = self.order[index];
            let queue = self.brains.get_mut(&brain)?;
            let tag = queue.pending.keys().next().copied();
            if let Some(tag) = tag {
                let item = queue.pending.get_mut(&tag).and_then(VecDeque::pop_front);
                if queue.pending.get(&tag).is_some_and(VecDeque::is_empty) {
                    queue.pending.remove(&tag);
                }
                if let Some(item) = item {
                    queue.queued -= 1;
                    self.queued -= 1;
                    self.cursor = (index + 1) % self.order.len();
                    return Some(item);
                }
            }
        }
        None
    }

    pub const fn queued(&self) -> usize {
        self.queued
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceInventory {
    pub cpu_cores: u32,
    pub memory_bytes: u64,
    pub gpu_memory_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementRequest {
    pub brain: BrainId,
    pub cpu_cores: u32,
    pub memory_bytes: u64,
    pub gpu_memory_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementDecision {
    pub brain: BrainId,
    pub shard: ShardId,
    pub explanation: String,
}

/// Stable first-fit placement with explicit resource checks.
pub fn choose_placement(
    request: &PlacementRequest,
    candidates: &[(ShardId, ResourceInventory)],
) -> Result<PlacementDecision, SchedulerError> {
    let mut candidates = candidates.to_vec();
    candidates.sort_by_key(|candidate| candidate.0);
    let (shard, _inventory) = candidates
        .into_iter()
        .find(|(_, inventory)| {
            inventory.cpu_cores >= request.cpu_cores
                && inventory.memory_bytes >= request.memory_bytes
                && inventory.gpu_memory_bytes >= request.gpu_memory_bytes
        })
        .ok_or(SchedulerError::NoPlacement)?;
    Ok(PlacementDecision {
        brain: request.brain,
        shard,
        explanation: "lowest stable shard identity satisfying the declared resource request"
            .to_owned(),
    })
}
