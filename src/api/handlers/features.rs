use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures_util::{stream::Stream, StreamExt};
use serde::Deserialize;
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

use crate::db::Database;
use crate::mcp::{PlanFeaturesResponse, ProposedFeature};
use crate::models::{
    CommitRef, CreateFeatureInput, CreateHistoryInput, Feature, FeatureDiff, FeatureHistory,
    FeatureId, FeatureState, FeatureSummary, FeatureTreeNode, HistoryDetails, ListFeaturesQuery,
    ProjectId, UpdateFeatureInput, VersionId,
};

use super::{internal_error, ApiError};
use crate::api::validation::{ValidatedJson, MAX_BULK_FEATURES};
use crate::serde_helpers::default_true;

// ============================================================
// Features
// ============================================================

/// List all features with optional pagination.
pub async fn list_features(
    State(db): State<Database>,
    Query(query): Query<ListFeaturesQuery>,
) -> Result<Json<Vec<FeatureSummary>>, ApiError> {
    // Use SQL-based pagination for efficiency
    let features = db
        .get_all_features_paginated(query.limit, query.offset)
        .await
        .map_err(internal_error)?;

    // Always return summaries only - use get_feature for full details
    let summaries: Vec<FeatureSummary> = features.into_iter().map(Into::into).collect();
    Ok(Json(summaries))
}

/// List all features for a specific project with optional pagination.
pub async fn list_project_features(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Query(query): Query<ListFeaturesQuery>,
) -> Result<Json<Vec<FeatureSummary>>, ApiError> {
    // Use SQL-based pagination for efficiency
    let features = db
        .get_features_by_project_paginated(project_id.into(), query.limit, query.offset)
        .await
        .map_err(internal_error)?;
    // Always return summaries only - use get_feature for full details
    let summaries: Vec<FeatureSummary> = features.into_iter().map(Into::into).collect();
    Ok(Json(summaries))
}

/// List top-level (root) features for a project.
pub async fn list_root_features(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<Feature>>, ApiError> {
    db.get_root_features(project_id.into())
        .await
        .map(Json)
        .map_err(internal_error)
}

/// Get the complete hierarchical feature tree for a project.
pub async fn get_feature_tree(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<FeatureTreeNode>>, ApiError> {
    db.get_feature_tree(project_id.into())
        .await
        .map(Json)
        .map_err(internal_error)
}

/// Query parameters for getting next workable feature.
#[derive(Debug, Deserialize)]
pub struct GetNextFeatureQuery {
    /// Optional version ID to filter features.
    pub version_id: Option<Uuid>,
}

/// Get the next workable feature for a project.
///
/// Returns the single highest-priority feature that is workable (proposed or in_progress).
/// Sort order: version > priority > created_at
/// - Features targeting "next" version (first unreleased) come first
/// - Then features with no version (backlog)
/// - Within each group: lower priority number wins
/// - Same priority: oldest created wins
pub async fn get_next_feature(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Query(query): Query<GetNextFeatureQuery>,
) -> Result<Json<Option<crate::models::FeatureWithContext>>, ApiError> {
    // Get the next workable feature
    let feature = db
        .get_next_workable_feature(project_id.into(), query.version_id.map(VersionId::from))
        .await
        .map_err(internal_error)?;

    // If we found a feature, enrich it with context
    match feature {
        Some(f) => {
            let feature_with_context = db
                .get_feature_with_context(f.id)
                .await
                .map_err(internal_error)?;
            Ok(Json(feature_with_context))
        }
        None => Ok(Json(None)),
    }
}

/// List direct child features of a parent feature.
pub async fn list_children(
    State(db): State<Database>,
    Path(parent_id): Path<Uuid>,
) -> Result<Json<Vec<Feature>>, ApiError> {
    db.get_children(parent_id.into())
        .await
        .map(Json)
        .map_err(internal_error)
}

/// Get implementation history entries for a feature.
pub async fn get_feature_history(
    State(db): State<Database>,
    Path(feature_id): Path<Uuid>,
) -> Result<Json<Vec<FeatureHistory>>, ApiError> {
    db.get_feature_history(feature_id.into())
        .await
        .map(Json)
        .map_err(internal_error)
}

/// Input for creating a history entry directly on a feature (CLI mode).
#[derive(Debug, Deserialize)]
pub struct CreateFeatureHistoryInput {
    pub summary: String,
    #[serde(default)]
    pub commits: Vec<CommitRef>,
    /// Version this work was done for.
    /// If not specified, defaults to the feature's target_version_id.
    pub version_id: Option<Uuid>,
    /// If true, also update feature state to 'implemented'. Defaults to true.
    #[serde(default = "default_true")]
    pub mark_implemented: bool,
}

/// Create a history entry directly on a feature.
///
/// Optionally marks the feature as implemented. Only allowed on leaf features.
pub async fn create_feature_history(
    State(db): State<Database>,
    Path(feature_id): Path<Uuid>,
    Json(input): Json<CreateFeatureHistoryInput>,
) -> Result<(StatusCode, Json<FeatureHistory>), ApiError> {
    // Verify feature exists
    let feature = db
        .get_feature(feature_id.into())
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::from((
            StatusCode::NOT_FOUND,
            "Feature not found".to_string(),
        )))?;

    // Verify it's a leaf feature
    if !db
        .is_leaf(feature_id.into())
        .await
        .map_err(internal_error)?
    {
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            "Cannot create history on a non-leaf feature".to_string(),
        )));
    }

    // Create history entry directly
    // If version_id not provided, database layer defaults to feature's target_version_id
    let history = db
        .create_history_entry(CreateHistoryInput {
            feature_id: feature_id.into(),
            version_id: input.version_id.map(VersionId::from),
            details: HistoryDetails {
                summary: input.summary,
                commits: input.commits,
            },
        })
        .await
        .map_err(internal_error)?;

    // Optionally update feature state to implemented and clear desired_details
    let needs_state_change = input.mark_implemented && feature.state != FeatureState::Implemented;
    let has_pending_changes = feature.desired_details.is_some();

    if needs_state_change || (input.mark_implemented && has_pending_changes) {
        db.update_feature(
            feature_id.into(),
            UpdateFeatureInput {
                parent_id: None,
                title: None,
                details: None,
                desired_details: if has_pending_changes {
                    Some(None) // Clear desired_details
                } else {
                    None // Don't touch
                },
                details_summary: None,
                state: if needs_state_change {
                    Some(FeatureState::Implemented)
                } else {
                    None
                },
                priority: None,
                target_version_id: None,
            },
        )
        .await
        .map_err(internal_error)?;
    }

    Ok((StatusCode::CREATED, Json(history)))
}

