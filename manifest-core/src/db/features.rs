use std::str::FromStr;

use anyhow::Result;
use chrono::Utc;
use sqlx::Row;

use super::helpers::*;
use super::{
    escape_like_pattern, Database, FeatureEvent, ManifestError, RootFeatureMigrationReport,
};
use crate::models::*;

/// SELECT columns for the features table (bare names).
const FEATURE_COLS: &str = "id, project_id, parent_id, title, details, desired_details, details_summary, state, priority, feature_number, target_version_id, verification_result, verified_at, claimed_by, claimed_at, claim_metadata, created_at, updated_at";

/// SELECT columns for the features table with `f.` table alias.
const FEATURE_COLS_F: &str = "f.id, f.project_id, f.parent_id, f.title, f.details, f.desired_details, f.details_summary, f.state, f.priority, f.feature_number, f.target_version_id, f.verification_result, f.verified_at, f.claimed_by, f.claimed_at, f.claim_metadata, f.created_at, f.updated_at";

/// Result of completing a feature, including advisory warnings.
#[derive(Debug)]
pub struct CompletionResult {
    pub feature: Feature,
    pub history: FeatureHistory,
    pub warnings: Vec<String>,
}

impl Database {
    /// Get all features across all projects with optional pagination and version filter.
    pub async fn get_all_features_paginated(
        &self,
        version_id: Option<VersionId>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<Feature>> {
        let mut next_param = 1u32;
        let mut sql = format!("SELECT {FEATURE_COLS} FROM features");
        if version_id.is_some() {
            sql.push_str(&format!(" WHERE target_version_id = ${next_param}"));
            next_param += 1;
        }
        sql.push_str(" ORDER BY priority, title");
        append_pagination(&mut sql, limit, offset, next_param, &self.dialect);

        let mut q = sqlx::query(&sql);
        if let Some(vid) = version_id {
            q = q.bind(vid.to_string());
        }
        if let Some(lim) = limit {
            q = q.bind(lim as i64);
        }
        if let Some(off) = offset {
            q = q.bind(off as i64);
        }
        let rows = q.fetch_all(&self.pool).await?;
        let mut features: Vec<Feature> = rows.iter().map(row_to_feature).collect::<Result<_>>()?;
        self.resolve_derived_states(&mut features).await?;
        Ok(features)
    }

    /// Get all features across all projects without pagination.
    pub async fn get_all_features(&self) -> Result<Vec<Feature>> {
        self.get_all_features_paginated(None, None, None).await
    }

    /// Get all features for a project with optional pagination and version filter.
    pub async fn get_features_by_project_paginated(
        &self,
        project_id: ProjectId,
        version_id: Option<VersionId>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<Feature>> {
        let mut next_param = 2u32;
        let mut sql = format!(
            "SELECT {FEATURE_COLS} FROM features WHERE project_id = $1"
        );
        if version_id.is_some() {
            sql.push_str(&format!(" AND target_version_id = ${next_param}"));
            next_param += 1;
        }
        sql.push_str(" ORDER BY priority, title");
        append_pagination(&mut sql, limit, offset, next_param, &self.dialect);

        let mut q = sqlx::query(&sql).bind(project_id.to_string());
        if let Some(vid) = version_id {
            q = q.bind(vid.to_string());
        }
        if let Some(lim) = limit {
            q = q.bind(lim as i64);
        }
        if let Some(off) = offset {
            q = q.bind(off as i64);
        }
        let rows = q.fetch_all(&self.pool).await?;
        let mut features: Vec<Feature> = rows.iter().map(row_to_feature).collect::<Result<_>>()?;
        self.resolve_derived_states(&mut features).await?;
        Ok(features)
    }

    /// Get all features for a project without pagination.
    pub async fn get_features_by_project(&self, project_id: ProjectId) -> Result<Vec<Feature>> {
        self.get_features_by_project_paginated(project_id, None, None, None)
            .await
    }

    /// Get a single feature by ID.
    pub async fn get_feature(&self, id: FeatureId) -> Result<Option<Feature>> {
        let sql = format!("SELECT {FEATURE_COLS} FROM features WHERE id = $1");
        let row = sqlx::query(&sql)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;

        match row.as_ref().map(row_to_feature).transpose()? {
            Some(mut feature) => {
                self.resolve_derived_state_single(&mut feature).await?;
                Ok(Some(feature))
            }
            None => Ok(None),
        }
    }

    /// Resolve a feature by UUID prefix (e.g., first 8 chars from short IDs).
    ///
    /// If `project_id` is provided, scopes the search to that project.
    /// Returns `Ok(Some(feature))` if exactly one feature matches the prefix.
    /// Returns `Ok(None)` if no features match.
    /// Returns `Err` if multiple features match (ambiguous prefix).
    pub async fn resolve_feature_by_prefix(
        &self,
        prefix: &str,
        project_id: Option<ProjectId>,
    ) -> Result<Option<Feature>> {
        let pattern = format!("{}%", prefix);
        let rows = match project_id {
            Some(pid) => {
                let sql = format!(
                    "SELECT {FEATURE_COLS} FROM features WHERE id LIKE $1 AND project_id = $2 LIMIT 2"
                );
                sqlx::query(&sql)
                    .bind(&pattern)
                    .bind(pid.to_string())
                    .fetch_all(&self.pool)
                    .await?
            }
            None => {
                let sql = format!("SELECT {FEATURE_COLS} FROM features WHERE id LIKE $1 LIMIT 2");
                sqlx::query(&sql)
                    .bind(&pattern)
                    .fetch_all(&self.pool)
                    .await?
            }
        };

        match rows.len() {
            0 => Ok(None),
            1 => {
                let mut feature = row_to_feature(&rows[0])?;
                self.resolve_derived_state_single(&mut feature).await?;
                Ok(Some(feature))
            }
            _ => Err(anyhow::anyhow!(
                "Ambiguous prefix '{}': matches multiple features",
                prefix
            )),
        }
    }

    /// Resolve a feature by display ID (e.g., "MAN-42").
    ///
    /// Parses the display ID into a key prefix and feature number, then looks up
    /// the project by key_prefix and the feature by project_id + feature_number.
    pub async fn resolve_feature_by_display_id(&self, display_id: &str) -> Result<Option<Feature>> {
        // Parse "PREFIX-NUMBER" format
        let Some((prefix, number_str)) = display_id.rsplit_once('-') else {
            return Ok(None);
        };
        let Ok(feature_number) = number_str.parse::<i32>() else {
            return Ok(None);
        };
        let prefix_upper = prefix.to_ascii_uppercase();

        // Look up project by key_prefix (case-insensitive via UPPER)
        let project_id: Option<String> =
            sqlx::query_scalar("SELECT id FROM projects WHERE UPPER(key_prefix) = $1")
                .bind(&prefix_upper)
                .fetch_optional(&self.pool)
                .await?;

        let Some(project_id) = project_id else {
            return Ok(None);
        };

        // Look up feature by project_id + feature_number
        let sql = format!(
            "SELECT {FEATURE_COLS} FROM features WHERE project_id = $1 AND feature_number = $2"
        );
        let row = sqlx::query(&sql)
            .bind(&project_id)
            .bind(feature_number)
            .fetch_optional(&self.pool)
            .await?;

        match row.as_ref().map(row_to_feature).transpose()? {
            Some(mut feature) => {
                self.resolve_derived_state_single(&mut feature).await?;
                Ok(Some(feature))
            }
            None => Ok(None),
        }
    }

    /// Get the diff between a feature's current and desired details.
    pub async fn get_feature_diff(&self, id: FeatureId) -> Result<Option<FeatureDiff>> {
        let feature = match self.get_feature(id).await? {
            Some(f) => f,
            None => return Ok(None),
        };

        let has_changes =
            feature.desired_details.is_some() && feature.desired_details != feature.details;

        Ok(Some(FeatureDiff {
            has_changes,
            current: feature.details,
            desired: feature.desired_details,
        }))
    }

    /// Create a new feature within a project.
    ///
    /// Features are automatically parented under the project's root feature if no parent is specified.
    /// In-progress or implemented features are auto-assigned to the "next" version.
    pub async fn create_feature(
        &self,
        project_id: ProjectId,
        input: CreateFeatureInput,
    ) -> Result<Feature> {
        let project = self
            .get_project(project_id)
            .await?
            .ok_or_else(|| ManifestError::not_found("Project"))?;

        // Guard rail: features must always have a parent
        let parent_id = input
            .parent_id
            .or(project.root_feature_id)
            .ok_or_else(|| ManifestError::validation("Feature must have a parent"))?;

        let id = input.id.unwrap_or_default();
        let now = Utc::now();
        let state = input.state.unwrap_or(FeatureState::Proposed);
        let priority = input.priority.unwrap_or(0);

        // Guard rail: reject assignment to released versions
        if let Some(vid) = input.target_version_id {
            self.validate_version_not_released(vid).await?;
        }

        // Guard rail: in-progress/implemented features must be in the "next" version
        let target_version_id =
            if state == FeatureState::InProgress || state == FeatureState::Implemented {
                let next_version = self.get_next_version(project_id).await?.map(|v| v.id);
                next_version.or(input.target_version_id)
            } else {
                match input.target_version_id {
                    Some(vid) => Some(vid),
                    None => {
                        if project.default_feature_destination == "next" {
                            self.get_next_version(project_id).await?.map(|v| v.id)
                        } else {
                            None // backlog
                        }
                    }
                }
            };

        // Assign next sequential feature_number
        let feature_number: i32 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(feature_number), 0) + 1 FROM features WHERE project_id = $1",
        )
        .bind(project_id.to_string())
        .fetch_one(&self.pool)
        .await?;

        sqlx::query(
            "INSERT INTO features (id, project_id, parent_id, title, details, state, priority, feature_number, target_version_id, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(id.to_string())
        .bind(project_id.to_string())
        .bind(Some(parent_id.to_string()))
        .bind(&input.title)
        .bind(&input.details)
        .bind(state.as_str())
        .bind(priority)
        .bind(feature_number)
        .bind(target_version_id.map(|u| u.to_string()))
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        let _ = self.events.send(FeatureEvent::Created { project_id });

        Ok(Feature {
            id,
            project_id,
            parent_id: Some(parent_id),
            title: input.title,
            details: input.details,
            desired_details: None,
            details_summary: None,
            state,
            priority,
            feature_number: Some(feature_number),
            target_version_id,
            verification_result: None,
            verified_at: None,
            claimed_by: None,
            claimed_at: None,
            claim_metadata: None,
            created_at: now,
            updated_at: now,
        })
    }

    /// Create multiple features in a single transaction.
    pub async fn create_features_bulk(
        &self,
        project_id: ProjectId,
        inputs: Vec<CreateFeatureInput>,
    ) -> Result<Vec<Feature>> {
        let project = self
            .get_project(project_id)
            .await?
            .ok_or_else(|| ManifestError::not_found("Project"))?;

        let now = Utc::now();
        let mut features = Vec::with_capacity(inputs.len());

        // Get default version and "next" version based on project setting
        let next_version_id = self.get_next_version(project_id).await?.map(|v| v.id);
        let default_version_id = if project.default_feature_destination == "next" {
            next_version_id
        } else {
            None // backlog
        };

        // Guard rail: reject assignment to released versions (validate once per unique version)
        let mut validated_versions = std::collections::HashSet::new();
        for input in &inputs {
            if let Some(vid) = input.target_version_id {
                if validated_versions.insert(vid) {
                    self.validate_version_not_released(vid).await?;
                }
            }
        }

        let mut tx = self.pool.begin().await?;

        // Get the starting feature_number for this batch
        let mut next_number: i32 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(feature_number), 0) + 1 FROM features WHERE project_id = $1",
        )
        .bind(project_id.to_string())
        .fetch_one(&mut *tx)
        .await?;

