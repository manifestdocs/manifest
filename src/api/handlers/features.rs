//! Feature CRUD and lifecycle endpoints.
//!
//! Handles creation, retrieval, update, deletion, and state transitions for
//! features and their tree hierarchy. Also provides SSE subscriptions for
//! real-time change notifications.

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
use validator::Validate;

use crate::db::Database;
use crate::mcp::{PlanFeaturesResponse, ProposedFeature};
use crate::models::{
    BreadcrumbItem, CommitRef, CreateFeatureInput, CreateHistoryInput, Feature, FeatureDiff,
    FeatureHistory, FeatureId, FeatureState, FeatureSummary, FeatureTreeNode, FeatureWithContext,
    HistoryDetails, ListFeaturesQuery, ProjectId, UpdateFeatureInput, VerificationComment,
    VerifyFeatureContextResponse, VersionId,
};

use super::{internal_error, ApiError};
use crate::api::validation::{ValidatedJson, MAX_BULK_FEATURES};

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
        .get_all_features_paginated(query.version_id, query.limit, query.offset)
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
        .get_features_by_project_paginated(
            project_id.into(),
            query.version_id,
            query.limit,
            query.offset,
        )
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
#[derive(Debug, Deserialize, Validate)]
pub struct CreateFeatureHistoryInput {
    #[validate(length(min = 1, max = 10_000))]
    pub summary: String,
    #[serde(default)]
    #[validate(length(max = 200))]
    pub commits: Vec<CommitRef>,
    /// Version this work was done for.
    /// If not specified, defaults to the feature's target_version_id.
    pub version_id: Option<Uuid>,
}

/// Create a history entry directly on a feature.
///
/// Optionally marks the feature as implemented. Only allowed on leaf features.
pub async fn create_feature_history(
    State(db): State<Database>,
    Path(feature_id): Path<Uuid>,
    ValidatedJson(input): ValidatedJson<CreateFeatureHistoryInput>,
) -> Result<(StatusCode, Json<FeatureHistory>), ApiError> {
    let feature_id = FeatureId::from(feature_id);
    let feature = require_leaf_feature(&db, feature_id).await?;
    let history = create_history_entry_from_input(&db, feature_id, input).await?;
    sync_feature_state_after_history(&db, feature_id, &feature).await?;

    Ok((StatusCode::CREATED, Json(history)))
}

async fn require_leaf_feature(db: &Database, feature_id: FeatureId) -> Result<Feature, ApiError> {
    let feature = db
        .get_feature(feature_id)
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::not_found("Feature"))?;

    if !db.is_leaf(feature_id).await.map_err(internal_error)? {
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            "Cannot create history on a non-leaf feature".to_string(),
        )));
    }

    Ok(feature)
}

async fn create_history_entry_from_input(
    db: &Database,
    feature_id: FeatureId,
    input: CreateFeatureHistoryInput,
) -> Result<FeatureHistory, ApiError> {
    db.create_history_entry(CreateHistoryInput {
        feature_id,
        version_id: input.version_id.map(VersionId::from),
        details: HistoryDetails {
            summary: input.summary,
            commits: input.commits,
            ..Default::default()
        },
    })
    .await
    .map_err(internal_error)
}

async fn sync_feature_state_after_history(
    db: &Database,
    feature_id: FeatureId,
    feature: &Feature,
) -> Result<(), ApiError> {
    let needs_state_change = feature.state != FeatureState::Implemented;
    let has_pending_changes = feature.desired_details.is_some();
    if !needs_state_change && !has_pending_changes {
        return Ok(());
    }

    db.update_feature(
        feature_id,
        UpdateFeatureInput {
            parent_id: None,
            title: None,
            details: None,
            desired_details: if has_pending_changes {
                Some(None)
            } else {
                None
            },
            details_summary: None,
            state: if needs_state_change {
                Some(FeatureState::Implemented)
            } else {
                None
            },
            priority: None,
            target_version_id: None,
            blocked_by: None,
        },
    )
    .await
    .map_err(internal_error)?;

    Ok(())
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
        .ok_or(ApiError::not_found("Feature"))
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
        .ok_or(ApiError::not_found("Feature"))
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
        .ok_or(ApiError::not_found("Feature"))
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
        .ok_or(ApiError::not_found("Feature"))
}

