use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{SessionId, TaskId};

/// A unit of work within a session, assigned to an AI agent.
///
/// Tasks are **ephemeral**—they exist only during an active session. When the
/// session completes, tasks are summarized into a history entry and deleted.
///
/// Tasks can optionally have a `parent_id` for sub-task relationships, but
/// AI agents are expected to manage their own internal work breakdown without
/// Manifest tracking granular sub-items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub session_id: SessionId,
    /// Optional parent task for sub-task relationships.
    pub parent_id: Option<TaskId>,
    pub title: String,
    /// Description of what work is included in this task.
    pub scope: String,
    pub status: TaskStatus,
    pub agent_type: AgentType,
    /// Path to a git worktree if working in isolation.
    pub worktree_path: Option<String>,
    /// Git branch name for this task's work.
    pub branch: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// The execution status of a task.
///
/// - `Pending`: Not yet started
/// - `Running`: Agent is actively working
/// - `Completed`: Work finished successfully
/// - `Failed`: Task could not be completed
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl FromStr for TaskStatus {
    type Err = super::ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => Err(super::ParseEnumError(s.to_string())),
        }
    }
}

/// The type of AI agent assigned to a task.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentType {
    /// Anthropic Claude
    Claude,
    /// Google Gemini
    Gemini,
    /// OpenAI Codex
    Codex,
    /// GitHub Copilot
    Copilot,
}

impl AgentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Gemini => "gemini",
            Self::Codex => "codex",
            Self::Copilot => "copilot",
        }
    }
}

impl FromStr for AgentType {
    type Err = super::ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude" => Ok(Self::Claude),
            "gemini" => Ok(Self::Gemini),
            "codex" => Ok(Self::Codex),
            "copilot" => Ok(Self::Copilot),
            _ => Err(super::ParseEnumError(s.to_string())),
        }
    }
}

/// Input for creating a new task within a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskInput {
    /// Optional parent task for sub-task relationships.
    pub parent_id: Option<TaskId>,
    pub title: String,
    /// Description of what work is included in this task.
    pub scope: String,
    /// The AI agent type to assign this task to.
    pub agent_type: AgentType,
}

/// Input for updating a task. Used by agents to report progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTaskInput {
    pub status: Option<TaskStatus>,
    /// Path to a git worktree if working in isolation.
    pub worktree_path: Option<String>,
    /// Git branch name for this task's work.
    pub branch: Option<String>,
}
