//! Explicitly authorised federation offers and positive-delay links.
//!
//! Federation is a link between independent brains, never shared mutable
//! state. Discovery may surface an offer but cannot create a link. Each link
//! has separate direction credits, deduplication and time translation.

use crate::deterministic::{BrainId, LogicalTag, StateDigestBuilder, StreamId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const FEDERATION_SCHEMA_VERSION: u32 = 1;
pub const MAX_FEDERATION_RECEIVED_SEQUENCES: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationScope {
    Private,
    TrustedRealm,
    Public,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederationOffer {
    pub schema_version: u32,
    pub listing_id: u64,
    pub brain_id: BrainId,
    pub scope: FederationScope,
    pub protocol_min: u32,
    pub protocol_max: u32,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub positive_delay_ticks: u64,
    pub expires_at_ms: u64,
    pub required_trust: String,
    pub digest: [u8; 16],
}

impl FederationOffer {
    pub fn new(
        listing_id: u64,
        brain_id: BrainId,
        scope: FederationScope,
        input_modalities: Vec<String>,
        output_modalities: Vec<String>,
        positive_delay_ticks: u64,
        expires_at_ms: u64,
        required_trust: impl Into<String>,
    ) -> Result<Self, FederationError> {
        let mut offer = Self {
            schema_version: FEDERATION_SCHEMA_VERSION,
            listing_id,
            brain_id,
            scope,
            protocol_min: FEDERATION_SCHEMA_VERSION,
            protocol_max: FEDERATION_SCHEMA_VERSION,
            input_modalities,
            output_modalities,
            positive_delay_ticks,
            expires_at_ms,
            required_trust: required_trust.into(),
            digest: [0; 16],
        };
        offer.digest = offer.compute_digest()?;
        offer.validate(0)?;
        Ok(offer)
    }

    pub fn validate(&self, now_ms: u64) -> Result<(), FederationError> {
        if self.schema_version != FEDERATION_SCHEMA_VERSION
            || self.listing_id == 0
            || self.protocol_min == 0
            || self.protocol_min > self.protocol_max
            || self.positive_delay_ticks == 0
            || self.expires_at_ms <= now_ms
            || self.required_trust.trim().is_empty()
        {
            return Err(FederationError::InvalidOffer);
        }
        if self.input_modalities.len() + self.output_modalities.len() > 64
            || self
                .input_modalities
                .iter()
                .chain(&self.output_modalities)
                .any(|value| value.len() > 128)
        {
            return Err(FederationError::InvalidOffer);
        }
        if self.compute_digest()? != self.digest {
            return Err(FederationError::DigestMismatch);
        }
        Ok(())
    }

    fn compute_digest(&self) -> Result<[u8; 16], FederationError> {
        let mut material = self.clone();
        material.digest = [0; 16];
        let bytes = serde_json::to_vec(&material).map_err(|_| FederationError::Encoding)?;
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("federation-offer:v1", bytes);
        Ok(digest.finish().0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederationConsent {
    pub local_approved: bool,
    pub remote_approved: bool,
    pub dual_authorisation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederationLink {
    pub link_id: StreamId,
    pub local_brain: BrainId,
    pub remote_brain: BrainId,
    pub positive_delay_ticks: u64,
    pub input_credits: u32,
    pub output_credits: u32,
    pub consent: FederationConsent,
    pub active: bool,
    pub path_epoch: u64,
    pub received_sequences: BTreeSet<u64>,
}

impl FederationLink {
    pub fn admit_tag(&self, tag: LogicalTag) -> Result<LogicalTag, FederationError> {
        if !self.active || self.positive_delay_ticks == 0 {
            return Err(FederationError::LinkInactive);
        }
        tag.positive_delay(self.positive_delay_ticks)
            .map_err(|_| FederationError::TimeOverflow)
    }

    pub fn receive_sequence(&mut self, sequence: u64) -> Result<bool, FederationError> {
        if !self.active {
            return Err(FederationError::LinkInactive);
        }
        if self.received_sequences.contains(&sequence) {
            return Ok(false);
        }
        if self.input_credits == 0 {
            return Err(FederationError::CreditExhausted);
        }
        if self.received_sequences.len() >= MAX_FEDERATION_RECEIVED_SEQUENCES {
            return Err(FederationError::ReceiveWindowFull);
        }
        self.input_credits -= 1;
        self.received_sequences.insert(sequence);
        Ok(true)
    }

    /// Return credits only for a committed remote input.  Duplicate
    /// acknowledgements cannot inflate the configured window.
    pub fn acknowledge_input(&mut self, credits: u32) {
        self.input_credits = self.input_credits.saturating_add(credits);
    }

    pub fn acknowledge_output(&mut self, credits: u32) {
        self.output_credits = self.output_credits.saturating_add(credits);
    }

    pub fn migrate_path(&mut self) -> Result<u64, FederationError> {
        self.path_epoch = self
            .path_epoch
            .checked_add(1)
            .ok_or(FederationError::TimeOverflow)?;
        Ok(self.path_epoch)
    }

    pub fn revoke(&mut self) {
        self.active = false;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FederationError {
    #[error("federation offer is invalid or expired")]
    InvalidOffer,
    #[error("federation offer digest does not match its contents")]
    DigestMismatch,
    #[error("federation encoding failed")]
    Encoding,
    #[error("federation link requires explicit local and remote consent")]
    ConsentRequired,
    #[error("zero-delay federation links are forbidden")]
    ZeroDelay,
    #[error("federation link is inactive")]
    LinkInactive,
    #[error("federation logical-time translation overflowed")]
    TimeOverflow,
    #[error("federation identity is blocked")]
    Blocked,
    #[error("federation receive credit window is exhausted")]
    CreditExhausted,
    #[error("federation receive window is full")]
    ReceiveWindowFull,
    #[error("federation link identity already exists")]
    DuplicateLink,
}

#[derive(Debug, Clone, Default)]
pub struct FederationDirectory {
    offers: BTreeMap<u64, FederationOffer>,
    blocked_brains: BTreeSet<BrainId>,
    links: BTreeMap<StreamId, FederationLink>,
}

impl FederationDirectory {
    pub fn publish_offer(
        &mut self,
        offer: FederationOffer,
        now_ms: u64,
    ) -> Result<(), FederationError> {
        offer.validate(now_ms)?;
        if self.blocked_brains.contains(&offer.brain_id) {
            return Err(FederationError::Blocked);
        }
        self.offers.insert(offer.listing_id, offer);
        Ok(())
    }

    pub fn withdraw_offer(&mut self, listing_id: u64) {
        self.offers.remove(&listing_id);
    }

    pub fn block(&mut self, brain: BrainId) {
        self.blocked_brains.insert(brain);
        self.offers.retain(|_, offer| offer.brain_id != brain);
        self.links
            .retain(|_, link| link.local_brain != brain && link.remote_brain != brain);
    }

    pub fn create_link(
        &mut self,
        link_id: StreamId,
        local_brain: BrainId,
        offer: &FederationOffer,
        consent: FederationConsent,
        input_credits: u32,
        output_credits: u32,
    ) -> Result<FederationLink, FederationError> {
        offer.validate(0)?;
        if self.blocked_brains.contains(&local_brain)
            || self.blocked_brains.contains(&offer.brain_id)
        {
            return Err(FederationError::Blocked);
        }
        if !consent.local_approved
            || !consent.remote_approved
            || input_credits == 0
            || output_credits == 0
        {
            return Err(FederationError::ConsentRequired);
        }
        if offer.positive_delay_ticks == 0 {
            return Err(FederationError::ZeroDelay);
        }
        if self.links.contains_key(&link_id) {
            return Err(FederationError::DuplicateLink);
        }
        let link = FederationLink {
            link_id,
            local_brain,
            remote_brain: offer.brain_id,
            positive_delay_ticks: offer.positive_delay_ticks,
            input_credits,
            output_credits,
            consent,
            active: true,
            path_epoch: 1,
            received_sequences: BTreeSet::new(),
        };
        self.links.insert(link_id, link.clone());
        Ok(link)
    }

    pub fn revoke_link(&mut self, link_id: StreamId) -> bool {
        self.links.get_mut(&link_id).is_some_and(|link| {
            link.revoke();
            true
        })
    }

    pub fn link(&self, link_id: StreamId) -> Option<&FederationLink> {
        self.links.get(&link_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_offer_never_activates_a_link_and_link_adds_positive_delay() {
        let local = BrainId::new(1).unwrap();
        let remote = BrainId::new(2).unwrap();
        let offer = FederationOffer::new(
            7,
            remote,
            FederationScope::Private,
            vec!["touch".into()],
            vec!["audio".into()],
            2,
            100,
            "trusted",
        )
        .unwrap();
        let mut directory = FederationDirectory::default();
        directory.publish_offer(offer.clone(), 1).unwrap();
        assert!(directory.link(StreamId::new(8).unwrap()).is_none());
        let mut link = directory
            .create_link(
                StreamId::new(8).unwrap(),
                local,
                &offer,
                FederationConsent {
                    local_approved: true,
                    remote_approved: true,
                    dual_authorisation: true,
                },
                4,
                4,
            )
            .unwrap();
        assert_eq!(
            link.admit_tag(LogicalTag::new(5, 3)).unwrap(),
            LogicalTag::new(7, 0)
        );
        assert!(link.receive_sequence(1).unwrap());
        assert!(!link.receive_sequence(1).unwrap());
    }

    #[test]
    fn revoked_or_blocked_federation_cannot_resume() {
        let local = BrainId::new(1).unwrap();
        let remote = BrainId::new(2).unwrap();
        let offer = FederationOffer::new(
            9,
            remote,
            FederationScope::TrustedRealm,
            vec![],
            vec![],
            1,
            50,
            "realm",
        )
        .unwrap();
        let mut directory = FederationDirectory::default();
        directory.publish_offer(offer.clone(), 0).unwrap();
        directory
            .create_link(
                StreamId::new(10).unwrap(),
                local,
                &offer,
                FederationConsent {
                    local_approved: true,
                    remote_approved: true,
                    dual_authorisation: false,
                },
                1,
                1,
            )
            .unwrap();
        assert!(directory.revoke_link(StreamId::new(10).unwrap()));
        assert!(!directory.link(StreamId::new(10).unwrap()).unwrap().active);
        directory.block(remote);
        assert!(directory.link(StreamId::new(10).unwrap()).is_none());
    }

    #[test]
    fn federation_input_credit_is_consumed_once_and_path_migration_is_explicit() {
        let local = BrainId::new(10).unwrap();
        let remote = BrainId::new(11).unwrap();
        let offer = FederationOffer::new(
            10,
            remote,
            FederationScope::Private,
            vec!["touch".into()],
            vec!["audio".into()],
            1,
            100,
            "trusted",
        )
        .unwrap();
        let mut directory = FederationDirectory::default();
        let mut link = directory
            .create_link(
                StreamId::new(10).unwrap(),
                local,
                &offer,
                FederationConsent {
                    local_approved: true,
                    remote_approved: true,
                    dual_authorisation: true,
                },
                1,
                1,
            )
            .unwrap();
        assert!(link.receive_sequence(1).unwrap());
        assert!(!link.receive_sequence(1).unwrap());
        assert!(matches!(
            link.receive_sequence(2),
            Err(FederationError::CreditExhausted)
        ));
        link.acknowledge_input(1);
        assert!(link.receive_sequence(2).unwrap());
        assert_eq!(link.migrate_path().unwrap(), 2);
    }
}
