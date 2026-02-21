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

/// GET /portfolio/events — SSE stream that fires a `change` event whenever
/// any feature in any project is modified. The client re-fetches GET /portfolio
/// on each event. No payload — the data endpoint does the work.
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
            Ok(_) => Some(Ok(Event::default()
                .event("change")
                .data("portfolio_changed"))),
            Err(_) => None, // Lagged receiver — client will catch up on next event
        };
        std::future::ready(val)
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
