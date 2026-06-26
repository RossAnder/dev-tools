//! Liveness-aware stuck-reset for scheduled-unit leases (focus 1C.3).
//!
//! ## The hole this closes
//! [`repo::claim_next_scheduled_unit`](lumina_core::repo::claim_next_scheduled_unit)'s
//! step-1 lazy reclaim is BLIND: it clears the lease of ANY unit whose
//! `lease_expires_at` has passed. That is correct when the forked driver session
//! is genuinely gone, but DANGEROUS when the fork is alive but merely SLOW (a long
//! operation between heartbeats) — a blind reclaim hands the unit to a second
//! claimer and TWO autonomous `claude` sessions then race the same driver job.
//!
//! This module is the SMARTER layer that runs in FRONT of that blind reclaim. For
//! every leased-and-expired `scheduled_units` row it CONSULTS the correlated PTY
//! session's liveness before clearing the lease:
//!   * the session is in a terminal/DEAD status (or no session row landed at all,
//!     past a grace window) ⇒ genuinely stuck ⇒ RECLAIM (clear the lease so the
//!     unit re-enters the ready set);
//!   * the session is still alive (spawning / active / idle / awaiting) ⇒
//!     slow-but-live ⇒ LEAVE the lease untouched (never clobber a live fork into a
//!     second racing session).
//!
//! Only the SERVER can see PTY liveness, so this reclaim lives here, not in the
//! `lumina-core` repo layer.
//!
//! ## The unit ↔ session correlation
//! The dispatch (`crate::mcp::scheduler::dispatch_scheduled_unit_flow`) leases a
//! unit with a fresh per-dispatch `agent_id` (`manual-dispatch-<uuid>`) and then
//! stamps that SAME id onto the spawned session's `pty_sessions.agent_id` column.
//! So the correlation key is `scheduled_units.assignee == pty_sessions.agent_id`.
//! (The lease GATES the spawn — we lease BEFORE the session id exists — so the
//! link is closed in this direction, by stamping the session post-spawn, rather
//! than by leasing with the session id.) A missing session row therefore means the
//! dispatch crashed before it could record one.
//!
//! ## The heartbeat contract (this reclaim is the SAFETY NET, not the primary path)
//! A LIVE forked driver is expected to call
//! [`renew_scheduled_lease`](lumina_core::repo::renew_scheduled_lease) as a
//! HEARTBEAT (the autonomous build-out skill the fork runs owns that cadence — a
//! skill concern), so a live fork's lease NEVER expires in the first place. This
//! liveness reclaim exists purely for forks that DIED without releasing their
//! lease (crashed / killed / OOM): it recovers their stranded unit without ever
//! clobbering a fork that is alive but momentarily behind on its heartbeat.
//!
//! ## Scope note: open-question parking is NOT a reclaim concern
//! "Parked on an open question" is a `work_items` state (`status='blocked'` +
//! `blocked_by_question_id`); a `scheduled_units` row carries no such field.
//! Reclaiming merely returns the unit to the ready set — whether a parked work
//! item should be RE-dispatched is the redispatch sibling's decision, not this
//! module's. So this reclaim never consults parking.
//!
//! Runtime `sqlx::query*` only (no bang macros); this is the CONTROL plane, so it
//! never shells git.

use lumina_core::args;
use lumina_core::db::{scalar_opt, AnyPool, DbClient};
use lumina_core::repo;

/// Grace window (seconds) for the AMBIGUOUS "expired lease but NO correlated
/// session row" case. A unit whose lease only JUST expired (within this window)
/// and has no session row yet is given the benefit of the doubt — its session row
/// may still be landing (the dispatch records the `pty_sessions` row a beat AFTER
/// it commits the lease), or the fork may be about to heartbeat. Only once the
/// lease has been expired LONGER than this — with still no session to be found —
/// do we treat it as a crashed fork that never recorded a process, and reclaim.
/// A DEAD session is reclaimed immediately regardless of this window (we have
/// positive evidence of death); the grace only softens the no-evidence case.
const RECLAIM_GRACE_SECS: i64 = 30;

/// PTY-session statuses that mean the forked driver is GONE. Mirrors the
/// `spawning|active|idle|awaiting|completed|failed|cancelled` lifecycle vocab
/// (see `lumina_core::domain::PtySession`): the three terminal states are dead;
/// everything else (including an unrecognised value) is treated as ALIVE, so an
/// unknown status never triggers a reclaim — conservative by construction.
fn is_session_dead(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}

