use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{FeatureId, HistoryId, ProofId};

/// The state of an individual test result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestState {
    Passed,
    Failed,
    Errored,
    Skipped,
}

impl TestState {
    pub fn as_str(&self) -> &'static str {
        match self {
            TestState::Passed => "passed",
            TestState::Failed => "failed",
            TestState::Errored => "errored",
            TestState::Skipped => "skipped",
        }
    }
}

impl FromStr for TestState {
    type Err = super::ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "passed" => Ok(Self::Passed),
            "failed" => Ok(Self::Failed),
            "errored" => Ok(Self::Errored),
            "skipped" => Ok(Self::Skipped),
            _ => Err(super::ParseEnumError(s.to_string())),
        }
    }
}

impl fmt::Display for TestState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for TestState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TestState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        TestState::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// A single test result within a proof.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    /// Name of the test (e.g., "creates a feature").
    pub name: String,
    /// Test suite or module (e.g., "db_spec").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suite: Option<String>,
    /// Result state: passed, failed, errored, or skipped.
    pub state: TestState,
    /// Source file path (relative to project root).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Line number in the source file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Duration in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Failure or error message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// A file path linked as evidence for a proof.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    /// File path (relative to project root).
    pub path: String,
    /// Optional note about why this file is evidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Maximum length for raw test output stored in a proof.
pub const PROOF_OUTPUT_MAX_CHARS: usize = 10_000;

/// A proof record — test evidence for a feature.
///
/// Proofs are standalone entities that belong to a feature but have their own
/// lifecycle. They can be created independently (TDD red/green cycle) or
/// linked to a feature completion event via `history_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proof {
    pub id: ProofId,
    pub feature_id: FeatureId,
    /// Optional link to the feature_history entry this proof accompanies.
    pub history_id: Option<HistoryId>,
    /// The command that was run (e.g., "cargo test auth_spec").
    pub command: String,
    /// Process exit code (0 = all tests passed).
    pub exit_code: i32,
    /// Raw stdout/stderr output, capped at 10K characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Structured test results for consistent rendering.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tests: Option<Vec<TestResult>>,
    /// Evidence file paths linked to this proof.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<Evidence>,
    /// Git commit SHA at the time of proving.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    /// Agent that produced the proof (e.g., "claude", "human").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Input for creating a new proof record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProofInput {
    /// The feature this proof belongs to.
    pub feature_id: FeatureId,
    /// Optional link to a feature_history entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_id: Option<HistoryId>,
    /// The command that was run.
    pub command: String,
    /// Process exit code.
    pub exit_code: i32,
    /// Raw stdout/stderr output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Structured test results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tests: Option<Vec<TestResult>>,
    /// Evidence file paths.
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    /// Git commit SHA.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    /// Agent type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
}
