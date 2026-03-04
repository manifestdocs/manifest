//! Spec template management endpoints.
//!
//! Each project can have one [`SpecTemplate`](manifest_core::models::SpecTemplate)
//! that provides default content for new feature specifications. Supports
//! get and upsert operations.

use axum::{
    extract::{Path, State},
    Json,
};
use uuid::Uuid;

use crate::db::Database;
use crate::models::{CreateTemplateInput, SpecTemplate, DEFAULT_TEMPLATE_CONTENT};

use super::{internal_error, ApiError};
use crate::api::validation::ValidatedJson;

// ============================================================
// Spec Template (one per project)
// ============================================================

/// Get the project's spec template.
pub async fn get_project_template(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Option<SpecTemplate>>, ApiError> {
    db.get_default_template(project_id.into())
        .await
        .map(Json)
        .map_err(internal_error)
}

/// Update the project's spec template (upsert — creates if none exists).
pub async fn update_project_template(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    ValidatedJson(input): ValidatedJson<crate::models::UpdateTemplateInput>,
) -> Result<Json<SpecTemplate>, ApiError> {
    let project_id = project_id.into();
    let template = db
        .get_default_template(project_id)
        .await
        .map_err(internal_error)?;

    match template {
        Some(t) => db
            .update_template(t.id, input)
            .await
            .map_err(internal_error)?
            .map(Json)
            .ok_or(ApiError::not_found("Template")),
        None => {
            // No template exists yet — create one
            let create_input = CreateTemplateInput {
                name: input.name.unwrap_or_else(|| "Default".to_string()),
                description: input.description,
                content: input
                    .content
                    .unwrap_or_else(|| DEFAULT_TEMPLATE_CONTENT.to_string()),
                is_default: true,
            };
            db.create_template(project_id, create_input)
                .await
                .map(Json)
                .map_err(internal_error)
        }
    }
}