        for input in inputs {
            let id = input.id.unwrap_or_default();
            let state = input.state.unwrap_or(FeatureState::Proposed);
            let priority = input.priority.unwrap_or(0);

            // Guard rail: features must always have a parent
            let parent_id = input
                .parent_id
                .or(project.root_feature_id)
                .ok_or_else(|| ManifestError::validation("Feature must have a parent"))?;

            // Guard rail: in-progress/implemented features must be in the "next" version
            let target_version_id =
                if state == FeatureState::InProgress || state == FeatureState::Implemented {
                    next_version_id.or(input.target_version_id)
                } else {
                    input.target_version_id.or(default_version_id)
                };

            let feature_number = next_number;
            next_number += 1;

            sqlx::query(
                "INSERT INTO features (id, project_id, parent_id, title, details, state, priority, feature_number, target_version_id, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            )
            .bind(id.to_string())
            .bind(project_id.to_string())
            .bind(Some(parent_id.to_string()))
            .bind(&input.title)
            .bind(&input.details)
            .bind(state.as_str())
            .bind(priority)
            .bind(feature_number)
            .bind(target_version_id.map(|u| u.to_string()))
            .bind(now.to_rfc3339())
            .bind(now.to_rfc3339())
            .execute(&mut *tx)
            .await?;

            features.push(Feature {
                id,
                project_id,
                parent_id: Some(parent_id),
                title: input.title,
                details: input.details,
                desired_details: None,
                details_summary: None,
                state,
                priority,
                feature_number: Some(feature_number),
                target_version_id,
                verification_result: None,
                verified_at: None,
                claimed_by: None,
                claimed_at: None,
                claim_metadata: None,
                created_at: now,
                updated_at: now,
            });
        }

        tx.commit().await?;

        let _ = self.events.send(FeatureEvent::Created { project_id });

        Ok(features)
    }

