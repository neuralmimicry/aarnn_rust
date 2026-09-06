//! Shared authenticated node-session binding for inter-node streams.
//!
//! The node ID and token headers identify the claimed workload, but neither
//! is identity proof. When the live transport profile is enabled, the peer's
//! mTLS leaf certificate must also match the deployment-managed fingerprint
//! for that node. Keeping this boundary independent of the causal and stable
//! data-plane modules prevents either stream from silently weakening the
//! other's authentication policy.

use std::collections::BTreeMap;
use tonic::{Request, Status, metadata::MetadataMap};

pub(crate) fn live_causal_transport_enabled() -> bool {
    match std::env::var("NM_CAUSAL_TRANSPORT_LIVE")
        .ok()
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("1" | "true" | "yes" | "on") => true,
        _ => false,
    }
}

pub(crate) fn configured_node_tokens() -> Result<BTreeMap<String, String>, String> {
    let raw = std::env::var("NM_CAUSAL_NODE_TOKENS")
        .map_err(|_| "live causal transport requires NM_CAUSAL_NODE_TOKENS".to_owned())?;
    let mut tokens = BTreeMap::new();
    for entry in raw.split(',') {
        let (node, token) = entry
            .split_once('=')
            .ok_or_else(|| "NM_CAUSAL_NODE_TOKENS entries must use node=token".to_owned())?;
        let node = node.trim();
        let token = token.trim();
        if node.is_empty()
            || token.is_empty()
            || tokens.insert(node.to_owned(), token.to_owned()).is_some()
        {
            return Err("NM_CAUSAL_NODE_TOKENS contains an invalid or duplicate node".to_owned());
        }
    }
    if tokens.is_empty() {
        return Err("NM_CAUSAL_NODE_TOKENS must not be empty".to_owned());
    }
    Ok(tokens)
}

pub(crate) fn configured_node_cert_fingerprints() -> Result<BTreeMap<String, String>, String> {
    let raw = std::env::var("NM_CAUSAL_NODE_CERT_SHA256").map_err(|_| {
        "live causal transport requires NM_CAUSAL_NODE_CERT_SHA256=node=sha256hex,...".to_owned()
    })?;
    let mut fingerprints = BTreeMap::new();
    for entry in raw.split(',') {
        let (node, fingerprint) = entry.split_once('=').ok_or_else(|| {
            "NM_CAUSAL_NODE_CERT_SHA256 entries must use node=sha256hex".to_owned()
        })?;
        let node = node.trim();
        let fingerprint = fingerprint.trim().to_ascii_lowercase();
        if node.is_empty()
            || fingerprint.len() != 64
            || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
            || fingerprints.insert(node.to_owned(), fingerprint).is_some()
        {
            return Err(
                "NM_CAUSAL_NODE_CERT_SHA256 contains an invalid or duplicate entry".to_owned(),
            );
        }
    }
    if fingerprints.is_empty() {
        return Err("NM_CAUSAL_NODE_CERT_SHA256 must not be empty".to_owned());
    }
    Ok(fingerprints)
}

pub(crate) fn configured_token_for_node(node_id: &str) -> Result<String, String> {
    configured_node_tokens()?
        .remove(node_id)
        .ok_or_else(|| format!("NM_CAUSAL_NODE_TOKENS has no credential for node {node_id}"))
}

/// Attach the deployment-managed node identity to an internal control-plane
/// request.  The reference profile deliberately leaves requests unchanged;
/// the live profile requires the same node claim and per-node credential that
/// the receiving service validates against the peer certificate.
pub(crate) fn attach_node_metadata<T>(
    request: &mut Request<T>,
    node_id: &str,
) -> Result<(), String> {
    if !live_causal_transport_enabled() {
        return Ok(());
    }
    if node_id.trim().is_empty() {
        return Err("live node transport requires a non-empty node identity".to_owned());
    }
    let token = std::env::var("NM_CAUSAL_NODE_TOKEN")
        .map_err(|_| "live causal transport requires NM_CAUSAL_NODE_TOKEN".to_owned())?;
    if token.trim().is_empty() {
        return Err("NM_CAUSAL_NODE_TOKEN must not be empty".to_owned());
    }
    let expected = configured_token_for_node(node_id)?;
    if expected != token {
        return Err(format!(
            "NM_CAUSAL_NODE_TOKENS does not contain the local credential for node {node_id}"
        ));
    }
    attach_node_metadata_with_token(request, node_id, &token)
}

/// Construct an internal peer request with the deployment-managed node
/// session attached. In the reference profile this deliberately preserves
/// the existing unauthenticated in-process compatibility path; when live
/// causal transport is enabled, failure to resolve the local credential
/// prevents the request from being sent.
pub(crate) fn authenticated_request<T>(payload: T, node_id: &str) -> Result<Request<T>, String> {
    let mut request = Request::new(payload);
    attach_node_metadata(&mut request, node_id)?;
    Ok(request)
}

