//! Query and filter types for the storage layer.

use crate::models::{FeatureId, FeatureState, ProjectId, VersionId};

/// Backend-agnostic query for listing features.
///
/// The SQLite backend compiles this to a WHERE clause.
/// The GitHub backend maps it to GraphQL filters and label queries.
///
/// Notably absent: arbitrary SQL. If a query pattern isn't expressible through
/// `FeatureQuery`, the answer is to extend `FeatureQuery`, not bypass the trait.
#[derive(Debug, Clone, Default)]
pub struct FeatureQuery {
    /// Filter by project.
    pub project_id: Option<ProjectId>,
    /// Filter by parent relationship.
    pub parent_id: Option<ParentFilter>,
    /// Filter by one or more states.
    pub state: Option<Vec<FeatureState>>,
    /// Filter by target version assignment.
    pub target_version_id: Option<VersionId>,
    /// Full-text search in title and details.
    pub search: Option<String>,
    /// Maximum number of results.
    pub limit: Option<i64>,
    /// Number of results to skip (for pagination).
    pub offset: Option<i64>,
}

impl FeatureQuery {
    pub fn for_project(project_id: ProjectId) -> Self {
        Self {
            project_id: Some(project_id),
            ..Default::default()
        }
    }

    pub fn with_state(mut self, state: FeatureState) -> Self {
        self.state = Some(vec![state]);
        self
    }

    pub fn with_states(mut self, states: Vec<FeatureState>) -> Self {
        self.state = Some(states);
        self
    }

    pub fn with_parent(mut self, parent: ParentFilter) -> Self {
        self.parent_id = Some(parent);
        self
    }

    pub fn with_version(mut self, version_id: VersionId) -> Self {
        self.target_version_id = Some(version_id);
        self
    }

    pub fn with_search(mut self, query: impl Into<String>) -> Self {
        self.search = Some(query.into());
        self
    }

    pub fn with_limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn with_offset(mut self, offset: i64) -> Self {
        self.offset = Some(offset);
        self
    }
}

/// Filter for parent relationship in feature queries.
#[derive(Debug, Clone)]
pub enum ParentFilter {
    /// Features with this specific parent.
    Exact(FeatureId),
    /// Root features (null parent_id).
    Root,
    /// All features regardless of parent.
    Any,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_query_default_is_empty() {
        let q = FeatureQuery::default();
        assert!(q.project_id.is_none());
        assert!(q.parent_id.is_none());
        assert!(q.state.is_none());
        assert!(q.target_version_id.is_none());
        assert!(q.search.is_none());
        assert!(q.limit.is_none());
        assert!(q.offset.is_none());
    }

    #[test]
    fn feature_query_builder_chains() {
        let project_id = ProjectId::new();
        let q = FeatureQuery::for_project(project_id)
            .with_state(FeatureState::Proposed)
            .with_limit(10)
            .with_offset(5);

        assert_eq!(q.project_id.unwrap(), project_id);
        assert_eq!(q.state.unwrap(), vec![FeatureState::Proposed]);
        assert_eq!(q.limit.unwrap(), 10);
        assert_eq!(q.offset.unwrap(), 5);
    }

    #[test]
    fn feature_query_with_search() {
        let q = FeatureQuery::default().with_search("auth");
        assert_eq!(q.search.unwrap(), "auth");
    }

    #[test]
    fn parent_filter_variants() {
        let id = FeatureId::new();
        let _exact = ParentFilter::Exact(id);
        let _root = ParentFilter::Root;
        let _any = ParentFilter::Any;
    }
}
