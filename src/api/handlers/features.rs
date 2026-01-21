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
    FeatureState, FeatureSummary, FeatureTreeNode, HistoryDetails, ListFeaturesQuery,
    UpdateFeatureInput,
};

use super::internal_error;
use crate::serde_helpers::default_true;

// ============================================================
// Features
// ============================================================

/// List all features with optional pagination.
pub async fn list_features(
    State(db): State<Database>,
    Query(query): Query<ListFeaturesQuery>,
) -> Result<Json<Vec<FeatureSummary>>, (StatusCode, String)> {
    // Use SQL-based pagination for efficiency
    let features = db
        .get_all_features_paginated(query.limit, query.offset)
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
) -> Result<Json<Vec<FeatureSummary>>, (StatusCode, String)> {
    // Use SQL-based pagination for efficiency
    let features = db
        .get_features_by_project_paginated(project_id, query.limit, query.offset)
        .map_err(internal_error)?;
    // Always return summaries only - use get_feature for full details
    let summaries: Vec<FeatureSummary> = features.into_iter().map(Into::into).collect();
    Ok(Json(summaries))
}

/// List top-level (root) features for a project.
pub async fn list_root_features(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<Feature>>, (StatusCode, String)> {
    db.get_root_features(project_id)
        .map(Json)
        .map_err(internal_error)
}

/// Get the complete hierarchical feature tree for a project.
pub async fn get_feature_tree(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<FeatureTreeNode>>, (StatusCode, String)> {
    db.get_feature_tree(project_id)
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
/// - Features targeting "now" version (first unreleased) come first
/// - Then features with no version (backlog)
/// - Within each group: lower priority number wins
/// - Same priority: oldest created wins
pub async fn get_next_feature(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Query(query): Query<GetNextFeatureQuery>,
) -> Result<Json<Option<crate::models::FeatureWithContext>>, (StatusCode, String)> {
    // Get the next workable feature
    let feature = db
        .get_next_workable_feature(project_id, query.version_id)
        .map_err(internal_error)?;

    // If we found a feature, enrich it with context
    match feature {
        Some(f) => {
            let feature_with_context = db.get_feature_with_context(f.id).map_err(internal_error)?;
            Ok(Json(feature_with_context))
        }
        None => Ok(Json(None)),
    }
}

/// List direct child features of a parent feature.
pub async fn list_children(
    State(db): State<Database>,
    Path(parent_id): Path<Uuid>,
) -> Result<Json<Vec<Feature>>, (StatusCode, String)> {
    db.get_children(parent_id).map(Json).map_err(internal_error)
}

/// Get implementation history entries for a feature.
pub async fn get_feature_history(
    State(db): State<Database>,
    Path(feature_id): Path<Uuid>,
) -> Result<Json<Vec<FeatureHistory>>, (StatusCode, String)> {
    db.get_feature_history(feature_id)
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
) -> Result<(StatusCode, Json<FeatureHistory>), (StatusCode, String)> {
    // Verify feature exists
    let feature = db
        .get_feature(feature_id)
        .map_err(internal_error)?
        .ok_or((StatusCode::NOT_FOUND, "Feature not found".to_string()))?;

    // Verify it's a leaf feature
    if !db.is_leaf(feature_id).map_err(internal_error)? {
        return Err((
            StatusCode::BAD_REQUEST,
            "Cannot create history on a non-leaf feature".to_string(),
        ));
    }

    // Create history entry directly
    // If version_id not provided, database layer defaults to feature's target_version_id
    let history = db
        .create_history_entry(CreateHistoryInput {
            feature_id,
            version_id: input.version_id,
            details: HistoryDetails {
                summary: input.summary,
                commits: input.commits,
            },
        })
        .map_err(internal_error)?;

    // Optionally update feature state to implemented
    if input.mark_implemented && feature.state != FeatureState::Implemented {
        db.update_feature(
            feature_id,
            UpdateFeatureInput {
                parent_id: None,
                title: None,
                details: None,
                desired_details: None,
                state: Some(FeatureState::Implemented),
                priority: None,
                target_version_id: None,
            },
        )
        .map_err(internal_error)?;
    }

    Ok((StatusCode::CREATED, Json(history)))
}

/// Get a feature by ID.
pub async fn get_feature(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<Feature>, (StatusCode, String)> {
    db.get_feature(id)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Feature not found".to_string()))
}

/// Get a feature with hierarchical context (parent, siblings, children, breadcrumb).
///
/// This endpoint provides AI agents with navigation context to understand where
/// a feature sits in the feature tree.
pub async fn get_feature_with_context(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<crate::models::FeatureWithContext>, (StatusCode, String)> {
    db.get_feature_with_context(id)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Feature not found".to_string()))
}

/// Get the diff between current and desired details for a feature.
pub async fn get_feature_diff(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<FeatureDiff>, (StatusCode, String)> {
    db.get_feature_diff(id)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Feature not found".to_string()))
}

/// Create a new feature in a project.
pub async fn create_feature(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<CreateFeatureInput>,
) -> Result<(StatusCode, Json<Feature>), (StatusCode, String)> {
    db.create_feature(project_id, input)
        .map(|f| (StatusCode::CREATED, Json(f)))
        .map_err(internal_error)
}

/// Update an existing feature.
pub async fn update_feature(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateFeatureInput>,
) -> Result<Json<Feature>, (StatusCode, String)> {
    db.update_feature(id, input)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Feature not found".to_string()))
}

/// Delete a feature and its descendants.
pub async fn delete_feature(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    if db.delete_feature(id).map_err(internal_error)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "Feature not found".to_string()))
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
) -> Result<Json<Vec<FeatureSummary>>, (StatusCode, String)> {
    db.search_features(&query.q, query.project_id, query.limit)
        .map(Json)
        .map_err(internal_error)
}

// ============================================================
// Bulk Feature Creation (for MCP plan_features)
// ============================================================

/// Input for bulk feature creation.
#[derive(Debug, Deserialize)]
pub struct BulkCreateFeaturesInput {
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
) -> Result<Json<PlanFeaturesResponse>, (StatusCode, String)> {
    // Verify project exists
    db.get_project(project_id)
        .map_err(internal_error)?
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;

    let mut created_ids = Vec::new();

    if input.confirm {
        // Flatten the tree into a list of inputs with pre-generated UUIDs
        // This allows us to use the transactional bulk insert
        let mut feature_inputs = Vec::new();
        for feature in &input.features {
            flatten_feature_tree(None, feature, &mut feature_inputs, &mut created_ids);
        }

        // Create all features in a single transaction
        db.create_features_bulk(project_id, feature_inputs)
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
    parent_id: Option<Uuid>,
    proposed: &ProposedFeature,
    inputs: &mut Vec<CreateFeatureInput>,
    created_ids: &mut Vec<Uuid>,
) -> Uuid {
    let id = Uuid::new_v4();
    created_ids.push(id);

    inputs.push(CreateFeatureInput {
        id: Some(id),
        parent_id,
        title: proposed.title.clone(),
        details: proposed.details.clone(),
        state: Some(FeatureState::InProgress),
        priority: Some(proposed.priority),
        target_version_id: None,
    });

    // Recursively flatten children with this feature's ID as parent
    for child in &proposed.children {
        flatten_feature_tree(Some(id), child, inputs, created_ids);
    }

    id
}

/// Subscribe to real-time feature change notifications via SSE.
pub async fn subscribe_project_features(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
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
