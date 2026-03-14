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
                format!(
                    "{truncated}\n\n... (output truncated at {PROOF_OUTPUT_MAX_CHARS} characters)"
                )
            } else {
                o
            }
        });

        let tests_json = input
            .test_suites
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;

        let evidence_json = if input.evidence.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&input.evidence)?)
        };

        self.conn
            .execute(
                "INSERT INTO proofs (id, feature_id, history_id, command, exit_code, output, tests, evidence, commit_sha, agent_type, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                libsql::params![
                    id.to_string(),
                    input.feature_id.to_string(),
                    match &input.history_id {
                        Some(h) => libsql::Value::Text(h.to_string()),
                        None => libsql::Value::Null,
                    },
                    input.command.clone(),
                    input.exit_code,
                    match &output {
                        Some(o) => libsql::Value::Text(o.clone()),
                        None => libsql::Value::Null,
                    },
                    match &tests_json {
                        Some(t) => libsql::Value::Text(t.clone()),
                        None => libsql::Value::Null,
                    },
                    match &evidence_json {
                        Some(e) => libsql::Value::Text(e.clone()),
                        None => libsql::Value::Null,
                    },
                    match &input.commit_sha {
                        Some(s) => libsql::Value::Text(s.clone()),
                        None => libsql::Value::Null,
                    },
                    match &input.agent_type {
                        Some(a) => libsql::Value::Text(a.clone()),
                        None => libsql::Value::Null,
                    },
                    now.to_rfc3339()
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(Proof {
            id,
            feature_id: input.feature_id,
            history_id: input.history_id,
            command: input.command,
            exit_code: input.exit_code,
            output,
            test_suites: input.test_suites,
            evidence: input.evidence,
            commit_sha: input.commit_sha,
            agent_type: input.agent_type,
            created_at: now,
        })
    }

    /// Get a proof by its ID.
    pub async fn get_proof(&self, id: ProofId) -> Result<Option<Proof>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, feature_id, history_id, command, exit_code, output, tests, evidence, commit_sha, agent_type, created_at
                 FROM proofs WHERE id = ?1",
                libsql::params![id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            Some(row) => Ok(Some(row_to_proof(&row)?)),
            None => Ok(None),
        }
    }

    /// Get all proofs for a feature, ordered by most recent first.
    pub async fn get_proofs_for_feature(&self, feature_id: FeatureId) -> Result<Vec<Proof>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, feature_id, history_id, command, exit_code, output, tests, evidence, commit_sha, agent_type, created_at
                 FROM proofs WHERE feature_id = ?1 ORDER BY created_at DESC",
                libsql::params![feature_id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            results.push(row_to_proof(&row)?);
        }
        Ok(results)
    }

    /// Get the latest proof for a feature (most recent by created_at).
    pub async fn get_latest_proof_for_feature(
        &self,
        feature_id: FeatureId,
    ) -> Result<Option<Proof>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, feature_id, history_id, command, exit_code, output, tests, evidence, commit_sha, agent_type, created_at
                 FROM proofs WHERE feature_id = ?1 ORDER BY created_at DESC LIMIT 1",
                libsql::params![feature_id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            Some(row) => Ok(Some(row_to_proof(&row)?)),
            None => Ok(None),
        }
    }
}
