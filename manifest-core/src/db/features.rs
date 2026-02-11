use anyhow::Result;
use chrono::Utc;
use sqlx::Row;

use super::helpers::*;
use super::{
    escape_like_pattern, Database, FeatureEvent, ManifestError, RootFeatureMigrationReport,
};
use crate::models::*;

/// SELECT columns for the features table (bare names).
const FEATURE_COLS: &str = "id, project_id, parent_id, title, details, desired_details, details_summary, state, priority, target_version_id, created_at, updated_at";

/// SELECT columns for the features table with `f.` table alias.
const FEATURE_COLS_F: &str = "f.id, f.project_id, f.parent_id, f.title, f.details, f.desired_details, f.details_summary, f.state, f.priority, f.target_version_id, f.created_at, f.updated_at";

impl Database {
    /// Get all features across all projects with optional pagination.
    pub async fn get_all_features_paginated(
        &self,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<Feature>> {
        let rows = match (limit, offset) {
            (Some(lim), Some(off)) => {
                let sql = format!(
                    "SELECT {FEATURE_COLS} FROM features ORDER BY priority, title LIMIT $1 OFFSET $2"
                );
                sqlx::query(&sql)
                    .bind(lim as i64)
                    .bind(off as i64)
                    .fetch_all(&self.pool)
                    .await?
            }
            (Some(lim), None) => {
                let sql = format!(
                    "SELECT {FEATURE_COLS} FROM features ORDER BY priority, title LIMIT $1"
                );
                sqlx::query(&sql)
                    .bind(lim as i64)
                    .fetch_all(&self.pool)
                    .await?
            }
            (None, Some(off)) => {
                let sql = format!(
                    "SELECT {FEATURE_COLS} FROM features ORDER BY priority, title {} OFFSET $1",
                    self.dialect.unlimited_offset_sql()
                );
                sqlx::query(&sql)
                    .bind(off as i64)
                    .fetch_all(&self.pool)
                    .await?
            }
            (None, None) => {
                let sql =
                    format!("SELECT {FEATURE_COLS} FROM features ORDER BY priority, title");
                sqlx::query(&sql).fetch_all(&self.pool).await?
            }
        };

        rows.iter().map(row_to_feature).collect()
    }

    /// Get all features across all projects without pagination.
    pub async fn get_all_features(&self) -> Result<Vec<Feature>> {
        self.get_all_features_paginated(None, None).await
    }

    /// Get all features for a project with optional pagination.
    pub async fn get_features_by_project_paginated(
        &self,
        project_id: ProjectId,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<Feature>> {
        let rows = match (limit, offset) {
            (Some(lim), Some(off)) => {
                let sql = format!(
                    "SELECT {FEATURE_COLS} FROM features WHERE project_id = $1 ORDER BY priority, title LIMIT $2 OFFSET $3"
                );
                sqlx::query(&sql)
                    .bind(project_id.to_string())
                    .bind(lim as i64)
                    .bind(off as i64)
                    .fetch_all(&self.pool)
                    .await?
            }
            (Some(lim), None) => {
                let sql = format!(
                    "SELECT {FEATURE_COLS} FROM features WHERE project_id = $1 ORDER BY priority, title LIMIT $2"
                );
                sqlx::query(&sql)
                    .bind(project_id.to_string())
                    .bind(lim as i64)
                    .fetch_all(&self.pool)
                    .await?
            }
            (None, Some(off)) => {
                let sql = format!(
                    "SELECT {FEATURE_COLS} FROM features WHERE project_id = $1 ORDER BY priority, title {} OFFSET $2",
                    self.dialect.unlimited_offset_sql()
                );
                sqlx::query(&sql)
                    .bind(project_id.to_string())
                    .bind(off as i64)
                    .fetch_all(&self.pool)
                    .await?
            }
            (None, None) => {
                let sql = format!(
                    "SELECT {FEATURE_COLS} FROM features WHERE project_id = $1 ORDER BY priority, title"
                );
                sqlx::query(&sql)
                    .bind(project_id.to_string())
                    .fetch_all(&self.pool)
                    .await?
            }
        };

        rows.iter().map(row_to_feature).collect()
    }

    /// Get all features for a project without pagination.
    pub async fn get_features_by_project(&self, project_id: ProjectId) -> Result<Vec<Feature>> {
        self.get_features_by_project_paginated(project_id, None, None)
            .await
    }

    /// Get a single feature by ID.
    pub async fn get_feature(&self, id: FeatureId) -> Result<Option<Feature>> {
        let sql = format!("SELECT {FEATURE_COLS} FROM features WHERE id = $1");
        let row = sqlx::query(&sql)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;

        row.as_ref().map(row_to_feature).transpose()
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
                let sql =
                    format!("SELECT {FEATURE_COLS} FROM features WHERE id LIKE $1 LIMIT 2");
                sqlx::query(&sql)
                    .bind(&pattern)
                    .fetch_all(&self.pool)
                    .await?
            }
        };

        match rows.len() {
            0 => Ok(None),
            1 => Ok(Some(row_to_feature(&rows[0])?)),
            _ => Err(anyhow::anyhow!(
                "Ambiguous prefix '{}': matches multiple features",
                prefix
            )),
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

        sqlx::query(
            "INSERT INTO features (id, project_id, parent_id, title, details, state, priority, target_version_id, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(id.to_string())
        .bind(project_id.to_string())
        .bind(Some(parent_id.to_string()))
        .bind(&input.title)
        .bind(&input.details)
        .bind(state.as_str())
        .bind(priority)
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
            target_version_id,
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

            sqlx::query(
                "INSERT INTO features (id, project_id, parent_id, title, details, state, priority, target_version_id, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            )
            .bind(id.to_string())
            .bind(project_id.to_string())
            .bind(Some(parent_id.to_string()))
            .bind(&input.title)
            .bind(&input.details)
            .bind(state.as_str())
            .bind(priority)
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
                target_version_id,
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
            let desired_details = input.desired_details.unwrap_or(existing.desired_details);
            let is_proposing = desired_details.is_some() && !had_desired_details;
            (desired_details, is_proposing)
        };
        // Guard rail: feature sets (parents with children) do not have mutable state.
        // Reject explicit state changes on non-leaf features so agents get clear feedback.
        // Only check is_leaf when a state change is actually requested (avoids extra query).
        if input.state.is_some() && !self.is_leaf(id).await? {
            return Err(ManifestError::invalid_state(
                "Cannot change state on a feature set. Feature sets group related capabilities — only leaf features have mutable state. To work on this area, start one of its child features instead."
            ).into());
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

        let _ = self.events.send(FeatureEvent::Updated {
            project_id: existing.project_id,
        });

        Ok(Some(Feature {
            id,
            project_id: existing.project_id,
            parent_id,
            title,
            details,
            desired_details,
            details_summary,
            state,
            priority,
            target_version_id,
            created_at: existing.created_at,
            updated_at: now,
        }))
    }

    /// Delete a feature and all its descendants recursively.
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

        rows.iter().map(row_to_feature).collect()
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

        rows.iter().map(row_to_feature).collect()
    }

    /// Check whether a feature is a leaf node (has no children).
    pub async fn is_leaf(&self, feature_id: FeatureId) -> Result<bool> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM features WHERE parent_id = $1")
            .bind(feature_id.to_string())
            .fetch_one(&self.pool)
            .await?;
        Ok(count == 0)
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
                    "SELECT id, project_id, parent_id, title, state, priority, target_version_id
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
                    "SELECT id, project_id, parent_id, title, state, priority, target_version_id
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

        rows.iter().map(row_to_feature_summary).collect()
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
                        .map(|f| FeatureTreeNode {
                            feature: f.clone(),
                            children: build_subtree(Some(f.id), children_map),
                            is_root: false,
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

        Ok(Some(FeatureWithContext {
            feature,
            parent,
            siblings,
            children,
            breadcrumb,
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
                 ORDER BY f.priority ASC, f.created_at ASC
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
}