/// Validate the node session attached to an internal RPC whose protobuf body
/// does not repeat the sender identity. The identity is taken only from the
/// authenticated metadata and certificate binding; callers must still apply
/// their method-specific authorisation and network checks.
pub(crate) fn validate_live_request<T>(request: &Request<T>) -> Result<(), Status> {
    let sender_node_id = request
        .metadata()
        .get("x-aarnn-node-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Status::unauthenticated("node transport identity metadata is required"))?;
    let peer_certificate_sha256 = request.peer_certs().and_then(|certificates| {
        certificates
            .first()
            .map(|certificate| certificate_sha256_der(certificate.as_ref()))
    });
    validate_peer_metadata(
        request.metadata(),
        sender_node_id,
        peer_certificate_sha256.as_deref(),
    )
}

fn attach_node_metadata_with_token<T>(
    request: &mut Request<T>,
    node_id: &str,
    token: &str,
) -> Result<(), String> {
    let node_metadata = tonic::metadata::MetadataValue::try_from(node_id)
        .map_err(|error| format!("node identity is not valid metadata: {error}"))?;
    let token_metadata = tonic::metadata::MetadataValue::try_from(token)
        .map_err(|error| format!("node credential is not valid metadata: {error}"))?;
    request
        .metadata_mut()
        .insert("x-aarnn-node-id", node_metadata);
    request
        .metadata_mut()
        .insert("x-aarnn-node-token", token_metadata);
    Ok(())
}

/// Verify the node claim against the authenticated transport session.
pub(crate) fn validate_peer_metadata(
    metadata: &MetadataMap,
    sender_node_id: &str,
    peer_certificate_sha256: Option<&str>,
) -> Result<(), Status> {
    let header_node = metadata
        .get("x-aarnn-node-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let supplied_token = metadata
        .get("x-aarnn-node-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if header_node != sender_node_id || supplied_token.is_empty() {
        return Err(Status::unauthenticated(
            "node transport identity metadata is missing or inconsistent",
        ));
    }
    let expected = configured_node_tokens().map_err(Status::failed_precondition)?;
    let fingerprints = configured_node_cert_fingerprints().map_err(Status::failed_precondition)?;
    validate_peer_metadata_with_config(
        metadata,
        sender_node_id,
        peer_certificate_sha256,
        &expected,
        &fingerprints,
    )
}

fn validate_peer_metadata_with_config(
    metadata: &MetadataMap,
    sender_node_id: &str,
    peer_certificate_sha256: Option<&str>,
    expected_tokens: &BTreeMap<String, String>,
    enrolled_fingerprints: &BTreeMap<String, String>,
) -> Result<(), Status> {
    let header_node = metadata
        .get("x-aarnn-node-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let supplied_token = metadata
        .get("x-aarnn-node-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if header_node != sender_node_id || supplied_token.is_empty() {
        return Err(Status::unauthenticated(
            "node transport identity metadata is missing or inconsistent",
        ));
    }
    if expected_tokens.get(sender_node_id).map(String::as_str) != Some(supplied_token) {
        return Err(Status::unauthenticated(
            "causal transport node credential is invalid",
        ));
    }
    if !peer_certificate_sha256.is_some_and(|actual| {
        enrolled_fingerprints
            .get(sender_node_id)
            .is_some_and(|expected| expected == actual)
    }) {
        return Err(Status::unauthenticated(
            "node sender certificate does not match the enrolled node",
        ));
    }
    Ok(())
}

pub(crate) fn certificate_sha256_der(certificate: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(certificate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_session_metadata_fails_before_secret_lookup() {
        let result = validate_peer_metadata(&MetadataMap::new(), "worker-a", None);
        assert_eq!(
            result.expect_err("missing session must fail").code(),
            tonic::Code::Unauthenticated
        );
    }

    #[test]
    fn certificate_fingerprint_is_lowercase_sha256_der() {
        assert_eq!(
            certificate_sha256_der(b"leaf"),
            certificate_sha256_der(b"leaf").to_ascii_lowercase()
        );
        assert_eq!(certificate_sha256_der(b"leaf").len(), 64);
    }

    fn authenticated_metadata(node: &str, token: &str) -> MetadataMap {
        let mut metadata = MetadataMap::new();
        metadata.insert("x-aarnn-node-id", node.parse().unwrap());
        metadata.insert("x-aarnn-node-token", token.parse().unwrap());
        metadata
    }

    #[test]
    fn control_plane_metadata_binds_claim_to_local_credential() {
        let mut request = Request::new(());
        attach_node_metadata_with_token(&mut request, "worker-a", "secret-a")
            .expect("valid local node metadata");
        assert_eq!(
            request
                .metadata()
                .get("x-aarnn-node-id")
                .and_then(|value| value.to_str().ok()),
            Some("worker-a")
        );
        assert_eq!(
            request
                .metadata()
                .get("x-aarnn-node-token")
                .and_then(|value| value.to_str().ok()),
            Some("secret-a")
        );
    }

    fn enrolled() -> (BTreeMap<String, String>, BTreeMap<String, String>) {
        (
            BTreeMap::from([(String::from("worker-a"), String::from("secret-a"))]),
            BTreeMap::from([(
                String::from("worker-a"),
                certificate_sha256_der(b"worker-a-leaf"),
            )]),
        )
    }

    #[test]
    fn stable_stream_rejects_wrong_token() {
        let (tokens, fingerprints) = enrolled();
        let result = validate_peer_metadata_with_config(
            &authenticated_metadata("worker-a", "wrong"),
            "worker-a",
            Some(&fingerprints["worker-a"]),
            &tokens,
            &fingerprints,
        );
        assert_eq!(
            result.expect_err("wrong token must fail").code(),
            tonic::Code::Unauthenticated
        );
    }

    #[test]
    fn stable_stream_rejects_missing_or_mismatched_certificate() {
        let (tokens, fingerprints) = enrolled();
        for certificate in [None, Some("not-the-enrolled-certificate")] {
            let result = validate_peer_metadata_with_config(
                &authenticated_metadata("worker-a", "secret-a"),
                "worker-a",
                certificate.as_deref(),
                &tokens,
                &fingerprints,
            );
            assert_eq!(
                result
                    .expect_err("un-enrolled certificate must fail")
                    .code(),
                tonic::Code::Unauthenticated
            );
        }
    }
}
