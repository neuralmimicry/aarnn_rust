// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Hosted tenancy: subscription entitlements and quota decisions for
//! customer-owned runtime workspaces.
//!
//! A hosted control API serves paying customers who grow their own AARNN
//! networks. Billing records each customer's subscription and the plan's
//! limits; this module reads that entitlement (`GET <entitlements_url>/<owner>`)
//! and turns it into allow/deny decisions. The decisions are pure functions so
//! they can be tested without a server, and every check fails closed: an
//! owner without an active subscription may not create, start or connect
//! anything, and an unreachable Billing service refuses the request rather
//! than granting unlimited capacity.
//!
//! Tenancy is off unless `NM_TENANCY_ENTITLEMENTS_URL` is set, so the research
//! deployment keeps its existing behaviour.

use anyhow::Context;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Service key whose entitlement governs hosted brains.
pub const TENANCY_SERVICE_KEY: &str = "aarnn";
const DEFAULT_CACHE_SECS: u64 = 15;
const DEFAULT_TIMEOUT_SECS: u64 = 5;

/// Plan limits for one owner. A zero limit allows nothing of that kind.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TenantLimits {
    /// Runtime workspaces (brains) the owner may hold at once.
    pub max_networks: u64,
    /// Upper bound on a brain's total neurons, including neurons grown later.
    pub max_neurons_per_network: u64,
    /// Brains that may be running (scheduled) at once.
    pub max_running_networks: u64,
    /// Distinct device principals the owner may connect across all brains.
    pub max_devices: u64,
}

/// The entitlement Billing returns for an owner.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Entitlement {
    pub active: bool,
    pub user_id: Option<String>,
    pub plan_id: Option<String>,
    pub subscription_id: Option<String>,
    pub status: Option<String>,
    pub current_period_end: Option<String>,
    pub limits: TenantLimits,
}

impl Entitlement {
    pub fn inactive() -> Self {
        Self::default()
    }
}

/// What the owner is already using, counted by the runtime manager.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TenantUsage {
    pub networks: u64,
    pub running_networks: u64,
    pub devices: u64,
}

/// The outcome of a quota check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuotaDecision {
    Allowed,
    /// No active subscription for the hosted service.
    SubscriptionRequired,
    /// A plan limit would be exceeded.
    Exceeded {
        quota: &'static str,
        limit: u64,
        used: u64,
    },
}

impl QuotaDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed)
    }
}

/// May the owner create (or import into a new) brain?
pub fn check_create(entitlement: &Entitlement, usage: TenantUsage) -> QuotaDecision {
    if !entitlement.active {
        return QuotaDecision::SubscriptionRequired;
    }
    let limit = entitlement.limits.max_networks;
    if usage.networks >= limit {
        return QuotaDecision::Exceeded {
            quota: "max_networks",
            limit,
            used: usage.networks,
        };
    }
    QuotaDecision::Allowed
}

/// May the owner start one more brain? `already_running` is true when the
/// brain being started is already counted in `usage.running_networks`.
pub fn check_start(
    entitlement: &Entitlement,
    usage: TenantUsage,
    already_running: bool,
) -> QuotaDecision {
    if !entitlement.active {
        return QuotaDecision::SubscriptionRequired;
    }
    if already_running {
        return QuotaDecision::Allowed;
    }
    let limit = entitlement.limits.max_running_networks;
    if usage.running_networks >= limit {
        return QuotaDecision::Exceeded {
            quota: "max_running_networks",
            limit,
            used: usage.running_networks,
        };
    }
    QuotaDecision::Allowed
}

/// May the owner connect a device principal? Granting a principal that is
/// already connected to another of the owner's brains does not count twice.
pub fn check_device_grant(
    entitlement: &Entitlement,
    usage: TenantUsage,
    principal_already_counted: bool,
) -> QuotaDecision {
    if !entitlement.active {
        return QuotaDecision::SubscriptionRequired;
    }
    if principal_already_counted {
        return QuotaDecision::Allowed;
    }
    let limit = entitlement.limits.max_devices;
    if usage.devices >= limit {
        return QuotaDecision::Exceeded {
            quota: "max_devices",
            limit,
            used: usage.devices,
        };
    }
    QuotaDecision::Allowed
}

/// The neuron cap to apply to every brain the owner holds. Zero means the
/// plan allows no brain at all (never "unlimited", unlike the engine's own
/// `max_total_neurons`).
pub fn neuron_cap(entitlement: &Entitlement) -> u64 {
    if !entitlement.active {
        return 0;
    }
    entitlement.limits.max_neurons_per_network
}

/// Reads entitlements from Billing, with a short per-owner cache so a busy
/// device does not turn every AER frame into a Billing request.
pub struct EntitlementClient {
    base_url: String,
    bearer_token: Option<String>,
    http: Client,
    ttl: Duration,
    cache: Mutex<HashMap<String, (Instant, Entitlement)>>,
}