/// Get the features that are blocking a given feature.
pub async fn get_feature_blockers(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<FeatureSummary>>, ApiError> {
    // Verify feature exists
    db.get_feature(id.into())
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::not_found("Feature"))?;

    db.get_feature_blockers(id.into())
        .await
        .map(Json)
        .map_err(internal_error)
}

/// Find a blocked ancestor feature set in the parent chain.
/// Returns the first blocked ancestor, or 204 if none found.
pub async fn find_blocked_ancestor(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<Option<BlockedAncestor>>, ApiError> {
    db.find_blocked_ancestor(id.into())
        .await
        .map(|opt| {
            Json(opt.map(|(id, title)| BlockedAncestor {
                id: id.into(),
                title,
            }))
        })
        .map_err(internal_error)
}

#[derive(Debug, serde::Serialize)]
pub struct BlockedAncestor {
    pub id: Uuid,
    pub title: String,
}

/// Delete a feature and its descendants.
pub async fn delete_feature(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    if db.delete_feature(id.into()).await.map_err(internal_error)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("Feature"))
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
    /// ID to resolve: UUID, display ID like "MAN-42", or UUID prefix.
    pub prefix: String,
    /// Optional project UUID to scope the search to.
    pub project_id: Option<Uuid>,
}

/// Resolve a feature by UUID, display ID (MAN-42), or UUID prefix.
/// Returns the matching feature if exactly one match is found.
pub async fn resolve_feature(
    State(db): State<Database>,
    Query(query): Query<ResolveFeatureQuery>,
) -> Result<Json<Feature>, ApiError> {
    let prefix = &query.prefix;

    // 1. Try full UUID
    if let Ok(uuid) = uuid::Uuid::parse_str(prefix) {
        if let Some(f) = db.get_feature(uuid.into()).await.map_err(internal_error)? {
            return Ok(Json(f));
        }
    }

    // 2. Try display ID format (LETTERS-DIGITS)
    if prefix.contains('-') && is_display_id_format(prefix) {
        if let Some(f) = db
            .resolve_feature_by_display_id(prefix)
            .await
            .map_err(internal_error)?
        {
            return Ok(Json(f));
        }
    }

    // 3. Fall back to UUID prefix match
    db.resolve_feature_by_prefix(prefix, query.project_id.map(ProjectId::from))
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or(ApiError::from((
            StatusCode::NOT_FOUND,
            format!("No feature found matching '{}'", prefix),
        )))
}

/// Check if a string matches the display ID format: `LETTERS-DIGITS` (e.g. "AUTH-42").
fn is_display_id_format(s: &str) -> bool {
    s.rsplit_once('-')
        .map(|(p, n)| {
            !p.is_empty()
                && p.chars().all(|c| c.is_ascii_alphabetic())
                && !n.is_empty()
                && n.chars().all(|c| c.is_ascii_digit())
        })
        .unwrap_or(false)
}

// ============================================================
// Bulk Feature Creation (for MCP plan_features)
// ============================================================

/// Input for bulk feature creation.
#[derive(Debug, Deserialize, Validate)]
pub struct BulkCreateFeaturesInput {
    /// The target version for all features. If null, features go to backlog.
    pub target_version_id: Option<Uuid>,
    /// The proposed feature tree.
    #[validate(length(max = 200))]
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
    ValidatedJson(input): ValidatedJson<BulkCreateFeaturesInput>,
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
        .ok_or(ApiError::not_found("Project"))?;

    let mut created_ids = Vec::new();

