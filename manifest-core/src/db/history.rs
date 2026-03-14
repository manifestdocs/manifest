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
            None => {
                let mut rows = self
                    .conn
                    .query(
                        "SELECT target_version_id FROM features WHERE id = ?1",
                        libsql::params![input.feature_id.to_string()],
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

                match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
                    Some(row) => row
                        .get::<Option<String>>(0)
                        .map_err(|e| anyhow::anyhow!("{}", e))?
                        .map(parse_id)
                        .transpose()?,
                    None => None,
                }
            }
        };

        let details_json = serde_json::to_string(&input.details)?;

        self.conn
            .execute(
                "INSERT INTO feature_history (id, feature_id, version_id, summary, details, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                libsql::params![
                    id.to_string(),
                    input.feature_id.to_string(),
                    version_id.map(|u| u.to_string()),
                    input.details.summary.clone(),
                    details_json,
                    now.to_rfc3339()
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

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
        let mut rows = self
            .conn
            .query(
                "SELECT id, feature_id, version_id, details, created_at
                 FROM feature_history WHERE feature_id = ?1 ORDER BY created_at DESC",
                libsql::params![feature_id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            results.push(row_to_feature_history(&row)?);
        }
        Ok(results)
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
             WHERE f.project_id = ?1",
        );
        let mut next_param: u32 = 2;
        let mut params: Vec<libsql::Value> = vec![libsql::Value::Text(project_id.to_string())];

        if let Some(ref vid) = version_id {
            sql.push_str(&format!(" AND fh.version_id = ?{next_param}"));
            next_param += 1;
            params.push(libsql::Value::Text(vid.to_string()));
        }
        if let Some(ref since_dt) = since {
            sql.push_str(&format!(" AND fh.created_at > ?{next_param}"));
            next_param += 1;
            params.push(libsql::Value::Text(since_dt.to_rfc3339()));
        }

        sql.push_str(" ORDER BY fh.created_at DESC");

        // Default limit/offset
        let limit = Some(limit.unwrap_or(50));
        let offset = Some(offset.unwrap_or(0));
        append_pagination(&mut sql, limit, offset, next_param);

        if let Some(lim) = limit {
            params.push(libsql::Value::Integer(i64::from(lim)));
        }
        if let Some(off) = offset {
            params.push(libsql::Value::Integer(i64::from(off)));
        }

        let mut rows = self
            .conn
            .query(&sql, params)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            results.push(row_to_project_history_entry(&row)?);
        }
        Ok(results)
    }
}
