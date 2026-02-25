use anyhow::Result;
use chrono::Utc;
use sqlx::Row;
use uuid::Uuid;

use super::Database;
use crate::models::{CreateMemoryInput, FeatureId, MemoryId, ProjectId, ProjectMemory};

impl Database {
    /// Create a new project memory entry.
    pub async fn create_memory(
        &self,
        project_id: ProjectId,
        input: &CreateMemoryInput,
    ) -> Result<ProjectMemory> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let tags_json = serde_json::to_string(&input.tags)?;

        sqlx::query(
            "INSERT INTO project_memories (id, project_id, content, tags, source_feature_id, created_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(id.to_string())
        .bind(project_id.to_string())
        .bind(&input.content)
        .bind(&tags_json)
        .bind(input.source_feature_id.map(|id| id.to_string()))
        .bind(&input.created_by)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        // Sync FTS index on SQLite (best-effort; falls back to LIKE if FTS unavailable)
        if self.dialect.is_sqlite() {
            let _ = sqlx::query(
                "INSERT INTO project_memories_fts(rowid, content, tags) \
                 SELECT rowid, content, tags FROM project_memories WHERE id = $1",
            )
            .bind(id.to_string())
            .execute(&self.pool)
            .await;
        }

        Ok(ProjectMemory {
            id,
            project_id,
            content: input.content.clone(),
            tags: input.tags.clone(),
            source_feature_id: input.source_feature_id,
            created_by: input.created_by.clone(),
            created_at: now,
            updated_at: now,
        })
    }

    /// Search project memories using FTS5 (SQLite) or LIKE (PostgreSQL/fallback).
    ///
    /// If no query is provided, returns all memories for the project ordered by created_at DESC.
    pub async fn search_memories(
        &self,
        project_id: ProjectId,
        query: Option<&str>,
        limit: u32,
    ) -> Result<Vec<ProjectMemory>> {
        let rows = match query {
            None => {
                // No query: return all memories for this project
                sqlx::query(
                    "SELECT id, project_id, content, tags, source_feature_id, created_by, created_at, updated_at
                     FROM project_memories
                     WHERE project_id = $1
                     ORDER BY created_at DESC
                     LIMIT $2",
                )
                .bind(project_id.to_string())
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
            Some(q) if self.dialect.is_sqlite() && !is_short_query(q) => {
                // SQLite with FTS5 — use full-text search
                // The FTS match returns rowid; join back to project_memories
                match sqlx::query(
                    "SELECT pm.id, pm.project_id, pm.content, pm.tags, pm.source_feature_id, pm.created_by, pm.created_at, pm.updated_at
                     FROM project_memories pm
                     JOIN project_memories_fts fts ON pm.rowid = fts.rowid
                     WHERE project_memories_fts MATCH $1
                       AND pm.project_id = $2
                     ORDER BY fts.rank
                     LIMIT $3",
                )
                .bind(sanitize_fts_query(q))
                .bind(project_id.to_string())
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await {
                    Ok(rows) => rows,
                    // FTS table may not exist on very old DBs; fall through to LIKE
                    Err(_) => self.like_search_memories(project_id, q, limit).await?,
                }
            }
            Some(q) => {
                // PostgreSQL or short query: LIKE search
                self.like_search_memories(project_id, q, limit).await?
            }
        };

        rows.iter()
            .map(|row| {
                let tags_json: String = row.get("tags");
                let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
                let source_feature_id = row
                    .get::<Option<String>, _>("source_feature_id")
                    .map(|s| {
                        Uuid::parse_str(&s)
                            .map(FeatureId::from)
                            .map_err(|e| anyhow::anyhow!(e))
                    })
                    .transpose()?;
                Ok(ProjectMemory {
                    id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                    project_id,
                    content: row.get("content"),
                    tags,
                    source_feature_id,
                    created_by: row.get("created_by"),
                    created_at: crate::db::helpers::parse_datetime(row.get("created_at"))?,
                    updated_at: crate::db::helpers::parse_datetime(row.get("updated_at"))?,
                })
            })
            .collect()
    }

    /// Delete a project memory entry.
    pub async fn delete_memory(&self, project_id: ProjectId, memory_id: MemoryId) -> Result<bool> {
        // Remove from FTS index before deleting (needs content still present in main table)
        if self.dialect.is_sqlite() {
            let _ = sqlx::query(
                "INSERT INTO project_memories_fts(project_memories_fts, rowid, content, tags) \
                 SELECT 'delete', rowid, content, tags FROM project_memories \
                 WHERE id = $1 AND project_id = $2",
            )
            .bind(memory_id.to_string())
            .bind(project_id.to_string())
            .execute(&self.pool)
            .await;
        }

        let rows_affected =
            sqlx::query("DELETE FROM project_memories WHERE id = $1 AND project_id = $2")
                .bind(memory_id.to_string())
                .bind(project_id.to_string())
                .execute(&self.pool)
                .await?
                .rows_affected();

        Ok(rows_affected > 0)
    }

    /// LIKE-based fallback search for PostgreSQL or short queries.
    async fn like_search_memories(
        &self,
        project_id: ProjectId,
        query: &str,
        limit: u32,
    ) -> Result<Vec<sqlx::any::AnyRow>> {
        let pattern = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        let rows = sqlx::query(
            "SELECT id, project_id, content, tags, source_feature_id, created_by, created_at, updated_at
             FROM project_memories
             WHERE project_id = $1
               AND (content LIKE $2 OR tags LIKE $2)
             ORDER BY created_at DESC
             LIMIT $3",
        )
        .bind(project_id.to_string())
        .bind(&pattern)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

/// Returns true if the query is too short or symbol-heavy for FTS5.
fn is_short_query(q: &str) -> bool {
    q.trim().len() < 3
}

/// Sanitize a user query for safe use in FTS5 MATCH.
///
/// FTS5 has a query syntax; we escape special characters so plain-text
/// queries don't cause parse errors.
fn sanitize_fts_query(q: &str) -> String {
    // Wrap in quotes to treat as a phrase; remove embedded quotes
    let cleaned = q.replace('"', " ");
    format!("\"{}\"", cleaned.trim())
}
