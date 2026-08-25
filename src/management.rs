//! Orchestrator-authorised management reference contract.

use crate::deterministic::{EventId, LeaseTerm};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    Read,
    Operate,
    Reset,
    Export,
    PeripheralInput,
    PeripheralOutput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub id: String,
}

#[derive(Debug, Clone, Default)]
pub struct Policy {
    grants: BTreeMap<String, BTreeSet<Capability>>,
}

impl Policy {
    pub fn grant(&mut self, principal: impl Into<String>, capability: Capability) {
        self.grants
            .entry(principal.into())
            .or_default()
            .insert(capability);
    }

    pub fn allows(&self, principal: &Principal, capability: &Capability) -> bool {
        self.grants
            .get(&principal.id)
            .is_some_and(|capabilities| capabilities.contains(capability))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationKind {
    Start,
    Stop,
    Reset,
    Export,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationState {
    Pending,
    Running,
    Succeeded,
    Failed { code: String },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    pub id: EventId,
    pub principal: Principal,
    pub idempotency_key: String,
    pub request_id: String,
    pub expected_version: u64,
    pub kind: OperationKind,
    pub state: OperationState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    pub request_id: String,
    pub principal: String,
    pub operation_id: EventId,
    pub outcome: String,
    pub leader_term: LeaseTerm,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ManagementError {
    #[error("principal {principal} lacks capability {capability:?}")]
    Forbidden {
        principal: String,
        capability: Capability,
    },
    #[error("request used stale leader term: expected {expected}, received {received}")]
    StaleLeader {
        expected: LeaseTerm,
        received: LeaseTerm,
    },
    #[error("expected resource version {expected}, current version is {current}")]
    VersionConflict { expected: u64, current: u64 },
    #[error("idempotency key {0} was reused for a different operation")]
    IdempotencyConflict(String),
    #[error("idempotency key is empty")]
    EmptyIdempotencyKey,
    #[error("request ID is empty")]
    EmptyRequestId,
    #[error("operation {0} is not present")]
    MissingOperation(EventId),
    #[error("operation identity space is exhausted")]
    OperationIdExhausted,
    #[error("resource version space is exhausted")]
    ResourceVersionOverflow,
}

#[derive(Debug, Clone)]
pub struct MutationContext {
    pub observed_leader_term: LeaseTerm,
    pub expected_version: u64,
    pub idempotency_key: String,
    pub request_id: String,
}

#[derive(Debug, Clone)]
pub struct ManagementOrchestrator {
    leader_term: LeaseTerm,
    resource_version: u64,
    next_operation: u64,
    policy: Policy,
    operations: BTreeMap<EventId, Operation>,
    idempotency: BTreeMap<String, EventId>,
    audit: Vec<AuditRecord>,
}

impl ManagementOrchestrator {
    pub fn new(leader_term: LeaseTerm, policy: Policy) -> Self {
        Self {
            leader_term,
            resource_version: 0,
            next_operation: 1,
            policy,
            operations: BTreeMap::new(),
            idempotency: BTreeMap::new(),
            audit: Vec::new(),
        }
    }

    pub fn replace_leader_term(&mut self, term: LeaseTerm) {
        if term > self.leader_term {
            self.leader_term = term;
        }
    }

    pub fn submit(
        &mut self,
        principal: Principal,
        capability: Capability,
        context: MutationContext,
        kind: OperationKind,
    ) -> Result<Operation, ManagementError> {
        if !self.policy.allows(&principal, &capability) {
            return Err(ManagementError::Forbidden {
                principal: principal.id,
                capability,
            });
        }
        if context.observed_leader_term != self.leader_term {
            return Err(ManagementError::StaleLeader {
                expected: self.leader_term,
                received: context.observed_leader_term,
            });
        }
        if context.idempotency_key.is_empty() {
            return Err(ManagementError::EmptyIdempotencyKey);
        }
        if context.request_id.is_empty() {
            return Err(ManagementError::EmptyRequestId);
        }
        if let Some(existing_id) = self.idempotency.get(&context.idempotency_key) {
            let existing = self
                .operations
                .get(existing_id)
                .expect("idempotency index is consistent");
            if existing.kind != kind || existing.principal != principal {
                return Err(ManagementError::IdempotencyConflict(
                    context.idempotency_key,
                ));
            }
            return Ok(existing.clone());
        }
        if context.expected_version != self.resource_version {
            return Err(ManagementError::VersionConflict {
                expected: context.expected_version,
                current: self.resource_version,
            });
        }
        let next_operation = self
            .next_operation
            .checked_add(1)
            .ok_or(ManagementError::OperationIdExhausted)?;
        let next_resource_version = self
            .resource_version
            .checked_add(1)
            .ok_or(ManagementError::ResourceVersionOverflow)?;
        let id =
            EventId::new(self.next_operation).map_err(|_| ManagementError::OperationIdExhausted)?;
        let operation = Operation {
            id,
            principal: principal.clone(),
            idempotency_key: context.idempotency_key.clone(),
            request_id: context.request_id.clone(),
            expected_version: context.expected_version,
            kind,
            state: OperationState::Pending,
        };
        self.idempotency.insert(context.idempotency_key, id);
        self.operations.insert(id, operation.clone());
        self.next_operation = next_operation;
        self.resource_version = next_resource_version;
        self.audit.push(AuditRecord {
            request_id: context.request_id,
            principal: principal.id,
            operation_id: id,
            outcome: "accepted".to_owned(),
            leader_term: self.leader_term,
        });
        Ok(operation)
    }

    pub fn transition(
        &mut self,
        operation_id: EventId,
        observed_leader_term: LeaseTerm,
        state: OperationState,
    ) -> Result<(), ManagementError> {
        if observed_leader_term != self.leader_term {
            return Err(ManagementError::StaleLeader {
                expected: self.leader_term,
                received: observed_leader_term,
            });
        }
        let operation = self
            .operations
            .get_mut(&operation_id)
            .ok_or(ManagementError::MissingOperation(operation_id))?;
        operation.state = state;
        self.audit.push(AuditRecord {
            request_id: operation.request_id.clone(),
            principal: operation.principal.id.clone(),
            operation_id,
            outcome: "transitioned".to_owned(),
            leader_term: self.leader_term,
        });
        Ok(())
    }

    pub fn operation(&self, operation_id: EventId) -> Option<&Operation> {
        self.operations.get(&operation_id)
    }

    pub fn audit(&self) -> &[AuditRecord] {
        &self.audit
    }

    pub const fn resource_version(&self) -> u64 {
        self.resource_version
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(term: LeaseTerm) -> MutationContext {
        MutationContext {
            observed_leader_term: term,
            expected_version: 0,
            idempotency_key: "operation-1".to_owned(),
            request_id: "request-1".to_owned(),
        }
    }

    #[test]
    fn stale_worker_cannot_transition_operation_after_fencing() {
        let mut policy = Policy::default();
        policy.grant("operator", Capability::Operate);
        let mut manager = ManagementOrchestrator::new(LeaseTerm::INITIAL, policy);
        let operation = manager
            .submit(
                Principal {
                    id: "operator".to_owned(),
                },
                Capability::Operate,
                context(LeaseTerm::INITIAL),
                OperationKind::Start,
            )
            .expect("operation is accepted by the current leader");

        let next_term = LeaseTerm::new(2).expect("non-zero term");
        manager.replace_leader_term(next_term);
        let result =
            manager.transition(operation.id, LeaseTerm::INITIAL, OperationState::Succeeded);

        assert!(matches!(
            result,
            Err(ManagementError::StaleLeader {
                expected,
                received
            }) if expected == next_term && received == LeaseTerm::INITIAL
        ));
        assert_eq!(
            manager
                .operation(operation.id)
                .map(|operation| &operation.state),
            Some(&OperationState::Pending)
        );
        assert_eq!(manager.audit().len(), 1);
    }
}