    if input.confirm {
        // Verify version exists if one was specified
        if let Some(vid) = input.target_version_id {
            db.get_version(vid.into())
                .await
                .map_err(internal_error)?
                .ok_or(ApiError::not_found("Version"))?;
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

    // Resolve the initial state: respect explicit state from proposal (for bootstrapping),
    // default to Proposed for normal planning workflow.
    let state = proposed
        .state
        .as_deref()
        .and_then(|s| std::str::FromStr::from_str(s).ok())
        .unwrap_or(FeatureState::Proposed);

    inputs.push(CreateFeatureInput {
        id: Some(id),
        parent_id,
        title: proposed.title.clone(),
        details: proposed.details.clone(),
        state: Some(state),
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

// ============================================================
// Verification
// ============================================================

/// Maximum diff size in characters before truncation.
const MAX_DIFF_CHARS: usize = 50_000;

/// File path patterns that should be stripped from diffs.
const SKIP_DIFF_PATTERNS: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "node_modules/",
    ".min.js",
    ".min.css",
];

#[derive(Deserialize, Validate)]
pub struct VerifyFeatureBody {
    #[validate(length(max = 250_000))]
    pub diff: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct RecordVerificationBody {
    #[validate(length(max = 200))]
    pub comments: Vec<VerificationComment>,
}

/// POST /features/:id/verify
///
/// Assembles the feature spec + breadcrumb context and optionally filters the provided diff.
/// Returns a structured context for the calling agent to analyze — no LLM call server-side.
pub async fn get_verify_context(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<VerifyFeatureBody>,
) -> Result<Json<VerifyFeatureContextResponse>, ApiError> {
    let ctx = db
        .get_feature_with_context(id.into())
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::not_found("Feature"))?;

    let spec = format_spec_context(&ctx);
    let (diff, diff_truncated) = match body.diff {
        Some(raw) => filter_and_truncate_diff(raw),
        None => (None, false),
    };

    let instructions = concat!(
        "Analyze the diff against the specification above. ",
        "Identify requirements in the spec that are NOT satisfied by the implementation. ",
        "For each gap, call record_verification with severity (critical/major/minor), ",
        "title (one-line summary), body (actionable explanation with suggested fix), ",
        "and file (affected path if known). ",
        "If the implementation fully satisfies the spec, call record_verification ",
        "with an empty comments array."
    )
    .to_string();

    Ok(Json(VerifyFeatureContextResponse {
        spec,
        diff,
        diff_truncated,
        instructions,
    }))
}

/// PUT /features/:id/verification
///
/// Stores agent-generated verification comments on the feature record.
/// Overwrites any previous verification result.
pub async fn record_feature_verification(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<RecordVerificationBody>,
) -> Result<Json<Feature>, ApiError> {
    // Verify feature exists
    db.get_feature(id.into())
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::not_found("Feature"))?;

    let feature = db
        .record_verification(id.into(), &body.comments)
        .await
        .map_err(internal_error)?;

    Ok(Json(feature))
}

/// Format feature spec + breadcrumb ancestors into a markdown string for verification context.
fn format_spec_context(ctx: &FeatureWithContext) -> String {
    let mut spec = String::new();

    // Breadcrumb ancestors (everything except the feature itself, which is last)
    let ancestors: &[BreadcrumbItem] = if ctx.breadcrumb.len() > 1 {
        &ctx.breadcrumb[..ctx.breadcrumb.len() - 1]
    } else {
        &[]
    };

    if !ancestors.is_empty() {
        spec.push_str("## Project Context\n\n");
        for item in ancestors {
            spec.push_str(&format!("### {}\n\n", item.title));
            if let Some(details) = &item.details {
                spec.push_str(details);
                spec.push_str("\n\n");
            }
        }
    }

    spec.push_str(&format!(
        "## Feature Specification: {}\n\n",
        ctx.feature.title
    ));
    match &ctx.feature.details {
        Some(details) => spec.push_str(details),
        None => spec.push_str("*No specification provided.*"),
    }

    spec
}

/// Filter out noise files from a diff and truncate to size limit.
/// Returns (filtered_diff, was_truncated).
fn filter_and_truncate_diff(diff: String) -> (Option<String>, bool) {
    if diff.is_empty() {
        return (None, false);
    }

    let filtered = filter_diff_files(&diff);
    if filtered.is_empty() {
        return (None, false);
    }

    if filtered.len() <= MAX_DIFF_CHARS {
        (Some(filtered), false)
    } else {
        (
            Some(truncate_at_file_boundary(&filtered, MAX_DIFF_CHARS)),
            true,
        )
    }
}

/// Remove lock files and other noise from a unified diff.
fn filter_diff_files(diff: &str) -> String {
    let mut result = String::new();
    let mut current_file = String::new();
    let mut skip_current = false;

    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            if !skip_current {
                result.push_str(&current_file);
            }
            current_file = format!("{line}\n");
            skip_current = SKIP_DIFF_PATTERNS.iter().any(|p| line.contains(p));
        } else {
            current_file.push_str(line);
            current_file.push('\n');
        }
    }
    if !skip_current {
        result.push_str(&current_file);
    }
    result
}

/// Truncate a diff at a file boundary to stay within the character limit.
fn truncate_at_file_boundary(diff: &str, max_chars: usize) -> String {
    let mut result = String::new();
    let mut current_file = String::new();

    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            if result.len() + current_file.len() > max_chars {
                break;
            }
            result.push_str(&current_file);
            current_file = format!("{line}\n");
        } else {
            current_file.push_str(line);
            current_file.push('\n');
        }
    }
    if result.len() + current_file.len() <= max_chars {
        result.push_str(&current_file);
    }
    result
}

