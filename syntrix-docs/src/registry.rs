//! Namespace registry.
//!
//! Reads the org's control namespace and determines which namespaces a peer
//! should open based on the member's role and active status.

use std::collections::{HashMap, HashSet};

use crate::OrgId;

/// A member of an organization.
#[derive(Debug, Clone)]
pub struct Member {
    pub author_id: [u8; 32],
    pub active: bool,
    pub role: String,
}

/// Capabilities granted to a role.
#[derive(Debug, Clone, Default)]
pub struct RoleGrants {
    pub read: Vec<String>,
    pub write: Vec<String>,
}

/// Tracks which namespaces a peer should open for each organization.
#[derive(Debug, Default)]
pub struct NamespaceRegistry {
    /// Members per org.
    members: HashMap<OrgId, HashMap<[u8; 32], Member>>,
    /// Role grants per org.
    grants: HashMap<OrgId, HashMap<String, RoleGrants>>,
}

impl NamespaceRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a member entry read from the control namespace.
    pub fn upsert_member(&mut self, org_id: OrgId, author_id: [u8; 32], member: Member) {
        self.members.entry(org_id).or_default().insert(author_id, member);
    }

    /// Record a role grant entry read from the control namespace.
    pub fn upsert_role(&mut self, org_id: OrgId, role: String, grants: RoleGrants) {
        self.grants.entry(org_id).or_default().insert(role, grants);
    }

    /// Check whether a member is active in the given organization.
    pub fn is_member_active(&self, org_id: &OrgId, author_id: &[u8; 32]) -> bool {
        self.members
            .get(org_id)
            .and_then(|m| m.get(author_id))
            .map(|m| m.active)
            .unwrap_or(false)
    }

    /// Get all namespaces the given member should have Read access to.
    pub fn readable_namespaces(&self, org_id: &OrgId, author_id: &[u8; 32]) -> HashSet<String> {
        let role = self
            .members
            .get(org_id)
            .and_then(|m| m.get(author_id))
            .map(|m| m.role.clone());

        let Some(role) = role else {
            return HashSet::new();
        };

        self.grants
            .get(org_id)
            .and_then(|g| g.get(&role))
            .map(|g| g.read.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get all namespaces the given member should have Write access to.
    pub fn writable_namespaces(&self, org_id: &OrgId, author_id: &[u8; 32]) -> HashSet<String> {
        let role = self
            .members
            .get(org_id)
            .and_then(|m| m.get(author_id))
            .map(|m| m.role.clone());

        let Some(role) = role else {
            return HashSet::new();
        };

        self.grants
            .get(org_id)
            .and_then(|g| g.get(&role))
            .map(|g| g.write.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// List all org IDs known to this registry.
    pub fn org_ids(&self) -> HashSet<OrgId> {
        self.members.keys().cloned().collect()
    }
}