impl EntitlementClient {
    pub fn new(
        base_url: impl Into<String>,
        bearer_token: Option<String>,
        timeout: Duration,
        ttl: Duration,
    ) -> anyhow::Result<Self> {
        let base_url = base_url.into().trim().trim_end_matches('/').to_string();
        anyhow::ensure!(
            !base_url.is_empty(),
            "tenancy entitlements URL must not be empty"
        );
        let http = Client::builder()
            .timeout(timeout)
            .build()
            .context("failed to build tenancy entitlement client")?;
        Ok(Self {
            base_url,
            bearer_token: bearer_token
                .map(|token| token.trim().to_string())
                .filter(|token| !token.is_empty()),
            http,
            ttl,
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// `NM_TENANCY_ENTITLEMENTS_URL` switches tenancy on. The bearer token
    /// (`NM_TENANCY_ENTITLEMENTS_TOKEN`) is the app token Billing issued for
    /// this control API.
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let Some(base_url) = env_nonempty("NM_TENANCY_ENTITLEMENTS_URL") else {
            return Ok(None);
        };
        let token = env_nonempty("NM_TENANCY_ENTITLEMENTS_TOKEN");
        let ttl = env_nonempty("NM_TENANCY_CACHE_SECS")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_CACHE_SECS);
        let timeout = env_nonempty("NM_TENANCY_TIMEOUT_SECS")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .max(1);
        Self::new(
            base_url,
            token,
            Duration::from_secs(timeout),
            Duration::from_secs(ttl),
        )
        .map(Some)
    }

    pub fn entitlement_url(&self, owner: &str) -> String {
        format!(
            "{}/{}?service={}",
            self.base_url,
            percent_encode_segment(owner),
            TENANCY_SERVICE_KEY
        )
    }

    /// Drop a cached entitlement, e.g. after a quota denial the caller wants
    /// to re-check against a freshly upgraded plan.
    pub fn invalidate(&self, owner: &str) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.remove(&owner.to_ascii_lowercase());
        }
    }

    pub async fn entitlement(&self, owner: &str) -> anyhow::Result<Entitlement> {
        let key = owner.to_ascii_lowercase();
        if !self.ttl.is_zero() {
            if let Ok(cache) = self.cache.lock() {
                if let Some((fetched_at, entitlement)) = cache.get(&key) {
                    if fetched_at.elapsed() < self.ttl {
                        return Ok(entitlement.clone());
                    }
                }
            }
        }
        let mut request = self.http.get(self.entitlement_url(owner));
        if let Some(token) = self.bearer_token.as_deref() {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .context("entitlement service unreachable")?;
        let status = response.status();
        let entitlement = if status == StatusCode::NOT_FOUND {
            Entitlement::inactive()
        } else if status.is_success() {
            response
                .json::<Entitlement>()
                .await
                .context("entitlement response was not valid JSON")?
        } else {
            anyhow::bail!("entitlement service returned HTTP {}", status.as_u16());
        };
        if !self.ttl.is_zero() {
            if let Ok(mut cache) = self.cache.lock() {
                cache.insert(key, (Instant::now(), entitlement.clone()));
            }
        }
        Ok(entitlement)
    }
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Percent-encode one URL path segment (RFC 3986 unreserved characters pass).
pub fn percent_encode_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active(limits: TenantLimits) -> Entitlement {
        Entitlement {
            active: true,
            plan_id: Some("builder".to_string()),
            limits,
            ..Entitlement::default()
        }
    }

    fn limits() -> TenantLimits {
        TenantLimits {
            max_networks: 2,
            max_neurons_per_network: 5000,
            max_running_networks: 1,
            max_devices: 2,
        }
    }

    #[test]
    fn inactive_subscription_refuses_everything() {
        let entitlement = Entitlement::inactive();
        let usage = TenantUsage::default();
        assert_eq!(
            check_create(&entitlement, usage),
            QuotaDecision::SubscriptionRequired
        );
        assert_eq!(
            check_start(&entitlement, usage, false),
            QuotaDecision::SubscriptionRequired
        );
        assert_eq!(
            check_device_grant(&entitlement, usage, false),
            QuotaDecision::SubscriptionRequired
        );
        assert_eq!(neuron_cap(&entitlement), 0);
    }

    #[test]
    fn create_is_bounded_by_max_networks() {
        let entitlement = active(limits());
        let mut usage = TenantUsage::default();
        assert!(check_create(&entitlement, usage).is_allowed());
        usage.networks = 2;
        assert_eq!(
            check_create(&entitlement, usage),
            QuotaDecision::Exceeded {
                quota: "max_networks",
                limit: 2,
                used: 2
            }
        );
    }

    #[test]
    fn start_counts_only_newly_running_brains() {
        let entitlement = active(limits());
        let usage = TenantUsage {
            running_networks: 1,
            ..TenantUsage::default()
        };
        assert!(check_start(&entitlement, usage, true).is_allowed());
        assert_eq!(
            check_start(&entitlement, usage, false),
            QuotaDecision::Exceeded {
                quota: "max_running_networks",
                limit: 1,
                used: 1
            }
        );
    }

    #[test]
    fn device_grants_count_distinct_principals() {
        let entitlement = active(limits());
        let usage = TenantUsage {
            devices: 2,
            ..TenantUsage::default()
        };
        assert!(check_device_grant(&entitlement, usage, true).is_allowed());
        assert!(!check_device_grant(&entitlement, usage, false).is_allowed());
    }

    #[test]
    fn neuron_cap_follows_the_plan() {
        assert_eq!(neuron_cap(&active(limits())), 5000);
    }

    #[test]
    fn entitlement_parses_billing_payload_with_defaults() {
        let parsed: Entitlement = serde_json::from_str(
            r#"{"active":true,"plan_id":"explorer","limits":{"max_networks":1}}"#,
        )
        .unwrap();
        assert!(parsed.active);
        assert_eq!(parsed.limits.max_networks, 1);
        assert_eq!(parsed.limits.max_devices, 0);
    }

    #[test]
    fn entitlement_url_encodes_the_owner() {
        let client = EntitlementClient::new(
            "http://billing:5020/api/internal/entitlements/",
            None,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(
            client.entitlement_url("ada lovelace+1"),
            "http://billing:5020/api/internal/entitlements/ada%20lovelace%2B1?service=aarnn"
        );
    }
}