// ============================================================
// Claim Management
// ============================================================

/// Input for setting a feature claim.
#[derive(Debug, Deserialize, Validate)]
pub struct SetClaimInput {
    #[validate(length(min = 1, max = 100))]
    pub agent_type: String,
    #[validate(length(max = 10_000))]
    pub metadata: Option<String>,
    /// Force claim even if another agent holds it. Default false.
    #[serde(default)]
    pub force: bool,
}

/// PUT /features/:id/claim — atomically claim a feature.
///
/// Returns 200 on success, 409 Conflict with structured body if another agent
/// already holds a claim (unless force=true).
pub async fn set_feature_claim(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
    ValidatedJson(input): ValidatedJson<SetClaimInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    db.claim_feature_atomic(
        id.into(),
        &input.agent_type,
        input.metadata.as_deref(),
        input.force,
    )
    .await
    .map_err(internal_error)?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ============================================================
// Feature Completion
// ============================================================

/// Input for completing a feature.
#[derive(Debug, Deserialize, Validate)]
pub struct CompleteFeatureInput {
    #[validate(length(min = 1, max = 10_000))]
    pub summary: String,
    #[serde(default)]
    #[validate(length(max = 200))]
    pub commits: Vec<CommitRef>,
    /// When true, skips proof and spec requirements. Used for bootstrapping existing projects
    /// where the code predates Manifest. History entry is tagged as "backfilled".
    #[serde(default)]
    pub backfill: bool,
}

/// POST /features/:id/complete — complete a feature (create history + update state + clear claims).
pub async fn complete_feature(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
    ValidatedJson(input): ValidatedJson<CompleteFeatureInput>,
) -> Result<(StatusCode, Json<CompleteFeatureResponse>), ApiError> {
    let result = db
        .complete_feature(id.into(), &input.summary, &input.commits, input.backfill)
        .await
        .map_err(internal_error)?;

    Ok((
        StatusCode::CREATED,
        Json(CompleteFeatureResponse {
            feature: result.feature,
            history: result.history,
            warnings: result.warnings,
        }),
    ))
}

/// Response for the complete_feature endpoint.
#[derive(Debug, serde::Serialize)]
pub struct CompleteFeatureResponse {
    pub feature: Feature,
    pub history: FeatureHistory,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Subscribe to real-time feature change notifications via SSE.
pub async fn subscribe_project_features(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let project_id: ProjectId = project_id.into();
    let rx = db.subscribe();

    let stream = BroadcastStream::new(rx)
        .filter_map(move |result| std::future::ready(feature_event_to_sse(result, project_id)));

    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn feature_event_to_sse(
    result: Result<
        crate::db::FeatureEvent,
        tokio_stream::wrappers::errors::BroadcastStreamRecvError,
    >,
    project_id: ProjectId,
) -> Option<Result<Event, Infallible>> {
    let event = result.ok()?;
    if event.project_id() != project_id {
        return None;
    }
    use crate::db::FeatureEvent;
    match event {
        FeatureEvent::Completed {
            feature_title,
            agent_type,
            ..
        } => {
            let payload = serde_json::json!({
                "feature_title": feature_title,
                "agent_type": agent_type,
            });
            Some(Ok(Event::default()
                .event("feature_completed")
                .data(payload.to_string())))
        }
        _ => Some(Ok(Event::default().event("change").data("feature_changed"))),
    }
}
