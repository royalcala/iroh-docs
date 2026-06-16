//! Accept callback for sync connections.
//!
//! Implements the `accept_cb` used by [`iroh_docs::net::handle_connection`]
//! to decide whether to allow or reject an incoming sync request.

use iroh::PublicKey;
use iroh_docs::{net::AcceptOutcome, net::AbortReason, NamespaceId};

use crate::registry::NamespaceRegistry;
use crate::OrgId;

/// Creates an accept callback that consults the given [`NamespaceRegistry`].
///
/// The returned closure can be passed directly to
/// [`iroh_docs::net::handle_connection`].
pub fn make_accept_cb(
    registry: std::sync::Arc<NamespaceRegistry>,
    org_id: OrgId,
) -> impl Fn(NamespaceId, PublicKey) -> std::future::Ready<AcceptOutcome> {
    move |_namespace: NamespaceId, peer: PublicKey| {
        let peer_bytes: &[u8; 32] = peer.as_bytes();

        if registry.is_member_active(&org_id, peer_bytes) {
            std::future::ready(AcceptOutcome::Allow)
        } else {
            std::future::ready(AcceptOutcome::Reject(AbortReason::NotFound))
        }
    }
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;
    use crate::registry::{Member, RoleGrants};

    fn make_pubkey(seed: u8) -> PublicKey {
        let secret = SecretKey::from_bytes(&[seed; 32]);
        secret.public()
    }

    #[test]
    fn test_accept_active_member() {
        let mut reg = NamespaceRegistry::new();
        let alice_key = make_pubkey(1);
        let alice_id = *alice_key.as_bytes();

        reg.upsert_member(
            "acme".into(),
            alice_id,
            Member {
                author_id: alice_id,
                active: true,
                role: "sales".into(),
            },
        );

        let cb = make_accept_cb(std::sync::Arc::new(reg), "acme".into());
        let outcome = cb(NamespaceId::default(), alice_key).into_inner();

        assert!(matches!(outcome, AcceptOutcome::Allow));
    }

    #[test]
    fn test_reject_inactive_member() {
        let mut reg = NamespaceRegistry::new();
        let bob_key = make_pubkey(2);
        let bob_id = *bob_key.as_bytes();

        reg.upsert_member(
            "acme".into(),
            bob_id,
            Member {
                author_id: bob_id,
                active: false,
                role: "sales".into(),
            },
        );

        let cb = make_accept_cb(std::sync::Arc::new(reg), "acme".into());
        let outcome = cb(NamespaceId::default(), bob_key).into_inner();

        assert!(matches!(outcome, AcceptOutcome::Reject(_)));
    }

    #[test]
    fn test_reject_unknown_peer() {
        let reg = NamespaceRegistry::new();
        let unknown_key = make_pubkey(99);

        let cb = make_accept_cb(std::sync::Arc::new(reg), "acme".into());
        let outcome = cb(NamespaceId::default(), unknown_key).into_inner();

        assert!(matches!(outcome, AcceptOutcome::Reject(_)));
    }
}