    /// Update an existing feature's fields.
    ///
    /// Setting `desired_details` (proposing changes) suppresses any concurrent state change
    /// to prevent agents from accidentally advancing state alongside a proposal.
    pub async fn update_feature(
        &self,
        id: FeatureId,
        input: UpdateFeatureInput,
    ) -> Result<Option<Feature>> {
        let Some(existing) = self.get_feature(id).await? else {
            return Ok(None);
        };

        let now = Utc::now();
        let title = input.title.unwrap_or(existing.title);
        let mut details = input.details.or(existing.details);
        let details_summary = input.details_summary.unwrap_or(existing.details_summary);

        // Guard rail: desired_details is only for proposing changes to implemented features.
        // For non-implemented features, redirect desired_details to details directly —
        // there's nothing to "review", the spec should just be edited.
        let (desired_details, is_proposing_changes) = if existing.state != FeatureState::Implemented
        {
            if let Some(Some(dd)) = input.desired_details {
                // Apply directly to details instead of desired_details
                details = Some(dd);
                (existing.desired_details, false)
            } else {
                (
                    input.desired_details.unwrap_or(existing.desired_details),
                    false,
                )
            }
        } else {
            let had_desired_details = existing.desired_details.is_some();
            let mut desired_details = input.desired_details.unwrap_or(existing.desired_details);
            if let Some(ref dd) = desired_details {
                if Some(dd.as_str()) == details.as_deref() {
                    desired_details = None;
                }
            }
            let is_proposing = desired_details.is_some() && !had_desired_details;
            (desired_details, is_proposing)
        };
        // Guard rail: feature sets (parents with children) do not have mutable state.
        // Exception: proposed <-> blocked transitions ARE allowed on feature sets
        // (blocking a set prevents starting any of its children).
        if input.state.is_some() && !self.is_leaf(id).await? {
            let is_blocked_transition = matches!(
                (existing.state, input.state),
                (FeatureState::Proposed, Some(FeatureState::Blocked))
                    | (FeatureState::Blocked, Some(FeatureState::Proposed))
            );
            if !is_blocked_transition {
                return Err(ManifestError::invalid_state(
                    "Cannot change state on a feature set. Feature sets group related capabilities — only leaf features have mutable state. To work on this area, start one of its child features instead."
                ).into());
            }
        }

        // Guard rail: blocked state transitions
        if let Some(new_state) = input.state {
            if new_state == FeatureState::Blocked {
                // Only proposed features can be blocked
                if existing.state != FeatureState::Proposed {
                    return Err(ManifestError::invalid_state(
                        "Only proposed features can be blocked. Current state must be 'proposed'.",
                    )
                    .into());
                }
                // Must provide blocker IDs
                let blocker_ids = input.blocked_by.as_deref().unwrap_or(&[]);
                if blocker_ids.is_empty() {
                    return Err(ManifestError::validation(
                        "blocked_by must contain at least one feature ID when transitioning to blocked.",
                    )
                    .into());
                }
                // Validate: no self-references, all in same project
                for bid in blocker_ids {
                    if *bid == id {
                        return Err(
                            ManifestError::validation("A feature cannot block itself.").into()
                        );
                    }
                }
                let blocker_id_strs: Vec<String> =
                    blocker_ids.iter().map(|b| b.to_string()).collect();
                let placeholders: String = blocker_id_strs
                    .iter()
                    .map(|id| format!("'{}'", id))
                    .collect::<Vec<_>>()
                    .join(", ");
                let count: i64 = sqlx::query_scalar(&format!(
                    "SELECT COUNT(*) FROM features WHERE id IN ({placeholders}) AND project_id = $1"
                ))
                .bind(existing.project_id.to_string())
                .fetch_one(&self.pool)
                .await?;
                if count != blocker_ids.len() as i64 {
                    return Err(ManifestError::validation(
                        "All blocker features must exist in the same project.",
                    )
                    .into());
                }
            }
            if existing.state == FeatureState::Blocked && new_state != FeatureState::Blocked {
                // Unblocking: only allowed transition is blocked -> proposed
                if new_state != FeatureState::Proposed {
                    return Err(ManifestError::invalid_state(
                        "Blocked features can only transition to 'proposed'. Unblock first, then change state.",
                    )
                    .into());
                }
            }
        }
        let state = if is_proposing_changes {
            existing.state
        } else {
            input.state.unwrap_or(existing.state)
        };
        let parent_id = input.parent_id.or(existing.parent_id);
        let priority = input.priority.unwrap_or(existing.priority);
        // Guard rail: reject explicit assignment to released versions
        if let Some(Some(vid)) = &input.target_version_id {
            self.validate_version_not_released(*vid).await?;
        }

        let mut target_version_id = input
            .target_version_id
            .unwrap_or(existing.target_version_id);

        // Guard rail: in-progress features must always be in the "next" version
        if state == FeatureState::InProgress && existing.state != FeatureState::InProgress {
            if let Some(next_ver) = self.get_next_version(existing.project_id).await? {
                target_version_id = Some(next_ver.id);
            }
        }

        // Guard rail: implemented features must have a version (assign to "next" if none)
        if state == FeatureState::Implemented && target_version_id.is_none() {
            target_version_id = self
                .get_next_version(existing.project_id)
                .await?
                .map(|v| v.id);
        }

        sqlx::query(
            "UPDATE features SET parent_id = $1, title = $2, details = $3, desired_details = $4, details_summary = $5, state = $6, priority = $7, target_version_id = $8, updated_at = $9 WHERE id = $10",
        )
        .bind(parent_id.map(|u| u.to_string()))
        .bind(&title)
        .bind(&details)
        .bind(&desired_details)
        .bind(&details_summary)
        .bind(state.as_str())
        .bind(priority)
        .bind(target_version_id.map(|u| u.to_string()))
        .bind(now.to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;

        // Handle blocker storage
        if state == FeatureState::Blocked {
            if let Some(ref blocker_ids) = input.blocked_by {
                self.set_feature_blockers(id, blocker_ids).await?;
            }
        }
        // Clear blockers when unblocking
        if existing.state == FeatureState::Blocked && state == FeatureState::Proposed {
            self.clear_feature_blockers(id).await?;
        }

        // Auto-resolve: when a feature becomes implemented, check if any blocked features can be unblocked
        if state == FeatureState::Implemented && existing.state != FeatureState::Implemented {
            self.auto_resolve_blocked_features(id).await?;
        }

        let _ = self.events.send(FeatureEvent::Updated {
            project_id: existing.project_id,
        });

        let mut feature = Feature {
            id,
            project_id: existing.project_id,
            parent_id,
            title,
            details,
            desired_details,
            details_summary,
            state,
            priority,
            feature_number: existing.feature_number,
            target_version_id,
            verification_result: existing.verification_result,
            verified_at: existing.verified_at,
            claimed_by: existing.claimed_by,
            claimed_at: existing.claimed_at,
            claim_metadata: existing.claim_metadata,
            created_at: existing.created_at,
            updated_at: now,
        };
        self.resolve_derived_state_single(&mut feature).await?;
        Ok(Some(feature))
    }

    /// Delete a feature and all its descendants recursively.
    #[must_use = "check whether the feature existed"]
    pub async fn delete_feature(&self, id: FeatureId) -> Result<bool> {
        // Get project_id before deleting
        let project_id: Option<ProjectId> =
            sqlx::query_scalar("SELECT project_id FROM features WHERE id = $1")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await?
                .map(|s: String| parse_id::<ProjectId>(s))
                .transpose()?;

        let id_str = id.to_string();

        // Delete feature history for descendants (recursive CTE)
        sqlx::query(
            "DELETE FROM feature_history WHERE feature_id IN (
                WITH RECURSIVE descendants AS (
                    SELECT id FROM features WHERE id = $1
                    UNION ALL
                    SELECT f.id FROM features f
                    INNER JOIN descendants d ON f.parent_id = d.id
                )
                SELECT id FROM descendants
            )",
        )
        .bind(&id_str)
        .execute(&self.pool)
        .await?;

        // Delete descendants and feature
        let result = sqlx::query(
            "DELETE FROM features WHERE id IN (
                WITH RECURSIVE descendants AS (
                    SELECT id FROM features WHERE id = $1
                    UNION ALL
                    SELECT f.id FROM features f
                    INNER JOIN descendants d ON f.parent_id = d.id
                )
                SELECT id FROM descendants
            )",
        )
        .bind(&id_str)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() > 0 {
            if let Some(project_id) = project_id {
                let _ = self.events.send(FeatureEvent::Deleted { project_id });
            }
        }

