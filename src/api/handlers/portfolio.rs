use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures_util::{stream::Stream, StreamExt};
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;

use crate::db::Database;
use crate::models::Portfolio;

use super::internal_error;

/// GET /portfolio — aggregated health snapshot for all projects.
pub async fn get_portfolio(State(db): State<Database>) -> Result<Json<Portfolio>, super::ApiError> {
    db.get_portfolio().await.map(Json).map_err(internal_error)
}

/// GET /portfolio/events — SSE stream that fires events whenever any feature
/// in any project is modified. Emits typed events:
/// - `change` — generic modification (create, update, delete)
/// - `feature_completed` — a feature was completed, with JSON payload
///   containing feature_title, project_name, and agent_type
///
/// This reuses the existing global broadcast channel in `Database::subscribe()`.
/// The per-project SSE handler filters by project_id; this handler emits for
/// every event regardless of project.
pub async fn subscribe_portfolio(
    State(db): State<Database>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = db.subscribe();

    let stream = BroadcastStream::new(rx).filter_map(|result| {
        let val = match result {
            Ok(event) => {
                use crate::db::FeatureEvent;
                match event {
                    FeatureEvent::Completed {
                        feature_title,
                        project_name,
                        agent_type,
                        ..
                    } => {
                        let payload = serde_json::json!({
                            "feature_title": feature_title,
                            "project_name": project_name,
                            "agent_type": agent_type,
                        });
                        Some(Ok(Event::default()
                            .event("feature_completed")
                            .data(payload.to_string())))
                    }
                    _ => Some(Ok(Event::default()
                        .event("change")
                        .data("portfolio_changed"))),
                }
            }
            Err(_) => None, // Lagged receiver — client will catch up on next event
        };
        std::future::ready(val)
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
