use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

pub const SERVICE_ACCESS_NONE: &str = "none";
pub const SERVICE_ACCESS_REQUEST: &str = "request";
pub const SERVICE_ACCESS_OBSERVE: &str = "observe";
pub const SERVICE_ACCESS_USE: &str = "use";
pub const SERVICE_ACCESS_CONTROL: &str = "control";
pub const SERVICE_ACCOUNT_ROLE: &str = "service_account";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedServiceAccess {
    pub service_key: String,
    pub access_level: String,
    pub public_access_level: String,
    pub visible_access_level: String,
    pub visible: bool,
    pub can_request: bool,
    pub can_observe: bool,
    pub can_use: bool,
    pub can_control: bool,
}

impl ResolvedServiceAccess {
    pub fn none(service_key: impl Into<String>) -> Self {
        Self::new(
            service_key.into(),
            SERVICE_ACCESS_NONE,
            SERVICE_ACCESS_NONE,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        service_key: String,
        access_level: &str,
        public_access_level: &str,
        visible_access_level: Option<&str>,
        visible: Option<bool>,
        can_request: Option<bool>,
        can_observe: Option<bool>,
        can_use: Option<bool>,
        can_control: Option<bool>,
    ) -> Self {
        let access_level = normalize_service_access_level(access_level, SERVICE_ACCESS_NONE);
        let public_access_level =
            normalize_service_access_level(public_access_level, SERVICE_ACCESS_NONE);
        let derived_visible_access_level = max_access_level(&access_level, &public_access_level);
        let visible_access_level = normalize_service_access_level(
            visible_access_level.unwrap_or(derived_visible_access_level.as_str()),
            derived_visible_access_level.as_str(),
        );
        Self {
            service_key,
            access_level: access_level.clone(),
            public_access_level: public_access_level.clone(),
            visible_access_level: visible_access_level.clone(),
            visible: visible.unwrap_or(visible_access_level != SERVICE_ACCESS_NONE),
            can_request: can_request
                .unwrap_or_else(|| access_at_least(&visible_access_level, SERVICE_ACCESS_REQUEST)),
            can_observe: can_observe
                .unwrap_or_else(|| access_at_least(&visible_access_level, SERVICE_ACCESS_OBSERVE)),
            can_use: can_use.unwrap_or_else(|| access_at_least(&access_level, SERVICE_ACCESS_USE)),
            can_control: can_control
                .unwrap_or_else(|| access_at_least(&access_level, SERVICE_ACCESS_CONTROL)),
        }
    }
}

pub type ServiceAccessMap = BTreeMap<String, ResolvedServiceAccess>;

#[derive(Clone, Copy, Debug)]
struct ServiceDefaults {
    public_access_level: &'static str,
    authenticated_access_level: &'static str,
}

const DEFAULT_SERVICE_CATALOG: [(&str, ServiceDefaults); 2] = [
    (
        "aarnn",
        ServiceDefaults {
            public_access_level: SERVICE_ACCESS_REQUEST,
            authenticated_access_level: SERVICE_ACCESS_REQUEST,
        },
    ),
    (
        "billing",
        ServiceDefaults {
            public_access_level: SERVICE_ACCESS_NONE,
            authenticated_access_level: SERVICE_ACCESS_USE,
        },
    ),
];

pub fn normalize_service_access_level(value: &str, fallback: &str) -> String {
    let cleaned = value.trim().to_lowercase();
    match cleaned.as_str() {
        SERVICE_ACCESS_NONE
        | SERVICE_ACCESS_REQUEST
        | SERVICE_ACCESS_OBSERVE
        | SERVICE_ACCESS_USE
        | SERVICE_ACCESS_CONTROL => cleaned,
        _ => fallback.to_string(),
    }
}

pub fn access_at_least(current: &str, required: &str) -> bool {
    let rank =
        |value: &str| match normalize_service_access_level(value, SERVICE_ACCESS_NONE).as_str() {
            SERVICE_ACCESS_REQUEST => 1,
            SERVICE_ACCESS_OBSERVE => 2,
            SERVICE_ACCESS_USE => 3,
            SERVICE_ACCESS_CONTROL => 4,
            _ => 0,
        };
    rank(current) >= rank(required)
}

pub fn max_access_level(left: &str, right: &str) -> String {
    if access_at_least(left, right) {
        normalize_service_access_level(left, SERVICE_ACCESS_NONE)
    } else {
        normalize_service_access_level(right, SERVICE_ACCESS_NONE)
    }
}

pub fn visible_service_keys(service_access: &ServiceAccessMap) -> Vec<String> {
    service_access
        .iter()
        .filter(|(_, access)| access.visible)
        .map(|(service_key, _)| service_key.clone())
        .collect()
}

pub fn resolve_service_access(
    raw_service_access: Option<&Value>,
    authenticated: bool,
    role: &str,
    groups: &[String],
    is_admin: bool,
    override_authenticated_grants: &[(&str, &str)],
) -> ServiceAccessMap {
    let normalized_role = role.trim().to_lowercase();
    let is_service_account = normalized_role == SERVICE_ACCOUNT_ROLE;
    let is_admin =
        is_admin || normalized_role == "admin" || groups.iter().any(|group| group == "admin");
    let override_map = override_authenticated_grants
        .iter()
        .map(|(service_key, access_level)| {
            (
                service_key.trim().to_lowercase(),
                normalize_service_access_level(access_level, SERVICE_ACCESS_NONE),
            )
        })
        .collect::<HashMap<_, _>>();

    let mut resolved = ServiceAccessMap::new();
    for (service_key, defaults) in DEFAULT_SERVICE_CATALOG {
        let granted_access_level = if is_admin {
            SERVICE_ACCESS_CONTROL.to_string()
        } else if authenticated && !is_service_account {
            override_map
                .get(service_key)
                .cloned()
                .unwrap_or_else(|| defaults.authenticated_access_level.to_string())
        } else {
            SERVICE_ACCESS_NONE.to_string()
        };
        resolved.insert(
            service_key.to_string(),
            ResolvedServiceAccess::new(
                service_key.to_string(),
                &granted_access_level,
                defaults.public_access_level,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
        );
    }

    let mut apply_entry = |service_key: &str, raw_entry: Option<&Value>| {
        let cleaned_service_key = service_key.trim().to_lowercase();
        if cleaned_service_key.is_empty() {
            return;
        }
        let default_entry = resolved.get(&cleaned_service_key);
        let default_public_access_level = default_entry
            .map(|entry| entry.public_access_level.as_str())
            .unwrap_or(SERVICE_ACCESS_NONE);
        let default_access_level = default_entry
            .map(|entry| entry.access_level.as_str())
            .unwrap_or(SERVICE_ACCESS_NONE);
        let entry = raw_entry.and_then(Value::as_object);
        let direct_access = raw_entry.and_then(Value::as_str);
        let access_level = normalize_service_access_level(
            entry
                .and_then(|item| item.get("access_level").or_else(|| item.get("level")))
                .and_then(Value::as_str)
                .or(direct_access)
                .unwrap_or(default_access_level),
            default_access_level,
        );
        let public_access_level = normalize_service_access_level(
            entry
                .and_then(|item| item.get("public_access_level"))
                .and_then(Value::as_str)
                .unwrap_or(default_public_access_level),
            default_public_access_level,
        );
        let visible_access_fallback = max_access_level(&access_level, &public_access_level);
        let visible_access_level = normalize_service_access_level(
            entry
                .and_then(|item| item.get("visible_access_level"))
                .and_then(Value::as_str)
                .unwrap_or(visible_access_fallback.as_str()),
            visible_access_fallback.as_str(),
        );
        let visible = entry
            .and_then(|item| item.get("visible"))
            .and_then(json_bool)
            .unwrap_or(visible_access_level != SERVICE_ACCESS_NONE);
        let can_request = entry
            .and_then(|item| item.get("can_request"))
            .and_then(json_bool)
            .unwrap_or_else(|| access_at_least(&visible_access_level, SERVICE_ACCESS_REQUEST));
        let can_observe = entry
            .and_then(|item| item.get("can_observe"))
            .and_then(json_bool)
            .unwrap_or_else(|| access_at_least(&visible_access_level, SERVICE_ACCESS_OBSERVE));
        let can_use = entry
            .and_then(|item| item.get("can_use"))
            .and_then(json_bool)
            .unwrap_or_else(|| access_at_least(&access_level, SERVICE_ACCESS_USE));
        let can_control = entry
            .and_then(|item| item.get("can_control"))
            .and_then(json_bool)
            .unwrap_or_else(|| access_at_least(&access_level, SERVICE_ACCESS_CONTROL));
        resolved.insert(
            cleaned_service_key.clone(),
            ResolvedServiceAccess::new(
                cleaned_service_key,
                &access_level,
                &public_access_level,
                Some(&visible_access_level),
                Some(visible),
                Some(can_request),
                Some(can_observe),
                Some(can_use),
                Some(can_control),
            ),
        );
    };

    match raw_service_access {
        Some(Value::Object(entries)) => {
            for (service_key, raw_entry) in entries {
                apply_entry(service_key, Some(raw_entry));
            }
        }
        Some(Value::Array(entries)) => {
            for raw_entry in entries {
                let Some(service_key) = raw_entry
                    .get("service_key")
                    .or_else(|| raw_entry.get("key"))
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                apply_entry(service_key, Some(raw_entry));
            }
        }
        _ => {}
    }

    resolved
}

pub fn normalise_groups(values: &[String], role: &str) -> Vec<String> {
    let mut groups = Vec::new();
    let mut seen = HashSet::new();
    let normalized_role = role.trim().to_lowercase();
    if !normalized_role.is_empty()
        && normalized_role != SERVICE_ACCOUNT_ROLE
        && seen.insert(normalized_role.clone())
    {
        groups.push(normalized_role);
    }
    for value in values {
        let normalized = value.trim().to_lowercase();
        if normalized.is_empty() {
            continue;
        }
        if seen.insert(normalized.clone()) {
            groups.push(normalized);
        }
    }
    groups
}

fn json_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(inner) => Some(*inner),
        Value::Number(inner) => Some(inner.as_i64().unwrap_or_default() != 0),
        Value::String(inner) => Some(matches!(
            inner.trim().to_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn anonymous_users_only_receive_public_request_visibility() {
        let resolved =
            resolve_service_access(None, false, "user", &["user".to_string()], false, &[]);
        let aarnn = resolved.get("aarnn").expect("aarnn service should exist");
        assert_eq!(aarnn.access_level, SERVICE_ACCESS_NONE);
        assert_eq!(aarnn.public_access_level, SERVICE_ACCESS_REQUEST);
        assert_eq!(aarnn.visible_access_level, SERVICE_ACCESS_REQUEST);
        assert!(aarnn.visible);
        assert!(aarnn.can_request);
        assert!(!aarnn.can_observe);
        assert!(!aarnn.can_use);
        assert!(!aarnn.can_control);
    }

    #[test]
    fn authenticated_users_receive_default_aarnn_and_billing_access() {
        let resolved =
            resolve_service_access(None, true, "user", &["user".to_string()], false, &[]);
        let aarnn = resolved.get("aarnn").expect("aarnn service should exist");
        let billing = resolved
            .get("billing")
            .expect("billing service should exist");
        assert_eq!(aarnn.access_level, SERVICE_ACCESS_REQUEST);
        assert!(aarnn.can_request);
        assert!(!aarnn.can_observe);
        assert_eq!(billing.access_level, SERVICE_ACCESS_USE);
        assert!(billing.can_use);
        assert!(!billing.can_control);
    }

    #[test]
    fn service_accounts_do_not_receive_human_default_access() {
        let resolved = resolve_service_access(
            None,
            true,
            SERVICE_ACCOUNT_ROLE,
            &["ops".to_string()],
            false,
            &[],
        );
        let aarnn = resolved.get("aarnn").expect("aarnn service should exist");
        let billing = resolved
            .get("billing")
            .expect("billing service should exist");
        assert_eq!(aarnn.access_level, SERVICE_ACCESS_NONE);
        assert_eq!(aarnn.visible_access_level, SERVICE_ACCESS_REQUEST);
        assert!(aarnn.can_request);
        assert!(!aarnn.can_use);
        assert_eq!(billing.access_level, SERVICE_ACCESS_NONE);
        assert!(!billing.can_use);
        assert!(!billing.visible);
    }

    #[test]
    fn local_override_promotes_aarnn_to_control() {
        let resolved = resolve_service_access(
            None,
            true,
            "user",
            &["user".to_string()],
            false,
            &[("aarnn", SERVICE_ACCESS_CONTROL)],
        );
        let aarnn = resolved.get("aarnn").expect("aarnn service should exist");
        assert_eq!(aarnn.access_level, SERVICE_ACCESS_CONTROL);
        assert!(aarnn.can_control);
    }

    #[test]
    fn explicit_object_entries_override_defaults() {
        let resolved = resolve_service_access(
            Some(&json!({
                "aarnn": {
                    "service_key": "aarnn",
                    "access_level": "observe",
                    "public_access_level": "request"
                }
            })),
            true,
            "user",
            &["user".to_string()],
            false,
            &[],
        );
        let aarnn = resolved.get("aarnn").expect("aarnn service should exist");
        assert_eq!(aarnn.access_level, SERVICE_ACCESS_OBSERVE);
        assert!(aarnn.can_observe);
        assert!(!aarnn.can_use);
    }

    #[test]
    fn explicit_array_entries_are_supported() {
        let resolved = resolve_service_access(
            Some(&json!([
                {
                    "service_key": "aarnn",
                    "access_level": "use",
                    "public_access_level": "request"
                }
            ])),
            true,
            "user",
            &["user".to_string()],
            false,
            &[],
        );
        let aarnn = resolved.get("aarnn").expect("aarnn service should exist");
        assert_eq!(aarnn.access_level, SERVICE_ACCESS_USE);
        assert!(aarnn.can_use);
    }

    #[test]
    fn admin_groups_escalate_to_control() {
        let resolved =
            resolve_service_access(None, true, "user", &["admin".to_string()], false, &[]);
        let aarnn = resolved.get("aarnn").expect("aarnn service should exist");
        let billing = resolved
            .get("billing")
            .expect("billing service should exist");
        assert_eq!(aarnn.access_level, SERVICE_ACCESS_CONTROL);
        assert_eq!(billing.access_level, SERVICE_ACCESS_CONTROL);
        assert!(aarnn.can_control);
        assert!(billing.can_control);
    }

    #[test]
    fn explicit_admin_flag_escalates_to_control() {
        let resolved = resolve_service_access(None, true, "user", &["user".to_string()], true, &[]);
        let aarnn = resolved.get("aarnn").expect("aarnn service should exist");
        assert_eq!(aarnn.access_level, SERVICE_ACCESS_CONTROL);
        assert!(aarnn.can_control);
    }
}
