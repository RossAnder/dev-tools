-- lumina migration 0002: kind-specific attributes, soft-delete, activity log.
--
-- Additive / non-destructive over 0001. Three changes:
--   1. Two new nullable columns on work_items: `attributes` (kind-specific
--      JSON payload) and `deleted_at` (soft-delete tombstone).
--   2. A new `work_item_activity` append-only per-item log (ordered by `seq`).
--   3. A BEFORE INSERT / BEFORE UPDATE trigger pair on work_items rejecting a
--      non-NULL but malformed `attributes` JSON value.
--
-- Design constraints (mirroring 0001):
--   * SQL kept ANSI-ish for a later Postgres port. The SQLite-specific guards
--     here are the json_valid() CHECK on activity.payload and the attributes
--     validity trigger pair below — on Postgres `attributes`/`payload` become
--     JSONB (which validates on write) and BOTH the CHECK and the trigger pair
--     drop away.
--   * SQLite ALTER TABLE ADD COLUMN cannot carry a non-constant DEFAULT, a
--     PK/UNIQUE, or a retroactive CHECK, so the new columns are plain nullable
--     TEXT and the attributes validity guard lives in the trigger pair instead
--     of a column CHECK.
--   * ADD COLUMN leaves existing rows at `attributes = NULL` and
--     `deleted_at = NULL` (= "no kind-specific fields" / "live row") — intended
--     and non-destructive.
--   * ids are app-generated UUIDv7 TEXT (see 0001); the schema only declares
--     TEXT.

PRAGMA foreign_keys = ON;

-- ---------------------------------------------------------------------------
-- work_items: additive columns.
-- `attributes` — nullable JSON-as-TEXT bag of kind-specific fields; NULL means
--   "no kind-specific fields". Validity is enforced by the trigger pair below
--   (a column CHECK is impossible via ADD COLUMN).
-- `deleted_at`  — nullable soft-delete tombstone (ISO-8601 TEXT); NULL = live.
-- ---------------------------------------------------------------------------
ALTER TABLE work_items ADD COLUMN attributes TEXT;
ALTER TABLE work_items ADD COLUMN deleted_at TEXT;

-- ---------------------------------------------------------------------------
-- work_item_activity: append-only per-item activity log.
-- `seq` is a per-work_item monotonic ordinal (allocation is the repo layer's
-- job); UNIQUE(work_item_id, seq) makes gaps/dupes structurally impossible.
-- ON DELETE CASCADE ties an item's activity rows to the item's lifetime.
-- `payload` holds optional JSON-as-TEXT, validated by the CHECK below.
-- ---------------------------------------------------------------------------
CREATE TABLE work_item_activity (
    id           TEXT PRIMARY KEY,
    work_item_id TEXT NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
    seq          INTEGER NOT NULL,
    entry_kind   TEXT NOT NULL,
    author       TEXT,
    summary      TEXT NOT NULL,
    payload      TEXT,
    created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (payload IS NULL OR json_valid(payload)),
    UNIQUE (work_item_id, seq)
);

CREATE INDEX idx_activity_work_item ON work_item_activity(work_item_id, seq);

-- ---------------------------------------------------------------------------
-- work_items.attributes validity guard (mirrors 0001's hierarchy trigger pair).
--
-- A column CHECK cannot be added retroactively via ALTER TABLE ADD COLUMN, so a
-- BEFORE INSERT / BEFORE UPDATE trigger pair is the authoritative guard: a
-- non-NULL `attributes` value that is not valid JSON aborts the write. NULL is
-- always permitted (= "no kind-specific fields"). The two bodies are
-- byte-identical apart from the trigger event.
-- ---------------------------------------------------------------------------
CREATE TRIGGER trg_work_items_attributes_insert
BEFORE INSERT ON work_items
FOR EACH ROW
WHEN NEW.attributes IS NOT NULL AND NOT json_valid(NEW.attributes)
BEGIN
    SELECT RAISE(ABORT, 'invalid work_item attributes: not valid JSON');
END;

CREATE TRIGGER trg_work_items_attributes_update
BEFORE UPDATE ON work_items
FOR EACH ROW
WHEN NEW.attributes IS NOT NULL AND NOT json_valid(NEW.attributes)
BEGIN
    SELECT RAISE(ABORT, 'invalid work_item attributes: not valid JSON');
END;
