use anyhow::Result;
use chrono::Utc;

use super::helpers::*;
use super::Database;
use crate::models::*;

impl Database {
    /// Create a new proof record for a feature.
    ///
    /// Output is truncated to [`PROOF_OUTPUT_MAX_CHARS`] if it exceeds the limit.
    pub async fn create_proof(&self, input: CreateProofInput) -> Result<Proof> {
        let id = ProofId::new();
        let now = Utc::now();

        // Truncate output if needed
        let output = input.output.map(|o| {
            if o.len() > PROOF_OUTPUT_MAX_CHARS {
                let truncated = &o[..PROOF_OUTPUT_MAX_CHARS];
                format!("{truncated}\n\n... (output truncated at {PROOF_OUTPUT_MAX_CHARS} characters)")
            } else {
                o
            }
        });

        let tests_json = input
            .tests
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;

        let evidence_json = if input.evidence.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&input.evidence)?)
        };

        sqlx::query(
            "INSERT INTO proofs (id, feature_id, history_id, command, exit_code, output, tests, evidence, commit_sha, agent_type, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(id.to_string())
        .bind(input.feature_id.to_string())
        .bind(input.history_id.map(|h| h.to_string()))
        .bind(&input.command)
        .bind(input.exit_code)
        .bind(&output)
        .bind(&tests_json)
        .bind(&evidence_json)
        .bind(&input.commit_sha)
        .bind(&input.agent_type)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(Proof {
            id,
            feature_id: input.feature_id,
            history_id: input.history_id,
            command: input.command,
            exit_code: input.exit_code,
            output,
            tests: input.tests,
            evidence: input.evidence,
            commit_sha: input.commit_sha,
            agent_type: input.agent_type,
            created_at: now,
        })
    }

    /// Get a proof by its ID.
    pub async fn get_proof(&self, id: ProofId) -> Result<Option<Proof>> {
        let row = sqlx::query(
            "SELECT id, feature_id, history_id, command, exit_code, output, tests, evidence, commit_sha, agent_type, created_at
             FROM proofs WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(row_to_proof).transpose()
    }

    /// Get all proofs for a feature, ordered by most recent first.
    pub async fn get_proofs_for_feature(&self, feature_id: FeatureId) -> Result<Vec<Proof>> {
        let rows = sqlx::query(
            "SELECT id, feature_id, history_id, command, exit_code, output, tests, evidence, commit_sha, agent_type, created_at
             FROM proofs WHERE feature_id = $1 ORDER BY created_at DESC",
        )
        .bind(feature_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_proof).collect()
    }

    /// Get the latest proof for a feature (most recent by created_at).
    pub async fn get_latest_proof_for_feature(
        &self,
        feature_id: FeatureId,
    ) -> Result<Option<Proof>> {
        let row = sqlx::query(
            "SELECT id, feature_id, history_id, command, exit_code, output, tests, evidence, commit_sha, agent_type, created_at
             FROM proofs WHERE feature_id = $1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(feature_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(row_to_proof).transpose()
    }

    /// Delete all proofs for a feature.
    pub async fn delete_proofs_for_feature(&self, feature_id: FeatureId) -> Result<u64> {
        let result = sqlx::query("DELETE FROM proofs WHERE feature_id = $1")
            .bind(feature_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}
