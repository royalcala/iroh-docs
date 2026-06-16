//! # iroh-syntrix-docs
//!
//! Authorization wrapper for [`iroh_docs`] that provides:
//!
//! - **NamespaceRegistry:** reads a control namespace to determine which namespaces a peer
//!   should open, based on org membership and role assignments.
//! - **Accept callback:** decides whether to accept or reject an incoming sync connection
//!   based on the peer's active status in the control namespace.
//!
//! ## Architecture
//!
//! ```text
//! iroh-syntrix-docs
//! ├── NamespaceRegistry  ← reads org_control, maps role → namespaces
//! └── accept_cb          ← allows/rejects sync handshakes
//! ```
//!
//! The control namespace is a regular [`iroh_docs`] namespace where the admin writes
//! membership and role entries:
//!
//! - `members/<author_id>` → `{ active: bool, role: "sales" }`
//! - `roles/<role_name>` → `{ read: [...namespace_ids], write: [...namespace_ids] }`

use iroh_docs::NamespaceId;

pub mod registry;
pub mod accept;

/// Identifies an organization within the system.
///
/// Used as a prefix for all namespaces belonging to the same org.
pub type OrgId = String;

/// A namespace identifier qualified with its owning organization.
#[derive(Debug, Clone)]
pub struct QualifiedNamespace {
    pub org_id: OrgId,
    pub namespace_id: NamespaceId,
}

/// Result type for this crate.
pub type Result<T> = anyhow::Result<T>;