        Ok(result.rows_affected() > 0)
    }

    /// Get the top-level features for a project (direct children of the root feature).
    pub async fn get_root_features(&self, project_id: ProjectId) -> Result<Vec<Feature>> {
        let project = self.get_project(project_id).await?;

        let rows = match project.and_then(|p| p.root_feature_id) {
            Some(root_id) => {
                let sql = format!(
                    "SELECT {FEATURE_COLS} FROM features WHERE project_id = $1 AND parent_id = $2 ORDER BY priority, title"
                );
                sqlx::query(&sql)
                    .bind(project_id.to_string())
                    .bind(root_id.to_string())
                    .fetch_all(&self.pool)
                    .await?
            }
            None => {
                let sql = format!(
                    "SELECT {FEATURE_COLS} FROM features WHERE project_id = $1 AND parent_id IS NULL ORDER BY priority, title"
                );
                sqlx::query(&sql)
                    .bind(project_id.to_string())
                    .fetch_all(&self.pool)
                    .await?
            }
        };

        let mut features: Vec<Feature> = rows.iter().map(row_to_feature).collect::<Result<_>>()?;
        self.resolve_derived_states(&mut features).await?;
        Ok(features)
    }

    /// Get the direct children of a feature.
    pub async fn get_children(&self, parent_id: FeatureId) -> Result<Vec<Feature>> {
        let sql = format!(
            "SELECT {FEATURE_COLS} FROM features WHERE parent_id = $1 ORDER BY priority, title"
        );
        let rows = sqlx::query(&sql)
            .bind(parent_id.to_string())
            .fetch_all(&self.pool)
            .await?;

        let mut features: Vec<Feature> = rows.iter().map(row_to_feature).collect::<Result<_>>()?;
        self.resolve_derived_states(&mut features).await?;
        Ok(features)
    }

    /// Check whether a feature is a leaf node (has no children).
    pub async fn is_leaf(&self, feature_id: FeatureId) -> Result<bool> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM features WHERE parent_id = $1")
            .bind(feature_id.to_string())
            .fetch_one(&self.pool)
            .await?;
        Ok(count == 0)
    }

    /// Enrich a batch of features with derived states for parents.
    /// Parents get their state computed from children; leaves keep their DB state.
    pub async fn resolve_derived_states(&self, features: &mut [Feature]) -> Result<()> {
        if features.is_empty() {
            return Ok(());
        }

        // Collect all feature IDs and query for children states in one batch
        let ids: Vec<String> = features.iter().map(|f| f.id.to_string()).collect();
        let placeholders: String = ids
            .iter()
            .map(|id| format!("'{}'", id))
            .collect::<Vec<_>>()
            .join(", ");
        let sql =
            format!("SELECT parent_id, state FROM features WHERE parent_id IN ({placeholders})");

        let rows = sqlx::query(&sql).fetch_all(&self.pool).await?;

        // Group child states by parent_id
        let mut children_states: std::collections::HashMap<FeatureId, Vec<FeatureState>> =
            std::collections::HashMap::new();
        for row in &rows {
            let parent_id: FeatureId = parse_id(row.get::<String, _>("parent_id"))?;
            let state = FeatureState::from_str(&row.get::<String, _>("state"))
                .unwrap_or(FeatureState::Proposed);
            children_states.entry(parent_id).or_default().push(state);
        }

        // Patch parent states in place
        for feature in features.iter_mut() {
            if let Some(states) = children_states.get(&feature.id) {
                if let Some(derived) = FeatureState::derive_from_children(states) {
                    feature.state = derived;
                }
            }
        }

        Ok(())
    }

    /// Enrich a single feature with derived state if it has children.
    async fn resolve_derived_state_single(&self, feature: &mut Feature) -> Result<()> {
        // Blocked feature sets preserve their explicit blocked state — do not derive.
        if feature.state == FeatureState::Blocked {
            return Ok(());
        }

        let rows = sqlx::query("SELECT state FROM features WHERE parent_id = $1")
            .bind(feature.id.to_string())
            .fetch_all(&self.pool)
            .await?;

        if !rows.is_empty() {
            let states: Vec<FeatureState> = rows
                .iter()
                .map(|r| {
                    FeatureState::from_str(&r.get::<String, _>("state"))
                        .unwrap_or(FeatureState::Proposed)
                })
                .collect();
            if let Some(derived) = FeatureState::derive_from_children(&states) {
                feature.state = derived;
            }
        }

        Ok(())
    }

    /// Enrich a batch of feature summaries with derived states for parents.
    pub async fn resolve_derived_states_summary(
        &self,
        features: &mut [FeatureSummary],
    ) -> Result<()> {
        if features.is_empty() {
            return Ok(());
        }

        let ids: Vec<String> = features.iter().map(|f| f.id.to_string()).collect();
        let placeholders: String = ids
            .iter()
            .map(|id| format!("'{}'", id))
            .collect::<Vec<_>>()
            .join(", ");
        let sql =
            format!("SELECT parent_id, state FROM features WHERE parent_id IN ({placeholders})");

        let rows = sqlx::query(&sql).fetch_all(&self.pool).await?;

        let mut children_states: std::collections::HashMap<FeatureId, Vec<FeatureState>> =
            std::collections::HashMap::new();
        for row in &rows {
            let parent_id: FeatureId = parse_id(row.get::<String, _>("parent_id"))?;
            let state = FeatureState::from_str(&row.get::<String, _>("state"))
                .unwrap_or(FeatureState::Proposed);
            children_states.entry(parent_id).or_default().push(state);
        }

        for feature in features.iter_mut() {
            if let Some(states) = children_states.get(&feature.id) {
                if let Some(derived) = FeatureState::derive_from_children(states) {
                    feature.state = derived;
                }
            }
        }

        Ok(())
    }

    /// Search features by title or details using LIKE matching, ranked by title matches first.
    pub async fn search_features(
        &self,
        query: &str,
        project_id: Option<ProjectId>,
        limit: Option<u32>,
    ) -> Result<Vec<FeatureSummary>> {
        let escaped_query = escape_like_pattern(query);
        let search_pattern = format!("%{}%", escaped_query);
        let limit_val = limit.unwrap_or(10) as i64;

        let rows = match project_id {
            Some(pid) => {
                sqlx::query(
                    "SELECT id, project_id, parent_id, title, state, priority, feature_number, target_version_id
                     FROM features
                     WHERE project_id = $1 AND (title LIKE $2 ESCAPE '\\' OR details LIKE $2 ESCAPE '\\')
                     ORDER BY
                         CASE WHEN title LIKE $2 ESCAPE '\\' THEN 0 ELSE 1 END,
                         priority,
                         title
                     LIMIT $3",
                )
                .bind(pid.to_string())
                .bind(&search_pattern)
                .bind(limit_val)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query(
                    "SELECT id, project_id, parent_id, title, state, priority, feature_number, target_version_id
                     FROM features
                     WHERE title LIKE $1 ESCAPE '\\' OR details LIKE $1 ESCAPE '\\'
                     ORDER BY
                         CASE WHEN title LIKE $1 ESCAPE '\\' THEN 0 ELSE 1 END,
                         priority,
                         title
                     LIMIT $2",
                )
                .bind(&search_pattern)
                .bind(limit_val)
                .fetch_all(&self.pool)
                .await?
            }
        };

        let mut summaries: Vec<FeatureSummary> = rows
            .iter()
            .map(row_to_feature_summary)
            .collect::<Result<_>>()?;
        self.resolve_derived_states_summary(&mut summaries).await?;
        Ok(summaries)
    }

    /// Build the complete feature tree for a project as nested nodes.
    pub async fn get_feature_tree(&self, project_id: ProjectId) -> Result<Vec<FeatureTreeNode>> {
        let project = self.get_project(project_id).await?;
        let root_feature_id = project.and_then(|p| p.root_feature_id);
        let features = self.get_features_by_project(project_id).await?;

        let mut children_map: std::collections::HashMap<Option<FeatureId>, Vec<Feature>> =
            std::collections::HashMap::new();
        let mut root_feature: Option<Feature> = None;

        for feature in features {
            if Some(feature.id) == root_feature_id {
                root_feature = Some(feature);
                continue;
            }
            children_map
                .entry(feature.parent_id)
                .or_default()
                .push(feature);
        }

        fn build_subtree(
            parent_id: Option<FeatureId>,
            children_map: &std::collections::HashMap<Option<FeatureId>, Vec<Feature>>,
        ) -> Vec<FeatureTreeNode> {
            children_map
                .get(&parent_id)
                .map(|features| {
                    features
                        .iter()
                        .map(|f| {
                            let children = build_subtree(Some(f.id), children_map);
                            let mut feature = f.clone();
                            // Derive parent state from children
                            if !children.is_empty() {
                                let child_states: Vec<FeatureState> =
                                    children.iter().map(|c| c.feature.state).collect();
                                if let Some(derived) =
                                    FeatureState::derive_from_children(&child_states)
                                {
                                    feature.state = derived;
                                }
                            }
                            FeatureTreeNode {
                                feature,
                                children,
                                is_root: false,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default()
        }

        if let Some(root) = root_feature {
            let children = build_subtree(Some(root.id), &children_map);
            Ok(vec![FeatureTreeNode {
                feature: root,
                children,
                is_root: true,
            }])
        } else {
            Ok(build_subtree(None, &children_map))
        }
    }

    // ============================================================
    // Feature Context
    // ============================================================

    /// Get a feature with its full navigational context (parent, siblings, children, breadcrumb).
    pub async fn get_feature_with_context(
        &self,
        id: FeatureId,
    ) -> Result<Option<FeatureWithContext>> {
        let feature = match self.get_feature(id).await? {
            Some(f) => f,
            None => return Ok(None),
        };

        // Get parent
        let parent = if let Some(parent_id) = feature.parent_id {
            sqlx::query("SELECT id, title, state FROM features WHERE id = $1")
                .bind(parent_id.to_string())
                .fetch_optional(&self.pool)
                .await?
                .map(|row| row_to_feature_summary_context(&row))
                .transpose()?
        } else {
            None
        };

        // Get siblings
        let siblings: Vec<FeatureSummaryContext> = if let Some(parent_id) = feature.parent_id {
            let rows = sqlx::query(
                "SELECT id, title, state FROM features
                 WHERE parent_id = $1 AND id != $2
                 ORDER BY priority, title",
            )
            .bind(parent_id.to_string())
            .bind(id.to_string())
            .fetch_all(&self.pool)
            .await?;
            rows.iter()
                .map(row_to_feature_summary_context)
                .collect::<Result<Vec<_>>>()?
        } else {
            let rows = sqlx::query(
                "SELECT id, title, state FROM features
                 WHERE project_id = $1 AND parent_id IS NULL AND id != $2
                 ORDER BY priority, title",
            )
            .bind(feature.project_id.to_string())
            .bind(id.to_string())
            .fetch_all(&self.pool)
            .await?;
            rows.iter()
                .map(row_to_feature_summary_context)
                .collect::<Result<Vec<_>>>()?
        };

        // Get children
        let children_rows = sqlx::query(
            "SELECT id, title, state FROM features
             WHERE parent_id = $1
             ORDER BY priority, title",
        )
        .bind(id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let children: Vec<FeatureSummaryContext> = children_rows
            .iter()
            .map(row_to_feature_summary_context)
            .collect::<Result<Vec<_>>>()?;

        // Get breadcrumb using recursive CTE (includes details for ancestor context)
        // For root features (parent_id IS NULL), return details_summary if available to avoid
        // sending full project instructions on every breadcrumb response.
        let breadcrumb_rows = sqlx::query(
            "WITH RECURSIVE ancestors AS (
                SELECT id, parent_id, title, details, details_summary, 0 as depth FROM features WHERE id = $1
                UNION ALL
                SELECT f.id, f.parent_id, f.title, f.details, f.details_summary, a.depth + 1
                FROM features f
                INNER JOIN ancestors a ON f.id = a.parent_id
            )
            SELECT id, title,
                CASE WHEN parent_id IS NULL AND details_summary IS NOT NULL
                     THEN details_summary
                     ELSE details
                END as details
            FROM ancestors ORDER BY depth DESC",
        )
        .bind(id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let breadcrumb: Vec<BreadcrumbItem> = breadcrumb_rows
            .iter()
            .map(|row| -> Result<BreadcrumbItem> {
                Ok(BreadcrumbItem {
                    id: parse_id(row.get::<String, _>("id"))?,
                    title: row.get("title"),
                    details: row.get("details"),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        // Derive parent state from children (zero-cost: children already in memory)
        let mut feature = feature;
        if !children.is_empty() {
            let child_states: Vec<FeatureState> = children.iter().map(|c| c.state).collect();
            if let Some(derived) = FeatureState::derive_from_children(&child_states) {
                feature.state = derived;
            }
        }

        // Fetch latest proof for this feature
        let latest_proof = self
            .get_latest_proof_for_feature(feature.id)
            .await?;

        Ok(Some(FeatureWithContext {
            feature,
            parent,
            siblings,
            children,
            breadcrumb,
            latest_proof,
        }))
    }

    /// Get the highest-priority proposed or in-progress leaf feature, preferring the "next" version.
    /// Excludes feature sets (parents with children) and root features since they are not implementable.
    pub async fn get_next_workable_feature(
        &self,
        project_id: ProjectId,
        version_id: Option<VersionId>,
    ) -> Result<Option<Feature>> {
        let row = if let Some(vid) = version_id {
            let sql = format!(
                "SELECT {FEATURE_COLS}
                 FROM features f
                 WHERE f.project_id = $1
                   AND f.target_version_id = $2
                   AND f.state IN ('proposed', 'in_progress')
                   AND f.parent_id IS NOT NULL
                   AND NOT EXISTS (SELECT 1 FROM features c WHERE c.parent_id = f.id)
                 ORDER BY
                     CASE f.state WHEN 'proposed' THEN 0 ELSE 1 END,
                     f.priority ASC, f.created_at ASC
                 LIMIT 1"
            );
            sqlx::query(&sql)
                .bind(project_id.to_string())
                .bind(vid.to_string())
                .fetch_optional(&self.pool)
                .await?
        } else {
            let sql = format!(
                "WITH next_version AS (
                    SELECT id FROM versions
                    WHERE project_id = $1 AND released_at IS NULL
                    ORDER BY created_at ASC LIMIT 1
                )
                SELECT {FEATURE_COLS_F}
                FROM features f
                LEFT JOIN next_version nv ON f.target_version_id = nv.id
                WHERE f.project_id = $1
                  AND f.state IN ('proposed', 'in_progress')
                  AND f.parent_id IS NOT NULL
                  AND NOT EXISTS (SELECT 1 FROM features c WHERE c.parent_id = f.id)
                ORDER BY
                    CASE WHEN f.target_version_id IS NOT NULL AND f.target_version_id = (SELECT id FROM next_version) THEN 0
                         WHEN f.target_version_id IS NULL THEN 1
                         ELSE 2 END,
                    CASE f.state WHEN 'proposed' THEN 0 ELSE 1 END,
                    f.priority ASC,
                    f.created_at ASC
                LIMIT 1"
            );
            sqlx::query(&sql)
                .bind(project_id.to_string())
                .fetch_optional(&self.pool)
                .await?
        };

        row.as_ref().map(row_to_feature).transpose()
    }

    // ============================================================
    // Feature Blockers
    // ============================================================

    /// Set the blocker features for a blocked feature (replaces existing blockers).
    pub async fn set_feature_blockers(
        &self,
        feature_id: FeatureId,
        blocker_ids: &[FeatureId],
    ) -> Result<()> {
        // Clear existing
        sqlx::query("DELETE FROM feature_blockers WHERE feature_id = $1")
            .bind(feature_id.to_string())
            .execute(&self.pool)
            .await?;

        let now = Utc::now().to_rfc3339();
        for blocker_id in blocker_ids {
            sqlx::query(
                "INSERT INTO feature_blockers (feature_id, blocker_feature_id, created_at) VALUES ($1, $2, $3)",
            )
            .bind(feature_id.to_string())
            .bind(blocker_id.to_string())
            .bind(&now)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// Clear all blocker entries for a feature.
    pub async fn clear_feature_blockers(&self, feature_id: FeatureId) -> Result<()> {
        sqlx::query("DELETE FROM feature_blockers WHERE feature_id = $1")
            .bind(feature_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get the IDs of features that are blocking the given feature.
    pub async fn get_feature_blockers(&self, feature_id: FeatureId) -> Result<Vec<FeatureSummary>> {
        let sql = "SELECT f.id, f.project_id, f.parent_id, f.title, f.state, f.priority, f.feature_number, f.target_version_id
                   FROM feature_blockers fb
                   JOIN features f ON f.id = fb.blocker_feature_id
                   WHERE fb.feature_id = $1
                   ORDER BY f.priority, f.title";
        let rows = sqlx::query(sql)
            .bind(feature_id.to_string())
            .fetch_all(&self.pool)
            .await?;

        rows.iter().map(row_to_feature_summary).collect()
    }

    /// When a feature transitions to `implemented`, check if any features blocked by it
    /// can now be auto-resolved (all their blockers are implemented).
    /// Returns the IDs of features that were unblocked.
    pub async fn auto_resolve_blocked_features(
        &self,
        implemented_id: FeatureId,
    ) -> Result<Vec<FeatureId>> {
        // Find all features that are blocked by the newly-implemented feature
        let blocked_feature_ids: Vec<String> = sqlx::query_scalar(
            "SELECT feature_id FROM feature_blockers WHERE blocker_feature_id = $1",
        )
        .bind(implemented_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut unblocked = Vec::new();
        let now = Utc::now().to_rfc3339();

        for blocked_id_str in blocked_feature_ids {
            // Check if ALL blockers of this feature are now implemented
            let remaining: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM feature_blockers fb
                 JOIN features f ON f.id = fb.blocker_feature_id
                 WHERE fb.feature_id = $1 AND f.state != 'implemented'",
            )
            .bind(&blocked_id_str)
            .fetch_one(&self.pool)
            .await?;

            if remaining == 0 {
                // All blockers implemented — transition to proposed
                sqlx::query("UPDATE features SET state = 'proposed', updated_at = $1 WHERE id = $2 AND state = 'blocked'")
                    .bind(&now)
                    .bind(&blocked_id_str)
                    .execute(&self.pool)
                    .await?;

                // Clear blocker entries
                sqlx::query("DELETE FROM feature_blockers WHERE feature_id = $1")
                    .bind(&blocked_id_str)
                    .execute(&self.pool)
                    .await?;

                let feature_id: FeatureId = parse_id(blocked_id_str)?;
                unblocked.push(feature_id);
            }
        }

        Ok(unblocked)
    }

    /// Walk up the parent chain to find the first blocked ancestor (feature set).
    /// Returns `Some((id, title))` if a blocked ancestor is found.
    pub async fn find_blocked_ancestor(
        &self,
        feature_id: FeatureId,
    ) -> Result<Option<(FeatureId, String)>> {
        let row = sqlx::query(
            "WITH RECURSIVE ancestors AS (
                SELECT parent_id FROM features WHERE id = $1
                UNION ALL
                SELECT f.parent_id FROM features f
                INNER JOIN ancestors a ON f.id = a.parent_id
            )
            SELECT f.id, f.title FROM features f
            INNER JOIN ancestors a ON f.id = a.parent_id
            WHERE f.state = 'blocked'
            LIMIT 1",
        )
        .bind(feature_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let id: FeatureId = parse_id(row.get::<String, _>("id"))?;
                let title: String = row.get("title");
                Ok(Some((id, title)))
            }
            None => Ok(None),
        }
    }

    // ============================================================
    // Verification
    // ============================================================

    /// Store agent-generated verification comments on a feature.
    /// Overwrites any previous verification result.
    pub async fn record_verification(
        &self,
        feature_id: FeatureId,
        comments: &[VerificationComment],
    ) -> Result<Feature> {
        let now = Utc::now().to_rfc3339();
        let json = serde_json::to_string(comments)?;

        let rows_affected = sqlx::query(
            "UPDATE features SET verification_result = $1, verified_at = $2, updated_at = $3 WHERE id = $4",
        )
        .bind(&json)
        .bind(&now)
        .bind(&now)
        .bind(feature_id.to_string())
        .execute(&self.pool)
        .await?
        .rows_affected();

        if rows_affected == 0 {
            return Err(anyhow::anyhow!("Feature not found: {}", feature_id));
        }

        self.get_feature(feature_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Feature not found after update: {}", feature_id))
    }

    // ============================================================
    // Data Migration
    // ============================================================

    /// Migrate legacy projects to use root features by creating a root node and reparenting orphans.
    pub async fn migrate_to_root_features(&self) -> Result<RootFeatureMigrationReport> {
        let mut report = RootFeatureMigrationReport::default();
        let projects = self.get_all_projects().await?;

        for project in projects {
            if project.root_feature_id.is_some() {
                report.projects_skipped += 1;
                continue;
            }

            let now = Utc::now();
            let root_feature_id = FeatureId::new();

            let mut tx = self.pool.begin().await?;

            // Create root feature
            sqlx::query(
                "INSERT INTO features (id, project_id, parent_id, title, details, state, priority, created_at, updated_at)
                 VALUES ($1, $2, NULL, $3, $4, 'implemented', 0, $5, $6)",
            )
            .bind(root_feature_id.to_string())
            .bind(project.id.to_string())
            .bind(&project.name)
            .bind(&project.instructions)
            .bind(now.to_rfc3339())
            .bind(now.to_rfc3339())
            .execute(&mut *tx)
            .await?;

            // Re-parent existing root features
            let result = sqlx::query(
                "UPDATE features SET parent_id = $1 WHERE project_id = $2 AND parent_id IS NULL AND id != $1",
            )
            .bind(root_feature_id.to_string())
            .bind(project.id.to_string())
            .execute(&mut *tx)
            .await?;
            report.features_reparented += result.rows_affected() as usize;

            // Update project
            sqlx::query("UPDATE projects SET root_feature_id = $1, updated_at = $2 WHERE id = $3")
                .bind(root_feature_id.to_string())
                .bind(now.to_rfc3339())
                .bind(project.id.to_string())
                .execute(&mut *tx)
                .await?;

            tx.commit().await?;

            report.projects_migrated += 1;
        }

        Ok(report)
    }

    /// Set claim fields on a feature (called when an agent starts work).
    pub async fn set_feature_claim(
        &self,
        id: FeatureId,
        agent_type: &str,
        metadata: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "UPDATE features SET claimed_by = $1, claimed_at = $2, claim_metadata = $3, updated_at = $4 WHERE id = $5",
        )
        .bind(agent_type)
        .bind(now.to_rfc3339())
        .bind(metadata)
        .bind(now.to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Atomically claim a feature: check for existing claims, transition state
    /// to in_progress, and set claim fields — all within a single transaction.
    ///
    /// Uses `BEGIN IMMEDIATE` on SQLite to acquire a write lock at transaction
    /// start, preventing two agents from simultaneously reading the feature as
    /// unclaimed and both proceeding to claim it.
    ///
    /// Returns the updated Feature on success.
    /// Returns `ManifestError::ClaimConflict` if another agent already holds a claim.
    /// Returns `ManifestError::NotFound` if the feature does not exist.
    pub async fn claim_feature_atomic(
        &self,
        id: FeatureId,
        agent_type: &str,
        metadata: Option<&str>,
        force: bool,
    ) -> Result<Feature> {
        let mut conn = self.pool.acquire().await?;

        // Use BEGIN IMMEDIATE for SQLite to acquire write lock immediately,
        // preventing concurrent readers from proceeding with stale data.
        // PostgreSQL uses standard BEGIN (row-level locking via SELECT FOR UPDATE).
        if self.dialect.is_sqlite() {
            sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
        } else {
            sqlx::query("BEGIN").execute(&mut *conn).await?;
        }

        // Fetch the feature within the transaction
        let row = sqlx::query(&format!(
            "SELECT {FEATURE_COLS} FROM features WHERE id = $1{}",
            if self.dialect.is_postgres() {
                " FOR UPDATE"
            } else {
                ""
            }
        ))
        .bind(id.to_string())
        .fetch_optional(&mut *conn)
        .await;

        let row = match row {
            Ok(Some(row)) => row,
            Ok(None) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                return Err(ManifestError::not_found("Feature").into());
            }
            Err(e) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                return Err(e.into());
            }
        };

        let feature: Feature = row_to_feature(&row)?;

        // Check for existing claim conflict (unless force=true)
        if !force && feature.claimed_by.is_some() && feature.state == FeatureState::InProgress {
            let conflict = super::ClaimConflictInfo {
                agent_type: feature.claimed_by.clone().unwrap_or_default(),
                feature_id: id.to_string(),
                claimed_at: feature
                    .claimed_at
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_default(),
                claim_metadata: feature.claim_metadata.clone(),
            };
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
            return Err(ManifestError::ClaimConflict(conflict).into());
        }

        let now = Utc::now();

        // Determine if state transition is needed
        let should_transition = matches!(
            feature.state,
            FeatureState::Proposed | FeatureState::Implemented
        );
        let new_state = if should_transition {
            FeatureState::InProgress
        } else {
            feature.state
        };

        // If transitioning to in_progress, auto-assign to "next" version
        let target_version_id = if new_state == FeatureState::InProgress
            && feature.state != FeatureState::InProgress
        {
            // Look up the next version (outside the critical path is fine,
            // but we do it inside the txn for consistency)
            let next_ver: Option<String> = sqlx::query_scalar(
                "SELECT id FROM versions WHERE project_id = $1 AND released_at IS NULL ORDER BY created_at LIMIT 1",
            )
            .bind(feature.project_id.to_string())
            .fetch_optional(&mut *conn)
            .await?;
            next_ver.or(feature.target_version_id.map(|v| v.to_string()))
        } else {
            feature.target_version_id.map(|v| v.to_string())
        };

        // Atomically update state + claim in a single UPDATE
        let result = sqlx::query(
            "UPDATE features SET state = $1, claimed_by = $2, claimed_at = $3, claim_metadata = $4, target_version_id = $5, updated_at = $6 WHERE id = $7",
        )
        .bind(new_state.as_str())
        .bind(agent_type)
        .bind(now.to_rfc3339())
        .bind(metadata)
        .bind(&target_version_id)
        .bind(now.to_rfc3339())
        .bind(id.to_string())
        .execute(&mut *conn)
        .await;

        if let Err(e) = result {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
            return Err(e.into());
        }

        // Commit the transaction
        sqlx::query("COMMIT").execute(&mut *conn).await?;

        // Emit event
        let _ = self.events.send(FeatureEvent::Updated {
            project_id: feature.project_id,
        });

        // Re-fetch the updated feature
        let updated = self
            .get_feature(id)
            .await?
            .ok_or_else(|| ManifestError::not_found("Feature"))?;

        Ok(updated)
    }

    /// Clear claim fields on a feature (called when work is completed).
    pub async fn clear_feature_claim(&self, id: FeatureId) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "UPDATE features SET claimed_by = NULL, claimed_at = NULL, claim_metadata = NULL, updated_at = $1 WHERE id = $2",
        )
        .bind(now.to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Complete a feature: create history entry, update state to implemented,
    /// clear claims, clear desired_details, and emit a Completed event.
    ///
    /// This consolidates the completion logic that was previously split between
    /// the MCP tool and API handler, following the principle:
    /// "business logic lives in the DB layer."
    pub async fn complete_feature(
        &self,
        feature_id: FeatureId,
        summary: &str,
        commits: &[CommitRef],
    ) -> Result<CompletionResult> {
        // Get current feature
        let feature = self
            .get_feature(feature_id)
            .await?
            .ok_or_else(|| ManifestError::not_found("Feature"))?;

        // Verify it's a leaf feature
        if !self.is_leaf(feature_id).await? {
            return Err(ManifestError::invalid_state("Cannot complete a non-leaf feature").into());
        }

        // Check proof requirements based on project testing policy
        let project = self
            .get_project(feature.project_id)
            .await?
            .ok_or_else(|| ManifestError::not_found("Project"))?;

        // Fetch latest proof once (skip when testing_policy == None)
        let latest_proof = if project.testing_policy != TestingPolicy::None {
            self.get_latest_proof_for_feature(feature_id).await?
        } else {
            None
        };

        // TDD hard gate
        if project.testing_policy == TestingPolicy::Tdd {
            match &latest_proof {
                None => {
                    return Err(ManifestError::invalid_state(
                        "Cannot complete feature: no proof recorded. This project requires test evidence (testing_policy: tdd). Call prove_feature with your test results first."
                    ).into());
                }
                Some(p) if p.exit_code != 0 => {
                    return Err(ManifestError::invalid_state(
                        "Cannot complete feature: latest proof has failing tests (exit code != 0). Fix the tests and call prove_feature again."
                    ).into());
                }
                _ => {} // passing proof exists, proceed
            }
        }

        // Advisory warnings (non-blocking)
        let mut warnings = Vec::new();

        if project.testing_policy == TestingPolicy::Advisory {
            if latest_proof.is_none()
                || latest_proof.as_ref().is_some_and(|p| p.exit_code != 0)
            {
                warnings.push(
                    "No passing test evidence recorded. Consider running tests and calling prove_feature.".to_string(),
                );
            }
        }

        // TDD structured-results warning (non-blocking)
        if project.testing_policy == TestingPolicy::Tdd {
            if let Some(ref proof) = latest_proof {
                if proof.test_suites.as_ref().is_none_or(|s| s.is_empty()) {
                    warnings.push(
                        "Proof recorded but has no structured test results. Include test_suites with { name, suite, state, file, line } for consistent display in the UI.".to_string(),
                    );
                }
            }
        }

        // Check if spec was updated since the feature was claimed
        if let Some(claimed_at) = feature.claimed_at {
            if feature.updated_at <= claimed_at {
                warnings.push(
                    "Feature spec not updated since work started. Call update_feature to reflect what was actually built.".to_string(),
                );
            }
        }

        // Capture agent_type before clearing claim
        let agent_type = feature.claimed_by.clone();

        // Create history entry
        let history = self
            .create_history_entry(CreateHistoryInput {
                feature_id,
                version_id: None, // will default to feature's target_version_id
                details: HistoryDetails {
                    summary: summary.to_string(),
                    commits: commits.to_vec(),
                },
            })
            .await?;

        // Update state to implemented + clear claims + clear desired_details
        let needs_state_change = feature.state != FeatureState::Implemented;
        let has_pending_changes = feature.desired_details.is_some();

        let input = UpdateFeatureInput {
            parent_id: None,
            title: None,
            details: None,
            desired_details: if has_pending_changes {
                Some(None) // Clear desired_details
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
        };

        // If neither state change nor pending changes, still clear claims
        if !needs_state_change && !has_pending_changes {
            self.clear_feature_claim(feature_id).await?;
        } else {
            self.update_feature(feature_id, input).await?;
            self.clear_feature_claim(feature_id).await?;
        }

        // Re-fetch feature with updated state
        let updated_feature = self
            .get_feature(feature_id)
            .await?
            .ok_or_else(|| ManifestError::not_found("Feature"))?;

        // Use project fetched earlier for the Completed event
        let project_name = project.name.clone();

        // Emit Completed event (richer than generic Updated)
        let _ = self.events.send(FeatureEvent::Completed {
            project_id: feature.project_id,
            feature_id,
            feature_title: updated_feature.title.clone(),
            project_name,
            agent_type,
        });

        Ok(CompletionResult {
            feature: updated_feature,
            history,
            warnings,
        })
    }
}
