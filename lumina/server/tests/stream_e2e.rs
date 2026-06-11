//! Wave-1 T6 e2e: the `/api/stream` telemetry WebSocket end-to-end over a
//! REAL socket — subscribe to `sprint-quiescence:<id>`, receive the `init`
//! full snapshot, then drive domain writes through the repo layer and assert
//! a coalesced `data` snapshot arrives carrying the live claimable count.
//!
//! ## Why a bound listener (vs the crate's `oneshot` idiom)
//!
//! The in-process `tower::ServiceExt::oneshot` path carries no hyper upgrade
//! state, so it can never complete a WS handshake — `http/stream.rs`'s own
//! unit tests cover the pre-upgrade origin gate that way, and THIS test owns
//! the post-upgrade happy path. Pattern mirrors `companion_e2e.rs` (the
//! repo's first ephemeral-listener e2e): bind `127.0.0.1:0`, serve the
//! router exactly as `app::serve` does
//! (`into_make_service_with_connect_info::<SocketAddr>()`), drive the WS leg
//! with tokio-tungstenite (the first WS *client* in `server/tests/` — custom
//! `Origin` header via `into_client_request()` since the stream handler's
//! allowlist rejects an absent Origin).
//!
//! ## Why this proves the whole foundation
//!
//! The writes below are plain `repo::*` calls on the SAME pool the server
//! state wraps. Nothing here pokes the stream machinery directly: the only
//! path from `repo::create_work_item` / `add_tasks_to_sprint` /
//! `set_sprint_status` to the socket is commit → `NotifyingTx` post-commit
//! flush → global `NotifyBus` → the connection's receiver → `ConnState::note`
//! → 150 ms coalesce → `drain` recompute (`get_sprint_quiescence`) →
//! dedupe-on-equal → `data` frame. A frame arriving with `claimable == 1`
//! therefore exercises every Wave-1 layer at once.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use lumina_core::db::{connect_in_memory, AnyPool};
use lumina_core::domain::{NewSprint, SprintStatus};
use lumina_core::repo;
use lumina_server::app::{build_router, AppState};
use lumina_server::stream::FrameIn;

type WsClient = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Per-frame read deadline. Generous: the coalesce window is 150 ms, so 5 s
/// only ever bites when the pipeline is genuinely broken.
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);

/// Read the next TEXT frame as JSON, skipping protocol Ping/Pong.
async fn next_json_frame(ws: &mut WsClient) -> serde_json::Value {
    loop {
        let msg = tokio::time::timeout(FRAME_TIMEOUT, ws.next())
            .await
            .expect("timed out waiting for a stream frame")
            .expect("ws stream ended unexpectedly")
            .expect("ws frame error");
        match msg {
            Message::Text(text) => {
                return serde_json::from_str(text.as_str()).expect("stream frame is JSON")
            }
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected non-text ws frame: {other:?}"),
        }
    }
}

/// Seed the legal `project → epic(+close criterion) → focus → story` chain
/// and one task under the story; returns the task id. (Mirror of
/// `companion_e2e.rs::seed_story_with_tasks`, trimmed to one task.) The task
/// lands with the create-time defaults `status='open'` + `lane='implement'`,
/// no assignee / question-park / dep edges — i.e. it satisfies the claim
/// readiness predicate as soon as its sprint goes `active`.
async fn seed_story_with_one_task(pool: &sqlx::SqlitePool) -> String {
    let project = repo::create_work_item(pool, "project", None, "P", None)
        .await
        .expect("project");
    let epic = repo::create_work_item_full(
        pool,
        "epic",
        Some(&project.to_string()),
        "E",
        None,
        repo::CreateOpts { origin: None, outcome: Some("the epic outcome"), shape: None, lane: None },
    )
    .await
    .expect("epic");
    repo::add_acceptance_criterion(pool, &epic.to_string(), "epic close criterion")
        .await
        .expect("epic close criterion");
    let focus = repo::create_work_item_full(
        pool,
        "focus",
        Some(&epic.to_string()),
        "FO",
        None,
        repo::CreateOpts { origin: None, outcome: None, shape: Some("vertical-slice"), lane: None },
    )
    .await
    .expect("focus");
    let story = repo::create_work_item(pool, "story", Some(&focus.to_string()), "S", None)
        .await
        .expect("story");
    repo::create_work_item(pool, "task", Some(&story.to_string()), "T1", None)
        .await
        .expect("task")
        .to_string()
}