/// Get a feature by ID.
pub async fn get_feature(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<Feature>, ApiError> {
    db.get_feature(id.into())
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or(ApiError::from((
            StatusCode::NOT_FOUND,
            "Feature not found".to_string(),
        )))
}

/// Get a feature with hierarchical context (parent, siblings, children, breadcrumb).
///
/// This endpoint provides AI agents with navigation context to understand where
/// a feature sits in the feature tree.
pub async fn get_feature_with_context(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<crate::models::FeatureWithContext>, ApiError> {
    db.get_feature_with_context(id.into())
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or(ApiError::from((
            StatusCode::NOT_FOUND,
            "Feature not found".to_string(),
        )))
}

/// Get the diff between current and desired details for a feature.
pub async fn get_feature_diff(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<FeatureDiff>, ApiError> {
    db.get_feature_diff(id.into())
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or(ApiError::from((
            StatusCode::NOT_FOUND,
            "Feature not found".to_string(),
        )))
}

/// Create a new feature in a project.
pub async fn create_feature(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    ValidatedJson(input): ValidatedJson<CreateFeatureInput>,
) -> Result<(StatusCode, Json<Feature>), ApiError> {
    db.create_feature(project_id.into(), input)
        .await
        .map(|f| (StatusCode::CREATED, Json(f)))
        .map_err(internal_error)
}

/// Update an existing feature.
pub async fn update_feature(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
    ValidatedJson(input): ValidatedJson<UpdateFeatureInput>,
) -> Result<Json<Feature>, ApiError> {
    db.update_feature(id.into(), input)
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or(ApiError::from((
            StatusCode::NOT_FOUND,
            "Feature not found".to_string(),
        )))
}

/// Delete a feature and its descendants.
pub async fn delete_feature(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    if db.delete_feature(id.into()).await.map_err(internal_error)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::from((
            StatusCode::NOT_FOUND,
            "Feature not found".to_string(),
        )))
    }
}

/// Query parameters for searching features.
#[derive(Debug, Deserialize)]
pub struct SearchFeaturesQuery {
    /// Search term to match against title and details.
    pub q: String,
    /// Optional project UUID to limit search to.
    pub project_id: Option<Uuid>,
    /// Maximum number of results to return. Defaults to 10.
    pub limit: Option<u32>,
}

/// Search features by title and details.
/// Returns summaries ranked by relevance.
pub async fn search_features(
    State(db): State<Database>,
    Query(query): Query<SearchFeaturesQuery>,
) -> Result<Json<Vec<FeatureSummary>>, ApiError> {
    db.search_features(&query.q, query.project_id.map(ProjectId::from), query.limit)
        .await
        .map(Json)
        .map_err(internal_error)
}

// ============================================================
// Feature Resolution (short ID prefix matching)
// ============================================================

#[derive(Debug, Deserialize)]
pub struct ResolveFeatureQuery {
    /// UUID prefix to resolve (e.g., first 8 characters).
    pub prefix: String,
    /// Optional project UUID to scope the search to.
    pub project_id: Option<Uuid>,
}

/// Resolve a feature by UUID prefix.
/// Returns the matching feature if exactly one match is found.
pub async fn resolve_feature(
    State(db): State<Database>,
    Query(query): Query<ResolveFeatureQuery>,
) -> Result<Json<Feature>, ApiError> {
    db.resolve_feature_by_prefix(&query.prefix, query.project_id.map(ProjectId::from))
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or(ApiError::from((
            StatusCode::NOT_FOUND,
            format!("No feature found matching prefix '{}'", query.prefix),
        )))
}

// ============================================================
// Bulk Feature Creation (for MCP plan_features)
// ============================================================

/// Input for bulk feature creation.
#[derive(Debug, Deserialize)]
pub struct BulkCreateFeaturesInput {
    /// The target version for all features. If null, features go to backlog.
    pub target_version_id: Option<Uuid>,
    /// The proposed feature tree.
    pub features: Vec<ProposedFeature>,
    /// If true, creates the features in the database. If false, returns preview only.
    #[serde(default)]
    pub confirm: bool,
}

/// Create multiple features at once with hierarchical structure.
///
/// When confirm=false (default), returns the proposed features without creating them.
/// When confirm=true, creates all features and returns their IDs.
pub async fn bulk_create_features(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<BulkCreateFeaturesInput>,
) -> Result<Json<PlanFeaturesResponse>, ApiError> {
    // Guard rail: cap total features in the tree
    let total = count_proposed_features(&input.features);
    if total > MAX_BULK_FEATURES {
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            format!("Too many features ({total}, max {MAX_BULK_FEATURES})"),
        )));
    }

    // Verify project exists
    db.get_project(project_id.into())
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::from((
            StatusCode::NOT_FOUND,
            "Project not found".to_string(),
        )))?;

    let mut created_ids = Vec::new();

    if input.confirm {
        // Verify version exists if one was specified
        if let Some(vid) = input.target_version_id {
            db.get_version(vid.into())
                .await
                .map_err(internal_error)?
                .ok_or(ApiError::from((
                    StatusCode::NOT_FOUND,
                    "Version not found".to_string(),
                )))?;
        }

        // Flatten the tree into a list of inputs with pre-generated UUIDs
        // This allows us to use the transactional bulk insert
        let mut feature_inputs = Vec::new();
        let target_version_id = input.target_version_id.map(VersionId::from);
        for feature in &input.features {
            flatten_feature_tree(
                None,
                feature,
                target_version_id,
                &mut feature_inputs,
                &mut created_ids,
            );
        }

        // Create all features in a single transaction
        db.create_features_bulk(project_id.into(), feature_inputs)
            .await
            .map_err(internal_error)?;
    }

    Ok(Json(PlanFeaturesResponse {
        proposed_features: input.features,
        created: input.confirm,
        created_feature_ids: created_ids,
    }))
}

