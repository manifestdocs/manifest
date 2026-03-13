//! Strongly-typed ID newtypes for domain entities.
//!
//! Each entity type gets its own ID type (e.g., `ProjectId`, `FeatureId`)
//! to prevent accidental misuse of one entity's ID as another's.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn inner(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }

        impl From<Uuid> for $name {
            fn from(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Uuid {
                id.0
            }
        }
    };
}

define_id!(
    /// Unique identifier for a [`Project`](super::Project).
    ProjectId
);

define_id!(
    /// Unique identifier for a [`Feature`](super::Feature).
    FeatureId
);

define_id!(
    /// Unique identifier for a [`Version`](super::Version).
    VersionId
);

define_id!(
    /// Unique identifier for a [`Session`](super::Session).
    SessionId
);

define_id!(
    /// Unique identifier for a [`Task`](super::Task).
    TaskId
);

define_id!(
    /// Unique identifier for a [`User`](super::User).
    UserId
);

define_id!(
    /// Unique identifier for a [`FeatureHistory`](super::FeatureHistory) entry.
    HistoryId
);

define_id!(
    /// Unique identifier for a [`ProjectMembership`](super::ProjectMembership).
    MembershipId
);

define_id!(
    /// Unique identifier for a [`ProjectDirectory`](super::ProjectDirectory).
    DirectoryId
);

define_id!(
    /// Unique identifier for a [`Proof`](super::Proof).
    ProofId
);

define_id!(
    /// Unique identifier for a [`SpecTemplate`](super::SpecTemplate).
    TemplateId
);

define_id!(
    /// Unique identifier for a [`Remote`](super::Remote).
    RemoteId
);
