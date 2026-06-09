//! Findings query / aggregation routes (B22, migration 0011 — Part B Phase B4).
//!
//! Two READ-only GETs that wrap the B20 repo read layer (no new SQL):
//!   * `GET /findings/query`                       — `repo::query_findings`.
//!   * `GET /work-items/{story_id}/finding-queue`  — `repo::get_story_finding_queue`.
//!
//! Each handler delegates to a single `repo::*` call, mirroring the matching
//! B21 MCP tool. Paths are relative to the `/api` mount point in `app.rs`.

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::routing::get;

use crate::app::AppState;
use lumina_core::domain::{Finding, QueryFindingsFilter};
use lumina_core::error::AppError;
use lumina_core::repo::{self, QueryFindingsResult};

/// Build the findings-query sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers. The
/// `GET /findings/query` static segment does not collide with the dynamic
/// `/findings/{id}` routes that `findings::router` declares.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/findings/query", get(query_findings_handler))
        .route(
            "/work-items/{story_id}/finding-queue",
            get(finding_queue_handler),
        )
}

/// `GET /findings/query` — query LIVE findings with the static NULL-guard filter,
/// optionally returning grouped axis counts. The filter arrives as querystring
/// params (`Query<QueryFindingsFilter>`); every field is optional, and an
/// absent field does not constrain its column. With `?count_by=severity` the
/// response is `{"counts":[{key,count}]}`; otherwise it is `{"findings":[...]}`
/// (the externally-tagged `QueryFindingsResult` shapes both). This is a READ —
/// no transaction, no event row.
async fn query_findings_handler(
    State(state): State<AppState>,
    Query(filter): Query<QueryFindingsFilter>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!(
        work_item_id = filter.work_item_id.as_deref().unwrap_or(""),
        count_by = ?filter.count_by,
        "http: GET /findings/query"
    );
    let result: QueryFindingsResult = repo::query_findings(state.pool.as_ref(), &filter).await?;
    Ok(Json(result))
}

/// `GET /work-items/{story_id}/finding-queue` — the story's live finding queue
/// (the story's own findings plus those on its descendant tasks), as returned by
/// `repo::get_story_finding_queue`. Returns `Json(Vec<Finding>)`. This is a READ.
async fn finding_queue_handler(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
) -> Result<Json<Vec<Finding>>, AppError> {
    tracing::debug!(story_id = %story_id, "http: GET /work-items/{{story_id}}/finding-queue");
    let queue = repo::get_story_finding_queue(state.pool.as_ref(), &story_id).await?;
    Ok(Json(queue))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt as _;

    use crate::app::{AppState, build_router};
    use lumina_core::db::connect_in_memory;
    use lumina_core::domain::Severity;
    use lumina_core::repo::{self, NewFinding};

    /// Drain a response body into bytes, then parse it as JSON.
    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("read body");
        serde_json::from_slice(&bytes).expect("parse json body")
    }

    /// Seed project→epic→focus→story→task and return the story id and task id.
    async fn seed_chain(pool: &sqlx::SqlitePool) -> (String, String) {
        let project = repo::create_work_item(pool, "project", None, "P", None)
            .await
            .expect("project");
        // migration-0010 valid chain: epic needs an outcome, focus needs a shape,
        // and a story requires the epic to carry >=1 close-criterion first.
        let epic = repo::create_work_item_full(
            pool, "epic", Some(&project.to_string()), "E", None,
            repo::CreateOpts { origin: None, outcome: Some("the epic outcome"), shape: None, lane: None },
        )
        .await
        .expect("epic");
        repo::add_acceptance_criterion(pool, &epic.to_string(), "epic close criterion")
            .await
            .expect("epic close criterion");
        let focus = repo::create_work_item_full(
            pool, "focus", Some(&epic.to_string()), "FO", None,
            repo::CreateOpts { origin: None, outcome: None, shape: Some("vertical-slice"), lane: None },
        )
        .await
        .expect("focus");
        let story = repo::create_work_item(pool, "story", Some(&focus.to_string()), "S", None)
            .await
            .expect("story");
        let task = repo::create_work_item(pool, "task", Some(&story.to_string()), "T", None)
            .await
            .expect("task");
        (story.to_string(), task.to_string())
    }

    /// Seed one finding under `work_item_id` and return its id.
    async fn seed_finding(
        pool: &sqlx::SqlitePool,
        work_item_id: &str,
        summary: &str,
        severity: Severity,
    ) -> String {
        let finding = NewFinding {
            summary: Some(summary),
            severity: Some(severity),
            category: Some("review"),
            ..NewFinding::default()
        };
        repo::create_finding(pool, work_item_id, &finding)
            .await
            .expect("seed finding")
            .to_string()
    }

    /// `GET /api/findings/query` with no filter returns the live findings, with
    /// `?count_by=severity` returns grouped counts that sum correctly, and with a
    /// `?severity=` filter narrows the set.
    #[tokio::test]
    async fn findings_query_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        seed_finding(&pool, &story_id, "missing null guard", Severity::Minor).await;
        seed_finding(&pool, &story_id, "unbounded retry loop", Severity::Major).await;
        seed_finding(&pool, &story_id, "second major", Severity::Major).await;
        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        // No filter → {"findings":[...]} with all three live findings.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/findings/query")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let findings = body["findings"].as_array().expect("findings array");
        assert_eq!(findings.len(), 3, "all three live findings returned");

        // ?count_by=severity → {"counts":[...]} that sums to the total.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/findings/query?count_by=severity")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let counts = body["counts"].as_array().expect("counts array");
        let total: i64 = counts.iter().map(|c| c["count"].as_i64().unwrap()).sum();
        assert_eq!(total, 3, "grouped counts sum to the finding total");
        let major = counts
            .iter()
            .find(|c| c["key"] == "major")
            .expect("major bucket");
        assert_eq!(major["count"], 2, "two major findings");

        // ?severity=major narrows the full-row set.
        let resp = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/findings/query?severity=major")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let findings = body["findings"].as_array().expect("findings array");
        assert_eq!(findings.len(), 2, "filter narrows to the two major findings");
    }

    /// `GET /api/work-items/{story_id}/finding-queue` returns the story's queue,
    /// including a finding seeded on a child task.
    #[tokio::test]
    async fn finding_queue_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, task_id) = seed_chain(&pool).await;
        seed_finding(&pool, &story_id, "story-level finding", Severity::Minor).await;
        seed_finding(&pool, &task_id, "task-level finding", Severity::Major).await;
        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/work-items/{story_id}/finding-queue"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Non-404 proves the queries::router() merge landed in http::router().
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let queue = body.as_array().expect("queue is a JSON array");
        assert_eq!(queue.len(), 2, "story + child-task findings both in the queue");
    }
}