/// Flatten a ProposedFeature tree into a list of CreateFeatureInput.
/// Pre-generates UUIDs so parent-child relationships can be established.
fn flatten_feature_tree(
    parent_id: Option<FeatureId>,
    proposed: &ProposedFeature,
    target_version_id: Option<VersionId>,
    inputs: &mut Vec<CreateFeatureInput>,
    created_ids: &mut Vec<Uuid>,
) {
    let id = FeatureId::new();
    created_ids.push(id.into());

    inputs.push(CreateFeatureInput {
        id: Some(id),
        parent_id,
        title: proposed.title.clone(),
        details: proposed.details.clone(),
        state: Some(FeatureState::Proposed),
        priority: Some(proposed.priority),
        target_version_id,
    });

    // Recursively flatten children with this feature's ID as parent
    for child in &proposed.children {
        flatten_feature_tree(Some(id), child, target_version_id, inputs, created_ids);
    }
}

/// Count total features in a proposed feature tree (including nested children).
fn count_proposed_features(features: &[ProposedFeature]) -> usize {
    features
        .iter()
        .map(|f| 1 + count_proposed_features(&f.children))
        .sum()
}

/// Subscribe to real-time feature change notifications via SSE.
pub async fn subscribe_project_features(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let project_id: ProjectId = project_id.into();
    let rx = db.subscribe();

    let stream = BroadcastStream::new(rx).filter_map(move |result| {
        let val = match result {
            Ok(event) if event.project_id() == project_id => {
                // Emit a simple "change" event - client will refetch
                Some(Ok(Event::default().event("change").data("feature_changed")))
            }
            Ok(_) => None,  // Different project, ignore
            Err(_) => None, // Lagged, ignore (client will catch up on next event)
        };
        std::future::ready(val)
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
