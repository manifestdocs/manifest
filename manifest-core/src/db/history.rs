use anyhow::Result;
use chrono::{DateTime, Utc};

use super::helpers::*;
use super::Database;
use crate::models::*;

impl Database {
    /// Record a history entry for a feature.
    ///
    /// If no version_id is provided, inherits the feature's current target_version_id.
    pub async fn create_history_entry(&self, input: CreateHistoryInput) -> Result<FeatureHistory> {
        let id = HistoryId::new();
        let now = Utc::now();

        // Get version_id from input or feature's target_version_id
        let version_id = match input.version_id {
            Some(vid) => Some(vid),
            None => sqlx::query_scalar::<_, Option<String>>(
                "SELECT target_version_id FROM features WHERE id = $1",
            )
            .bind(input.feature_id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .flatten()
            .map(parse_id)
            .transpose()?,
        };

        let details_json = serde_json::to_string(&input.details)?;

        sqlx::query(
            "INSERT INTO feature_history (id, feature_id, version_id, summary, details, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id.to_string())
        .bind(input.feature_id.to_string())
        .bind(version_id.map(|u| u.to_string()))
        .bind(&input.details.summary)
        .bind(&details_json)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(FeatureHistory {
            id,
            feature_id: input.feature_id,
            version_id,
            details: input.details,
            created_at: now,
        })
    }

    /// Get all history entries for a feature, most recent first.
    pub async fn get_feature_history(&self, feature_id: FeatureId) -> Result<Vec<FeatureHistory>> {
        let rows = sqlx::query(
            "SELECT id, feature_id, version_id, details, created_at
             FROM feature_history WHERE feature_id = $1 ORDER BY created_at DESC",
        )
        .bind(feature_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_feature_history).collect()
    }

    /// Get history entries across all features in a project, with optional version and date filters.
    pub async fn get_project_history(
        &self,
        project_id: ProjectId,
        version_id: Option<VersionId>,
        limit: Option<u32>,
        offset: Option<u32>,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<ProjectHistoryEntry>> {
        let limit_val = limit.unwrap_or(50) as i64;
        let offset_val = offset.unwrap_or(0) as i64;

        // Build dynamic query based on filters
        let rows = match (version_id, since) {
            (Some(vid), Some(since_dt)) => {
                sqlx::query(
                    "SELECT fh.id, fh.feature_id, f.title, f.state, fh.version_id, v.name, fh.details, fh.created_at
                     FROM feature_history fh
                     INNER JOIN features f ON f.id = fh.feature_id
                     LEFT JOIN versions v ON v.id = fh.version_id
                     WHERE f.project_id = $1 AND fh.version_id = $2 AND fh.created_at > $3
                     ORDER BY fh.created_at DESC LIMIT $4 OFFSET $5",
                )
                .bind(project_id.to_string())
                .bind(vid.to_string())
                .bind(since_dt.to_rfc3339())
                .bind(limit_val)
                .bind(offset_val)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(vid), None) => {
                sqlx::query(
                    "SELECT fh.id, fh.feature_id, f.title, f.state, fh.version_id, v.name, fh.details, fh.created_at
                     FROM feature_history fh
                     INNER JOIN features f ON f.id = fh.feature_id
                     LEFT JOIN versions v ON v.id = fh.version_id
                     WHERE f.project_id = $1 AND fh.version_id = $2
                     ORDER BY fh.created_at DESC LIMIT $3 OFFSET $4",
                )
                .bind(project_id.to_string())
                .bind(vid.to_string())
                .bind(limit_val)
                .bind(offset_val)
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(since_dt)) => {
                sqlx::query(
                    "SELECT fh.id, fh.feature_id, f.title, f.state, fh.version_id, v.name, fh.details, fh.created_at
                     FROM feature_history fh
                     INNER JOIN features f ON f.id = fh.feature_id
                     LEFT JOIN versions v ON v.id = fh.version_id
                     WHERE f.project_id = $1 AND fh.created_at > $2
                     ORDER BY fh.created_at DESC LIMIT $3 OFFSET $4",
                )
                .bind(project_id.to_string())
                .bind(since_dt.to_rfc3339())
                .bind(limit_val)
                .bind(offset_val)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query(
                    "SELECT fh.id, fh.feature_id, f.title, f.state, fh.version_id, v.name, fh.details, fh.created_at
                     FROM feature_history fh
                     INNER JOIN features f ON f.id = fh.feature_id
                     LEFT JOIN versions v ON v.id = fh.version_id
                     WHERE f.project_id = $1
                     ORDER BY fh.created_at DESC LIMIT $2 OFFSET $3",
                )
                .bind(project_id.to_string())
                .bind(limit_val)
                .bind(offset_val)
                .fetch_all(&self.pool)
                .await?
            }
        };

        rows.iter().map(row_to_project_history_entry).collect()
    }
}
