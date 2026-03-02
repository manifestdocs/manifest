use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use validator::Validate;

use super::{ProjectId, TemplateId};

/// A specification template for writing feature specs.
///
/// Each project has one template, used by MCP tools when guiding agents to
/// write specs for features that have no details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecTemplate {
    pub id: TemplateId,
    pub project_id: ProjectId,
    pub name: String,
    pub description: Option<String>,
    pub content: String,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input for creating a new spec template.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CreateTemplateInput {
    #[validate(length(min = 1, max = 200))]
    pub name: String,
    #[validate(length(max = 1_000))]
    pub description: Option<String>,
    #[validate(length(min = 1, max = 10_000))]
    pub content: String,
    #[serde(default)]
    pub is_default: bool,
}

/// Input for updating an existing spec template. All fields are optional.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct UpdateTemplateInput {
    #[validate(length(min = 1, max = 200))]
    pub name: Option<String>,
    #[validate(length(max = 1_000))]
    pub description: Option<String>,
    #[validate(length(min = 1, max = 10_000))]
    pub content: Option<String>,
    pub is_default: Option<bool>,
}

/// The default template content shipped with every new project.
pub const DEFAULT_TEMPLATE_CONTENT: &str = "\
## Goal

<!-- One or two sentences: what capability this adds and why it matters.
     Focus on the outcome, not the implementation. -->

## Rules

<!-- Business logic, constraints, and edge cases the agent won't discover from
     code alone. Don't repeat what's in parent features or project instructions.
     Examples: validation rules, rate limits, ordering guarantees, error behavior. -->

## Acceptance Criteria

<!-- Each criterion should be a specific, verifiable outcome an agent can assert
     in a test. Use concrete values. The more precise, the better the tests. -->

- [ ] [Specific, verifiable outcome with concrete values]
- [ ] [Edge case or error handling expectation]
- [ ] [Additional criteria as needed]";