/// Run one liveness-aware reclaim pass over the `scheduled_units` queue and return
/// the number of leases cleared. Every error is LOGGED and SWALLOWED (never
/// propagated) — like the scheduler scan, one bad row must not kill the loop, and
/// a lookup failure leaves the lease untouched (fail safe: never reclaim on
/// uncertainty). Read-mostly: at most one owner-guarded
/// [`repo::release_scheduled_unit`] write per genuinely-dead unit.
pub async fn reclaim_dead_units(db: &AnyPool) -> usize {
    // The grace threshold is a bound PARAM so the SQL string stays `'static`
    // (the `DbClient` seam requires it). `datetime('now', '-N seconds')` shares
    // the `CURRENT_TIMESTAMP` storage format the lease was stamped in, so the
    // lexical `<` comparison is correct.
    let grace_modifier = format!("-{RECLAIM_GRACE_SECS} seconds");

    // Candidate set ≡ leased AND expired (the same predicate the blind reclaim
    // uses), each row also carrying a `beyond_grace` flag for the no-session case.
    let candidates: Vec<(String, String, i64)> = match db
        .query_all::<(String, String, i64)>(
            r#"
            SELECT
                id,
                assignee,
                CASE WHEN lease_expires_at < datetime('now', $1) THEN 1 ELSE 0 END AS beyond_grace
            FROM scheduled_units
            WHERE assignee IS NOT NULL
              AND lease_expires_at IS NOT NULL
              AND lease_expires_at < datetime('now')
            "#,
            args![grace_modifier],
        )
        .await
    {
        Ok(c) => c,
        Err(err) => {
            tracing::warn!(error = %err, "scheduler reclaim: candidate query failed; skipping pass");
            return 0;
        }
    };

    let mut reclaimed = 0usize;
    for (unit_id, assignee, beyond_grace) in candidates {
        // Correlate the lease owner to its spawned PTY session (the dispatch
        // stamped `pty_sessions.agent_id = <lease owner>`). `None` ⇒ no session
        // row was ever recorded for this lease.
        let session_status = match scalar_opt::<String>(
            db,
            "SELECT status FROM pty_sessions WHERE agent_id = $1 ORDER BY started_at DESC LIMIT 1",
            args![assignee.clone()],
        )
        .await
        {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!(
                    unit_id = %unit_id,
                    error = %err,
                    "scheduler reclaim: session liveness lookup failed; leaving lease (fail safe)"
                );
                continue;
            }
        };

        let (should_reclaim, reason) = match session_status.as_deref() {
            // Positive evidence of death — reclaim now, grace irrelevant.
            Some(s) if is_session_dead(s) => (true, "correlated session is dead"),
            // Alive (slow-but-live, or just unrecognised) — NEVER clobber it.
            Some(_) => (false, "correlated session is alive"),
            // No session row + long past expiry ⇒ a crashed fork that never
            // recorded a process ⇒ reclaim.
            None if beyond_grace != 0 => (true, "no correlated session (beyond grace)"),
            // No session row but only just expired ⇒ the row may still be landing
            // ⇒ hold off.
            None => (false, "no correlated session (within grace)"),
        };

        if !should_reclaim {
            tracing::debug!(
                unit_id = %unit_id,
                reason,
                "scheduler reclaim: leaving expired lease in place"
            );
            continue;
        }

        // Owner-guarded clear (release with the dead owner's id). Ok(false) means
        // the lease changed hands between our read and this write — a benign race,
        // nothing to do.
        match repo::release_scheduled_unit(db, &unit_id, &assignee).await {
            Ok(true) => {
                reclaimed += 1;
                tracing::info!(
                    unit_id = %unit_id,
                    reason,
                    "scheduler reclaim: cleared a stuck lease; unit re-enters the ready set"
                );
            }
            Ok(false) => {
                tracing::debug!(
                    unit_id = %unit_id,
                    "scheduler reclaim: lease already changed hands; nothing to clear"
                );
            }
            Err(err) => {
                tracing::warn!(unit_id = %unit_id, error = %err, "scheduler reclaim: release failed");
            }
        }
    }

    reclaimed
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::db::{connect_in_memory, AnyPool};
    use lumina_core::repo::create_work_item;
    use uuid::Uuid;

    /// Seed a LEASED `scheduled_units` row (kind `build_story`, `status='pending'`)
    /// whose lease + last-update timestamps are `lease_modifier` from `now`
    /// (e.g. `"-3600 seconds"` = expired an hour ago; `"-5 seconds"` = just
    /// expired). Raw runtime sqlx INSERT — seeding is allowed (NOT a bang macro,
    /// so the macro-eradication gate stays at 0); there is no create-leased-unit
    /// repo mutator. Returns the unit id.
    async fn seed_leased_unit(
        db: &AnyPool,
        work_item_id: &str,
        assignee: &str,
        lease_modifier: &str,
    ) -> String {
        let unit_id = Uuid::now_v7().to_string();
        db.execute(
            r#"
            INSERT INTO scheduled_units
                (id, kind, work_item_id, status, assignee, lease_expires_at, updated_at)
            VALUES ($1, 'build_story', $2, 'pending', $3, datetime('now', $4), datetime('now', $4))
            "#,
            args![
                unit_id.clone(),
                work_item_id.to_owned(),
                assignee.to_owned(),
                lease_modifier.to_owned()
            ],
        )
        .await
        .expect("seed leased scheduled_unit");
        unit_id
    }

    /// Seed a `pty_sessions` row correlated to `agent_id` (the lease owner) in the
    /// given lifecycle `status`, via the existing `repo::pty` helpers (no raw
    /// column drift). This mirrors what the dispatch does: spawn a session, then
    /// stamp its `agent_id` with the lease owner.
    async fn seed_session(db: &AnyPool, id: &str, agent_id: &str, status: &str) {
        repo::pty::create_pty_session(db, id, None, None, "/tmp", "{}", Some("autonomous"))
            .await
            .expect("create pty session");
        repo::pty::update_pty_session_correlation(db, id, None, Some(agent_id))
            .await
            .expect("stamp session agent_id");
        repo::pty::update_pty_session_status(db, id, status, None)
            .await
            .expect("set session status");
    }

    /// Read a unit's current lease owner (`None` once the lease is cleared). Uses
    /// an `Option<String>` tuple decode so a NULL `assignee` maps to `None` —
    /// `scalar_opt::<String>` would decode a NULL into `Some("")` and mask a
    /// cleared lease.
    async fn lease_owner(db: &AnyPool, unit_id: &str) -> Option<String> {
        let (owner, _): (Option<String>, i64) = db
            .query_one(
                "SELECT assignee, 1 FROM scheduled_units WHERE id = $1",
                args![unit_id.to_owned()],
            )
            .await
            .expect("read lease owner");
        owner
    }

    async fn project(db: &AnyPool) -> String {
        create_work_item(db, "project", None, "P", None)
            .await
            .expect("project")
            .to_string()
    }

    /// **The recover-a-crashed-fork criterion.** A leased unit whose correlated
    /// PTY session is in a DEAD status (`failed`) with an expired lease is
    /// reclaimed — the lease is cleared and the unit is ready again.
    #[tokio::test]
    async fn dead_session_expired_lease_is_reclaimed() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let project = project(&db).await;
        let agent = "manual-dispatch-dead";
        let unit = seed_leased_unit(&db, &project, agent, "-3600 seconds").await;
        seed_session(&db, "sess-dead", agent, "failed").await;

        let n = reclaim_dead_units(&db).await;

        assert_eq!(n, 1, "the dead-fork unit is reclaimed");
        assert!(
            lease_owner(&db, &unit).await.is_none(),
            "the dead-fork lease is cleared → the unit re-enters the ready set"
        );
    }

    /// **The do-not-clobber-a-live-fork criterion.** A leased unit whose
    /// correlated PTY session is ALIVE (`idle`, i.e. running between heartbeats)
    /// is NOT reclaimed even though its lease has expired — a slow-but-live fork
    /// keeps its lease and is never handed to a second racing session.
    #[tokio::test]
    async fn live_session_expired_lease_is_not_reclaimed() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let project = project(&db).await;
        let agent = "manual-dispatch-live";
        let unit = seed_leased_unit(&db, &project, agent, "-3600 seconds").await;
        // `idle` is an ALIVE lifecycle status (the supervisor's between-turns
        // resting state), distinct from the terminal `completed|failed|cancelled`.
        seed_session(&db, "sess-live", agent, "idle").await;

        let n = reclaim_dead_units(&db).await;

        assert_eq!(n, 0, "a slow-but-live fork is never reclaimed");
        assert_eq!(
            lease_owner(&db, &unit).await.as_deref(),
            Some(agent),
            "the live fork retains its lease (no second racing session)"
        );
    }

    /// The GRACE window: a unit whose lease only just expired and has NO session
    /// row yet (the dispatch's `pty_sessions` insert hasn't landed) is treated
    /// conservatively — NOT reclaimed.
    #[tokio::test]
    async fn no_session_within_grace_is_not_reclaimed() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let project = project(&db).await;
        let agent = "manual-dispatch-just-leased";
        // Expired 5s ago — well within the 30s grace window.
        let unit = seed_leased_unit(&db, &project, agent, "-5 seconds").await;
        // No session row seeded.

        let n = reclaim_dead_units(&db).await;

        assert_eq!(n, 0, "a just-expired, session-less unit is given grace");
        assert_eq!(
            lease_owner(&db, &unit).await.as_deref(),
            Some(agent),
            "the lease is held through the grace window"
        );
    }

    /// The other side of the grace gate: a unit with NO session row whose lease
    /// expired LONG ago (a crashed fork that never recorded a process) is
    /// reclaimed once it is beyond the grace window.
    #[tokio::test]
    async fn no_session_beyond_grace_is_reclaimed() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let project = project(&db).await;
        let agent = "manual-dispatch-ghost";
        let unit = seed_leased_unit(&db, &project, agent, "-3600 seconds").await;
        // No session row was ever recorded.

        let n = reclaim_dead_units(&db).await;

        assert_eq!(n, 1, "a long-expired, session-less lease is reclaimed");
        assert!(
            lease_owner(&db, &unit).await.is_none(),
            "the stranded lease is cleared"
        );
    }

    /// `is_session_dead` classifies the lifecycle vocab: the three terminal
    /// statuses are dead; the live statuses (and any unrecognised value) are alive.
    #[test]
    fn dead_status_classification() {
        for dead in ["completed", "failed", "cancelled"] {
            assert!(is_session_dead(dead), "{dead} is a terminal/dead status");
        }
        for alive in ["spawning", "active", "idle", "awaiting", "something-new"] {
            assert!(
                !is_session_dead(alive),
                "{alive} is treated as alive (conservative — never reclaim on it)"
            );
        }
    }
}
