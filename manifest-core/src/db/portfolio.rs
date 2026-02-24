use anyhow::Result;
use chrono::Utc;
use sqlx::Row;

use super::helpers::{parse_datetime, parse_id};
use super::Database;
use crate::models::{
    Portfolio, PortfolioCompletion, PortfolioFeatureRef, PortfolioNextFeature, PortfolioProject,
    PortfolioVersionSummary, ProjectId, VersionId,
};

impl Database {
    /// Build a portfolio snapshot for all projects.
    ///
    /// Runs several focused queries per project. Total query count scales with
    /// the number of projects but is fast in practice (SQLite, <20 projects).
    pub async fn get_portfolio(&self) -> Result<Portfolio> {
        let projects = self.get_all_projects().await?;
        let mut portfolio_projects = Vec::with_capacity(projects.len());

        for project in &projects {
            let pid = project.id;

            let next_version = self.portfolio_next_version(pid).await?;
            let next_version_id = next_version.as_ref().map(|v| v.id);
            let next_feature = self.portfolio_next_feature(pid, next_version_id).await?;
            let (in_progress, in_progress_total) = self.portfolio_in_progress(pid).await?;
            let (blocked, blocked_count) = self.portfolio_blocked(pid).await?;
            let (recent_completions, last_activity_at) = self.portfolio_recent(pid).await?;

            portfolio_projects.push(PortfolioProject {
                id: pid,
                name: project.name.clone(),
                slug: project.slug.clone(),
                next_version,
                next_feature,
                in_progress,
                in_progress_total,
                blocked,
                blocked_count,
                recent_completions,
                last_activity_at,
            });
        }

        Ok(Portfolio {
            projects: portfolio_projects,
        })
    }

    /// Get the next unreleased version with its leaf-feature progress counts.
    async fn portfolio_next_version(
        &self,
        project_id: ProjectId,
    ) -> Result<Option<PortfolioVersionSummary>> {
        let row = sqlx::query(
            "SELECT
                v.id as version_id,
                v.name as version_name,
                COUNT(f.id) as feature_count,
                SUM(CASE WHEN f.state = 'implemented' THEN 1 ELSE 0 END) as implemented_count
             FROM versions v
             LEFT JOIN features f
                ON f.target_version_id = v.id
               AND f.project_id = $1
               AND f.parent_id IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM features c WHERE c.parent_id = f.id)
             WHERE v.project_id = $1 AND v.released_at IS NULL
             GROUP BY v.id, v.name
             ORDER BY v.created_at ASC
             LIMIT 1",
        )
        .bind(project_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.map(|r| {
            Ok(PortfolioVersionSummary {
                id: parse_id::<VersionId>(r.get("version_id"))?,
                name: r.get("version_name"),
                feature_count: r.get::<i64, _>("feature_count"),
                implemented_count: r.get::<i64, _>("implemented_count"),
            })
        })
        .transpose()
    }

    /// Get the highest-priority proposed leaf feature, preferring version-assigned over backlog.
    async fn portfolio_next_feature(
        &self,
        project_id: ProjectId,
        next_version_id: Option<VersionId>,
    ) -> Result<Option<PortfolioNextFeature>> {
        let row = sqlx::query(
            "SELECT f.id, f.title, f.target_version_id
             FROM features f
             LEFT JOIN versions v ON v.id = f.target_version_id AND v.released_at IS NULL
             WHERE f.project_id = $1
               AND f.state = 'proposed'
               AND f.parent_id IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM features c WHERE c.parent_id = f.id)
             ORDER BY
               CASE WHEN v.id IS NOT NULL THEN 0 ELSE 1 END,
               f.priority ASC,
               f.created_at ASC
             LIMIT 1",
        )
        .bind(project_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.map(|r| {
            let feature_id = parse_id(r.get("id"))?;
            let target_version_id: Option<String> = r.get("target_version_id");
            let in_version = next_version_id.is_some()
                && target_version_id
                    .as_ref()
                    .map(|id| {
                        id == &next_version_id
                            .unwrap()
                            .to_string()
                    })
                    .unwrap_or(false);
            Ok(PortfolioNextFeature {
                id: feature_id,
                title: r.get("title"),
                in_version,
            })
        })
        .transpose()
    }

    /// Get in-progress leaf features (up to 5) and their total count.
    async fn portfolio_in_progress(
        &self,
        project_id: ProjectId,
    ) -> Result<(Vec<PortfolioFeatureRef>, i64)> {
        let rows = sqlx::query(
            "SELECT f.id, f.title FROM features f
             WHERE f.project_id = $1
               AND f.state = 'in_progress'
               AND f.parent_id IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM features c WHERE c.parent_id = f.id)
             ORDER BY f.priority ASC, f.created_at ASC",
        )
        .bind(project_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let total = rows.len() as i64;
        let features = rows
            .iter()
            .take(5)
            .map(|r| {
                Ok(PortfolioFeatureRef {
                    id: parse_id(r.get("id"))?,
                    title: r.get("title"),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok((features, total))
    }

    /// Get all blocked leaf features and their count.
    async fn portfolio_blocked(
        &self,
        project_id: ProjectId,
    ) -> Result<(Vec<PortfolioFeatureRef>, i64)> {
        let rows = sqlx::query(
            "SELECT f.id, f.title FROM features f
             WHERE f.project_id = $1
               AND f.state = 'blocked'
               AND f.parent_id IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM features c WHERE c.parent_id = f.id)
             ORDER BY f.priority ASC, f.created_at ASC",
        )
        .bind(project_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let count = rows.len() as i64;
        let features = rows
            .iter()
            .map(|r| {
                Ok(PortfolioFeatureRef {
                    id: parse_id(r.get("id"))?,
                    title: r.get("title"),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok((features, count))
    }

    /// Get recent completions (last 7 days, up to 5) and the overall last activity timestamp.
    async fn portfolio_recent(
        &self,
        project_id: ProjectId,
    ) -> Result<(Vec<PortfolioCompletion>, Option<chrono::DateTime<Utc>>)> {
        let since = (Utc::now() - chrono::Duration::days(7)).to_rfc3339();

        let recent_rows = sqlx::query(
            "SELECT f.id, f.title, fh.created_at
             FROM feature_history fh
             JOIN features f ON fh.feature_id = f.id
             WHERE f.project_id = $1
               AND fh.created_at > $2
             ORDER BY fh.created_at DESC
             LIMIT 5",
        )
        .bind(project_id.to_string())
        .bind(&since)
        .fetch_all(&self.pool)
        .await?;

        let recent_completions = recent_rows
            .iter()
            .map(|r| {
                Ok(PortfolioCompletion {
                    id: parse_id(r.get("id"))?,
                    title: r.get("title"),
                    completed_at: parse_datetime(r.get("created_at"))?,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        // Last activity: most recent history entry across all time.
        let last_row = sqlx::query(
            "SELECT MAX(fh.created_at) as last_at
             FROM feature_history fh
             JOIN features f ON fh.feature_id = f.id
             WHERE f.project_id = $1",
        )
        .bind(project_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        let last_activity_at = last_row
            .and_then(|r| r.get::<Option<String>, _>("last_at"))
            .map(parse_datetime)
            .transpose()?;

        Ok((recent_completions, last_activity_at))
    }
}
