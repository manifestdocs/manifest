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
        // Build dynamic WHERE clause
        let mut sql = String::from(
            "SELECT fh.id, fh.feature_id, f.title, f.state, fh.version_id, v.name, fh.details, fh.created_at
             FROM feature_history fh
             INNER JOIN features f ON f.id = fh.feature_id
             LEFT JOIN versions v ON v.id = fh.version_id
             WHERE f.project_id = $1",
        );
        let mut next_param: u32 = 2;

        if version_id.is_some() {
            sql.push_str(&format!(" AND fh.version_id = ${next_param}"));
            next_param += 1;
        }
        if since.is_some() {
            sql.push_str(&format!(" AND fh.created_at > ${next_param}"));
            next_param += 1;
        }

        sql.push_str(" ORDER BY fh.created_at DESC");

        // Default limit/offset
        let limit = Some(limit.unwrap_or(50));
        let offset = Some(offset.unwrap_or(0));
        append_pagination(&mut sql, limit, offset, next_param, &self.dialect);

        // Bind in the same order params were appended
        let mut q = sqlx::query(&sql).bind(project_id.to_string());
        if let Some(vid) = version_id {
            q = q.bind(vid.to_string());
        }
        if let Some(since_dt) = since {
            q = q.bind(since_dt.to_rfc3339());
        }
        if let Some(lim) = limit {
            q = q.bind(lim as i64);
        }
        if let Some(off) = offset {
            q = q.bind(off as i64);
        }

        let rows = q.fetch_all(&self.pool).await?;
        rows.iter().map(row_to_project_history_entry).collect()
    }
}