/// The Wave-1 happy path: subscribe → `init` (fresh sprint, all-zero counts)
/// → repo writes (task seeded + bound, sprint walked `draft→ready→active`) →
/// a `data` frame reflects `claimable == 1` within the timeout.
#[tokio::test]
async fn stream_e2e_subscribe_init_then_live_data_after_writes() {
    // --- 1. Shared pool + state; SEED THE SPRINT FIRST so subscribe's init
    //     resolves against a real row (a draft, taskless sprint snapshots as
    //     zeros with done=true).
    let pool = connect_in_memory().await.expect("in-memory pool");
    let sprint = repo::create_sprint(
        &pool,
        &NewSprint { title: None, worktree_id: None, predecessor_sprint_id: None },
    )
    .await
    .expect("sprint")
    .to_string();

    let state = AppState::new(Arc::new(AnyPool::from(pool.clone())));
    let app = build_router(state);

    // --- 2. Ephemeral listener, served exactly as `app::serve` does
    //     (with_connect_info: the merged router carries routes that extract
    //     ConnectInfo). Mirrors companion_e2e::Stack::spawn.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral listener");
    let port = listener.local_addr().expect("local addr").port();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("server task");
    });

    // --- 3. WS client handshake with an allowlisted Origin (the handler
    //     rejects an absent Origin pre-upgrade).
    let mut req = format!("ws://127.0.0.1:{port}/api/stream")
        .into_client_request()
        .expect("client request");
    req.headers_mut()
        .insert("Origin", "http://127.0.0.1".parse().expect("origin header value"));
    let (mut ws, _resp) = connect_async(req).await.expect("ws connect");

    // --- 4. Subscribe; the very first frame is the `init` full snapshot.
    //     The wire bytes come from the server's own `FrameIn` enum — the
    //     single source of truth for the inbound frame shape.
    let topic = format!("sprint-quiescence:{sprint}");
    let subscribe =
        serde_json::to_string(&FrameIn::Subscribe { topic: topic.clone() }).expect("serialise");
    ws.send(Message::Text(subscribe.into()))
        .await
        .expect("send subscribe");

    let init = next_json_frame(&mut ws).await;
    assert_eq!(init["type"], "init", "first frame is the init snapshot: {init}");
    assert_eq!(init["topic"], topic.as_str());
    assert_eq!(init["data"]["claimable"], 0, "fresh sprint has no claimable work: {init}");
    assert_eq!(init["data"]["in_progress"], 0);
    assert_eq!(init["data"]["done"], true, "a taskless sprint is trivially done: {init}");

    // --- 5. Drive the writes that make `claimable` become 1, against the
    //     SAME pool the server reads. Each commit publishes post-commit to
    //     the global notify bus the connection subscribed to at setup.
    //       * one ready task (status='open', lane='implement', no deps);
    //       * bound to the sprint via the junction;
    //       * sprint walked draft→ready→active (the LEGAL path — a direct
    //         draft→active flip is rejected as Validation).
    let task = seed_story_with_one_task(&pool).await;
    repo::add_tasks_to_sprint(&pool, &sprint, &[task.as_str()])
        .await
        .expect("bind task to sprint");
    repo::set_sprint_status(&pool, &sprint, SprintStatus::Ready)
        .await
        .expect("draft -> ready");
    repo::set_sprint_status(&pool, &sprint, SprintStatus::Active)
        .await
        .expect("ready -> active");

    // --- 6. Read `data` frames until the live claimable count lands.
    //     Intermediate frames are legitimate (e.g. the task-bind recompute
    //     flips done=false while the sprint is still draft ⇒ claimable=0);
    //     dedupe-on-equal means unchanged recomputes emit nothing. An
    //     `error` frame would mean a recompute failure — fail loudly.
    let mut claimable_one_seen = false;
    for _ in 0..16 {
        let frame = next_json_frame(&mut ws).await;
        match frame["type"].as_str() {
            Some("data") => {
                assert_eq!(frame["topic"], topic.as_str(), "single-topic connection: {frame}");
                if frame["data"]["claimable"] == 1 {
                    assert_eq!(frame["data"]["done"], false, "one ready task ⇒ not done: {frame}");
                    assert_eq!(frame["data"]["stalled"], false);
                    claimable_one_seen = true;
                    break;
                }
            }
            // Bus lag is theoretically possible (cap 1024 — not here), and
            // self-healing: every topic re-snapshots. Keep reading.
            Some("skipped") => continue,
            _ => panic!("unexpected frame while awaiting the data snapshot: {frame}"),
        }
    }
    assert!(
        claimable_one_seen,
        "a data frame with claimable == 1 must arrive after the writes"
    );

    server.abort();
}
